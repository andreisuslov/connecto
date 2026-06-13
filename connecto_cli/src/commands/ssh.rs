//! SSH server management command
//!
//! Enables/disables the SSH server on Windows, macOS, and Linux. Each OS
//! implementation lives in a compile-time-gated submodule, so only the
//! current platform's commands (and PowerShell payloads) are compiled into
//! the binary. All external commands run through `tokio::process` so the
//! async runtime is never blocked.

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos as platform;

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
use windows as platform;

// Linux is also the fallback implementation for other unix-likes,
// matching the previous runtime dispatch behavior.
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod linux;
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
use linux as platform;

use anyhow::Result;
use colored::Colorize;

/// Check if running as root (Unix)
#[cfg(unix)]
fn is_elevated() -> bool {
    // Check if running as root (uid 0)
    unsafe { libc::geteuid() == 0 }
}

/// Enable SSH server
pub async fn enable() -> Result<()> {
    println!();
    println!(
        "{}",
        "  CONNECTO SSH SETUP  ".on_bright_blue().white().bold()
    );
    println!();

    platform::enable().await
}

/// Disable SSH server
pub async fn disable() -> Result<()> {
    println!();
    println!("{}", "  CONNECTO SSH  ".on_bright_blue().white().bold());
    println!();

    platform::disable().await
}

/// Show SSH server status
pub async fn status() -> Result<()> {
    println!();
    println!("{}", "  SSH STATUS  ".on_bright_blue().white().bold());
    println!();

    platform::status().await
}

fn print_success_message() {
    println!();
    println!("{}", "SSH Server is now enabled!".green().bold());
    println!();
    println!("Other devices can now SSH into this machine after pairing.");
    println!();
}
