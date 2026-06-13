//! Fallback networking module
//!
//! Provides alternative connection methods when standard network discovery fails:
//! - Ad-hoc WiFi network creation and joining
//! - Bluetooth Low Energy discovery (scanning on all platforms, advertising on Linux)
//!
//! The cross-platform [`AdHocNetwork`] owns the lifecycle: previous-network
//! tracking, static-IP bookkeeping, restore-on-failure, and restore-on-drop.
//! The actual system commands live in per-OS backends in the `macos`, `linux`,
//! and `windows` submodules, which contain only command primitives
//! (create/join/restore/current-state) behind the private [`AdHocBackend`]
//! trait. All backend commands are blocking; async callers must go through
//! `spawn_blocking` (as [`FallbackHandler::establish_fallback_connection`]
//! does), which also keeps the restore-on-drop backstop usable from `Drop`.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
use crate::error::ConnectoError;
use crate::error::Result;
use std::time::Duration;
#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
use tracing::{debug, info, warn};

/// The prefix for connecto ad-hoc network names
pub const ADHOC_NETWORK_PREFIX: &str = "Connecto-";

/// Default channel for ad-hoc network
pub const ADHOC_CHANNEL: u32 = 11;

/// IP address the hosting side pins on the ad-hoc rendezvous subnet
pub const ADHOC_HOST_IP: &str = "192.168.73.1";

/// IP address the joining side pins on the ad-hoc rendezvous subnet
pub const ADHOC_CLIENT_IP: &str = "192.168.73.2";

/// Netmask of the ad-hoc rendezvous subnet
pub const ADHOC_NETMASK: &str = "255.255.255.0";

/// CIDR prefix length of the ad-hoc rendezvous subnet
pub const ADHOC_PREFIX_LEN: u8 = 24;

/// Result of a successful backend create/join primitive
#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
#[derive(Debug, Default)]
struct AdHocOutcome {
    /// A static IP was pinned on a system interface and must be undone
    /// (reset to DHCP / removed) during restore
    static_ip_configured: bool,
    /// Non-fatal problem the user should be told about (e.g. joined the
    /// network but could not configure the rendezvous IP)
    warning: Option<String>,
}

/// Per-OS command primitives driven by the shared [`AdHocNetwork`] lifecycle
///
/// Implementations live in the platform submodules; tests inject a mock.
/// `restore` must be idempotent and safe to call even when nothing was
/// disturbed (it is invoked on every failure path).
#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
trait AdHocBackend: Send {
    /// Best-effort SSID of the currently associated network
    ///
    /// Returns `Ok(None)` when not associated or when the name cannot be
    /// determined (e.g. macOS redacts it without location permission).
    fn current_network(&mut self) -> Result<Option<String>>;

    /// Create and host the ad-hoc network
    fn create_network(&mut self, ssid: &str) -> Result<AdHocOutcome>;

    /// Join an existing ad-hoc network
    fn join_network(&mut self, ssid: &str) -> Result<AdHocOutcome>;

    /// Tear down anything created/joined, rejoin `previous_network` when
    /// known, and undo static IP configuration when `static_ip_configured`
    fn restore(&mut self, previous_network: Option<&str>, static_ip_configured: bool)
        -> Result<()>;
}

/// Sanitize a device name into the SSID suffix
///
/// Reuses the crate-wide device-name policy ([`crate::sanitize_device_name`])
/// and caps the suffix at 20 characters so the full SSID stays well within
/// the 32-byte SSID limit.
#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
fn sanitize_ssid_component(device_name: &str) -> String {
    let mut sanitized = crate::device_name::sanitize_device_name(device_name);
    sanitized.truncate(20);
    while sanitized.ends_with('-') {
        sanitized.pop();
    }
    sanitized
}

/// Generate the default hosted-network password (Windows requires 8+ chars)
#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
fn generate_password(sanitized_name: &str) -> String {
    format!(
        "connecto{}",
        sanitized_name.chars().take(4).collect::<String>()
    )
}

/// Derive the WPA2 key protecting a Windows-hosted ad-hoc network from its
/// SSID
///
/// Windows hosted networks require a key (macOS/Linux create open
/// networks). The SSID is `ADHOC_NETWORK_PREFIX` + the sanitized device
/// name and the host derives its key from that same sanitized name, so any
/// joiner can recompute the key from the SSID alone.
#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
fn derive_join_password(ssid: &str) -> String {
    generate_password(ssid.strip_prefix(ADHOC_NETWORK_PREFIX).unwrap_or(ssid))
}

/// Parse a macOS product version like `"14.4.1"` into `(major, minor)`
#[cfg(any(target_os = "macos", test))]
fn parse_macos_version(version: &str) -> Option<(u32, u32)> {
    let mut parts = version.trim().split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = match parts.next() {
        Some(minor) => minor.parse().ok()?,
        None => 0,
    };
    Some((major, minor))
}

/// Whether this macOS version no longer supports command-line IBSS creation
///
/// Apple turned the private `airport` utility into a no-op stub in macOS
/// 14.4: it exits 0 for any arguments without creating anything, so ad-hoc
/// creation must fail fast instead of falsely reporting success.
/// Unparseable versions are conservatively treated as modern.
#[cfg(any(target_os = "macos", test))]
fn macos_ibss_unsupported(version: &str) -> bool {
    match parse_macos_version(version) {
        Some((major, minor)) => (major, minor) >= (14, 4),
        None => true,
    }
}

/// Cross-platform ad-hoc WiFi network manager
///
/// Tracks the pre-existing network state before any mutation and guarantees
/// restoration on explicit [`cleanup`](Self::cleanup), on create/join
/// failure, and on drop.
#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
pub struct AdHocNetwork {
    network_name: String,
    is_hosting: bool,
    joined: bool,
    previous_network: Option<String>,
    /// Set whenever a backend pinned a static IP on a system interface;
    /// restore resets DHCP based on this flag, independent of whether the
    /// previous network name could be captured.
    static_ip_configured: bool,
    #[cfg(target_os = "windows")]
    password: String,
    backend: Box<dyn AdHocBackend>,
}

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
impl AdHocNetwork {
    /// Create a new ad-hoc network manager
    pub fn new(device_name: &str) -> Self {
        #[cfg(target_os = "macos")]
        let backend: Box<dyn AdHocBackend> = Box::new(macos::MacosBackend::new());
        #[cfg(target_os = "linux")]
        let backend: Box<dyn AdHocBackend> = Box::new(linux::LinuxBackend::new());
        #[cfg(target_os = "windows")]
        let backend: Box<dyn AdHocBackend> = Box::new(windows::WindowsBackend::new(
            generate_password(&sanitize_ssid_component(device_name)),
        ));

        Self::with_backend(device_name, backend)
    }

    /// Construct with an explicit backend (test seam)
    fn with_backend(device_name: &str, backend: Box<dyn AdHocBackend>) -> Self {
        let sanitized = sanitize_ssid_component(device_name);

        Self {
            network_name: format!("{}{}", ADHOC_NETWORK_PREFIX, sanitized),
            is_hosting: false,
            joined: false,
            previous_network: None,
            static_ip_configured: false,
            #[cfg(target_os = "windows")]
            password: generate_password(&sanitized),
            backend,
        }
    }

    /// Get the network name
    pub fn network_name(&self) -> &str {
        &self.network_name
    }

    /// Get the password for the hosted network (Windows hosted networks
    /// require one; other platforms create open networks)
    #[cfg(target_os = "windows")]
    pub fn password(&self) -> &str {
        &self.password
    }

    /// Create and host an ad-hoc network
    ///
    /// On failure the previous network state is restored before returning.
    pub fn create_network(&mut self) -> Result<String> {
        info!("Creating ad-hoc network: {}", self.network_name);

        self.save_current_network();

        let ssid = self.network_name.clone();
        match self.backend.create_network(&ssid) {
            Ok(outcome) => {
                self.is_hosting = true;
                self.apply_outcome(outcome);
                info!("Ad-hoc network '{}' created", self.network_name);
                Ok(self.network_name.clone())
            }
            Err(e) => {
                self.restore_after_failure("create");
                Err(e)
            }
        }
    }

    /// Join an existing connecto ad-hoc network
    ///
    /// On failure the previous network state is restored before returning.
    pub fn join_network(&mut self, network_name: &str) -> Result<()> {
        info!("Joining ad-hoc network: {}", network_name);

        self.save_current_network();

        match self.backend.join_network(network_name) {
            Ok(outcome) => {
                self.network_name = network_name.to_string();
                self.joined = true;
                self.apply_outcome(outcome);
                info!("Successfully joined network: {}", network_name);
                Ok(())
            }
            Err(e) => {
                self.restore_after_failure("join");
                Err(e)
            }
        }
    }

    /// Scan for connecto ad-hoc networks
    pub fn scan_for_networks() -> Result<Vec<String>> {
        #[cfg(target_os = "macos")]
        return macos::scan_for_networks();
        #[cfg(target_os = "linux")]
        return linux::scan_for_networks();
        #[cfg(target_os = "windows")]
        return windows::scan_for_networks();
    }

    /// Tear down the ad-hoc network and restore the previous network state
    ///
    /// Resets DHCP whenever a static IP was pinned, even when the previous
    /// network name could not be captured. Idempotent; the `Drop` impl is
    /// only a backstop for paths that never reach an explicit cleanup.
    pub fn cleanup(&mut self) -> Result<()> {
        let result = self
            .backend
            .restore(self.previous_network.as_deref(), self.static_ip_configured);
        self.is_hosting = false;
        self.joined = false;
        self.static_ip_configured = false;
        result
    }

    /// Whether any system state was changed that `cleanup` must undo
    fn needs_restore(&self) -> bool {
        self.is_hosting || self.joined || self.static_ip_configured
    }

    fn save_current_network(&mut self) {
        match self.backend.current_network() {
            Ok(current) => {
                self.previous_network = current;
                debug!("Saved current network: {:?}", self.previous_network);
            }
            Err(e) => {
                warn!(
                    "Could not record the current WiFi network (it will not be auto-restored): {}",
                    e
                );
            }
        }
    }

    fn apply_outcome(&mut self, outcome: AdHocOutcome) {
        self.static_ip_configured |= outcome.static_ip_configured;
        if let Some(warning) = outcome.warning {
            warn!("{}", warning);
        }
    }

    fn restore_after_failure(&mut self, action: &str) {
        if let Err(e) = self.cleanup() {
            warn!(
                "Failed to restore network state after ad-hoc {} failure: {}",
                action, e
            );
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
impl Drop for AdHocNetwork {
    fn drop(&mut self) {
        if self.needs_restore() {
            if let Err(e) = self.cleanup() {
                warn!("Failed to restore network state on drop: {}", e);
            }
        }
    }
}

/// Run a blocking ad-hoc operation off the async runtime
#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
async fn run_blocking<T, F>(f: F) -> Result<T>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| ConnectoError::Network(format!("Ad-hoc network task failed: {}", e)))
}

/// Fallback connection handler
pub struct FallbackHandler {
    #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
    adhoc: Option<AdHocNetwork>,
    #[cfg(feature = "bluetooth")]
    bluetooth_browser: Option<crate::bluetooth::BluetoothBrowser>,
    #[cfg(all(feature = "bluetooth", target_os = "linux"))]
    bluetooth_advertiser: Option<crate::bluetooth::BluetoothAdvertiser>,
    #[allow(dead_code)]
    timeout: Duration,
}

impl FallbackHandler {
    /// Create a new fallback handler
    pub fn new(
        #[cfg_attr(
            not(any(target_os = "macos", target_os = "linux", target_os = "windows")),
            allow(unused_variables)
        )]
        device_name: &str,
        timeout: Duration,
    ) -> Self {
        Self {
            #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
            adhoc: Some(AdHocNetwork::new(device_name)),
            #[cfg(feature = "bluetooth")]
            bluetooth_browser: None,
            #[cfg(all(feature = "bluetooth", target_os = "linux"))]
            bluetooth_advertiser: None,
            timeout,
        }
    }

    /// Try to establish connectivity using fallback methods
    /// Returns the IP address to use for connection if successful
    #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
    pub async fn establish_fallback_connection(
        &mut self,
        is_listener: bool,
    ) -> Result<Option<String>> {
        if is_listener {
            // Listener: Create an ad-hoc network
            let Some(adhoc) = self.adhoc.take() else {
                return Ok(None);
            };
            let (adhoc, result) = run_blocking(move || {
                let mut adhoc = adhoc;
                let result = adhoc.create_network();
                (adhoc, result)
            })
            .await?;
            self.adhoc = Some(adhoc);

            match result {
                Ok(network_name) => {
                    info!("Created fallback network: {}", network_name);
                    // Wait a moment for network to stabilize
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    Ok(Some(ADHOC_HOST_IP.to_string()))
                }
                Err(e) => {
                    warn!("Failed to create ad-hoc network: {}", e);
                    Err(e)
                }
            }
        } else {
            // Scanner: Look for and join connecto ad-hoc networks
            let networks = run_blocking(AdHocNetwork::scan_for_networks).await??;

            let Some(network) = networks.first().cloned() else {
                info!("No connecto ad-hoc networks found");
                return Ok(None);
            };
            let Some(adhoc) = self.adhoc.take() else {
                return Ok(None);
            };
            let (adhoc, result) = run_blocking(move || {
                let mut adhoc = adhoc;
                let result = adhoc.join_network(&network);
                (adhoc, result)
            })
            .await?;
            self.adhoc = Some(adhoc);
            result?;

            // Wait for connection to establish
            tokio::time::sleep(Duration::from_secs(2)).await;
            // Return host IP to connect to
            Ok(Some(ADHOC_HOST_IP.to_string()))
        }
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    pub async fn establish_fallback_connection(
        &mut self,
        _is_listener: bool,
    ) -> Result<Option<String>> {
        // Ad-hoc networking not implemented for this platform
        Ok(None)
    }

    /// Clean up fallback connections, restoring the previous network state
    ///
    /// Blocking; async callers should wrap this in `spawn_blocking`.
    #[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
    pub fn cleanup(&mut self) {
        if let Some(ref mut adhoc) = self.adhoc {
            if adhoc.needs_restore() {
                if let Err(e) = adhoc.cleanup() {
                    warn!("Failed to restore previous network state: {}", e);
                }
            }
        }
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    pub fn cleanup(&mut self) {
        // Nothing to clean up on this platform
    }

    // ========================================================================
    // Bluetooth Methods
    // ========================================================================

    /// Scan for Connecto devices via Bluetooth Low Energy
    ///
    /// Returns discovered devices that advertise the Connecto BLE service.
    /// Works on macOS, Linux, and Windows via btleplug.
    #[cfg(feature = "bluetooth")]
    pub async fn scan_bluetooth(
        &mut self,
        duration: Duration,
    ) -> Result<Vec<crate::bluetooth::BluetoothDevice>> {
        use tracing::info;

        // Initialize browser if not already done
        if self.bluetooth_browser.is_none() {
            match crate::bluetooth::BluetoothBrowser::new().await {
                Ok(browser) => {
                    self.bluetooth_browser = Some(browser);
                }
                Err(e) => {
                    return Err(e);
                }
            }
        }

        if let Some(ref browser) = self.bluetooth_browser {
            info!("Starting Bluetooth scan for {} seconds", duration.as_secs());
            browser.scan(duration).await
        } else {
            Ok(vec![])
        }
    }

    /// Scan for Connecto devices via Bluetooth Low Energy (stub for non-bluetooth builds)
    #[cfg(not(feature = "bluetooth"))]
    pub async fn scan_bluetooth(&mut self, _duration: Duration) -> Result<Vec<()>> {
        Ok(vec![])
    }

    /// Start advertising this device via Bluetooth Low Energy
    ///
    /// Only supported on Linux. Returns an error on other platforms.
    #[cfg(all(feature = "bluetooth", target_os = "linux"))]
    pub async fn start_bluetooth_advertising(
        &mut self,
        name: &str,
        ip: std::net::IpAddr,
        port: u16,
    ) -> Result<()> {
        use tracing::info;

        // Initialize advertiser if not already done
        if self.bluetooth_advertiser.is_none() {
            match crate::bluetooth::BluetoothAdvertiser::new().await {
                Ok(advertiser) => {
                    self.bluetooth_advertiser = Some(advertiser);
                }
                Err(e) => {
                    return Err(e);
                }
            }
        }

        if let Some(ref mut advertiser) = self.bluetooth_advertiser {
            info!(
                "Starting Bluetooth advertising: {} at {}:{}",
                name, ip, port
            );
            advertiser.advertise(name, ip, port).await
        } else {
            Err(crate::error::ConnectoError::Bluetooth(
                "Bluetooth advertiser not initialized".to_string(),
            ))
        }
    }

    /// Start advertising this device via Bluetooth Low Energy (non-Linux stub)
    #[cfg(all(feature = "bluetooth", not(target_os = "linux")))]
    pub async fn start_bluetooth_advertising(
        &mut self,
        _name: &str,
        _ip: std::net::IpAddr,
        _port: u16,
    ) -> Result<()> {
        Err(crate::error::ConnectoError::Bluetooth(
            "Bluetooth advertising not supported on this platform. \
             Use 'connecto listen' on a Linux device for BLE advertising."
                .to_string(),
        ))
    }

    /// Start advertising (stub for non-bluetooth builds)
    #[cfg(not(feature = "bluetooth"))]
    pub async fn start_bluetooth_advertising(
        &mut self,
        _name: &str,
        _ip: std::net::IpAddr,
        _port: u16,
    ) -> Result<()> {
        Err(crate::error::ConnectoError::Bluetooth(
            "Bluetooth feature not enabled. Rebuild with --features bluetooth".to_string(),
        ))
    }

    /// Stop Bluetooth advertising
    #[cfg(all(feature = "bluetooth", target_os = "linux"))]
    pub async fn stop_bluetooth_advertising(&mut self) -> Result<()> {
        if let Some(ref mut advertiser) = self.bluetooth_advertiser {
            advertiser.stop().await
        } else {
            Ok(())
        }
    }

    /// Stop Bluetooth advertising (non-Linux stub)
    #[cfg(all(feature = "bluetooth", not(target_os = "linux")))]
    pub async fn stop_bluetooth_advertising(&mut self) -> Result<()> {
        Ok(())
    }

    /// Stop Bluetooth advertising (non-bluetooth stub)
    #[cfg(not(feature = "bluetooth"))]
    pub async fn stop_bluetooth_advertising(&mut self) -> Result<()> {
        Ok(())
    }

    /// Check if Bluetooth is available on this system
    #[cfg(feature = "bluetooth")]
    pub async fn is_bluetooth_available(&self) -> bool {
        crate::bluetooth::BluetoothBrowser::new().await.is_ok()
    }

    /// Check if Bluetooth is available (stub for non-bluetooth builds)
    #[cfg(not(feature = "bluetooth"))]
    pub async fn is_bluetooth_available(&self) -> bool {
        false
    }
}

#[cfg(test)]
#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Debug, Clone, PartialEq)]
    enum Call {
        CurrentNetwork,
        Create(String),
        Join(String),
        Restore {
            previous: Option<String>,
            static_ip_configured: bool,
        },
    }

    /// Scripted backend that records every primitive invocation
    struct MockBackend {
        calls: Arc<Mutex<Vec<Call>>>,
        current: Option<String>,
        /// `Some(outcome)` => succeed once with it; `None` => fail
        create_result: Option<AdHocOutcome>,
        join_result: Option<AdHocOutcome>,
    }

    impl AdHocBackend for MockBackend {
        fn current_network(&mut self) -> Result<Option<String>> {
            self.calls.lock().unwrap().push(Call::CurrentNetwork);
            Ok(self.current.clone())
        }

        fn create_network(&mut self, ssid: &str) -> Result<AdHocOutcome> {
            self.calls
                .lock()
                .unwrap()
                .push(Call::Create(ssid.to_string()));
            self.create_result
                .take()
                .ok_or_else(|| ConnectoError::Network("mock create failure".to_string()))
        }

        fn join_network(&mut self, ssid: &str) -> Result<AdHocOutcome> {
            self.calls
                .lock()
                .unwrap()
                .push(Call::Join(ssid.to_string()));
            self.join_result
                .take()
                .ok_or_else(|| ConnectoError::Network("mock join failure".to_string()))
        }

        fn restore(
            &mut self,
            previous_network: Option<&str>,
            static_ip_configured: bool,
        ) -> Result<()> {
            self.calls.lock().unwrap().push(Call::Restore {
                previous: previous_network.map(String::from),
                static_ip_configured,
            });
            Ok(())
        }
    }

    fn mocked_network(
        current: Option<&str>,
        create_result: Option<AdHocOutcome>,
        join_result: Option<AdHocOutcome>,
    ) -> (AdHocNetwork, Arc<Mutex<Vec<Call>>>) {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let backend = MockBackend {
            calls: Arc::clone(&calls),
            current: current.map(String::from),
            create_result,
            join_result,
        };
        (
            AdHocNetwork::with_backend("Test Device", Box::new(backend)),
            calls,
        )
    }

    fn restore_calls(calls: &Arc<Mutex<Vec<Call>>>) -> Vec<Call> {
        calls
            .lock()
            .unwrap()
            .iter()
            .filter(|c| matches!(c, Call::Restore { .. }))
            .cloned()
            .collect()
    }

    #[test]
    fn test_create_failure_restores_eagerly() {
        let (mut network, calls) = mocked_network(Some("HomeWiFi"), None, None);

        assert!(network.create_network().is_err());

        // Restore ran inside the failure path, before the caller sees the Err
        assert_eq!(
            restore_calls(&calls),
            vec![Call::Restore {
                previous: Some("HomeWiFi".to_string()),
                static_ip_configured: false,
            }]
        );

        // The failure path reset the flags, so Drop must not restore again
        drop(network);
        assert_eq!(restore_calls(&calls).len(), 1);
    }

    #[test]
    fn test_join_failure_restores_eagerly() {
        let (mut network, calls) = mocked_network(Some("HomeWiFi"), None, None);

        assert!(network.join_network("Connecto-other").is_err());
        assert_eq!(
            restore_calls(&calls),
            vec![Call::Restore {
                previous: Some("HomeWiFi".to_string()),
                static_ip_configured: false,
            }]
        );
    }

    #[test]
    fn test_cleanup_resets_dhcp_even_without_previous_network() {
        // previous_network is None (e.g. modern macOS where the SSID is
        // unreadable) but a static IP was pinned: restore must still run
        // with static_ip_configured=true so DHCP gets reset.
        let (mut network, calls) = mocked_network(
            None,
            Some(AdHocOutcome {
                static_ip_configured: true,
                warning: None,
            }),
            None,
        );

        network.create_network().unwrap();
        network.cleanup().unwrap();

        assert_eq!(
            restore_calls(&calls),
            vec![Call::Restore {
                previous: None,
                static_ip_configured: true,
            }]
        );

        // Explicit cleanup cleared the flags; Drop is a no-op afterwards
        drop(network);
        assert_eq!(restore_calls(&calls).len(), 1);
    }

    #[test]
    fn test_drop_restores_after_successful_create() {
        let (mut network, calls) = mocked_network(
            Some("HomeWiFi"),
            Some(AdHocOutcome {
                static_ip_configured: true,
                warning: None,
            }),
            None,
        );

        network.create_network().unwrap();
        drop(network);

        assert_eq!(
            restore_calls(&calls),
            vec![Call::Restore {
                previous: Some("HomeWiFi".to_string()),
                static_ip_configured: true,
            }]
        );
    }

    #[test]
    fn test_drop_restores_after_successful_join() {
        let (mut network, calls) = mocked_network(
            Some("HomeWiFi"),
            None,
            Some(AdHocOutcome {
                static_ip_configured: true,
                warning: None,
            }),
        );

        network.join_network("Connecto-host").unwrap();
        assert_eq!(network.network_name(), "Connecto-host");
        drop(network);

        assert_eq!(
            restore_calls(&calls),
            vec![Call::Restore {
                previous: Some("HomeWiFi".to_string()),
                static_ip_configured: true,
            }]
        );
    }

    #[test]
    fn test_drop_without_any_action_does_not_restore() {
        let (network, calls) = mocked_network(Some("HomeWiFi"), None, None);
        drop(network);
        assert!(restore_calls(&calls).is_empty());
    }

    #[test]
    fn test_network_name_sanitization() {
        let adhoc = AdHocNetwork::new("My Device!@#$%");
        assert!(adhoc.network_name().starts_with(ADHOC_NETWORK_PREFIX));
        assert_eq!(adhoc.network_name(), "Connecto-my-device");
    }

    #[test]
    fn test_network_name_empty_input_falls_back() {
        let adhoc = AdHocNetwork::new("!!!");
        assert_eq!(adhoc.network_name(), "Connecto-device");
    }

    #[test]
    fn test_network_name_length() {
        let adhoc = AdHocNetwork::new("This is a very long device name that should be truncated");
        // ADHOC_NETWORK_PREFIX (9 chars) + max 20 chars = 29 max
        assert!(adhoc.network_name().len() <= 29);
        assert!(!adhoc.network_name().ends_with('-'));
    }

    #[test]
    fn test_generate_password_minimum_length() {
        // Windows hosted networks require 8+ characters even for empty names
        assert!(generate_password("").len() >= 8);
        assert_eq!(generate_password("abcdef"), "connectoabcd");
    }

    #[test]
    fn test_derive_join_password_matches_host_password() {
        // The joining side must recompute exactly the key a Windows host
        // generated from its sanitized device name
        let ssid = format!("{}my-device", ADHOC_NETWORK_PREFIX);
        assert_eq!(derive_join_password(&ssid), generate_password("my-device"));
        assert_eq!(derive_join_password(&ssid), "connectomy-d");
        // Non-connecto SSIDs still produce a deterministic key
        assert_eq!(derive_join_password("Other"), "connectoOthe");
    }

    #[test]
    fn test_parse_macos_version() {
        assert_eq!(parse_macos_version("14.4"), Some((14, 4)));
        assert_eq!(parse_macos_version("14.4.1"), Some((14, 4)));
        assert_eq!(parse_macos_version("15.7.7"), Some((15, 7)));
        assert_eq!(parse_macos_version("14"), Some((14, 0)));
        assert_eq!(parse_macos_version(" 13.6 "), Some((13, 6)));
        assert_eq!(parse_macos_version("garbage"), None);
        assert_eq!(parse_macos_version("14.x"), None);
        assert_eq!(parse_macos_version(""), None);
    }

    #[test]
    fn test_macos_version_gate() {
        // >= 14.4 cannot create IBSS networks
        assert!(macos_ibss_unsupported("14.4"));
        assert!(macos_ibss_unsupported("14.5"));
        assert!(macos_ibss_unsupported("15.0"));
        assert!(macos_ibss_unsupported("15.7.7"));
        // older versions still can
        assert!(!macos_ibss_unsupported("14.3.1"));
        assert!(!macos_ibss_unsupported("13.6"));
        assert!(!macos_ibss_unsupported("14"));
        // unparseable input is conservatively treated as modern (fail fast)
        assert!(macos_ibss_unsupported("garbage"));
        assert!(macos_ibss_unsupported(""));
    }
}
