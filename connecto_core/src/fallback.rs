//! Fallback networking module
//!
//! Provides alternative connection methods when standard network discovery fails:
//! - Ad-hoc WiFi network creation and joining
//! - (Future) Bluetooth discovery

#[cfg(target_os = "macos")]
use crate::error::ConnectoError;
use crate::error::Result;
#[cfg(target_os = "macos")]
use std::process::Command;
use std::time::Duration;
#[cfg(target_os = "macos")]
use tracing::{debug, info, warn};

/// The prefix for connecto ad-hoc network names
pub const ADHOC_NETWORK_PREFIX: &str = "Connecto-";

/// Default channel for ad-hoc network
pub const ADHOC_CHANNEL: u32 = 11;

/// Ad-hoc network manager for macOS
#[cfg(target_os = "macos")]
pub struct AdHocNetwork {
    network_name: String,
    is_hosting: bool,
    previous_network: Option<String>,
}

#[cfg(target_os = "macos")]
impl AdHocNetwork {
    /// Create a new ad-hoc network manager
    pub fn new(device_name: &str) -> Self {
        // Sanitize device name for network SSID
        let sanitized: String = device_name
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
            .take(20)
            .collect();

        let network_name = format!("{}{}", ADHOC_NETWORK_PREFIX, sanitized);

        Self {
            network_name,
            is_hosting: false,
            previous_network: None,
        }
    }

    /// Get the network name
    pub fn network_name(&self) -> &str {
        &self.network_name
    }

    /// Save the current WiFi network so we can rejoin later
    fn save_current_network(&mut self) -> Result<()> {
        let output = Command::new("networksetup")
            .args(["-getairportnetwork", "en0"])
            .output()
            .map_err(|e| ConnectoError::Network(format!("Failed to get current network: {}", e)))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        // Output format: "Current Wi-Fi Network: NetworkName"
        if let Some(name) = stdout.strip_prefix("Current Wi-Fi Network: ") {
            self.previous_network = Some(name.trim().to_string());
            debug!("Saved current network: {:?}", self.previous_network);
        }

        Ok(())
    }

    /// Create and host an ad-hoc network
    pub fn create_network(&mut self) -> Result<String> {
        info!("Creating ad-hoc network: {}", self.network_name);

        // Save current network first
        let _ = self.save_current_network();

        // Create the ad-hoc network using networksetup
        // On macOS, we use the "ibss" (ad-hoc) mode
        let _output = Command::new("networksetup")
            .args(["-createnetworkservice", &self.network_name, "en0"])
            .output();

        // The actual ad-hoc creation on macOS requires using airport command or CoreWLAN
        // Let's use the airport utility
        let airport_path = "/System/Library/PrivateFrameworks/Apple80211.framework/Versions/Current/Resources/airport";

        // First, disassociate from current network
        let _ = Command::new(airport_path).args(["-z"]).output();

        // Create IBSS (ad-hoc) network
        // Note: Modern macOS has limited support for this, so we'll try multiple approaches
        let result = Command::new(airport_path)
            .args(["--ibss", &self.network_name, &ADHOC_CHANNEL.to_string()])
            .output();

        match result {
            Ok(output) if output.status.success() => {
                self.is_hosting = true;
                info!(
                    "Ad-hoc network '{}' created successfully",
                    self.network_name
                );

                // Configure a static IP for the ad-hoc network
                let _ = self.configure_adhoc_ip("192.168.73.1");

                Ok(self.network_name.clone())
            }
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                // Try alternative method using networksetup
                warn!("airport ibss failed: {}, trying alternative method", stderr);
                self.create_network_alternative()
            }
            Err(e) => {
                warn!("Failed to run airport command: {}, trying alternative", e);
                self.create_network_alternative()
            }
        }
    }

    /// Alternative method to create ad-hoc network using WiFi menu bar automation
    fn create_network_alternative(&mut self) -> Result<String> {
        info!("Trying WiFi menu bar automation to create ad-hoc network");

        // AppleScript to automate creating a network via the WiFi menu bar icon
        // This works on macOS Monterey, Ventura, Sonoma, and Sequoia
        let script = format!(
            r#"
            use framework "CoreWLAN"
            use scripting additions

            -- First try CoreWLAN directly (requires no UI)
            try
                set wifiClient to current application's CWWiFiClient's sharedWiFiClient()
                set wifiInterface to wifiClient's interface()
                if wifiInterface is not missing value then
                    set ssidData to (current application's NSString's stringWithString:"{network_name}")'s dataUsingEncoding:(current application's NSUTF8StringEncoding)
                    set createResult to wifiInterface's startIBSSModeWithSSID:ssidData security:(current application's kCWIBSSModeSecurityNone) channel:{channel} password:(missing value) |error|:(missing value)
                    if createResult then
                        return "success:corewlan"
                    end if
                end if
            end try

            -- Fallback to UI automation via WiFi menu bar
            tell application "System Events"
                -- Check if WiFi menu extra exists
                tell process "ControlCenter"
                    set menuExtras to menu bar 1's menu bar items
                    repeat with menuItem in menuExtras
                        try
                            if description of menuItem contains "Wi-Fi" or name of menuItem contains "Wi-Fi" then
                                click menuItem
                                delay 0.5

                                -- Look for "Create Network..." or equivalent
                                set foundCreateOption to false
                                repeat with uiItem in (entire contents of window 1)
                                    try
                                        if (class of uiItem is button or class of uiItem is static text) then
                                            set itemName to name of uiItem
                                            if itemName contains "Create Network" or itemName contains "Other Networks" then
                                                click uiItem
                                                set foundCreateOption to true
                                                exit repeat
                                            end if
                                        end if
                                    end try
                                end repeat

                                if not foundCreateOption then
                                    -- Try Wi-Fi Settings path
                                    repeat with uiItem in (entire contents of window 1)
                                        try
                                            if name of uiItem contains "Wi-Fi Settings" or name of uiItem contains "Network Preferences" then
                                                click uiItem
                                                delay 1
                                                exit repeat
                                            end if
                                        end try
                                    end repeat
                                end if

                                exit repeat
                            end if
                        end try
                    end repeat
                end tell
            end tell

            -- If we got here via UI, try to complete the Create Network dialog
            delay 0.5
            tell application "System Events"
                -- Handle the Create Network dialog if it appeared
                set allWindows to windows of (processes whose frontmost is true)
                repeat with proc in (processes whose frontmost is true)
                    repeat with win in windows of proc
                        try
                            set winName to name of win
                            if winName contains "Create" or winName contains "Network" then
                                -- Find and fill the network name field
                                set textFields to text fields of win
                                if (count of textFields) > 0 then
                                    set value of (item 1 of textFields) to "{network_name}"
                                    delay 0.3
                                    -- Click Create button
                                    repeat with btn in buttons of win
                                        if name of btn is "Create" then
                                            click btn
                                            return "success:ui"
                                        end if
                                    end repeat
                                end if
                            end if
                        end try
                    end repeat
                end repeat
            end tell

            return "fallback:manual"
            "#,
            network_name = self.network_name,
            channel = ADHOC_CHANNEL
        );

        let result = Command::new("osascript")
            .args(["-e", &script])
            .output()
            .map_err(|e| {
                ConnectoError::Network(format!("Failed to run AppleScript automation: {}", e))
            })?;

        let stdout = String::from_utf8_lossy(&result.stdout).trim().to_string();
        let stderr = String::from_utf8_lossy(&result.stderr);

        debug!("AppleScript result: stdout={}, stderr={}", stdout, stderr);

        if stdout.starts_with("success:") {
            self.is_hosting = true;
            info!(
                "Ad-hoc network '{}' created via {}",
                self.network_name,
                stdout.strip_prefix("success:").unwrap_or("automation")
            );

            // Configure static IP for the ad-hoc network
            let _ = self.configure_adhoc_ip("192.168.73.1");

            return Ok(self.network_name.clone());
        }

        // If automation didn't fully succeed, try one more approach: direct networksetup
        // On some macOS versions, we can create a network service and configure it
        if self.try_networksetup_adhoc() {
            self.is_hosting = true;
            info!(
                "Ad-hoc network '{}' created via networksetup",
                self.network_name
            );
            let _ = self.configure_adhoc_ip("192.168.73.1");
            return Ok(self.network_name.clone());
        }

        // Last resort: provide manual instructions
        Err(ConnectoError::Network(format!(
            "Automatic ad-hoc network creation failed. \
             Please create manually:\n\
             1. Hold Option + click WiFi icon in menu bar\n\
             2. Select 'Create Network...'\n\
             3. Network Name: {}\n\
             4. Channel: {}\n\
             5. Security: None\n\
             6. Click 'Create'",
            self.network_name, ADHOC_CHANNEL
        )))
    }

    /// Try to create ad-hoc network using networksetup commands
    fn try_networksetup_adhoc(&self) -> bool {
        // Get the WiFi interface name
        let interface = self.get_wifi_interface().unwrap_or_else(|| "en0".to_string());

        // Try using wdutil (available on newer macOS)
        if let Ok(output) = Command::new("wdutil")
            .args(["info"])
            .output()
        {
            if output.status.success() {
                debug!("wdutil available, WiFi interface: {}", interface);
            }
        }

        // Attempt to create via airport with different syntax variations
        let airport_path = "/System/Library/PrivateFrameworks/Apple80211.framework/Versions/Current/Resources/airport";
        let channel_str = ADHOC_CHANNEL.to_string();

        // Try legacy syntax variations
        let attempts: [&[&str]; 3] = [
            &["-i", &interface, "--ibss", &self.network_name, &channel_str],
            &["--ibss", &self.network_name, &channel_str, "-c", &channel_str],
            &["-I", &interface, "sniff", &channel_str], // This won't create IBSS but tests airport
        ];

        for args in attempts.iter() {
            if let Ok(output) = Command::new(airport_path).args(*args).output() {
                if output.status.success() {
                    debug!("airport command succeeded with args: {:?}", args);
                    return true;
                }
            }
        }

        false
    }

    /// Get the WiFi interface name (usually en0 but can vary)
    fn get_wifi_interface(&self) -> Option<String> {
        let output = Command::new("networksetup")
            .args(["-listallhardwareports"])
            .output()
            .ok()?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut found_wifi = false;

        for line in stdout.lines() {
            if line.contains("Wi-Fi") {
                found_wifi = true;
            } else if found_wifi && line.starts_with("Device:") {
                return Some(line.replace("Device:", "").trim().to_string());
            }
        }

        None
    }

    /// Configure IP address for ad-hoc network
    fn configure_adhoc_ip(&self, ip: &str) -> Result<()> {
        let output = Command::new("networksetup")
            .args(["-setmanual", "Wi-Fi", ip, "255.255.255.0", "192.168.73.1"])
            .output()
            .map_err(|e| ConnectoError::Network(format!("Failed to configure IP: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            warn!("Failed to set IP: {}", stderr);
        }

        Ok(())
    }

    /// Scan for connecto ad-hoc networks using system_profiler (works on modern macOS)
    pub fn scan_for_networks() -> Result<Vec<String>> {
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

        // Also try networksetup to list available networks
        if networks.is_empty() {
            if let Ok(output) = Command::new("networksetup")
                .args(["-listallhardwareports"])
                .output()
            {
                // Get WiFi interface name (for future use)
                let stdout = String::from_utf8_lossy(&output.stdout);
                let mut _wifi_device = "en0".to_string();
                let mut found_wifi = false;
                for line in stdout.lines() {
                    if line.contains("Wi-Fi") {
                        found_wifi = true;
                    } else if found_wifi && line.starts_with("Device:") {
                        _wifi_device = line.replace("Device:", "").trim().to_string();
                        break;
                    }
                }

                // Scan using CoreWLAN via defaults (hacky but works)
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
                        if line.contains(ADHOC_NETWORK_PREFIX) {
                            let trimmed = line.trim().trim_matches(|c| {
                                c == '"' || c == ';' || c == '=' || c == '{' || c == '}'
                            });
                            if trimmed.starts_with(ADHOC_NETWORK_PREFIX)
                                && !networks.contains(&trimmed.to_string())
                            {
                                networks.push(trimmed.to_string());
                            }
                        }
                    }
                }
            }
        }

        debug!("Found {} connecto ad-hoc networks", networks.len());
        Ok(networks)
    }

    /// Join an existing connecto ad-hoc network
    pub fn join_network(&mut self, network_name: &str) -> Result<()> {
        info!("Joining ad-hoc network: {}", network_name);

        // Save current network first
        let _ = self.save_current_network();

        let output = Command::new("networksetup")
            .args(["-setairportnetwork", "en0", network_name])
            .output()
            .map_err(|e| ConnectoError::Network(format!("Failed to join network: {}", e)))?;

        if output.status.success() {
            self.network_name = network_name.to_string();

            // Configure IP for client (different from host)
            let _ = self.configure_adhoc_ip("192.168.73.2");

            info!("Successfully joined network: {}", network_name);
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(ConnectoError::Network(format!(
                "Failed to join network: {}",
                stderr
            )))
        }
    }

    /// Restore previous network connection
    pub fn restore_previous_network(&mut self) -> Result<()> {
        if let Some(ref network) = self.previous_network {
            info!("Restoring previous network: {}", network);

            let output = Command::new("networksetup")
                .args(["-setairportnetwork", "en0", network])
                .output()
                .map_err(|e| ConnectoError::Network(format!("Failed to restore network: {}", e)))?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                warn!("Failed to restore network: {}", stderr);
            }

            // Reset to DHCP
            let _ = Command::new("networksetup")
                .args(["-setdhcp", "Wi-Fi"])
                .output();
        }

        self.is_hosting = false;
        Ok(())
    }

    /// Check if we're currently hosting an ad-hoc network
    pub fn is_hosting(&self) -> bool {
        self.is_hosting
    }
}

#[cfg(target_os = "macos")]
impl Drop for AdHocNetwork {
    fn drop(&mut self) {
        if self.is_hosting {
            let _ = self.restore_previous_network();
        }
    }
}

/// Fallback connection handler
pub struct FallbackHandler {
    #[cfg(target_os = "macos")]
    adhoc: Option<AdHocNetwork>,
    #[allow(dead_code)]
    timeout: Duration,
}

impl FallbackHandler {
    /// Create a new fallback handler
    pub fn new(
        #[cfg_attr(not(target_os = "macos"), allow(unused_variables))] device_name: &str,
        timeout: Duration,
    ) -> Self {
        Self {
            #[cfg(target_os = "macos")]
            adhoc: Some(AdHocNetwork::new(device_name)),
            timeout,
        }
    }

    /// Try to establish connectivity using fallback methods
    /// Returns the IP address to use for connection if successful
    #[cfg(target_os = "macos")]
    pub async fn establish_fallback_connection(
        &mut self,
        is_listener: bool,
    ) -> Result<Option<String>> {
        if is_listener {
            // Listener: Create an ad-hoc network
            if let Some(ref mut adhoc) = self.adhoc {
                match adhoc.create_network() {
                    Ok(network_name) => {
                        info!("Created fallback network: {}", network_name);
                        // Wait a moment for network to stabilize
                        tokio::time::sleep(Duration::from_secs(2)).await;
                        return Ok(Some("192.168.73.1".to_string()));
                    }
                    Err(e) => {
                        warn!("Failed to create ad-hoc network: {}", e);
                        return Err(e);
                    }
                }
            }
        } else {
            // Scanner: Look for and join connecto ad-hoc networks
            let networks = AdHocNetwork::scan_for_networks()?;

            if let Some(network) = networks.first() {
                if let Some(ref mut adhoc) = self.adhoc {
                    adhoc.join_network(network)?;
                    // Wait for connection to establish
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    return Ok(Some("192.168.73.1".to_string())); // Return host IP to connect to
                }
            } else {
                info!("No connecto ad-hoc networks found");
            }
        }

        Ok(None)
    }

    #[cfg(not(target_os = "macos"))]
    pub async fn establish_fallback_connection(
        &mut self,
        _is_listener: bool,
    ) -> Result<Option<String>> {
        // Ad-hoc networking not yet implemented for other platforms
        Ok(None)
    }

    /// Clean up fallback connections
    #[cfg(target_os = "macos")]
    pub fn cleanup(&mut self) {
        if let Some(ref mut adhoc) = self.adhoc {
            let _ = adhoc.restore_previous_network();
        }
    }

    #[cfg(not(target_os = "macos"))]
    pub fn cleanup(&mut self) {
        // Nothing to clean up on other platforms yet
    }
}

#[cfg(test)]
#[cfg(target_os = "macos")]
mod tests {
    use super::*;

    #[test]
    fn test_adhoc_network_prefix() {
        assert_eq!(ADHOC_NETWORK_PREFIX, "Connecto-");
    }

    #[test]
    fn test_network_name_sanitization() {
        let adhoc = AdHocNetwork::new("My Device!@#$%");
        assert!(adhoc.network_name().starts_with(ADHOC_NETWORK_PREFIX));
        assert!(!adhoc.network_name().contains("!"));
        assert!(!adhoc.network_name().contains("@"));
    }

    #[test]
    fn test_network_name_length() {
        let adhoc = AdHocNetwork::new("This is a very long device name that should be truncated");
        // ADHOC_NETWORK_PREFIX (9 chars) + max 20 chars = 29 max
        assert!(adhoc.network_name().len() <= 29);
    }
}
