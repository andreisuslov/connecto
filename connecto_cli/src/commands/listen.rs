//! Listen command - Start listening for pairing requests

use anyhow::Result;
use colored::Colorize;
use connecto_core::{
    discovery::{get_hostname, get_local_addresses, ServiceAdvertiser},
    keys::KeyManager,
    protocol::{HandshakeServer, ServerEvent},
};
use tokio::sync::mpsc;

use super::{error, info, success};

pub async fn run(port: u16, name: Option<String>, verify: bool, once: bool) -> Result<()> {
    let device_name = name.unwrap_or_else(get_hostname);
    let key_manager = KeyManager::new()?;

    // Print header
    println!();
    println!("{}", "  CONNECTO LISTENER  ".on_bright_blue().white().bold());
    println!();

    // Show local addresses
    let addresses = get_local_addresses();
    if addresses.is_empty() {
        error("No network interfaces found");
        return Ok(());
    }

    info(&format!("Device name: {}", device_name.cyan()));
    info(&format!("Port: {}", port.to_string().cyan()));
    println!();

    println!("{}", "Local IP addresses:".bold());
    for addr in &addresses {
        if addr.is_ipv4() {
            println!("  {} {}", "•".green(), addr);
        }
    }
    println!();

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

    // Handle events in a separate task
    let event_handler = tokio::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            match event {
                ServerEvent::Started { address } => {
                    info(&format!("Server started on {}", address));
                }
                ServerEvent::ClientConnected { address } => {
                    println!();
                    info(&format!(
                        "Connection from {}",
                        address.to_string().yellow()
                    ));
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
                    println!(
                        "  {} They can now SSH to this machine.",
                        "→".cyan()
                    );
                    println!();
                }
                ServerEvent::Error { message } => {
                    error(&format!("Error: {}", message));
                }
            }
        }
    });

    // Run server
    if once {
        info("Waiting for a single pairing request...");
        server.handle_one(event_tx).await?;
        success("Pairing complete, exiting.");
    } else {
        // Run continuously
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
