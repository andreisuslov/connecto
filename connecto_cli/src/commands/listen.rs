//! Listen command - Start listening for pairing requests

use anyhow::Result;
use colored::Colorize;
#[cfg(target_os = "macos")]
use connecto_core::fallback::AdHocNetwork;
use connecto_core::{
    discovery::{get_hostname, get_local_addresses, ServiceAdvertiser},
    keys::KeyManager,
    protocol::{HandshakeServer, ServerEvent},
};
use tokio::sync::mpsc;

#[cfg(target_os = "macos")]
use super::warn;
use super::{error, info, success};

/// Ensure macOS firewall allows incoming connections to connecto
#[cfg(target_os = "macos")]
fn ensure_macos_firewall() {
    use std::process::Command;

    // Get the path to the current executable first
    let exe_path = match std::env::current_exe() {
        Ok(p) => p,
        Err(_) => return,
    };

    // Resolve symlinks to get the real path (important for Homebrew)
    let exe_path = exe_path.canonicalize().unwrap_or(exe_path);

    let exe_str = match exe_path.to_str() {
        Some(s) => s,
        None => return,
    };

    // Check if firewall is enabled
    let fw_state = Command::new("/usr/libexec/ApplicationFirewall/socketfilterfw")
        .arg("--getglobalstate")
        .output();

    let output = fw_state
        .as_ref()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();

    // Debug: print what we got
    if std::env::var("CONNECTO_DEBUG").is_ok() {
        eprintln!("[DEBUG] Firewall state output: {}", output);
        eprintln!("[DEBUG] Executable path: {}", exe_str);
    }

    // Firewall is enabled if output contains "enabled" (case insensitive) or "State = 1"
    let firewall_enabled =
        output.to_lowercase().contains("enabled") || output.contains("State = 1");

    if !firewall_enabled {
        return; // Firewall is off, nothing to do
    }

    info("macOS firewall is enabled - checking access...");

    // Try to add and unblock using osascript for GUI sudo prompt
    // Always try this - the command is idempotent (safe to run multiple times)
    let script = format!(
        r#"do shell script "/usr/libexec/ApplicationFirewall/socketfilterfw --add '{}' && /usr/libexec/ApplicationFirewall/socketfilterfw --unblockapp '{}'" with administrator privileges"#,
        exe_str, exe_str
    );

    let result = Command::new("osascript").args(["-e", &script]).output();

    match result {
        Ok(output) if output.status.success() => {
            success("Firewall exception added for connecto");
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if stderr.contains("User canceled") || stderr.contains("(-128)") {
                warn("Firewall setup canceled - incoming connections may be blocked");
            } else {
                warn("Could not add firewall exception automatically");
            }
            println!(
                "  {} Run manually: {}",
                "→".cyan(),
                format!(
                    "sudo /usr/libexec/ApplicationFirewall/socketfilterfw --add '{}' --unblockapp '{}'",
                    exe_str, exe_str
                )
                .dimmed()
            );
        }
        Err(_) => {
            warn("Could not add firewall exception automatically");
            println!(
                "  {} Run manually: {}",
                "→".cyan(),
                format!(
                    "sudo /usr/libexec/ApplicationFirewall/socketfilterfw --add '{}' --unblockapp '{}'",
                    exe_str, exe_str
                )
                .dimmed()
            );
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn ensure_macos_firewall() {
    // No-op on other platforms
}

pub async fn run(port: u16, name: Option<String>, verify: bool, continuous: bool) -> Result<()> {
    run_with_adhoc(port, name, verify, continuous, false).await
}

pub async fn run_with_adhoc(
    port: u16,
    name: Option<String>,
    verify: bool,
    continuous: bool,
    force_adhoc: bool,
) -> Result<()> {
    let device_name = name.unwrap_or_else(get_hostname);
    let key_manager = KeyManager::new()?;

    // Print header
    println!();
    println!(
        "{}",
        "  CONNECTO LISTENER  ".on_bright_blue().white().bold()
    );
    println!();

    // Track if we should try ad-hoc as fallback
    #[cfg(target_os = "macos")]
    let mut _adhoc_network: Option<AdHocNetwork> = None;

    // If force_adhoc, create ad-hoc network immediately
    #[cfg(target_os = "macos")]
    if force_adhoc {
        info("Creating ad-hoc WiFi network (forced)...");
        let mut network = AdHocNetwork::new(&device_name);

        match network.create_network() {
            Ok(network_name) => {
                success(&format!(
                    "Ad-hoc network created: {}",
                    network_name.magenta().bold()
                ));
                println!();
                println!("{}", "Other devices can now:".dimmed());
                println!(
                    "  {} Join WiFi network '{}'",
                    "1.".cyan(),
                    network_name.cyan()
                );
                println!("  {} Run 'connecto scan' to find this device", "2.".cyan());
                println!();
                _adhoc_network = Some(network);
            }
            Err(e) => {
                warn(&format!(
                    "Could not create ad-hoc network automatically: {}",
                    e
                ));
                println!();
                println!("{}", "To create manually:".dimmed());
                println!(
                    "  {} Hold Option + click WiFi icon in menu bar",
                    "1.".cyan()
                );
                println!("  {} Click 'Create Network...'", "2.".cyan());
                println!(
                    "  {} Name it: {}",
                    "3.".cyan(),
                    network.network_name().cyan()
                );
                println!("  {} Click Create", "4.".cyan());
                println!();
            }
        }
    }

    // Show local addresses
    let addresses = get_local_addresses();
    if addresses.is_empty() && !force_adhoc {
        error("No network interfaces found");
        return Ok(());
    }

    info(&format!("Device name: {}", device_name.cyan()));
    info(&format!("Port: {}", port.to_string().cyan()));
    if force_adhoc {
        info(&format!("Mode: {}", "Ad-hoc (direct connection)".magenta()));
    }
    println!();

    if !addresses.is_empty() {
        println!("{}", "Local IP addresses:".bold());
        for addr in &addresses {
            if addr.is_ipv4() {
                println!("  {} {}", "•".green(), addr);
            }
        }
        println!();
    }

    // Ensure firewall allows connecto (macOS)
    ensure_macos_firewall();

    // Start mDNS advertising
    let mut advertiser = ServiceAdvertiser::new()?;
    advertiser.advertise(&device_name, port)?;
    success("mDNS service registered - device is now discoverable");

    // Start handshake server
    let mut server = HandshakeServer::new(key_manager, &device_name).with_verification(verify);
    let addr = server.listen(port).await?;

    println!();
    println!(
        "{}",
        format!("Listening for pairing requests on port {}...", addr.port())
            .green()
            .bold()
    );
    println!("{}", "Press Ctrl+C to stop".dimmed());
    println!();

    // Create event channel
    let (event_tx, mut event_rx) = mpsc::channel(10);

    // Get local subnets for VPN detection
    let local_subnets: Vec<String> = addresses
        .iter()
        .filter_map(|addr| {
            if let std::net::IpAddr::V4(ipv4) = addr {
                let octets = ipv4.octets();
                Some(format!("{}.{}.{}", octets[0], octets[1], octets[2]))
            } else {
                None
            }
        })
        .collect();

    // Handle events in a separate task
    let event_handler = tokio::spawn(async move {
        let mut last_client_ip: Option<String> = None;

        while let Some(event) = event_rx.recv().await {
            match event {
                ServerEvent::Started { address } => {
                    info(&format!("Server started on {}", address));
                }
                ServerEvent::ClientConnected { address } => {
                    println!();
                    last_client_ip = Some(address.ip().to_string());
                    info(&format!("Connection from {}", address.to_string().yellow()));
                }
                ServerEvent::PairingRequest {
                    device_name,
                    address,
                } => {
                    info(&format!(
                        "Pairing request from {} ({})",
                        device_name.cyan().bold(),
                        address
                    ));
                }
                ServerEvent::KeyReceived { comment } => {
                    info(&format!("Received key: {}", comment.dimmed()));
                }
                ServerEvent::PairingComplete { device_name } => {
                    println!();
                    success(&format!(
                        "Successfully paired with {}!",
                        device_name.green().bold()
                    ));
                    println!("  {} They can now SSH to this machine.", "→".cyan());

                    // Check if client is from a different subnet (VPN scenario)
                    if let Some(ref client_ip) = last_client_ip {
                        let client_subnet: String =
                            client_ip.split('.').take(3).collect::<Vec<_>>().join(".");

                        if !local_subnets.iter().any(|s| s == &client_subnet) {
                            println!();
                            println!(
                                "{}",
                                "VPN/Cross-subnet connection detected!".yellow().bold()
                            );
                            println!(
                                "  {} Tell {} to save your subnet for future scans:",
                                "→".cyan(),
                                device_name.cyan()
                            );
                            println!(
                                "    {}",
                                format!("connecto config add-subnet {}.0/24", client_subnet)
                                    .dimmed()
                            );
                        }
                    }
                    println!();
                }
                ServerEvent::Error { message } => {
                    error(&format!("Error: {}", message));
                }
            }
        }
    });

    // Run server
    if continuous {
        // Run continuously until Ctrl+C
        info("Running in continuous mode (Ctrl+C to stop)...");
        tokio::select! {
            result = server.run(event_tx) => {
                if let Err(e) = result {
                    error(&format!("Server error: {}", e));
                }
            }
            _ = tokio::signal::ctrl_c() => {
                println!();
                info("Shutting down...");
            }
        }
    } else {
        // Default: handle one pairing and exit
        server.handle_one(event_tx).await?;
    }

    // Clean up
    advertiser.stop()?;
    event_handler.abort();

    success("Connecto listener stopped");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_compiles() {
        assert!(true);
    }

    #[tokio::test]
    async fn test_get_hostname_works() {
        let hostname = get_hostname();
        assert!(!hostname.is_empty());
    }
}
