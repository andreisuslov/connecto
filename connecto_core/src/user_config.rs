//! Persistent user configuration
//!
//! Stores user-level settings shared by the Connecto CLI and GUI: extra
//! subnets to scan and an optional default SSH key. The on-disk location
//! (`directories::ProjectDirs::from("com", "connecto", "connecto")`,
//! `config.json`) and JSON field names are identical to the original CLI
//! implementation, so existing user configs keep working unchanged.

use crate::error::{ConnectoError, Result};
use crate::fsutil;
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// Connecto user configuration
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    /// Additional subnets to scan (in CIDR notation)
    #[serde(default)]
    pub subnets: Vec<String>,

    /// Default SSH key to use for pairing (path to private key)
    #[serde(default)]
    pub default_key: Option<String>,
}

impl Config {
    /// Get the config file path
    pub fn path() -> Result<PathBuf> {
        let proj_dirs = ProjectDirs::from("com", "connecto", "connecto").ok_or_else(|| {
            ConnectoError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Could not determine config directory",
            ))
        })?;

        Ok(proj_dirs.config_dir().join("config.json"))
    }

    /// Load config from file, or return default if not exists
    pub fn load() -> Result<Self> {
        Self::load_from(&Self::path()?)
    }

    /// Save config to file
    pub fn save(&self) -> Result<()> {
        self.save_to(&Self::path()?)
    }

    /// Add a subnet to the config
    ///
    /// Returns `false` if the subnet was already present.
    pub fn add_subnet(&mut self, subnet: &str) -> bool {
        let subnet = subnet.to_string();
        if self.subnets.contains(&subnet) {
            return false;
        }
        self.subnets.push(subnet);
        true
    }

    /// Remove a subnet from the config
    ///
    /// Returns `false` if the subnet was not present.
    pub fn remove_subnet(&mut self, subnet: &str) -> bool {
        let len_before = self.subnets.len();
        self.subnets.retain(|s| s != subnet);
        self.subnets.len() < len_before
    }

    /// Set the default SSH key
    pub fn set_default_key(&mut self, key_path: &str) {
        self.default_key = Some(key_path.to_string());
    }

    /// Clear the default SSH key
    pub fn clear_default_key(&mut self) {
        self.default_key = None;
    }

    /// Load config from an explicit path (used by tests)
    fn load_from(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }

        let content = fs::read_to_string(path)?;
        Ok(serde_json::from_str(&content)?)
    }

    /// Save config to an explicit path (used by tests)
    fn save_to(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let content = serde_json::to_string_pretty(self)?;
        fsutil::write_atomic(path, &content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert!(config.subnets.is_empty());
        assert!(config.default_key.is_none());
    }

    #[test]
    fn test_add_subnet() {
        let mut config = Config::default();
        assert!(config.add_subnet("10.0.0.0/24"));
        assert!(!config.add_subnet("10.0.0.0/24")); // duplicate
        assert_eq!(config.subnets.len(), 1);
    }

    #[test]
    fn test_remove_subnet() {
        let mut config = Config::default();
        config.add_subnet("10.0.0.0/24");
        assert!(config.remove_subnet("10.0.0.0/24"));
        assert!(!config.remove_subnet("10.0.0.0/24")); // already removed
        assert!(config.subnets.is_empty());
    }

    #[test]
    fn test_default_key() {
        let mut config = Config::default();
        config.set_default_key("/home/user/.ssh/id_ed25519");
        assert_eq!(
            config.default_key.as_deref(),
            Some("/home/user/.ssh/id_ed25519")
        );
        config.clear_default_key();
        assert!(config.default_key.is_none());
    }

    #[test]
    fn test_serialization() {
        let mut config = Config::default();
        config.add_subnet("10.0.0.0/24");
        config.add_subnet("192.168.1.0/24");

        let json = serde_json::to_string(&config).unwrap();
        let loaded: Config = serde_json::from_str(&json).unwrap();

        assert_eq!(loaded, config);
    }

    #[test]
    fn test_on_disk_format_matches_cli() {
        // The JSON field names must stay `subnets` and `default_key` so
        // configs written by the original CLI keep loading.
        let config = Config {
            subnets: vec!["10.0.0.0/24".to_string()],
            default_key: Some("/k/id".to_string()),
        };
        let json = serde_json::to_string_pretty(&config).unwrap();
        assert!(json.contains("\"subnets\""));
        assert!(json.contains("\"default_key\""));

        let legacy = r#"{"subnets":["192.168.0.0/24"],"default_key":null}"#;
        let loaded: Config = serde_json::from_str(legacy).unwrap();
        assert_eq!(loaded.subnets, vec!["192.168.0.0/24".to_string()]);
        assert!(loaded.default_key.is_none());
    }

    #[test]
    fn test_save_and_load_round_trip() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("nested").join("config.json");

        let mut config = Config::default();
        config.add_subnet("10.0.0.0/24");
        config.set_default_key("/k/id");

        config.save_to(&path).unwrap();
        let loaded = Config::load_from(&path).unwrap();
        assert_eq!(loaded, config);
    }

    #[test]
    fn test_load_missing_file_returns_default() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("config.json");

        let loaded = Config::load_from(&path).unwrap();
        assert_eq!(loaded, Config::default());
    }

    #[test]
    fn test_missing_fields_default() {
        let loaded: Config = serde_json::from_str("{}").unwrap();
        assert!(loaded.subnets.is_empty());
        assert!(loaded.default_key.is_none());
    }
}
