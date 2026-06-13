//! Linux implementation of `connecto ssh` (systemd/service-managed sshd)
//!
//! Also used as the fallback for non-macOS, non-Windows unix-likes.

use crate::SilentExit;
use anyhow::Result;
use colored::Colorize;
use tokio::process::Command;

pub(super) async fn enable() -> Result<()> {
    if !super::is_elevated() {
        println!("{} This command requires root privileges.", "✗".red());
        println!();
        println!("Please run with sudo:");
        println!("  {}", "sudo connecto ssh on".cyan());
        return Err(SilentExit.into());
    }

    println!("{} Enabling SSH server...", "→".cyan());
    println!();

    // Check if openssh-server is installed
    let which_output = Command::new("which").arg("sshd").output().await;

    let sshd_installed = which_output.map(|o| o.status.success()).unwrap_or(false);

    if !sshd_installed {
        println!("{} OpenSSH server not found.", "✗".red());
        println!();
        println!("Install it with your package manager:");
        println!(
            "  {} (Debian/Ubuntu)",
            "sudo apt install openssh-server".cyan()
        );
        println!(
            "  {} (Fedora/RHEL)",
            "sudo dnf install openssh-server".cyan()
        );
        println!("  {} (Arch)", "sudo pacman -S openssh".cyan());
        return Err(SilentExit.into());
    }

    // Try systemctl first (most modern distros)
    let systemctl_exists = Command::new("which")
        .arg("systemctl")
        .output()
        .await
        .map(|o| o.status.success())
        .unwrap_or(false);

    if systemctl_exists {
        // Start sshd
        println!("{} Starting SSH service...", "→".cyan());
        let start_output = Command::new("systemctl")
            .args(["start", "sshd"])
            .output()
            .await;

        // Try 'ssh' service name if 'sshd' fails (Ubuntu uses 'ssh')
        let start_ok = match start_output {
            Ok(o) if o.status.success() => true,
            _ => Command::new("systemctl")
                .args(["start", "ssh"])
                .output()
                .await
                .map(|o| o.status.success())
                .unwrap_or(false),
        };

        if start_ok {
            println!("{} SSH service started.", "✓".green());
        } else {
            println!("{} Failed to start SSH service.", "✗".red());
            return Err(SilentExit.into());
        }

        // Enable on boot
        println!("{} Configuring automatic startup...", "→".cyan());
        let enable_output = Command::new("systemctl")
            .args(["enable", "sshd"])
            .output()
            .await;

        let enable_ok = match enable_output {
            Ok(o) if o.status.success() => true,
            _ => Command::new("systemctl")
                .args(["enable", "ssh"])
                .output()
                .await
                .map(|o| o.status.success())
                .unwrap_or(false),
        };

        if enable_ok {
            println!("{} SSH will start automatically on boot.", "✓".green());
        } else {
            println!(
                "{} Warning: Could not enable automatic startup.",
                "⚠".yellow()
            );
        }
    } else {
        // Fallback for non-systemd systems
        println!("{} Starting SSH service...", "→".cyan());
        let output = Command::new("service")
            .args(["sshd", "start"])
            .output()
            .await;

        match output {
            Ok(o) if o.status.success() => {
                println!("{} SSH service started.", "✓".green());
            }
            _ => {
                // Try 'ssh' service name
                let output2 = Command::new("service")
                    .args(["ssh", "start"])
                    .output()
                    .await;

                if output2.map(|o| o.status.success()).unwrap_or(false) {
                    println!("{} SSH service started.", "✓".green());
                } else {
                    println!("{} Failed to start SSH service.", "✗".red());
                    return Err(SilentExit.into());
                }
            }
        }
    }

    super::print_success_message();
    Ok(())
}

pub(super) async fn disable() -> Result<()> {
    if !super::is_elevated() {
        println!("{} This command requires root privileges.", "✗".red());
        println!();
        println!("Please run with sudo:");
        println!("  {}", "sudo connecto ssh off".cyan());
        return Err(SilentExit.into());
    }

    println!("{} Disabling SSH server...", "→".cyan());

    let systemctl_exists = Command::new("which")
        .arg("systemctl")
        .output()
        .await
        .map(|o| o.status.success())
        .unwrap_or(false);

    if systemctl_exists {
        // Stop sshd
        let _ = Command::new("systemctl")
            .args(["stop", "sshd"])
            .output()
            .await;
        let _ = Command::new("systemctl")
            .args(["stop", "ssh"])
            .output()
            .await;

        // Verify the service actually stopped before claiming success.
        let mut still_active = false;
        for service in ["sshd", "ssh"] {
            let active = Command::new("systemctl")
                .args(["is-active", service])
                .output()
                .await
                .map(|o| String::from_utf8_lossy(&o.stdout).trim() == "active")
                .unwrap_or(false);
            if active {
                still_active = true;
                break;
            }
        }

        if still_active {
            println!("{} Failed to stop SSH service (still active).", "✗".red());
            return Err(SilentExit.into());
        }

        println!("{} SSH service stopped.", "✓".green());

        // Disable on boot
        let _ = Command::new("systemctl")
            .args(["disable", "sshd"])
            .output()
            .await;
        let _ = Command::new("systemctl")
            .args(["disable", "ssh"])
            .output()
            .await;

        println!("{} SSH automatic startup disabled.", "✓".green());
    } else {
        let _ = Command::new("service")
            .args(["sshd", "stop"])
            .output()
            .await;
        let _ = Command::new("service").args(["ssh", "stop"]).output().await;

        println!("{} SSH service stopped.", "✓".green());
    }

    println!();
    println!("{}", "SSH Server is now disabled.".yellow());
    println!();

    Ok(())
}

pub(super) async fn status() -> Result<()> {
    let systemctl_exists = Command::new("which")
        .arg("systemctl")
        .output()
        .await
        .map(|o| o.status.success())
        .unwrap_or(false);

    if systemctl_exists {
        // Check if the sshd service is active; a non-success exit (unknown
        // or inactive unit) falls back to the 'ssh' unit name used on
        // Ubuntu/Debian
        let status_output = Command::new("systemctl")
            .args(["is-active", "sshd"])
            .output()
            .await;

        let is_active = match status_output {
            Ok(o)
                if o.status.success() && String::from_utf8_lossy(&o.stdout).trim() == "active" =>
            {
                true
            }
            _ => Command::new("systemctl")
                .args(["is-active", "ssh"])
                .output()
                .await
                .map(|o| {
                    o.status.success() && String::from_utf8_lossy(&o.stdout).trim() == "active"
                })
                .unwrap_or(false),
        };

        if is_active {
            println!("{} SSH server is {}", "•".green(), "running".green().bold());
        } else {
            println!(
                "{} SSH server is {}",
                "•".yellow(),
                "stopped".yellow().bold()
            );
        }

        // Check if enabled on boot; same 'ssh' unit-name fallback as above
        // (an 'sshd' alias also reports "alias", not "enabled")
        let enabled_output = Command::new("systemctl")
            .args(["is-enabled", "sshd"])
            .output()
            .await;

        let is_enabled = match enabled_output {
            Ok(o)
                if o.status.success() && String::from_utf8_lossy(&o.stdout).trim() == "enabled" =>
            {
                true
            }
            _ => Command::new("systemctl")
                .args(["is-enabled", "ssh"])
                .output()
                .await
                .map(|o| {
                    o.status.success() && String::from_utf8_lossy(&o.stdout).trim() == "enabled"
                })
                .unwrap_or(false),
        };

        if is_enabled {
            println!("{} Starts automatically on boot", "•".green());
        } else {
            println!(
                "{} Automatic startup is {}",
                "•".yellow(),
                "disabled".yellow()
            );
        }
    } else {
        // Fallback: check if sshd process is running
        let pgrep_output = Command::new("pgrep").args(["-x", "sshd"]).output().await;

        match pgrep_output {
            Ok(out) if out.status.success() => {
                println!("{} SSH server is {}", "•".green(), "running".green().bold());
            }
            _ => {
                println!(
                    "{} SSH server is {}",
                    "•".yellow(),
                    "not running".yellow().bold()
                );
            }
        }
    }

    // Check if port 22 is listening
    let ss_output = Command::new("ss").args(["-tlnp"]).output().await;

    if let Ok(out) = ss_output {
        let output = String::from_utf8_lossy(&out.stdout);
        if output.contains(":22 ") || output.contains(":22\t") {
            println!("{} Listening on port {}", "•".green(), "22".cyan());
        }
    }

    println!();
    Ok(())
}
