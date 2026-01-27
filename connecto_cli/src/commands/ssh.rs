//! SSH server management command
//!
//! Enables/disables SSH server on Windows, macOS, and Linux

use anyhow::Result;
use colored::Colorize;
use std::process::Command;

/// Check if running as root/administrator
fn is_elevated() -> bool {
    #[cfg(target_os = "windows")]
    {
        let output = Command::new("powershell")
            .args(["-Command", "([Security.Principal.WindowsPrincipal] [Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)"])
            .output();

        match output {
            Ok(out) => String::from_utf8_lossy(&out.stdout).trim() == "True",
            Err(_) => false,
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        // Check if running as root (uid 0)
        unsafe { libc::geteuid() == 0 }
    }
}

/// Get the current platform
fn get_platform() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    }
}

/// Enable SSH server
pub async fn enable() -> Result<()> {
    println!();
    println!("{}", "  CONNECTO SSH SETUP  ".on_bright_blue().white().bold());
    println!();

    match get_platform() {
        "windows" => enable_windows().await,
        "macos" => enable_macos().await,
        "linux" => enable_linux().await,
        _ => {
            println!("{} Unsupported platform.", "✗".red());
            Ok(())
        }
    }
}

/// Disable SSH server
pub async fn disable() -> Result<()> {
    println!();
    println!("{}", "  CONNECTO SSH  ".on_bright_blue().white().bold());
    println!();

    match get_platform() {
        "windows" => disable_windows().await,
        "macos" => disable_macos().await,
        "linux" => disable_linux().await,
        _ => {
            println!("{} Unsupported platform.", "✗".red());
            Ok(())
        }
    }
}

/// Show SSH server status
pub async fn status() -> Result<()> {
    println!();
    println!("{}", "  SSH STATUS  ".on_bright_blue().white().bold());
    println!();

    match get_platform() {
        "windows" => status_windows().await,
        "macos" => status_macos().await,
        "linux" => status_linux().await,
        _ => {
            println!("{} Unsupported platform.", "✗".red());
            Ok(())
        }
    }
}

// ============================================================================
// Windows Implementation
// ============================================================================

async fn enable_windows() -> Result<()> {
    if !is_elevated() {
        println!("{} This command requires Administrator privileges.", "✗".red());
        println!();
        println!("Please run PowerShell as Administrator and try again:");
        println!("  {}", "connecto ssh on".cyan());
        return Ok(());
    }

    println!("{} Enabling OpenSSH Server...", "→".cyan());
    println!();

    // Check if OpenSSH Server is installed
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

    // Start the sshd service
    println!("{} Starting SSH service...", "→".cyan());

    let start_output = Command::new("powershell")
        .args(["-Command", "Start-Service sshd"])
        .output()?;

    if !start_output.status.success() {
        let stderr = String::from_utf8_lossy(&start_output.stderr);
        if !stderr.contains("already") {
            println!("{} Failed to start SSH service.", "✗".red());
            if !stderr.is_empty() {
                println!("{}", stderr.dimmed());
            }
            return Ok(());
        }
    }

    println!("{} SSH service started.", "✓".green());

    // Set to automatic startup
    println!("{} Configuring automatic startup...", "→".cyan());

    let auto_output = Command::new("powershell")
        .args(["-Command", "Set-Service -Name sshd -StartupType 'Automatic'"])
        .output()?;

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
            "#
        ])
        .output()?;

    if firewall_output.status.success() {
        println!("{} Firewall configured for SSH (port 22).", "✓".green());
    } else {
        println!("{} Warning: Could not configure firewall.", "⚠".yellow());
    }

    print_success_message();
    Ok(())
}

async fn disable_windows() -> Result<()> {
    if !is_elevated() {
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

async fn status_windows() -> Result<()> {
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

// ============================================================================
// macOS Implementation
// ============================================================================

async fn enable_macos() -> Result<()> {
    if !is_elevated() {
        println!("{} This command requires root privileges.", "✗".red());
        println!();
        println!("Please run with sudo:");
        println!("  {}", "sudo connecto ssh on".cyan());
        return Ok(());
    }

    println!("{} Enabling Remote Login (SSH)...", "→".cyan());
    println!();

    // Enable Remote Login using systemsetup
    let output = Command::new("systemsetup")
        .args(["-setremotelogin", "on"])
        .output()?;

    if output.status.success() {
        println!("{} Remote Login (SSH) enabled.", "✓".green());
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // Check if it's already enabled
        if stderr.contains("already") || stderr.contains("Remote Login") {
            println!("{} Remote Login (SSH) is already enabled.", "✓".green());
        } else {
            println!("{} Failed to enable Remote Login.", "✗".red());
            if !stderr.is_empty() {
                println!("{}", stderr.dimmed());
            }
            println!();
            println!("You can also enable SSH in:");
            println!("  {} > {} > {}",
                "System Preferences".cyan(),
                "Sharing".cyan(),
                "Remote Login".cyan()
            );
            return Ok(());
        }
    }

    print_success_message();
    Ok(())
}

async fn disable_macos() -> Result<()> {
    if !is_elevated() {
        println!("{} This command requires root privileges.", "✗".red());
        println!();
        println!("Please run with sudo:");
        println!("  {}", "sudo connecto ssh off".cyan());
        return Ok(());
    }

    println!("{} Disabling Remote Login (SSH)...", "→".cyan());

    let output = Command::new("systemsetup")
        .args(["-setremotelogin", "off"])
        .output()?;

    if output.status.success() {
        println!("{} Remote Login (SSH) disabled.", "✓".green());
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        println!("{} Failed to disable Remote Login.", "✗".red());
        if !stderr.is_empty() {
            println!("{}", stderr.dimmed());
        }
    }

    println!();
    println!("{}", "SSH Server is now disabled.".yellow());
    println!();

    Ok(())
}

async fn status_macos() -> Result<()> {
    // Check if sshd is running (doesn't require admin)
    let pgrep_output = Command::new("pgrep")
        .args(["-x", "sshd"])
        .output();

    let sshd_running = pgrep_output.map(|o| o.status.success()).unwrap_or(false);

    if sshd_running {
        println!("{} SSH server is {}", "•".green(), "running".green().bold());
    } else {
        println!("{} SSH server is {}", "•".yellow(), "not running".yellow().bold());
        println!();
        println!("Enable with: {}", "sudo connecto ssh on".cyan());
        println!();
        println!("Or enable in: {} > {} > {}",
            "System Settings".cyan(),
            "General".cyan(),
            "Sharing > Remote Login".cyan()
        );
        return Ok(());
    }

    // Check if port 22 is listening
    let lsof_output = Command::new("lsof")
        .args(["-i", ":22", "-P", "-n"])
        .output();

    if let Ok(out) = lsof_output {
        if out.status.success() && !out.stdout.is_empty() {
            println!("{} Listening on port {}", "•".green(), "22".cyan());
        }
    }

    println!();
    Ok(())
}

// ============================================================================
// Linux Implementation
// ============================================================================

async fn enable_linux() -> Result<()> {
    if !is_elevated() {
        println!("{} This command requires root privileges.", "✗".red());
        println!();
        println!("Please run with sudo:");
        println!("  {}", "sudo connecto ssh on".cyan());
        return Ok(());
    }

    println!("{} Enabling SSH server...", "→".cyan());
    println!();

    // Check if openssh-server is installed
    let which_output = Command::new("which")
        .arg("sshd")
        .output();

    let sshd_installed = which_output.map(|o| o.status.success()).unwrap_or(false);

    if !sshd_installed {
        println!("{} OpenSSH server not found.", "✗".red());
        println!();
        println!("Install it with your package manager:");
        println!("  {} (Debian/Ubuntu)", "sudo apt install openssh-server".cyan());
        println!("  {} (Fedora/RHEL)", "sudo dnf install openssh-server".cyan());
        println!("  {} (Arch)", "sudo pacman -S openssh".cyan());
        return Ok(());
    }

    // Try systemctl first (most modern distros)
    let systemctl_exists = Command::new("which")
        .arg("systemctl")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if systemctl_exists {
        // Start sshd
        println!("{} Starting SSH service...", "→".cyan());
        let start_output = Command::new("systemctl")
            .args(["start", "sshd"])
            .output();

        // Try 'ssh' service name if 'sshd' fails (Ubuntu uses 'ssh')
        let start_ok = match start_output {
            Ok(o) if o.status.success() => true,
            _ => {
                Command::new("systemctl")
                    .args(["start", "ssh"])
                    .output()
                    .map(|o| o.status.success())
                    .unwrap_or(false)
            }
        };

        if start_ok {
            println!("{} SSH service started.", "✓".green());
        } else {
            println!("{} Failed to start SSH service.", "✗".red());
            return Ok(());
        }

        // Enable on boot
        println!("{} Configuring automatic startup...", "→".cyan());
        let enable_output = Command::new("systemctl")
            .args(["enable", "sshd"])
            .output();

        let enable_ok = match enable_output {
            Ok(o) if o.status.success() => true,
            _ => {
                Command::new("systemctl")
                    .args(["enable", "ssh"])
                    .output()
                    .map(|o| o.status.success())
                    .unwrap_or(false)
            }
        };

        if enable_ok {
            println!("{} SSH will start automatically on boot.", "✓".green());
        } else {
            println!("{} Warning: Could not enable automatic startup.", "⚠".yellow());
        }
    } else {
        // Fallback for non-systemd systems
        println!("{} Starting SSH service...", "→".cyan());
        let output = Command::new("service")
            .args(["sshd", "start"])
            .output();

        match output {
            Ok(o) if o.status.success() => {
                println!("{} SSH service started.", "✓".green());
            }
            _ => {
                // Try 'ssh' service name
                let output2 = Command::new("service")
                    .args(["ssh", "start"])
                    .output();

                if output2.map(|o| o.status.success()).unwrap_or(false) {
                    println!("{} SSH service started.", "✓".green());
                } else {
                    println!("{} Failed to start SSH service.", "✗".red());
                    return Ok(());
                }
            }
        }
    }

    print_success_message();
    Ok(())
}

async fn disable_linux() -> Result<()> {
    if !is_elevated() {
        println!("{} This command requires root privileges.", "✗".red());
        println!();
        println!("Please run with sudo:");
        println!("  {}", "sudo connecto ssh off".cyan());
        return Ok(());
    }

    println!("{} Disabling SSH server...", "→".cyan());

    let systemctl_exists = Command::new("which")
        .arg("systemctl")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if systemctl_exists {
        // Stop sshd
        let _ = Command::new("systemctl")
            .args(["stop", "sshd"])
            .output();
        let _ = Command::new("systemctl")
            .args(["stop", "ssh"])
            .output();

        println!("{} SSH service stopped.", "✓".green());

        // Disable on boot
        let _ = Command::new("systemctl")
            .args(["disable", "sshd"])
            .output();
        let _ = Command::new("systemctl")
            .args(["disable", "ssh"])
            .output();

        println!("{} SSH automatic startup disabled.", "✓".green());
    } else {
        let _ = Command::new("service")
            .args(["sshd", "stop"])
            .output();
        let _ = Command::new("service")
            .args(["ssh", "stop"])
            .output();

        println!("{} SSH service stopped.", "✓".green());
    }

    println!();
    println!("{}", "SSH Server is now disabled.".yellow());
    println!();

    Ok(())
}

async fn status_linux() -> Result<()> {
    let systemctl_exists = Command::new("which")
        .arg("systemctl")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);

    if systemctl_exists {
        // Check if sshd service is active
        let status_output = Command::new("systemctl")
            .args(["is-active", "sshd"])
            .output();

        let is_active = match status_output {
            Ok(o) => String::from_utf8_lossy(&o.stdout).trim() == "active",
            Err(_) => {
                // Try 'ssh' service name
                Command::new("systemctl")
                    .args(["is-active", "ssh"])
                    .output()
                    .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "active")
                    .unwrap_or(false)
            }
        };

        if is_active {
            println!("{} SSH server is {}", "•".green(), "running".green().bold());
        } else {
            println!("{} SSH server is {}", "•".yellow(), "stopped".yellow().bold());
        }

        // Check if enabled on boot
        let enabled_output = Command::new("systemctl")
            .args(["is-enabled", "sshd"])
            .output();

        let is_enabled = match enabled_output {
            Ok(o) => String::from_utf8_lossy(&o.stdout).trim() == "enabled",
            Err(_) => {
                Command::new("systemctl")
                    .args(["is-enabled", "ssh"])
                    .output()
                    .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "enabled")
                    .unwrap_or(false)
            }
        };

        if is_enabled {
            println!("{} Starts automatically on boot", "•".green());
        } else {
            println!("{} Automatic startup is {}", "•".yellow(), "disabled".yellow());
        }
    } else {
        // Fallback: check if sshd process is running
        let pgrep_output = Command::new("pgrep")
            .args(["-x", "sshd"])
            .output();

        match pgrep_output {
            Ok(out) if out.status.success() => {
                println!("{} SSH server is {}", "•".green(), "running".green().bold());
            }
            _ => {
                println!("{} SSH server is {}", "•".yellow(), "not running".yellow().bold());
            }
        }
    }

    // Check if port 22 is listening
    let ss_output = Command::new("ss")
        .args(["-tlnp"])
        .output();

    if let Ok(out) = ss_output {
        let output = String::from_utf8_lossy(&out.stdout);
        if output.contains(":22 ") || output.contains(":22\t") {
            println!("{} Listening on port {}", "•".green(), "22".cyan());
        }
    }

    println!();
    Ok(())
}

// ============================================================================
// Helpers
// ============================================================================

fn print_success_message() {
    println!();
    println!("{}", "SSH Server is now enabled!".green().bold());
    println!();
    println!("Other devices can now SSH into this machine after pairing.");
    println!();
}
