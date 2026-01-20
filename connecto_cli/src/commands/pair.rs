//! Pair command - Initiate pairing with a device

use anyhow::{anyhow, Result};
use colored::Colorize;
use connecto_core::{
    discovery::get_hostname,
    keys::{KeyAlgorithm, KeyManager, SshKeyPair},
    protocol::HandshakeClient,
    DEFAULT_PORT,
};
use indicatif::{ProgressBar, ProgressStyle};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;

use super::scan::load_cached_devices;
use super::{error, info, success, warn};
use crate::config::Config;

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
    let effective_key_path =
        key_path.or_else(|| Config::load().ok().and_then(|cfg| cfg.default_key.clone()));

    // Create spinner
    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏")
            .template("{spinner:.magenta} {msg}")
            .unwrap(),
    );

    let (key_pair, using_existing_key, existing_key_path) =
        if let Some(key_file) = effective_key_path {
            // Use existing key
            let expanded_path = expand_path(&key_file)?;
            let pub_key_path = format!("{}.pub", expanded_path);

            if !std::path::Path::new(&expanded_path).exists() {
                return Err(anyhow!("Key file not found: {}", expanded_path));
            }
            if !std::path::Path::new(&pub_key_path).exists() {
                return Err(anyhow!("Public key not found: {}", pub_key_path));
            }

            info(&format!("Using existing key: {}", expanded_path.cyan()));

            spinner.set_message("Loading existing SSH key...");
            spinner.enable_steady_tick(Duration::from_millis(80));

            let key_pair = SshKeyPair::load_from_file(&expanded_path)?;
            (key_pair, true, Some(expanded_path))
        } else {
            // Generate new key
            let algorithm = if rsa {
                warn("Using RSA-4096 (Ed25519 is recommended for better security)");
                KeyAlgorithm::Rsa4096
            } else {
                info("Using Ed25519 key (modern, secure, fast)");
                KeyAlgorithm::Ed25519
            };

            let key_comment = comment.unwrap_or_else(|| {
                let user = std::env::var("USER")
                    .or_else(|_| std::env::var("USERNAME"))
                    .unwrap_or_else(|_| "user".to_string());
                let hostname = get_hostname();
                format!("{}@{}", user, hostname)
            });

            spinner.set_message("Generating SSH key pair...");
            spinner.enable_steady_tick(Duration::from_millis(80));

            let key_pair = SshKeyPair::generate(algorithm, &key_comment)?;
            (key_pair, false, None)
        };

    spinner.set_message("Connecting and exchanging keys...");

    // Create client and pair
    let client = HandshakeClient::new(&get_hostname());
    let result = client.pair(&address, &key_pair).await;

    spinner.finish_and_clear();

    match result {
        Ok(pairing_result) => {
            println!();
            success("Pairing successful!");
            println!();

            // Determine the key path to use in SSH config
            let private_path = if using_existing_key {
                // Use the existing key path
                let path = existing_key_path.unwrap();
                println!("{}", "Using existing key:".bold());
                println!("  {} {}", "•".green(), path.dimmed());
                println!();
                PathBuf::from(path)
            } else {
                // Save the new key locally
                let key_manager = KeyManager::new()?;
                let key_name = format!("connecto_{}", sanitize_name(&pairing_result.server_name));
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
            let host_alias = sanitize_name(&pairing_result.server_name);

            match add_to_ssh_config(
                &host_alias,
                &primary_ip,
                &pairing_result.ssh_user,
                &private_path,
            ) {
                Ok(true) => {
                    success(&format!("Added to ~/.ssh/config as '{}'", host_alias));
                    println!();
                    println!("{}", "You can now connect with:".bold());
                    println!();
                    println!("  {}", format!("ssh {}", host_alias).cyan().bold());
                }
                Ok(false) => {
                    info(&format!("Host '{}' already in ~/.ssh/config", host_alias));
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

fn sanitize_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>()
        .to_lowercase()
}

fn extract_ip_from_address(address: &str) -> String {
    address.split(':').next().unwrap_or(address).to_string()
}

/// Expand ~ to home directory in path
fn expand_path(path: &str) -> Result<String> {
    if path.starts_with("~/") {
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .map_err(|_| anyhow!("HOME/USERPROFILE not set"))?;
        Ok(path.replacen("~", &home, 1))
    } else {
        Ok(path.to_string())
    }
}

/// Add a host entry to ~/.ssh/config
/// Returns Ok(true) if added, Ok(false) if already exists, Err on failure
fn add_to_ssh_config(
    host: &str,
    hostname: &str,
    user: &str,
    identity_file: &PathBuf,
) -> Result<bool> {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map_err(|_| anyhow!("HOME/USERPROFILE not set"))?;
    let ssh_dir = PathBuf::from(&home).join(".ssh");
    let config_path = ssh_dir.join("config");

    // Create .ssh directory if it doesn't exist
    if !ssh_dir.exists() {
        std::fs::create_dir_all(&ssh_dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&ssh_dir, std::fs::Permissions::from_mode(0o700))?;
        }
    }

    // Check if host already exists in config
    if config_path.exists() {
        let content = std::fs::read_to_string(&config_path)?;
        let host_pattern = format!("Host {}", host);
        if content
            .lines()
            .any(|line| line.trim() == host_pattern || line.trim() == format!("Host {}", host))
        {
            return Ok(false); // Already exists
        }
    }

    // Append to config
    let entry = format!(
        "\n# Added by connecto\nHost {}\n    HostName {}\n    User {}\n    IdentityFile {}\n",
        host,
        hostname,
        user,
        identity_file.display()
    );

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&config_path)?;

    file.write_all(entry.as_bytes())?;

    // Set proper permissions on config file
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&config_path, std::fs::Permissions::from_mode(0o600))?;
    }

    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_name() {
        assert_eq!(sanitize_name("My Device"), "my_device");
        assert_eq!(sanitize_name("test-host"), "test-host");
        assert_eq!(sanitize_name("Host (123)"), "host__123_");
    }

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

    #[test]
    fn test_resolve_target_invalid_index() {
        // Should fail because there's no cache
        let result = resolve_target("999");
        assert!(result.is_err());
    }
}
