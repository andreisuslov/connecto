//! Sync command - Bidirectional SSH key pairing between two devices

use anyhow::Result;
use colored::Colorize;
use connecto_core::{
    discovery::{get_hostname, get_local_addresses},
    keys::KeyManager,
    sanitize_device_name,
    sync::{SyncEvent, SyncHandler},
    AddOutcome, Config, HostEntry, SshConfig,
};
use std::path::Path;
use tokio::sync::mpsc;

use super::{default_key_comment, error, info, resolve_key_pair, success, warn};
use crate::SilentExit;

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
        return Err(SilentExit.into());
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

    // Determine which key to use
    // Priority: 1. --key flag, 2. config default_key, 3. generate new key
    let config = Config::load().unwrap_or_default();
    let comment = default_key_comment(&device_name);
    let (key_pair, existing_key_path) =
        resolve_key_pair(key_path.as_deref(), use_rsa, Some(&comment), &config)?;

    // The IdentityFile written to ~/.ssh/config below must be the file that
    // actually exists on disk. (Generated keys used to be saved under the
    // LOCAL device name while the config entry pointed at a path derived from
    // the PEER name — a file that was never created.)
    let identity_path = match existing_key_path {
        Some(path) => path,
        None => {
            let key_name = format!("connecto_sync_{}", sanitize_device_name(&device_name));
            let (priv_path, _pub_path) = key_manager.save_key_pair(&key_pair, &key_name)?;

            info(&format!(
                "Key saved: {}",
                priv_path.display().to_string().dimmed()
            ));

            priv_path
        }
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
            let ssh_config = SshConfig::at_default()?;
            let host_alias = write_ssh_config_entry(
                &ssh_config,
                &sync_result.peer_name,
                &sync_result.peer_address.to_string(),
                &sync_result.peer_user,
                &identity_path,
            )?;

            println!("{}", "Next steps:".bold());
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
            return Err(SilentExit.into());
        }
    }

    Ok(())
}

/// Write the synced peer's SSH config entry, pointing at the actual key file
///
/// Returns the host alias the entry was written under.
fn write_ssh_config_entry(
    ssh_config: &SshConfig,
    peer_name: &str,
    peer_ip: &str,
    peer_user: &str,
    identity_file: &Path,
) -> Result<String> {
    let host_alias = sanitize_device_name(peer_name);
    let entry = HostEntry {
        host: host_alias.clone(),
        hostname: peer_ip.to_string(),
        user: peer_user.to_string(),
        identity_file: identity_file.display().to_string(),
    };

    if ssh_config.add_host(&entry)? == AddOutcome::Replaced {
        warn(&format!(
            "Host '{}' already exists in SSH config, updating...",
            host_alias
        ));
    }
    info(&format!("Added '{}' to SSH config", host_alias.cyan()));

    Ok(host_alias)
}

#[cfg(test)]
mod tests {
    use super::*;
    use connecto_core::keys::{KeyAlgorithm, SshKeyPair};
    use tempfile::TempDir;

    /// Regression test for the shipped sync bug: the key was saved under the
    /// LOCAL device name while the config entry pointed at a path derived from
    /// the PEER name, so the written IdentityFile never existed on disk.
    #[test]
    fn test_written_identity_file_exists_on_disk() {
        let temp = TempDir::new().unwrap();
        let ssh_dir = temp.path().join(".ssh");
        let key_manager = KeyManager::with_dir(ssh_dir.clone());

        // Key is saved under the LOCAL device name...
        let key_pair = SshKeyPair::generate(KeyAlgorithm::Ed25519, "user@local").unwrap();
        let key_name = format!("connecto_sync_{}", sanitize_device_name("Local Machine"));
        let (priv_path, _) = key_manager.save_key_pair(&key_pair, &key_name).unwrap();

        // ...while the config entry is written for the PEER.
        let ssh_config = SshConfig::new(ssh_dir.join("config"));
        let alias =
            write_ssh_config_entry(&ssh_config, "Peer Box", "10.0.0.7", "bob", &priv_path).unwrap();

        let hosts = ssh_config.list_hosts().unwrap();
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].host, alias);
        assert_eq!(hosts[0].hostname, "10.0.0.7");
        assert_eq!(hosts[0].user, "bob");
        assert!(
            Path::new(&hosts[0].identity_file).exists(),
            "IdentityFile written to SSH config must exist on disk: {}",
            hosts[0].identity_file
        );
    }

    #[test]
    fn test_write_ssh_config_entry_replaces_existing_alias() {
        let temp = TempDir::new().unwrap();
        let ssh_dir = temp.path().join(".ssh");
        std::fs::create_dir_all(&ssh_dir).unwrap();
        let key_a = ssh_dir.join("connecto_sync_a");
        let key_b = ssh_dir.join("connecto_sync_b");
        std::fs::write(&key_a, "a").unwrap();
        std::fs::write(&key_b, "b").unwrap();

        let ssh_config = SshConfig::new(ssh_dir.join("config"));
        write_ssh_config_entry(&ssh_config, "Peer", "10.0.0.1", "bob", &key_a).unwrap();
        write_ssh_config_entry(&ssh_config, "Peer", "10.0.0.2", "bob", &key_b).unwrap();

        let hosts = ssh_config.list_hosts().unwrap();
        assert_eq!(hosts.len(), 1, "re-sync must update, not duplicate");
        assert_eq!(hosts[0].hostname, "10.0.0.2");
        assert_eq!(hosts[0].identity_file, key_b.display().to_string());
    }
}
