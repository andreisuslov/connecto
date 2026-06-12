//! macOS command primitives for ad-hoc WiFi networks
//!
//! Driven exclusively by the shared `AdHocNetwork` orchestration in the
//! parent module. Everything here is blocking; async callers go through
//! `spawn_blocking` at a higher level.
//!
//! Platform notes (live-verified on macOS 15.7):
//! - The private `airport` utility is a no-op stub since macOS 14.4: it
//!   exits 0 for any arguments without doing anything, so creation is
//!   version-gated and must be positively confirmed on older systems.
//! - `networksetup -getairportnetwork` reports "not associated" even while
//!   associated on modern macOS; the current SSID is read from
//!   `ipconfig getsummary` instead (which redacts the name to `<redacted>`
//!   without location permission — treated as "unknown").

use super::{
    AdHocBackend, AdHocOutcome, ADHOC_CHANNEL, ADHOC_CLIENT_IP, ADHOC_HOST_IP, ADHOC_NETMASK,
    ADHOC_NETWORK_PREFIX,
};
use crate::error::{ConnectoError, Result};
use std::process::Command;
use std::time::Duration;
use tracing::{debug, info, warn};

/// Path of the legacy `airport` utility (a no-op stub since macOS 14.4)
const AIRPORT_PATH: &str =
    "/System/Library/PrivateFrameworks/Apple80211.framework/Versions/Current/Resources/airport";

/// SSID value `ipconfig getsummary` reports when the process lacks
/// location permission (macOS 14+): the name is unknown, not absent
const REDACTED_SSID: &str = "<redacted>";

/// The Wi-Fi hardware port as listed by `networksetup -listallhardwareports`
#[derive(Debug, Clone)]
struct WifiInterface {
    /// Network service name (e.g. "Wi-Fi"), used by `-setmanual`/`-setdhcp`
    service: String,
    /// BSD device name (e.g. "en0"), used by `-setairportnetwork`/`ipconfig`
    device: String,
}

pub(super) struct MacosBackend {
    interface: Option<WifiInterface>,
}

impl MacosBackend {
    pub(super) fn new() -> Self {
        Self { interface: None }
    }

    /// Detect (and cache) the Wi-Fi interface instead of assuming `en0`
    fn interface(&mut self) -> Result<WifiInterface> {
        if let Some(ref interface) = self.interface {
            return Ok(interface.clone());
        }
        let detected = detect_wifi_interface()?;
        debug!(
            "Detected Wi-Fi interface: {} ({})",
            detected.device, detected.service
        );
        self.interface = Some(detected.clone());
        Ok(detected)
    }
}

impl AdHocBackend for MacosBackend {
    fn current_network(&mut self) -> Result<Option<String>> {
        let interface = self.interface()?;
        current_ssid(&interface.device)
    }

    fn create_network(&mut self, ssid: &str) -> Result<AdHocOutcome> {
        let version = macos_product_version()?;
        if super::macos_ibss_unsupported(&version) {
            // Fail fast: `airport --ibss` would exit 0 without creating
            // anything, leaving the user "hosting" a network that does not
            // exist. Nothing has been touched yet at this point.
            return Err(ConnectoError::Network(manual_creation_instructions(
                ssid, &version,
            )));
        }

        let interface = self.interface()?;

        // Disassociate from the current network, then create the IBSS network
        let _ = Command::new(AIRPORT_PATH).args(["-z"]).output();

        let output = Command::new(AIRPORT_PATH)
            .args(["--ibss", ssid, &ADHOC_CHANNEL.to_string()])
            .output()
            .map_err(|e| ConnectoError::Network(format!("Failed to run airport: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ConnectoError::Network(format!(
                "airport --ibss failed: {}",
                stderr.trim()
            )));
        }

        // airport exit codes cannot be trusted; positively confirm the
        // network is up before reporting success.
        if !confirm_associated(&interface.device, ssid) {
            return Err(ConnectoError::Network(format!(
                "airport reported success, but the ad-hoc network '{}' never came up. {}",
                ssid,
                manual_creation_instructions(ssid, &version)
            )));
        }

        configure_static_ip(&interface.service, ADHOC_HOST_IP)?;

        Ok(AdHocOutcome {
            static_ip_configured: true,
            warning: None,
        })
    }

    fn join_network(&mut self, ssid: &str) -> Result<AdHocOutcome> {
        let interface = self.interface()?;

        let output = Command::new("networksetup")
            .args(["-setairportnetwork", &interface.device, ssid])
            .output()
            .map_err(|e| ConnectoError::Network(format!("Failed to join network: {}", e)))?;

        // networksetup reports some join failures on stdout with exit code 0
        let stdout = String::from_utf8_lossy(&output.stdout);
        if !output.status.success()
            || stdout.contains("Could not find network")
            || stdout.contains("Failed to join")
        {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ConnectoError::Network(format!(
                "Failed to join network '{}': {} {}",
                ssid,
                stdout.trim(),
                stderr.trim()
            )));
        }

        // Configure the client side of the rendezvous subnet
        configure_static_ip(&interface.service, ADHOC_CLIENT_IP)?;

        Ok(AdHocOutcome {
            static_ip_configured: true,
            warning: None,
        })
    }

    fn restore(
        &mut self,
        previous_network: Option<&str>,
        static_ip_configured: bool,
    ) -> Result<()> {
        let interface = self.interface()?;

        if let Some(previous) = previous_network {
            // Skip the rejoin when we are still on the previous network
            // (e.g. restore after a fail-fast error that never touched WiFi)
            let already_there =
                matches!(current_ssid(&interface.device), Ok(Some(ref s)) if s == previous);

            if !already_there {
                info!("Restoring previous network: {}", previous);
                let output = Command::new("networksetup")
                    .args(["-setairportnetwork", &interface.device, previous])
                    .output()
                    .map_err(|e| {
                        ConnectoError::Network(format!("Failed to restore network: {}", e))
                    })?;

                if !output.status.success() {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    warn!("Failed to restore network: {}", stderr.trim());
                }
            }
        }

        // Reset DHCP whenever a manual IP was pinned, independent of whether
        // the previous network name was captured (it never is on modern
        // macOS, where the SSID is redacted).
        if static_ip_configured {
            let output = Command::new("networksetup")
                .args(["-setdhcp", &interface.service])
                .output()
                .map_err(|e| ConnectoError::Network(format!("Failed to reset DHCP: {}", e)))?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(ConnectoError::Network(format!(
                    "Failed to reset {} to DHCP: {}",
                    interface.service,
                    stderr.trim()
                )));
            }
        }

        Ok(())
    }
}

/// Scan for connecto ad-hoc networks using system_profiler (works on modern macOS)
pub(super) fn scan_for_networks() -> Result<Vec<String>> {
    // Use system_profiler which works on all macOS versions
    let output = Command::new("system_profiler")
        .args(["SPAirPortDataType", "-json"])
        .output()
        .map_err(|e| ConnectoError::Network(format!("Failed to scan networks: {}", e)))?;

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Parse JSON to find networks starting with our prefix
    let mut networks = Vec::new();

    // Simple string search for network names (avoiding full JSON parsing dependency)
    for line in stdout.lines() {
        let trimmed = line.trim();
        if trimmed.contains(ADHOC_NETWORK_PREFIX) {
            // Extract the network name from JSON-like format
            if let Some(start) = trimmed.find(ADHOC_NETWORK_PREFIX) {
                let rest = &trimmed[start..];
                // Find end of network name (quote or comma)
                let end = rest.find(['"', ',', ':']).unwrap_or(rest.len());
                let network_name = rest[..end].trim().to_string();
                if !network_name.is_empty() && !networks.contains(&network_name) {
                    networks.push(network_name);
                }
            }
        }
    }

    // Fallback: look through known networks recorded in system preferences
    if networks.is_empty() {
        if let Ok(scan_output) = Command::new("defaults")
            .args([
                "read",
                "/Library/Preferences/SystemConfiguration/com.apple.airport.preferences",
                "KnownNetworks",
            ])
            .output()
        {
            let scan_stdout = String::from_utf8_lossy(&scan_output.stdout);
            for line in scan_stdout.lines() {
                let trimmed = line
                    .trim()
                    .trim_matches(|c| c == '"' || c == ';' || c == '=' || c == '{' || c == '}');
                if trimmed.starts_with(ADHOC_NETWORK_PREFIX)
                    && !networks.contains(&trimmed.to_string())
                {
                    networks.push(trimmed.to_string());
                }
            }
        }
    }

    debug!("Found {} connecto ad-hoc networks", networks.len());
    Ok(networks)
}

/// Detect the Wi-Fi hardware port (service name + BSD device)
fn detect_wifi_interface() -> Result<WifiInterface> {
    let output = Command::new("networksetup")
        .args(["-listallhardwareports"])
        .output()
        .map_err(|e| ConnectoError::Network(format!("Failed to list hardware ports: {}", e)))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut service: Option<String> = None;

    for line in stdout.lines() {
        if let Some(name) = line.strip_prefix("Hardware Port:") {
            let name = name.trim();
            service =
                (name.contains("Wi-Fi") || name.contains("AirPort")).then(|| name.to_string());
        } else if let Some(device) = line.strip_prefix("Device:") {
            if let Some(service) = service.take() {
                return Ok(WifiInterface {
                    service,
                    device: device.trim().to_string(),
                });
            }
        }
    }

    Err(ConnectoError::Network(
        "No Wi-Fi interface found on this Mac".to_string(),
    ))
}

/// Read the currently associated SSID via `ipconfig getsummary`
///
/// Returns `None` when not associated or when macOS redacts the name.
fn current_ssid(device: &str) -> Result<Option<String>> {
    let output = Command::new("ipconfig")
        .args(["getsummary", device])
        .output()
        .map_err(|e| ConnectoError::Network(format!("Failed to query WiFi state: {}", e)))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        // Match "  SSID : name" but not "  BSSID : ..."
        if let Some(value) = line.trim().strip_prefix("SSID : ") {
            let ssid = value.trim();
            if ssid.is_empty() || ssid == REDACTED_SSID {
                return Ok(None);
            }
            return Ok(Some(ssid.to_string()));
        }
    }

    Ok(None)
}

/// Poll until the device is associated with `ssid` (positive confirmation)
fn confirm_associated(device: &str, ssid: &str) -> bool {
    for _ in 0..5 {
        std::thread::sleep(Duration::from_secs(1));
        if matches!(current_ssid(device), Ok(Some(ref current)) if current == ssid) {
            return true;
        }
    }
    false
}

/// Pin a static IP on the rendezvous subnet
fn configure_static_ip(service: &str, ip: &str) -> Result<()> {
    let output = Command::new("networksetup")
        .args(["-setmanual", service, ip, ADHOC_NETMASK, ADHOC_HOST_IP])
        .output()
        .map_err(|e| ConnectoError::Network(format!("Failed to configure IP: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(ConnectoError::Network(format!(
            "Failed to set static IP {} on {}: {}",
            ip,
            service,
            stderr.trim()
        )));
    }

    Ok(())
}

fn macos_product_version() -> Result<String> {
    let output = Command::new("sw_vers")
        .args(["-productVersion"])
        .output()
        .map_err(|e| ConnectoError::Network(format!("Failed to detect macOS version: {}", e)))?;

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn manual_creation_instructions(ssid: &str, version: &str) -> String {
    format!(
        "macOS {} cannot create ad-hoc networks from the command line \
         (Apple disabled the airport utility in macOS 14.4). \
         Create the network manually:\n\
         1. Hold Option + click WiFi icon in menu bar\n\
         2. Select 'Create Network...'\n\
         3. Network Name: {}\n\
         4. Channel: {}\n\
         5. Security: None\n\
         6. Click 'Create'",
        version, ssid, ADHOC_CHANNEL
    )
}
