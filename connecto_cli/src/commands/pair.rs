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
use std::time::Duration;

use super::scan::load_cached_devices;
use super::{error, info, success, warn};

pub async fn run(target: String, comment: Option<String>, rsa: bool) -> Result<()> {
    println!();
    println!("{}", "  CONNECTO PAIRING  ".on_bright_magenta().white().bold());
    println!();

    // Resolve target to address
    let address = resolve_target(&target)?;

    info(&format!("Connecting to {}...", address.cyan()));
    println!();

    // Generate key
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

    // Create spinner for key generation
    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏")
            .template("{spinner:.magenta} {msg}")
            .unwrap(),
    );
    spinner.set_message("Generating SSH key pair...");
    spinner.enable_steady_tick(Duration::from_millis(80));

    let key_pair = SshKeyPair::generate(algorithm, &key_comment)?;

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

            // Save the key locally
            let key_manager = KeyManager::new()?;
            let key_name = format!("connecto_{}", sanitize_name(&pairing_result.server_name));
            let (private_path, public_path) = key_manager.save_key_pair(&key_pair, &key_name)?;

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

            // Show SSH command
            let primary_ip = extract_ip_from_address(&address);
            println!("{}", "You can now connect with:".bold());
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
            println!();

            // Show how to add to SSH config
            println!("{}", "Or add this to your ~/.ssh/config:".dimmed());
            println!();
            println!("  {}", format!("Host {}", pairing_result.server_name.replace(' ', "-")).dimmed());
            println!("  {}", format!("    HostName {}", primary_ip).dimmed());
            println!("  {}", format!("    User {}", pairing_result.ssh_user).dimmed());
            println!(
                "  {}",
                format!("    IdentityFile {}", private_path.display()).dimmed()
            );
            println!();
        }
        Err(e) => {
            error(&format!("Pairing failed: {}", e));
            println!();
            println!("{}", "Troubleshooting:".bold());
            println!("  {} Make sure the target is running 'connecto listen'", "•".dimmed());
            println!("  {} Check that the address is correct", "•".dimmed());
            println!("  {} Verify firewall allows the connection", "•".dimmed());
            println!();
            return Err(e.into());
        }
    }

    Ok(())
}

fn resolve_target(target: &str) -> Result<String> {
    // First, check if it's a number (device index from scan)
    if let Ok(index) = target.parse::<usize>() {
        let devices = load_cached_devices().map_err(|_| {
            anyhow!(
                "No cached devices found. Run 'connecto scan' first, or provide an IP:port address."
            )
        })?;

        if index == 0 || index > devices.len() {
            return Err(anyhow!(
                "Invalid device number {}. Run 'connecto scan' to see available devices.",
                index
            ));
        }

        let device = &devices[index - 1];
        device.connection_string().ok_or_else(|| {
            anyhow!("Device {} has no IP address", device.name)
        })
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
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect::<String>()
        .to_lowercase()
}

fn extract_ip_from_address(address: &str) -> String {
    address
        .split(':')
        .next()
        .unwrap_or(address)
        .to_string()
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
