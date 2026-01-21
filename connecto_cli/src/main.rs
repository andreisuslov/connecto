//! Connecto CLI - AirDrop-like SSH pairing tool
//!
//! Usage:
//!   connecto listen    - Start listening for pairing requests
//!   connecto scan      - Scan for available devices
//!   connecto pair <n>  - Pair with device number n

mod commands;
mod config;

use anyhow::Result;
use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::{generate, Shell};
use tracing_subscriber::EnvFilter;

/// Connecto - AirDrop-like SSH key pairing for your terminal
#[derive(Parser)]
#[command(name = "connecto")]
#[command(author = "Connecto Team")]
#[command(version)]
#[command(about = "Easily pair SSH keys between devices on your local network")]
#[command(long_about = r#"
Connecto makes SSH key setup as easy as AirDrop.

On the target machine (where you want to SSH into):
  $ connecto listen

On the client machine (where you want to SSH from):
  $ connecto scan
  $ connecto pair 1

That's it! You can now SSH to the target machine without passwords.
"#)]
struct Cli {
    /// Enable verbose output
    #[arg(short, long, global = true)]
    verbose: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start listening for pairing requests (run on target machine)
    Listen {
        /// Port to listen on
        #[arg(short, long, default_value_t = connecto_core::DEFAULT_PORT)]
        port: u16,

        /// Custom device name (defaults to hostname)
        #[arg(short, long)]
        name: Option<String>,

        /// Require verification code
        #[arg(long)]
        verify: bool,

        /// Keep listening after first pairing (default: exit after one)
        #[arg(short, long)]
        continuous: bool,
    },

    /// Scan the local network for devices running Connecto
    Scan {
        /// How long to scan in seconds
        #[arg(short, long, default_value_t = 5)]
        timeout: u64,

        /// Subnet to scan (e.g., 10.105.225.0/24). Can be specified multiple times.
        #[arg(short, long)]
        subnet: Vec<String>,
    },

    /// Pair with a discovered device
    Pair {
        /// Device number from scan results, or IP:port address
        target: String,

        /// Custom key comment (defaults to user@hostname)
        #[arg(short, long)]
        comment: Option<String>,

        /// Generate RSA key instead of Ed25519
        #[arg(long)]
        rsa: bool,

        /// Use existing SSH key instead of generating a new one
        #[arg(short, long, value_name = "PATH")]
        key: Option<String>,
    },

    /// List authorized keys on this machine
    Keys {
        #[command(subcommand)]
        action: Option<KeysAction>,
    },

    /// Generate a new SSH key pair
    Keygen {
        /// Key name (stored in ~/.ssh/)
        #[arg(short, long, default_value = "connecto_key")]
        name: String,

        /// Key comment
        #[arg(short, long)]
        comment: Option<String>,

        /// Generate RSA key instead of Ed25519
        #[arg(long)]
        rsa: bool,
    },

    /// Manage configuration (saved subnets, etc.)
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },

    /// List paired hosts (from ~/.ssh/config)
    Hosts,

    /// Remove a paired host and delete its keys
    Unpair {
        /// Host name to unpair
        host: String,
    },

    /// Test SSH connection to a paired host
    Test {
        /// Host name to test
        host: String,
    },

    /// Update IP address for a paired host
    UpdateIp {
        /// Host name to update
        host: String,

        /// New IP address
        ip: String,
    },

    /// Export paired hosts configuration
    Export {
        /// Output file (default: stdout)
        #[arg(short, long)]
        output: Option<String>,
    },

    /// Import paired hosts configuration
    Import {
        /// Input file
        file: String,
    },

    /// Generate shell completions
    Completions {
        /// Shell to generate completions for
        #[arg(value_enum)]
        shell: Shell,
    },

    /// Sync SSH keys bidirectionally with another device
    Sync {
        /// Port to use for sync
        #[arg(short, long, default_value_t = connecto_core::DEFAULT_PORT)]
        port: u16,

        /// Custom device name (defaults to hostname)
        #[arg(short, long)]
        name: Option<String>,

        /// Timeout in seconds for peer discovery
        #[arg(short, long, default_value_t = connecto_core::DEFAULT_SYNC_TIMEOUT_SECS)]
        timeout: u64,

        /// Generate RSA key instead of Ed25519
        #[arg(long)]
        rsa: bool,

        /// Use existing SSH key instead of generating a new one
        #[arg(short, long, value_name = "PATH")]
        key: Option<String>,
    },
}

#[derive(Subcommand)]
enum KeysAction {
    /// List all authorized keys
    List,
    /// Remove a key by number or pattern
    Remove {
        /// Key number or search pattern
        target: String,
    },
}

#[derive(Subcommand)]
enum ConfigAction {
    /// Add a subnet to always scan (e.g., 10.105.225.0/24)
    AddSubnet {
        /// Subnet in CIDR notation
        subnet: String,
    },
    /// Remove a saved subnet
    RemoveSubnet {
        /// Subnet to remove
        subnet: String,
    },
    /// Set default SSH key for all pairings
    SetDefaultKey {
        /// Path to private key (e.g., ~/.ssh/id_ed25519)
        key_path: String,
    },
    /// Clear the default SSH key
    ClearDefaultKey,
    /// List current configuration
    List,
    /// Show config file path
    Path,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Set up logging
    let filter = if cli.verbose {
        EnvFilter::new("debug")
    } else {
        EnvFilter::new("info")
    };

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .without_time()
        .with_target(false)
        .init();

    match cli.command {
        Commands::Listen {
            port,
            name,
            verify,
            continuous,
        } => commands::listen::run(port, name, verify, continuous).await,
        Commands::Scan { timeout, subnet } => {
            commands::scan::run_with_options(timeout, false, subnet).await
        }
        Commands::Pair {
            target,
            comment,
            rsa,
            key,
        } => commands::pair::run(target, comment, rsa, key).await,
        Commands::Keys { action } => commands::keys::run(action).await,
        Commands::Keygen { name, comment, rsa } => commands::keygen::run(name, comment, rsa).await,
        Commands::Config { action } => run_config(action),
        Commands::Hosts => run_hosts(),
        Commands::Unpair { host } => run_unpair(&host),
        Commands::Test { host } => run_test(&host),
        Commands::UpdateIp { host, ip } => run_update_ip(&host, &ip),
        Commands::Export { output } => run_export(output.as_deref()),
        Commands::Import { file } => run_import(&file),
        Commands::Completions { shell } => {
            generate(
                shell,
                &mut Cli::command(),
                "connecto",
                &mut std::io::stdout(),
            );
            Ok(())
        }
        Commands::Sync {
            port,
            name,
            timeout,
            rsa,
            key,
        } => commands::sync::run(port, name, timeout, rsa, key).await,
    }
}

fn run_hosts() -> Result<()> {
    use colored::Colorize;
    use std::fs;

    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map_err(|_| anyhow::anyhow!("HOME/USERPROFILE not set"))?;
    let config_path = std::path::PathBuf::from(&home).join(".ssh").join("config");

    if !config_path.exists() {
        println!("{}", "No SSH config file found.".dimmed());
        return Ok(());
    }

    let content = fs::read_to_string(&config_path)?;

    // Find hosts added by connecto
    let mut connecto_hosts: Vec<(String, String, String)> = Vec::new();
    let mut in_connecto_block = false;
    let mut current_host: Option<String> = None;
    let mut current_hostname: Option<String> = None;
    let mut current_user: Option<String> = None;

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed == "# Added by connecto" {
            in_connecto_block = true;
            continue;
        }

        if in_connecto_block {
            if trimmed.starts_with("Host ") && !trimmed.contains('*') {
                // Save previous host if any
                if let (Some(h), Some(hn), Some(u)) = (
                    current_host.take(),
                    current_hostname.take(),
                    current_user.take(),
                ) {
                    connecto_hosts.push((h, hn, u));
                }
                current_host = Some(trimmed.strip_prefix("Host ").unwrap().to_string());
            } else if trimmed.starts_with("HostName ") {
                current_hostname = Some(trimmed.strip_prefix("HostName ").unwrap().to_string());
            } else if trimmed.starts_with("User ") {
                current_user = Some(trimmed.strip_prefix("User ").unwrap().to_string());
            } else if trimmed.starts_with("IdentityFile ") {
                // End of this host block
                if let (Some(h), Some(hn), Some(u)) = (
                    current_host.take(),
                    current_hostname.take(),
                    current_user.take(),
                ) {
                    connecto_hosts.push((h, hn, u));
                }
                in_connecto_block = false;
            } else if trimmed.is_empty() || (trimmed.starts_with("Host ") && !in_connecto_block) {
                in_connecto_block = false;
            }
        }
    }

    // Handle last host if still pending
    if let (Some(h), Some(hn), Some(u)) = (current_host, current_hostname, current_user) {
        connecto_hosts.push((h, hn, u));
    }

    if connecto_hosts.is_empty() {
        println!("{}", "No paired hosts found.".dimmed());
        println!();
        println!(
            "Pair with a device using: {}",
            "connecto scan && connecto pair 0".cyan()
        );
        return Ok(());
    }

    println!("{}", "Paired hosts:".bold());
    println!();
    for (host, hostname, user) in &connecto_hosts {
        println!(
            "  {} {} → {}@{}",
            "•".green(),
            host.cyan().bold(),
            user.dimmed(),
            hostname.dimmed()
        );
    }
    println!();
    println!("{}", "Connect with:".dimmed());
    println!("  {} ssh <hostname>", "→".cyan());
    println!();

    Ok(())
}

fn run_config(action: ConfigAction) -> Result<()> {
    use colored::Colorize;

    match action {
        ConfigAction::AddSubnet { subnet } => {
            let mut cfg = config::Config::load()?;
            if cfg.add_subnet(&subnet) {
                cfg.save()?;
                println!("{} Added subnet: {}", "✓".green(), subnet.cyan());
            } else {
                println!("{} Subnet already exists: {}", "→".yellow(), subnet);
            }
        }
        ConfigAction::RemoveSubnet { subnet } => {
            let mut cfg = config::Config::load()?;
            if cfg.remove_subnet(&subnet) {
                cfg.save()?;
                println!("{} Removed subnet: {}", "✓".green(), subnet);
            } else {
                println!("{} Subnet not found: {}", "✗".red(), subnet);
            }
        }
        ConfigAction::SetDefaultKey { key_path } => {
            // Expand ~ to home directory
            let expanded_path = if key_path.starts_with("~/") {
                let home = std::env::var("HOME")
                    .or_else(|_| std::env::var("USERPROFILE"))
                    .map_err(|_| anyhow::anyhow!("HOME/USERPROFILE not set"))?;
                key_path.replacen("~", &home, 1)
            } else {
                key_path.clone()
            };

            // Verify the key exists
            let key_file = std::path::Path::new(&expanded_path);
            if !key_file.exists() {
                println!("{} Key file not found: {}", "✗".red(), expanded_path);
                return Ok(());
            }

            // Verify it's a valid SSH key (check for public key too)
            let pub_key_path = format!("{}.pub", expanded_path);
            if !std::path::Path::new(&pub_key_path).exists() {
                println!(
                    "{} Public key not found: {}",
                    "✗".red(),
                    pub_key_path.dimmed()
                );
                println!(
                    "  {} Both private and public key files are required.",
                    "→".yellow()
                );
                return Ok(());
            }

            let mut cfg = config::Config::load()?;
            cfg.set_default_key(&expanded_path);
            cfg.save()?;
            println!("{} Default key set: {}", "✓".green(), expanded_path.cyan());
            println!("  {} All future pairings will use this key.", "→".dimmed());
        }
        ConfigAction::ClearDefaultKey => {
            let mut cfg = config::Config::load()?;
            if cfg.default_key.is_some() {
                cfg.clear_default_key();
                cfg.save()?;
                println!("{} Default key cleared.", "✓".green());
                println!("  {} Pairings will generate new keys again.", "→".dimmed());
            } else {
                println!("{} No default key was set.", "→".yellow());
            }
        }
        ConfigAction::List => {
            let cfg = config::Config::load()?;
            let mut has_config = false;

            if !cfg.subnets.is_empty() {
                has_config = true;
                println!("{}", "Configured subnets:".bold());
                for subnet in &cfg.subnets {
                    println!("  {} {}", "•".cyan(), subnet);
                }
            }

            if let Some(key) = &cfg.default_key {
                has_config = true;
                println!();
                println!("{}", "Default SSH key:".bold());
                println!("  {} {}", "•".cyan(), key);
            }

            if !has_config {
                println!("{}", "No configuration set.".dimmed());
                println!();
                println!(
                    "Add subnets to scan with: {}",
                    "connecto config add-subnet <cidr>".cyan()
                );
                println!(
                    "Set default key with: {}",
                    "connecto config set-default-key <path>".cyan()
                );
            }
        }
        ConfigAction::Path => {
            let path = config::Config::path()?;
            println!("{}", path.display());
        }
    }
    Ok(())
}

/// Remove a paired host from SSH config and delete its keys
fn run_unpair(host: &str) -> Result<()> {
    use colored::Colorize;
    use std::fs;

    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map_err(|_| anyhow::anyhow!("HOME/USERPROFILE not set"))?;
    let ssh_dir = std::path::PathBuf::from(&home).join(".ssh");
    let config_path = ssh_dir.join("config");

    if !config_path.exists() {
        println!("{} No SSH config file found.", "✗".red());
        return Ok(());
    }

    let content = fs::read_to_string(&config_path)?;
    let mut new_lines: Vec<&str> = Vec::new();
    let mut skip_block = false;
    let mut found = false;
    let mut identity_file: Option<String> = None;

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed == "# Added by connecto" {
            // Check next line for our host
            skip_block = false;
            new_lines.push(line);
            continue;
        }

        if trimmed.starts_with("Host ") && !trimmed.contains('*') {
            let host_name = trimmed.strip_prefix("Host ").unwrap().trim();
            if host_name == host {
                skip_block = true;
                found = true;
                // Remove the "# Added by connecto" line we just added
                if new_lines.last().map(|l| l.trim()) == Some("# Added by connecto") {
                    new_lines.pop();
                }
                continue;
            }
        }

        if skip_block {
            if trimmed.starts_with("IdentityFile ") {
                identity_file = Some(
                    trimmed
                        .strip_prefix("IdentityFile ")
                        .unwrap()
                        .trim()
                        .to_string(),
                );
            }
            if trimmed.is_empty()
                || (trimmed.starts_with("Host ") && !trimmed.starts_with("HostName"))
            {
                skip_block = false;
                if !trimmed.is_empty() {
                    new_lines.push(line);
                }
            }
            continue;
        }

        new_lines.push(line);
    }

    if !found {
        println!("{} Host '{}' not found in SSH config.", "✗".red(), host);
        return Ok(());
    }

    // Write updated config
    fs::write(&config_path, new_lines.join("\n") + "\n")?;
    println!("{} Removed '{}' from SSH config.", "✓".green(), host.cyan());

    // Delete key files
    if let Some(key_path) = identity_file {
        let key_path = std::path::PathBuf::from(&key_path);
        let pub_path = key_path.with_extension("pub");

        if key_path.exists() {
            fs::remove_file(&key_path)?;
            println!(
                "{} Deleted private key: {}",
                "✓".green(),
                key_path.display().to_string().dimmed()
            );
        }
        if pub_path.exists() {
            fs::remove_file(&pub_path)?;
            println!(
                "{} Deleted public key: {}",
                "✓".green(),
                pub_path.display().to_string().dimmed()
            );
        }
    }

    Ok(())
}

/// Test SSH connection to a paired host
fn run_test(host: &str) -> Result<()> {
    use colored::Colorize;
    use std::process::Command;

    println!(
        "{} Testing connection to {}...",
        "→".cyan(),
        host.cyan().bold()
    );

    let output = Command::new("ssh")
        .args([
            "-o",
            "ConnectTimeout=5",
            "-o",
            "BatchMode=yes",
            host,
            "echo",
            "connecto-ok",
        ])
        .output();

    match output {
        Ok(result) => {
            if result.status.success() {
                let stdout = String::from_utf8_lossy(&result.stdout);
                if stdout.trim() == "connecto-ok" {
                    println!("{} Connection successful!", "✓".green());
                    Ok(())
                } else {
                    println!(
                        "{} Connection established but unexpected response.",
                        "⚠".yellow()
                    );
                    Ok(())
                }
            } else {
                let stderr = String::from_utf8_lossy(&result.stderr);
                println!("{} Connection failed.", "✗".red());
                if !stderr.is_empty() {
                    println!("{}", stderr.dimmed());
                }
                println!();
                println!("{}", "Troubleshooting:".bold());
                println!("  {} Check if the host is online", "•".dimmed());
                println!(
                    "  {} Verify the IP is correct: {}",
                    "•".dimmed(),
                    "connecto hosts".cyan()
                );
                println!(
                    "  {} Update IP if changed: {}",
                    "•".dimmed(),
                    format!("connecto update-ip {} <new-ip>", host).cyan()
                );
                Ok(())
            }
        }
        Err(e) => {
            println!("{} Failed to run ssh: {}", "✗".red(), e);
            Ok(())
        }
    }
}

/// Update IP address for a paired host
fn run_update_ip(host: &str, new_ip: &str) -> Result<()> {
    use colored::Colorize;
    use std::fs;

    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map_err(|_| anyhow::anyhow!("HOME/USERPROFILE not set"))?;
    let config_path = std::path::PathBuf::from(&home).join(".ssh").join("config");

    if !config_path.exists() {
        println!("{} No SSH config file found.", "✗".red());
        return Ok(());
    }

    let content = fs::read_to_string(&config_path)?;
    let mut new_content = String::new();
    let mut in_target_block = false;
    let mut found = false;
    let mut old_ip = String::new();

    for line in content.lines() {
        let trimmed = line.trim();

        if trimmed.starts_with("Host ") && !trimmed.contains('*') {
            let host_name = trimmed.strip_prefix("Host ").unwrap().trim();
            in_target_block = host_name == host;
        }

        if in_target_block && trimmed.starts_with("HostName ") {
            old_ip = trimmed
                .strip_prefix("HostName ")
                .unwrap()
                .trim()
                .to_string();
            new_content.push_str(&format!("    HostName {}\n", new_ip));
            found = true;
            continue;
        }

        new_content.push_str(line);
        new_content.push('\n');
    }

    if !found {
        println!("{} Host '{}' not found in SSH config.", "✗".red(), host);
        return Ok(());
    }

    fs::write(&config_path, new_content)?;
    println!(
        "{} Updated '{}' IP: {} → {}",
        "✓".green(),
        host.cyan(),
        old_ip.dimmed(),
        new_ip.cyan().bold()
    );

    Ok(())
}

/// Export paired hosts configuration
fn run_export(output: Option<&str>) -> Result<()> {
    use colored::Colorize;
    use serde::{Deserialize, Serialize};
    use std::fs;

    #[derive(Serialize, Deserialize)]
    struct ExportedHost {
        host: String,
        hostname: String,
        user: String,
        identity_file: String,
    }

    #[derive(Serialize, Deserialize)]
    struct ExportData {
        version: u32,
        hosts: Vec<ExportedHost>,
        subnets: Vec<String>,
    }

    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map_err(|_| anyhow::anyhow!("HOME/USERPROFILE not set"))?;
    let config_path = std::path::PathBuf::from(&home).join(".ssh").join("config");

    let mut hosts = Vec::new();

    if config_path.exists() {
        let content = fs::read_to_string(&config_path)?;
        let mut in_connecto_block = false;
        let mut current = ExportedHost {
            host: String::new(),
            hostname: String::new(),
            user: String::new(),
            identity_file: String::new(),
        };

        for line in content.lines() {
            let trimmed = line.trim();

            if trimmed == "# Added by connecto" {
                in_connecto_block = true;
                continue;
            }

            if in_connecto_block {
                if trimmed.starts_with("Host ") && !trimmed.contains('*') {
                    if !current.host.is_empty() {
                        hosts.push(current);
                        current = ExportedHost {
                            host: String::new(),
                            hostname: String::new(),
                            user: String::new(),
                            identity_file: String::new(),
                        };
                    }
                    current.host = trimmed.strip_prefix("Host ").unwrap().to_string();
                } else if trimmed.starts_with("HostName ") {
                    current.hostname = trimmed.strip_prefix("HostName ").unwrap().to_string();
                } else if trimmed.starts_with("User ") {
                    current.user = trimmed.strip_prefix("User ").unwrap().to_string();
                } else if trimmed.starts_with("IdentityFile ") {
                    current.identity_file =
                        trimmed.strip_prefix("IdentityFile ").unwrap().to_string();
                    hosts.push(current);
                    current = ExportedHost {
                        host: String::new(),
                        hostname: String::new(),
                        user: String::new(),
                        identity_file: String::new(),
                    };
                    in_connecto_block = false;
                }
            }
        }
    }

    let cfg = config::Config::load().unwrap_or_default();

    let export_data = ExportData {
        version: 1,
        hosts,
        subnets: cfg.subnets,
    };

    let json = serde_json::to_string_pretty(&export_data)?;

    if let Some(path) = output {
        fs::write(path, &json)?;
        println!(
            "{} Exported {} host(s) to {}",
            "✓".green(),
            export_data.hosts.len(),
            path.cyan()
        );
    } else {
        println!("{}", json);
    }

    Ok(())
}

/// Import paired hosts configuration
fn run_import(file: &str) -> Result<()> {
    use colored::Colorize;
    use serde::{Deserialize, Serialize};
    use std::fs;

    #[derive(Serialize, Deserialize)]
    struct ExportedHost {
        host: String,
        hostname: String,
        user: String,
        identity_file: String,
    }

    #[derive(Serialize, Deserialize)]
    struct ExportData {
        version: u32,
        hosts: Vec<ExportedHost>,
        subnets: Vec<String>,
    }

    let content = fs::read_to_string(file)?;
    let data: ExportData = serde_json::from_str(&content)?;

    if data.version != 1 {
        return Err(anyhow::anyhow!(
            "Unsupported export version: {}",
            data.version
        ));
    }

    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map_err(|_| anyhow::anyhow!("HOME/USERPROFILE not set"))?;
    let ssh_dir = std::path::PathBuf::from(&home).join(".ssh");
    let config_path = ssh_dir.join("config");

    // Create .ssh directory if needed
    if !ssh_dir.exists() {
        fs::create_dir_all(&ssh_dir)?;
    }

    // Read existing config
    let mut existing = String::new();
    if config_path.exists() {
        existing = fs::read_to_string(&config_path)?;
    }

    // Add hosts that don't already exist
    let mut added = 0;
    for host in &data.hosts {
        let host_pattern = format!("Host {}", host.host);
        if !existing.contains(&host_pattern) {
            let entry = format!(
                "\n# Added by connecto\nHost {}\n    HostName {}\n    User {}\n    IdentityFile {}\n",
                host.host, host.hostname, host.user, host.identity_file
            );
            existing.push_str(&entry);
            added += 1;
        }
    }

    if added > 0 {
        fs::write(&config_path, &existing)?;
        println!("{} Imported {} host(s) to SSH config.", "✓".green(), added);
    } else {
        println!("{} All hosts already exist in SSH config.", "→".yellow());
    }

    // Import subnets
    let mut cfg = config::Config::load().unwrap_or_default();
    let mut subnet_added = 0;
    for subnet in &data.subnets {
        if cfg.add_subnet(subnet) {
            subnet_added += 1;
        }
    }

    if subnet_added > 0 {
        cfg.save()?;
        println!(
            "{} Imported {} subnet(s) to config.",
            "✓".green(),
            subnet_added
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn test_cli_parsing() {
        Cli::command().debug_assert();
    }

    #[test]
    fn test_listen_defaults() {
        let cli = Cli::try_parse_from(["connecto", "listen"]).unwrap();
        match cli.command {
            Commands::Listen {
                port,
                name,
                verify,
                continuous,
            } => {
                assert_eq!(port, connecto_core::DEFAULT_PORT);
                assert!(name.is_none());
                assert!(!verify);
                assert!(!continuous);
            }
            _ => panic!("Expected Listen command"),
        }
    }

    #[test]
    fn test_scan_defaults() {
        let cli = Cli::try_parse_from(["connecto", "scan"]).unwrap();
        match cli.command {
            Commands::Scan { timeout, subnet } => {
                assert_eq!(timeout, 5);
                assert!(subnet.is_empty());
            }
            _ => panic!("Expected Scan command"),
        }
    }

    #[test]
    fn test_scan_with_subnet() {
        let cli = Cli::try_parse_from(["connecto", "scan", "--subnet", "10.0.0.0/24"]).unwrap();
        match cli.command {
            Commands::Scan { timeout, subnet } => {
                assert_eq!(timeout, 5);
                assert_eq!(subnet, vec!["10.0.0.0/24"]);
            }
            _ => panic!("Expected Scan command"),
        }
    }

    #[test]
    fn test_scan_with_multiple_subnets() {
        let cli = Cli::try_parse_from([
            "connecto",
            "scan",
            "--subnet",
            "10.0.0.0/24",
            "--subnet",
            "192.168.1.0/24",
        ])
        .unwrap();
        match cli.command {
            Commands::Scan { subnet, .. } => {
                assert_eq!(subnet.len(), 2);
                assert_eq!(subnet[0], "10.0.0.0/24");
                assert_eq!(subnet[1], "192.168.1.0/24");
            }
            _ => panic!("Expected Scan command"),
        }
    }

    #[test]
    fn test_pair_target() {
        let cli = Cli::try_parse_from(["connecto", "pair", "1"]).unwrap();
        match cli.command {
            Commands::Pair {
                target,
                comment,
                rsa,
                key,
            } => {
                assert_eq!(target, "1");
                assert!(comment.is_none());
                assert!(!rsa);
                assert!(key.is_none());
            }
            _ => panic!("Expected Pair command"),
        }
    }

    #[test]
    fn test_verbose_flag() {
        let cli = Cli::try_parse_from(["connecto", "-v", "scan"]).unwrap();
        assert!(cli.verbose);
    }

    #[test]
    fn test_sync_defaults() {
        let cli = Cli::try_parse_from(["connecto", "sync"]).unwrap();
        match cli.command {
            Commands::Sync {
                port,
                name,
                timeout,
                rsa,
                key,
            } => {
                assert_eq!(port, connecto_core::DEFAULT_PORT);
                assert!(name.is_none());
                assert_eq!(timeout, connecto_core::DEFAULT_SYNC_TIMEOUT_SECS);
                assert!(!rsa);
                assert!(key.is_none());
            }
            _ => panic!("Expected Sync command"),
        }
    }

    #[test]
    fn test_sync_with_options() {
        let cli = Cli::try_parse_from([
            "connecto",
            "sync",
            "--port",
            "9000",
            "--name",
            "MyDevice",
            "--timeout",
            "120",
            "--rsa",
        ])
        .unwrap();
        match cli.command {
            Commands::Sync {
                port,
                name,
                timeout,
                rsa,
                key,
            } => {
                assert_eq!(port, 9000);
                assert_eq!(name, Some("MyDevice".to_string()));
                assert_eq!(timeout, 120);
                assert!(rsa);
                assert!(key.is_none());
            }
            _ => panic!("Expected Sync command"),
        }
    }
}
