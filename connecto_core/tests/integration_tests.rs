//! Integration tests for Connecto Core
//!
//! These tests verify that the different components work together correctly.

use connecto_core::{
    keys::{KeyAlgorithm, KeyManager, SshKeyPair},
    protocol::{HandshakeClient, HandshakeServer, Message, ServerEvent, PROTOCOL_VERSION},
    DEFAULT_PORT,
};
use std::time::Duration;
use tempfile::TempDir;
use tokio::sync::mpsc;

/// Test that we can generate keys and parse them back
#[test]
fn test_key_roundtrip() {
    // Generate Ed25519 key
    let key_pair = SshKeyPair::generate(KeyAlgorithm::Ed25519, "test@connecto").unwrap();

    // Parse the public key
    let parsed = SshKeyPair::parse_public_key(&key_pair.public_key).unwrap();

    // Verify the key type
    assert!(key_pair.public_key.starts_with("ssh-ed25519"));
    assert_eq!(parsed.algorithm().as_str(), "ssh-ed25519");
}

/// Test that keys can be saved and loaded from disk
#[test]
fn test_key_persistence() {
    let temp_dir = TempDir::new().unwrap();
    let ssh_dir = temp_dir.path().join(".ssh");

    let manager = KeyManager::with_dir(ssh_dir.clone());
    let key_pair = SshKeyPair::generate(KeyAlgorithm::Ed25519, "test@connecto").unwrap();

    // Save the key
    let (private_path, public_path) = manager.save_key_pair(&key_pair, "test_key").unwrap();

    // Verify files exist
    assert!(private_path.exists());
    assert!(public_path.exists());

    // Read and verify content
    let saved_private = std::fs::read_to_string(&private_path).unwrap();
    let saved_public = std::fs::read_to_string(&public_path).unwrap();

    assert_eq!(saved_private, key_pair.private_key);
    assert_eq!(saved_public, key_pair.public_key);
}

/// Test authorized_keys management
#[test]
fn test_authorized_keys_management() {
    let temp_dir = TempDir::new().unwrap();
    let ssh_dir = temp_dir.path().join(".ssh");

    let manager = KeyManager::with_dir(ssh_dir);

    // Generate some keys
    let key1 = SshKeyPair::generate(KeyAlgorithm::Ed25519, "user1@host").unwrap();
    let key2 = SshKeyPair::generate(KeyAlgorithm::Ed25519, "user2@host").unwrap();
    let key3 = SshKeyPair::generate(KeyAlgorithm::Ed25519, "user3@host").unwrap();

    // Add keys
    manager.add_authorized_key(&key1.public_key).unwrap();
    manager.add_authorized_key(&key2.public_key).unwrap();
    manager.add_authorized_key(&key3.public_key).unwrap();

    // Verify all keys are present
    let keys = manager.list_authorized_keys().unwrap();
    assert_eq!(keys.len(), 3);

    // Remove one key
    let removed = manager.remove_authorized_key(&key2.public_key).unwrap();
    assert!(removed);

    // Verify key was removed
    let keys = manager.list_authorized_keys().unwrap();
    assert_eq!(keys.len(), 2);
    assert!(!keys.iter().any(|k| k.contains("user2@host")));
}

/// Test the handshake protocol message serialization
#[test]
fn test_protocol_messages() {
    // Test Hello message
    let hello = Message::Hello {
        version: PROTOCOL_VERSION,
        device_name: "Test Device".to_string(),
    };

    let json = hello.to_json().unwrap();
    let parsed = Message::from_json(&json).unwrap();

    match parsed {
        Message::Hello {
            version,
            device_name,
        } => {
            assert_eq!(version, PROTOCOL_VERSION);
            assert_eq!(device_name, "Test Device");
        }
        _ => panic!("Expected Hello message"),
    }

    // Test HelloAck message
    let hello_ack = Message::HelloAck {
        version: PROTOCOL_VERSION,
        device_name: "Server".to_string(),
        verification_code: Some("1234".to_string()),
    };

    let json = hello_ack.to_json().unwrap();
    let parsed = Message::from_json(&json).unwrap();

    match parsed {
        Message::HelloAck {
            version,
            device_name,
            verification_code,
        } => {
            assert_eq!(version, PROTOCOL_VERSION);
            assert_eq!(device_name, "Server");
            assert_eq!(verification_code, Some("1234".to_string()));
        }
        _ => panic!("Expected HelloAck message"),
    }

    // Test KeyExchange message
    let key_exchange = Message::KeyExchange {
        public_key: "ssh-ed25519 AAAA... test@connecto".to_string(),
        comment: "test@connecto".to_string(),
    };

    let json = key_exchange.to_json().unwrap();
    let parsed = Message::from_json(&json).unwrap();

    match parsed {
        Message::KeyExchange {
            public_key,
            comment,
        } => {
            assert!(public_key.starts_with("ssh-ed25519"));
            assert_eq!(comment, "test@connecto");
        }
        _ => panic!("Expected KeyExchange message"),
    }
}

/// Test full pairing workflow
#[tokio::test]
async fn test_full_pairing_workflow() {
    let temp_dir = TempDir::new().unwrap();
    let server_ssh_dir = temp_dir.path().join("server/.ssh");

    // Set up server
    let server_key_manager = KeyManager::with_dir(server_ssh_dir.clone());
    let mut server = HandshakeServer::new(server_key_manager, "Test Server");
    let addr = server.listen(0).await.unwrap(); // Random port
    let server_addr = format!("127.0.0.1:{}", addr.port());

    let (event_tx, mut event_rx) = mpsc::channel(10);

    // Run server in background
    let server_task = tokio::spawn(async move { server.handle_one(event_tx).await });

    // Give server time to start
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Client side: generate key and pair
    let key_pair = SshKeyPair::generate(KeyAlgorithm::Ed25519, "client@test").unwrap();
    let client = HandshakeClient::new("Test Client");

    let result = client.pair(&server_addr, &key_pair).await.unwrap();

    // Verify result
    assert_eq!(result.server_name, "Test Server");
    assert!(!result.ssh_user.is_empty());

    // Wait for server to complete
    server_task.await.unwrap().unwrap();

    // Verify key was added to authorized_keys
    let server_manager = KeyManager::with_dir(server_ssh_dir);
    let authorized = server_manager.list_authorized_keys().unwrap();

    assert_eq!(authorized.len(), 1);
    assert!(authorized[0].contains("client@test"));

    // Verify events were emitted
    let mut events = Vec::new();
    while let Ok(event) = event_rx.try_recv() {
        events.push(event);
    }

    // Should have received: Started, ClientConnected, PairingRequest, KeyReceived, PairingComplete
    assert!(events
        .iter()
        .any(|e| matches!(e, ServerEvent::Started { .. })));
    assert!(events
        .iter()
        .any(|e| matches!(e, ServerEvent::ClientConnected { .. })));
    assert!(events
        .iter()
        .any(|e| matches!(e, ServerEvent::PairingComplete { .. })));
}

/// Test protocol version mismatch handling
#[tokio::test]
async fn test_protocol_version_mismatch() {
    // This tests that if we manually send a message with wrong version,
    // the server should reject it. For now, we just test message creation.

    let msg = Message::Hello {
        version: 999, // Invalid version
        device_name: "Bad Client".to_string(),
    };

    let json = msg.to_json().unwrap();
    let parsed: Message = Message::from_json(&json).unwrap();

    match parsed {
        Message::Hello { version, .. } => {
            assert_eq!(version, 999);
        }
        _ => panic!("Wrong message type"),
    }
}

/// Test error message handling
#[test]
fn test_error_message() {
    let error_msg = Message::Error {
        code: 42,
        message: "Something went wrong".to_string(),
    };

    let json = error_msg.to_json().unwrap();
    let parsed = Message::from_json(&json).unwrap();

    match parsed {
        Message::Error { code, message } => {
            assert_eq!(code, 42);
            assert_eq!(message, "Something went wrong");
        }
        _ => panic!("Expected Error message"),
    }
}

/// Test multiple key algorithms
#[test]
fn test_key_algorithms() {
    // Ed25519
    let ed_key = SshKeyPair::generate(KeyAlgorithm::Ed25519, "ed25519@test").unwrap();
    assert!(ed_key.public_key.starts_with("ssh-ed25519"));

    // RSA (this is slower but should work)
    let rsa_key = SshKeyPair::generate(KeyAlgorithm::Rsa4096, "rsa@test").unwrap();
    assert!(rsa_key.public_key.starts_with("ssh-rsa"));
}

/// Test key manager directory creation
#[test]
fn test_key_manager_creates_directory() {
    let temp_dir = TempDir::new().unwrap();
    let ssh_dir = temp_dir.path().join("deeply/nested/.ssh");

    // Directory shouldn't exist yet
    assert!(!ssh_dir.exists());

    let manager = KeyManager::with_dir(ssh_dir.clone());
    manager.ensure_ssh_dir().unwrap();

    // Now it should exist
    assert!(ssh_dir.exists());
}

/// Test discovery module data structures
#[test]
fn test_discovered_device() {
    use connecto_core::DiscoveredDevice;

    let device = DiscoveredDevice {
        name: "Test Device".to_string(),
        hostname: "test.local.".to_string(),
        addresses: vec![
            "192.168.1.100".parse().unwrap(),
            "10.0.0.50".parse().unwrap(),
        ],
        port: DEFAULT_PORT,
        instance_name: "test-instance".to_string(),
    };

    // Test primary address selection (should prefer first IPv4)
    let primary = device.primary_address().unwrap();
    assert!(primary.is_ipv4());

    // Test connection string
    let conn_str = device.connection_string().unwrap();
    assert!(conn_str.contains(":8099"));

    // Test serialization
    let json = serde_json::to_string(&device).unwrap();
    let deserialized: DiscoveredDevice = serde_json::from_str(&json).unwrap();
    assert_eq!(device, deserialized);
}
