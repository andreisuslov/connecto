//! Windows implementation of `connecto ssh` (OpenSSH Server via PowerShell)

use crate::SilentExit;
use anyhow::Result;
use colored::Colorize;
use tokio::process::Command;

/// Check for Administrator privileges without blocking the async runtime
///
/// Delegates to the single workspace-wide elevation check in
/// `connecto_core::keys` (a cached PowerShell probe).
async fn is_elevated() -> bool {
    tokio::task::spawn_blocking(connecto_core::keys::is_windows_admin)
        .await
        .unwrap_or(false)
}

pub(super) async fn enable() -> Result<()> {
    if !is_elevated().await {
        println!(
            "{} This command requires Administrator privileges.",
            "✗".red()
        );
        println!();
        println!("Please run PowerShell as Administrator and try again:");
        println!("  {}", "connecto ssh on".cyan());
        return Err(SilentExit.into());
    }

    println!("{} Enabling OpenSSH Server...", "→".cyan());
    println!();

    // Check if OpenSSH Server is installed
    println!("{} Checking OpenSSH Server installation...", "→".cyan());

    // First check if sshd service already exists (works on all Windows versions)
    let service_check = Command::new("powershell")
        .args([
            "-Command",
            "Get-Service sshd -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Name",
        ])
        .output()
        .await?;

    let sshd_exists = !String::from_utf8_lossy(&service_check.stdout)
        .trim()
        .is_empty();

    if sshd_exists {
        println!("{} OpenSSH Server already installed.", "✓".green());
    } else {
        // Try Windows 10/Server 2016+ method first (Add-WindowsCapability)
        let capability_check = Command::new("powershell")
            .args([
                "-Command",
                "Get-Command Add-WindowsCapability -ErrorAction SilentlyContinue",
            ])
            .output()
            .await?;

        if capability_check.status.success()
            && !String::from_utf8_lossy(&capability_check.stdout)
                .trim()
                .is_empty()
        {
            // Modern Windows - use Add-WindowsCapability
            println!("{} Installing OpenSSH Server...", "→".cyan());

            let install_output = Command::new("powershell")
                .args([
                    "-Command",
                    "Add-WindowsCapability -Online -Name OpenSSH.Server~~~~0.0.1.0",
                ])
                .output()
                .await?;

            if !install_output.status.success() {
                let stderr = String::from_utf8_lossy(&install_output.stderr);
                println!("{} Failed to install OpenSSH Server.", "✗".red());
                if !stderr.is_empty() {
                    println!("{}", stderr.dimmed());
                }
                return Err(SilentExit.into());
            }

            println!("{} OpenSSH Server installed.", "✓".green());
        } else {
            // Older Windows (Server 2012 R2, etc.) - OpenSSH must be installed manually
            println!("{} OpenSSH Server is not installed.", "✗".red());
            println!();
            println!("Your Windows version requires manual OpenSSH installation:");
            println!();
            println!("  1. Download OpenSSH from:");
            println!(
                "     {}",
                "https://github.com/PowerShell/Win32-OpenSSH/releases".cyan()
            );
            println!();
            println!("  2. Extract to C:\\Program Files\\OpenSSH");
            println!();
            println!("  3. Run as Administrator:");
            println!("     {}", "powershell -ExecutionPolicy Bypass -File \"C:\\Program Files\\OpenSSH\\install-sshd.ps1\"".dimmed());
            println!();
            println!("  4. Then run {} again.", "connecto ssh on".cyan());
            return Err(SilentExit.into());
        }
    }

    // Start the sshd service
    println!("{} Starting SSH service...", "→".cyan());

    let start_output = Command::new("powershell")
        .args(["-Command", "Start-Service sshd"])
        .output()
        .await?;

    if !start_output.status.success() {
        let stderr = String::from_utf8_lossy(&start_output.stderr);
        if !stderr.contains("already") {
            println!("{} Failed to start SSH service.", "✗".red());
            if !stderr.is_empty() {
                println!("{}", stderr.dimmed());
            }
            return Err(SilentExit.into());
        }
    }

    println!("{} SSH service started.", "✓".green());

    // Set to automatic startup
    println!("{} Configuring automatic startup...", "→".cyan());

    let auto_output = Command::new("powershell")
        .args([
            "-Command",
            "Set-Service -Name sshd -StartupType 'Automatic'",
        ])
        .output()
        .await?;

    if !auto_output.status.success() {
        println!("{} Warning: Could not set automatic startup.", "⚠".yellow());
    } else {
        println!("{} SSH will start automatically on boot.", "✓".green());
    }

    // Configure firewall rule
    println!("{} Configuring firewall...", "→".cyan());

    let firewall_output = Command::new("powershell")
        .args([
            "-Command",
            r#"
            $rule = Get-NetFirewallRule -Name 'OpenSSH-Server-In-TCP' -ErrorAction SilentlyContinue
            if (-not $rule) {
                New-NetFirewallRule -Name 'OpenSSH-Server-In-TCP' -DisplayName 'OpenSSH Server (sshd)' -Enabled True -Direction Inbound -Protocol TCP -Action Allow -LocalPort 22
                'created'
            } else {
                Enable-NetFirewallRule -Name 'OpenSSH-Server-In-TCP'
                'enabled'
            }
            "#,
        ])
        .output()
        .await?;

    if firewall_output.status.success() {
        println!("{} Firewall configured for SSH (port 22).", "✓".green());
    } else {
        println!("{} Warning: Could not configure firewall.", "⚠".yellow());
    }

    super::print_success_message();
    Ok(())
}

pub(super) async fn disable() -> Result<()> {
    if !is_elevated().await {
        println!(
            "{} This command requires Administrator privileges.",
            "✗".red()
        );
        println!();
        println!("Please run PowerShell as Administrator and try again:");
        println!("  {}", "connecto ssh off".cyan());
        return Err(SilentExit.into());
    }

    println!("{} Disabling OpenSSH Server...", "→".cyan());

    // Stop the service
    let stop_output = Command::new("powershell")
        .args([
            "-Command",
            "Stop-Service sshd -ErrorAction SilentlyContinue",
        ])
        .output()
        .await?;

    let stop_ok = stop_output.status.success();
    if stop_ok {
        println!("{} SSH service stopped.", "✓".green());
    }

    // Disable automatic startup
    let disable_output = Command::new("powershell")
        .args([
            "-Command",
            "Set-Service -Name sshd -StartupType 'Disabled' -ErrorAction SilentlyContinue",
        ])
        .output()
        .await?;

    let disable_ok = disable_output.status.success();
    if disable_ok {
        println!("{} SSH automatic startup disabled.", "✓".green());
    }

    // Only claim success when both steps actually succeeded.
    if !(stop_ok && disable_ok) {
        println!();
        println!("{} Failed to disable SSH server.", "✗".red());
        return Err(SilentExit.into());
    }

    println!();
    println!("{}", "SSH Server is now disabled.".yellow());
    println!();

    Ok(())
}

pub(super) async fn status() -> Result<()> {
    let status_output = Command::new("powershell")
        .args([
            "-Command",
            "Get-Service sshd -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Status",
        ])
        .output()
        .await?;

    let status = String::from_utf8_lossy(&status_output.stdout)
        .trim()
        .to_string();

    if status.is_empty() {
        println!(
            "{} OpenSSH Server is {}",
            "•".red(),
            "not installed".red().bold()
        );
        println!();
        println!("Install and enable with: {}", "connecto ssh on".cyan());
        return Ok(());
    }

    match status.as_str() {
        "Running" => {
            println!("{} SSH server is {}", "•".green(), "running".green().bold());
        }
        "Stopped" => {
            println!(
                "{} SSH server is {}",
                "•".yellow(),
                "stopped".yellow().bold()
            );
        }
        _ => {
            println!("{} SSH server status: {}", "•".dimmed(), status);
        }
    }

    // Check startup type
    let startup_output = Command::new("powershell")
        .args([
            "-Command",
            "Get-Service sshd | Select-Object -ExpandProperty StartType",
        ])
        .output()
        .await?;

    let startup = String::from_utf8_lossy(&startup_output.stdout)
        .trim()
        .to_string();

    match startup.as_str() {
        "Automatic" => {
            println!("{} Starts automatically on boot", "•".green());
        }
        "Disabled" => {
            println!(
                "{} Automatic startup is {}",
                "•".yellow(),
                "disabled".yellow()
            );
        }
        _ => {
            println!("{} Startup type: {}", "•".dimmed(), startup);
        }
    }

    // Check firewall
    let firewall_output = Command::new("powershell")
        .args(["-Command", "Get-NetFirewallRule -Name 'OpenSSH-Server-In-TCP' -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Enabled"])
        .output()
        .await?;

    let firewall = String::from_utf8_lossy(&firewall_output.stdout)
        .trim()
        .to_string();

    if firewall == "True" {
        println!("{} Firewall allows SSH (port 22)", "•".green());
    } else {
        println!("{} Firewall rule not configured", "•".yellow());
    }

    println!();
    Ok(())
}
