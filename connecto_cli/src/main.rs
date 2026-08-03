//! Connecto CLI - AirDrop-like SSH pairing tool
//!
//! Usage:
//!   connecto listen    - Start listening for pairing requests
//!   connecto scan      - Scan for available devices
//!   connecto pair <n>  - Pair with device number n

mod commands;

use anyhow::Result;
use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::{generate, Shell};
use connecto_core::{paths, Config, SshConfig};
use tracing_subscriber::EnvFilter;

/// Marker error for failures whose diagnostics were already printed
///
/// Failure sites print their rich colored diagnostics and then return this
/// error; `main` recognizes it, skips re-printing, and exits with status 1.
#[derive(Debug)]
pub struct SilentExit;

impl std::fmt::Display for SilentExit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("command failed")
    }
}

impl std::error::Error for SilentExit {}

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

        /// Create an ad-hoc WiFi network (bypasses router, for isolated networks)
        #[arg(long)]
        adhoc: bool,

        /// Enable Bluetooth Low Energy advertising (Linux only)
        #[arg(long)]
        bluetooth: bool,

        /// Run the listener in the background and return immediately.
        ///
        /// Use this when starting the listener over SSH: Windows terminates a
        /// session's processes when the connection closes, so an attached
        /// listener dies as soon as you disconnect and pairing then fails with
        /// "connection refused".
        #[arg(long)]
        detach: bool,
    },

    /// Scan the local network for devices running Connecto
    Scan {
        /// How long to scan in seconds
        #[arg(short, long, default_value_t = 5)]
        timeout: u64,

        /// Subnet to scan (e.g., 10.105.225.0/24). Can be specified multiple times.
        #[arg(short, long)]
        subnet: Vec<String>,

        /// Enable Bluetooth Low Energy scanning as a fallback
        #[arg(long)]
        bluetooth: bool,
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

    /// Manage SSH server (Windows: enable/disable OpenSSH Server)
    Ssh {
        #[command(subcommand)]
        action: SshAction,
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
enum SshAction {
    /// Enable SSH server (install and start OpenSSH Server on Windows)
    On,
    /// Disable SSH server (stop and disable OpenSSH Server on Windows)
    Off,
    /// Show SSH server status
    Status,
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
async fn main() {
    // Enable ANSI color support on Windows; disable colors entirely when the
    // console does not support virtual terminal processing.
    #[cfg(windows)]
    {
        if !enable_vt() {
            colored::control::set_override(false);
        }
    }

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

    if let Err(e) = run(cli).await {
        // SilentExit means the failure site already printed its diagnostics.
        if e.downcast_ref::<SilentExit>().is_none() {
            eprintln!("Error: {e:#}");
        }
        std::process::exit(1);
    }
}

async fn run(cli: Cli) -> Result<()> {
    match cli.command {
        Commands::Listen {
            port,
            name,
            verify,
            continuous,
            adhoc,
            bluetooth,
            detach,
        } => {
            if detach {
                let exe = std::env::current_exe()
                    .map_err(|e| anyhow::anyhow!("could not locate the connecto binary: {e}"))?;
                let args = commands::detach::listen_argv(
                    port, &name, verify, continuous, adhoc, bluetooth,
                );
                let pid = commands::detach::spawn_detached(exe, &args)?;
                commands::success(&format!("Listener running in the background (pid {pid})"));
                commands::info(&format!(
                    "Pair to this machine on port {port}, then the listener exits on its own."
                ));
                return Ok(());
            }
            commands::listen::run_with_adhoc(port, name, verify, continuous, adhoc, bluetooth).await
        }
        Commands::Scan {
            timeout,
            subnet,
            bluetooth,
        } => commands::scan::run_with_options(timeout, false, subnet, bluetooth).await,
        Commands::Pair {
            target,
            comment,
            rsa,
            key,
        } => commands::pair::run(target, comment, rsa, key).await,
        Commands::Keys { action } => commands::keys::run(action).await,
        Commands::Keygen { name, comment, rsa } => commands::keygen::run(name, comment, rsa).await,
        Commands::Config { action } => run_config(action),
        Commands::Hosts => commands::hosts::run_hosts(&SshConfig::at_default()?),
        Commands::Unpair { host } => commands::hosts::run_unpair(&SshConfig::at_default()?, &host),
        Commands::Test { host } => commands::hosts::run_test(&host),
        Commands::UpdateIp { host, ip } => {
            commands::hosts::run_update_ip(&SshConfig::at_default()?, &host, &ip)
        }
        Commands::Export { output } => {
            commands::hosts::run_export(&SshConfig::at_default()?, output.as_deref())
        }
        Commands::Import { file } => commands::hosts::run_import(&SshConfig::at_default()?, &file),
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
        Commands::Ssh { action } => match action {
            SshAction::On => commands::ssh::enable().await,
            SshAction::Off => commands::ssh::disable().await,
            SshAction::Status => commands::ssh::status().await,
        },
    }
}

fn run_config(action: ConfigAction) -> Result<()> {
    use colored::Colorize;

    match action {
        ConfigAction::AddSubnet { subnet } => {
            let mut cfg = Config::load()?;
            if cfg.add_subnet(&subnet) {
                cfg.save()?;
                println!("{} Added subnet: {}", "✓".green(), subnet.cyan());
            } else {
                println!("{} Subnet already exists: {}", "→".yellow(), subnet);
            }
        }
        ConfigAction::RemoveSubnet { subnet } => {
            let mut cfg = Config::load()?;
            if cfg.remove_subnet(&subnet) {
                cfg.save()?;
                println!("{} Removed subnet: {}", "✓".green(), subnet);
            } else {
                println!("{} Subnet not found: {}", "✗".red(), subnet);
                return Err(SilentExit.into());
            }
        }
        ConfigAction::SetDefaultKey { key_path } => {
            let expanded = paths::expand_tilde(&key_path)?;

            // Verify the key exists
            if !expanded.exists() {
                println!("{} Key file not found: {}", "✗".red(), expanded.display());
                return Err(SilentExit.into());
            }

            // Verify it's a valid SSH key (check for public key too)
            let pub_key_path = format!("{}.pub", expanded.display());
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
                return Err(SilentExit.into());
            }

            let mut cfg = Config::load()?;
            cfg.set_default_key(&expanded.display().to_string());
            cfg.save()?;
            println!(
                "{} Default key set: {}",
                "✓".green(),
                expanded.display().to_string().cyan()
            );
            println!("  {} All future pairings will use this key.", "→".dimmed());
        }
        ConfigAction::ClearDefaultKey => {
            let mut cfg = Config::load()?;
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
            let cfg = Config::load()?;
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
            let path = Config::path()?;
            println!("{}", path.display());
        }
    }
    Ok(())
}

/// Enable ANSI escape-code (virtual terminal) processing on the Windows console
///
/// Returns `false` when the console mode could not be queried or updated
/// (e.g. pre-VT Windows, or stdout is not a console), so the caller can
/// disable colors instead of emitting garbage escape sequences.
#[cfg(windows)]
fn enable_vt() -> bool {
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::System::Console::{
        GetConsoleMode, GetStdHandle, SetConsoleMode, CONSOLE_MODE,
        ENABLE_VIRTUAL_TERMINAL_PROCESSING, STD_OUTPUT_HANDLE,
    };

    // SAFETY: Win32 console calls on the process's own stdout handle; the
    // mode pointer is a local that outlives the call.
    unsafe {
        let handle = GetStdHandle(STD_OUTPUT_HANDLE);
        if handle == INVALID_HANDLE_VALUE || handle.is_null() {
            return false;
        }

        let mut mode: CONSOLE_MODE = 0;
        if GetConsoleMode(handle, &mut mode) == 0 {
            return false;
        }
        if mode & ENABLE_VIRTUAL_TERMINAL_PROCESSING != 0 {
            return true;
        }
        SetConsoleMode(handle, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING) != 0
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
            Commands::Listen {
                port,
                name,
                verify,
                continuous,
                adhoc,
                bluetooth,
                detach,
            } => {
                assert_eq!(port, connecto_core::DEFAULT_PORT);
                assert!(name.is_none());
                assert!(!verify);
                assert!(!continuous);
                assert!(!adhoc);
                assert!(!bluetooth);
                assert!(!detach);
            }
            _ => panic!("Expected Listen command"),
        }
    }

    #[test]
    fn test_listen_detach_flag() {
        let cli = Cli::try_parse_from(["connecto", "listen", "--detach"]).unwrap();
        match cli.command {
            Commands::Listen { detach, .. } => assert!(detach),
            _ => panic!("Expected Listen command"),
        }
    }

    #[test]
    fn test_scan_defaults() {
        let cli = Cli::try_parse_from(["connecto", "scan"]).unwrap();
        match cli.command {
            Commands::Scan {
                timeout,
                subnet,
                bluetooth,
            } => {
                assert_eq!(timeout, 5);
                assert!(subnet.is_empty());
                assert!(!bluetooth);
            }
            _ => panic!("Expected Scan command"),
        }
    }

    #[test]
    fn test_scan_with_subnet() {
        let cli = Cli::try_parse_from(["connecto", "scan", "--subnet", "10.0.0.0/24"]).unwrap();
        match cli.command {
            Commands::Scan {
                timeout,
                subnet,
                bluetooth,
            } => {
                assert_eq!(timeout, 5);
                assert_eq!(subnet, vec!["10.0.0.0/24"]);
                assert!(!bluetooth);
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
            Commands::Scan {
                subnet, bluetooth, ..
            } => {
                assert_eq!(subnet.len(), 2);
                assert_eq!(subnet[0], "10.0.0.0/24");
                assert_eq!(subnet[1], "192.168.1.0/24");
                assert!(!bluetooth);
            }
            _ => panic!("Expected Scan command"),
        }
    }

    #[test]
    fn test_scan_with_bluetooth() {
        let cli = Cli::try_parse_from(["connecto", "scan", "--bluetooth"]).unwrap();
        match cli.command {
            Commands::Scan { bluetooth, .. } => {
                assert!(bluetooth);
            }
            _ => panic!("Expected Scan command"),
        }
    }

    #[test]
    fn test_listen_with_bluetooth() {
        let cli = Cli::try_parse_from(["connecto", "listen", "--bluetooth"]).unwrap();
        match cli.command {
            Commands::Listen { bluetooth, .. } => {
                assert!(bluetooth);
            }
            _ => panic!("Expected Listen command"),
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

    #[test]
    fn test_silent_exit_is_detectable_via_downcast() {
        let err: anyhow::Error = SilentExit.into();
        assert!(err.downcast_ref::<SilentExit>().is_some());
    }
}
