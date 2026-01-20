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

#[cfg(test)]
mod tests {
    use super::*;

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
}
