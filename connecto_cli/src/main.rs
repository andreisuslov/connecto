//! Connecto CLI - AirDrop-like SSH pairing tool
//!
//! Usage:
//!   connecto listen    - Start listening for pairing requests
//!   connecto scan      - Scan for available devices
//!   connecto pair <n>  - Pair with device number n

mod commands;
mod config;

use anyhow::Result;
use clap::{Parser, Subcommand};
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
        Commands::Listen { port, name, verify, continuous } => {
            commands::listen::run(port, name, verify, continuous).await
        }
        Commands::Scan { timeout, subnet } => {
            commands::scan::run_with_options(timeout, false, subnet).await
        }
        Commands::Pair { target, comment, rsa } => {
            commands::pair::run(target, comment, rsa).await
        }
        Commands::Keys { action } => {
            commands::keys::run(action).await
        }
        Commands::Keygen { name, comment, rsa } => {
            commands::keygen::run(name, comment, rsa).await
        }
        Commands::Config { action } => {
            run_config(action)
        }
        Commands::Hosts => {
            run_hosts()
        }
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
                if let (Some(h), Some(hn), Some(u)) = (current_host.take(), current_hostname.take(), current_user.take()) {
                    connecto_hosts.push((h, hn, u));
                }
                current_host = Some(trimmed.strip_prefix("Host ").unwrap().to_string());
            } else if trimmed.starts_with("HostName ") {
                current_hostname = Some(trimmed.strip_prefix("HostName ").unwrap().to_string());
            } else if trimmed.starts_with("User ") {
                current_user = Some(trimmed.strip_prefix("User ").unwrap().to_string());
            } else if trimmed.starts_with("IdentityFile ") {
                // End of this host block
                if let (Some(h), Some(hn), Some(u)) = (current_host.take(), current_hostname.take(), current_user.take()) {
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
        println!("Pair with a device using: {}", "connecto scan && connecto pair 0".cyan());
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
        ConfigAction::List => {
            let cfg = config::Config::load()?;
            if cfg.subnets.is_empty() {
                println!("{}", "No subnets configured.".dimmed());
                println!();
                println!("Add subnets to scan with: {}", "connecto config add-subnet <cidr>".cyan());
            } else {
                println!("{}", "Configured subnets:".bold());
                for subnet in &cfg.subnets {
                    println!("  {} {}", "•".cyan(), subnet);
                }
            }
        }
        ConfigAction::Path => {
            let path = config::Config::path()?;
            println!("{}", path.display());
        }
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
            Commands::Listen { port, name, verify, continuous } => {
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
            "connecto", "scan",
            "--subnet", "10.0.0.0/24",
            "--subnet", "192.168.1.0/24"
        ]).unwrap();
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
            Commands::Pair { target, comment, rsa } => {
                assert_eq!(target, "1");
                assert!(comment.is_none());
                assert!(!rsa);
            }
            _ => panic!("Expected Pair command"),
        }
    }

    #[test]
    fn test_verbose_flag() {
        let cli = Cli::try_parse_from(["connecto", "-v", "scan"]).unwrap();
        assert!(cli.verbose);
    }
}
