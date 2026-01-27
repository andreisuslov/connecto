//! SSH server management command (Windows-specific)
//!
//! Enables/disables OpenSSH Server on Windows machines

use anyhow::Result;
use colored::Colorize;
use std::process::Command;

/// Check if we're running on Windows
fn is_windows() -> bool {
    cfg!(target_os = "windows")
}

/// Check if running as administrator (Windows)
#[cfg(target_os = "windows")]
fn is_admin() -> bool {
    use std::process::Command;

    let output = Command::new("powershell")
        .args(["-Command", "([Security.Principal.WindowsPrincipal] [Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)"])
        .output();

    match output {
        Ok(out) => String::from_utf8_lossy(&out.stdout).trim() == "True",
        Err(_) => false,
    }
}

#[cfg(not(target_os = "windows"))]
fn is_admin() -> bool {
    false
}

/// Enable SSH server
pub async fn enable() -> Result<()> {
    println!();
    println!("{}", "  CONNECTO SSH SETUP  ".on_bright_blue().white().bold());
    println!();

    if !is_windows() {
        println!("{} This command is only available on Windows.", "✗".red());
        println!();
        println!("On macOS/Linux, SSH is typically pre-installed.");
        println!("Enable it via System Preferences (macOS) or install openssh-server (Linux).");
        return Ok(());
    }

    if !is_admin() {
        println!("{} This command requires Administrator privileges.", "✗".red());
        println!();
        println!("Please run PowerShell as Administrator and try again:");
        println!("  {}", "connecto ssh on".cyan());
        return Ok(());
    }

    println!("{} Enabling OpenSSH Server...", "→".cyan());
    println!();

    // Step 1: Check if OpenSSH Server is installed
    println!("{} Checking OpenSSH Server installation...", "→".cyan());

    let check_output = Command::new("powershell")
        .args([
            "-Command",
            "Get-WindowsCapability -Online | Where-Object Name -like 'OpenSSH.Server*' | Select-Object -ExpandProperty State"
        ])
        .output()?;

    let state = String::from_utf8_lossy(&check_output.stdout).trim().to_string();

    if state != "Installed" {
        println!("{} Installing OpenSSH Server...", "→".cyan());

        let install_output = Command::new("powershell")
            .args([
                "-Command",
                "Add-WindowsCapability -Online -Name OpenSSH.Server~~~~0.0.1.0"
            ])
            .output()?;

        if !install_output.status.success() {
            let stderr = String::from_utf8_lossy(&install_output.stderr);
            println!("{} Failed to install OpenSSH Server.", "✗".red());
            if !stderr.is_empty() {
                println!("{}", stderr.dimmed());
            }
            return Ok(());
        }

        println!("{} OpenSSH Server installed.", "✓".green());
    } else {
        println!("{} OpenSSH Server already installed.", "✓".green());
    }

    // Step 2: Start the sshd service
    println!("{} Starting SSH service...", "→".cyan());

    let start_output = Command::new("powershell")
        .args(["-Command", "Start-Service sshd"])
        .output()?;

    if !start_output.status.success() {
        let stderr = String::from_utf8_lossy(&start_output.stderr);
        // Check if it's already running
        if !stderr.contains("already") {
            println!("{} Failed to start SSH service.", "✗".red());
            if !stderr.is_empty() {
                println!("{}", stderr.dimmed());
            }
            return Ok(());
        }
    }

    println!("{} SSH service started.", "✓".green());

    // Step 3: Set to automatic startup
    println!("{} Configuring automatic startup...", "→".cyan());

    let auto_output = Command::new("powershell")
        .args(["-Command", "Set-Service -Name sshd -StartupType 'Automatic'"])
        .output()?;

    if !auto_output.status.success() {
        println!("{} Warning: Could not set automatic startup.", "⚠".yellow());
    } else {
        println!("{} SSH will start automatically on boot.", "✓".green());
    }

    // Step 4: Configure firewall rule
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
            "#
        ])
        .output()?;

    if firewall_output.status.success() {
        println!("{} Firewall configured for SSH (port 22).", "✓".green());
    } else {
        println!("{} Warning: Could not configure firewall.", "⚠".yellow());
    }

    println!();
    println!("{}", "SSH Server is now enabled!".green().bold());
    println!();
    println!("Other devices can now SSH into this machine after pairing.");
    println!();

    Ok(())
}

/// Disable SSH server
pub async fn disable() -> Result<()> {
    println!();
    println!("{}", "  CONNECTO SSH  ".on_bright_blue().white().bold());
    println!();

    if !is_windows() {
        println!("{} This command is only available on Windows.", "✗".red());
        return Ok(());
    }

    if !is_admin() {
        println!("{} This command requires Administrator privileges.", "✗".red());
        println!();
        println!("Please run PowerShell as Administrator and try again:");
        println!("  {}", "connecto ssh off".cyan());
        return Ok(());
    }

    println!("{} Disabling OpenSSH Server...", "→".cyan());

    // Stop the service
    let stop_output = Command::new("powershell")
        .args(["-Command", "Stop-Service sshd -ErrorAction SilentlyContinue"])
        .output()?;

    if stop_output.status.success() {
        println!("{} SSH service stopped.", "✓".green());
    }

    // Disable automatic startup
    let disable_output = Command::new("powershell")
        .args(["-Command", "Set-Service -Name sshd -StartupType 'Disabled' -ErrorAction SilentlyContinue"])
        .output()?;

    if disable_output.status.success() {
        println!("{} SSH automatic startup disabled.", "✓".green());
    }

    println!();
    println!("{}", "SSH Server is now disabled.".yellow());
    println!();

    Ok(())
}

/// Show SSH server status
pub async fn status() -> Result<()> {
    println!();
    println!("{}", "  SSH STATUS  ".on_bright_blue().white().bold());
    println!();

    if !is_windows() {
        // On Unix, check if sshd is running
        let output = Command::new("pgrep")
            .args(["-x", "sshd"])
            .output();

        match output {
            Ok(out) if out.status.success() => {
                println!("{} SSH server is {}", "•".green(), "running".green().bold());
            }
            _ => {
                println!("{} SSH server is {}", "•".red(), "not running".red().bold());
            }
        }

        // Check if port 22 is listening
        let listen_output = Command::new("lsof")
            .args(["-i", ":22", "-P", "-n"])
            .output();

        if let Ok(out) = listen_output {
            if out.status.success() && !out.stdout.is_empty() {
                println!("{} Listening on port {}", "•".green(), "22".cyan());
            }
        }

        return Ok(());
    }

    // Windows status check
    let status_output = Command::new("powershell")
        .args(["-Command", "Get-Service sshd -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Status"])
        .output()?;

    let status = String::from_utf8_lossy(&status_output.stdout).trim().to_string();

    if status.is_empty() {
        println!("{} OpenSSH Server is {}", "•".red(), "not installed".red().bold());
        println!();
        println!("Install and enable with: {}", "connecto ssh on".cyan());
        return Ok(());
    }

    match status.as_str() {
        "Running" => {
            println!("{} SSH server is {}", "•".green(), "running".green().bold());
        }
        "Stopped" => {
            println!("{} SSH server is {}", "•".yellow(), "stopped".yellow().bold());
        }
        _ => {
            println!("{} SSH server status: {}", "•".dimmed(), status);
        }
    }

    // Check startup type
    let startup_output = Command::new("powershell")
        .args(["-Command", "Get-Service sshd | Select-Object -ExpandProperty StartType"])
        .output()?;

    let startup = String::from_utf8_lossy(&startup_output.stdout).trim().to_string();

    match startup.as_str() {
        "Automatic" => {
            println!("{} Starts automatically on boot", "•".green());
        }
        "Disabled" => {
            println!("{} Automatic startup is {}", "•".yellow(), "disabled".yellow());
        }
        _ => {
            println!("{} Startup type: {}", "•".dimmed(), startup);
        }
    }

    // Check firewall
    let firewall_output = Command::new("powershell")
        .args(["-Command", "Get-NetFirewallRule -Name 'OpenSSH-Server-In-TCP' -ErrorAction SilentlyContinue | Select-Object -ExpandProperty Enabled"])
        .output()?;

    let firewall = String::from_utf8_lossy(&firewall_output.stdout).trim().to_string();

    if firewall == "True" {
        println!("{} Firewall allows SSH (port 22)", "•".green());
    } else {
        println!("{} Firewall rule not configured", "•".yellow());
    }

    println!();

    Ok(())
}
