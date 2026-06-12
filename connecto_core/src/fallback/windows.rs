//! Windows command primitives for ad-hoc WiFi networks
//!
//! Uses `netsh wlan` hosted networks. Driven exclusively by the shared
//! `AdHocNetwork` orchestration in the parent module. Everything here is
//! blocking; async callers go through `spawn_blocking` at a higher level.

use super::{
    AdHocBackend, AdHocOutcome, ADHOC_CLIENT_IP, ADHOC_HOST_IP, ADHOC_NETMASK, ADHOC_NETWORK_PREFIX,
};
use crate::error::{ConnectoError, Result};
use std::process::Command;
use std::time::Duration;
use tracing::{debug, info, warn};

pub(super) struct WindowsBackend {
    password: String,
    /// The hosted network was started (stopped on restore)
    hosting: bool,
    /// Interface we pinned a static IP on (reset to DHCP on restore)
    configured_interface: Option<String>,
}

impl WindowsBackend {
    pub(super) fn new(password: String) -> Self {
        Self {
            password,
            hosting: false,
            configured_interface: None,
        }
    }

    /// Pin a static IP on the rendezvous subnet
    fn configure_static_ip(&mut self, interface: &str, ip: &str) -> Result<()> {
        let output = Command::new("netsh")
            .args([
                "interface",
                "ipv4",
                "set",
                "address",
                &format!("name={}", interface),
                "static",
                ip,
                ADHOC_NETMASK,
            ])
            .output()
            .map_err(|e| ConnectoError::Network(format!("Failed to configure IP: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            return Err(ConnectoError::Network(format!(
                "Failed to set static IP {} on '{}': {} {}",
                ip,
                interface,
                stdout.trim(),
                stderr.trim()
            )));
        }

        self.configured_interface = Some(interface.to_string());
        Ok(())
    }
}

impl AdHocBackend for WindowsBackend {
    fn current_network(&mut self) -> Result<Option<String>> {
        let output = Command::new("netsh")
            .args(["wlan", "show", "interfaces"])
            .output()
            .map_err(|e| ConnectoError::Network(format!("Failed to get current network: {}", e)))?;

        let stdout = String::from_utf8_lossy(&output.stdout);

        // Parse output to find current SSID
        for line in stdout.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("SSID") && !trimmed.contains("BSSID") {
                if let Some(ssid) = trimmed.split(':').nth(1) {
                    let network = ssid.trim().to_string();
                    if !network.is_empty() {
                        return Ok(Some(network));
                    }
                }
                break;
            }
        }

        Ok(None)
    }

    /// Create and host an ad-hoc network (requires admin privileges)
    fn create_network(&mut self, ssid: &str) -> Result<AdHocOutcome> {
        // Check for hosted network support
        check_hosted_network_support()?;

        // Configure the hosted network
        let output = Command::new("netsh")
            .args([
                "wlan",
                "set",
                "hostednetwork",
                "mode=allow",
                &format!("ssid={}", ssid),
                &format!("key={}", self.password),
            ])
            .output()
            .map_err(|e| ConnectoError::Network(format!("netsh set failed: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            // Check for admin privileges error
            if stdout.contains("administrator") || stderr.contains("administrator") {
                return Err(ConnectoError::Network(
                    "Administrator privileges required.\n\
                     Right-click terminal and select 'Run as Administrator'."
                        .to_string(),
                ));
            }
            return Err(ConnectoError::Network(format!(
                "Failed to configure hosted network: {} {}",
                stdout, stderr
            )));
        }

        // Start the hosted network
        let start_output = Command::new("netsh")
            .args(["wlan", "start", "hostednetwork"])
            .output()
            .map_err(|e| ConnectoError::Network(format!("netsh start failed: {}", e)))?;

        if !start_output.status.success() {
            let stderr = String::from_utf8_lossy(&start_output.stderr);
            let stdout = String::from_utf8_lossy(&start_output.stdout);
            return Err(ConnectoError::Network(format!(
                "Failed to start hosted network: {} {}",
                stdout, stderr
            )));
        }

        self.hosting = true;
        info!("Hosted network '{}' started", ssid);

        // Find the hosted network's virtual adapter and pin the host IP.
        // No hardcoded adapter-name guess: when the adapter cannot be
        // identified we report it instead of configuring a random interface.
        match find_hosted_network_adapter() {
            Some(adapter) => match self.configure_static_ip(&adapter, ADHOC_HOST_IP) {
                Ok(()) => Ok(AdHocOutcome {
                    static_ip_configured: true,
                    warning: None,
                }),
                Err(e) => Ok(AdHocOutcome {
                    static_ip_configured: false,
                    warning: Some(format!(
                        "Hosted network started, but the host IP could not be configured: {}. \
                         Assign {} / {} to adapter '{}' manually.",
                        e, ADHOC_HOST_IP, ADHOC_NETMASK, adapter
                    )),
                }),
            },
            None => Ok(AdHocOutcome {
                static_ip_configured: false,
                warning: Some(format!(
                    "Hosted network started, but its virtual adapter could not be identified. \
                     Assign {} / {} to the hosted network adapter manually \
                     (see 'netsh interface show interface').",
                    ADHOC_HOST_IP, ADHOC_NETMASK
                )),
            }),
        }
    }

    fn join_network(&mut self, ssid: &str) -> Result<AdHocOutcome> {
        // Connect to the network (will prompt for password if needed)
        let output = Command::new("netsh")
            .args(["wlan", "connect", &format!("name={}", ssid)])
            .output()
            .map_err(|e| ConnectoError::Network(format!("Failed to join network: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            return Err(ConnectoError::Network(format!(
                "Failed to join network: {} {}",
                stdout, stderr
            )));
        }

        // `netsh wlan connect` returns before association completes; wait for
        // the WLAN interface to report the joined SSID, then pin the client
        // IP on the rendezvous subnet so the host at 192.168.73.1 is
        // actually reachable.
        match wait_for_interface_on_ssid(ssid) {
            Some(interface) => match self.configure_static_ip(&interface, ADHOC_CLIENT_IP) {
                Ok(()) => Ok(AdHocOutcome {
                    static_ip_configured: true,
                    warning: None,
                }),
                Err(e) => Ok(AdHocOutcome {
                    static_ip_configured: false,
                    warning: Some(format!(
                        "Joined '{}' but could not configure the rendezvous IP \
                         (administrator privileges may be required): {}. \
                         Assign {} / {} to interface '{}' manually.",
                        ssid, e, ADHOC_CLIENT_IP, ADHOC_NETMASK, interface
                    )),
                }),
            },
            None => Ok(AdHocOutcome {
                static_ip_configured: false,
                warning: Some(format!(
                    "Joined '{}' but the WLAN interface could not be identified, so no IP was \
                     configured on the rendezvous subnet. Assign {} / {} manually or the host \
                     at {} will be unreachable.",
                    ssid, ADHOC_CLIENT_IP, ADHOC_NETMASK, ADHOC_HOST_IP
                )),
            }),
        }
    }

    // `_static_ip_configured` mirrors `configured_interface` on Windows: the
    // backend tracks precisely which interface it pinned, which is what the
    // DHCP reset below needs.
    fn restore(
        &mut self,
        previous_network: Option<&str>,
        _static_ip_configured: bool,
    ) -> Result<()> {
        // Stop the hosted network
        if self.hosting {
            let _ = Command::new("netsh")
                .args(["wlan", "stop", "hostednetwork"])
                .output();

            let _ = Command::new("netsh")
                .args(["wlan", "set", "hostednetwork", "mode=disallow"])
                .output();

            self.hosting = false;
        }

        // Reset the interface we pinned back to DHCP
        if let Some(interface) = self.configured_interface.take() {
            let output = Command::new("netsh")
                .args([
                    "interface",
                    "ipv4",
                    "set",
                    "address",
                    &format!("name={}", interface),
                    "source=dhcp",
                ])
                .output();

            match output {
                Ok(out) if out.status.success() => {}
                Ok(out) => {
                    // The hosted-network virtual adapter disappears once the
                    // network is stopped; failing to reset it is fine.
                    let stderr = String::from_utf8_lossy(&out.stderr);
                    let stdout = String::from_utf8_lossy(&out.stdout);
                    debug!(
                        "Could not reset '{}' to DHCP: {} {}",
                        interface,
                        stdout.trim(),
                        stderr.trim()
                    );
                }
                Err(e) => {
                    warn!(
                        "Failed to run netsh to reset '{}' to DHCP: {}",
                        interface, e
                    );
                }
            }
        }

        // Reconnect to previous network if we have one
        if let Some(previous) = previous_network {
            // Skip when still associated with it (e.g. restore after a
            // failure that never switched networks)
            let already_there = matches!(self.current_network(), Ok(Some(ref s)) if s == previous);

            if !already_there {
                info!("Restoring previous network: {}", previous);

                let output = Command::new("netsh")
                    .args(["wlan", "connect", &format!("name={}", previous)])
                    .output()
                    .map_err(|e| {
                        ConnectoError::Network(format!("Failed to restore network: {}", e))
                    })?;

                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    warn!("Failed to restore network: {}", stderr);
                }
            }
        }

        Ok(())
    }
}

/// Scan for connecto ad-hoc networks
pub(super) fn scan_for_networks() -> Result<Vec<String>> {
    let mut networks = Vec::new();

    let output = Command::new("netsh")
        .args(["wlan", "show", "networks", "mode=bssid"])
        .output()
        .map_err(|e| ConnectoError::Network(format!("Failed to scan networks: {}", e)))?;

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Parse output to find SSIDs starting with our prefix
    for line in stdout.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("SSID") && !trimmed.contains("BSSID") {
            if let Some(ssid) = trimmed.split(':').nth(1) {
                let network = ssid.trim().to_string();
                if network.starts_with(ADHOC_NETWORK_PREFIX) && !networks.contains(&network) {
                    networks.push(network);
                }
            }
        }
    }

    debug!("Found {} connecto ad-hoc networks", networks.len());
    Ok(networks)
}

/// Check if the WiFi adapter supports hosted network
fn check_hosted_network_support() -> Result<()> {
    let output = Command::new("netsh")
        .args(["wlan", "show", "drivers"])
        .output()
        .map_err(|e| ConnectoError::Network(format!("Failed to check drivers: {}", e)))?;

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Look for "Hosted network supported : Yes"
    let supported = stdout
        .lines()
        .any(|line| line.contains("Hosted network supported") && line.contains("Yes"));

    if !supported {
        return Err(ConnectoError::Network(
            "WiFi adapter does not support Hosted Network.\n\
             Check: netsh wlan show drivers (look for 'Hosted network supported: Yes')"
                .to_string(),
        ));
    }

    Ok(())
}

/// Find the virtual adapter created for the hosted network
///
/// Returns `None` when it cannot be identified; callers must not guess.
fn find_hosted_network_adapter() -> Option<String> {
    let output = Command::new("netsh")
        .args(["wlan", "show", "hostednetwork"])
        .output()
        .ok()?;

    let stdout = String::from_utf8_lossy(&output.stdout);

    // The hosted network surfaces as a "Local Area Connection*" interface
    let interface_output = Command::new("netsh")
        .args(["interface", "show", "interface"])
        .output()
        .ok()?;

    let interface_stdout = String::from_utf8_lossy(&interface_output.stdout);

    for line in interface_stdout.lines() {
        if line.contains("Local Area Connection")
            && (line.contains("Hosted") || stdout.contains("Started"))
        {
            // Extract interface name (last column)
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 4 {
                return Some(parts[3..].join(" "));
            }
        }
    }

    None
}

/// Wait until `netsh wlan show interfaces` lists an interface associated
/// with `ssid`, returning that interface's name
fn wait_for_interface_on_ssid(ssid: &str) -> Option<String> {
    for _ in 0..5 {
        std::thread::sleep(Duration::from_secs(1));
        if let Some(interface) = interface_for_ssid(ssid) {
            return Some(interface);
        }
    }
    None
}

/// Parse `netsh wlan show interfaces` for the interface joined to `ssid`
fn interface_for_ssid(ssid: &str) -> Option<String> {
    let output = Command::new("netsh")
        .args(["wlan", "show", "interfaces"])
        .output()
        .ok()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut current_name: Option<String> = None;

    for line in stdout.lines() {
        let trimmed = line.trim();
        // Each interface block starts with "Name : <interface>".
        // Note: field labels are localized on non-English Windows; when
        // parsing fails the caller falls back to an explicit warning.
        if trimmed.starts_with("Name") {
            if let Some(value) = trimmed.split(':').nth(1) {
                current_name = Some(value.trim().to_string());
            }
        } else if trimmed.starts_with("SSID") && !trimmed.contains("BSSID") {
            if let Some(value) = trimmed.split(':').nth(1) {
                if value.trim() == ssid {
                    return current_name;
                }
            }
        }
    }

    None
}
