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
use std::time::Duration;
use tokio::net::TcpStream;

use super::{info, success, warn};

/// Port the OS SSH server listens on
pub const SSH_PORT: u16 = 22;

/// True when `host:port` accepts a TCP connection within 3 seconds
pub async fn port_open(host: &str, port: u16) -> bool {
    matches!(
        tokio::time::timeout(Duration::from_secs(3), TcpStream::connect((host, port))).await,
        Ok(Ok(_))
    )
}

/// Make sure the local SSH server is running before a listener hands out
/// access: a key installed into authorized_keys is useless while nothing
/// accepts connections on port 22. Enables the server when it is down.
pub async fn ensure_running() -> Result<()> {
    if port_open("127.0.0.1", SSH_PORT).await {
        success("SSH server is running on port 22");
        return Ok(());
    }

    warn("SSH server is not running; enabling it so paired devices can log in");
    let result = platform::enable().await;
    if result.is_err() {
        info(
            "Paired devices cannot log in until the SSH server runs; the listener was not started.",
        );
    }
    result
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_port_open_detects_listener() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        assert!(port_open("127.0.0.1", port).await);

        drop(listener);
        assert!(!port_open("127.0.0.1", port).await);
    }
}
