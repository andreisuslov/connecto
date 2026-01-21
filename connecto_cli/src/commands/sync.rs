//! Sync command - Bidirectional SSH key pairing between two devices

use anyhow::Result;
use colored::Colorize;
use connecto_core::{
    discovery::{get_hostname, get_local_addresses},
    keys::{KeyAlgorithm, KeyManager, SshKeyPair},
    sync::{SyncEvent, SyncHandler},
};
use std::path::PathBuf;
use tokio::sync::mpsc;

use super::{error, info, success, warn};

pub async fn run(
    port: u16,
    name: Option<String>,
    timeout_secs: u64,
    use_rsa: bool,
    key_path: Option<String>,
) -> Result<()> {
    let device_name = name.unwrap_or_else(get_hostname);
    let key_manager = KeyManager::new()?;

    // Print header
    println!();
    println!("{}", "  CONNECTO SYNC  ".on_bright_magenta().white().bold());
    println!();

    // Show local addresses
    let addresses = get_local_addresses();
    if addresses.is_empty() {
        error("No network interfaces found");
        return Ok(());
    }

    info(&format!("Device name: {}", device_name.cyan()));
    info(&format!("Port: {}", port.to_string().cyan()));
    info(&format!("Timeout: {}s", timeout_secs.to_string().cyan()));
    println!();

    println!("{}", "Local IP addresses:".bold());
    for addr in &addresses {
        if addr.is_ipv4() {
            println!("  {} {}", "•".green(), addr);
        }
    }
    println!();

    // Get or generate key pair
    let key_pair = if let Some(key_path) = key_path {
        info(&format!("Using existing key: {}", key_path.dimmed()));
        SshKeyPair::load_from_file(&key_path)?
    } else {
        let algorithm = if use_rsa {
            KeyAlgorithm::Rsa4096
        } else {
            KeyAlgorithm::Ed25519
        };

        // Generate key for this sync
        let user = std::env::var("USER")
            .or_else(|_| std::env::var("USERNAME"))
            .unwrap_or_else(|_| "user".to_string());
        let comment = format!("{}@{}", user, device_name);

        info(&format!(
            "Generating {} key for sync...",
            if use_rsa { "RSA-4096" } else { "Ed25519" }
        ));

        let key_pair = SshKeyPair::generate(algorithm, &comment)?;

        // Save the key
        let key_name = format!("connecto_sync_{}", sanitize_hostname(&device_name));
        let (priv_path, _pub_path) = key_manager.save_key_pair(&key_pair, &key_name)?;

        info(&format!(
            "Key saved: {}",
            priv_path.display().to_string().dimmed()
        ));

        key_pair
    };

    println!();
    println!("{}", "Waiting for sync peer...".magenta().bold());
    println!(
        "{}",
        "Run 'connecto sync' on another device on the same network".dimmed()
    );
    println!("{}", "Press Ctrl+C to cancel".dimmed());
    println!();

    // Create sync handler
    let sync_key_manager = KeyManager::new()?;
    let handler = SyncHandler::new(sync_key_manager, &device_name, key_pair.clone());

    // Create event channel
    let (event_tx, mut event_rx) = mpsc::channel(10);

    // Handle events in a separate task
    let event_handler = tokio::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            match event {
                SyncEvent::Started { address } => {
                    info(&format!("Listening on {}", address.to_string().cyan()));
                }
                SyncEvent::Searching => {
                    info("Searching for sync peers via mDNS...");
                }
                SyncEvent::PeerFound {
                    device_name,
                    address,
                } => {
                    println!();
                    info(&format!(
                        "Found peer: {} ({})",
                        device_name.cyan().bold(),
                        address
                    ));
                }
                SyncEvent::Connected { device_name } => {
                    info(&format!("Connected to {}", device_name.cyan().bold()));
                }
                SyncEvent::KeyReceived {
                    device_name,
                    key_comment,
                } => {
                    info(&format!(
                        "Received key from {}: {}",
                        device_name.cyan(),
                        key_comment.dimmed()
                    ));
                }
                SyncEvent::KeyAccepted => {
                    info("Our key was accepted by peer");
                }
                SyncEvent::Completed {
                    peer_name,
                    peer_user: _,
                } => {
                    println!();
                    success(&format!(
                        "Sync completed with {}!",
                        peer_name.green().bold()
                    ));
                    println!("  {} Bidirectional SSH access established.", "→".cyan());
                    println!(
                        "  {} You can SSH to them, and they can SSH to you.",
                        "→".cyan()
                    );
                }
                SyncEvent::Failed { message } => {
                    error(&format!("Sync failed: {}", message));
                }
            }
        }
    });

    // Run sync with Ctrl+C handling
    let result = tokio::select! {
        result = handler.run(port, timeout_secs, event_tx) => result,
        _ = tokio::signal::ctrl_c() => {
            println!();
            info("Sync cancelled by user");
            event_handler.abort();
            return Ok(());
        }
    };

    event_handler.abort();

    match result {
        Ok(sync_result) => {
            println!();
            println!("{}", "Sync Summary:".bold());
            println!("  {} Peer: {}", "•".green(), sync_result.peer_name.cyan());
            println!("  {} User: {}", "•".green(), sync_result.peer_user);
            println!(
                "  {} Address: {}:{}",
                "•".green(),
                sync_result.peer_address,
                sync_result.peer_port
            );
            println!();

            // Add to SSH config
            add_to_ssh_config(
                &sync_result.peer_name,
                &sync_result.peer_address.to_string(),
                &sync_result.peer_user,
                &key_pair,
                &key_manager,
            )?;

            println!("{}", "Next steps:".bold());
            let host_alias = sanitize_hostname(&sync_result.peer_name);
            println!(
                "  {} SSH to peer: {}",
                "→".cyan(),
                format!("ssh {}", host_alias).green()
            );
            println!();

            success("Sync successful!");
        }
        Err(e) => {
            println!();
            error(&format!("Sync failed: {}", e));

            // Provide helpful suggestions
            println!();
            println!("{}", "Troubleshooting:".bold());
            println!(
                "  {} Make sure both devices are on the same network",
                "•".dimmed()
            );
            println!("  {} Check that mDNS/Bonjour is not blocked", "•".dimmed());
            println!(
                "  {} Try increasing timeout: {}",
                "•".dimmed(),
                format!("connecto sync --timeout {}", timeout_secs * 2).cyan()
            );
        }
    }

    Ok(())
}

/// Sanitize hostname for use as SSH host alias
fn sanitize_hostname(hostname: &str) -> String {
    hostname
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

/// Add synced peer to SSH config
fn add_to_ssh_config(
    peer_name: &str,
    peer_ip: &str,
    peer_user: &str,
    _key_pair: &SshKeyPair,
    _key_manager: &KeyManager,
) -> Result<()> {
    use std::fs::{self, OpenOptions};
    use std::io::Write;

    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map_err(|_| anyhow::anyhow!("HOME/USERPROFILE not set"))?;

    let ssh_dir = PathBuf::from(&home).join(".ssh");
    let config_path = ssh_dir.join("config");

    // Ensure .ssh directory exists
    if !ssh_dir.exists() {
        fs::create_dir_all(&ssh_dir)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&ssh_dir, fs::Permissions::from_mode(0o700))?;
        }
    }

    let host_alias = sanitize_hostname(peer_name);

    // Check if host already exists
    if config_path.exists() {
        let content = fs::read_to_string(&config_path)?;
        if content.contains(&format!("Host {}", host_alias)) {
            warn(&format!(
                "Host '{}' already exists in SSH config, updating...",
                host_alias
            ));
            // Remove existing entry and re-add
            let new_content = remove_host_from_config(&content, &host_alias);
            fs::write(&config_path, new_content)?;
        }
    }

    // Find the identity file path
    let key_name = format!("connecto_sync_{}", host_alias);
    let identity_file = ssh_dir.join(&key_name);

    // Write SSH config entry
    let entry = format!(
        "\n# Added by connecto\nHost {}\n    HostName {}\n    User {}\n    IdentityFile {}\n",
        host_alias,
        peer_ip,
        peer_user,
        identity_file.display()
    );

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&config_path)?;

    file.write_all(entry.as_bytes())?;

    info(&format!("Added '{}' to SSH config", host_alias.cyan()));

    Ok(())
}

/// Remove a host entry from SSH config content
fn remove_host_from_config(content: &str, host_alias: &str) -> String {
    let mut new_lines: Vec<&str> = Vec::new();
    let mut skip_block = false;

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed == "# Added by connecto" {
            // Check if this is for our host
            skip_block = false;
            new_lines.push(line);
            continue;
        }

        if trimmed.starts_with("Host ") && !trimmed.contains('*') {
            let current_host = trimmed.strip_prefix("Host ").unwrap().trim();
            if current_host == host_alias {
                skip_block = true;
                // Remove the "# Added by connecto" line we just added
                if new_lines.last().map(|l| l.trim()) == Some("# Added by connecto") {
                    new_lines.pop();
                }
                continue;
            }
        }

        if skip_block {
            if trimmed.starts_with("IdentityFile ") {
                skip_block = false;
            }
            continue;
        }

        new_lines.push(line);
    }

    new_lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_hostname() {
        assert_eq!(sanitize_hostname("My-Laptop"), "my-laptop");
        assert_eq!(sanitize_hostname("Device (Work)"), "device--work");
        assert_eq!(sanitize_hostname("Test.local."), "test-local");
        assert_eq!(sanitize_hostname("---test---"), "test");
    }

    #[test]
    fn test_remove_host_from_config() {
        let config = r#"# Some comment
Host existing
    HostName 1.2.3.4
    User alice

# Added by connecto
Host target-host
    HostName 5.6.7.8
    User bob
    IdentityFile ~/.ssh/connecto_target

Host another
    HostName 9.10.11.12
    User charlie
"#;

        let result = remove_host_from_config(config, "target-host");
        assert!(!result.contains("target-host"));
        assert!(result.contains("existing"));
        assert!(result.contains("another"));
    }

    #[test]
    fn test_module_compiles() {
        assert!(true);
    }
}
