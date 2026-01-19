//! Configuration management for Connecto CLI

use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// Connecto CLI configuration
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Config {
    /// Additional subnets to scan (in CIDR notation)
    #[serde(default)]
    pub subnets: Vec<String>,
}

impl Config {
    /// Get the config file path
    pub fn path() -> Result<PathBuf> {
        let proj_dirs = ProjectDirs::from("com", "connecto", "connecto")
            .context("Could not determine config directory")?;

        Ok(proj_dirs.config_dir().join("config.json"))
    }

    /// Load config from file, or return default if not exists
    pub fn load() -> Result<Self> {
        let path = Self::path()?;

        if !path.exists() {
            return Ok(Self::default());
        }

        let content = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read config from {}", path.display()))?;

        let config: Config = serde_json::from_str(&content)
            .with_context(|| "Failed to parse config file")?;

        Ok(config)
    }

    /// Save config to file
    pub fn save(&self) -> Result<()> {
        let path = Self::path()?;

        // Create parent directory if needed
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create config directory {}", parent.display()))?;
        }

        let content = serde_json::to_string_pretty(self)
            .context("Failed to serialize config")?;

        fs::write(&path, content)
            .with_context(|| format!("Failed to write config to {}", path.display()))?;

        Ok(())
    }

    /// Add a subnet to the config
    pub fn add_subnet(&mut self, subnet: &str) -> bool {
        let subnet = subnet.to_string();
        if self.subnets.contains(&subnet) {
            return false;
        }
        self.subnets.push(subnet);
        true
    }

    /// Remove a subnet from the config
    pub fn remove_subnet(&mut self, subnet: &str) -> bool {
        let len_before = self.subnets.len();
        self.subnets.retain(|s| s != subnet);
        self.subnets.len() < len_before
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert!(config.subnets.is_empty());
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
    fn test_serialization() {
        let mut config = Config::default();
        config.add_subnet("10.0.0.0/24");
        config.add_subnet("192.168.1.0/24");

        let json = serde_json::to_string(&config).unwrap();
        let loaded: Config = serde_json::from_str(&json).unwrap();

        assert_eq!(loaded.subnets, config.subnets);
    }
}
