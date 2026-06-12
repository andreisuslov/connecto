//! Linux command primitives for ad-hoc WiFi networks
//!
//! Uses nmcli/NetworkManager with an `iw` fallback (requires root). Driven
//! exclusively by the shared `AdHocNetwork` orchestration in the parent
//! module. Everything here is blocking; async callers go through
//! `spawn_blocking` at a higher level.

use super::{
    AdHocBackend, AdHocOutcome, ADHOC_CLIENT_IP, ADHOC_HOST_IP, ADHOC_NETWORK_PREFIX,
    ADHOC_PREFIX_LEN,
};
use crate::error::{ConnectoError, Result};
use std::process::Command;
use tracing::{debug, info, warn};

pub(super) struct LinuxBackend {
    interface: Option<String>,
    /// nmcli connection we created (deleted on restore)
    connection_name: Option<String>,
    /// Whether the `iw` IBSS fallback was used (left on restore)
    used_iw: bool,
    /// IP we added via `ip addr add` (removed on restore)
    configured_ip: Option<String>,
}

impl LinuxBackend {
    pub(super) fn new() -> Self {
        Self {
            interface: None,
            connection_name: None,
            used_iw: false,
            configured_ip: None,
        }
    }

    /// Detect (and cache) the WiFi interface instead of assuming `wlan0`
    fn interface(&mut self) -> Result<String> {
        if let Some(ref interface) = self.interface {
            return Ok(interface.clone());
        }
        let detected = detect_wifi_interface().ok_or_else(|| {
            ConnectoError::Network(
                "No WiFi interface found. Install NetworkManager: sudo apt install network-manager"
                    .to_string(),
            )
        })?;
        debug!("Detected WiFi interface: {}", detected);
        self.interface = Some(detected.clone());
        Ok(detected)
    }

    /// Create ad-hoc network using nmcli (NetworkManager)
    fn create_network_nmcli(&mut self, ssid: &str, interface: &str) -> Result<()> {
        // Delete any existing connection with this name
        let _ = Command::new("nmcli")
            .args(["connection", "delete", ssid])
            .output();

        // Create ad-hoc connection
        let output = Command::new("nmcli")
            .args([
                "connection",
                "add",
                "type",
                "wifi",
                "ifname",
                interface,
                "mode",
                "adhoc",
                "ssid",
                ssid,
                "con-name",
                ssid,
            ])
            .output()
            .map_err(|e| ConnectoError::Network(format!("nmcli add failed: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ConnectoError::Network(format!(
                "nmcli connection add failed: {}",
                stderr
            )));
        }

        // The connection profile exists from here on; make sure restore
        // deletes it even if bringing it up fails below.
        self.connection_name = Some(ssid.to_string());

        // Configure static IP (lives inside the NM connection profile, so
        // deleting the connection on restore also removes it)
        let _ = Command::new("nmcli")
            .args([
                "connection",
                "modify",
                ssid,
                "ipv4.method",
                "manual",
                "ipv4.addresses",
                &format!("{}/{}", ADHOC_HOST_IP, ADHOC_PREFIX_LEN),
            ])
            .output();

        // Bring up the connection
        let up_output = Command::new("nmcli")
            .args(["connection", "up", ssid])
            .output()
            .map_err(|e| ConnectoError::Network(format!("nmcli up failed: {}", e)))?;

        if !up_output.status.success() {
            let stderr = String::from_utf8_lossy(&up_output.stderr);
            return Err(ConnectoError::Network(format!(
                "nmcli connection up failed: {}",
                stderr
            )));
        }

        Ok(())
    }

    /// Create ad-hoc network using iw (requires root)
    fn create_network_iw(&mut self, ssid: &str, interface: &str) -> Result<()> {
        // First, bring down any existing connection
        let _ = Command::new("ip")
            .args(["link", "set", interface, "down"])
            .output();

        // Set interface to IBSS (ad-hoc) mode
        let output = Command::new("iw")
            .args([
                "dev", interface, "ibss", "join", ssid,
                "2462", // ADHOC_CHANNEL (11) frequency in MHz
            ])
            .output()
            .map_err(|e| ConnectoError::Network(format!("iw ibss join failed: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ConnectoError::Network(format!("iw failed: {}", stderr)));
        }

        self.used_iw = true;

        // Bring interface up
        let _ = Command::new("ip")
            .args(["link", "set", interface, "up"])
            .output();

        // Configure IP address
        self.configure_adhoc_ip(ADHOC_HOST_IP, interface)?;

        Ok(())
    }

    /// Configure IP address for ad-hoc network
    fn configure_adhoc_ip(&mut self, ip: &str, interface: &str) -> Result<()> {
        // Flush existing IPs
        let _ = Command::new("ip")
            .args(["addr", "flush", "dev", interface])
            .output();

        // Add new IP
        let output = Command::new("ip")
            .args([
                "addr",
                "add",
                &format!("{}/{}", ip, ADHOC_PREFIX_LEN),
                "dev",
                interface,
            ])
            .output()
            .map_err(|e| ConnectoError::Network(format!("Failed to configure IP: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // Ignore "file exists" error (IP already configured)
            if !stderr.contains("File exists") {
                return Err(ConnectoError::Network(format!(
                    "Failed to set IP {}: {}",
                    ip,
                    stderr.trim()
                )));
            }
        }

        self.configured_ip = Some(ip.to_string());
        Ok(())
    }
}

impl AdHocBackend for LinuxBackend {
    fn current_network(&mut self) -> Result<Option<String>> {
        let interface = self.interface()?;

        let output = Command::new("nmcli")
            .args(["-t", "-f", "NAME,DEVICE", "connection", "show", "--active"])
            .output()
            .map_err(|e| ConnectoError::Network(format!("Failed to get current network: {}", e)))?;

        let stdout = String::from_utf8_lossy(&output.stdout);

        // Parse output format: "NetworkName:wlan0"
        for line in stdout.lines() {
            if line.contains(&interface) {
                if let Some(name) = line.split(':').next() {
                    return Ok(Some(name.to_string()));
                }
            }
        }

        Ok(None)
    }

    fn create_network(&mut self, ssid: &str) -> Result<AdHocOutcome> {
        let interface = self.interface()?;

        // Try nmcli first (requires NetworkManager)
        match self.create_network_nmcli(ssid, &interface) {
            Ok(()) => {
                info!("Ad-hoc network '{}' created via nmcli", ssid);
                // The static IP lives inside the NM connection profile;
                // deleting the connection on restore undoes it.
                return Ok(AdHocOutcome {
                    static_ip_configured: false,
                    warning: None,
                });
            }
            Err(e) => {
                warn!("nmcli failed: {}, trying iw fallback", e);
                // Drop the half-created nmcli connection before falling back
                if let Some(name) = self.connection_name.take() {
                    let _ = Command::new("nmcli")
                        .args(["connection", "delete", &name])
                        .output();
                }
            }
        }

        // Fallback to iw (requires root)
        match self.create_network_iw(ssid, &interface) {
            Ok(()) => {
                info!("Ad-hoc network '{}' created via iw", ssid);
                Ok(AdHocOutcome {
                    static_ip_configured: true,
                    warning: None,
                })
            }
            Err(e) => Err(ConnectoError::Network(format!(
                "Failed to create ad-hoc network: {}. \
                 \nTry running with sudo, or add yourself to the 'netdev' group:\n  \
                 sudo usermod -aG netdev $USER\n  \
                 (then log out and back in)",
                e
            ))),
        }
    }

    fn join_network(&mut self, ssid: &str) -> Result<AdHocOutcome> {
        let interface = self.interface()?;

        let output = Command::new("nmcli")
            .args(["device", "wifi", "connect", ssid])
            .output()
            .map_err(|e| ConnectoError::Network(format!("Failed to join network: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ConnectoError::Network(format!(
                "Failed to join network: {}",
                stderr
            )));
        }

        // Configure IP for client (different from host)
        match self.configure_adhoc_ip(ADHOC_CLIENT_IP, &interface) {
            Ok(()) => Ok(AdHocOutcome {
                static_ip_configured: true,
                warning: None,
            }),
            Err(e) => Ok(AdHocOutcome {
                static_ip_configured: false,
                warning: Some(format!(
                    "Joined '{}' but could not configure the rendezvous IP {}: {}. \
                     Assign it manually: sudo ip addr add {}/{} dev {}",
                    ssid, ADHOC_CLIENT_IP, e, ADHOC_CLIENT_IP, ADHOC_PREFIX_LEN, interface
                )),
            }),
        }
    }

    // `_static_ip_configured` mirrors `configured_ip` on Linux: the backend
    // tracks precisely which IP it pinned (and on which interface), which is
    // what the removal below needs.
    fn restore(
        &mut self,
        previous_network: Option<&str>,
        _static_ip_configured: bool,
    ) -> Result<()> {
        // Delete the ad-hoc connection if we created one via nmcli
        if let Some(name) = self.connection_name.take() {
            let _ = Command::new("nmcli")
                .args(["connection", "delete", &name])
                .output();
        }

        // If using iw, leave the IBSS network
        if self.used_iw {
            if let Some(ref interface) = self.interface {
                let _ = Command::new("iw")
                    .args(["dev", interface, "ibss", "leave"])
                    .output();
            }
            self.used_iw = false;
        }

        // Remove the static IP we pinned via `ip addr add`
        if let (Some(ip), Some(interface)) = (self.configured_ip.take(), self.interface.as_deref())
        {
            let _ = Command::new("ip")
                .args([
                    "addr",
                    "del",
                    &format!("{}/{}", ip, ADHOC_PREFIX_LEN),
                    "dev",
                    interface,
                ])
                .output();
        }

        // Reconnect to previous network if we have one
        if let Some(previous) = previous_network {
            info!("Restoring previous network: {}", previous);

            let output = Command::new("nmcli")
                .args(["connection", "up", previous])
                .output()
                .map_err(|e| ConnectoError::Network(format!("Failed to restore network: {}", e)))?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                warn!("Failed to restore network: {}", stderr);
            }
        }

        Ok(())
    }
}

/// Scan for connecto ad-hoc networks
pub(super) fn scan_for_networks() -> Result<Vec<String>> {
    let mut networks = Vec::new();

    // Try nmcli first
    if let Ok(output) = Command::new("nmcli")
        .args(["-t", "-f", "SSID", "device", "wifi", "list"])
        .output()
    {
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            let ssid = line.trim();
            if ssid.starts_with(ADHOC_NETWORK_PREFIX) && !networks.contains(&ssid.to_string()) {
                networks.push(ssid.to_string());
            }
        }
    }

    // Fallback to iw scan (requires root typically), on the detected
    // interface rather than a hardcoded "wlan0"
    if networks.is_empty() {
        if let Some(interface) = detect_wifi_interface() {
            if let Ok(output) = Command::new("iw")
                .args(["dev", &interface, "scan"])
                .output()
            {
                let stdout = String::from_utf8_lossy(&output.stdout);
                for line in stdout.lines() {
                    let trimmed = line.trim();
                    if let Some(ssid) = trimmed.strip_prefix("SSID:") {
                        let ssid = ssid.trim().to_string();
                        if ssid.starts_with(ADHOC_NETWORK_PREFIX) && !networks.contains(&ssid) {
                            networks.push(ssid);
                        }
                    }
                }
            }
        }
    }

    debug!("Found {} connecto ad-hoc networks", networks.len());
    Ok(networks)
}

/// Get the WiFi interface name (nmcli first, then iw)
fn detect_wifi_interface() -> Option<String> {
    // Try nmcli first
    if let Ok(output) = Command::new("nmcli")
        .args(["-t", "-f", "DEVICE,TYPE", "device", "status"])
        .output()
    {
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            let parts: Vec<&str> = line.split(':').collect();
            if parts.len() >= 2 && parts[1] == "wifi" {
                return Some(parts[0].to_string());
            }
        }
    }

    // Fallback to iw
    if let Ok(output) = Command::new("iw").args(["dev"]).output() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines() {
            let trimmed = line.trim();
            if let Some(interface) = trimmed.strip_prefix("Interface ") {
                return Some(interface.to_string());
            }
        }
    }

    None
}
