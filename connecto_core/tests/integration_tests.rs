//! Integration tests for Connecto Core
//!
//! These tests verify that the different components work together correctly.

use connecto_core::{
    keys::{KeyAlgorithm, KeyManager, SshKeyPair},
    protocol::{
        verification_code_for_key, ApprovalCallback, HandshakeClient, HandshakeServer, Message,
        PairingRequest, ServerEvent,
    },
    ServiceAdvertiser, ServiceBrowser, SubnetScanner,
};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tempfile::TempDir;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

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

/// Start a one-shot handshake server on a random port for negative-path tests
///
/// Returns the server's ssh dir, its address, the server task handle, and the
/// TempDir guard. The task keeps accepting until a pairing SUCCEEDS, so tests
/// that only exercise failure paths must abort the handle when done.
async fn start_test_server(
    approval: Option<ApprovalCallback>,
) -> (
    PathBuf,
    String,
    JoinHandle<connecto_core::Result<()>>,
    TempDir,
) {
    let temp_dir = TempDir::new().unwrap();
    let ssh_dir = temp_dir.path().join(".ssh");
    let key_manager = KeyManager::with_dir(ssh_dir.clone());

    let mut server = HandshakeServer::new(key_manager, "Test Server");
    if let Some(callback) = approval {
        server = server.with_approval(callback);
    }
    let addr = server.listen(0).await.unwrap();

    let (event_tx, _event_rx) = mpsc::channel(32);
    let handle = tokio::spawn(async move { server.handle_one(event_tx).await });

    (
        ssh_dir,
        format!("127.0.0.1:{}", addr.port()),
        handle,
        temp_dir,
    )
}

/// Test protocol version rejection against a REAL running server
#[tokio::test]
async fn test_protocol_version_mismatch() {
    let (ssh_dir, addr, server, _guard) = start_test_server(None).await;

    let stream = TcpStream::connect(&addr).await.unwrap();
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);

    let hello = Message::Hello {
        version: 999, // Unsupported version
        device_name: "Bad Client".to_string(),
    };
    writer
        .write_all(hello.to_json().unwrap().as_bytes())
        .await
        .unwrap();

    // The server must reply with a protocol Error message
    let mut line = String::new();
    tokio::time::timeout(Duration::from_secs(5), reader.read_line(&mut line))
        .await
        .expect("server did not reply to bad version")
        .unwrap();

    match Message::from_json(&line).unwrap() {
        Message::Error { code, message } => {
            assert_eq!(code, 1);
            assert!(
                message.contains("version"),
                "unexpected reason: {}",
                message
            );
        }
        other => panic!("Expected Error, got {:?}", other),
    }

    server.abort();
    let keys = KeyManager::with_dir(ssh_dir)
        .list_authorized_keys()
        .unwrap();
    assert!(keys.is_empty());
}

/// A non-JSON line must result in a clean close, not a key installation
#[tokio::test]
async fn test_server_closes_connection_on_garbage_line() {
    let (ssh_dir, addr, server, _guard) = start_test_server(None).await;

    let mut stream = TcpStream::connect(&addr).await.unwrap();
    stream
        .write_all(b"this is definitely not json\n")
        .await
        .unwrap();

    // The server drops the connection; the read must terminate with EOF
    // (0 bytes) or a reset, never hang.
    let mut buf = [0u8; 64];
    let read = tokio::time::timeout(Duration::from_secs(5), stream.read(&mut buf))
        .await
        .expect("server kept the connection open after garbage input");
    // A connection reset (Err) is also a clean rejection
    if let Ok(n) = read {
        assert_eq!(n, 0, "server sent unexpected bytes: {:?}", &buf[..n]);
    }

    server.abort();
    let keys = KeyManager::with_dir(ssh_dir)
        .list_authorized_keys()
        .unwrap();
    assert!(keys.is_empty());
}

/// A line larger than the 64 KiB frame cap must be rejected
#[tokio::test]
async fn test_server_rejects_oversized_message() {
    let (ssh_dir, addr, server, _guard) = start_test_server(None).await;

    let mut stream = TcpStream::connect(&addr).await.unwrap();
    // 64 KiB + slack of 'a' bytes, no newline until the end. The server stops
    // reading at the cap and drops the connection, so the tail writes may
    // fail with EPIPE/ECONNRESET - that is the expected rejection.
    let oversized = vec![b'a'; 64 * 1024 + 512];
    let _ = stream.write_all(&oversized).await;
    let _ = stream.write_all(b"\n").await;

    let mut buf = [0u8; 64];
    let read = tokio::time::timeout(Duration::from_secs(5), stream.read(&mut buf))
        .await
        .expect("server kept the connection open after oversized input");
    // A connection reset (Err) is also a clean rejection
    if let Ok(n) = read {
        assert_eq!(n, 0, "server sent unexpected bytes: {:?}", &buf[..n]);
    }

    server.abort();
    let keys = KeyManager::with_dir(ssh_dir)
        .list_authorized_keys()
        .unwrap();
    assert!(keys.is_empty());
}

/// A denied approval must leave authorized_keys empty and fail the client
#[tokio::test]
async fn test_denied_approval_installs_no_key() {
    let (ssh_dir, addr, server, _guard) = start_test_server(Some(Box::new(|_request| false))).await;

    let key_pair = SshKeyPair::generate(KeyAlgorithm::Ed25519, "denied@test").unwrap();
    let client = HandshakeClient::new("Denied Client");

    let result = client.pair(&addr, &key_pair).await;
    match result {
        Err(e) => assert!(
            e.to_string().contains("rejected"),
            "unexpected error: {}",
            e
        ),
        Ok(_) => panic!("pairing must fail when approval is denied"),
    }

    server.abort();
    let keys = KeyManager::with_dir(ssh_dir)
        .list_authorized_keys()
        .unwrap();
    assert!(keys.is_empty(), "denied key must not be installed");
}

/// The approval callback sees the received key's fingerprint and the same
/// verification code the client derives locally
#[tokio::test]
async fn test_approval_callback_sees_matching_verification_code() {
    let seen: Arc<Mutex<Option<PairingRequest>>> = Arc::new(Mutex::new(None));
    let seen_in_callback = Arc::clone(&seen);

    let (ssh_dir, addr, server, _guard) = start_test_server(Some(Box::new(move |request| {
        *seen_in_callback.lock().unwrap() = Some(request.clone());
        true
    })))
    .await;

    let key_pair = SshKeyPair::generate(KeyAlgorithm::Ed25519, "approved@test").unwrap();
    let client = HandshakeClient::new("Approved Client");
    let result = client.pair(&addr, &key_pair).await.unwrap();

    // Server side completed (handle_one returns after a successful pairing)
    server.await.unwrap().unwrap();

    let request = seen.lock().unwrap().clone().expect("callback never ran");
    assert_eq!(request.device_name, "Approved Client");
    assert_eq!(request.key_comment, "approved@test");
    // The code shown on the listener matches the code shown on the client
    assert_eq!(request.verification_code, result.verification_code);
    assert_eq!(
        request.verification_code,
        verification_code_for_key(&key_pair.public_key).unwrap()
    );
    assert_eq!(
        request.fingerprint,
        connecto_core::fingerprint_sha256(&key_pair.public_key).unwrap()
    );

    let keys = KeyManager::with_dir(ssh_dir)
        .list_authorized_keys()
        .unwrap();
    assert_eq!(keys.len(), 1);
    assert!(keys[0].contains("approved@test"));
}

/// The SubnetScanner's TCP probe must discover a real HandshakeServer
///
/// Exercises the probe + identify_device path (TCP connect, Hello/HelloAck
/// exchange) against a live server on loopback - no mDNS required, so this
/// runs cross-platform and on CI.
#[tokio::test]
async fn test_subnet_scanner_discovers_loopback_listener() {
    let temp_dir = TempDir::new().unwrap();
    let key_manager = KeyManager::with_dir(temp_dir.path().join(".ssh"));

    let mut server = HandshakeServer::new(key_manager, "Scan Target");
    let port = server.listen(0).await.unwrap().port();

    // run() serves probes concurrently; it shuts down when the event channel
    // closes, so keep the receiver alive for the duration of the scan.
    let (event_tx, event_rx) = mpsc::channel(32);
    let server_task = tokio::spawn(async move { server.run(event_tx).await });

    // 127.0.0.0/30 expands to exactly 127.0.0.1 (our listener) and 127.0.0.2
    let scanner = SubnetScanner::new(port, Duration::from_secs(2));
    let devices = scanner.scan_subnets(&["127.0.0.0/30".to_string()]).await;

    let loopback: std::net::IpAddr = "127.0.0.1".parse().unwrap();
    let found = devices
        .iter()
        .find(|d| d.addresses.contains(&loopback))
        .unwrap_or_else(|| panic!("loopback listener not discovered, got: {:?}", devices));
    assert_eq!(found.name, "Scan Target");
    assert_eq!(found.port, port);

    drop(event_rx);
    server_task.abort();
}

/// mDNS advertise + browse round trip over the real network stack
///
/// Ignored by default: requires multicast-capable network; run manually with
/// --ignored. Multicast support on CI runners is unreliable, so opt-in is
/// deliberate.
///
/// KNOWN FAILURE (2026-06-12): this currently fails even on a working
/// network because ServiceAdvertiser::advertise registers the ServiceInfo
/// with no IP addresses and never calls enable_addr_auto(); mdns-sd 0.11
/// refuses to answer PTR queries for an address-less service
/// (dns_parser.rs add_answer_with_additionals bails when intf_addrs is
/// empty), so browsers never even see ServiceFound. This test is the
/// regression check for that production fix.
#[tokio::test]
#[ignore = "requires multicast-capable network; run manually with --ignored"]
async fn test_mdns_advertise_browse_roundtrip() {
    // Unique per-run instance name so a stale or concurrent advertisement on
    // the LAN cannot satisfy the assertion.
    let unique_name = format!("itest-mdns-{}", std::process::id());
    let port = 18099;

    let mut advertiser = ServiceAdvertiser::new().unwrap();
    advertiser.advertise(&unique_name, port).unwrap();

    let browser = ServiceBrowser::new().unwrap();
    let devices = browser
        .scan_for_duration(Duration::from_secs(5))
        .await
        .unwrap();

    let found = devices
        .iter()
        .find(|d| d.instance_name.contains(&unique_name))
        .unwrap_or_else(|| panic!("advertised instance not browsed, got: {:?}", devices));
    assert_eq!(found.port, port);
    assert!(!found.addresses.is_empty());

    advertiser.stop().unwrap();
}
