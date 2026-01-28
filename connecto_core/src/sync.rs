//! Bidirectional sync module for Connecto
//!
//! Enables two devices to simultaneously exchange SSH keys so both can SSH to each other.

use crate::error::{ConnectoError, Result};
use crate::keys::{KeyManager, SshKeyPair};
use crate::protocol::{Message, PROTOCOL_VERSION};
use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use rand::Rng;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

/// Service type for sync discovery (different from regular pairing)
pub const SYNC_SERVICE_TYPE: &str = "_connecto-sync._tcp.local.";

/// Default timeout for peer discovery
pub const DEFAULT_SYNC_TIMEOUT_SECS: u64 = 60;

/// Events emitted during sync operation
#[derive(Debug, Clone)]
pub enum SyncEvent {
    /// Started listening and advertising
    Started { address: SocketAddr },
    /// Searching for sync peer
    Searching,
    /// Found a potential sync peer
    PeerFound {
        device_name: String,
        address: SocketAddr,
    },
    /// Connected to peer, beginning key exchange
    Connected { device_name: String },
    /// Received peer's public key
    KeyReceived {
        device_name: String,
        key_comment: String,
    },
    /// Our key was accepted by peer
    KeyAccepted,
    /// Sync completed successfully
    Completed {
        peer_name: String,
        peer_user: String,
    },
    /// Sync failed
    Failed { message: String },
}

/// Result of a successful sync operation
#[derive(Debug, Clone)]
pub struct SyncResult {
    pub peer_name: String,
    pub peer_user: String,
    pub peer_address: IpAddr,
    pub peer_port: u16,
}

/// A discovered sync peer
#[derive(Debug, Clone)]
struct SyncPeer {
    device_name: String,
    addresses: Vec<IpAddr>,
    port: u16,
    #[allow(dead_code)]
    instance_name: String,
}

impl SyncPeer {
    fn primary_address(&self) -> Option<IpAddr> {
        self.addresses
            .iter()
            .find(|addr| addr.is_ipv4())
            .or(self.addresses.first())
            .copied()
    }

    fn connection_string(&self) -> Option<String> {
        self.primary_address()
            .map(|addr| format!("{}:{}", addr, self.port))
    }
}

/// Handler for bidirectional sync operations
pub struct SyncHandler {
    key_manager: Arc<KeyManager>,
    device_name: String,
    key_pair: SshKeyPair,
}

impl SyncHandler {
    /// Create a new sync handler
    pub fn new(key_manager: KeyManager, device_name: &str, key_pair: SshKeyPair) -> Self {
        Self {
            key_manager: Arc::new(key_manager),
            device_name: device_name.to_string(),
            key_pair,
        }
    }

    /// Run the sync operation
    ///
    /// This will:
    /// 1. Start listening on the specified port
    /// 2. Advertise via mDNS
    /// 3. Scan for other sync peers
    /// 4. When a peer is found, determine who initiates based on priority
    /// 5. Exchange keys bidirectionally
    pub async fn run(
        &self,
        port: u16,
        timeout_secs: u64,
        event_tx: mpsc::Sender<SyncEvent>,
    ) -> Result<SyncResult> {
        // Start listening
        let listener = TcpListener::bind(format!("0.0.0.0:{}", port))
            .await
            .map_err(|e| ConnectoError::Network(format!("Failed to bind: {}", e)))?;

        let local_addr = listener.local_addr()?;
        info!("Sync server listening on {}", local_addr);
        let _ = event_tx
            .send(SyncEvent::Started {
                address: local_addr,
            })
            .await;

        // Start mDNS advertising
        let mut advertiser = SyncAdvertiser::new()?;
        advertiser.advertise(&self.device_name, local_addr.port())?;

        // Start browsing for peers
        let _ = event_tx.send(SyncEvent::Searching).await;
        let browser = SyncBrowser::new()?;

        // Generate our initiator priority
        let our_priority: u64 = rand::thread_rng().gen();
        debug!("Our initiator priority: {}", our_priority);

        // Prepare our SSH user
        let ssh_user = std::env::var("USER")
            .or_else(|_| std::env::var("USERNAME"))
            .unwrap_or_else(|_| "unknown".to_string());

        // Create channels for coordination
        let (peer_found_tx, mut peer_found_rx) = mpsc::channel::<SyncPeer>(10);

        // Clone values for the browser task
        let device_name = self.device_name.clone();
        let browse_timeout = Duration::from_secs(timeout_secs);

        // Start browser in background
        let browser_handle = tokio::spawn(async move {
            browser
                .find_peers(&device_name, browse_timeout, peer_found_tx)
                .await
        });

        // Main event loop - either accept incoming connection or connect to found peer
        let timeout = tokio::time::sleep(Duration::from_secs(timeout_secs));
        tokio::pin!(timeout);

        let result = loop {
            tokio::select! {
                // Timeout
                _ = &mut timeout => {
                    let _ = event_tx.send(SyncEvent::Failed {
                        message: "Timeout waiting for sync peer".to_string(),
                    }).await;
                    break Err(ConnectoError::Timeout("No sync peer found".to_string()));
                }

                // Incoming connection
                accept_result = listener.accept() => {
                    match accept_result {
                        Ok((stream, peer_addr)) => {
                            info!("Incoming sync connection from {}", peer_addr);

                            // Handle as responder (we respond to their SyncHello)
                            match self.handle_as_responder(
                                stream,
                                peer_addr,
                                our_priority,
                                &ssh_user,
                                event_tx.clone(),
                            ).await {
                                Ok(result) => break Ok(result),
                                Err(e) => {
                                    warn!("Responder sync failed: {}", e);
                                    // Continue waiting for other connections
                                    continue;
                                }
                            }
                        }
                        Err(e) => {
                            warn!("Accept failed: {}", e);
                            continue;
                        }
                    }
                }

                // Found a peer via mDNS
                Some(peer) = peer_found_rx.recv() => {
                    info!("Found sync peer via mDNS: {}", peer.device_name);
                    let _ = event_tx.send(SyncEvent::PeerFound {
                        device_name: peer.device_name.clone(),
                        address: peer.primary_address()
                            .map(|ip| SocketAddr::new(ip, peer.port))
                            .unwrap_or_else(|| SocketAddr::new(IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED), peer.port)),
                    }).await;

                    if let Some(conn_str) = peer.connection_string() {
                        // Try to connect as initiator
                        match self.handle_as_initiator(
                            &conn_str,
                            our_priority,
                            &ssh_user,
                            event_tx.clone(),
                        ).await {
                            Ok(result) => break Ok(result),
                            Err(e) => {
                                warn!("Initiator sync failed: {}", e);
                                // Continue waiting - maybe they'll connect to us
                                continue;
                            }
                        }
                    }
                }
            }
        };

        // Cleanup
        browser_handle.abort();
        advertiser.stop()?;

        result
    }

    /// Handle sync as the initiator (we send SyncHello first)
    async fn handle_as_initiator(
        &self,
        address: &str,
        our_priority: u64,
        ssh_user: &str,
        event_tx: mpsc::Sender<SyncEvent>,
    ) -> Result<SyncResult> {
        let stream = TcpStream::connect(address)
            .await
            .map_err(|e| ConnectoError::Network(format!("Failed to connect: {}", e)))?;

        let peer_addr = stream.peer_addr()?;
        let (reader, mut writer) = stream.into_split();
        let mut reader = BufReader::new(reader);
        let mut line = String::new();

        // Send SyncHello
        let sync_hello = Message::SyncHello {
            version: PROTOCOL_VERSION,
            device_name: self.device_name.clone(),
            initiator_priority: our_priority,
            public_key: self.key_pair.public_key.clone(),
            key_comment: self.key_pair.comment.clone(),
            ssh_user: ssh_user.to_string(),
        };
        writer.write_all(sync_hello.to_json()?.as_bytes()).await?;

        // Read SyncHelloAck
        line.clear();
        reader.read_line(&mut line).await?;
        let response = Message::from_json(&line)?;

        match response {
            Message::SyncHelloAck {
                version,
                device_name: peer_name,
                public_key: peer_key,
                key_comment: peer_comment,
                ssh_user: peer_user,
                accept_sync,
            } => {
                if version != PROTOCOL_VERSION {
                    return Err(ConnectoError::Protocol(
                        "Protocol version mismatch".to_string(),
                    ));
                }

                if !accept_sync {
                    return Err(ConnectoError::SyncRejected(
                        "Peer declined sync".to_string(),
                    ));
                }

                let _ = event_tx
                    .send(SyncEvent::Connected {
                        device_name: peer_name.clone(),
                    })
                    .await;

                // Add peer's key to our authorized_keys
                debug!("Adding peer key to authorized_keys: {}", peer_comment);
                self.key_manager.add_authorized_key(&peer_key)?;

                let _ = event_tx
                    .send(SyncEvent::KeyReceived {
                        device_name: peer_name.clone(),
                        key_comment: peer_comment,
                    })
                    .await;

                // Send SyncComplete
                let complete = Message::SyncComplete {
                    success: true,
                    message: "Key exchange successful".to_string(),
                };
                writer.write_all(complete.to_json()?.as_bytes()).await?;

                // Read SyncComplete from peer
                line.clear();
                reader.read_line(&mut line).await?;
                let peer_complete = Message::from_json(&line)?;

                match peer_complete {
                    Message::SyncComplete { success, message } => {
                        if !success {
                            return Err(ConnectoError::Sync(format!(
                                "Peer reported failure: {}",
                                message
                            )));
                        }
                        let _ = event_tx.send(SyncEvent::KeyAccepted).await;
                    }
                    _ => {
                        return Err(ConnectoError::Protocol("Expected SyncComplete".to_string()));
                    }
                }

                let peer_ip = peer_addr.ip();
                let peer_port = peer_addr.port();

                let _ = event_tx
                    .send(SyncEvent::Completed {
                        peer_name: peer_name.clone(),
                        peer_user: peer_user.clone(),
                    })
                    .await;

                Ok(SyncResult {
                    peer_name,
                    peer_user,
                    peer_address: peer_ip,
                    peer_port,
                })
            }
            Message::Error { message, .. } => Err(ConnectoError::Sync(message)),
            _ => Err(ConnectoError::Protocol("Unexpected response".to_string())),
        }
    }

    /// Handle sync as the responder (we receive SyncHello first)
    async fn handle_as_responder(
        &self,
        stream: TcpStream,
        peer_addr: SocketAddr,
        our_priority: u64,
        ssh_user: &str,
        event_tx: mpsc::Sender<SyncEvent>,
    ) -> Result<SyncResult> {
        let (reader, mut writer) = stream.into_split();
        let mut reader = BufReader::new(reader);
        let mut line = String::new();

        // Read SyncHello
        line.clear();
        reader.read_line(&mut line).await?;
        let hello = Message::from_json(&line)?;

        match hello {
            Message::SyncHello {
                version,
                device_name: peer_name,
                initiator_priority: peer_priority,
                public_key: peer_key,
                key_comment: peer_comment,
                ssh_user: peer_user,
            } => {
                if version != PROTOCOL_VERSION {
                    let error_msg = Message::Error {
                        code: 1,
                        message: format!(
                            "Protocol version mismatch: expected {}, got {}",
                            PROTOCOL_VERSION, version
                        ),
                    };
                    writer.write_all(error_msg.to_json()?.as_bytes()).await?;
                    return Err(ConnectoError::Protocol(
                        "Protocol version mismatch".to_string(),
                    ));
                }

                // Check if this is ourselves (same device trying to sync with itself)
                if peer_name == self.device_name && peer_priority == our_priority {
                    let error_msg = Message::SyncHelloAck {
                        version: PROTOCOL_VERSION,
                        device_name: self.device_name.clone(),
                        public_key: String::new(),
                        key_comment: String::new(),
                        ssh_user: String::new(),
                        accept_sync: false,
                    };
                    writer.write_all(error_msg.to_json()?.as_bytes()).await?;
                    return Err(ConnectoError::SyncWithSelf);
                }

                // Priority tie-breaker: higher priority wins (becomes the effective initiator)
                // If we have lower priority, we yield and let them be the initiator
                // This message exchange is just for the sync, so we always accept
                let _ = event_tx
                    .send(SyncEvent::Connected {
                        device_name: peer_name.clone(),
                    })
                    .await;

                // Add peer's key to our authorized_keys
                debug!("Adding peer key to authorized_keys: {}", peer_comment);
                self.key_manager.add_authorized_key(&peer_key)?;

                let _ = event_tx
                    .send(SyncEvent::KeyReceived {
                        device_name: peer_name.clone(),
                        key_comment: peer_comment,
                    })
                    .await;

                // Send SyncHelloAck with our key
                let ack = Message::SyncHelloAck {
                    version: PROTOCOL_VERSION,
                    device_name: self.device_name.clone(),
                    public_key: self.key_pair.public_key.clone(),
                    key_comment: self.key_pair.comment.clone(),
                    ssh_user: ssh_user.to_string(),
                    accept_sync: true,
                };
                writer.write_all(ack.to_json()?.as_bytes()).await?;

                // Read SyncComplete from peer
                line.clear();
                reader.read_line(&mut line).await?;
                let peer_complete = Message::from_json(&line)?;

                match peer_complete {
                    Message::SyncComplete { success, message } => {
                        if !success {
                            return Err(ConnectoError::Sync(format!(
                                "Peer reported failure: {}",
                                message
                            )));
                        }
                        let _ = event_tx.send(SyncEvent::KeyAccepted).await;
                    }
                    _ => {
                        return Err(ConnectoError::Protocol("Expected SyncComplete".to_string()));
                    }
                }

                // Send our SyncComplete
                let complete = Message::SyncComplete {
                    success: true,
                    message: "Key exchange successful".to_string(),
                };
                writer.write_all(complete.to_json()?.as_bytes()).await?;

                let peer_ip = peer_addr.ip();
                let peer_port = peer_addr.port();

                let _ = event_tx
                    .send(SyncEvent::Completed {
                        peer_name: peer_name.clone(),
                        peer_user: peer_user.clone(),
                    })
                    .await;

                Ok(SyncResult {
                    peer_name,
                    peer_user,
                    peer_address: peer_ip,
                    peer_port,
                })
            }
            _ => {
                let error_msg = Message::Error {
                    code: 2,
                    message: "Expected SyncHello message".to_string(),
                };
                writer.write_all(error_msg.to_json()?.as_bytes()).await?;
                Err(ConnectoError::Protocol("Expected SyncHello".to_string()))
            }
        }
    }
}

/// mDNS service advertiser for sync
struct SyncAdvertiser {
    daemon: ServiceDaemon,
    service_fullname: Option<String>,
}

impl SyncAdvertiser {
    fn new() -> Result<Self> {
        let daemon = ServiceDaemon::new().map_err(|e| {
            ConnectoError::Discovery(format!("Failed to create mDNS daemon: {}", e))
        })?;

        Ok(Self {
            daemon,
            service_fullname: None,
        })
    }

    fn advertise(&mut self, device_name: &str, port: u16) -> Result<()> {
        let hostname = hostname::get()
            .map(|h: std::ffi::OsString| h.to_string_lossy().to_string())
            .unwrap_or_else(|_| "unknown".to_string());

        let service_hostname = format!("{}.local.", hostname);
        let instance_name = format!("{} ({})", device_name, hostname);

        let service_info = ServiceInfo::new(
            SYNC_SERVICE_TYPE,
            &instance_name,
            &service_hostname,
            "",
            port,
            None,
        )
        .map_err(|e| ConnectoError::Discovery(format!("Failed to create service info: {}", e)))?;

        let fullname = service_info.get_fullname().to_string();

        self.daemon
            .register(service_info)
            .map_err(|e| ConnectoError::Discovery(format!("Failed to register service: {}", e)))?;

        self.service_fullname = Some(fullname.clone());
        info!("Advertising sync service: {}", fullname);

        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        if let Some(fullname) = self.service_fullname.take() {
            // Ignore errors during unregister - the daemon may already be shut down
            let _ = self.daemon.unregister(&fullname);
            info!("Stopped advertising sync service");
        }
        Ok(())
    }
}

impl Drop for SyncAdvertiser {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

/// mDNS browser for finding sync peers
struct SyncBrowser {
    daemon: ServiceDaemon,
}

impl SyncBrowser {
    fn new() -> Result<Self> {
        let daemon = ServiceDaemon::new().map_err(|e| {
            ConnectoError::Discovery(format!("Failed to create mDNS daemon: {}", e))
        })?;

        Ok(Self { daemon })
    }

    async fn find_peers(
        &self,
        our_device_name: &str,
        timeout: Duration,
        peer_tx: mpsc::Sender<SyncPeer>,
    ) -> Result<()> {
        let receiver = self
            .daemon
            .browse(SYNC_SERVICE_TYPE)
            .map_err(|e| ConnectoError::Discovery(format!("Failed to browse: {}", e)))?;

        let our_name = our_device_name.to_string();

        // Run browser in blocking thread
        let handle = std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + timeout;

            while std::time::Instant::now() < deadline {
                match receiver.recv_timeout(Duration::from_millis(100)) {
                    Ok(ServiceEvent::ServiceResolved(info)) => {
                        let device_name = info.get_fullname().to_string();

                        // Skip our own service
                        if device_name.contains(&our_name) {
                            debug!("Skipping our own sync service");
                            continue;
                        }

                        let peer = SyncPeer {
                            device_name,
                            addresses: info.get_addresses().iter().copied().collect(),
                            port: info.get_port(),
                            instance_name: info.get_fullname().to_string(),
                        };

                        debug!("Found sync peer: {:?}", peer);

                        if peer_tx.blocking_send(peer).is_err() {
                            // Channel closed, stop browsing
                            break;
                        }
                    }
                    Ok(ServiceEvent::SearchStopped(_)) => {
                        break;
                    }
                    Err(flume::RecvTimeoutError::Timeout) => {
                        continue;
                    }
                    Err(flume::RecvTimeoutError::Disconnected) => {
                        break;
                    }
                    Ok(_) => {}
                }
            }
        });

        // Wait for the thread to complete
        tokio::task::spawn_blocking(move || {
            let _ = handle.join();
        })
        .await
        .map_err(|e| ConnectoError::Discovery(format!("Browser thread failed: {}", e)))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::KeyAlgorithm;
    use tempfile::TempDir;

    #[test]
    fn test_sync_service_type() {
        assert_eq!(SYNC_SERVICE_TYPE, "_connecto-sync._tcp.local.");
        // Must be different from regular pairing service
        assert_ne!(SYNC_SERVICE_TYPE, crate::discovery::SERVICE_TYPE);
    }

    #[test]
    fn test_sync_result() {
        let result = SyncResult {
            peer_name: "Device B".to_string(),
            peer_user: "bob".to_string(),
            peer_address: "192.168.1.100".parse().unwrap(),
            peer_port: 8099,
        };

        assert_eq!(result.peer_name, "Device B");
        assert_eq!(result.peer_user, "bob");
        assert_eq!(result.peer_address.to_string(), "192.168.1.100");
        assert_eq!(result.peer_port, 8099);
    }

    #[test]
    fn test_sync_peer_primary_address_prefers_ipv4() {
        let peer = SyncPeer {
            device_name: "Test".to_string(),
            addresses: vec![
                "::1".parse().unwrap(),
                "192.168.1.100".parse().unwrap(),
                "fe80::1".parse().unwrap(),
            ],
            port: 8099,
            instance_name: "test".to_string(),
        };

        let primary = peer.primary_address().unwrap();
        assert!(primary.is_ipv4());
        assert_eq!(primary.to_string(), "192.168.1.100");
    }

    #[test]
    fn test_sync_peer_connection_string() {
        let peer = SyncPeer {
            device_name: "Test".to_string(),
            addresses: vec!["192.168.1.100".parse().unwrap()],
            port: 8099,
            instance_name: "test".to_string(),
        };

        assert_eq!(
            peer.connection_string(),
            Some("192.168.1.100:8099".to_string())
        );
    }

    #[test]
    fn test_sync_handler_creation() {
        let temp_dir = TempDir::new().unwrap();
        let ssh_dir = temp_dir.path().join(".ssh");
        let key_manager = KeyManager::with_dir(ssh_dir);
        let key_pair = SshKeyPair::generate(KeyAlgorithm::Ed25519, "test@sync").unwrap();

        let handler = SyncHandler::new(key_manager, "Test Device", key_pair);
        assert_eq!(handler.device_name, "Test Device");
    }

    #[test]
    fn test_sync_event_variants() {
        let addr: SocketAddr = "127.0.0.1:8099".parse().unwrap();

        let events = vec![
            SyncEvent::Started { address: addr },
            SyncEvent::Searching,
            SyncEvent::PeerFound {
                device_name: "Peer".to_string(),
                address: addr,
            },
            SyncEvent::Connected {
                device_name: "Peer".to_string(),
            },
            SyncEvent::KeyReceived {
                device_name: "Peer".to_string(),
                key_comment: "test@peer".to_string(),
            },
            SyncEvent::KeyAccepted,
            SyncEvent::Completed {
                peer_name: "Peer".to_string(),
                peer_user: "user".to_string(),
            },
            SyncEvent::Failed {
                message: "Error".to_string(),
            },
        ];

        // Just verify all variants can be created
        assert_eq!(events.len(), 8);
    }

    #[tokio::test]
    async fn test_sync_bidirectional_key_exchange() {
        // This test simulates two devices doing a sync
        let temp_dir_a = TempDir::new().unwrap();
        let temp_dir_b = TempDir::new().unwrap();

        let ssh_dir_a = temp_dir_a.path().join(".ssh");
        let ssh_dir_b = temp_dir_b.path().join(".ssh");

        let key_manager_a = KeyManager::with_dir(ssh_dir_a.clone());
        let key_manager_b = KeyManager::with_dir(ssh_dir_b.clone());

        let key_pair_a = SshKeyPair::generate(KeyAlgorithm::Ed25519, "alice@device-a").unwrap();
        let key_pair_b = SshKeyPair::generate(KeyAlgorithm::Ed25519, "bob@device-b").unwrap();

        // Store the public keys for later verification
        let key_a_pub = key_pair_a.public_key.clone();
        let key_b_pub = key_pair_b.public_key.clone();

        let handler_a = SyncHandler::new(key_manager_a, "Device A", key_pair_a);
        let handler_b = SyncHandler::new(key_manager_b, "Device B", key_pair_b);

        // Start handler B as a listener
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let addr_str = addr.to_string();

        let (event_tx_a, mut event_rx_a) = mpsc::channel(10);
        let (event_tx_b, mut event_rx_b) = mpsc::channel(10);

        // Run B as responder
        let b_handle = tokio::spawn(async move {
            let (stream, peer_addr) = listener.accept().await.unwrap();
            let our_priority: u64 = rand::thread_rng().gen();
            let ssh_user = "bob".to_string();
            handler_b
                .handle_as_responder(stream, peer_addr, our_priority, &ssh_user, event_tx_b)
                .await
        });

        // Give B time to start
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Run A as initiator
        let our_priority: u64 = rand::thread_rng().gen();
        let ssh_user = "alice".to_string();
        let result_a = handler_a
            .handle_as_initiator(&addr_str, our_priority, &ssh_user, event_tx_a)
            .await
            .unwrap();

        // Wait for B to complete
        let result_b = b_handle.await.unwrap().unwrap();

        // Verify results
        assert_eq!(result_a.peer_name, "Device B");
        assert_eq!(result_a.peer_user, "bob");
        assert_eq!(result_b.peer_name, "Device A");
        assert_eq!(result_b.peer_user, "alice");

        // Verify keys were exchanged
        let key_manager_a = KeyManager::with_dir(ssh_dir_a);
        let key_manager_b = KeyManager::with_dir(ssh_dir_b);

        let keys_a = key_manager_a.list_authorized_keys().unwrap();
        let keys_b = key_manager_b.list_authorized_keys().unwrap();

        // A should have B's key
        assert_eq!(keys_a.len(), 1);
        assert!(keys_a[0].contains("bob@device-b"));

        // B should have A's key
        assert_eq!(keys_b.len(), 1);
        assert!(keys_b[0].contains("alice@device-a"));

        // Verify events were sent
        let mut events_a = Vec::new();
        while let Ok(event) = event_rx_a.try_recv() {
            events_a.push(event);
        }
        assert!(!events_a.is_empty());

        let mut events_b = Vec::new();
        while let Ok(event) = event_rx_b.try_recv() {
            events_b.push(event);
        }
        assert!(!events_b.is_empty());
    }
}
