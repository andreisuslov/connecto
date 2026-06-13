//! Bidirectional sync module for Connecto
//!
//! Enables two devices to simultaneously exchange SSH keys so both can SSH to each other.

use crate::discovery::{DiscoveryEvent, ServiceAdvertiser, ServiceBrowser, TXT_PRIORITY_KEY};
use crate::error::{ConnectoError, Result};
use crate::keys::{KeyManager, SshKeyPair};
use crate::protocol::{
    negotiate_version, read_message, write_message, Message, HANDSHAKE_READ_TIMEOUT,
    PROTOCOL_VERSION,
};
use rand::Rng;
use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::BufReader;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio::task::JoinSet;
use tokio::time::Instant;
use tracing::{debug, info, warn};

/// Service type for sync discovery (different from regular pairing)
pub const SYNC_SERVICE_TYPE: &str = "_connecto-sync._tcp.local.";

/// Default timeout for peer discovery
pub const DEFAULT_SYNC_TIMEOUT_SECS: u64 = 60;

/// Delay between re-initiation attempts toward a discovered peer
///
/// A rejected or failed initiation is retried on this cadence (bounded by
/// the run's overall deadline) instead of waiting for another mDNS
/// resolution: a transient failure of the winning direction, or a peer that
/// can accept inbound connections but not discover us, would otherwise stall
/// the sync until the full timeout.
const SYNC_RETRY_DELAY: Duration = Duration::from_secs(3);

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

/// Decide whether an incoming initiator wins the sync arbitration
///
/// The initiator wins iff its `(priority, device name)` pair is strictly
/// greater than ours; the device name is the deterministic secondary
/// tie-break for equal priorities. The responder accepts the sync only when
/// the initiator wins; otherwise it replies `accept_sync: false` and keeps
/// listening, because OUR initiator role outranks theirs and will be (or has
/// been) accepted by their responder. Exactly one direction wins, so mutual
/// simultaneous initiation converges instead of deadlocking.
fn initiator_wins(peer_priority: u64, peer_name: &str, our_priority: u64, our_name: &str) -> bool {
    (peer_priority, peer_name) > (our_priority, our_name)
}

/// Bound a sync attempt by the run's overall deadline
async fn with_deadline<F>(deadline: Instant, attempt: F) -> Result<SyncResult>
where
    F: Future<Output = Result<SyncResult>>,
{
    match tokio::time::timeout_at(deadline, attempt).await {
        Ok(result) => result,
        Err(_) => Err(ConnectoError::Timeout(
            "Sync attempt exceeded the overall deadline".to_string(),
        )),
    }
}

/// Handler for bidirectional sync operations
#[derive(Clone)]
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
    /// 2. Advertise via mDNS, publishing our random per-run priority as a
    ///    TXT property (used by browsers to recognize their own advertisement)
    /// 3. Scan for other sync peers
    /// 4. Attempt key exchange both ways; the responder-side arbitration
    ///    (see [`initiator_wins`]) ensures exactly one direction completes
    /// 5. Exchange keys bidirectionally; a failed install on either side
    ///    prevents (or rolls back) the other side's install, so an aborted
    ///    exchange leaves no one-sided SSH access behind
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

        // Generate our initiator priority for this run
        let our_priority: u64 = rand::thread_rng().gen();
        debug!("Our initiator priority: {}", our_priority);
        let priority_txt = our_priority.to_string();

        // Start mDNS advertising on the sync service type, publishing the
        // per-run priority so peers can tell us apart from a same-named device
        let mut advertiser = ServiceAdvertiser::new_for_service(SYNC_SERVICE_TYPE)?;
        advertiser.advertise_with_properties(
            &self.device_name,
            local_addr.port(),
            &[(TXT_PRIORITY_KEY, priority_txt.as_str())],
        )?;

        // Start browsing for peers, skipping only our own advertisement
        // (same name AND same TXT priority)
        let _ = event_tx.send(SyncEvent::Searching).await;
        let browser = ServiceBrowser::new_for_service(SYNC_SERVICE_TYPE)?
            .skip_own_instance(&self.device_name, &priority_txt);
        let peer_rx = browser.browse()?;

        let result = self
            .run_loop(listener, peer_rx, our_priority, timeout_secs, event_tx)
            .await;

        // Cleanup. Dropping the browser (and advertiser) shuts their mDNS
        // daemons down, which also terminates the browse bridge thread.
        advertiser.stop()?;
        drop(browser);

        result
    }

    /// Core sync loop over an already-bound listener and a peer discovery
    /// channel (separated from [`Self::run`] so it can be driven without real
    /// mDNS in tests)
    ///
    /// Initiator and responder attempts are spawned as concurrent tasks
    /// rather than awaited inline, so an incoming connection is still served
    /// while our own outgoing attempt is in flight; this is what lets mutual
    /// simultaneous initiation converge. Every attempt is additionally
    /// bounded by the run's overall deadline.
    async fn run_loop(
        &self,
        listener: TcpListener,
        mut peer_rx: mpsc::Receiver<DiscoveryEvent>,
        our_priority: u64,
        timeout_secs: u64,
        event_tx: mpsc::Sender<SyncEvent>,
    ) -> Result<SyncResult> {
        // Prepare our SSH user
        let ssh_user = std::env::var("USER")
            .or_else(|_| std::env::var("USERNAME"))
            .unwrap_or_else(|_| "unknown".to_string());

        let deadline = Instant::now() + Duration::from_secs(timeout_secs);
        let mut browse_active = true;
        let mut attempts: JoinSet<Result<SyncResult>> = JoinSet::new();
        // Peers we already run an initiation chain for (keyed by connection
        // string); repeated DeviceFound events must not spawn parallel chains.
        let mut initiating: std::collections::HashSet<String> = std::collections::HashSet::new();

        loop {
            tokio::select! {
                // Overall timeout
                _ = tokio::time::sleep_until(deadline) => {
                    let _ = event_tx.send(SyncEvent::Failed {
                        message: "Timeout waiting for sync peer".to_string(),
                    }).await;
                    break Err(ConnectoError::Timeout("No sync peer found".to_string()));
                }

                // Incoming connection: serve it as responder in a background
                // task so we keep accepting (and initiating) meanwhile
                accept_result = listener.accept() => {
                    match accept_result {
                        Ok((stream, peer_addr)) => {
                            info!("Incoming sync connection from {}", peer_addr);
                            let this = self.clone();
                            let ssh_user = ssh_user.clone();
                            let event_tx = event_tx.clone();
                            attempts.spawn(async move {
                                with_deadline(deadline, this.handle_as_responder(
                                    stream,
                                    peer_addr,
                                    our_priority,
                                    &ssh_user,
                                    event_tx,
                                )).await
                            });
                        }
                        Err(e) => {
                            warn!("Accept failed: {}", e);
                        }
                    }
                }

                // Found a peer via mDNS: try to connect as initiator in a
                // background task
                event = peer_rx.recv(), if browse_active => {
                    match event {
                        Some(DiscoveryEvent::DeviceFound(peer)) => {
                            info!("Found sync peer via mDNS: {}", peer.name);
                            let _ = event_tx.send(SyncEvent::PeerFound {
                                device_name: peer.name.clone(),
                                address: peer.primary_address()
                                    .map(|ip| SocketAddr::new(ip, peer.port))
                                    .unwrap_or_else(|| SocketAddr::new(IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED), peer.port)),
                            }).await;

                            if let Some(conn_str) = peer.connection_string() {
                                if initiating.insert(conn_str.clone()) {
                                    let this = self.clone();
                                    let ssh_user = ssh_user.clone();
                                    let event_tx = event_tx.clone();
                                    attempts.spawn(async move {
                                        with_deadline(deadline, async {
                                            // Retry rejected/failed initiations on a
                                            // fixed cadence until the overall deadline;
                                            // see SYNC_RETRY_DELAY for the rationale.
                                            loop {
                                                match this.handle_as_initiator(
                                                    &conn_str,
                                                    our_priority,
                                                    &ssh_user,
                                                    event_tx.clone(),
                                                ).await {
                                                    Ok(result) => break Ok(result),
                                                    Err(e) => {
                                                        warn!(
                                                            "Sync initiation to {} failed: {}; retrying in {:?}",
                                                            conn_str, e, SYNC_RETRY_DELAY
                                                        );
                                                        tokio::time::sleep(SYNC_RETRY_DELAY).await;
                                                    }
                                                }
                                            }
                                        }).await
                                    });
                                }
                            }
                        }
                        Some(_) => {
                            // Other discovery events are not relevant for sync
                        }
                        None => {
                            // Browse channel closed; disable this select arm
                            // and keep waiting for incoming connections.
                            debug!("Sync peer browse channel closed");
                            browse_active = false;
                        }
                    }
                }

                // A sync attempt finished: first success wins. Failures here
                // are responder-side errors or initiation chains that hit the
                // deadline (initiation failures retry internally above); they
                // just keep us waiting.
                Some(joined) = attempts.join_next(), if !attempts.is_empty() => {
                    match joined {
                        Ok(Ok(result)) => break Ok(result),
                        Ok(Err(e)) => {
                            warn!("Sync attempt failed: {}", e);
                        }
                        Err(e) => {
                            warn!("Sync attempt task failed: {}", e);
                        }
                    }
                }
            }
        }
        // Dropping the JoinSet aborts any attempts still in flight
    }

    /// Roll back a key installed during a failed exchange
    ///
    /// Only removes the key when this exchange actually installed it
    /// (`installed == true`); a key that was already authorized before the
    /// exchange (the re-sync case) is kept, so a failed re-sync can never
    /// revoke previously granted SSH access.
    fn rollback_installed_key(&self, peer_key: &str, peer_name: &str, installed: bool) {
        if !installed {
            info!(
                "Key for {} was already authorized before this exchange; keeping it",
                peer_name
            );
            return;
        }
        match self.key_manager.remove_authorized_key(peer_key) {
            Ok(true) => info!("Rolled back key installed for {}", peer_name),
            Ok(false) => warn!("Rollback found no matching key in authorized_keys"),
            Err(rollback_err) => warn!(
                "Rollback failed; key for {} remains installed: {}",
                peer_name, rollback_err
            ),
        }
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

        // Send SyncHello
        let sync_hello = Message::SyncHello {
            version: PROTOCOL_VERSION,
            device_name: self.device_name.clone(),
            initiator_priority: our_priority,
            public_key: self.key_pair.public_key.clone(),
            key_comment: self.key_pair.comment.clone(),
            ssh_user: ssh_user.to_string(),
        };
        write_message(&mut writer, &sync_hello).await?;

        // Read SyncHelloAck
        let response = read_message(&mut reader, HANDSHAKE_READ_TIMEOUT).await?;

        match response {
            Message::SyncHelloAck {
                version,
                device_name: peer_name,
                public_key: peer_key,
                key_comment: peer_comment,
                ssh_user: peer_user,
                accept_sync,
            } => {
                negotiate_version(version).map_err(ConnectoError::Protocol)?;

                if !accept_sync {
                    // The peer outranks us (sync arbitration): keep listening
                    // as responder; their initiator role will reach us.
                    return Err(ConnectoError::SyncRejected(
                        "Peer declined sync (peer outranks us); waiting for them to initiate"
                            .to_string(),
                    ));
                }

                let _ = event_tx
                    .send(SyncEvent::Connected {
                        device_name: peer_name.clone(),
                    })
                    .await;

                let _ = event_tx
                    .send(SyncEvent::KeyReceived {
                        device_name: peer_name.clone(),
                        key_comment: peer_comment,
                    })
                    .await;

                // Install the peer's key BEFORE confirming, so the exchange
                // stays symmetric: a failed install on our side is reported
                // to the responder (success: false), which then never
                // installs our key either.
                debug!("Adding peer key to authorized_keys");
                let installed = match self.key_manager.add_authorized_key(&peer_key) {
                    Ok(installed) => installed,
                    Err(e) => {
                        let failure = Message::SyncComplete {
                            success: false,
                            message: format!("Key installation failed: {}", e),
                        };
                        let _ = write_message(&mut writer, &failure).await;
                        return Err(e);
                    }
                };

                // Confirm the exchange; every failure from here on rolls our
                // freshly installed key back so a half-completed exchange
                // leaves no SSH access behind.
                let complete = Message::SyncComplete {
                    success: true,
                    message: "Key exchange successful".to_string(),
                };
                if let Err(e) = write_message(&mut writer, &complete).await {
                    warn!(
                        "Sync with {} failed after key install; rolling back: {}",
                        peer_name, e
                    );
                    self.rollback_installed_key(&peer_key, &peer_name, installed);
                    return Err(e);
                }

                // Read SyncComplete from peer
                let peer_complete = match read_message(&mut reader, HANDSHAKE_READ_TIMEOUT).await {
                    Ok(msg) => msg,
                    Err(e) => {
                        warn!(
                            "Sync with {} failed after key install; rolling back: {}",
                            peer_name, e
                        );
                        self.rollback_installed_key(&peer_key, &peer_name, installed);
                        return Err(e);
                    }
                };

                match peer_complete {
                    Message::SyncComplete { success, message } => {
                        if !success {
                            // The responder's install failed; it kept nothing,
                            // so neither do we.
                            self.rollback_installed_key(&peer_key, &peer_name, installed);
                            return Err(ConnectoError::Sync(format!(
                                "Peer reported failure: {}",
                                message
                            )));
                        }
                        let _ = event_tx.send(SyncEvent::KeyAccepted).await;
                    }
                    _ => {
                        self.rollback_installed_key(&peer_key, &peer_name, installed);
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

        // Read SyncHello
        let hello = read_message(&mut reader, HANDSHAKE_READ_TIMEOUT).await?;

        match hello {
            Message::SyncHello {
                version,
                device_name: peer_name,
                initiator_priority: peer_priority,
                public_key: peer_key,
                key_comment: peer_comment,
                ssh_user: peer_user,
            } => {
                if let Err(reason) = negotiate_version(version) {
                    let error_msg = Message::Error {
                        code: 1,
                        message: reason.clone(),
                    };
                    write_message(&mut writer, &error_msg).await?;
                    return Err(ConnectoError::Protocol(reason));
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
                    write_message(&mut writer, &error_msg).await?;
                    return Err(ConnectoError::SyncWithSelf);
                }

                // Sync arbitration: accept only if the initiator outranks us
                // (higher priority; device name breaks ties). Otherwise we
                // decline and keep listening - OUR initiator role outranks
                // theirs and will be accepted by their responder.
                if !initiator_wins(peer_priority, &peer_name, our_priority, &self.device_name) {
                    debug!(
                        "Declining sync from {} (priority {} does not outrank our {})",
                        peer_name, peer_priority, our_priority
                    );
                    let decline = Message::SyncHelloAck {
                        version: PROTOCOL_VERSION,
                        device_name: self.device_name.clone(),
                        public_key: String::new(),
                        key_comment: String::new(),
                        ssh_user: String::new(),
                        accept_sync: false,
                    };
                    write_message(&mut writer, &decline).await?;
                    return Err(ConnectoError::SyncRejected(format!(
                        "Initiator {} does not outrank us; we will initiate instead",
                        peer_name
                    )));
                }

                let _ = event_tx
                    .send(SyncEvent::Connected {
                        device_name: peer_name.clone(),
                    })
                    .await;

                let _ = event_tx
                    .send(SyncEvent::KeyReceived {
                        device_name: peer_name.clone(),
                        key_comment: peer_comment,
                    })
                    .await;

                // Send SyncHelloAck with our key. The peer's key is NOT
                // installed yet: installation happens only after the
                // initiator confirms success with SyncComplete.
                let ack = Message::SyncHelloAck {
                    version: PROTOCOL_VERSION,
                    device_name: self.device_name.clone(),
                    public_key: self.key_pair.public_key.clone(),
                    key_comment: self.key_pair.comment.clone(),
                    ssh_user: ssh_user.to_string(),
                    accept_sync: true,
                };
                write_message(&mut writer, &ack).await?;

                // Read SyncComplete from peer. A `success: false` here means
                // the initiator's install failed (it sends its confirmation
                // only after installing our key); we have installed nothing
                // yet, so failing out keeps the exchange symmetric.
                let peer_complete = read_message(&mut reader, HANDSHAKE_READ_TIMEOUT).await?;

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

                // The initiator confirmed (and has installed our key);
                // install its key. If installation fails we report the
                // failure to the peer, which then rolls its install back.
                debug!("Adding peer key to authorized_keys");
                let installed = match self.key_manager.add_authorized_key(&peer_key) {
                    Ok(installed) => installed,
                    Err(e) => {
                        let failure = Message::SyncComplete {
                            success: false,
                            message: format!("Key installation failed: {}", e),
                        };
                        let _ = write_message(&mut writer, &failure).await;
                        return Err(e);
                    }
                };

                // Send our SyncComplete; on failure roll the freshly
                // installed key back so a half-completed exchange leaves no
                // SSH access behind.
                let complete = Message::SyncComplete {
                    success: true,
                    message: "Key exchange successful".to_string(),
                };
                if let Err(e) = write_message(&mut writer, &complete).await {
                    warn!(
                        "Sync with {} failed after key install; rolling back: {}",
                        peer_name, e
                    );
                    self.rollback_installed_key(&peer_key, &peer_name, installed);
                    return Err(e);
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
            _ => {
                let error_msg = Message::Error {
                    code: 2,
                    message: "Expected SyncHello message".to_string(),
                };
                write_message(&mut writer, &error_msg).await?;
                Err(ConnectoError::Protocol("Expected SyncHello".to_string()))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::KeyAlgorithm;
    use tempfile::TempDir;

    #[test]
    fn test_sync_service_type_differs_from_pairing_service() {
        // The sync and pairing services must never share an mDNS service
        // type, or each would discover the other's listeners.
        assert_ne!(SYNC_SERVICE_TYPE, crate::discovery::SERVICE_TYPE);
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
        let _key_a_pub = key_pair_a.public_key.clone();
        let _key_b_pub = key_pair_b.public_key.clone();

        let handler_a = SyncHandler::new(key_manager_a, "Device A", key_pair_a);
        let handler_b = SyncHandler::new(key_manager_b, "Device B", key_pair_b);

        // Start handler B as a listener
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let addr_str = addr.to_string();

        let (event_tx_a, mut event_rx_a) = mpsc::channel(10);
        let (event_tx_b, mut event_rx_b) = mpsc::channel(10);

        // Run B as responder with a LOW priority so A's initiation wins the
        // arbitration (responders now accept only initiators that outrank them)
        let b_handle = tokio::spawn(async move {
            let (stream, peer_addr) = listener.accept().await.unwrap();
            let our_priority: u64 = 1;
            let ssh_user = "bob".to_string();
            handler_b
                .handle_as_responder(stream, peer_addr, our_priority, &ssh_user, event_tx_b)
                .await
        });

        // Give B time to start
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Run A as initiator with a HIGH priority
        let our_priority: u64 = u64::MAX;
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

    #[test]
    fn test_initiator_wins_arbitration() {
        // Higher priority wins regardless of name
        assert!(initiator_wins(10, "A", 5, "Z"));
        assert!(!initiator_wins(5, "Z", 10, "A"));

        // Equal priority: lexicographically greater device name wins
        assert!(initiator_wins(5, "Device B", 5, "Device A"));
        assert!(!initiator_wins(5, "Device A", 5, "Device B"));

        // Fully equal (self-sync shape) never wins
        assert!(!initiator_wins(5, "Device A", 5, "Device A"));
    }

    fn loopback_device(name: &str, addr: SocketAddr) -> crate::discovery::DiscoveredDevice {
        crate::discovery::DiscoveredDevice {
            name: name.to_string(),
            hostname: "loopback.local.".to_string(),
            addresses: vec![addr.ip()],
            port: addr.port(),
            instance_name: format!("{}.{}", name, SYNC_SERVICE_TYPE),
        }
    }

    /// Drive two in-process SyncHandlers that discover and initiate to each
    /// other SIMULTANEOUSLY (the deadlock scenario from the architecture
    /// review) and assert both complete with exchanged keys.
    async fn run_mutual_sync(priority_a: u64, priority_b: u64) {
        let temp_dir_a = TempDir::new().unwrap();
        let temp_dir_b = TempDir::new().unwrap();
        let ssh_dir_a = temp_dir_a.path().join(".ssh");
        let ssh_dir_b = temp_dir_b.path().join(".ssh");

        let key_pair_a = SshKeyPair::generate(KeyAlgorithm::Ed25519, "alice@device-a").unwrap();
        let key_pair_b = SshKeyPair::generate(KeyAlgorithm::Ed25519, "bob@device-b").unwrap();

        let handler_a = SyncHandler::new(
            KeyManager::with_dir(ssh_dir_a.clone()),
            "Device A",
            key_pair_a,
        );
        let handler_b = SyncHandler::new(
            KeyManager::with_dir(ssh_dir_b.clone()),
            "Device B",
            key_pair_b,
        );

        let listener_a = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let listener_b = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr_a = listener_a.local_addr().unwrap();
        let addr_b = listener_b.local_addr().unwrap();

        let (peer_tx_a, peer_rx_a) = mpsc::channel(4);
        let (peer_tx_b, peer_rx_b) = mpsc::channel(4);
        let (event_tx_a, _event_rx_a) = mpsc::channel(64);
        let (event_tx_b, _event_rx_b) = mpsc::channel(64);

        // Both sides "discover" each other at the same time
        peer_tx_a
            .send(DiscoveryEvent::DeviceFound(loopback_device(
                "Device B", addr_b,
            )))
            .await
            .unwrap();
        peer_tx_b
            .send(DiscoveryEvent::DeviceFound(loopback_device(
                "Device A", addr_a,
            )))
            .await
            .unwrap();

        let task_a = {
            let handler = handler_a.clone();
            tokio::spawn(async move {
                handler
                    .run_loop(listener_a, peer_rx_a, priority_a, 10, event_tx_a)
                    .await
            })
        };
        let task_b = {
            let handler = handler_b.clone();
            tokio::spawn(async move {
                handler
                    .run_loop(listener_b, peer_rx_b, priority_b, 10, event_tx_b)
                    .await
            })
        };

        let (result_a, result_b) = tokio::time::timeout(Duration::from_secs(8), async move {
            (task_a.await.unwrap(), task_b.await.unwrap())
        })
        .await
        .expect("mutual simultaneous sync deadlocked");

        let result_a = result_a.expect("sync A failed");
        let result_b = result_b.expect("sync B failed");
        assert_eq!(result_a.peer_name, "Device B");
        assert_eq!(result_b.peer_name, "Device A");

        // Keep the discovery channels open until both syncs completed
        drop(peer_tx_a);
        drop(peer_tx_b);

        let keys_a = KeyManager::with_dir(ssh_dir_a)
            .list_authorized_keys()
            .unwrap();
        let keys_b = KeyManager::with_dir(ssh_dir_b)
            .list_authorized_keys()
            .unwrap();
        assert_eq!(keys_a.len(), 1, "A must hold exactly B's key");
        assert!(keys_a[0].contains("bob@device-b"));
        assert_eq!(keys_b.len(), 1, "B must hold exactly A's key");
        assert!(keys_b[0].contains("alice@device-a"));
    }

    #[tokio::test]
    async fn test_mutual_simultaneous_sync_converges() {
        run_mutual_sync(u64::MAX, 1).await;
    }

    #[tokio::test]
    async fn test_mutual_simultaneous_sync_converges_on_equal_priorities() {
        // Equal priorities fall back to the device-name tie-break
        run_mutual_sync(7, 7).await;
    }

    #[tokio::test]
    async fn test_responder_rejects_initiator_it_outranks() {
        let temp_dir = TempDir::new().unwrap();
        let ssh_dir = temp_dir.path().join(".ssh");
        let key_pair = SshKeyPair::generate(KeyAlgorithm::Ed25519, "bob@responder").unwrap();
        let handler =
            SyncHandler::new(KeyManager::with_dir(ssh_dir.clone()), "Responder", key_pair);

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (event_tx, _event_rx) = mpsc::channel(16);

        let responder = tokio::spawn(async move {
            let (stream, peer_addr) = listener.accept().await.unwrap();
            handler
                .handle_as_responder(stream, peer_addr, 1000, "bob", event_tx)
                .await
        });

        // Raw initiator with a LOWER priority
        let stream = TcpStream::connect(addr).await.unwrap();
        let (reader, mut writer) = stream.into_split();
        let mut reader = BufReader::new(reader);
        let initiator_key = SshKeyPair::generate(KeyAlgorithm::Ed25519, "eve@initiator").unwrap();
        let hello = Message::SyncHello {
            version: PROTOCOL_VERSION,
            device_name: "Initiator".to_string(),
            initiator_priority: 5,
            public_key: initiator_key.public_key.clone(),
            key_comment: initiator_key.comment.clone(),
            ssh_user: "eve".to_string(),
        };
        write_message(&mut writer, &hello).await.unwrap();

        let ack = read_message(&mut reader, HANDSHAKE_READ_TIMEOUT)
            .await
            .unwrap();
        match ack {
            Message::SyncHelloAck { accept_sync, .. } => assert!(!accept_sync),
            other => panic!("Expected SyncHelloAck, got {:?}", other),
        }

        let result = responder.await.unwrap();
        assert!(matches!(result, Err(ConnectoError::SyncRejected(_))));

        // The declined initiator's key must NOT be installed
        let keys = KeyManager::with_dir(ssh_dir)
            .list_authorized_keys()
            .unwrap();
        assert!(keys.is_empty());
    }

    #[tokio::test]
    async fn test_responder_installs_nothing_without_initiator_confirmation() {
        let temp_dir = TempDir::new().unwrap();
        let ssh_dir = temp_dir.path().join(".ssh");
        let key_pair = SshKeyPair::generate(KeyAlgorithm::Ed25519, "bob@responder").unwrap();
        let handler =
            SyncHandler::new(KeyManager::with_dir(ssh_dir.clone()), "Responder", key_pair);

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (event_tx, _event_rx) = mpsc::channel(16);

        let responder = tokio::spawn(async move {
            let (stream, peer_addr) = listener.accept().await.unwrap();
            handler
                .handle_as_responder(stream, peer_addr, 1, "bob", event_tx)
                .await
        });

        // Initiator that wins arbitration, reads the ack... then vanishes
        // without ever confirming with SyncComplete.
        let stream = TcpStream::connect(addr).await.unwrap();
        let (reader, mut writer) = stream.into_split();
        let mut reader = BufReader::new(reader);
        let initiator_key = SshKeyPair::generate(KeyAlgorithm::Ed25519, "eve@initiator").unwrap();
        let hello = Message::SyncHello {
            version: PROTOCOL_VERSION,
            device_name: "Initiator".to_string(),
            initiator_priority: u64::MAX,
            public_key: initiator_key.public_key.clone(),
            key_comment: initiator_key.comment.clone(),
            ssh_user: "eve".to_string(),
        };
        write_message(&mut writer, &hello).await.unwrap();

        let ack = read_message(&mut reader, HANDSHAKE_READ_TIMEOUT)
            .await
            .unwrap();
        match ack {
            Message::SyncHelloAck { accept_sync, .. } => assert!(accept_sync),
            other => panic!("Expected SyncHelloAck, got {:?}", other),
        }
        drop(writer);
        drop(reader);

        let result = responder.await.unwrap();
        assert!(result.is_err(), "responder must fail without confirmation");

        // Regression: the key used to be installed BEFORE the exchange was
        // confirmed; an aborted exchange must leave authorized_keys empty.
        let keys = KeyManager::with_dir(ssh_dir)
            .list_authorized_keys()
            .unwrap();
        assert!(keys.is_empty());
    }

    /// A rejected first initiation must be retried (and succeed) within the
    /// run's deadline instead of stalling until timeout.
    #[tokio::test]
    async fn test_run_loop_retries_after_declined_initiation() {
        let temp_dir = TempDir::new().unwrap();
        let ssh_dir = temp_dir.path().join(".ssh");
        let key_pair = SshKeyPair::generate(KeyAlgorithm::Ed25519, "alice@initiator").unwrap();
        let handler =
            SyncHandler::new(KeyManager::with_dir(ssh_dir.clone()), "Initiator", key_pair);

        // Scripted peer: declines the first attempt, accepts the second.
        let peer_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let peer_addr = peer_listener.local_addr().unwrap();
        let responder_key = SshKeyPair::generate(KeyAlgorithm::Ed25519, "bob@responder").unwrap();
        let scripted = tokio::spawn(async move {
            for accept in [false, true] {
                let (stream, _) = peer_listener.accept().await.unwrap();
                let (reader, mut writer) = stream.into_split();
                let mut reader = BufReader::new(reader);
                read_message(&mut reader, HANDSHAKE_READ_TIMEOUT)
                    .await
                    .unwrap();

                let ack = Message::SyncHelloAck {
                    version: PROTOCOL_VERSION,
                    device_name: "Responder".to_string(),
                    public_key: if accept {
                        responder_key.public_key.clone()
                    } else {
                        String::new()
                    },
                    key_comment: if accept {
                        responder_key.comment.clone()
                    } else {
                        String::new()
                    },
                    ssh_user: if accept {
                        "bob".to_string()
                    } else {
                        String::new()
                    },
                    accept_sync: accept,
                };
                write_message(&mut writer, &ack).await.unwrap();

                if accept {
                    let msg = read_message(&mut reader, HANDSHAKE_READ_TIMEOUT)
                        .await
                        .unwrap();
                    assert!(
                        matches!(msg, Message::SyncComplete { success: true, .. }),
                        "initiator must confirm only after installing, got {:?}",
                        msg
                    );
                    let complete = Message::SyncComplete {
                        success: true,
                        message: "ok".to_string(),
                    };
                    write_message(&mut writer, &complete).await.unwrap();
                }
            }
        });

        // Drive run_loop with a single DeviceFound for the scripted peer.
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let (peer_tx, peer_rx) = mpsc::channel(4);
        let (event_tx, _event_rx) = mpsc::channel(64);
        peer_tx
            .send(DiscoveryEvent::DeviceFound(loopback_device(
                "Responder",
                peer_addr,
            )))
            .await
            .unwrap();

        let result = handler
            .run_loop(listener, peer_rx, 42, 15, event_tx)
            .await
            .expect("a declined initiation must be retried until it succeeds");
        assert_eq!(result.peer_name, "Responder");
        scripted.await.unwrap();
        drop(peer_tx);

        let keys = KeyManager::with_dir(ssh_dir)
            .list_authorized_keys()
            .unwrap();
        assert_eq!(keys.len(), 1);
        assert!(keys[0].contains("bob@responder"));
    }

    /// Regression for the rollback-removes-pre-existing-key bug: when the
    /// peer's key was ALREADY authorized before the exchange (re-sync), a
    /// failed exchange must NOT remove it.
    #[tokio::test]
    async fn test_failed_exchange_preserves_preexisting_key() {
        let temp_dir = TempDir::new().unwrap();
        let ssh_dir = temp_dir.path().join(".ssh");
        let key_pair = SshKeyPair::generate(KeyAlgorithm::Ed25519, "alice@initiator").unwrap();
        let key_manager = KeyManager::with_dir(ssh_dir.clone());
        let responder_key =
            SshKeyPair::generate(KeyAlgorithm::Ed25519, "mallory@responder").unwrap();

        // Pre-seed the peer's key, as an earlier successful exchange would
        assert!(key_manager
            .add_authorized_key(&responder_key.public_key)
            .unwrap());

        let handler = SyncHandler::new(key_manager, "Initiator", key_pair);

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();

        // Scripted responder: accepts, then reports failure in SyncComplete,
        // which makes the initiator run its rollback path.
        let scripted = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let (reader, mut writer) = stream.into_split();
            let mut reader = BufReader::new(reader);
            read_message(&mut reader, HANDSHAKE_READ_TIMEOUT)
                .await
                .unwrap();

            let ack = Message::SyncHelloAck {
                version: PROTOCOL_VERSION,
                device_name: "Responder".to_string(),
                public_key: responder_key.public_key.clone(),
                key_comment: responder_key.comment.clone(),
                ssh_user: "mallory".to_string(),
                accept_sync: true,
            };
            write_message(&mut writer, &ack).await.unwrap();

            read_message(&mut reader, HANDSHAKE_READ_TIMEOUT)
                .await
                .unwrap();
            let failure = Message::SyncComplete {
                success: false,
                message: "installation failed".to_string(),
            };
            write_message(&mut writer, &failure).await.unwrap();
        });

        let (event_tx, _event_rx) = mpsc::channel(16);
        let result = handler
            .handle_as_initiator(&addr, 42, "alice", event_tx)
            .await;
        assert!(result.is_err());
        scripted.await.unwrap();

        // The pre-existing key predates the failed exchange and must survive
        let keys = KeyManager::with_dir(ssh_dir)
            .list_authorized_keys()
            .unwrap();
        assert_eq!(
            keys.len(),
            1,
            "rollback must not remove a key that predates the exchange"
        );
        assert!(keys[0].contains("mallory@responder"));
    }

    /// A responder receiving `SyncComplete { success: false }` (initiator's
    /// install failed) must fail without installing the initiator's key.
    #[tokio::test]
    async fn test_responder_installs_nothing_on_initiator_reported_failure() {
        let temp_dir = TempDir::new().unwrap();
        let ssh_dir = temp_dir.path().join(".ssh");
        let key_pair = SshKeyPair::generate(KeyAlgorithm::Ed25519, "bob@responder").unwrap();
        let handler =
            SyncHandler::new(KeyManager::with_dir(ssh_dir.clone()), "Responder", key_pair);

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (event_tx, _event_rx) = mpsc::channel(16);

        let responder = tokio::spawn(async move {
            let (stream, peer_addr) = listener.accept().await.unwrap();
            handler
                .handle_as_responder(stream, peer_addr, 1, "bob", event_tx)
                .await
        });

        // Winning initiator whose key installation "failed"
        let stream = TcpStream::connect(addr).await.unwrap();
        let (reader, mut writer) = stream.into_split();
        let mut reader = BufReader::new(reader);
        let initiator_key = SshKeyPair::generate(KeyAlgorithm::Ed25519, "eve@initiator").unwrap();
        let hello = Message::SyncHello {
            version: PROTOCOL_VERSION,
            device_name: "Initiator".to_string(),
            initiator_priority: u64::MAX,
            public_key: initiator_key.public_key.clone(),
            key_comment: initiator_key.comment.clone(),
            ssh_user: "eve".to_string(),
        };
        write_message(&mut writer, &hello).await.unwrap();

        let ack = read_message(&mut reader, HANDSHAKE_READ_TIMEOUT)
            .await
            .unwrap();
        assert!(matches!(
            ack,
            Message::SyncHelloAck {
                accept_sync: true,
                ..
            }
        ));

        let failure = Message::SyncComplete {
            success: false,
            message: "Key installation failed".to_string(),
        };
        write_message(&mut writer, &failure).await.unwrap();

        let result = responder.await.unwrap();
        assert!(matches!(result, Err(ConnectoError::Sync(_))));

        let keys = KeyManager::with_dir(ssh_dir)
            .list_authorized_keys()
            .unwrap();
        assert!(
            keys.is_empty(),
            "responder must not keep an install when the initiator reported failure"
        );
    }

    #[tokio::test]
    async fn test_initiator_installs_nothing_when_rejected_or_peer_fails() {
        for peer_succeeds_handshake in [false, true] {
            let temp_dir = TempDir::new().unwrap();
            let ssh_dir = temp_dir.path().join(".ssh");
            let key_pair = SshKeyPair::generate(KeyAlgorithm::Ed25519, "alice@initiator").unwrap();
            let handler =
                SyncHandler::new(KeyManager::with_dir(ssh_dir.clone()), "Initiator", key_pair);

            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap().to_string();
            let responder_key =
                SshKeyPair::generate(KeyAlgorithm::Ed25519, "mallory@responder").unwrap();

            // Scripted responder: either declines outright, or accepts and
            // then reports failure in SyncComplete.
            let scripted = tokio::spawn(async move {
                let (stream, _) = listener.accept().await.unwrap();
                let (reader, mut writer) = stream.into_split();
                let mut reader = BufReader::new(reader);
                read_message(&mut reader, HANDSHAKE_READ_TIMEOUT)
                    .await
                    .unwrap();

                let ack = Message::SyncHelloAck {
                    version: PROTOCOL_VERSION,
                    device_name: "Responder".to_string(),
                    public_key: responder_key.public_key.clone(),
                    key_comment: responder_key.comment.clone(),
                    ssh_user: "mallory".to_string(),
                    accept_sync: peer_succeeds_handshake,
                };
                write_message(&mut writer, &ack).await.unwrap();

                if peer_succeeds_handshake {
                    // Consume the initiator's SyncComplete, then report failure
                    read_message(&mut reader, HANDSHAKE_READ_TIMEOUT)
                        .await
                        .unwrap();
                    let failure = Message::SyncComplete {
                        success: false,
                        message: "installation failed".to_string(),
                    };
                    write_message(&mut writer, &failure).await.unwrap();
                }
            });

            let (event_tx, _event_rx) = mpsc::channel(16);
            let result = handler
                .handle_as_initiator(&addr, 42, "alice", event_tx)
                .await;
            assert!(result.is_err());
            scripted.await.unwrap();

            // Neither a rejection nor a peer-reported failure may leave the
            // peer's key installed.
            let keys = KeyManager::with_dir(ssh_dir)
                .list_authorized_keys()
                .unwrap();
            assert!(keys.is_empty(), "no key may be installed on failure");
        }
    }
}
