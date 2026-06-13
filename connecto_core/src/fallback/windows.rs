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
    /// WLAN profile we installed for joining (deleted on restore)
    created_profile: Option<String>,
}

impl WindowsBackend {
    pub(super) fn new(password: String) -> Self {
        Self {
            password,
            hosting: false,
            configured_interface: None,
            created_profile: None,
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

        // Parse output to find current SSID. split_once keeps everything
        // after the first colon so SSIDs containing ':' survive intact.
        for line in stdout.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("SSID") && !trimmed.contains("BSSID") {
                if let Some((_, value)) = trimmed.split_once(':') {
                    let network = value.trim().to_string();
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
        // `netsh wlan connect` only works against a saved profile (it does
        // NOT prompt for credentials), so install a temporary profile that
        // matches the target network first.
        let network = inspect_network(ssid)?;

        if network.is_adhoc {
            return Err(ConnectoError::Network(format!(
                "'{}' is an IBSS (ad-hoc) network, and Windows 8.1 and later \
                 cannot join IBSS networks. Run 'connecto listen' on this \
                 Windows machine instead, so it hosts the network, and join \
                 from the other device.",
                ssid
            )));
        }

        // Open network (macOS/Linux host) vs WPA2 hosted network (Windows
        // host; both sides derive the key from the SSID).
        let password = if network.is_open {
            None
        } else {
            Some(super::derive_join_password(ssid))
        };

        add_profile(ssid, password.as_deref())?;
        // The profile exists from here on; make sure restore deletes it
        // even if connecting fails below.
        self.created_profile = Some(ssid.to_string());

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

        // Delete the temporary WLAN profile we installed for joining
        // (disassociates from the ad-hoc network as a side effect)
        if let Some(profile) = self.created_profile.take() {
            let _ = Command::new("netsh")
                .args(["wlan", "delete", "profile", &format!("name={}", profile)])
                .output();
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

    // Parse output to find SSIDs starting with our prefix (split_once so
    // SSIDs containing ':' survive intact)
    for line in stdout.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("SSID") && !trimmed.contains("BSSID") {
            if let Some((_, value)) = trimmed.split_once(':') {
                let network = value.trim().to_string();
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
        .args(["interface", "show", "interface"])
        .output()
        .ok()?;

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Once started, the hosted network surfaces as a *connected* virtual
    // adapter named "Local Area Connection* N" — the literal asterisk marks
    // Microsoft's hosted-network/Wi-Fi Direct virtual adapters. A plain
    // "Local Area Connection" (no asterisk) is a physical NIC and must not
    // match. Columns: Admin State / State / Type / Interface Name.
    for line in stdout.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 4 && parts[1] == "Connected" {
            let name = parts[3..].join(" ");
            if name.starts_with("Local Area Connection*") {
                return Some(name);
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
            if let Some((_, value)) = trimmed.split_once(':') {
                current_name = Some(value.trim().to_string());
            }
        } else if trimmed.starts_with("SSID") && !trimmed.contains("BSSID") {
            if let Some((_, value)) = trimmed.split_once(':') {
                if value.trim() == ssid {
                    return current_name;
                }
            }
        }
    }

    None
}

/// What `netsh wlan show networks` reports about a visible network
struct VisibleNetwork {
    is_adhoc: bool,
    is_open: bool,
}

/// Look up `ssid` in the current scan results to learn its network type
/// and authentication, which the join profile must match
///
/// Errors when the network is not visible or the fields cannot be parsed
/// (e.g. localized netsh output): guessing a profile would mis-join or
/// fail silently, so the caller gets an honest error instead.
fn inspect_network(ssid: &str) -> Result<VisibleNetwork> {
    let output = Command::new("netsh")
        .args(["wlan", "show", "networks", "mode=bssid"])
        .output()
        .map_err(|e| ConnectoError::Network(format!("Failed to scan networks: {}", e)))?;

    let stdout = String::from_utf8_lossy(&output.stdout);

    let mut in_block = false;
    let mut network_type: Option<String> = None;
    let mut authentication: Option<String> = None;

    for line in stdout.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("SSID") && !trimmed.contains("BSSID") {
            if in_block {
                break; // reached the next network's block
            }
            if let Some((_, value)) = trimmed.split_once(':') {
                in_block = value.trim() == ssid;
            }
        } else if in_block {
            if let Some((label, value)) = trimmed.split_once(':') {
                let label = label.trim();
                if label.starts_with("Network type") {
                    network_type = Some(value.trim().to_string());
                } else if label.starts_with("Authentication") {
                    authentication = Some(value.trim().to_string());
                }
            }
        }
    }

    match (network_type, authentication) {
        (Some(network_type), Some(authentication)) => Ok(VisibleNetwork {
            is_adhoc: network_type.eq_ignore_ascii_case("Adhoc"),
            is_open: authentication.to_ascii_lowercase().contains("open"),
        }),
        _ => Err(ConnectoError::Network(format!(
            "Could not determine the type of network '{}' from 'netsh wlan \
             show networks' (it may be out of range, or the netsh output is \
             localized). Join it manually from WiFi settings instead — if it \
             asks for a key, use '{}' — then re-run this command.",
            ssid,
            super::derive_join_password(ssid)
        ))),
    }
}

/// Install a temporary WLAN profile for `ssid` via `netsh wlan add profile`
///
/// `netsh wlan connect` refuses to connect to an SSID without a saved
/// profile (it never prompts), so the joiner installs one — open, or
/// WPA2-PSK with `password` — and restore deletes it again.
fn add_profile(ssid: &str, password: Option<&str>) -> Result<()> {
    let security = match password {
        Some(password) => format!(
            "<authEncryption><authentication>WPA2PSK</authentication>\
             <encryption>AES</encryption><useOneX>false</useOneX></authEncryption>\
             <sharedKey><keyType>passPhrase</keyType><protected>false</protected>\
             <keyMaterial>{}</keyMaterial></sharedKey>",
            xml_escape(password)
        ),
        None => "<authEncryption><authentication>open</authentication>\
                 <encryption>none</encryption><useOneX>false</useOneX></authEncryption>"
            .to_string(),
    };

    let profile_xml = format!(
        "<?xml version=\"1.0\"?>\
         <WLANProfile xmlns=\"http://www.microsoft.com/networking/WLAN/profile/v1\">\
         <name>{ssid}</name>\
         <SSIDConfig><SSID><name>{ssid}</name></SSID></SSIDConfig>\
         <connectionType>ESS</connectionType>\
         <connectionMode>manual</connectionMode>\
         <MSM><security>{security}</security></MSM>\
         </WLANProfile>",
        ssid = xml_escape(ssid),
        security = security
    );

    let path = std::env::temp_dir().join(format!("connecto-wlan-{}.xml", std::process::id()));
    std::fs::write(&path, &profile_xml)
        .map_err(|e| ConnectoError::Network(format!("Failed to write WLAN profile: {}", e)))?;

    let output = Command::new("netsh")
        .args([
            "wlan",
            "add",
            "profile",
            &format!("filename={}", path.display()),
        ])
        .output();

    // The file is only needed for the add call; it contains the key, so do
    // not leave it behind regardless of the outcome.
    let _ = std::fs::remove_file(&path);

    let output =
        output.map_err(|e| ConnectoError::Network(format!("netsh add profile failed: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        return Err(ConnectoError::Network(format!(
            "Failed to install WLAN profile for '{}': {} {}",
            ssid,
            stdout.trim(),
            stderr.trim()
        )));
    }

    Ok(())
}

/// Minimal XML escaping for WLAN profile fields
fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
