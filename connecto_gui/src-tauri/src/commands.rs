//! Tauri commands for the GUI

use connecto_core::{
    discovery::{
        get_hostname, get_local_addresses, DiscoveredDevice, ServiceAdvertiser, ServiceBrowser,
    },
    keys::{KeyAlgorithm, KeyManager, SshKeyPair},
    protocol::{HandshakeClient, HandshakeServer},
};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tauri::State;

use crate::state::AppState;

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
    pub error: Option<String>,
}

/// Server status for the frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerStatus {
    pub listening: bool,
    pub port: u16,
    pub device_name: String,
    pub addresses: Vec<String>,
}

/// Paired host from SSH config
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairedHost {
    pub host: String,
    pub hostname: String,
    pub user: String,
    pub identity_file: String,
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
#[tauri::command]
pub async fn scan_devices(
    timeout_secs: u64,
    state: State<'_, AppState>,
) -> Result<Vec<DeviceInfo>, String> {
    let browser = ServiceBrowser::new().map_err(|e| e.to_string())?;

    let devices = browser
        .scan_for_duration(Duration::from_secs(timeout_secs))
        .await
        .map_err(|e| e.to_string())?;

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
    // Determine algorithm
    let algorithm = if use_rsa {
        KeyAlgorithm::Rsa4096
    } else {
        KeyAlgorithm::Ed25519
    };

    // Generate comment
    let comment = custom_comment.unwrap_or_else(|| {
        let user = std::env::var("USER").unwrap_or_else(|_| "user".to_string());
        let hostname = get_hostname();
        format!("{}@{}", user, hostname)
    });

    // Generate key pair
    let key_pair = SshKeyPair::generate(algorithm, &comment).map_err(|e| e.to_string())?;

    // Create client and pair
    let client = HandshakeClient::new(&get_hostname());
    let result = client.pair(&address, &key_pair).await;

    match result {
        Ok(pairing_result) => {
            // Save the key locally
            let key_manager = KeyManager::new().map_err(|e| e.to_string())?;
            let key_name = format!(
                "connecto_{}",
                pairing_result
                    .server_name
                    .chars()
                    .map(|c| if c.is_alphanumeric() { c } else { '_' })
                    .collect::<String>()
                    .to_lowercase()
            );

            let (private_path, public_path) = key_manager
                .save_key_pair(&key_pair, &key_name)
                .map_err(|e| e.to_string())?;

            let ip = address.split(':').next().unwrap_or(&address);
            let ssh_command = format!(
                "ssh -i {} {}@{}",
                private_path.display(),
                pairing_result.ssh_user,
                ip
            );

            Ok(PairingInfo {
                success: true,
                server_name: pairing_result.server_name,
                ssh_user: pairing_result.ssh_user,
                ssh_command,
                private_key_path: private_path.to_string_lossy().to_string(),
                public_key_path: public_path.to_string_lossy().to_string(),
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
            error: Some(e.to_string()),
        }),
    }
}

/// Start the listener server
#[tauri::command]
pub async fn start_listener(
    port: u16,
    device_name: Option<String>,
    state: State<'_, AppState>,
) -> Result<ServerStatus, String> {
    let name = device_name.unwrap_or_else(get_hostname);

    // Start mDNS advertiser
    let mut advertiser = ServiceAdvertiser::new().map_err(|e| e.to_string())?;
    advertiser
        .advertise(&name, port)
        .map_err(|e| e.to_string())?;

    // Store advertiser in state
    {
        let mut adv = state.advertiser.lock().await;
        *adv = Some(advertiser);
    }

    // Start handshake server
    let key_manager = KeyManager::new().map_err(|e| e.to_string())?;
    let mut server = HandshakeServer::new(key_manager, &name);
    let addr = server.listen(port).await.map_err(|e| e.to_string())?;

    // Store listening state
    {
        let mut listening = state.is_listening.lock().await;
        *listening = true;
    }

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
    })
}

/// Stop the listener server
#[tauri::command]
pub async fn stop_listener(state: State<'_, AppState>) -> Result<(), String> {
    // Stop advertiser
    {
        let mut adv = state.advertiser.lock().await;
        if let Some(ref mut advertiser) = *adv {
            advertiser.stop().map_err(|e| e.to_string())?;
        }
        *adv = None;
    }

    // Update listening state
    {
        let mut listening = state.is_listening.lock().await;
        *listening = false;
    }

    Ok(())
}

/// Get listening status
#[tauri::command]
pub async fn get_listener_status(state: State<'_, AppState>) -> Result<bool, String> {
    Ok(*state.is_listening.lock().await)
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
    use std::fs;

    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map_err(|_| "HOME/USERPROFILE not set".to_string())?;
    let config_path = std::path::PathBuf::from(&home).join(".ssh").join("config");

    if !config_path.exists() {
        return Ok(Vec::new());
    }

    let content = fs::read_to_string(&config_path).map_err(|e| e.to_string())?;

    let mut hosts: Vec<PairedHost> = Vec::new();
    let mut in_connecto_block = false;
    let mut current_host: Option<String> = None;
    let mut current_hostname: Option<String> = None;
    let mut current_user: Option<String> = None;
    let mut current_identity: Option<String> = None;

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed == "# Added by connecto" {
            in_connecto_block = true;
            continue;
        }

        if in_connecto_block {
            if trimmed.starts_with("Host ") && !trimmed.contains('*') {
                // Save previous host if complete
                if let (Some(h), Some(hn), Some(u), Some(id)) = (
                    current_host.take(),
                    current_hostname.take(),
                    current_user.take(),
                    current_identity.take(),
                ) {
                    hosts.push(PairedHost {
                        host: h,
                        hostname: hn,
                        user: u,
                        identity_file: id,
                    });
                }
                current_host = Some(trimmed.strip_prefix("Host ").unwrap().to_string());
            } else if trimmed.starts_with("HostName ") {
                current_hostname = Some(trimmed.strip_prefix("HostName ").unwrap().to_string());
            } else if trimmed.starts_with("User ") {
                current_user = Some(trimmed.strip_prefix("User ").unwrap().to_string());
            } else if trimmed.starts_with("IdentityFile ") {
                current_identity = Some(trimmed.strip_prefix("IdentityFile ").unwrap().to_string());
                // End of this host block
                if let (Some(h), Some(hn), Some(u), Some(id)) = (
                    current_host.take(),
                    current_hostname.take(),
                    current_user.take(),
                    current_identity.take(),
                ) {
                    hosts.push(PairedHost {
                        host: h,
                        hostname: hn,
                        user: u,
                        identity_file: id,
                    });
                }
                in_connecto_block = false;
            } else if trimmed.is_empty() {
                in_connecto_block = false;
            }
        }
    }

    // Handle last host if still pending
    if let (Some(h), Some(hn), Some(u), Some(id)) = (
        current_host,
        current_hostname,
        current_user,
        current_identity,
    ) {
        hosts.push(PairedHost {
            host: h,
            hostname: hn,
            user: u,
            identity_file: id,
        });
    }

    Ok(hosts)
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

/// Helper function to get SSH directory
fn get_ssh_dir() -> Result<std::path::PathBuf, String> {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map_err(|_| "HOME/USERPROFILE not set".to_string())?;
    Ok(std::path::PathBuf::from(&home).join(".ssh"))
}

/// Parse public key to extract algorithm, comment, and fingerprint
fn parse_public_key_info(public_key_content: &str) -> (String, String, String) {
    let parts: Vec<&str> = public_key_content.split_whitespace().collect();

    let algorithm = parts.first().unwrap_or(&"unknown").to_string();
    let comment = if parts.len() > 2 {
        parts[2..].join(" ")
    } else {
        String::new()
    };

    // Calculate fingerprint from the key data
    let fingerprint = if parts.len() >= 2 {
        // Base64 decode the key data and hash it
        if let Ok(key_bytes) = base64_decode(parts[1]) {
            let hash = sha256_hash(&key_bytes);
            format!("SHA256:{}", base64_encode_no_padding(&hash))
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    (algorithm, comment, fingerprint)
}

/// Simple base64 decode (for fingerprint calculation)
fn base64_decode(input: &str) -> Result<Vec<u8>, ()> {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    let input = input.trim_end_matches('=');
    let mut output = Vec::with_capacity(input.len() * 3 / 4);
    let mut buffer = 0u32;
    let mut bits = 0;

    for &byte in input.as_bytes() {
        let val = ALPHABET.iter().position(|&c| c == byte).ok_or(())?;
        buffer = (buffer << 6) | (val as u32);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            output.push((buffer >> bits) as u8);
        }
    }

    Ok(output)
}

/// Simple SHA256 hash
fn sha256_hash(data: &[u8]) -> [u8; 32] {
    use std::num::Wrapping;

    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];

    let mut h: [Wrapping<u32>; 8] = [
        Wrapping(0x6a09e667),
        Wrapping(0xbb67ae85),
        Wrapping(0x3c6ef372),
        Wrapping(0xa54ff53a),
        Wrapping(0x510e527f),
        Wrapping(0x9b05688c),
        Wrapping(0x1f83d9ab),
        Wrapping(0x5be0cd19),
    ];

    // Padding
    let ml = (data.len() as u64) * 8;
    let mut padded = data.to_vec();
    padded.push(0x80);
    while (padded.len() % 64) != 56 {
        padded.push(0);
    }
    padded.extend_from_slice(&ml.to_be_bytes());

    // Process blocks
    for chunk in padded.chunks(64) {
        let mut w = [0u32; 64];
        for (i, word) in chunk.chunks(4).enumerate() {
            w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let (mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh) =
            (h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]);

        for i in 0..64 {
            let s1 = Wrapping(e.0.rotate_right(6) ^ e.0.rotate_right(11) ^ e.0.rotate_right(25));
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = hh + s1 + ch + Wrapping(K[i]) + Wrapping(w[i]);
            let s0 = Wrapping(a.0.rotate_right(2) ^ a.0.rotate_right(13) ^ a.0.rotate_right(22));
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0 + maj;

            hh = g;
            g = f;
            f = e;
            e = d + temp1;
            d = c;
            c = b;
            b = a;
            a = temp1 + temp2;
        }

        h[0] += a;
        h[1] += b;
        h[2] += c;
        h[3] += d;
        h[4] += e;
        h[5] += f;
        h[6] += g;
        h[7] += hh;
    }

    let mut result = [0u8; 32];
    for (i, &val) in h.iter().enumerate() {
        result[i * 4..(i + 1) * 4].copy_from_slice(&val.0.to_be_bytes());
    }
    result
}

/// Base64 encode without padding (for fingerprint display)
fn base64_encode_no_padding(data: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    let mut result = String::new();
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as usize;
        let b1 = chunk.get(1).copied().unwrap_or(0) as usize;
        let b2 = chunk.get(2).copied().unwrap_or(0) as usize;

        result.push(ALPHABET[b0 >> 2] as char);
        result.push(ALPHABET[((b0 & 0x03) << 4) | (b1 >> 4)] as char);

        if chunk.len() > 1 {
            result.push(ALPHABET[((b1 & 0x0f) << 2) | (b2 >> 6)] as char);
        }
        if chunk.len() > 2 {
            result.push(ALPHABET[b2 & 0x3f] as char);
        }
    }
    result
}

/// List local SSH keys in the given directory (for testing)
pub fn list_local_keys_in_dir(ssh_dir: &std::path::Path) -> Result<Vec<LocalKeyInfo>, String> {
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
pub fn delete_local_key_in_dir(ssh_dir: &std::path::Path, name: &str) -> Result<(), String> {
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
pub fn get_key_details_in_dir(
    ssh_dir: &std::path::Path,
    name: &str,
) -> Result<LocalKeyInfo, String> {
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
    ssh_dir: &std::path::Path,
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

    #[test]
    fn test_get_addresses() {
        // This may return empty on some systems
        let _ = get_addresses();
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
    fn test_list_local_keys_finds_key_pairs() {
        let temp_dir = TempDir::new().unwrap();
        let ssh_dir = temp_dir.path().join(".ssh");
        std::fs::create_dir_all(&ssh_dir).unwrap();

        // Create a test key pair
        let private_key = r#"-----BEGIN OPENSSH PRIVATE KEY-----
b3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQAAAAAAAAABAAAAMwAAAAtzc2gtZW
QyNTUxOQAAACBHK9toTP8HtIHvBB3X6bq5LYTTLIl4nC28HqWGJcZoGwAAAJgPP4xYDz+M
WAAAAAtzc2gtZWQyNTUxOQAAACBHK9toTP8HtIHvBB3X6bq5LYTTLIl4nC28HqWGJcZoGw
AAAEB/qvjQ6fU+2xYYZM3BkllsQYYTQrjglCgbwW0WO1iXP0cr22hM/we0ge8EHdfpurkt
hNMsiXicLbwepYYlxmgbAAAADnRlc3RAY29ubmVjdG8BAgMEBQ==
-----END OPENSSH PRIVATE KEY-----"#;
        let public_key = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIEcr22hM/we0ge8EHdfpurkthNMsiXicLbwepYYlxmgb test@connecto";

        std::fs::write(ssh_dir.join("test_key"), private_key).unwrap();
        std::fs::write(ssh_dir.join("test_key.pub"), public_key).unwrap();

        let keys = list_local_keys_in_dir(&ssh_dir).unwrap();
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].name, "test_key");
        assert_eq!(keys[0].algorithm, "ssh-ed25519");
        assert_eq!(keys[0].comment, "test@connecto");
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
    fn test_list_local_keys_multiple_keys() {
        let temp_dir = TempDir::new().unwrap();
        let ssh_dir = temp_dir.path().join(".ssh");
        std::fs::create_dir_all(&ssh_dir).unwrap();

        // Create multiple key pairs
        let private_key = r#"-----BEGIN OPENSSH PRIVATE KEY-----
b3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQAAAAAAAAABAAAAMwAAAAtzc2gtZW
QyNTUxOQAAACBHK9toTP8HtIHvBB3X6bq5LYTTLIl4nC28HqWGJcZoGwAAAJgPP4xYDz+M
WAAAAAtzc2gtZWQyNTUxOQAAACBHK9toTP8HtIHvBB3X6bq5LYTTLIl4nC28HqWGJcZoGw
AAAEB/qvjQ6fU+2xYYZM3BkllsQYYTQrjglCgbwW0WO1iXP0cr22hM/we0ge8EHdfpurkt
hNMsiXicLbwepYYlxmgbAAAADnRlc3RAY29ubmVjdG8BAgMEBQ==
-----END OPENSSH PRIVATE KEY-----"#;

        std::fs::write(ssh_dir.join("id_ed25519"), private_key).unwrap();
        std::fs::write(ssh_dir.join("id_ed25519.pub"), "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIEcr22hM/we0ge8EHdfpurkthNMsiXicLbwepYYlxmgb user@host1").unwrap();

        std::fs::write(ssh_dir.join("connecto_server"), private_key).unwrap();
        std::fs::write(ssh_dir.join("connecto_server.pub"), "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIEcr22hM/we0ge8EHdfpurkthNMsiXicLbwepYYlxmgb user@host2").unwrap();

        let keys = list_local_keys_in_dir(&ssh_dir).unwrap();
        assert_eq!(keys.len(), 2);
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
    fn test_get_key_details_returns_info() {
        let temp_dir = TempDir::new().unwrap();
        let ssh_dir = temp_dir.path().join(".ssh");
        std::fs::create_dir_all(&ssh_dir).unwrap();

        let private_key = r#"-----BEGIN OPENSSH PRIVATE KEY-----
b3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQAAAAAAAAABAAAAMwAAAAtzc2gtZW
QyNTUxOQAAACBHK9toTP8HtIHvBB3X6bq5LYTTLIl4nC28HqWGJcZoGwAAAJgPP4xYDz+M
WAAAAAtzc2gtZWQyNTUxOQAAACBHK9toTP8HtIHvBB3X6bq5LYTTLIl4nC28HqWGJcZoGw
AAAEB/qvjQ6fU+2xYYZM3BkllsQYYTQrjglCgbwW0WO1iXP0cr22hM/we0ge8EHdfpurkt
hNMsiXicLbwepYYlxmgbAAAADnRlc3RAY29ubmVjdG8BAgMEBQ==
-----END OPENSSH PRIVATE KEY-----"#;
        let public_key = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIEcr22hM/we0ge8EHdfpurkthNMsiXicLbwepYYlxmgb test@connecto";

        std::fs::write(ssh_dir.join("my_key"), private_key).unwrap();
        std::fs::write(ssh_dir.join("my_key.pub"), public_key).unwrap();

        let details = get_key_details_in_dir(&ssh_dir, "my_key").unwrap();
        assert_eq!(details.name, "my_key");
        assert_eq!(details.algorithm, "ssh-ed25519");
        assert_eq!(details.comment, "test@connecto");
        assert!(details.private_key_path.contains("my_key"));
        assert!(details.public_key_path.contains("my_key.pub"));
        assert!(!details.fingerprint.is_empty());
    }

    #[test]
    fn test_rename_local_key() {
        let temp_dir = TempDir::new().unwrap();
        let ssh_dir = temp_dir.path().join(".ssh");
        std::fs::create_dir_all(&ssh_dir).unwrap();

        let private_key = r#"-----BEGIN OPENSSH PRIVATE KEY-----
b3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQAAAAAAAAABAAAAMwAAAAtzc2gtZW
QyNTUxOQAAACBHK9toTP8HtIHvBB3X6bq5LYTTLIl4nC28HqWGJcZoGwAAAJgPP4xYDz+M
WAAAAAtzc2gtZWQyNTUxOQAAACBHK9toTP8HtIHvBB3X6bq5LYTTLIl4nC28HqWGJcZoGw
AAAEB/qvjQ6fU+2xYYZM3BkllsQYYTQrjglCgbwW0WO1iXP0cr22hM/we0ge8EHdfpurkt
hNMsiXicLbwepYYlxmgbAAAADnRlc3RAY29ubmVjdG8BAgMEBQ==
-----END OPENSSH PRIVATE KEY-----"#;
        let public_key = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIEcr22hM/we0ge8EHdfpurkthNMsiXicLbwepYYlxmgb test@connecto";

        std::fs::write(ssh_dir.join("old_name"), private_key).unwrap();
        std::fs::write(ssh_dir.join("old_name.pub"), public_key).unwrap();

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
