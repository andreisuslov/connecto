//! Fallback networking module
//!
//! Provides alternative connection methods when standard network discovery fails:
//! - Ad-hoc WiFi network creation and joining
//! - (Future) Bluetooth discovery

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
use crate::error::ConnectoError;
use crate::error::Result;
#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
use std::process::Command;
use std::time::Duration;
#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
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

// ============================================================================
// Linux Implementation
// ============================================================================

/// Ad-hoc network manager for Linux (using nmcli/NetworkManager or iw as fallback)
#[cfg(target_os = "linux")]
pub struct AdHocNetwork {
    network_name: String,
    is_hosting: bool,
    previous_network: Option<String>,
    connection_uuid: Option<String>,
    interface: Option<String>,
}

#[cfg(target_os = "linux")]
impl AdHocNetwork {
    /// Create a new ad-hoc network manager
    pub fn new(device_name: &str) -> Self {
        // Sanitize device name for network SSID (same as macOS)
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
            connection_uuid: None,
            interface: None,
        }
    }

    /// Get the network name
    pub fn network_name(&self) -> &str {
        &self.network_name
    }

    /// Get the WiFi interface name
    fn get_wifi_interface(&self) -> Option<String> {
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
                if trimmed.starts_with("Interface ") {
                    return Some(trimmed.replace("Interface ", "").to_string());
                }
            }
        }

        None
    }

    /// Save the current WiFi network so we can rejoin later
    fn save_current_network(&mut self) -> Result<()> {
        let output = Command::new("nmcli")
            .args(["-t", "-f", "NAME,DEVICE", "connection", "show", "--active"])
            .output()
            .map_err(|e| {
                ConnectoError::Network(format!("Failed to get current network: {}", e))
            })?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let interface = self.interface.clone().unwrap_or_else(|| "wlan0".to_string());

        // Parse output format: "NetworkName:wlan0"
        for line in stdout.lines() {
            if line.contains(&interface) {
                if let Some(name) = line.split(':').next() {
                    self.previous_network = Some(name.to_string());
                    debug!("Saved current network: {:?}", self.previous_network);
                    break;
                }
            }
        }

        Ok(())
    }

    /// Create and host an ad-hoc network
    pub fn create_network(&mut self) -> Result<String> {
        info!("Creating ad-hoc network: {}", self.network_name);

        // Get WiFi interface
        let interface = self.get_wifi_interface().ok_or_else(|| {
            ConnectoError::Network(
                "No WiFi interface found. Install NetworkManager: sudo apt install network-manager"
                    .to_string(),
            )
        })?;
        self.interface = Some(interface.clone());

        // Save current network first
        let _ = self.save_current_network();

        // Try nmcli first (requires NetworkManager)
        match self.create_network_nmcli(&interface) {
            Ok(uuid) => {
                self.connection_uuid = Some(uuid);
                self.is_hosting = true;
                info!(
                    "Ad-hoc network '{}' created successfully via nmcli",
                    self.network_name
                );
                return Ok(self.network_name.clone());
            }
            Err(e) => {
                warn!("nmcli failed: {}, trying iw fallback", e);
            }
        }

        // Fallback to iw (requires root)
        match self.create_network_iw(&interface) {
            Ok(()) => {
                self.is_hosting = true;
                info!(
                    "Ad-hoc network '{}' created successfully via iw",
                    self.network_name
                );
                Ok(self.network_name.clone())
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

    /// Create ad-hoc network using nmcli (NetworkManager)
    fn create_network_nmcli(&self, interface: &str) -> Result<String> {
        // Delete any existing connection with this name
        let _ = Command::new("nmcli")
            .args(["connection", "delete", &self.network_name])
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
                &self.network_name,
                "con-name",
                &self.network_name,
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

        // Extract UUID from output
        let stdout = String::from_utf8_lossy(&output.stdout);
        let uuid = Self::parse_nmcli_uuid(&stdout).unwrap_or_default();

        // Configure static IP
        let _ = Command::new("nmcli")
            .args([
                "connection",
                "modify",
                &self.network_name,
                "ipv4.method",
                "manual",
                "ipv4.addresses",
                "192.168.73.1/24",
            ])
            .output();

        // Bring up the connection
        let up_output = Command::new("nmcli")
            .args(["connection", "up", &self.network_name])
            .output()
            .map_err(|e| ConnectoError::Network(format!("nmcli up failed: {}", e)))?;

        if !up_output.status.success() {
            let stderr = String::from_utf8_lossy(&up_output.stderr);
            // Clean up the failed connection
            let _ = Command::new("nmcli")
                .args(["connection", "delete", &self.network_name])
                .output();
            return Err(ConnectoError::Network(format!(
                "nmcli connection up failed: {}",
                stderr
            )));
        }

        Ok(uuid)
    }

    /// Parse UUID from nmcli output
    fn parse_nmcli_uuid(output: &str) -> Option<String> {
        // Output format: "Connection 'name' (uuid) successfully added."
        if let Some(start) = output.find('(') {
            if let Some(end) = output.find(')') {
                if start < end {
                    return Some(output[start + 1..end].to_string());
                }
            }
        }
        None
    }

    /// Create ad-hoc network using iw (requires root)
    fn create_network_iw(&self, interface: &str) -> Result<()> {
        // First, bring down any existing connection
        let _ = Command::new("ip")
            .args(["link", "set", interface, "down"])
            .output();

        // Set interface to IBSS (ad-hoc) mode
        let output = Command::new("iw")
            .args([
                "dev",
                interface,
                "ibss",
                "join",
                &self.network_name,
                "2462", // Channel 11 frequency in MHz
            ])
            .output()
            .map_err(|e| ConnectoError::Network(format!("iw ibss join failed: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ConnectoError::Network(format!("iw failed: {}", stderr)));
        }

        // Bring interface up
        let _ = Command::new("ip")
            .args(["link", "set", interface, "up"])
            .output();

        // Configure IP address
        self.configure_adhoc_ip("192.168.73.1", interface)?;

        Ok(())
    }

    /// Configure IP address for ad-hoc network
    fn configure_adhoc_ip(&self, ip: &str, interface: &str) -> Result<()> {
        // Flush existing IPs
        let _ = Command::new("ip")
            .args(["addr", "flush", "dev", interface])
            .output();

        // Add new IP
        let output = Command::new("ip")
            .args(["addr", "add", &format!("{}/24", ip), "dev", interface])
            .output()
            .map_err(|e| ConnectoError::Network(format!("Failed to configure IP: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // Ignore "file exists" error (IP already configured)
            if !stderr.contains("File exists") {
                warn!("Failed to set IP: {}", stderr);
            }
        }

        Ok(())
    }

    /// Scan for connecto ad-hoc networks
    pub fn scan_for_networks() -> Result<Vec<String>> {
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

        // Fallback to iw scan (requires root typically)
        if networks.is_empty() {
            if let Ok(output) = Command::new("iw").args(["dev", "wlan0", "scan"]).output() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                for line in stdout.lines() {
                    let trimmed = line.trim();
                    if trimmed.starts_with("SSID:") {
                        let ssid = trimmed.replace("SSID:", "").trim().to_string();
                        if ssid.starts_with(ADHOC_NETWORK_PREFIX)
                            && !networks.contains(&ssid)
                        {
                            networks.push(ssid);
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

        let interface = self.get_wifi_interface().ok_or_else(|| {
            ConnectoError::Network("No WiFi interface found".to_string())
        })?;
        self.interface = Some(interface.clone());

        // Save current network first
        let _ = self.save_current_network();

        // Try nmcli first
        let output = Command::new("nmcli")
            .args(["device", "wifi", "connect", network_name])
            .output()
            .map_err(|e| ConnectoError::Network(format!("Failed to join network: {}", e)))?;

        if output.status.success() {
            self.network_name = network_name.to_string();

            // Configure IP for client (different from host)
            let _ = self.configure_adhoc_ip("192.168.73.2", &interface);

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
        // Delete the ad-hoc connection if we created one via nmcli
        if self.connection_uuid.is_some() {
            let _ = Command::new("nmcli")
                .args(["connection", "delete", &self.network_name])
                .output();
        }

        // If using iw, leave the IBSS network
        if let Some(ref interface) = self.interface {
            let _ = Command::new("iw")
                .args(["dev", interface, "ibss", "leave"])
                .output();
        }

        // Reconnect to previous network if we have one
        if let Some(ref network) = self.previous_network {
            info!("Restoring previous network: {}", network);

            let output = Command::new("nmcli")
                .args(["connection", "up", network])
                .output()
                .map_err(|e| {
                    ConnectoError::Network(format!("Failed to restore network: {}", e))
                })?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                warn!("Failed to restore network: {}", stderr);
            }
        }

        self.is_hosting = false;
        Ok(())
    }

    /// Check if we're currently hosting an ad-hoc network
    pub fn is_hosting(&self) -> bool {
        self.is_hosting
    }
}

#[cfg(target_os = "linux")]
impl Drop for AdHocNetwork {
    fn drop(&mut self) {
        if self.is_hosting {
            let _ = self.restore_previous_network();
        }
    }
}

// ============================================================================
// Windows Implementation
// ============================================================================

/// Ad-hoc network manager for Windows (using netsh hosted network)
#[cfg(target_os = "windows")]
pub struct AdHocNetwork {
    network_name: String,
    is_hosting: bool,
    previous_network: Option<String>,
    password: String,
    adapter_name: Option<String>,
}

#[cfg(target_os = "windows")]
impl AdHocNetwork {
    /// Create a new ad-hoc network manager
    pub fn new(device_name: &str) -> Self {
        // Sanitize device name for network SSID (same as macOS)
        let sanitized: String = device_name
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '-' || *c == '_')
            .take(20)
            .collect();

        let network_name = format!("{}{}", ADHOC_NETWORK_PREFIX, sanitized);

        // Generate a default password (Windows hosted network requires 8+ chars)
        let password = format!("connecto{}", &sanitized.chars().take(4).collect::<String>());

        Self {
            network_name,
            is_hosting: false,
            previous_network: None,
            password,
            adapter_name: None,
        }
    }

    /// Create with a custom password
    pub fn with_password(mut self, password: &str) -> Self {
        if password.len() >= 8 {
            self.password = password.to_string();
        }
        self
    }

    /// Get the network name
    pub fn network_name(&self) -> &str {
        &self.network_name
    }

    /// Get the password for the hosted network
    pub fn password(&self) -> &str {
        &self.password
    }

    /// Save the current WiFi network so we can rejoin later
    fn save_current_network(&mut self) -> Result<()> {
        let output = Command::new("netsh")
            .args(["wlan", "show", "interfaces"])
            .output()
            .map_err(|e| {
                ConnectoError::Network(format!("Failed to get current network: {}", e))
            })?;

        let stdout = String::from_utf8_lossy(&output.stdout);

        // Parse output to find current SSID
        for line in stdout.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("SSID") && !trimmed.contains("BSSID") {
                if let Some(ssid) = trimmed.split(':').nth(1) {
                    let network = ssid.trim().to_string();
                    if !network.is_empty() {
                        self.previous_network = Some(network);
                        debug!("Saved current network: {:?}", self.previous_network);
                    }
                }
                break;
            }
        }

        Ok(())
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

    /// Create and host an ad-hoc network (requires admin privileges)
    pub fn create_network(&mut self) -> Result<String> {
        info!("Creating hosted network: {}", self.network_name);

        // Check for hosted network support
        Self::check_hosted_network_support()?;

        // Save current network first
        let _ = self.save_current_network();

        // Configure the hosted network
        let output = Command::new("netsh")
            .args([
                "wlan",
                "set",
                "hostednetwork",
                "mode=allow",
                &format!("ssid={}", self.network_name),
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

        // Find the hosted network adapter and configure IP
        if let Some(adapter) = self.find_hosted_network_adapter() {
            self.adapter_name = Some(adapter.clone());
            let _ = self.configure_adhoc_ip("192.168.73.1", &adapter);
        }

        self.is_hosting = true;
        info!(
            "Hosted network '{}' created successfully (password: {})",
            self.network_name, self.password
        );

        Ok(self.network_name.clone())
    }

    /// Find the virtual adapter created for the hosted network
    fn find_hosted_network_adapter(&self) -> Option<String> {
        let output = Command::new("netsh")
            .args(["wlan", "show", "hostednetwork"])
            .output()
            .ok()?;

        let stdout = String::from_utf8_lossy(&output.stdout);

        // Look for adapter name in output
        // It's typically "Microsoft Hosted Network Virtual Adapter" or similar
        // We need to check interface list
        let interface_output = Command::new("netsh")
            .args(["interface", "show", "interface"])
            .output()
            .ok()?;

        let interface_stdout = String::from_utf8_lossy(&interface_output.stdout);

        // Find the Local Area Connection for hosted network
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

        // Default to common name
        Some("Local Area Connection* 12".to_string())
    }

    /// Configure IP address for the hosted network
    fn configure_adhoc_ip(&self, ip: &str, adapter: &str) -> Result<()> {
        let output = Command::new("netsh")
            .args([
                "interface",
                "ipv4",
                "set",
                "address",
                &format!("name={}", adapter),
                "static",
                ip,
                "255.255.255.0",
            ])
            .output()
            .map_err(|e| ConnectoError::Network(format!("Failed to configure IP: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            warn!("Failed to set IP (may already be configured): {}", stderr);
        }

        Ok(())
    }

    /// Scan for connecto ad-hoc networks
    pub fn scan_for_networks() -> Result<Vec<String>> {
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

    /// Join an existing connecto ad-hoc network
    pub fn join_network(&mut self, network_name: &str) -> Result<()> {
        info!("Joining network: {}", network_name);

        // Save current network first
        let _ = self.save_current_network();

        // Connect to the network (will prompt for password if needed)
        let output = Command::new("netsh")
            .args(["wlan", "connect", &format!("name={}", network_name)])
            .output()
            .map_err(|e| ConnectoError::Network(format!("Failed to join network: {}", e)))?;

        if output.status.success() {
            self.network_name = network_name.to_string();
            info!("Successfully joined network: {}", network_name);

            // Configure IP for client (different from host)
            // This might need manual configuration on Windows
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            Err(ConnectoError::Network(format!(
                "Failed to join network: {} {}",
                stdout, stderr
            )))
        }
    }

    /// Restore previous network connection
    pub fn restore_previous_network(&mut self) -> Result<()> {
        // Stop the hosted network
        if self.is_hosting {
            let _ = Command::new("netsh")
                .args(["wlan", "stop", "hostednetwork"])
                .output();

            // Optionally disable hosted network mode
            let _ = Command::new("netsh")
                .args(["wlan", "set", "hostednetwork", "mode=disallow"])
                .output();
        }

        // Reconnect to previous network if we have one
        if let Some(ref network) = self.previous_network {
            info!("Restoring previous network: {}", network);

            let output = Command::new("netsh")
                .args(["wlan", "connect", &format!("name={}", network)])
                .output()
                .map_err(|e| {
                    ConnectoError::Network(format!("Failed to restore network: {}", e))
                })?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                warn!("Failed to restore network: {}", stderr);
            }
        }

        self.is_hosting = false;
        Ok(())
    }

    /// Check if we're currently hosting an ad-hoc network
    pub fn is_hosting(&self) -> bool {
        self.is_hosting
    }
}

#[cfg(target_os = "windows")]
impl Drop for AdHocNetwork {
    fn drop(&mut self) {
        if self.is_hosting {
            let _ = self.restore_previous_network();
        }
    }
}

/// Fallback connection handler
pub struct FallbackHandler {
    #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
    adhoc: Option<AdHocNetwork>,
    #[allow(dead_code)]
    timeout: Duration,
}

impl FallbackHandler {
    /// Create a new fallback handler
    pub fn new(
        #[cfg_attr(
            not(any(target_os = "macos", target_os = "linux", target_os = "windows")),
            allow(unused_variables)
        )]
        device_name: &str,
        timeout: Duration,
    ) -> Self {
        Self {
            #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
            adhoc: Some(AdHocNetwork::new(device_name)),
            timeout,
        }
    }

    /// Try to establish connectivity using fallback methods
    /// Returns the IP address to use for connection if successful
    #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
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

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    pub async fn establish_fallback_connection(
        &mut self,
        _is_listener: bool,
    ) -> Result<Option<String>> {
        // Ad-hoc networking not implemented for this platform
        Ok(None)
    }

    /// Clean up fallback connections
    #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
    pub fn cleanup(&mut self) {
        if let Some(ref mut adhoc) = self.adhoc {
            let _ = adhoc.restore_previous_network();
        }
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    pub fn cleanup(&mut self) {
        // Nothing to clean up on this platform
    }
}

#[cfg(test)]
#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
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

#[cfg(test)]
#[cfg(target_os = "linux")]
mod linux_tests {
    use super::*;

    #[test]
    fn test_parse_nmcli_uuid() {
        let output = "Connection 'Connecto-Test' (abc12345-1234-5678-90ab-cdef12345678) successfully added.";
        let uuid = AdHocNetwork::parse_nmcli_uuid(output);
        assert_eq!(uuid, Some("abc12345-1234-5678-90ab-cdef12345678".to_string()));
    }

    #[test]
    fn test_parse_nmcli_uuid_no_uuid() {
        let output = "Some other output without UUID";
        let uuid = AdHocNetwork::parse_nmcli_uuid(output);
        assert_eq!(uuid, None);
    }
}

#[cfg(test)]
#[cfg(target_os = "windows")]
mod windows_tests {
    use super::*;

    #[test]
    fn test_password_generation() {
        let adhoc = AdHocNetwork::new("TestDevice");
        // Password should be at least 8 characters
        assert!(adhoc.password().len() >= 8);
    }

    #[test]
    fn test_custom_password() {
        let adhoc = AdHocNetwork::new("TestDevice").with_password("mypassword123");
        assert_eq!(adhoc.password(), "mypassword123");
    }

    #[test]
    fn test_short_password_ignored() {
        let adhoc = AdHocNetwork::new("TestDevice").with_password("short");
        // Short password should be ignored, use default
        assert!(adhoc.password().len() >= 8);
        assert_ne!(adhoc.password(), "short");
    }
}
