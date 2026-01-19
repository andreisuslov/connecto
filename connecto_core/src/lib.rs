//! Connecto Core Library
//!
//! This crate provides the core functionality for the Connecto application,
//! including mDNS-based device discovery, SSH key management, and the
//! pairing protocol for secure key exchange.
//!
//! # Architecture
//!
//! The library is organized into three main modules:
//!
//! - [`discovery`]: mDNS-based device discovery using the `mdns-sd` crate
//! - [`keys`]: SSH key generation, parsing, and management
//! - [`protocol`]: The handshake protocol for secure key exchange
//!
//! # Example
//!
//! ```no_run
//! use connecto_core::{
//!     discovery::{ServiceAdvertiser, ServiceBrowser, DEFAULT_PORT},
//!     keys::{KeyAlgorithm, KeyManager, SshKeyPair},
//!     protocol::{HandshakeClient, HandshakeServer},
//! };
//!
//! // Server side: advertise and listen for pairing requests
//! async fn run_server() -> connecto_core::Result<()> {
//!     let key_manager = KeyManager::new()?;
//!     let mut server = HandshakeServer::new(key_manager, "My Device");
//!     let addr = server.listen(DEFAULT_PORT).await?;
//!     println!("Listening on {}", addr);
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

pub mod discovery;
pub mod error;
pub mod keys;
pub mod protocol;

// Re-export commonly used types
pub use discovery::{DiscoveredDevice, DiscoveryEvent, ServiceAdvertiser, ServiceBrowser, SubnetScanner, DEFAULT_PORT, SERVICE_TYPE};
pub use error::{ConnectoError, Result};
pub use keys::{KeyAlgorithm, KeyManager, SshKeyPair};
pub use protocol::{HandshakeClient, HandshakeServer, Message, PairingResult, ServerEvent, PROTOCOL_VERSION};

/// Get the version of the connecto_core library
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Get the hostname of this device
pub fn hostname() -> String {
    discovery::get_hostname()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version() {
        let v = version();
        assert!(!v.is_empty());
    }

    #[test]
    fn test_hostname() {
        let h = hostname();
        assert!(!h.is_empty());
    }

    #[test]
    fn test_re_exports() {
        // Verify that re-exports work
        let _ = DEFAULT_PORT;
        let _ = SERVICE_TYPE;
        let _ = PROTOCOL_VERSION;
        let _ = KeyAlgorithm::default();
    }
}
