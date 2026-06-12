//! Connecto Core Library
//!
//! This crate provides the core functionality for the Connecto application,
//! including mDNS-based device discovery, SSH key management, and the
//! pairing protocol for secure key exchange.
//!
//! # Architecture
//!
//! The library is organized into the following modules:
//!
//! - [`discovery`]: mDNS-based device discovery (`mdns-sd`) plus direct
//!   subnet scanning for networks where mDNS is blocked
//! - [`keys`]: SSH key generation, parsing, fingerprints, and
//!   `authorized_keys` management
//! - [`protocol`]: The newline-delimited-JSON handshake protocol for key
//!   exchange (pairing), including verification-code derivation
//! - [`sync`]: Bidirectional key exchange between two devices, with
//!   priority-based arbitration
//! - [`ssh_config`]: The single parser/writer for connecto-managed
//!   `~/.ssh/config` entries (owns the marker constant)
//! - [`paths`]: Cross-platform home and SSH directory resolution
//! - [`device_name`]: Sanitization of device names into SSH host aliases
//! - [`user_config`]: Persistent user configuration (extra subnets, default key)
//! - [`fsutil`]: Atomic file writes
//! - [`fallback`]: Ad-hoc WiFi fallback with per-OS backends
//! - [`error`]: The crate-wide [`ConnectoError`] / [`Result`] types
//! - `bluetooth` (feature `bluetooth`): BLE discovery fallback
//!
//! # Example
//!
//! ```no_run
//! use connecto_core::{
//!     discovery::{ServiceAdvertiser, ServiceBrowser, DEFAULT_PORT},
//!     keys::{KeyAlgorithm, KeyManager, SshKeyPair},
//!     protocol::{HandshakeClient, HandshakeServer, ServerEvent},
//! };
//! use tokio::sync::mpsc;
//!
//! // Server side: advertise, bind, and SERVE pairing requests.
//! // `listen` only binds the socket — nothing is accepted until `run`
//! // (or `handle_one`) is awaited.
//! async fn run_server() -> connecto_core::Result<()> {
//!     let key_manager = KeyManager::new()?;
//!     let mut advertiser = ServiceAdvertiser::new()?;
//!     advertiser.advertise("My Device", DEFAULT_PORT)?;
//!
//!     let mut server = HandshakeServer::new(key_manager, "My Device");
//!     let addr = server.listen(DEFAULT_PORT).await?;
//!     println!("Listening on {}", addr);
//!
//!     let (event_tx, mut event_rx) = mpsc::channel(16);
//!     let events = tokio::spawn(async move {
//!         while let Some(event) = event_rx.recv().await {
//!             if let ServerEvent::PairingComplete { device_name } = event {
//!                 println!("Paired with {}", device_name);
//!             }
//!         }
//!     });
//!
//!     // Serves pairing requests until every event receiver is dropped.
//!     server.run(event_tx).await?;
//!     let _ = events.await;
//!     advertiser.stop()?;
//!     Ok(())
//! }
//!
//! // Client side: discover and pair with a device
//! async fn run_client() -> connecto_core::Result<()> {
//!     let browser = ServiceBrowser::new()?;
//!     let devices = browser.scan_for_duration(std::time::Duration::from_secs(5)).await?;
//!
//!     if let Some(device) = devices.first() {
//!         let key_pair = SshKeyPair::generate(KeyAlgorithm::Ed25519, "user@host")?;
//!         let client = HandshakeClient::new("My Laptop");
//!
//!         if let Some(addr) = device.connection_string() {
//!             let result = client.pair(&addr, &key_pair).await?;
//!             println!("Paired with {}!", result.server_name);
//!         }
//!     }
//!     Ok(())
//! }
//! ```

pub mod device_name;
pub mod discovery;
pub mod error;
pub mod fallback;
pub mod fsutil;
pub mod keys;
pub mod paths;
pub mod protocol;
pub mod ssh_config;
pub mod sync;
pub mod user_config;

#[cfg(feature = "bluetooth")]
pub mod bluetooth;

// Re-export commonly used types
pub use device_name::sanitize_device_name;
pub use discovery::{
    DiscoveredDevice, DiscoveryEvent, ServiceAdvertiser, ServiceBrowser, SubnetScanner,
    DEFAULT_PORT, SERVICE_TYPE,
};
pub use error::{ConnectoError, Result};
pub use keys::{fingerprint_sha256, KeyAlgorithm, KeyManager, SshKeyPair};
pub use protocol::{
    negotiate_version, verification_code_for_key, ApprovalCallback, HandshakeClient,
    HandshakeServer, Message, PairingRequest, PairingResult, ServerEvent, MIN_SUPPORTED_VERSION,
    PROTOCOL_VERSION,
};
pub use ssh_config::{AddOutcome, HostEntry, SshConfig, CONNECTO_MARKER};
pub use sync::{SyncEvent, SyncHandler, SyncResult, DEFAULT_SYNC_TIMEOUT_SECS, SYNC_SERVICE_TYPE};
pub use user_config::Config;

#[cfg(feature = "bluetooth")]
pub use bluetooth::{
    BluetoothAdvertiser, BluetoothBrowser, BluetoothDevice, BluetoothEvent, BLE_CHAR_DEVICE_INFO,
    BLE_SERVICE_UUID,
};
