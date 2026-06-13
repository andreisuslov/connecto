//! Scan command - Discover devices on the local network

use anyhow::Result;
use colored::Colorize;
use connecto_core::discovery::{DiscoveredDevice, ServiceBrowser, SubnetScanner, DEFAULT_PORT};
#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
use connecto_core::fallback::AdHocNetwork;
#[cfg(any(
    feature = "bluetooth",
    any(target_os = "macos", target_os = "linux", target_os = "windows")
))]
use connecto_core::fallback::FallbackHandler;
use connecto_core::Config;
use directories::ProjectDirs;
use std::fs;
#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::{info, success};

/// Per-user cache of discovered devices, read back by `connecto pair <n>`
///
/// Lives in the user's cache directory (same `ProjectDirs` qualifiers as the
/// user config), not in world-writable `/tmp`.
fn default_cache_path() -> Result<PathBuf> {
    let proj_dirs = ProjectDirs::from("com", "connecto", "connecto")
        .ok_or_else(|| anyhow::anyhow!("Could not determine cache directory"))?;
    Ok(proj_dirs.cache_dir().join("devices.json"))
}

#[allow(dead_code)]
pub async fn run(timeout: u64) -> Result<()> {
    run_with_options(timeout, false, vec![], false).await
}

#[allow(dead_code)]
pub async fn run_with_fallback(timeout: u64, fallback: bool) -> Result<()> {
    run_with_options(timeout, fallback, vec![], false).await
}

pub async fn run_with_options(
    timeout: u64,
    _fallback: bool,
    cli_subnets: Vec<String>,
    bluetooth_enabled: bool,
) -> Result<()> {
    println!();
    println!("{}", "  CONNECTO SCANNER  ".on_bright_cyan().white().bold());
    println!();

    // Load saved subnets from config
    let config = Config::load().unwrap_or_default();
    let mut all_subnets = cli_subnets;
    for subnet in config.subnets {
        if !all_subnets.contains(&subnet) {
            all_subnets.push(subnet);
        }
    }

    info("Scanning for devices...");
    println!();

    // Try mDNS first
    let spinner = super::spinner("cyan", "Searching via mDNS...");

    let browser = ServiceBrowser::new()?;
    let mut devices = browser
        .scan_for_duration(Duration::from_secs(timeout))
        .await?;

    spinner.finish_and_clear();

    // If mDNS found nothing, try subnet scanning (local + configured subnets)
    if devices.is_empty() {
        let spinner = super::spinner("yellow", "Scanning subnets...");

        let scanner = SubnetScanner::new(DEFAULT_PORT, Duration::from_millis(500));

        // Scan local subnets
        devices = scanner.scan().await;

        // Also scan configured subnets
        if !all_subnets.is_empty() {
            let extra = scanner.scan_subnets(&all_subnets).await;
            for device in extra {
                if !devices.iter().any(|d| d.addresses == device.addresses) {
                    devices.push(device);
                }
            }
        }

        spinner.finish_and_clear();
    };

    // If still no devices, try fallback: scan for ad-hoc networks
    if devices.is_empty() {
        let spinner = super::spinner("magenta", "Scanning for Connecto ad-hoc networks...");

        // Look for connecto ad-hoc networks (blocking subprocess work runs
        // off the async runtime)
        #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
        {
            let adhoc_networks = tokio::task::spawn_blocking(AdHocNetwork::scan_for_networks)
                .await
                .unwrap_or_else(|_| Ok(Vec::new()))
                .unwrap_or_default();
            spinner.finish_and_clear();

            if !adhoc_networks.is_empty() {
                info(&format!(
                    "Found {} Connecto ad-hoc network(s)",
                    adhoc_networks.len()
                ));
                println!();

                for (i, network) in adhoc_networks.iter().enumerate() {
                    println!(
                        "  {} {} {}",
                        format!("[{}]", i).green().bold(),
                        "Ad-hoc:".magenta(),
                        network.cyan().bold()
                    );
                }
                println!();

                // Ask user if they want to join
                println!(
                    "{}",
                    "These are direct Connecto networks (router bypass).".dimmed()
                );
                println!(
                    "  {} connecto scan --join {}",
                    "→".cyan(),
                    adhoc_networks.first().unwrap_or(&String::new())
                );
                println!();

                // Try to join the first one automatically to probe for the host
                let mut handler = FallbackHandler::new("scanner", Duration::from_secs(10));
                let join_result = handler.establish_fallback_connection(false).await;

                // Always restore the previous WiFi state before showing
                // results, whatever the join attempt did (the AdHocNetwork
                // Drop impl is only the backstop).
                let _ = tokio::task::spawn_blocking(move || handler.cleanup()).await;

                if let Ok(Some(host_ip)) = join_result {
                    info(&format!(
                        "Found Connecto host at {} via ad-hoc network",
                        host_ip
                    ));
                    println!(
                        "  {} {}",
                        "→".cyan(),
                        "Previous WiFi restored - rejoin the ad-hoc network to pair.".dimmed()
                    );

                    let device = DiscoveredDevice {
                        name: adhoc_networks
                            .first()
                            .unwrap_or(&"Connecto-Device".to_string())
                            .clone(),
                        hostname: "adhoc.local.".to_string(),
                        addresses: vec![host_ip
                            .parse::<IpAddr>()
                            .unwrap_or(IpAddr::V4(std::net::Ipv4Addr::new(192, 168, 73, 1)))],
                        port: DEFAULT_PORT,
                        instance_name: "adhoc._connecto._tcp.local.".to_string(),
                    };
                    devices.push(device);
                }
            }
        }

        #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
        {
            spinner.finish_and_clear();
        }
    }

    // If still no devices and bluetooth is enabled, try BLE scanning
    #[cfg(feature = "bluetooth")]
    if devices.is_empty() && bluetooth_enabled {
        let spinner = super::spinner("blue", "Scanning via Bluetooth...");

        let mut handler = FallbackHandler::new("scanner", Duration::from_secs(timeout));
        match handler.scan_bluetooth(Duration::from_secs(timeout)).await {
            Ok(bt_devices) => {
                spinner.finish_and_clear();

                if !bt_devices.is_empty() {
                    info(&format!(
                        "Found {} device(s) via Bluetooth",
                        bt_devices.len()
                    ));

                    // Convert BluetoothDevice to DiscoveredDevice
                    for bt_device in bt_devices {
                        if let Some(ip) = bt_device.ip_address {
                            let device = DiscoveredDevice {
                                name: format!("{} (BLE)", bt_device.name),
                                hostname: format!(
                                    "{}.ble.local.",
                                    bt_device.ble_address.replace(':', "")
                                ),
                                addresses: vec![ip],
                                port: bt_device.port,
                                instance_name: format!("{}._connecto._tcp.local.", bt_device.name),
                            };
                            // Only add if we don't already have this device
                            if !devices.iter().any(|d| d.addresses.contains(&ip)) {
                                devices.push(device);
                            }
                        } else {
                            // Device found but no IP info - display for reference
                            println!(
                                "  {} {} {}",
                                "•".blue(),
                                bt_device.name.cyan(),
                                format!("(BLE: {}, no IP info)", bt_device.ble_address).dimmed()
                            );
                        }
                    }
                }
            }
            Err(e) => {
                spinner.finish_and_clear();
                println!(
                    "  {} Bluetooth scan failed: {}",
                    "!".yellow(),
                    e.to_string().dimmed()
                );
            }
        }
    }

    // Suppress unused variable warning when bluetooth feature is disabled
    #[cfg(not(feature = "bluetooth"))]
    let _ = bluetooth_enabled;

    if devices.is_empty() {
        println!("{}", "No devices found.".yellow());
        println!();
        println!("{}", "Make sure:".dimmed());
        println!(
            "  {} The target device is running 'connecto listen'",
            "•".dimmed()
        );
        println!(
            "  {} Your firewall allows connections on port 8099",
            "•".dimmed()
        );
        println!();
        println!(
            "{}",
            "If devices are on different subnets (e.g., VPN):".dimmed()
        );
        println!("  {} connecto config add-subnet 10.x.x.0/24", "→".cyan());
        println!();
        println!(
            "{}",
            "If the target IS listening on this network, your router may be".dimmed()
        );
        println!(
            "{}",
            "isolating devices from each other (AP/client isolation, common".dimmed()
        );
        println!(
            "{}",
            "on guest networks). 'connecto listen --adhoc' on the target".dimmed()
        );
        println!(
            "{}",
            "creates a direct WiFi network that bypasses it.".dimmed()
        );
        println!();
        println!("{}", "Or pair directly if you know the IP:".dimmed());
        println!("  {} connecto pair <ip>:8099", "→".cyan());
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
        let num = format!("[{}]", i).green().bold();
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
    cache_devices_to(&default_cache_path()?, devices)
}

/// Write the device cache to an explicit path (separated for testing)
fn cache_devices_to(path: &Path, devices: &[DiscoveredDevice]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string(devices)?;
    fs::write(path, json)?;
    Ok(())
}

/// Load cached devices from the scan command
pub fn load_cached_devices() -> Result<Vec<DiscoveredDevice>> {
    load_cached_devices_from(&default_cache_path()?)
}

/// Load the device cache from an explicit path (separated for testing)
fn load_cached_devices_from(path: &Path) -> Result<Vec<DiscoveredDevice>> {
    let content = fs::read_to_string(path)?;
    let devices: Vec<DiscoveredDevice> = serde_json::from_str(&content)?;
    Ok(devices)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_extract_friendly_name() {
        let full = "My Device (hostname)._connecto._tcp.local.";
        assert_eq!(extract_friendly_name(full), "My Device (hostname)");

        let simple = "Test";
        assert_eq!(extract_friendly_name(simple), "Test");
    }

    #[test]
    fn test_device_cache_round_trip() {
        let temp = TempDir::new().unwrap();
        // Nested path: the cache dir is created on demand.
        let path = temp.path().join("cache").join("devices.json");

        let devices = vec![DiscoveredDevice {
            name: "Test Device._connecto._tcp.local.".to_string(),
            hostname: "test.local.".to_string(),
            addresses: vec!["192.168.1.10".parse().unwrap()],
            port: 8099,
            instance_name: "Test._connecto._tcp.local.".to_string(),
        }];

        cache_devices_to(&path, &devices).unwrap();
        let loaded = load_cached_devices_from(&path).unwrap();

        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].name, devices[0].name);
        assert_eq!(loaded[0].addresses, devices[0].addresses);
        assert_eq!(loaded[0].port, devices[0].port);
    }

    #[test]
    fn test_load_cached_devices_missing_file_errors() {
        let temp = TempDir::new().unwrap();
        assert!(load_cached_devices_from(&temp.path().join("nope.json")).is_err());
    }
}
