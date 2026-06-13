//! macOS implementation of `connecto ssh` (Remote Login via systemsetup)

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

    println!("{} Enabling Remote Login (SSH)...", "→".cyan());
    println!();

    // Enable Remote Login using systemsetup
    let output = Command::new("systemsetup")
        .args(["-setremotelogin", "on"])
        .output()
        .await?;

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
            println!(
                "  {} > {} > {}",
                "System Preferences".cyan(),
                "Sharing".cyan(),
                "Remote Login".cyan()
            );
            return Err(SilentExit.into());
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

    println!("{} Disabling Remote Login (SSH)...", "→".cyan());

    let output = Command::new("systemsetup")
        .args(["-setremotelogin", "off"])
        .output()
        .await?;

    // Only claim success when disabling actually succeeded.
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        println!("{} Failed to disable Remote Login.", "✗".red());
        if !stderr.is_empty() {
            println!("{}", stderr.dimmed());
        }
        return Err(SilentExit.into());
    }

    println!("{} Remote Login (SSH) disabled.", "✓".green());

    println!();
    println!("{}", "SSH Server is now disabled.".yellow());
    println!();

    Ok(())
}

pub(super) async fn status() -> Result<()> {
    // Check if sshd is running (doesn't require admin)
    let pgrep_output = Command::new("pgrep").args(["-x", "sshd"]).output().await;

    let sshd_running = pgrep_output.map(|o| o.status.success()).unwrap_or(false);

    if sshd_running {
        println!("{} SSH server is {}", "•".green(), "running".green().bold());
    } else {
        println!(
            "{} SSH server is {}",
            "•".yellow(),
            "not running".yellow().bold()
        );
        println!();
        println!("Enable with: {}", "sudo connecto ssh on".cyan());
        println!();
        println!(
            "Or enable in: {} > {} > {}",
            "System Settings".cyan(),
            "General".cyan(),
            "Sharing > Remote Login".cyan()
        );
        return Ok(());
    }

    // Check if port 22 is listening
    let lsof_output = Command::new("lsof")
        .args(["-i", ":22", "-P", "-n"])
        .output()
        .await;

    if let Ok(out) = lsof_output {
        if out.status.success() && !out.stdout.is_empty() {
            println!("{} Listening on port {}", "•".green(), "22".cyan());
        }
    }

    println!();
    Ok(())
}
