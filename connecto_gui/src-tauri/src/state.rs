//! Application state management

use connecto_core::discovery::{DiscoveredDevice, ServiceAdvertiser};
use tokio::sync::Mutex;

/// Global application state
pub struct AppState {
    /// Currently discovered devices
    pub discovered_devices: Mutex<Vec<DiscoveredDevice>>,
    /// mDNS service advertiser
    pub advertiser: Mutex<Option<ServiceAdvertiser>>,
    /// Whether the listener is active
    pub is_listening: Mutex<bool>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            discovered_devices: Mutex::new(Vec::new()),
            advertiser: Mutex::new(None),
            is_listening: Mutex::new(false),
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_app_state_creation() {
        let state = AppState::new();
        assert!(state.discovered_devices.lock().await.is_empty());
        assert!(state.advertiser.lock().await.is_none());
        assert!(!*state.is_listening.lock().await);
    }

    #[tokio::test]
    async fn test_app_state_default() {
        let state = AppState::default();
        assert!(!*state.is_listening.lock().await);
    }
}
