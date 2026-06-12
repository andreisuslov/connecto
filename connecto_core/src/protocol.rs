//! Handshake protocol for Connecto pairing
//!
//! Defines the protocol for exchanging SSH keys between devices

use crate::error::{ConnectoError, Result};
use crate::keys::{fingerprint_sha256, KeyManager, SshKeyPair};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{
    AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader,
};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, Semaphore};
use tokio::task::JoinSet;
use tracing::{debug, error, info, warn};

/// Current protocol version.
///
/// # Version policy
///
/// A peer is accepted when its advertised version lies in
/// [`MIN_SUPPORTED_VERSION`]`..=`[`PROTOCOL_VERSION`]. We always reply with
/// our own version, so after the hello exchange both sides operate at
/// `min(theirs, ours)`. Versions outside the supported range (older than
/// [`MIN_SUPPORTED_VERSION`] or newer than this build) are rejected with a
/// [`Message::Error`]; a newer peer is expected to retry at a version we
/// support. The check is centralized in [`negotiate_version`] and used by the
/// handshake server, the handshake client, and both sync roles.
pub const PROTOCOL_VERSION: u32 = 1;

/// Oldest peer protocol version this build still accepts
pub const MIN_SUPPORTED_VERSION: u32 = 1;

/// Maximum number of pairing handshakes [`HandshakeServer::run`] serves concurrently
const MAX_CONCURRENT_HANDSHAKES: usize = 8;

/// Validate a peer's protocol version against the supported range
///
/// Returns the effective version to operate at (`min(peer, ours)`), or a
/// human-readable rejection reason. See [`PROTOCOL_VERSION`] for the policy.
pub fn negotiate_version(peer_version: u32) -> std::result::Result<u32, String> {
    if (MIN_SUPPORTED_VERSION..=PROTOCOL_VERSION).contains(&peer_version) {
        Ok(peer_version.min(PROTOCOL_VERSION))
    } else {
        Err(format!(
            "Unsupported protocol version {} (supported: {} through {})",
            peer_version, MIN_SUPPORTED_VERSION, PROTOCOL_VERSION
        ))
    }
}

/// Derive the 6-digit pairing verification code for a public key.
///
/// Derivation: parse the OpenSSH public key, compute its SHA-256 fingerprint,
/// interpret the first four bytes of the raw digest as a big-endian `u32`,
/// reduce it modulo 1,000,000, and zero-pad to six digits.
///
/// Both sides derive the code independently from key material rather than
/// trusting a value sent over the wire: the listening side derives it from
/// the key it RECEIVED in [`Message::KeyExchange`], while the pairing side
/// derives it from the key it SENT. The codes match only if the same key
/// crossed the wire, so a man-in-the-middle that substitutes its own key
/// changes the code displayed on the listening side.
pub fn verification_code_for_key(public_key_openssh: &str) -> Result<String> {
    let public_key = SshKeyPair::parse_public_key(public_key_openssh)?;
    let digest = public_key.fingerprint(ssh_key::HashAlg::Sha256);
    let bytes = digest.as_bytes();
    let n = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    Ok(format!("{:06}", n % 1_000_000))
}

/// Details of an incoming pairing request, handed to the [`ApprovalCallback`]
#[derive(Debug, Clone)]
pub struct PairingRequest {
    /// Name the connecting device announced in its `Hello`
    pub device_name: String,
    /// Comment field of the received public key
    pub key_comment: String,
    /// SHA-256 fingerprint of the received public key
    pub fingerprint: String,
    /// Code derived from the received key; see [`verification_code_for_key`]
    pub verification_code: String,
}

/// Callback consulted before a received key is installed.
///
/// Returning `false` rejects the pairing: the connecting party gets a
/// [`Message::Error`] and the key is NOT installed. The callback may block
/// (e.g. an interactive confirmation prompt); the server invokes it via
/// `tokio::task::spawn_blocking`, so it never blocks the async runtime.
pub type ApprovalCallback = Box<dyn Fn(&PairingRequest) -> bool + Send + Sync>;

/// Maximum length in bytes of a single newline-delimited protocol message
pub(crate) const MAX_MESSAGE_LEN: u64 = 64 * 1024;

/// Read timeout applied to handshake and sync protocol messages
pub(crate) const HANDSHAKE_READ_TIMEOUT: Duration = Duration::from_secs(30);

/// Write a [`Message`] as newline-delimited JSON and flush the writer
pub(crate) async fn write_message<W: AsyncWrite + Unpin>(
    writer: &mut W,
    msg: &Message,
) -> Result<()> {
    writer.write_all(msg.to_json()?.as_bytes()).await?;
    writer.flush().await?;
    Ok(())
}

/// Read a single newline-delimited [`Message`] from the reader
///
/// The read is bounded both in time (`timeout`, mapped to
/// [`ConnectoError::Timeout`]) and in size ([`MAX_MESSAGE_LEN`], mapped to
/// [`ConnectoError::Protocol`]) so a misbehaving peer can neither park the
/// connection forever nor make us buffer an unbounded line. EOF before a
/// complete message maps to [`ConnectoError::Network`].
pub(crate) async fn read_message<R: AsyncBufRead + Unpin>(
    reader: &mut R,
    timeout: Duration,
) -> Result<Message> {
    let mut line = String::new();
    let bytes_read = tokio::time::timeout(timeout, async {
        // Cap how much a single read_line may buffer; the +1 lets a line of
        // exactly MAX_MESSAGE_LEN bytes (including the newline) through while
        // still detecting longer ones.
        let mut limited = reader.take(MAX_MESSAGE_LEN + 1);
        limited.read_line(&mut line).await
    })
    .await
    .map_err(|_| ConnectoError::Timeout("Timed out waiting for message".to_string()))?
    .map_err(|e| ConnectoError::Network(format!("Failed to read message: {}", e)))?;

    if bytes_read == 0 {
        return Err(ConnectoError::Network(
            "Connection closed by peer".to_string(),
        ));
    }
    if bytes_read as u64 > MAX_MESSAGE_LEN {
        return Err(ConnectoError::Protocol(format!(
            "Message exceeds maximum length of {} bytes",
            MAX_MESSAGE_LEN
        )));
    }

    Message::from_json(&line)
}

/// Message types in the handshake protocol
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Message {
    /// Initial hello from client
    Hello { version: u32, device_name: String },

    /// Server acknowledges hello
    HelloAck {
        version: u32,
        device_name: String,
        verification_code: Option<String>,
    },

    /// Client sends its public key
    KeyExchange { public_key: String, comment: String },

    /// Server acknowledges key received and installed
    KeyAccepted { message: String },

    /// Error occurred
    Error { code: u32, message: String },

    /// Pairing complete
    PairingComplete { ssh_user: String },

    // Sync protocol messages (bidirectional pairing)
    /// Initial sync hello with priority and key
    SyncHello {
        version: u32,
        device_name: String,
        initiator_priority: u64,
        public_key: String,
        key_comment: String,
        ssh_user: String,
    },

    /// Sync hello acknowledgment with key
    SyncHelloAck {
        version: u32,
        device_name: String,
        public_key: String,
        key_comment: String,
        ssh_user: String,
        accept_sync: bool,
    },

    /// Sync complete confirmation
    SyncComplete { success: bool, message: String },
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
    Started {
        address: SocketAddr,
    },
    ClientConnected {
        address: SocketAddr,
    },
    PairingRequest {
        device_name: String,
        address: SocketAddr,
    },
    KeyReceived {
        comment: String,
        /// SHA-256 fingerprint of the received public key
        fingerprint: String,
        /// Code derived from the received key; see [`verification_code_for_key`]
        verification_code: String,
    },
    PairingComplete {
        device_name: String,
    },
    Error {
        message: String,
    },
}

/// Handshake server that listens for pairing requests
pub struct HandshakeServer {
    listener: Option<TcpListener>,
    key_manager: Arc<KeyManager>,
    device_name: String,
    approval: Option<Arc<ApprovalCallback>>,
}

impl HandshakeServer {
    /// Create a new handshake server
    ///
    /// Without an [`ApprovalCallback`] (see [`Self::with_approval`]) every
    /// well-formed pairing request is auto-accepted.
    pub fn new(key_manager: KeyManager, device_name: &str) -> Self {
        Self {
            listener: None,
            key_manager: Arc::new(key_manager),
            device_name: device_name.to_string(),
            approval: None,
        }
    }

    /// Gate key installation behind an approval callback
    ///
    /// The callback runs after the peer's key has been received and before it
    /// is installed; see [`ApprovalCallback`] for the contract.
    pub fn with_approval(mut self, callback: ApprovalCallback) -> Self {
        self.approval = Some(Arc::new(callback));
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
    ///
    /// Per-client handshakes run as tasks tracked in a [`JoinSet`], with at
    /// most [`MAX_CONCURRENT_HANDSHAKES`] in flight; excess connections are
    /// dropped. The loop exits when the event channel is closed (every
    /// receiver dropped), after which in-flight handshakes are drained with a
    /// short timeout.
    pub async fn run(&mut self, event_tx: mpsc::Sender<ServerEvent>) -> Result<()> {
        let listener = self
            .listener
            .take()
            .ok_or_else(|| ConnectoError::Network("Server not started".to_string()))?;

        let addr = listener.local_addr()?;
        let _ = event_tx.send(ServerEvent::Started { address: addr }).await;

        let mut handshakes: JoinSet<()> = JoinSet::new();
        let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT_HANDSHAKES));

        loop {
            // Reap finished handshake tasks so the set does not grow unboundedly
            while handshakes.try_join_next().is_some() {}

            tokio::select! {
                _ = event_tx.closed() => {
                    debug!("Event channel closed; shutting handshake server down");
                    break;
                }
                accepted = listener.accept() => match accepted {
                    Ok((stream, peer_addr)) => {
                        let Ok(permit) = Arc::clone(&semaphore).try_acquire_owned() else {
                            warn!(
                                "Too many concurrent handshakes; dropping connection from {}",
                                peer_addr
                            );
                            continue;
                        };

                        info!("Client connected from {}", peer_addr);
                        let _ = event_tx
                            .send(ServerEvent::ClientConnected { address: peer_addr })
                            .await;

                        let key_manager = Arc::clone(&self.key_manager);
                        let device_name = self.device_name.clone();
                        let approval = self.approval.clone();
                        let event_tx = event_tx.clone();

                        handshakes.spawn(async move {
                            let _permit = permit;
                            if let Err(e) = handle_client(
                                stream,
                                peer_addr,
                                key_manager,
                                device_name,
                                approval,
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

        // Give in-flight handshakes a short grace period, then abort them
        let drain = async { while handshakes.join_next().await.is_some() {} };
        if tokio::time::timeout(Duration::from_secs(5), drain)
            .await
            .is_err()
        {
            warn!("Timed out draining in-flight handshakes; aborting them");
            handshakes.shutdown().await;
        }

        Ok(())
    }

    /// Handle a single pairing request (useful for one-shot mode)
    /// Keeps accepting connections until one successfully completes pairing.
    /// This prevents incomplete handshakes (like from scanners) from consuming the session.
    pub async fn handle_one(&mut self, event_tx: mpsc::Sender<ServerEvent>) -> Result<()> {
        let listener = self
            .listener
            .take()
            .ok_or_else(|| ConnectoError::Network("Server not started".to_string()))?;

        let addr = listener.local_addr()?;
        let _ = event_tx.send(ServerEvent::Started { address: addr }).await;

        loop {
            let (stream, peer_addr) = listener.accept().await?;
            info!("Client connected from {}", peer_addr);
            let _ = event_tx
                .send(ServerEvent::ClientConnected { address: peer_addr })
                .await;

            match handle_client(
                stream,
                peer_addr,
                Arc::clone(&self.key_manager),
                self.device_name.clone(),
                self.approval.clone(),
                event_tx.clone(),
            )
            .await
            {
                Ok(()) => {
                    // Successful pairing, exit the loop
                    return Ok(());
                }
                Err(e) => {
                    // Failed handshake (e.g., scanner probe, incomplete connection)
                    // This is expected behavior - scanners probe to identify devices
                    // Log at debug level and continue waiting for real pairing requests
                    debug!(
                        "Incomplete handshake from {} (likely scanner probe): {}",
                        peer_addr, e
                    );
                }
            }
        }
    }
}

async fn handle_client(
    stream: TcpStream,
    peer_addr: SocketAddr,
    key_manager: Arc<KeyManager>,
    device_name: String,
    approval: Option<Arc<ApprovalCallback>>,
    event_tx: mpsc::Sender<ServerEvent>,
) -> Result<()> {
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);

    // Read Hello message
    let hello = read_message(&mut reader, HANDSHAKE_READ_TIMEOUT).await?;

    let client_name = match hello {
        Message::Hello {
            version,
            device_name: client_name,
        } => {
            if let Err(reason) = negotiate_version(version) {
                let error_msg = Message::Error {
                    code: 1,
                    message: reason.clone(),
                };
                write_message(&mut writer, &error_msg).await?;
                return Err(ConnectoError::Handshake(reason));
            }
            client_name
        }
        _ => {
            let error_msg = Message::Error {
                code: 2,
                message: "Expected Hello message".to_string(),
            };
            write_message(&mut writer, &error_msg).await?;
            return Err(ConnectoError::Handshake("Expected Hello".to_string()));
        }
    };

    let _ = event_tx
        .send(ServerEvent::PairingRequest {
            device_name: client_name.clone(),
            address: peer_addr,
        })
        .await;

    // Send HelloAck. The legacy `verification_code` field stays in the wire
    // format for compatibility but no longer carries a value: a code invented
    // here and sent to the connecting party proves nothing, since the peer
    // would merely echo back what we told it. The verification code is now
    // derived from the key material itself once KeyExchange arrives; see
    // [`verification_code_for_key`].
    let hello_ack = Message::HelloAck {
        version: PROTOCOL_VERSION,
        device_name: device_name.clone(),
        verification_code: None,
    };
    write_message(&mut writer, &hello_ack).await?;

    // Read KeyExchange. The timeout/EOF handling inside read_message covers
    // scanner probes that disconnect (or go silent) after HelloAck.
    let key_exchange = read_message(&mut reader, HANDSHAKE_READ_TIMEOUT).await?;

    match key_exchange {
        Message::KeyExchange {
            public_key,
            comment,
        } => {
            debug!("Received public key with comment: {}", comment);

            // Validate the key and derive its identity material before
            // anything is installed.
            let fingerprint = match fingerprint_sha256(&public_key) {
                Ok(fingerprint) => fingerprint,
                Err(e) => {
                    let reason = format!("Invalid public key: {}", e);
                    let error_msg = Message::Error {
                        code: 5,
                        message: reason.clone(),
                    };
                    write_message(&mut writer, &error_msg).await?;
                    return Err(ConnectoError::Handshake(reason));
                }
            };
            let verification_code = verification_code_for_key(&public_key)?;

            let _ = event_tx
                .send(ServerEvent::KeyReceived {
                    comment: comment.clone(),
                    fingerprint: fingerprint.clone(),
                    verification_code: verification_code.clone(),
                })
                .await;

            // Consult the approval callback (if any) BEFORE installing the
            // key. The callback may block on user input, so it runs on the
            // blocking thread pool.
            let approved = match &approval {
                None => true,
                Some(callback) => {
                    let callback = Arc::clone(callback);
                    let request = PairingRequest {
                        device_name: client_name.clone(),
                        key_comment: comment.clone(),
                        fingerprint: fingerprint.clone(),
                        verification_code: verification_code.clone(),
                    };
                    tokio::task::spawn_blocking(move || callback(&request))
                        .await
                        .map_err(|e| {
                            ConnectoError::Handshake(format!("Approval callback failed: {}", e))
                        })?
                }
            };

            if !approved {
                info!("Pairing with {} rejected by approval callback", client_name);
                let error_msg = Message::Error {
                    code: 4,
                    message: "Pairing rejected by user".to_string(),
                };
                write_message(&mut writer, &error_msg).await?;
                return Err(ConnectoError::Handshake(
                    "Pairing rejected by user".to_string(),
                ));
            }

            // Install the key. Every failure after this point must roll the
            // installation back so a half-completed exchange does not leave
            // the peer with SSH access.
            key_manager.add_authorized_key(&public_key)?;

            let post_install = async {
                let accepted = Message::KeyAccepted {
                    message: "Key added to authorized_keys".to_string(),
                };
                write_message(&mut writer, &accepted).await?;

                // Get current user (USER on Unix, USERNAME on Windows)
                let ssh_user = std::env::var("USER")
                    .or_else(|_| std::env::var("USERNAME"))
                    .unwrap_or_else(|_| "unknown".to_string());

                let complete = Message::PairingComplete { ssh_user };
                write_message(&mut writer, &complete).await?;
                Ok::<(), ConnectoError>(())
            };

            if let Err(e) = post_install.await {
                warn!(
                    "Pairing with {} failed after key install; rolling back: {}",
                    client_name, e
                );
                match key_manager.remove_authorized_key(&public_key) {
                    Ok(true) => info!("Rolled back key installed for {}", client_name),
                    Ok(false) => warn!("Rollback found no matching key in authorized_keys"),
                    Err(rollback_err) => warn!(
                        "Rollback failed; key for {} remains installed: {}",
                        client_name, rollback_err
                    ),
                }
                return Err(e);
            }

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
            write_message(&mut writer, &error_msg).await?;
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
    ///
    /// The returned [`PairingResult`] carries the verification code derived
    /// from OUR public key (the key we sent); display it so the user can
    /// compare it with the code shown on the listening device.
    pub async fn pair(&self, address: &str, key_pair: &SshKeyPair) -> Result<PairingResult> {
        // Derived locally from the key we are about to send; the server
        // derives the same code from what it receives.
        let verification_code = verification_code_for_key(&key_pair.public_key)?;

        let stream = TcpStream::connect(address)
            .await
            .map_err(|e| ConnectoError::Network(format!("Failed to connect: {}", e)))?;

        let (reader, mut writer) = stream.into_split();
        let mut reader = BufReader::new(reader);

        // Send Hello
        let hello = Message::Hello {
            version: PROTOCOL_VERSION,
            device_name: self.device_name.clone(),
        };
        write_message(&mut writer, &hello).await?;

        // Read HelloAck
        let hello_ack = read_message(&mut reader, HANDSHAKE_READ_TIMEOUT).await?;

        let server_name = match hello_ack {
            Message::HelloAck {
                version,
                device_name,
                // Legacy field: v1 servers sent a server-generated code here,
                // which proved nothing. Ignored; see verification_code_for_key.
                verification_code: _,
            } => {
                negotiate_version(version).map_err(ConnectoError::Handshake)?;
                device_name
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
        write_message(&mut writer, &key_exchange).await?;

        // Read KeyAccepted
        let accepted = read_message(&mut reader, HANDSHAKE_READ_TIMEOUT).await?;

        match accepted {
            Message::KeyAccepted { .. } => {}
            Message::Error { message, .. } => {
                return Err(ConnectoError::Handshake(message));
            }
            _ => {
                return Err(ConnectoError::Handshake("Expected KeyAccepted".to_string()));
            }
        }

        // Read PairingComplete
        let complete = read_message(&mut reader, HANDSHAKE_READ_TIMEOUT).await?;

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
    /// Code derived from OUR public key (see [`verification_code_for_key`]);
    /// the user should compare it with the code shown on the listening device.
    pub verification_code: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_negotiate_version_accepts_supported_range() {
        for version in MIN_SUPPORTED_VERSION..=PROTOCOL_VERSION {
            assert_eq!(negotiate_version(version), Ok(version));
        }
    }

    #[test]
    fn test_negotiate_version_rejects_out_of_range() {
        assert!(negotiate_version(0).is_err());
        assert!(negotiate_version(PROTOCOL_VERSION + 1).is_err());
        assert!(negotiate_version(999).is_err());
    }

    #[test]
    fn test_verification_code_format_and_determinism() {
        use crate::keys::KeyAlgorithm;

        let key_pair = SshKeyPair::generate(KeyAlgorithm::Ed25519, "code@test").unwrap();
        let code = verification_code_for_key(&key_pair.public_key).unwrap();

        assert_eq!(code.len(), 6);
        assert!(code.chars().all(|c| c.is_ascii_digit()));
        // Deterministic for the same key material
        assert_eq!(
            code,
            verification_code_for_key(&key_pair.public_key).unwrap()
        );
    }

    #[test]
    fn test_verification_code_known_fixture() {
        // Same fixture as keys::tests::test_fingerprint_sha256_known_fixture;
        // pins the derivation (first 4 digest bytes as BE u32, mod 1e6).
        let public_key =
            "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOe+ZEWqatUUD1EaBS1DRn/J0hKQPSiW0fLQ7a7Af96S fixture@connecto";
        let code = verification_code_for_key(public_key).unwrap();
        assert_eq!(code, "627765");
    }

    #[test]
    fn test_verification_code_rejects_invalid_key() {
        assert!(verification_code_for_key("not-a-key").is_err());
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

    #[tokio::test]
    async fn test_handshake_server_creation() {
        let temp_dir = TempDir::new().unwrap();
        let ssh_dir = temp_dir.path().join(".ssh");
        let key_manager = KeyManager::with_dir(ssh_dir);

        let server = HandshakeServer::new(key_manager, "Test Server");
        assert_eq!(server.device_name, "Test Server");
        assert!(server.approval.is_none());
    }

    #[tokio::test]
    async fn test_handshake_server_with_approval() {
        let temp_dir = TempDir::new().unwrap();
        let ssh_dir = temp_dir.path().join(".ssh");
        let key_manager = KeyManager::with_dir(ssh_dir);

        let server =
            HandshakeServer::new(key_manager, "Test Server").with_approval(Box::new(|_| true));
        assert!(server.approval.is_some());
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
        let server_handle = tokio::spawn(async move { server.handle_one(event_tx).await });

        // Give server time to start
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        // Create client and key pair
        let client = HandshakeClient::new("Test Client");
        let key_pair = SshKeyPair::generate(KeyAlgorithm::Ed25519, "test@connecto").unwrap();

        // Perform pairing
        let result = client.pair(&server_addr, &key_pair).await.unwrap();

        assert_eq!(result.server_name, "Test Server");
        assert!(!result.ssh_user.is_empty());
        // The client-side code is derived from the key it sent
        assert_eq!(
            result.verification_code,
            verification_code_for_key(&key_pair.public_key).unwrap()
        );

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

    #[tokio::test]
    async fn test_run_exits_when_event_channel_closes() {
        let temp_dir = TempDir::new().unwrap();
        let ssh_dir = temp_dir.path().join(".ssh");
        let key_manager = KeyManager::with_dir(ssh_dir);

        let mut server = HandshakeServer::new(key_manager, "Test Server");
        server.listen(0).await.unwrap();

        let (event_tx, event_rx) = mpsc::channel(10);
        let server_handle = tokio::spawn(async move { server.run(event_tx).await });

        // Dropping the only receiver must shut the server loop down
        drop(event_rx);

        let result = tokio::time::timeout(Duration::from_secs(5), server_handle)
            .await
            .expect("server.run did not exit after event channel closed")
            .unwrap();
        assert!(result.is_ok());
    }

    // Framing helper tests

    #[tokio::test]
    async fn test_write_read_message_roundtrip() {
        let (client, mut server) = tokio::io::duplex(1024);

        let msg = Message::Hello {
            version: PROTOCOL_VERSION,
            device_name: "Round Trip".to_string(),
        };
        write_message(&mut server, &msg).await.unwrap();

        let mut reader = BufReader::new(client);
        let received = read_message(&mut reader, Duration::from_secs(1))
            .await
            .unwrap();

        match received {
            Message::Hello {
                version,
                device_name,
            } => {
                assert_eq!(version, PROTOCOL_VERSION);
                assert_eq!(device_name, "Round Trip");
            }
            _ => panic!("Wrong message type"),
        }
    }

    #[tokio::test]
    async fn test_read_message_timeout() {
        // The server side never sends anything, so the read must time out.
        let (client, _server) = tokio::io::duplex(64);
        let mut reader = BufReader::new(client);

        let result = read_message(&mut reader, Duration::from_millis(100)).await;
        assert!(matches!(result, Err(ConnectoError::Timeout(_))));
    }

    #[tokio::test]
    async fn test_read_message_rejects_oversized_line() {
        let (client, mut server) = tokio::io::duplex(1024);

        // Write the oversized line from a task; the duplex buffer is small so
        // the writer blocks until the reader consumes (or gives up).
        let writer = tokio::spawn(async move {
            let oversized = vec![b'a'; MAX_MESSAGE_LEN as usize + 100];
            let _ = server.write_all(&oversized).await;
            let _ = server.write_all(b"\n").await;
        });

        let mut reader = BufReader::new(client);
        let result = read_message(&mut reader, Duration::from_secs(5)).await;
        assert!(matches!(result, Err(ConnectoError::Protocol(_))));

        // Unblock the writer task by closing the read side before joining it.
        drop(reader);
        let _ = writer.await;
    }

    #[tokio::test]
    async fn test_read_message_eof() {
        let (client, server) = tokio::io::duplex(64);
        drop(server);

        let mut reader = BufReader::new(client);
        let result = read_message(&mut reader, Duration::from_secs(1)).await;
        assert!(matches!(result, Err(ConnectoError::Network(_))));
    }

    // Sync protocol message tests

    #[test]
    fn test_sync_hello_serialization() {
        let msg = Message::SyncHello {
            version: PROTOCOL_VERSION,
            device_name: "Device A".to_string(),
            initiator_priority: 12345678901234567890,
            public_key: "ssh-ed25519 AAAAC3... test@device-a".to_string(),
            key_comment: "test@device-a".to_string(),
            ssh_user: "alice".to_string(),
        };

        let json = msg.to_json().unwrap();
        assert!(json.contains("SyncHello"));
        assert!(json.contains("initiator_priority"));
        assert!(json.ends_with('\n'));

        let deserialized = Message::from_json(&json).unwrap();
        match deserialized {
            Message::SyncHello {
                version,
                device_name,
                initiator_priority,
                public_key,
                key_comment,
                ssh_user,
            } => {
                assert_eq!(version, PROTOCOL_VERSION);
                assert_eq!(device_name, "Device A");
                assert_eq!(initiator_priority, 12345678901234567890);
                assert!(public_key.contains("ssh-ed25519"));
                assert_eq!(key_comment, "test@device-a");
                assert_eq!(ssh_user, "alice");
            }
            _ => panic!("Wrong message type"),
        }
    }

    #[test]
    fn test_sync_hello_ack_serialization() {
        let msg = Message::SyncHelloAck {
            version: PROTOCOL_VERSION,
            device_name: "Device B".to_string(),
            public_key: "ssh-ed25519 AAAAC3... test@device-b".to_string(),
            key_comment: "test@device-b".to_string(),
            ssh_user: "bob".to_string(),
            accept_sync: true,
        };

        let json = msg.to_json().unwrap();
        assert!(json.contains("SyncHelloAck"));
        assert!(json.contains("accept_sync"));

        let deserialized = Message::from_json(&json).unwrap();
        match deserialized {
            Message::SyncHelloAck {
                version,
                device_name,
                public_key,
                key_comment,
                ssh_user,
                accept_sync,
            } => {
                assert_eq!(version, PROTOCOL_VERSION);
                assert_eq!(device_name, "Device B");
                assert!(public_key.contains("ssh-ed25519"));
                assert_eq!(key_comment, "test@device-b");
                assert_eq!(ssh_user, "bob");
                assert!(accept_sync);
            }
            _ => panic!("Wrong message type"),
        }
    }

    #[test]
    fn test_sync_hello_ack_rejection() {
        let msg = Message::SyncHelloAck {
            version: PROTOCOL_VERSION,
            device_name: "Device B".to_string(),
            public_key: "".to_string(),
            key_comment: "".to_string(),
            ssh_user: "".to_string(),
            accept_sync: false,
        };

        let json = msg.to_json().unwrap();
        let deserialized = Message::from_json(&json).unwrap();

        match deserialized {
            Message::SyncHelloAck { accept_sync, .. } => {
                assert!(!accept_sync);
            }
            _ => panic!("Wrong message type"),
        }
    }

    #[test]
    fn test_sync_complete_serialization() {
        let msg = Message::SyncComplete {
            success: true,
            message: "Sync completed successfully".to_string(),
        };

        let json = msg.to_json().unwrap();
        assert!(json.contains("SyncComplete"));

        let deserialized = Message::from_json(&json).unwrap();
        match deserialized {
            Message::SyncComplete { success, message } => {
                assert!(success);
                assert_eq!(message, "Sync completed successfully");
            }
            _ => panic!("Wrong message type"),
        }
    }

    #[test]
    fn test_sync_complete_failure() {
        let msg = Message::SyncComplete {
            success: false,
            message: "Key installation failed".to_string(),
        };

        let json = msg.to_json().unwrap();
        let deserialized = Message::from_json(&json).unwrap();

        match deserialized {
            Message::SyncComplete { success, message } => {
                assert!(!success);
                assert_eq!(message, "Key installation failed");
            }
            _ => panic!("Wrong message type"),
        }
    }
}
