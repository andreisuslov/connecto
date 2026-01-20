//! Handshake protocol for Connecto pairing
//!
//! Defines the protocol for exchanging SSH keys between devices

use crate::error::{ConnectoError, Result};
use crate::keys::{KeyManager, SshKeyPair};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

/// Protocol version for compatibility checking
pub const PROTOCOL_VERSION: u32 = 1;

/// Message types in the handshake protocol
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Message {
    /// Initial hello from client
    Hello {
        version: u32,
        device_name: String,
    },

    /// Server acknowledges hello
    HelloAck {
        version: u32,
        device_name: String,
        verification_code: Option<String>,
    },

    /// Client sends its public key
    KeyExchange {
        public_key: String,
        comment: String,
    },

    /// Server acknowledges key received and installed
    KeyAccepted {
        message: String,
    },

    /// Error occurred
    Error {
        code: u32,
        message: String,
    },

    /// Pairing complete
    PairingComplete {
        ssh_user: String,
    },
}

impl Message {
    /// Serialize message to JSON with newline
    pub fn to_json(&self) -> Result<String> {
        let json = serde_json::to_string(self)?;
        Ok(format!("{}\n", json))
    }

    /// Deserialize message from JSON
    pub fn from_json(s: &str) -> Result<Self> {
        Ok(serde_json::from_str(s.trim())?)
    }
}

/// Events emitted by the handshake server
#[derive(Debug, Clone)]
pub enum ServerEvent {
    Started { address: SocketAddr },
    ClientConnected { address: SocketAddr },
    PairingRequest { device_name: String, address: SocketAddr },
    KeyReceived { comment: String },
    PairingComplete { device_name: String },
    Error { message: String },
}

/// Handshake server that listens for pairing requests
pub struct HandshakeServer {
    listener: Option<TcpListener>,
    key_manager: Arc<KeyManager>,
    device_name: String,
    require_verification: bool,
}

impl HandshakeServer {
    /// Create a new handshake server
    pub fn new(key_manager: KeyManager, device_name: &str) -> Self {
        Self {
            listener: None,
            key_manager: Arc::new(key_manager),
            device_name: device_name.to_string(),
            require_verification: false,
        }
    }

    /// Enable verification code requirement
    pub fn with_verification(mut self, require: bool) -> Self {
        self.require_verification = require;
        self
    }

    /// Start listening on the specified port
    pub async fn listen(&mut self, port: u16) -> Result<SocketAddr> {
        let addr = format!("0.0.0.0:{}", port);
        let listener = TcpListener::bind(&addr)
            .await
            .map_err(|e| ConnectoError::Network(format!("Failed to bind: {}", e)))?;

        let local_addr = listener.local_addr()?;
        info!("Handshake server listening on {}", local_addr);

        self.listener = Some(listener);
        Ok(local_addr)
    }

    /// Accept and handle incoming connections
    pub async fn run(&mut self, event_tx: mpsc::Sender<ServerEvent>) -> Result<()> {
        let listener = self
            .listener
            .take()
            .ok_or_else(|| ConnectoError::Network("Server not started".to_string()))?;

        let addr = listener.local_addr()?;
        let _ = event_tx.send(ServerEvent::Started { address: addr }).await;

        loop {
            match listener.accept().await {
                Ok((stream, peer_addr)) => {
                    info!("Client connected from {}", peer_addr);
                    let _ = event_tx
                        .send(ServerEvent::ClientConnected { address: peer_addr })
                        .await;

                    let key_manager = Arc::clone(&self.key_manager);
                    let device_name = self.device_name.clone();
                    let require_verification = self.require_verification;
                    let event_tx = event_tx.clone();

                    tokio::spawn(async move {
                        if let Err(e) = handle_client(
                            stream,
                            peer_addr,
                            key_manager,
                            device_name,
                            require_verification,
                            event_tx,
                        )
                        .await
                        {
                            error!("Error handling client {}: {}", peer_addr, e);
                        }
                    });
                }
                Err(e) => {
                    warn!("Failed to accept connection: {}", e);
                }
            }
        }
    }

    /// Handle a single pairing request (useful for one-shot mode)
    pub async fn handle_one(&mut self, event_tx: mpsc::Sender<ServerEvent>) -> Result<()> {
        let listener = self
            .listener
            .take()
            .ok_or_else(|| ConnectoError::Network("Server not started".to_string()))?;

        let addr = listener.local_addr()?;
        let _ = event_tx.send(ServerEvent::Started { address: addr }).await;

        let (stream, peer_addr) = listener.accept().await?;
        info!("Client connected from {}", peer_addr);
        let _ = event_tx
            .send(ServerEvent::ClientConnected { address: peer_addr })
            .await;

        handle_client(
            stream,
            peer_addr,
            Arc::clone(&self.key_manager),
            self.device_name.clone(),
            self.require_verification,
            event_tx,
        )
        .await
    }
}

async fn handle_client(
    stream: TcpStream,
    peer_addr: SocketAddr,
    key_manager: Arc<KeyManager>,
    device_name: String,
    require_verification: bool,
    event_tx: mpsc::Sender<ServerEvent>,
) -> Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    let mut line = String::new();

    // Read Hello message
    line.clear();
    reader.read_line(&mut line).await?;
    let hello = Message::from_json(&line)?;

    let client_name = match hello {
        Message::Hello {
            version,
            device_name: client_name,
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
                return Err(ConnectoError::Handshake("Protocol version mismatch".to_string()));
            }
            client_name
        }
        _ => {
            let error_msg = Message::Error {
                code: 2,
                message: "Expected Hello message".to_string(),
            };
            writer.write_all(error_msg.to_json()?.as_bytes()).await?;
            return Err(ConnectoError::Handshake("Expected Hello".to_string()));
        }
    };

    let _ = event_tx
        .send(ServerEvent::PairingRequest {
            device_name: client_name.clone(),
            address: peer_addr,
        })
        .await;

    // Generate verification code if required
    let verification_code = if require_verification {
        Some(generate_verification_code())
    } else {
        None
    };

    // Send HelloAck
    let hello_ack = Message::HelloAck {
        version: PROTOCOL_VERSION,
        device_name: device_name.clone(),
        verification_code: verification_code.clone(),
    };
    writer.write_all(hello_ack.to_json()?.as_bytes()).await?;

    // Read KeyExchange
    line.clear();
    reader.read_line(&mut line).await?;
    let key_exchange = Message::from_json(&line)?;

    match key_exchange {
        Message::KeyExchange { public_key, comment } => {
            debug!("Received public key with comment: {}", comment);

            let _ = event_tx
                .send(ServerEvent::KeyReceived {
                    comment: comment.clone(),
                })
                .await;

            // Add the key to authorized_keys
            key_manager.add_authorized_key(&public_key)?;

            // Send KeyAccepted
            let accepted = Message::KeyAccepted {
                message: "Key added to authorized_keys".to_string(),
            };
            writer.write_all(accepted.to_json()?.as_bytes()).await?;

            // Get current user (USER on Unix, USERNAME on Windows)
            let ssh_user = std::env::var("USER")
                .or_else(|_| std::env::var("USERNAME"))
                .unwrap_or_else(|_| "unknown".to_string());

            // Send PairingComplete
            let complete = Message::PairingComplete { ssh_user };
            writer.write_all(complete.to_json()?.as_bytes()).await?;

            let _ = event_tx
                .send(ServerEvent::PairingComplete {
                    device_name: client_name,
                })
                .await;

            Ok(())
        }
        _ => {
            let error_msg = Message::Error {
                code: 3,
                message: "Expected KeyExchange message".to_string(),
            };
            writer.write_all(error_msg.to_json()?.as_bytes()).await?;
            Err(ConnectoError::Handshake("Expected KeyExchange".to_string()))
        }
    }
}

/// Client for initiating pairing with a server
pub struct HandshakeClient {
    device_name: String,
}

impl HandshakeClient {
    /// Create a new handshake client
    pub fn new(device_name: &str) -> Self {
        Self {
            device_name: device_name.to_string(),
        }
    }

    /// Connect to a server and perform key exchange
    pub async fn pair(&self, address: &str, key_pair: &SshKeyPair) -> Result<PairingResult> {
        let stream = TcpStream::connect(address)
            .await
            .map_err(|e| ConnectoError::Network(format!("Failed to connect: {}", e)))?;

        let (reader, mut writer) = stream.into_split();
        let mut reader = BufReader::new(reader);
        let mut line = String::new();

        // Send Hello
        let hello = Message::Hello {
            version: PROTOCOL_VERSION,
            device_name: self.device_name.clone(),
        };
        writer.write_all(hello.to_json()?.as_bytes()).await?;

        // Read HelloAck
        line.clear();
        reader.read_line(&mut line).await?;
        let hello_ack = Message::from_json(&line)?;

        let (server_name, verification_code) = match hello_ack {
            Message::HelloAck {
                version,
                device_name,
                verification_code,
            } => {
                if version != PROTOCOL_VERSION {
                    return Err(ConnectoError::Handshake(
                        "Protocol version mismatch".to_string(),
                    ));
                }
                (device_name, verification_code)
            }
            Message::Error { message, .. } => {
                return Err(ConnectoError::Handshake(message));
            }
            _ => {
                return Err(ConnectoError::Handshake("Unexpected response".to_string()));
            }
        };

        // Send KeyExchange
        let key_exchange = Message::KeyExchange {
            public_key: key_pair.public_key.clone(),
            comment: key_pair.comment.clone(),
        };
        writer.write_all(key_exchange.to_json()?.as_bytes()).await?;

        // Read KeyAccepted
        line.clear();
        reader.read_line(&mut line).await?;
        let accepted = Message::from_json(&line)?;

        match accepted {
            Message::KeyAccepted { .. } => {}
            Message::Error { message, .. } => {
                return Err(ConnectoError::Handshake(message));
            }
            _ => {
                return Err(ConnectoError::Handshake(
                    "Expected KeyAccepted".to_string(),
                ));
            }
        }

        // Read PairingComplete
        line.clear();
        reader.read_line(&mut line).await?;
        let complete = Message::from_json(&line)?;

        match complete {
            Message::PairingComplete { ssh_user } => Ok(PairingResult {
                server_name,
                ssh_user,
                verification_code,
            }),
            _ => Err(ConnectoError::Handshake(
                "Expected PairingComplete".to_string(),
            )),
        }
    }
}

/// Result of a successful pairing
#[derive(Debug, Clone)]
pub struct PairingResult {
    pub server_name: String,
    pub ssh_user: String,
    pub verification_code: Option<String>,
}

/// Generate a random 4-digit verification code
pub fn generate_verification_code() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    format!("{:04}", rng.gen_range(0..10000))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_protocol_version() {
        assert_eq!(PROTOCOL_VERSION, 1);
    }

    #[test]
    fn test_message_hello_serialization() {
        let msg = Message::Hello {
            version: 1,
            device_name: "Test Device".to_string(),
        };

        let json = msg.to_json().unwrap();
        assert!(json.contains("Hello"));
        assert!(json.contains("Test Device"));
        assert!(json.ends_with('\n'));
    }

    #[test]
    fn test_message_hello_deserialization() {
        let json = r#"{"type":"Hello","version":1,"device_name":"Test"}"#;
        let msg = Message::from_json(json).unwrap();

        match msg {
            Message::Hello {
                version,
                device_name,
            } => {
                assert_eq!(version, 1);
                assert_eq!(device_name, "Test");
            }
            _ => panic!("Wrong message type"),
        }
    }

    #[test]
    fn test_message_hello_ack_serialization() {
        let msg = Message::HelloAck {
            version: 1,
            device_name: "Server".to_string(),
            verification_code: Some("1234".to_string()),
        };

        let json = msg.to_json().unwrap();
        let deserialized = Message::from_json(&json).unwrap();

        match deserialized {
            Message::HelloAck {
                version,
                device_name,
                verification_code,
            } => {
                assert_eq!(version, 1);
                assert_eq!(device_name, "Server");
                assert_eq!(verification_code, Some("1234".to_string()));
            }
            _ => panic!("Wrong message type"),
        }
    }

    #[test]
    fn test_message_key_exchange_serialization() {
        let msg = Message::KeyExchange {
            public_key: "ssh-ed25519 AAAAC3... test@connecto".to_string(),
            comment: "test@connecto".to_string(),
        };

        let json = msg.to_json().unwrap();
        assert!(json.contains("KeyExchange"));
        assert!(json.contains("ssh-ed25519"));
    }

    #[test]
    fn test_message_error_serialization() {
        let msg = Message::Error {
            code: 42,
            message: "Something went wrong".to_string(),
        };

        let json = msg.to_json().unwrap();
        let deserialized = Message::from_json(&json).unwrap();

        match deserialized {
            Message::Error { code, message } => {
                assert_eq!(code, 42);
                assert_eq!(message, "Something went wrong");
            }
            _ => panic!("Wrong message type"),
        }
    }

    #[test]
    fn test_message_pairing_complete_serialization() {
        let msg = Message::PairingComplete {
            ssh_user: "testuser".to_string(),
        };

        let json = msg.to_json().unwrap();
        let deserialized = Message::from_json(&json).unwrap();

        match deserialized {
            Message::PairingComplete { ssh_user } => {
                assert_eq!(ssh_user, "testuser");
            }
            _ => panic!("Wrong message type"),
        }
    }

    #[test]
    fn test_generate_verification_code() {
        let code = generate_verification_code();
        assert_eq!(code.len(), 4);
        assert!(code.chars().all(|c| c.is_ascii_digit()));
    }

    #[test]
    fn test_generate_verification_code_uniqueness() {
        let codes: Vec<_> = (0..100).map(|_| generate_verification_code()).collect();
        // Not all codes should be the same (statistically very unlikely)
        let unique: std::collections::HashSet<_> = codes.iter().collect();
        assert!(unique.len() > 1);
    }

    #[test]
    fn test_handshake_client_creation() {
        let client = HandshakeClient::new("Test Client");
        assert_eq!(client.device_name, "Test Client");
    }

    #[test]
    fn test_pairing_result() {
        let result = PairingResult {
            server_name: "Server".to_string(),
            ssh_user: "user".to_string(),
            verification_code: Some("1234".to_string()),
        };

        assert_eq!(result.server_name, "Server");
        assert_eq!(result.ssh_user, "user");
        assert_eq!(result.verification_code, Some("1234".to_string()));
    }

    #[tokio::test]
    async fn test_handshake_server_creation() {
        let temp_dir = TempDir::new().unwrap();
        let ssh_dir = temp_dir.path().join(".ssh");
        let key_manager = KeyManager::with_dir(ssh_dir);

        let server = HandshakeServer::new(key_manager, "Test Server");
        assert_eq!(server.device_name, "Test Server");
        assert!(!server.require_verification);
    }

    #[tokio::test]
    async fn test_handshake_server_with_verification() {
        let temp_dir = TempDir::new().unwrap();
        let ssh_dir = temp_dir.path().join(".ssh");
        let key_manager = KeyManager::with_dir(ssh_dir);

        let server = HandshakeServer::new(key_manager, "Test Server").with_verification(true);
        assert!(server.require_verification);
    }

    #[tokio::test]
    async fn test_handshake_server_listen() {
        let temp_dir = TempDir::new().unwrap();
        let ssh_dir = temp_dir.path().join(".ssh");
        let key_manager = KeyManager::with_dir(ssh_dir);

        let mut server = HandshakeServer::new(key_manager, "Test Server");
        let addr = server.listen(0).await.unwrap(); // Port 0 = random available port

        assert!(addr.port() > 0);
    }

    #[tokio::test]
    async fn test_full_handshake() {
        use crate::keys::{KeyAlgorithm, SshKeyPair};

        let temp_dir = TempDir::new().unwrap();
        let ssh_dir = temp_dir.path().join(".ssh");
        let key_manager = KeyManager::with_dir(ssh_dir.clone());

        // Start server
        let mut server = HandshakeServer::new(key_manager, "Test Server");
        let addr = server.listen(0).await.unwrap();
        let server_addr = format!("127.0.0.1:{}", addr.port());

        let (event_tx, mut event_rx) = mpsc::channel(10);

        // Run server in background
        let server_handle = tokio::spawn(async move {
            server.handle_one(event_tx).await
        });

        // Give server time to start
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        // Create client and key pair
        let client = HandshakeClient::new("Test Client");
        let key_pair = SshKeyPair::generate(KeyAlgorithm::Ed25519, "test@connecto").unwrap();

        // Perform pairing
        let result = client.pair(&server_addr, &key_pair).await.unwrap();

        assert_eq!(result.server_name, "Test Server");
        assert!(!result.ssh_user.is_empty());

        // Verify key was added
        let key_manager = KeyManager::with_dir(ssh_dir);
        let keys = key_manager.list_authorized_keys().unwrap();
        assert_eq!(keys.len(), 1);
        assert!(keys[0].contains("test@connecto"));

        // Wait for server to finish
        server_handle.await.unwrap().unwrap();

        // Verify events were sent
        let mut events = Vec::new();
        while let Ok(event) = event_rx.try_recv() {
            events.push(event);
        }
        assert!(!events.is_empty());
    }
}
