//! Scan command - Discover devices on the local network

use anyhow::Result;
use colored::Colorize;
use connecto_core::discovery::{DiscoveredDevice, ServiceBrowser};
use indicatif::{ProgressBar, ProgressStyle};
use std::fs;
use std::io::Write;
use std::time::Duration;

use super::{info, success};

/// File to cache discovered devices for the pair command
const CACHE_FILE: &str = "/tmp/connecto_devices.json";

pub async fn run(timeout: u64) -> Result<()> {
    println!();
    println!("{}", "  CONNECTO SCANNER  ".on_bright_cyan().white().bold());
    println!();

    info(&format!("Scanning for {} seconds...", timeout));
    println!();

    // Create a spinner
    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏")
            .template("{spinner:.cyan} {msg}")
            .unwrap(),
    );
    spinner.set_message("Searching for Connecto devices on the network...");
    spinner.enable_steady_tick(Duration::from_millis(80));

    // Scan for devices
    let browser = ServiceBrowser::new()?;
    let devices = browser
        .scan_for_duration(Duration::from_secs(timeout))
        .await?;

    spinner.finish_and_clear();

    if devices.is_empty() {
        println!("{}", "No devices found.".yellow());
        println!();
        println!("{}", "Make sure:".dimmed());
        println!("  {} The target device is running 'connecto listen'", "•".dimmed());
        println!("  {} Both devices are on the same network", "•".dimmed());
        println!("  {} Your firewall allows mDNS (port 5353)", "•".dimmed());
        println!();
        return Ok(());
    }

    // Display found devices
    success(&format!("Found {} device(s):", devices.len()));
    println!();

    display_devices(&devices);

    // Cache devices for pair command
    cache_devices(&devices)?;

    println!();
    println!(
        "{}",
        format!(
            "To pair with a device, run: {}",
            "connecto pair <number>".cyan()
        )
        .dimmed()
    );
    println!(
        "{}",
        format!(
            "Or connect directly: {}",
            "connecto pair <ip>:<port>".cyan()
        )
        .dimmed()
    );
    println!();

    Ok(())
}

fn display_devices(devices: &[DiscoveredDevice]) {
    for (i, device) in devices.iter().enumerate() {
        let num = format!("[{}]", i + 1).green().bold();
        let name = extract_friendly_name(&device.name);

        print!("{} {} ", num, name.cyan().bold());

        if let Some(addr) = device.primary_address() {
            print!("({}:{})", addr.to_string().yellow(), device.port);
        }

        println!();

        // Show additional addresses if any
        if device.addresses.len() > 1 {
            for addr in &device.addresses {
                if Some(*addr) != device.primary_address() {
                    println!("    {} {}", "└".dimmed(), addr.to_string().dimmed());
                }
            }
        }
    }
}

/// Extract a friendly name from the full service name
fn extract_friendly_name(full_name: &str) -> String {
    // Service name format: "Device Name (hostname)._connecto._tcp.local."
    full_name
        .split("._connecto")
        .next()
        .unwrap_or(full_name)
        .to_string()
}

fn cache_devices(devices: &[DiscoveredDevice]) -> Result<()> {
    let json = serde_json::to_string(devices)?;
    let mut file = fs::File::create(CACHE_FILE)?;
    file.write_all(json.as_bytes())?;
    Ok(())
}

/// Load cached devices from the scan command
pub fn load_cached_devices() -> Result<Vec<DiscoveredDevice>> {
    let content = fs::read_to_string(CACHE_FILE)?;
    let devices: Vec<DiscoveredDevice> = serde_json::from_str(&content)?;
    Ok(devices)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_friendly_name() {
        let full = "My Device (hostname)._connecto._tcp.local.";
        assert_eq!(extract_friendly_name(full), "My Device (hostname)");

        let simple = "Test";
        assert_eq!(extract_friendly_name(simple), "Test");
    }

    #[test]
    fn test_cache_file_path() {
        assert_eq!(CACHE_FILE, "/tmp/connecto_devices.json");
    }
}
