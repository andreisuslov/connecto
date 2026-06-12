//! Tauri commands for the GUI

use connecto_core::{
    discovery::{
        get_hostname, get_local_addresses, DiscoveredDevice, ServiceAdvertiser, ServiceBrowser,
        SubnetScanner, DEFAULT_PORT,
    },
    keys::{fingerprint_sha256, KeyAlgorithm, KeyManager, SshKeyPair},
    paths,
    protocol::{HandshakeClient, HandshakeServer, ServerEvent},
    sanitize_device_name,
    ssh_config::{HostEntry, SshConfig},
    sync::{SyncEvent, SyncHandler},
    user_config::Config,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tauri::State;
use tokio::sync::{mpsc, Mutex};

use crate::state::{AppState, ListenerHandle, SyncStatus};

/// Device info for the frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub name: String,
    pub hostname: String,
    pub addresses: Vec<String>,
    pub port: u16,
    pub index: usize,
}

impl From<(usize, &DiscoveredDevice)> for DeviceInfo {
    fn from((index, device): (usize, &DiscoveredDevice)) -> Self {
        Self {
            name: device.name.clone(),
            hostname: device.hostname.clone(),
            addresses: device.addresses.iter().map(|a| a.to_string()).collect(),
            port: device.port,
            index,
        }
    }
}

/// Pairing result for the frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairingInfo {
    pub success: bool,
    pub server_name: String,
    pub ssh_user: String,
    pub ssh_command: String,
    pub private_key_path: String,
    pub public_key_path: String,
    /// SSH config alias written to `~/.ssh/config` (`ssh <alias>`), if the
    /// config update succeeded
    pub host_alias: Option<String>,
    pub error: Option<String>,
}

/// Server status for the frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerStatus {
    pub listening: bool,
    pub port: u16,
    pub device_name: String,
    pub addresses: Vec<String>,
    /// Most recent listener event (client connected / key received / pairing
    /// complete), for the UI to poll
    pub last_event: Option<String>,
}

/// Paired host from SSH config
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairedHost {
    pub host: String,
    pub hostname: String,
    pub user: String,
    pub identity_file: String,
}

impl From<HostEntry> for PairedHost {
    fn from(entry: HostEntry) -> Self {
        Self {
            host: entry.host,
            hostname: entry.hostname,
            user: entry.user,
            identity_file: entry.identity_file,
        }
    }
}

/// Local SSH key information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalKeyInfo {
    pub name: String,
    pub algorithm: String,
    pub comment: String,
    pub private_key_path: String,
    pub public_key_path: String,
    pub fingerprint: String,
    pub created: Option<String>,
}

// ============================================================================
// Shared helpers (testable without a UI)
// ============================================================================

/// Resolve the key pair to use for pairing/sync
///
/// Honors the user-configured default key when set (mirroring the CLI's
/// `--key`/`default_key` priority); otherwise generates a fresh key.
/// Returns the key pair and, when an existing key was used, its private key
/// path (a generated key still needs to be saved by the caller).
fn resolve_key_pair(
    user_config: &Config,
    use_rsa: bool,
    comment: &str,
) -> Result<(SshKeyPair, Option<PathBuf>), String> {
    if let Some(default_key) = user_config.default_key.as_deref() {
        let path = paths::expand_tilde(default_key).map_err(|e| e.to_string())?;
        let key_pair = SshKeyPair::load_from_file(&path.to_string_lossy())
            .map_err(|e| format!("Failed to load default key {}: {}", path.display(), e))?;
        return Ok((key_pair, Some(path)));
    }

    let algorithm = if use_rsa {
        KeyAlgorithm::Rsa4096
    } else {
        KeyAlgorithm::Ed25519
    };
    let key_pair = SshKeyPair::generate(algorithm, comment).map_err(|e| e.to_string())?;
    Ok((key_pair, None))
}

/// Write a connecto-managed SSH config entry for a paired/synced device
///
/// The alias is derived with the shared sanitizer and the entry points at the
/// actual private key path. Returns the alias.
fn write_host_entry(
    config: &SshConfig,
    device_name: &str,
    hostname: &str,
    user: &str,
    identity_file: &Path,
) -> Result<String, String> {
    let alias = sanitize_device_name(device_name);
    let entry = HostEntry {
        host: alias.clone(),
        hostname: hostname.to_string(),
        user: user.to_string(),
        identity_file: identity_file.to_string_lossy().to_string(),
    };
    config.add_host(&entry).map_err(|e| e.to_string())?;
    Ok(alias)
}

/// Write the post-pair/post-sync host entry into the default SSH config
fn write_host_entry_at_default(
    device_name: &str,
    hostname: &str,
    user: &str,
    identity_file: &Path,
) -> Result<String, String> {
    let config = SshConfig::at_default().map_err(|e| e.to_string())?;
    write_host_entry(&config, device_name, hostname, user, identity_file)
}

/// List connecto-managed hosts from an explicit SSH config (for testing)
fn list_paired_hosts_in(config: &SshConfig) -> Result<Vec<PairedHost>, String> {
    Ok(config
        .list_hosts()
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(PairedHost::from)
        .collect())
}

/// Map a listener `ServerEvent` to a user-facing status message
fn listener_event_message(event: &ServerEvent) -> String {
    match event {
        ServerEvent::Started { address } => format!("Listening on {}", address),
        ServerEvent::ClientConnected { address } => format!("Client connected from {}", address),
        ServerEvent::PairingRequest {
            device_name,
            address,
        } => format!("Pairing request from {} ({})", device_name, address),
        ServerEvent::KeyReceived { comment } => format!("Received key: {}", comment),
        ServerEvent::PairingComplete { device_name } => {
            format!("Pairing complete with {}", device_name)
        }
        ServerEvent::Error { message } => format!("Error: {}", message),
    }
}

/// Apply a `SyncEvent` to the shared sync status
async fn apply_sync_event(status: &Arc<Mutex<SyncStatus>>, event: SyncEvent) {
    let mut status = status.lock().await;
    match event {
        SyncEvent::Started { address } => {
            status.phase = "started".to_string();
            status.status_message = format!("Listening on {}", address);
        }
        SyncEvent::Searching => {
            status.phase = "searching".to_string();
            status.status_message = "Searching for sync peer...".to_string();
        }
        SyncEvent::PeerFound {
            device_name,
            address,
        } => {
            status.phase = "peer-found".to_string();
            status.status_message = format!("Found peer {} at {}", device_name, address);
            status.peer_name = Some(device_name);
        }
        SyncEvent::Connected { device_name } => {
            status.phase = "connected".to_string();
            status.status_message = format!("Connected to {}", device_name);
            status.peer_name = Some(device_name);
        }
        SyncEvent::KeyReceived {
            device_name,
            key_comment,
        } => {
            status.phase = "key-received".to_string();
            status.status_message = format!("Received key from {} ({})", device_name, key_comment);
        }
        SyncEvent::KeyAccepted => {
            status.phase = "key-accepted".to_string();
            status.status_message = "Our key was accepted by the peer".to_string();
        }
        SyncEvent::Completed { peer_name, .. } => {
            status.phase = "completed".to_string();
            status.status_message = format!("Sync completed with {}", peer_name);
            status.peer_name = Some(peer_name);
        }
        SyncEvent::Failed { message } => {
            status.phase = "failed".to_string();
            status.status_message = format!("Sync failed: {}", message);
        }
    }
}

/// Get the current hostname
#[tauri::command]
pub fn get_device_name() -> String {
    get_hostname()
}

/// Get local IP addresses
#[tauri::command]
pub fn get_addresses() -> Vec<String> {
    get_local_addresses()
        .iter()
        .filter(|a| a.is_ipv4())
        .map(|a| a.to_string())
        .collect()
}

/// Scan for devices on the network
///
/// Scans via mDNS first, then additionally probes any user-configured subnets
/// (`connecto config add-subnet ...`) so VPN/cross-subnet hosts that mDNS
/// cannot reach are also found, deduplicated by address like the CLI scan.
#[tauri::command]
pub async fn scan_devices(
    timeout_secs: u64,
    state: State<'_, AppState>,
) -> Result<Vec<DeviceInfo>, String> {
    let browser = ServiceBrowser::new().map_err(|e| e.to_string())?;

    let mut devices = browser
        .scan_for_duration(Duration::from_secs(timeout_secs))
        .await
        .map_err(|e| e.to_string())?;

    let user_config = Config::load().unwrap_or_default();
    if !user_config.subnets.is_empty() {
        let scanner = SubnetScanner::new(DEFAULT_PORT, Duration::from_millis(500));
        let extra = scanner.scan_subnets(&user_config.subnets).await;
        for device in extra {
            if !devices.iter().any(|d| d.addresses == device.addresses) {
                devices.push(device);
            }
        }
    }

    // Store devices in state
    {
        let mut cached = state.discovered_devices.lock().await;
        *cached = devices.clone();
    }

    Ok(devices
        .iter()
        .enumerate()
        .map(|(i, d)| DeviceInfo::from((i, d)))
        .collect())
}

/// Pair with a device by index
#[tauri::command]
pub async fn pair_with_device(
    device_index: usize,
    use_rsa: bool,
    custom_comment: Option<String>,
    state: State<'_, AppState>,
) -> Result<PairingInfo, String> {
    // Get the device from cache
    let device = {
        let devices = state.discovered_devices.lock().await;
        devices
            .get(device_index)
            .cloned()
            .ok_or_else(|| "Device not found. Please scan again.".to_string())?
    };

    let address = device
        .connection_string()
        .ok_or_else(|| "Device has no IP address".to_string())?;

    pair_with_address(address, use_rsa, custom_comment).await
}

/// Pair with a device by address
#[tauri::command]
pub async fn pair_with_address(
    address: String,
    use_rsa: bool,
    custom_comment: Option<String>,
) -> Result<PairingInfo, String> {
    // Generate comment
    let comment = custom_comment.unwrap_or_else(|| {
        let user = std::env::var("USER").unwrap_or_else(|_| "user".to_string());
        let hostname = get_hostname();
        format!("{}@{}", user, hostname)
    });

    // Use the configured default key if set, otherwise generate a new pair
    let user_config = Config::load().unwrap_or_default();
    let (key_pair, existing_key_path) = resolve_key_pair(&user_config, use_rsa, &comment)?;

    // Create client and pair
    let client = HandshakeClient::new(&get_hostname());
    let result = client.pair(&address, &key_pair).await;

    match result {
        Ok(pairing_result) => {
            let alias = sanitize_device_name(&pairing_result.server_name);

            // Use the existing key's path, or save the generated key under the
            // CLI naming convention
            let (private_path, public_path) = match &existing_key_path {
                Some(path) => (
                    path.clone(),
                    PathBuf::from(format!("{}.pub", path.display())),
                ),
                None => {
                    let key_manager = KeyManager::new().map_err(|e| e.to_string())?;
                    let key_name = format!("connecto_{}", alias);
                    key_manager
                        .save_key_pair(&key_pair, &key_name)
                        .map_err(|e| e.to_string())?
                }
            };

            let ip = address.split(':').next().unwrap_or(&address);
            let ssh_command = format!(
                "ssh -i {} {}@{}",
                private_path.display(),
                pairing_result.ssh_user,
                ip
            );

            // Write the ~/.ssh/config entry so `ssh <alias>` works, like the CLI
            let host_alias = write_host_entry_at_default(
                &pairing_result.server_name,
                ip,
                &pairing_result.ssh_user,
                &private_path,
            )
            .ok();

            Ok(PairingInfo {
                success: true,
                server_name: pairing_result.server_name,
                ssh_user: pairing_result.ssh_user,
                ssh_command,
                private_key_path: private_path.to_string_lossy().to_string(),
                public_key_path: public_path.to_string_lossy().to_string(),
                host_alias,
                error: None,
            })
        }
        Err(e) => Ok(PairingInfo {
            success: false,
            server_name: String::new(),
            ssh_user: String::new(),
            ssh_command: String::new(),
            private_key_path: String::new(),
            public_key_path: String::new(),
            host_alias: None,
            error: Some(e.to_string()),
        }),
    }
}

/// Start the listener server
///
/// Binds the handshake server, starts mDNS advertising, and keeps the server
/// running in a background task until `stop_listener` is called. Listener
/// events are consumed into a status field the UI can poll.
#[tauri::command]
pub async fn start_listener(
    port: u16,
    device_name: Option<String>,
    state: State<'_, AppState>,
) -> Result<ServerStatus, String> {
    let name = device_name.unwrap_or_else(get_hostname);

    let mut listener_guard = state.listener.lock().await;
    if let Some(handle) = listener_guard.as_ref() {
        if !handle.server_task.is_finished() {
            return Err("Listener is already running".to_string());
        }
    }
    // Clean up a previously crashed/finished listener before restarting
    if let Some(old) = listener_guard.take() {
        old.server_task.abort();
        old.event_task.abort();
    }

    // Bind first; only advertise once the socket is actually listening
    let key_manager = KeyManager::new().map_err(|e| e.to_string())?;
    let mut server = HandshakeServer::new(key_manager, &name);
    let addr = server.listen(port).await.map_err(|e| e.to_string())?;

    let mut advertiser = ServiceAdvertiser::new().map_err(|e| e.to_string())?;
    advertiser
        .advertise(&name, addr.port())
        .map_err(|e| e.to_string())?;

    // Consume server events into the pollable activity field
    let (event_tx, mut event_rx) = mpsc::channel(16);
    let activity = Arc::clone(&state.listener_activity);
    let event_task = tokio::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            let message = listener_event_message(&event);
            *activity.lock().await = Some(message);
        }
    });

    // Drive the handshake server until stopped
    let activity = Arc::clone(&state.listener_activity);
    let server_task = tokio::spawn(async move {
        if let Err(e) = server.run(event_tx).await {
            *activity.lock().await = Some(format!("Listener stopped: {}", e));
        }
    });

    *state.listener_activity.lock().await = None;
    *listener_guard = Some(ListenerHandle {
        advertiser,
        server_task,
        event_task,
        port: addr.port(),
        device_name: name.clone(),
    });
    drop(listener_guard);

    // Get addresses for display
    let addresses: Vec<String> = get_local_addresses()
        .iter()
        .filter(|a| a.is_ipv4())
        .map(|a| a.to_string())
        .collect();

    Ok(ServerStatus {
        listening: true,
        port: addr.port(),
        device_name: name,
        addresses,
        last_event: None,
    })
}

/// Stop the listener server
///
/// Aborts the server task (closing the socket), waits for it to finish, then
/// stops mDNS advertising before reporting stopped.
#[tauri::command]
pub async fn stop_listener(state: State<'_, AppState>) -> Result<(), String> {
    let handle = state.listener.lock().await.take();

    if let Some(handle) = handle {
        let ListenerHandle {
            mut advertiser,
            server_task,
            event_task,
            ..
        } = handle;

        server_task.abort();
        event_task.abort();
        // Wait for the tasks to actually terminate so the socket is closed
        // before we report the listener as stopped.
        let _ = server_task.await;
        let _ = event_task.await;

        // Unregister the mDNS service; dropping the advertiser also shuts
        // its daemon down.
        advertiser.stop().map_err(|e| e.to_string())?;
    }

    *state.listener_activity.lock().await = None;
    Ok(())
}

/// Get listening status
#[tauri::command]
pub async fn get_listener_status(state: State<'_, AppState>) -> Result<bool, String> {
    let guard = state.listener.lock().await;
    Ok(guard.as_ref().is_some_and(|h| !h.server_task.is_finished()))
}

/// Get detailed listener status, including the most recent listener event
#[tauri::command]
pub async fn get_listener_info(state: State<'_, AppState>) -> Result<ServerStatus, String> {
    let (listening, port, device_name) = {
        let guard = state.listener.lock().await;
        match guard.as_ref() {
            Some(h) if !h.server_task.is_finished() => (true, h.port, h.device_name.clone()),
            _ => (false, 0, String::new()),
        }
    };

    let last_event = state.listener_activity.lock().await.clone();
    let addresses: Vec<String> = get_local_addresses()
        .iter()
        .filter(|a| a.is_ipv4())
        .map(|a| a.to_string())
        .collect();

    Ok(ServerStatus {
        listening,
        port,
        device_name,
        addresses,
        last_event,
    })
}

/// List authorized keys
#[tauri::command]
pub fn list_authorized_keys() -> Result<Vec<String>, String> {
    let key_manager = KeyManager::new().map_err(|e| e.to_string())?;
    key_manager
        .list_authorized_keys()
        .map_err(|e| e.to_string())
}

/// Remove an authorized key
#[tauri::command]
pub fn remove_authorized_key(key: String) -> Result<bool, String> {
    let key_manager = KeyManager::new().map_err(|e| e.to_string())?;
    key_manager
        .remove_authorized_key(&key)
        .map_err(|e| e.to_string())
}

/// Generate a new SSH key pair
#[tauri::command]
pub fn generate_key_pair(
    name: String,
    comment: Option<String>,
    use_rsa: bool,
) -> Result<(String, String), String> {
    let algorithm = if use_rsa {
        KeyAlgorithm::Rsa4096
    } else {
        KeyAlgorithm::Ed25519
    };

    let key_comment = comment.unwrap_or_else(|| {
        let user = std::env::var("USER").unwrap_or_else(|_| "user".to_string());
        let hostname = get_hostname();
        format!("{}@{}", user, hostname)
    });

    let key_pair = SshKeyPair::generate(algorithm, &key_comment).map_err(|e| e.to_string())?;

    let key_manager = KeyManager::new().map_err(|e| e.to_string())?;
    let (private_path, public_path) = key_manager
        .save_key_pair(&key_pair, &name)
        .map_err(|e| e.to_string())?;

    Ok((
        private_path.to_string_lossy().to_string(),
        public_path.to_string_lossy().to_string(),
    ))
}

/// List paired hosts from SSH config
#[tauri::command]
pub fn list_paired_hosts() -> Result<Vec<PairedHost>, String> {
    let config = SshConfig::at_default().map_err(|e| e.to_string())?;
    list_paired_hosts_in(&config)
}

// ============================================================================
// Local SSH key management
// ============================================================================

/// Files to ignore when scanning for SSH keys
const IGNORED_FILES: &[&str] = &[
    "config",
    "known_hosts",
    "known_hosts.old",
    "authorized_keys",
    "authorized_keys2",
    "environment",
    "rc",
];

/// Parse public key to extract algorithm, comment, and fingerprint
fn parse_public_key_info(public_key_content: &str) -> (String, String, String) {
    let parts: Vec<&str> = public_key_content.split_whitespace().collect();

    let algorithm = parts.first().unwrap_or(&"unknown").to_string();
    let comment = if parts.len() > 2 {
        parts[2..].join(" ")
    } else {
        String::new()
    };

    // `SHA256:<base64>` form, matching `ssh-keygen -lf`; empty when the key
    // cannot be parsed (same fallback the old hand-rolled code had)
    let fingerprint = fingerprint_sha256(public_key_content).unwrap_or_default();

    (algorithm, comment, fingerprint)
}

/// List local SSH keys in the given directory (for testing)
pub fn list_local_keys_in_dir(ssh_dir: &Path) -> Result<Vec<LocalKeyInfo>, String> {
    use std::fs;

    if !ssh_dir.exists() {
        return Ok(Vec::new());
    }

    let entries = fs::read_dir(ssh_dir).map_err(|e| e.to_string())?;
    let mut keys = Vec::new();

    for entry in entries.flatten() {
        let path = entry.path();
        let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

        // Skip if it's a .pub file, directory, or ignored file
        if file_name.ends_with(".pub") || path.is_dir() || IGNORED_FILES.contains(&file_name) {
            continue;
        }

        // Check if corresponding .pub file exists
        let pub_path = ssh_dir.join(format!("{}.pub", file_name));
        if !pub_path.exists() {
            continue;
        }

        // Read public key to extract info
        let public_key_content = match fs::read_to_string(&pub_path) {
            Ok(content) => content,
            Err(_) => continue,
        };

        let (algorithm, comment, fingerprint) = parse_public_key_info(&public_key_content);

        // Get file creation time if available
        let created = fs::metadata(&path)
            .ok()
            .and_then(|m| m.created().ok())
            .map(|t| {
                let duration = t.duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
                // Format as ISO date
                let secs = duration.as_secs();
                let days = secs / 86400;
                let years = 1970 + (days / 365);
                format!("{}", years)
            });

        keys.push(LocalKeyInfo {
            name: file_name.to_string(),
            algorithm,
            comment,
            private_key_path: path.to_string_lossy().to_string(),
            public_key_path: pub_path.to_string_lossy().to_string(),
            fingerprint,
            created,
        });
    }

    // Sort by name
    keys.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(keys)
}

/// Delete a local SSH key pair (for testing)
pub fn delete_local_key_in_dir(ssh_dir: &Path, name: &str) -> Result<(), String> {
    use std::fs;

    let private_path = ssh_dir.join(name);
    let public_path = ssh_dir.join(format!("{}.pub", name));

    if !private_path.exists() && !public_path.exists() {
        return Err(format!("Key '{}' not found", name));
    }

    if private_path.exists() {
        fs::remove_file(&private_path).map_err(|e| e.to_string())?;
    }

    if public_path.exists() {
        fs::remove_file(&public_path).map_err(|e| e.to_string())?;
    }

    Ok(())
}

/// Get detailed information about a specific key (for testing)
pub fn get_key_details_in_dir(ssh_dir: &Path, name: &str) -> Result<LocalKeyInfo, String> {
    use std::fs;

    let private_path = ssh_dir.join(name);
    let public_path = ssh_dir.join(format!("{}.pub", name));

    if !private_path.exists() || !public_path.exists() {
        return Err(format!("Key '{}' not found", name));
    }

    let public_key_content = fs::read_to_string(&public_path).map_err(|e| e.to_string())?;
    let (algorithm, comment, fingerprint) = parse_public_key_info(&public_key_content);

    let created = fs::metadata(&private_path)
        .ok()
        .and_then(|m| m.created().ok())
        .map(|t| {
            let duration = t.duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
            let secs = duration.as_secs();
            let days = secs / 86400;
            let years = 1970 + (days / 365);
            format!("{}", years)
        });

    Ok(LocalKeyInfo {
        name: name.to_string(),
        algorithm,
        comment,
        private_key_path: private_path.to_string_lossy().to_string(),
        public_key_path: public_path.to_string_lossy().to_string(),
        fingerprint,
        created,
    })
}

/// Rename a local SSH key pair (for testing)
pub fn rename_local_key_in_dir(
    ssh_dir: &Path,
    old_name: &str,
    new_name: &str,
) -> Result<(), String> {
    use std::fs;

    let old_private = ssh_dir.join(old_name);
    let old_public = ssh_dir.join(format!("{}.pub", old_name));
    let new_private = ssh_dir.join(new_name);
    let new_public = ssh_dir.join(format!("{}.pub", new_name));

    if !old_private.exists() {
        return Err(format!("Key '{}' not found", old_name));
    }

    if new_private.exists() || new_public.exists() {
        return Err(format!("Key '{}' already exists", new_name));
    }

    fs::rename(&old_private, &new_private).map_err(|e| e.to_string())?;

    if old_public.exists() {
        fs::rename(&old_public, &new_public).map_err(|e| e.to_string())?;
    }

    Ok(())
}

// ============================================================================
// Tauri commands for local key management
// ============================================================================

/// Helper function to get SSH directory
fn get_ssh_dir() -> Result<PathBuf, String> {
    paths::ssh_dir().map_err(|e| e.to_string())
}

/// List all local SSH keys
#[tauri::command]
pub fn list_local_keys() -> Result<Vec<LocalKeyInfo>, String> {
    let ssh_dir = get_ssh_dir()?;
    list_local_keys_in_dir(&ssh_dir)
}

/// Delete a local SSH key pair
#[tauri::command]
pub fn delete_local_key(name: String) -> Result<(), String> {
    let ssh_dir = get_ssh_dir()?;
    delete_local_key_in_dir(&ssh_dir, &name)
}

/// Get detailed information about a specific key
#[tauri::command]
pub fn get_key_details(name: String) -> Result<LocalKeyInfo, String> {
    let ssh_dir = get_ssh_dir()?;
    get_key_details_in_dir(&ssh_dir, &name)
}

/// Rename a local SSH key pair
#[tauri::command]
pub fn rename_local_key(old_name: String, new_name: String) -> Result<(), String> {
    let ssh_dir = get_ssh_dir()?;
    rename_local_key_in_dir(&ssh_dir, &old_name, &new_name)
}

// ============================================================================
// Sync commands
// ============================================================================

/// Sync result for the frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncResultInfo {
    pub success: bool,
    pub peer_name: String,
    pub peer_user: String,
    pub peer_address: String,
    pub ssh_command: String,
    /// SSH config alias written to `~/.ssh/config` (`ssh <alias>`), if the
    /// config update succeeded
    pub host_alias: Option<String>,
    pub error: Option<String>,
}

/// Sync status for the frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncStatusInfo {
    pub is_syncing: bool,
    pub status_message: String,
    pub phase: String,
    pub peer_name: Option<String>,
}

impl SyncResultInfo {
    fn failure(error: String) -> Self {
        Self {
            success: false,
            peer_name: String::new(),
            peer_user: String::new(),
            peer_address: String::new(),
            ssh_command: String::new(),
            host_alias: None,
            error: Some(error),
        }
    }
}

/// Start sync operation
///
/// Runs the sync in a background task so `cancel_sync` can abort it; sync
/// events are consumed into the pollable sync status.
#[tauri::command]
pub async fn start_sync(
    port: u16,
    device_name: Option<String>,
    timeout_secs: u64,
    use_rsa: bool,
    state: State<'_, AppState>,
) -> Result<SyncResultInfo, String> {
    let name = device_name.unwrap_or_else(get_hostname);

    let user = std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "user".to_string());
    let comment = format!("{}@{}", user, name);

    // Use the configured default key if set, otherwise generate a new pair
    let user_config = Config::load().unwrap_or_default();
    let (key_pair, existing_key_path) = resolve_key_pair(&user_config, use_rsa, &comment)?;

    // The actual private key path, used for the SSH config entry below
    let private_path = match &existing_key_path {
        Some(path) => path.clone(),
        None => {
            let key_manager = KeyManager::new().map_err(|e| e.to_string())?;
            let key_name = format!("connecto_sync_{}", sanitize_device_name(&name));
            let (private_path, _) = key_manager
                .save_key_pair(&key_pair, &key_name)
                .map_err(|e| e.to_string())?;
            private_path
        }
    };

    let sync_key_manager = KeyManager::new().map_err(|e| e.to_string())?;
    let handler = SyncHandler::new(sync_key_manager, &name, key_pair);

    // Spawn the sync task while holding the task slot, so two concurrent
    // start_sync calls cannot both start.
    let (sync_task, event_task) = {
        let mut task_guard = state.sync_task.lock().await;
        if task_guard.is_some() {
            return Err("Sync operation already in progress".to_string());
        }

        {
            let mut status = state.sync_status.lock().await;
            *status = SyncStatus {
                is_syncing: true,
                status_message: "Starting sync...".to_string(),
                phase: "starting".to_string(),
                peer_name: None,
            };
        }

        // Consume sync events into the pollable status
        let (event_tx, mut event_rx) = mpsc::channel(16);
        let status_handle = Arc::clone(&state.sync_status);
        let event_task = tokio::spawn(async move {
            while let Some(event) = event_rx.recv().await {
                apply_sync_event(&status_handle, event).await;
            }
        });

        let sync_task =
            tokio::spawn(async move { handler.run(port, timeout_secs, event_tx).await });
        *task_guard = Some(sync_task.abort_handle());
        (sync_task, event_task)
    };

    // Wait for the sync task to finish (or be aborted by cancel_sync)
    let run_result = sync_task.await;

    // The task is done; release the slot and only now flip the status
    *state.sync_task.lock().await = None;
    event_task.abort();

    let result = match run_result {
        Ok(result) => result,
        Err(join_error) => {
            let message = if join_error.is_cancelled() {
                "Sync cancelled".to_string()
            } else {
                format!("Sync task failed: {}", join_error)
            };
            let mut status = state.sync_status.lock().await;
            status.is_syncing = false;
            status.phase = "cancelled".to_string();
            status.status_message = message.clone();
            return Ok(SyncResultInfo::failure(message));
        }
    };

    match result {
        Ok(sync_result) => {
            let peer_address = sync_result.peer_address.to_string();
            let ssh_command = format!(
                "ssh -i {} {}@{}",
                private_path.display(),
                sync_result.peer_user,
                peer_address
            );

            // Write the ~/.ssh/config entry so `ssh <alias>` works, like the CLI
            let host_alias = write_host_entry_at_default(
                &sync_result.peer_name,
                &peer_address,
                &sync_result.peer_user,
                &private_path,
            )
            .ok();

            {
                let mut status = state.sync_status.lock().await;
                status.is_syncing = false;
                status.phase = "completed".to_string();
                status.status_message = format!("Sync completed with {}!", sync_result.peer_name);
                status.peer_name = Some(sync_result.peer_name.clone());
            }

            Ok(SyncResultInfo {
                success: true,
                peer_name: sync_result.peer_name,
                peer_user: sync_result.peer_user,
                peer_address,
                ssh_command,
                host_alias,
                error: None,
            })
        }
        Err(e) => {
            {
                let mut status = state.sync_status.lock().await;
                status.is_syncing = false;
                status.phase = "failed".to_string();
                status.status_message = format!("Sync failed: {}", e);
            }

            Ok(SyncResultInfo::failure(e.to_string()))
        }
    }
}

/// Get sync status
#[tauri::command]
pub async fn get_sync_status(state: State<'_, AppState>) -> Result<SyncStatusInfo, String> {
    let status = state.sync_status.lock().await;
    Ok(SyncStatusInfo {
        is_syncing: status.is_syncing,
        status_message: status.status_message.clone(),
        phase: status.phase.clone(),
        peer_name: status.peer_name.clone(),
    })
}

/// Cancel sync operation
///
/// Aborts the running sync task; the status flips only once the task has
/// actually finished (handled by `start_sync`'s completion path).
#[tauri::command]
pub async fn cancel_sync(state: State<'_, AppState>) -> Result<(), String> {
    let task_guard = state.sync_task.lock().await;
    if let Some(handle) = task_guard.as_ref() {
        handle.abort();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_device_info_creation() {
        let device = DiscoveredDevice {
            name: "Test Device".to_string(),
            hostname: "test.local.".to_string(),
            addresses: vec!["192.168.1.100".parse().unwrap()],
            port: 8099,
            instance_name: "test".to_string(),
        };

        let info = DeviceInfo::from((0, &device));
        assert_eq!(info.name, "Test Device");
        assert_eq!(info.index, 0);
    }

    #[test]
    fn test_get_device_name() {
        let name = get_device_name();
        assert!(!name.is_empty());
    }

    // Tests for SSH config integration

    fn config_in(temp_dir: &TempDir) -> SshConfig {
        SshConfig::new(temp_dir.path().join(".ssh").join("config"))
    }

    #[test]
    fn test_write_host_entry_round_trip() {
        let temp_dir = TempDir::new().unwrap();
        let config = config_in(&temp_dir);
        let key_path = temp_dir.path().join(".ssh").join("connecto_my-device");

        let alias =
            write_host_entry(&config, "My Device", "192.168.1.5", "alice", &key_path).unwrap();
        assert_eq!(alias, "my-device");

        let hosts = list_paired_hosts_in(&config).unwrap();
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].host, "my-device");
        assert_eq!(hosts[0].hostname, "192.168.1.5");
        assert_eq!(hosts[0].user, "alice");
        assert_eq!(hosts[0].identity_file, key_path.to_string_lossy());
    }

    #[test]
    fn test_write_host_entry_replaces_same_device() {
        let temp_dir = TempDir::new().unwrap();
        let config = config_in(&temp_dir);
        let key_path = temp_dir.path().join(".ssh").join("connecto_box");

        write_host_entry(&config, "Box", "10.0.0.1", "alice", &key_path).unwrap();
        write_host_entry(&config, "Box", "10.0.0.2", "bob", &key_path).unwrap();

        let hosts = list_paired_hosts_in(&config).unwrap();
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].hostname, "10.0.0.2");
        assert_eq!(hosts[0].user, "bob");
    }

    #[test]
    fn test_list_paired_hosts_in_empty_config() {
        let temp_dir = TempDir::new().unwrap();
        let config = config_in(&temp_dir);

        assert!(list_paired_hosts_in(&config).unwrap().is_empty());
    }

    // Tests for key resolution

    #[test]
    fn test_resolve_key_pair_generates_without_default_key() {
        let user_config = Config::default();

        let (key_pair, existing) = resolve_key_pair(&user_config, false, "test@connecto").unwrap();
        assert!(existing.is_none());
        assert!(key_pair.public_key.starts_with("ssh-ed25519 "));
        assert!(key_pair.public_key.contains("test@connecto"));
    }

    #[test]
    fn test_resolve_key_pair_uses_default_key() {
        let temp_dir = TempDir::new().unwrap();
        let ssh_dir = temp_dir.path().join(".ssh");
        let key_manager = KeyManager::with_dir(ssh_dir);

        let generated = SshKeyPair::generate(KeyAlgorithm::Ed25519, "default@connecto").unwrap();
        let (private_path, _) = key_manager.save_key_pair(&generated, "my_default").unwrap();

        let user_config = Config {
            subnets: Vec::new(),
            default_key: Some(private_path.to_string_lossy().to_string()),
        };

        let (key_pair, existing) = resolve_key_pair(&user_config, false, "ignored").unwrap();
        assert_eq!(existing.as_deref(), Some(private_path.as_path()));
        assert_eq!(key_pair.public_key, generated.public_key);
    }

    #[test]
    fn test_resolve_key_pair_missing_default_key_errors() {
        let user_config = Config {
            subnets: Vec::new(),
            default_key: Some("/definitely/not/a/key".to_string()),
        };

        assert!(resolve_key_pair(&user_config, false, "comment").is_err());
    }

    // Tests for fingerprint display format

    #[test]
    fn test_parse_public_key_info_uses_core_fingerprint() {
        let key_pair = SshKeyPair::generate(KeyAlgorithm::Ed25519, "fp@connecto").unwrap();

        let (algorithm, comment, fingerprint) = parse_public_key_info(&key_pair.public_key);
        assert_eq!(algorithm, "ssh-ed25519");
        assert_eq!(comment, "fp@connecto");
        assert!(fingerprint.starts_with("SHA256:"));
        assert_eq!(
            fingerprint,
            fingerprint_sha256(&key_pair.public_key).unwrap()
        );
    }

    #[test]
    fn test_parse_public_key_info_invalid_key_has_empty_fingerprint() {
        let (algorithm, _, fingerprint) = parse_public_key_info("ssh-ed25519 AAAA... bad@key");
        assert_eq!(algorithm, "ssh-ed25519");
        assert!(fingerprint.is_empty());
    }

    // Tests for local key management

    #[test]
    fn test_list_local_keys_empty_dir() {
        let temp_dir = TempDir::new().unwrap();
        let ssh_dir = temp_dir.path().join(".ssh");
        std::fs::create_dir_all(&ssh_dir).unwrap();

        let keys = list_local_keys_in_dir(&ssh_dir).unwrap();
        assert!(keys.is_empty());
    }

    #[test]
    fn test_list_local_keys_ignores_non_key_files() {
        let temp_dir = TempDir::new().unwrap();
        let ssh_dir = temp_dir.path().join(".ssh");
        std::fs::create_dir_all(&ssh_dir).unwrap();

        // Create non-key files
        std::fs::write(ssh_dir.join("config"), "Host *\n").unwrap();
        std::fs::write(ssh_dir.join("known_hosts"), "").unwrap();
        std::fs::write(ssh_dir.join("authorized_keys"), "").unwrap();

        let keys = list_local_keys_in_dir(&ssh_dir).unwrap();
        assert!(keys.is_empty());
    }

    #[test]
    fn test_delete_local_key_removes_both_files() {
        let temp_dir = TempDir::new().unwrap();
        let ssh_dir = temp_dir.path().join(".ssh");
        std::fs::create_dir_all(&ssh_dir).unwrap();

        let private_path = ssh_dir.join("test_key");
        let public_path = ssh_dir.join("test_key.pub");

        std::fs::write(&private_path, "private").unwrap();
        std::fs::write(&public_path, "public").unwrap();

        assert!(private_path.exists());
        assert!(public_path.exists());

        let result = delete_local_key_in_dir(&ssh_dir, "test_key");
        assert!(result.is_ok());
        assert!(!private_path.exists());
        assert!(!public_path.exists());
    }

    #[test]
    fn test_delete_local_key_nonexistent() {
        let temp_dir = TempDir::new().unwrap();
        let ssh_dir = temp_dir.path().join(".ssh");
        std::fs::create_dir_all(&ssh_dir).unwrap();

        let result = delete_local_key_in_dir(&ssh_dir, "nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn test_rename_local_key() {
        let temp_dir = TempDir::new().unwrap();
        let ssh_dir = temp_dir.path().join(".ssh");
        std::fs::create_dir_all(&ssh_dir).unwrap();

        std::fs::write(ssh_dir.join("old_name"), "private_content").unwrap();
        std::fs::write(
            ssh_dir.join("old_name.pub"),
            "ssh-ed25519 AAAA... test@connecto",
        )
        .unwrap();

        let result = rename_local_key_in_dir(&ssh_dir, "old_name", "new_name");
        assert!(result.is_ok());

        assert!(!ssh_dir.join("old_name").exists());
        assert!(!ssh_dir.join("old_name.pub").exists());
        assert!(ssh_dir.join("new_name").exists());
        assert!(ssh_dir.join("new_name.pub").exists());
    }

    #[test]
    fn test_rename_local_key_target_exists() {
        let temp_dir = TempDir::new().unwrap();
        let ssh_dir = temp_dir.path().join(".ssh");
        std::fs::create_dir_all(&ssh_dir).unwrap();

        std::fs::write(ssh_dir.join("key1"), "private1").unwrap();
        std::fs::write(ssh_dir.join("key1.pub"), "public1").unwrap();
        std::fs::write(ssh_dir.join("key2"), "private2").unwrap();
        std::fs::write(ssh_dir.join("key2.pub"), "public2").unwrap();

        let result = rename_local_key_in_dir(&ssh_dir, "key1", "key2");
        assert!(result.is_err()); // Should fail, target exists
    }
}
