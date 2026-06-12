//! Pair command - Initiate pairing with a device

use anyhow::{anyhow, Result};
use colored::Colorize;
use connecto_core::{
    discovery::get_hostname, keys::KeyManager, protocol::HandshakeClient, sanitize_device_name,
    AddOutcome, Config, HostEntry, SshConfig, DEFAULT_PORT,
};

use super::scan::load_cached_devices;
use super::{error, info, resolve_key_pair, spinner, success, warn};

pub async fn run(
    target: String,
    comment: Option<String>,
    rsa: bool,
    key_path: Option<String>,
) -> Result<()> {
    println!();
    println!(
        "{}",
        "  CONNECTO PAIRING  ".on_bright_magenta().white().bold()
    );
    println!();

    // Resolve target to address
    let address = resolve_target(&target)?;

    info(&format!("Connecting to {}...", address.cyan()));
    println!();

    // Determine which key to use
    // Priority: 1. --key flag, 2. config default_key, 3. generate new key
    let config = Config::load().unwrap_or_default();
    let (key_pair, existing_key_path) =
        resolve_key_pair(key_path.as_deref(), rsa, comment.as_deref(), &config)?;

    let spinner = spinner("magenta", "Connecting and exchanging keys...");

    // Create client and pair
    let client = HandshakeClient::new(&get_hostname());
    let result = client.pair(&address, &key_pair).await;

    spinner.finish_and_clear();

    match result {
        Ok(pairing_result) => {
            println!();
            success("Pairing successful!");
            println!();
            // Derived locally from the key we sent; the listener derives the
            // same code from the key it received, so matching codes rule out
            // a man-in-the-middle that swapped the key.
            info(&format!(
                "Verification code: {} — confirm it matches on {}",
                pairing_result.verification_code.green().bold(),
                pairing_result.server_name.cyan()
            ));
            println!();

            // Determine the key path to use in SSH config
            let private_path = if let Some(path) = existing_key_path {
                println!("{}", "Using existing key:".bold());
                println!("  {} {}", "•".green(), path.display().to_string().dimmed());
                println!();
                path
            } else {
                // Save the new key locally
                let key_manager = KeyManager::new()?;
                let key_name = format!(
                    "connecto_{}",
                    sanitize_device_name(&pairing_result.server_name)
                );
                let (private_path, public_path) =
                    key_manager.save_key_pair(&key_pair, &key_name)?;

                println!("{}", "Key saved:".bold());
                println!(
                    "  {} Private: {}",
                    "•".green(),
                    private_path.display().to_string().dimmed()
                );
                println!(
                    "  {} Public:  {}",
                    "•".green(),
                    public_path.display().to_string().dimmed()
                );
                println!();
                private_path
            };

            // Auto-configure SSH config
            let primary_ip = extract_ip_from_address(&address);
            let host_alias = sanitize_device_name(&pairing_result.server_name);
            let entry = HostEntry {
                host: host_alias.clone(),
                hostname: primary_ip.clone(),
                user: pairing_result.ssh_user.clone(),
                identity_file: private_path.display().to_string(),
            };

            match SshConfig::at_default().and_then(|cfg| cfg.add_host(&entry)) {
                Ok(AddOutcome::Added) => {
                    success(&format!("Added to ~/.ssh/config as '{}'", host_alias));
                    println!();
                    println!("{}", "You can now connect with:".bold());
                    println!();
                    println!("  {}", format!("ssh {}", host_alias).cyan().bold());
                }
                Ok(AddOutcome::Replaced) => {
                    info(&format!(
                        "Host '{}' already in ~/.ssh/config (entry updated)",
                        host_alias
                    ));
                    println!();
                    println!("{}", "You can connect with:".bold());
                    println!();
                    println!("  {}", format!("ssh {}", host_alias).cyan().bold());
                }
                Err(e) => {
                    warn(&format!("Could not update ~/.ssh/config: {}", e));
                    println!();
                    println!("{}", "You can connect with:".bold());
                    println!();
                    println!(
                        "  {}",
                        format!(
                            "ssh -i {} {}@{}",
                            private_path.display(),
                            pairing_result.ssh_user,
                            primary_ip
                        )
                        .cyan()
                        .bold()
                    );
                }
            }
            println!();
        }
        Err(e) => {
            error(&format!("Pairing failed: {}", e));
            println!();
            println!("{}", "Troubleshooting:".bold());
            println!(
                "  {} Make sure the target is running 'connecto listen'",
                "•".dimmed()
            );
            println!("  {} Check that the address is correct", "•".dimmed());
            println!("  {} Verify firewall allows the connection", "•".dimmed());
            println!();
            return Err(e.into());
        }
    }

    Ok(())
}

fn resolve_target(target: &str) -> Result<String> {
    // First, check if it's a number (device index from scan, 0-based)
    if let Ok(index) = target.parse::<usize>() {
        let devices = load_cached_devices().map_err(|_| {
            anyhow!(
                "No cached devices found. Run 'connecto scan' first, or provide an IP:port address."
            )
        })?;

        if index >= devices.len() {
            return Err(anyhow!(
                "Invalid device number {}. Run 'connecto scan' to see available devices (0-{}).",
                index,
                devices.len().saturating_sub(1)
            ));
        }

        let device = &devices[index];
        device
            .connection_string()
            .ok_or_else(|| anyhow!("Device {} has no IP address", device.name))
    } else if target.contains(':') {
        // It's an address with port
        Ok(target.to_string())
    } else {
        // It's just an IP, add default port
        Ok(format!("{}:{}", target, DEFAULT_PORT))
    }
}

fn extract_ip_from_address(address: &str) -> String {
    address.split(':').next().unwrap_or(address).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_ip_from_address() {
        assert_eq!(extract_ip_from_address("192.168.1.1:8099"), "192.168.1.1");
        assert_eq!(extract_ip_from_address("10.0.0.1"), "10.0.0.1");
    }

    #[test]
    fn test_resolve_target_with_port() {
        let result = resolve_target("192.168.1.1:8080").unwrap();
        assert_eq!(result, "192.168.1.1:8080");
    }

    #[test]
    fn test_resolve_target_without_port() {
        let result = resolve_target("192.168.1.1").unwrap();
        assert_eq!(result, format!("192.168.1.1:{}", DEFAULT_PORT));
    }
}
