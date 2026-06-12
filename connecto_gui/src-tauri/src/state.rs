//! Application state management

use connecto_core::discovery::{DiscoveredDevice, ServiceAdvertiser};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::task::{AbortHandle, JoinHandle};

/// Sync operation status
#[derive(Debug, Clone, Default)]
pub struct SyncStatus {
    pub is_syncing: bool,
    pub status_message: String,
    /// Machine-readable phase: starting / started / searching / peer-found /
    /// connected / key-received / key-accepted / completed / failed / cancelled
    pub phase: String,
    pub peer_name: Option<String>,
}

/// Everything owned by a running listener
///
/// Dropping the advertiser unregisters the mDNS service and shuts its daemon
/// down; aborting the tasks closes the handshake socket.
pub struct ListenerHandle {
    /// mDNS advertiser kept alive for the lifetime of the listener
    pub advertiser: ServiceAdvertiser,
    /// Task driving `HandshakeServer::run`
    pub server_task: JoinHandle<()>,
    /// Task consuming `ServerEvent`s into `listener_activity`
    pub event_task: JoinHandle<()>,
    pub port: u16,
    pub device_name: String,
}

/// Global application state
pub struct AppState {
    /// Currently discovered devices
    pub discovered_devices: Mutex<Vec<DiscoveredDevice>>,
    /// The running listener, if any
    pub listener: Mutex<Option<ListenerHandle>>,
    /// Latest listener event message, written by the listener's event task
    /// (shared via `Arc` so the spawned task can outlive a command invocation)
    pub listener_activity: Arc<Mutex<Option<String>>>,
    /// Sync operation status, updated live by the sync event task
    pub sync_status: Arc<Mutex<SyncStatus>>,
    /// Abort handle for the in-flight sync task; `Some` while a sync runs
    pub sync_task: Mutex<Option<AbortHandle>>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            discovered_devices: Mutex::new(Vec::new()),
            listener: Mutex::new(None),
            listener_activity: Arc::new(Mutex::new(None)),
            sync_status: Arc::new(Mutex::new(SyncStatus::default())),
            sync_task: Mutex::new(None),
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
        assert!(state.listener.lock().await.is_none());
        assert!(state.listener_activity.lock().await.is_none());
        assert!(state.sync_task.lock().await.is_none());
        assert!(!state.sync_status.lock().await.is_syncing);
    }

    #[tokio::test]
    async fn test_app_state_default() {
        let state = AppState::default();
        assert!(state.listener.lock().await.is_none());
        assert!(!state.sync_status.lock().await.is_syncing);
    }
}
