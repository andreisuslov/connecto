//! Connecto CLI - AirDrop-like SSH pairing tool
//!
//! Usage:
//!   connecto listen    - Start listening for pairing requests
//!   connecto scan      - Scan for available devices
//!   connecto pair <n>  - Pair with device number n

mod commands;

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

        /// Handle only one pairing request and exit
        #[arg(long)]
        once: bool,
    },

    /// Scan the local network for devices running Connecto
    Scan {
        /// How long to scan in seconds
        #[arg(short, long, default_value_t = 5)]
        timeout: u64,

        /// Skip mDNS and scan subnet directly (for corporate networks)
        #[arg(short, long)]
        fallback: bool,
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
        Commands::Listen { port, name, verify, once } => {
            commands::listen::run(port, name, verify, once).await
        }
        Commands::Scan { timeout, fallback } => {
            commands::scan::run_with_fallback(timeout, fallback).await
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
    }
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
            Commands::Listen { port, name, verify, once } => {
                assert_eq!(port, connecto_core::DEFAULT_PORT);
                assert!(name.is_none());
                assert!(!verify);
                assert!(!once);
            }
            _ => panic!("Expected Listen command"),
        }
    }

    #[test]
    fn test_scan_defaults() {
        let cli = Cli::try_parse_from(["connecto", "scan"]).unwrap();
        match cli.command {
            Commands::Scan { timeout } => {
                assert_eq!(timeout, 5);
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
