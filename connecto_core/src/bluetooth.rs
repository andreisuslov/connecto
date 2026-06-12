//! Bluetooth Low Energy discovery module
//!
//! Provides BLE-based device discovery as a fallback when mDNS/subnet scanning fails:
//! - Scanning: All platforms (macOS, Linux, Windows) via btleplug
//! - Advertising: Linux only via bluer (BlueZ D-Bus API)
//!
//! # BLE Protocol
//!
//! Connecto uses a custom GATT service for device discovery:
//! - Service UUID: `e7b3f5a0-3b6d-4c2a-9e8f-1a2b3c4d5e6f`
//! - Device Info Characteristic: `e7b3f5a1-3b6d-4c2a-9e8f-1a2b3c4d5e6f`
//!
//! The characteristic contains encoded device information (IP, port, name).

use crate::error::{ConnectoError, Result};
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

/// Connecto BLE Service UUID
pub const BLE_SERVICE_UUID: uuid::Uuid = uuid::uuid!("e7b3f5a0-3b6d-4c2a-9e8f-1a2b3c4d5e6f");

/// Device Info Characteristic UUID - contains IP, port, and device name
pub const BLE_CHAR_DEVICE_INFO: uuid::Uuid = uuid::uuid!("e7b3f5a1-3b6d-4c2a-9e8f-1a2b3c4d5e6f");

/// Magic bytes for Connecto BLE protocol
const BLE_MAGIC: [u8; 2] = [0x43, 0x4F]; // "CO"

/// Protocol version
const BLE_PROTOCOL_VERSION: u8 = 1;

/// IP type markers
const IP_TYPE_V4: u8 = 0x04;
const IP_TYPE_V6: u8 = 0x06;

/// Discovered Bluetooth device
#[derive(Debug, Clone)]
pub struct BluetoothDevice {
    /// Device display name
    pub name: String,
    /// IP address parsed from BLE characteristic (if available)
    pub ip_address: Option<IpAddr>,
    /// TCP port for connecto service
    pub port: u16,
    /// Signal strength (if available)
    pub rssi: Option<i16>,
    /// BLE MAC address
    pub ble_address: String,
}

/// Bluetooth discovery events
#[derive(Debug, Clone)]
pub enum BluetoothEvent {
    /// A new device was discovered
    DeviceFound(BluetoothDevice),
    /// A previously discovered device is no longer visible
    DeviceLost(String),
    /// Scanning has started
    ScanStarted,
    /// Scanning has stopped
    ScanStopped,
    /// An error occurred during scanning
    Error(String),
}

/// Encode device information for BLE characteristic
///
/// Format (21 bytes for IPv4, 33 bytes for IPv6):
/// | Field      | Bytes | Description              |
/// |------------|-------|--------------------------|
/// | Magic      | 2     | "CO" (0x43, 0x4F)        |
/// | Version    | 1     | Protocol version (1)     |
/// | Port       | 2     | TCP port (big endian)    |
/// | IP Type    | 1     | 0x04=IPv4, 0x06=IPv6     |
/// | IP Address | 4/16  | IP bytes                 |
/// | Name Len   | 1     | Device name length       |
/// | Name       | 0-10  | UTF-8 name (truncated)   |
pub fn encode_device_info(ip: IpAddr, port: u16, name: &str) -> Vec<u8> {
    let mut data = Vec::with_capacity(33);

    // Magic
    data.extend_from_slice(&BLE_MAGIC);

    // Version
    data.push(BLE_PROTOCOL_VERSION);

    // Port (big endian)
    data.extend_from_slice(&port.to_be_bytes());

    // IP type and address
    match ip {
        IpAddr::V4(addr) => {
            data.push(IP_TYPE_V4);
            data.extend_from_slice(&addr.octets());
        }
        IpAddr::V6(addr) => {
            data.push(IP_TYPE_V6);
            data.extend_from_slice(&addr.octets());
        }
    }

    // Name (truncated to 10 bytes)
    let name_bytes = name.as_bytes();
    let name_len = name_bytes.len().min(10);
    data.push(name_len as u8);
    data.extend_from_slice(&name_bytes[..name_len]);

    data
}

/// Decode device information from BLE characteristic
pub fn decode_device_info(data: &[u8]) -> Result<(IpAddr, u16, String)> {
    if data.len() < 8 {
        return Err(ConnectoError::Bluetooth("Data too short".to_string()));
    }

    // Verify magic
    if data[0..2] != BLE_MAGIC {
        return Err(ConnectoError::Bluetooth("Invalid magic bytes".to_string()));
    }

    // Check version
    if data[2] != BLE_PROTOCOL_VERSION {
        return Err(ConnectoError::Bluetooth(format!(
            "Unsupported protocol version: {}",
            data[2]
        )));
    }

    // Parse port
    let port = u16::from_be_bytes([data[3], data[4]]);

    // Parse IP
    let ip_type = data[5];
    let (ip, name_offset) = match ip_type {
        IP_TYPE_V4 => {
            if data.len() < 11 {
                return Err(ConnectoError::Bluetooth(
                    "Data too short for IPv4".to_string(),
                ));
            }
            let octets: [u8; 4] = data[6..10].try_into().unwrap();
            (IpAddr::V4(octets.into()), 10)
        }
        IP_TYPE_V6 => {
            if data.len() < 23 {
                return Err(ConnectoError::Bluetooth(
                    "Data too short for IPv6".to_string(),
                ));
            }
            let octets: [u8; 16] = data[6..22].try_into().unwrap();
            (IpAddr::V6(octets.into()), 22)
        }
        _ => {
            return Err(ConnectoError::Bluetooth(format!(
                "Unknown IP type: {}",
                ip_type
            )))
        }
    };

    // Parse name
    let name = if data.len() > name_offset {
        let name_len = data[name_offset] as usize;
        if data.len() >= name_offset + 1 + name_len {
            String::from_utf8_lossy(&data[name_offset + 1..name_offset + 1 + name_len]).to_string()
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    Ok((ip, port, name))
}

/// BLE Scanner using btleplug (works on all platforms)
pub struct BluetoothBrowser {
    adapter: btleplug::platform::Adapter,
    discovered_devices: Arc<Mutex<HashMap<String, BluetoothDevice>>>,
}

impl BluetoothBrowser {
    /// Create a new Bluetooth browser
    ///
    /// # Errors
    /// Returns an error if no Bluetooth adapter is found or if Bluetooth is disabled.
    pub async fn new() -> Result<Self> {
        use btleplug::api::Manager as _;
        use btleplug::platform::Manager;

        let manager = Manager::new()
            .await
            .map_err(|e| ConnectoError::Bluetooth(format!("Failed to create BLE manager: {}", e)))?;

        let adapters = manager
            .adapters()
            .await
            .map_err(|e| ConnectoError::Bluetooth(format!("Failed to get adapters: {}", e)))?;

        let adapter = adapters.into_iter().next().ok_or_else(|| {
            ConnectoError::Bluetooth(
                "Bluetooth adapter not found. Ensure Bluetooth is enabled in system settings."
                    .to_string(),
            )
        })?;

        info!("Bluetooth adapter initialized");

        Ok(Self {
            adapter,
            discovered_devices: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    /// Scan for Connecto devices via BLE
    ///
    /// Scans for devices advertising the Connecto BLE service UUID and attempts
    /// to read device information from the characteristic.
    pub async fn scan(&self, duration: Duration) -> Result<Vec<BluetoothDevice>> {
        use btleplug::api::{Central, CentralEvent, ScanFilter};
        use futures::StreamExt;

        info!("Starting BLE scan for {} seconds", duration.as_secs());

        // Set up scan filter for our service UUID
        let filter = ScanFilter {
            services: vec![BLE_SERVICE_UUID],
        };

        // Start scanning
        self.adapter
            .start_scan(filter)
            .await
            .map_err(|e| ConnectoError::Bluetooth(format!("Failed to start scan: {}", e)))?;

        // Get event stream
        let mut events = self
            .adapter
            .events()
            .await
            .map_err(|e| ConnectoError::Bluetooth(format!("Failed to get events: {}", e)))?;

        // Collect devices during the scan duration
        let discovered = self.discovered_devices.clone();
        let scan_task = async {
            let deadline = tokio::time::Instant::now() + duration;

            while tokio::time::Instant::now() < deadline {
                let timeout_duration = deadline - tokio::time::Instant::now();

                match tokio::time::timeout(timeout_duration, events.next()).await {
                    Ok(Some(event)) => {
                        if let CentralEvent::DeviceDiscovered(id) = event {
                            debug!("Discovered BLE device: {:?}", id);
                            if let Ok(peripheral) = self.adapter.peripheral(&id).await {
                                if let Ok(Some(device)) =
                                    self.process_peripheral(&peripheral).await
                                {
                                    let mut devices = discovered.lock().await;
                                    devices.insert(device.ble_address.clone(), device);
                                }
                            }
                        }
                    }
                    Ok(None) => break,
                    Err(_) => break, // Timeout reached
                }
            }
        };

        scan_task.await;

        // Stop scanning
        let _ = self.adapter.stop_scan().await;

        let devices = self.discovered_devices.lock().await;
        let result: Vec<BluetoothDevice> = devices.values().cloned().collect();

        info!("BLE scan complete. Found {} devices", result.len());
        Ok(result)
    }

    /// Process a discovered peripheral and extract device info
    async fn process_peripheral(
        &self,
        peripheral: &btleplug::platform::Peripheral,
    ) -> Result<Option<BluetoothDevice>> {
        use btleplug::api::Peripheral as _;

        // Get peripheral properties
        let properties = peripheral
            .properties()
            .await
            .map_err(|e| ConnectoError::Bluetooth(format!("Failed to get properties: {}", e)))?;

        let Some(props) = properties else {
            return Ok(None);
        };

        let ble_address = peripheral.id().to_string();
        let rssi = props.rssi;
        let local_name = props.local_name.clone();

        debug!(
            "Processing peripheral: {} (name: {:?}, rssi: {:?})",
            ble_address, local_name, rssi
        );

        // Try to connect and read the characteristic
        match peripheral.connect().await {
            Ok(()) => {
                debug!("Connected to {}", ble_address);
            }
            Err(e) => {
                warn!("Failed to connect to {}: {}", ble_address, e);
                // Return basic device info without IP
                return Ok(Some(BluetoothDevice {
                    name: local_name.unwrap_or_else(|| "Unknown".to_string()),
                    ip_address: None,
                    port: 0,
                    rssi,
                    ble_address,
                }));
            }
        }

        // Discover services
        if let Err(e) = peripheral.discover_services().await {
            warn!("Failed to discover services: {}", e);
            let _ = peripheral.disconnect().await;
            return Ok(None);
        }

        // Find our service and characteristic
        let characteristics = peripheral.characteristics();
        let device_info_char = characteristics
            .iter()
            .find(|c| c.uuid == BLE_CHAR_DEVICE_INFO);

        let device = if let Some(char) = device_info_char {
            match peripheral.read(char).await {
                Ok(data) => match decode_device_info(&data) {
                    Ok((ip, port, name)) => {
                        debug!(
                            "Decoded device info: {} at {}:{}",
                            name,
                            ip,
                            port
                        );
                        Some(BluetoothDevice {
                            name: if name.is_empty() {
                                local_name.unwrap_or_else(|| "Unknown".to_string())
                            } else {
                                name
                            },
                            ip_address: Some(ip),
                            port,
                            rssi,
                            ble_address,
                        })
                    }
                    Err(e) => {
                        warn!("Failed to decode device info: {}", e);
                        None
                    }
                },
                Err(e) => {
                    warn!("Failed to read characteristic: {}", e);
                    None
                }
            }
        } else {
            warn!("Device info characteristic not found");
            None
        };

        // Disconnect
        let _ = peripheral.disconnect().await;

        Ok(device)
    }

    /// Get all currently discovered devices
    pub async fn get_devices(&self) -> Vec<BluetoothDevice> {
        let devices = self.discovered_devices.lock().await;
        devices.values().cloned().collect()
    }

    /// Clear discovered devices
    pub async fn clear_devices(&self) {
        let mut devices = self.discovered_devices.lock().await;
        devices.clear();
    }
}

/// BLE Advertiser for Linux (uses bluer/BlueZ)
///
/// On non-Linux platforms, all methods return "not supported" errors.
#[cfg(target_os = "linux")]
pub struct BluetoothAdvertiser {
    session: bluer::Session,
    adapter: bluer::Adapter,
    advertisement_handle: Option<bluer::adv::AdvertisementHandle>,
    app_handle: Option<bluer::gatt::local::ApplicationHandle>,
}

#[cfg(target_os = "linux")]
impl BluetoothAdvertiser {
    /// Create a new Bluetooth advertiser
    ///
    /// # Errors
    /// Returns an error if BlueZ is not available or Bluetooth is disabled.
    pub async fn new() -> Result<Self> {
        let session = bluer::Session::new()
            .await
            .map_err(|e| ConnectoError::Bluetooth(format!("Failed to create BlueZ session: {}", e)))?;

        let adapter_name = session
            .default_adapter()
            .await
            .map_err(|e| ConnectoError::Bluetooth(format!("Failed to get default adapter: {}", e)))?;

        let adapter = session
            .adapter(&adapter_name)
            .map_err(|e| ConnectoError::Bluetooth(format!("Failed to get adapter: {}", e)))?;

        // Ensure adapter is powered on
        if !adapter.is_powered().await.unwrap_or(false) {
            adapter
                .set_powered(true)
                .await
                .map_err(|e| ConnectoError::Bluetooth(format!("Failed to power on adapter: {}", e)))?;
        }

        info!("BlueZ adapter initialized: {}", adapter_name);

        Ok(Self {
            session,
            adapter,
            advertisement_handle: None,
            app_handle: None,
        })
    }

    /// Start advertising the Connecto BLE service
    ///
    /// # Arguments
    /// * `name` - Device display name
    /// * `ip` - IP address to advertise
    /// * `port` - TCP port to advertise
    pub async fn advertise(&mut self, name: &str, ip: IpAddr, port: u16) -> Result<()> {
        use bluer::adv::{Advertisement, AdvertisementHandle};
        use bluer::gatt::local::{
            Application, Characteristic, CharacteristicRead, CharacteristicReadRequest, Service,
        };
        use std::collections::BTreeMap;

        info!("Starting BLE advertising: {} at {}:{}", name, ip, port);

        // Stop any existing advertisement
        self.stop().await?;

        // Create device info data
        let device_info = encode_device_info(ip, port, name);
        let device_info_arc = Arc::new(device_info);

        // Create GATT service
        let device_info_for_read = device_info_arc.clone();
        let service = Service {
            uuid: BLE_SERVICE_UUID,
            primary: true,
            characteristics: vec![Characteristic {
                uuid: BLE_CHAR_DEVICE_INFO,
                read: Some(CharacteristicRead {
                    read: true,
                    fun: Box::new(move |_req| {
                        let data = device_info_for_read.clone();
                        Box::pin(async move { Ok(data.to_vec()) })
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            }],
            ..Default::default()
        };

        let app = Application {
            services: vec![service],
            ..Default::default()
        };

        // Register GATT application
        let app_handle = self
            .adapter
            .serve_gatt_application(app)
            .await
            .map_err(|e| {
                ConnectoError::Bluetooth(format!("Failed to register GATT application: {}", e))
            })?;

        self.app_handle = Some(app_handle);

        // Create advertisement
        let mut service_uuids = std::collections::BTreeSet::new();
        service_uuids.insert(BLE_SERVICE_UUID);

        let le_advertisement = Advertisement {
            advertisement_type: bluer::adv::Type::Peripheral,
            service_uuids,
            local_name: Some(name.to_string()),
            discoverable: Some(true),
            ..Default::default()
        };

        let adv_handle = self
            .adapter
            .advertise(le_advertisement)
            .await
            .map_err(|e| ConnectoError::Bluetooth(format!("Failed to start advertising: {}", e)))?;

        self.advertisement_handle = Some(adv_handle);

        info!("BLE advertising started successfully");
        Ok(())
    }

    /// Stop advertising
    pub async fn stop(&mut self) -> Result<()> {
        if let Some(handle) = self.advertisement_handle.take() {
            drop(handle);
            info!("BLE advertising stopped");
        }
        if let Some(handle) = self.app_handle.take() {
            drop(handle);
        }
        Ok(())
    }
}

#[cfg(target_os = "linux")]
impl Drop for BluetoothAdvertiser {
    fn drop(&mut self) {
        // Handles are dropped automatically
    }
}

/// Stub BluetoothAdvertiser for non-Linux platforms
#[cfg(not(target_os = "linux"))]
pub struct BluetoothAdvertiser {
    _private: (),
}

#[cfg(not(target_os = "linux"))]
impl BluetoothAdvertiser {
    /// Create a new Bluetooth advertiser (not supported on this platform)
    pub async fn new() -> Result<Self> {
        Err(ConnectoError::Bluetooth(
            "Bluetooth advertising not supported on this platform. \
             Use 'connecto listen' on a Linux device for BLE advertising."
                .to_string(),
        ))
    }

    /// Start advertising (not supported)
    pub async fn advertise(&mut self, _name: &str, _ip: IpAddr, _port: u16) -> Result<()> {
        Err(ConnectoError::Bluetooth(
            "Bluetooth advertising not supported on this platform.".to_string(),
        ))
    }

    /// Stop advertising (no-op)
    pub async fn stop(&mut self) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[test]
    fn test_encode_decode_ipv4() {
        let ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100));
        let port = 12345;
        let name = "TestDevice";

        let encoded = encode_device_info(ip, port, name);
        let (decoded_ip, decoded_port, decoded_name) = decode_device_info(&encoded).unwrap();

        assert_eq!(decoded_ip, ip);
        assert_eq!(decoded_port, port);
        assert_eq!(decoded_name, name);
    }

    #[test]
    fn test_encode_decode_ipv6() {
        let ip = IpAddr::V6("fe80::1".parse().unwrap());
        let port = 54321;
        let name = "IPv6Device";

        let encoded = encode_device_info(ip, port, name);
        let (decoded_ip, decoded_port, decoded_name) = decode_device_info(&encoded).unwrap();

        assert_eq!(decoded_ip, ip);
        assert_eq!(decoded_port, port);
        assert_eq!(decoded_name, name);
    }

    #[test]
    fn test_name_truncation() {
        let ip = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1));
        let port = 8080;
        let long_name = "ThisIsAVeryLongDeviceName";

        let encoded = encode_device_info(ip, port, long_name);
        let (_, _, decoded_name) = decode_device_info(&encoded).unwrap();

        assert_eq!(decoded_name.len(), 10);
        assert_eq!(decoded_name, "ThisIsAVer");
    }

    #[test]
    fn test_empty_name() {
        let ip = IpAddr::V4(Ipv4Addr::new(1, 2, 3, 4));
        let port = 443;
        let name = "";

        let encoded = encode_device_info(ip, port, name);
        let (decoded_ip, decoded_port, decoded_name) = decode_device_info(&encoded).unwrap();

        assert_eq!(decoded_ip, ip);
        assert_eq!(decoded_port, port);
        assert_eq!(decoded_name, "");
    }

    #[test]
    fn test_invalid_magic() {
        let data = vec![0x00, 0x00, 0x01, 0x00, 0x50, 0x04, 192, 168, 1, 1, 0];
        let result = decode_device_info(&data);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Invalid magic"));
    }

    #[test]
    fn test_invalid_version() {
        let data = vec![0x43, 0x4F, 0x99, 0x00, 0x50, 0x04, 192, 168, 1, 1, 0];
        let result = decode_device_info(&data);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Unsupported protocol version"));
    }

    #[test]
    fn test_data_too_short() {
        let data = vec![0x43, 0x4F, 0x01];
        let result = decode_device_info(&data);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("too short"));
    }
}
