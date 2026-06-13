//! Listen command - Start listening for pairing requests

use anyhow::Result;
use colored::Colorize;
#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
use connecto_core::fallback::AdHocNetwork;
#[cfg(feature = "bluetooth")]
use connecto_core::fallback::FallbackHandler;
use connecto_core::{
    discovery::{get_hostname, get_local_addresses, ServiceAdvertiser},
    keys::KeyManager,
    protocol::{HandshakeServer, PairingRequest, ServerEvent},
};
use dialoguer::Confirm;
#[cfg(feature = "bluetooth")]
use std::time::Duration;
use tokio::sync::mpsc;

#[cfg(any(
    feature = "bluetooth",
    any(target_os = "macos", target_os = "linux", target_os = "windows")
))]
use super::warn;
use super::{error, info, success};
use crate::SilentExit;

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

pub async fn run_with_adhoc(
    port: u16,
    name: Option<String>,
    verify: bool,
    continuous: bool,
    force_adhoc: bool,
    bluetooth_enabled: bool,
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
    #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
    let mut adhoc_network: Option<AdHocNetwork> = None;

    // If force_adhoc, create ad-hoc network immediately. The blocking
    // subprocess work runs off the async runtime; on failure the network
    // state has already been restored by the time the error is returned.
    #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
    if force_adhoc {
        info("Creating ad-hoc WiFi network (forced)...");
        let network = AdHocNetwork::new(&device_name);

        let (network, create_result) = tokio::task::spawn_blocking(move || {
            let mut network = network;
            let result = network.create_network();
            (network, result)
        })
        .await?;

        match create_result {
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
                #[cfg(target_os = "windows")]
                println!("  {} Password: {}", "3.".cyan(), network.password().cyan());
                println!();
                adhoc_network = Some(network);
            }
            Err(e) => {
                // On macOS the error already carries the manual creation
                // steps (modern macOS cannot create ad-hoc networks from the
                // command line at all).
                warn(&format!(
                    "Could not create ad-hoc network automatically: {}",
                    e
                ));
                println!();
                #[cfg(target_os = "linux")]
                {
                    println!("{}", "To create manually:".dimmed());
                    println!(
                        "  {} Run: nmcli con add type wifi ifname <wifi-interface> mode adhoc ssid \"{}\"",
                        "1.".cyan(),
                        network.network_name()
                    );
                    println!(
                        "  {} Configure: nmcli con modify \"{}\" ipv4.method manual ipv4.addresses 192.168.73.1/24",
                        "2.".cyan(),
                        network.network_name()
                    );
                    println!(
                        "  {} Activate: nmcli con up \"{}\"",
                        "3.".cyan(),
                        network.network_name()
                    );
                }
                #[cfg(target_os = "windows")]
                {
                    println!("{}", "To create manually (run as Administrator):".dimmed());
                    println!(
                        "  {} netsh wlan set hostednetwork mode=allow ssid=\"{}\" key=\"yourpassword\"",
                        "1.".cyan(),
                        network.network_name()
                    );
                    println!("  {} netsh wlan start hostednetwork", "2.".cyan());
                }
                println!();
            }
        }
    }

    // Show local addresses
    let addresses = get_local_addresses();
    if addresses.is_empty() && !force_adhoc {
        error("No network interfaces found");
        return Err(SilentExit.into());
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

    // Start Bluetooth advertising if enabled (Linux only)
    #[cfg(feature = "bluetooth")]
    let mut bluetooth_handler: Option<FallbackHandler> = None;

    #[cfg(feature = "bluetooth")]
    if bluetooth_enabled {
        let mut handler = FallbackHandler::new(&device_name, Duration::from_secs(60));

        // Find first IPv4 address to advertise
        if let Some(ip) = addresses.iter().find(|a| a.is_ipv4()) {
            match handler
                .start_bluetooth_advertising(&device_name, *ip, port)
                .await
            {
                Ok(()) => {
                    success("Bluetooth advertising started");
                    bluetooth_handler = Some(handler);
                }
                Err(e) => {
                    warn(&format!("Bluetooth advertising failed: {}", e));
                    #[cfg(not(target_os = "linux"))]
                    {
                        println!(
                            "  {} Bluetooth advertising is only supported on Linux.",
                            "→".cyan()
                        );
                    }
                    #[cfg(target_os = "linux")]
                    {
                        println!(
                            "  {} Ensure BlueZ is installed: sudo apt install bluez",
                            "→".cyan()
                        );
                        println!(
                            "  {} Add user to bluetooth group: sudo usermod -aG bluetooth $USER",
                            "→".cyan()
                        );
                    }
                }
            }
        } else {
            warn("No IPv4 address found for Bluetooth advertising");
        }
    }

    // Suppress unused variable warning when bluetooth feature is disabled
    #[cfg(not(feature = "bluetooth"))]
    let _ = bluetooth_enabled;

    // Start handshake server. With --verify each pairing request must be
    // approved interactively before the key is installed; the prompt shows
    // the received key's fingerprint and a verification code derived from the
    // key material, to be compared with the code shown on the pairing device.
    // Without --verify pairing is auto-accepted, and the fingerprint/code of
    // what was installed are printed by the event handler below.
    let mut server = HandshakeServer::new(key_manager, &device_name);
    if verify {
        // The callback blocks on user input; HandshakeServer invokes it via
        // spawn_blocking, so this never stalls the async runtime. The mutex
        // serializes concurrent approval prompts (continuous mode can serve
        // several handshakes at once): at most one prompt owns the TTY at a
        // time, so an answer is always bound to the request shown above it.
        let prompt_gate = std::sync::Mutex::new(());
        server = server.with_approval(Box::new(move |request: &PairingRequest| {
            let _guard = prompt_gate
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            println!();
            println!("{}", "Pairing approval required".yellow().bold());
            println!(
                "  {} Device:      {}",
                "•".cyan(),
                request.device_name.cyan().bold()
            );
            println!("  {} Key comment: {}", "•".cyan(), request.key_comment);
            println!("  {} Fingerprint: {}", "•".cyan(), request.fingerprint);
            println!(
                "  {} Code:        {}",
                "•".cyan(),
                request.verification_code.green().bold()
            );
            println!(
                "  {}",
                "Compare the code with the one shown on the pairing device.".dimmed()
            );
            Confirm::new()
                .with_prompt("Approve this pairing?")
                .default(false)
                .interact()
                .unwrap_or(false)
        }));
    }
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

    // Local addresses for the cross-subnet heuristic below: a client that is
    // loopback or one of our own addresses is never a VPN/cross-subnet peer.
    let local_addresses = addresses.clone();

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
                ServerEvent::KeyReceived {
                    comment,
                    fingerprint,
                    verification_code,
                } => {
                    info(&format!("Received key: {}", comment.dimmed()));
                    info(&format!("Fingerprint: {}", fingerprint));
                    info(&format!(
                        "Verification code: {} {}",
                        verification_code.bold(),
                        "(compare with the pairing device)".dimmed()
                    ));
                }
                ServerEvent::PairingComplete { device_name } => {
                    println!();
                    success(&format!(
                        "Successfully paired with {}!",
                        device_name.green().bold()
                    ));
                    println!("  {} They can now SSH to this machine.", "→".cyan());

                    // Check if client is from a different subnet (VPN scenario).
                    // Loopback clients and our own addresses are local by
                    // definition; suggesting `config add-subnet 127.0.0.0/24`
                    // would be useless.
                    if let Some(ref client_ip) = last_client_ip {
                        let is_local_client = client_ip
                            .parse::<std::net::IpAddr>()
                            .map(|ip| ip.is_loopback() || local_addresses.contains(&ip))
                            .unwrap_or(false);
                        let client_subnet: String =
                            client_ip.split('.').take(3).collect::<Vec<_>>().join(".");

                        if !is_local_client && !local_subnets.iter().any(|s| s == &client_subnet) {
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

    // Run server. Both modes race against Ctrl+C so the cleanup below
    // (mDNS unregister, event drain, Bluetooth stop, and the ad-hoc
    // network Drop at the end of this scope) also runs on interrupt.
    let server_result: Result<()> = if continuous {
        // Run continuously until Ctrl+C. On interrupt we SIGNAL the server
        // instead of dropping its future, so it stops accepting and drains
        // in-flight handshakes (with its internal grace period) rather than
        // aborting them between key install and confirmation.
        info("Running in continuous mode (Ctrl+C to stop)...");
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
        let server_fut = server.run_with_shutdown(event_tx, shutdown_rx);
        tokio::pin!(server_fut);
        let run_result = tokio::select! {
            result = &mut server_fut => result,
            _ = tokio::signal::ctrl_c() => {
                println!();
                info("Shutting down...");
                let _ = shutdown_tx.send(());
                server_fut.await
            }
        };
        match run_result {
            Ok(()) => Ok(()),
            Err(e) => {
                error(&format!("Server error: {}", e));
                Err(SilentExit.into())
            }
        }
    } else {
        // Default: handle one pairing and exit
        tokio::select! {
            result = server.handle_one(event_tx) => result.map_err(Into::into),
            _ = tokio::signal::ctrl_c() => {
                println!();
                info("Shutting down...");
                Ok(())
            }
        }
    };

    // The server future has completed or been dropped by now, so every event
    // sender is gone; await the handler (instead of aborting it) so queued
    // pairing output drains before anything below prints.
    let _ = event_handler.await;

    // Clean up
    advertiser.stop()?;

    // Stop Bluetooth advertising
    #[cfg(feature = "bluetooth")]
    if let Some(mut handler) = bluetooth_handler {
        let _ = handler.stop_bluetooth_advertising().await;
    }

    // Tear down the ad-hoc network and restore the previous WiFi state
    // (explicitly and off the async runtime; Drop is only the backstop)
    #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
    if let Some(network) = adhoc_network.take() {
        let _ = tokio::task::spawn_blocking(move || {
            let mut network = network;
            if let Err(e) = network.cleanup() {
                warn(&format!("Could not restore the previous network: {}", e));
            }
        })
        .await;
    }

    server_result?;
    success("Connecto listener stopped");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_get_hostname_works() {
        let hostname = get_hostname();
        assert!(!hostname.is_empty());
    }
}
