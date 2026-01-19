//! mDNS-based device discovery module
//!
//! Handles automatic discovery of Connecto instances on the local network

use crate::error::{ConnectoError, Result};
use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{debug, info};

/// The mDNS service type for Connecto
pub const SERVICE_TYPE: &str = "_connecto._tcp.local.";
/// Default port for the Connecto handshake service
pub const DEFAULT_PORT: u16 = 8099;

/// Represents a discovered Connecto device
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiscoveredDevice {
    pub name: String,
    pub hostname: String,
    pub addresses: Vec<IpAddr>,
    pub port: u16,
    pub instance_name: String,
}

impl DiscoveredDevice {
    /// Get the primary IP address (prefers IPv4)
    pub fn primary_address(&self) -> Option<IpAddr> {
        // Prefer IPv4 addresses
        self.addresses
            .iter()
            .find(|addr| addr.is_ipv4())
            .or(self.addresses.first())
            .copied()
    }

    /// Format as a connection string
    pub fn connection_string(&self) -> Option<String> {
        self.primary_address()
            .map(|addr| format!("{}:{}", addr, self.port))
    }
}

/// Events emitted during discovery
#[derive(Debug, Clone)]
pub enum DiscoveryEvent {
    DeviceFound(DiscoveredDevice),
    DeviceLost(String), // instance name
    SearchStarted,
    SearchStopped,
}

/// Service advertiser for making this device discoverable
pub struct ServiceAdvertiser {
    daemon: ServiceDaemon,
    service_fullname: Option<String>,
}

impl ServiceAdvertiser {
    /// Create a new service advertiser
    pub fn new() -> Result<Self> {
        let daemon = ServiceDaemon::new()
            .map_err(|e| ConnectoError::Discovery(format!("Failed to create mDNS daemon: {}", e)))?;

        Ok(Self {
            daemon,
            service_fullname: None,
        })
    }

    /// Start advertising this device
    pub fn advertise(&mut self, device_name: &str, port: u16) -> Result<()> {
        let hostname = hostname::get()
            .map(|h: std::ffi::OsString| h.to_string_lossy().to_string())
            .unwrap_or_else(|_| "unknown".to_string());

        let service_hostname = format!("{}.local.", hostname);
        let instance_name = format!("{} ({})", device_name, hostname);

        let service_info = ServiceInfo::new(
            SERVICE_TYPE,
            &instance_name,
            &service_hostname,
            "",
            port,
            None,
        )
        .map_err(|e| ConnectoError::Discovery(format!("Failed to create service info: {}", e)))?;

        let fullname = service_info.get_fullname().to_string();

        self.daemon
            .register(service_info)
            .map_err(|e| ConnectoError::Discovery(format!("Failed to register service: {}", e)))?;

        self.service_fullname = Some(fullname.clone());
        info!("Advertising service: {}", fullname);

        Ok(())
    }

    /// Stop advertising
    pub fn stop(&mut self) -> Result<()> {
        if let Some(fullname) = self.service_fullname.take() {
            self.daemon
                .unregister(&fullname)
                .map_err(|e| ConnectoError::Discovery(format!("Failed to unregister: {}", e)))?;
            info!("Stopped advertising service");
        }
        Ok(())
    }
}

impl Drop for ServiceAdvertiser {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

/// Service browser for discovering other devices
pub struct ServiceBrowser {
    daemon: ServiceDaemon,
    devices: Arc<Mutex<HashMap<String, DiscoveredDevice>>>,
}

impl ServiceBrowser {
    /// Create a new service browser
    pub fn new() -> Result<Self> {
        let daemon = ServiceDaemon::new()
            .map_err(|e| ConnectoError::Discovery(format!("Failed to create mDNS daemon: {}", e)))?;

        Ok(Self {
            daemon,
            devices: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    /// Start browsing for devices
    pub fn browse(&self) -> Result<mpsc::Receiver<DiscoveryEvent>> {
        let receiver = self
            .daemon
            .browse(SERVICE_TYPE)
            .map_err(|e| ConnectoError::Discovery(format!("Failed to browse: {}", e)))?;

        let (tx, rx) = mpsc::channel(100);
        let devices = Arc::clone(&self.devices);

        std::thread::spawn(move || {
            let rt = tokio::runtime::Handle::try_current().ok();

            while let Ok(event) = receiver.recv() {
                match event {
                    ServiceEvent::ServiceResolved(info) => {
                        let device = DiscoveredDevice {
                            name: info.get_fullname().to_string(),
                            hostname: info.get_hostname().to_string(),
                            addresses: info.get_addresses().iter().copied().collect(),
                            port: info.get_port(),
                            instance_name: info.get_fullname().to_string(),
                        };

                        debug!("Discovered device: {:?}", device);

                        {
                            let mut devs = devices.lock().unwrap();
                            devs.insert(device.instance_name.clone(), device.clone());
                        }

                        let event = DiscoveryEvent::DeviceFound(device);
                        if let Some(ref handle) = rt {
                            let tx = tx.clone();
                            handle.spawn(async move {
                                let _ = tx.send(event).await;
                            });
                        } else {
                            // Blocking send if no runtime
                            let _ = tx.blocking_send(event);
                        }
                    }
                    ServiceEvent::ServiceRemoved(_, fullname) => {
                        {
                            let mut devs = devices.lock().unwrap();
                            devs.remove(&fullname);
                        }

                        let event = DiscoveryEvent::DeviceLost(fullname);
                        if let Some(ref handle) = rt {
                            let tx = tx.clone();
                            handle.spawn(async move {
                                let _ = tx.send(event).await;
                            });
                        } else {
                            let _ = tx.blocking_send(event);
                        }
                    }
                    ServiceEvent::SearchStarted(_) => {
                        debug!("mDNS search started");
                        let _ = tx.blocking_send(DiscoveryEvent::SearchStarted);
                    }
                    ServiceEvent::SearchStopped(_) => {
                        debug!("mDNS search stopped");
                        let _ = tx.blocking_send(DiscoveryEvent::SearchStopped);
                        break;
                    }
                    _ => {}
                }
            }
        });

        Ok(rx)
    }

    /// Get currently discovered devices
    pub fn get_devices(&self) -> Vec<DiscoveredDevice> {
        let devices = self.devices.lock().unwrap();
        devices.values().cloned().collect()
    }

    /// Scan for devices for a specified duration
    pub async fn scan_for_duration(&self, duration: Duration) -> Result<Vec<DiscoveredDevice>> {
        let mut rx = self.browse()?;

        let timeout = tokio::time::sleep(duration);
        tokio::pin!(timeout);

        loop {
            tokio::select! {
                _ = &mut timeout => {
                    break;
                }
                event = rx.recv() => {
                    match event {
                        Some(DiscoveryEvent::DeviceFound(device)) => {
                            debug!("Found device during scan: {}", device.name);
                        }
                        Some(DiscoveryEvent::SearchStopped) => {
                            break;
                        }
                        None => break,
                        _ => {}
                    }
                }
            }
        }

        Ok(self.get_devices())
    }
}

/// Get the current hostname
pub fn get_hostname() -> String {
    hostname::get()
        .map(|h: std::ffi::OsString| h.to_string_lossy().to_string())
        .unwrap_or_else(|_| "unknown".to_string())
}

/// Get local IP addresses
pub fn get_local_addresses() -> Vec<IpAddr> {
    let mut addresses = Vec::new();

    if let Ok(ifaces) = local_ip_address::list_afinet_netifas() {
        for (_, ip) in ifaces {
            if !ip.is_loopback() {
                addresses.push(ip);
            }
        }
    }

    addresses
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_service_type_constant() {
        assert_eq!(SERVICE_TYPE, "_connecto._tcp.local.");
    }

    #[test]
    fn test_default_port() {
        assert_eq!(DEFAULT_PORT, 8099);
    }

    #[test]
    fn test_discovered_device_creation() {
        let device = DiscoveredDevice {
            name: "Test Device".to_string(),
            hostname: "test.local.".to_string(),
            addresses: vec!["192.168.1.100".parse().unwrap()],
            port: 8099,
            instance_name: "test-instance".to_string(),
        };

        assert_eq!(device.name, "Test Device");
        assert_eq!(device.port, 8099);
    }

    #[test]
    fn test_primary_address_prefers_ipv4() {
        let device = DiscoveredDevice {
            name: "Test".to_string(),
            hostname: "test.local.".to_string(),
            addresses: vec![
                "::1".parse().unwrap(),
                "192.168.1.100".parse().unwrap(),
                "fe80::1".parse().unwrap(),
            ],
            port: 8099,
            instance_name: "test".to_string(),
        };

        let primary = device.primary_address().unwrap();
        assert!(primary.is_ipv4());
        assert_eq!(primary.to_string(), "192.168.1.100");
    }

    #[test]
    fn test_primary_address_fallback_to_ipv6() {
        let device = DiscoveredDevice {
            name: "Test".to_string(),
            hostname: "test.local.".to_string(),
            addresses: vec!["::1".parse().unwrap()],
            port: 8099,
            instance_name: "test".to_string(),
        };

        let primary = device.primary_address().unwrap();
        assert!(primary.is_ipv6());
    }

    #[test]
    fn test_connection_string() {
        let device = DiscoveredDevice {
            name: "Test".to_string(),
            hostname: "test.local.".to_string(),
            addresses: vec!["192.168.1.100".parse().unwrap()],
            port: 8099,
            instance_name: "test".to_string(),
        };

        assert_eq!(
            device.connection_string(),
            Some("192.168.1.100:8099".to_string())
        );
    }

    #[test]
    fn test_connection_string_empty_addresses() {
        let device = DiscoveredDevice {
            name: "Test".to_string(),
            hostname: "test.local.".to_string(),
            addresses: vec![],
            port: 8099,
            instance_name: "test".to_string(),
        };

        assert_eq!(device.connection_string(), None);
    }

    #[test]
    fn test_discovered_device_equality() {
        let device1 = DiscoveredDevice {
            name: "Test".to_string(),
            hostname: "test.local.".to_string(),
            addresses: vec!["192.168.1.100".parse().unwrap()],
            port: 8099,
            instance_name: "test".to_string(),
        };

        let device2 = device1.clone();
        assert_eq!(device1, device2);
    }

    #[test]
    fn test_discovered_device_serialization() {
        let device = DiscoveredDevice {
            name: "Test Device".to_string(),
            hostname: "test.local.".to_string(),
            addresses: vec!["192.168.1.100".parse().unwrap()],
            port: 8099,
            instance_name: "test-instance".to_string(),
        };

        let json = serde_json::to_string(&device).unwrap();
        let deserialized: DiscoveredDevice = serde_json::from_str(&json).unwrap();

        assert_eq!(device, deserialized);
    }

    #[test]
    fn test_get_hostname() {
        let hostname = get_hostname();
        assert!(!hostname.is_empty());
    }

    #[test]
    fn test_discovery_event_variants() {
        let device = DiscoveredDevice {
            name: "Test".to_string(),
            hostname: "test.local.".to_string(),
            addresses: vec![],
            port: 8099,
            instance_name: "test".to_string(),
        };

        let event1 = DiscoveryEvent::DeviceFound(device);
        let event2 = DiscoveryEvent::DeviceLost("test".to_string());
        let event3 = DiscoveryEvent::SearchStarted;
        let event4 = DiscoveryEvent::SearchStopped;

        // Just verify these compile and can be pattern matched
        match event1 {
            DiscoveryEvent::DeviceFound(_) => {}
            _ => panic!("Wrong variant"),
        }
        match event2 {
            DiscoveryEvent::DeviceLost(_) => {}
            _ => panic!("Wrong variant"),
        }
        match event3 {
            DiscoveryEvent::SearchStarted => {}
            _ => panic!("Wrong variant"),
        }
        match event4 {
            DiscoveryEvent::SearchStopped => {}
            _ => panic!("Wrong variant"),
        }
    }

    // Integration test - requires network access
    #[tokio::test]
    #[ignore] // Run manually with: cargo test -- --ignored
    async fn test_service_advertiser_creation() {
        let advertiser = ServiceAdvertiser::new();
        assert!(advertiser.is_ok());
    }

    #[tokio::test]
    #[ignore] // Run manually with: cargo test -- --ignored
    async fn test_service_browser_creation() {
        let browser = ServiceBrowser::new();
        assert!(browser.is_ok());
    }
}
