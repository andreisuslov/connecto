//! SSH Key management module
//!
//! Handles generation, parsing, and storage of SSH keys

use crate::error::{ConnectoError, Result};
use directories::UserDirs;
use ssh_key::{Algorithm, LineEnding, PrivateKey, PublicKey};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

/// Supported SSH key algorithms
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyAlgorithm {
    Ed25519,
    Rsa4096,
}

impl Default for KeyAlgorithm {
    fn default() -> Self {
        Self::Ed25519
    }
}

/// Represents an SSH key pair
#[derive(Debug, Clone)]
pub struct SshKeyPair {
    pub private_key: String,
    pub public_key: String,
    pub algorithm: KeyAlgorithm,
    pub comment: String,
}

impl SshKeyPair {
    /// Generate a new SSH key pair
    pub fn generate(algorithm: KeyAlgorithm, comment: &str) -> Result<Self> {
        let mut rng = rand::thread_rng();

        let private_key = match algorithm {
            KeyAlgorithm::Ed25519 => PrivateKey::random(&mut rng, Algorithm::Ed25519),
            KeyAlgorithm::Rsa4096 => PrivateKey::random(&mut rng, Algorithm::Rsa { hash: None }),
        }
        .map_err(|e| ConnectoError::KeyGeneration(e.to_string()))?;

        let private_key_str = private_key
            .to_openssh(LineEnding::LF)
            .map_err(|e| ConnectoError::KeyGeneration(e.to_string()))?
            .to_string();

        let public_key = private_key.public_key();
        let public_key_str = format!(
            "{} {}",
            public_key
                .to_openssh()
                .map_err(|e| ConnectoError::KeyGeneration(e.to_string()))?,
            comment
        );

        Ok(Self {
            private_key: private_key_str,
            public_key: public_key_str,
            algorithm,
            comment: comment.to_string(),
        })
    }

    /// Parse a public key from OpenSSH format
    pub fn parse_public_key(key_str: &str) -> Result<PublicKey> {
        // Extract just the key part (without comment)
        let parts: Vec<&str> = key_str.split_whitespace().collect();
        if parts.len() < 2 {
            return Err(ConnectoError::KeyParsing(
                "Invalid public key format".to_string(),
            ));
        }

        let key_data = format!("{} {}", parts[0], parts[1]);
        PublicKey::from_openssh(&key_data)
            .map_err(|e| ConnectoError::KeyParsing(e.to_string()))
    }
}

/// Manager for SSH key files on disk
pub struct KeyManager {
    ssh_dir: PathBuf,
}

impl KeyManager {
    /// Create a new KeyManager with the default SSH directory
    pub fn new() -> Result<Self> {
        let ssh_dir = Self::default_ssh_dir()?;
        Ok(Self { ssh_dir })
    }

    /// Create a KeyManager with a custom SSH directory (useful for testing)
    pub fn with_dir(ssh_dir: PathBuf) -> Self {
        Self { ssh_dir }
    }

    /// Get the default SSH directory path
    pub fn default_ssh_dir() -> Result<PathBuf> {
        UserDirs::new()
            .map(|dirs| dirs.home_dir().join(".ssh"))
            .ok_or_else(|| ConnectoError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Could not determine home directory",
            )))
    }

    /// Ensure the SSH directory exists with proper permissions
    pub fn ensure_ssh_dir(&self) -> Result<()> {
        if !self.ssh_dir.exists() {
            fs::create_dir_all(&self.ssh_dir)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&self.ssh_dir, fs::Permissions::from_mode(0o700))?;
            }
        }
        Ok(())
    }

    /// Save a key pair to disk
    pub fn save_key_pair(&self, key_pair: &SshKeyPair, name: &str) -> Result<(PathBuf, PathBuf)> {
        self.ensure_ssh_dir()?;

        let private_path = self.ssh_dir.join(name);
        let public_path = self.ssh_dir.join(format!("{}.pub", name));

        // Write private key
        fs::write(&private_path, &key_pair.private_key)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&private_path, fs::Permissions::from_mode(0o600))?;
        }

        // Write public key
        fs::write(&public_path, &key_pair.public_key)?;

        Ok((private_path, public_path))
    }

    /// Get the path to authorized_keys file
    pub fn authorized_keys_path(&self) -> PathBuf {
        self.ssh_dir.join("authorized_keys")
    }

    /// Add a public key to authorized_keys
    pub fn add_authorized_key(&self, public_key: &str) -> Result<()> {
        self.ensure_ssh_dir()?;

        let auth_keys_path = self.authorized_keys_path();

        // Check if key already exists
        if auth_keys_path.exists() {
            let existing = fs::read_to_string(&auth_keys_path)?;
            // Extract the key fingerprint (second part) for comparison
            let new_key_parts: Vec<&str> = public_key.split_whitespace().collect();
            if new_key_parts.len() >= 2 {
                let new_key_data = new_key_parts[1];
                if existing.contains(new_key_data) {
                    return Ok(()); // Key already authorized
                }
            }
        }

        // Append the key
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&auth_keys_path)?;

        writeln!(file, "{}", public_key)?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&auth_keys_path, fs::Permissions::from_mode(0o600))?;
        }

        Ok(())
    }

    /// Remove a public key from authorized_keys
    pub fn remove_authorized_key(&self, public_key: &str) -> Result<bool> {
        let auth_keys_path = self.authorized_keys_path();

        if !auth_keys_path.exists() {
            return Ok(false);
        }

        let content = fs::read_to_string(&auth_keys_path)?;
        let key_parts: Vec<&str> = public_key.split_whitespace().collect();

        if key_parts.len() < 2 {
            return Err(ConnectoError::KeyParsing("Invalid key format".to_string()));
        }

        let key_data = key_parts[1];
        let new_content: Vec<&str> = content
            .lines()
            .filter(|line| !line.contains(key_data))
            .collect();

        let removed = new_content.len() < content.lines().count();
        fs::write(&auth_keys_path, new_content.join("\n") + "\n")?;

        Ok(removed)
    }

    /// List all authorized keys
    pub fn list_authorized_keys(&self) -> Result<Vec<String>> {
        let auth_keys_path = self.authorized_keys_path();

        if !auth_keys_path.exists() {
            return Ok(Vec::new());
        }

        let content = fs::read_to_string(&auth_keys_path)?;
        Ok(content
            .lines()
            .filter(|line| !line.trim().is_empty() && !line.starts_with('#'))
            .map(String::from)
            .collect())
    }
}

impl Default for KeyManager {
    fn default() -> Self {
        Self::new().expect("Failed to create default KeyManager")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_key_algorithm_default() {
        let algo = KeyAlgorithm::default();
        assert_eq!(algo, KeyAlgorithm::Ed25519);
    }

    #[test]
    fn test_generate_ed25519_key() {
        let key_pair = SshKeyPair::generate(KeyAlgorithm::Ed25519, "test@connecto").unwrap();

        assert!(key_pair.private_key.contains("OPENSSH PRIVATE KEY"));
        assert!(key_pair.public_key.starts_with("ssh-ed25519 "));
        assert!(key_pair.public_key.contains("test@connecto"));
        assert_eq!(key_pair.algorithm, KeyAlgorithm::Ed25519);
    }

    #[test]
    fn test_parse_public_key() {
        let key_pair = SshKeyPair::generate(KeyAlgorithm::Ed25519, "test@connecto").unwrap();
        let parsed = SshKeyPair::parse_public_key(&key_pair.public_key);

        assert!(parsed.is_ok());
    }

    #[test]
    fn test_parse_invalid_public_key() {
        let result = SshKeyPair::parse_public_key("invalid-key");
        assert!(result.is_err());
    }

    #[test]
    fn test_key_manager_with_custom_dir() {
        let temp_dir = TempDir::new().unwrap();
        let ssh_dir = temp_dir.path().join(".ssh");

        let manager = KeyManager::with_dir(ssh_dir.clone());
        manager.ensure_ssh_dir().unwrap();

        assert!(ssh_dir.exists());
    }

    #[test]
    fn test_save_key_pair() {
        let temp_dir = TempDir::new().unwrap();
        let ssh_dir = temp_dir.path().join(".ssh");

        let manager = KeyManager::with_dir(ssh_dir.clone());
        let key_pair = SshKeyPair::generate(KeyAlgorithm::Ed25519, "test@connecto").unwrap();

        let (private_path, public_path) = manager.save_key_pair(&key_pair, "connecto_test").unwrap();

        assert!(private_path.exists());
        assert!(public_path.exists());

        let private_content = fs::read_to_string(&private_path).unwrap();
        assert!(private_content.contains("OPENSSH PRIVATE KEY"));

        let public_content = fs::read_to_string(&public_path).unwrap();
        assert!(public_content.starts_with("ssh-ed25519 "));
    }

    #[test]
    fn test_add_authorized_key() {
        let temp_dir = TempDir::new().unwrap();
        let ssh_dir = temp_dir.path().join(".ssh");

        let manager = KeyManager::with_dir(ssh_dir);
        let key_pair = SshKeyPair::generate(KeyAlgorithm::Ed25519, "test@connecto").unwrap();

        manager.add_authorized_key(&key_pair.public_key).unwrap();

        let keys = manager.list_authorized_keys().unwrap();
        assert_eq!(keys.len(), 1);
        assert!(keys[0].contains("test@connecto"));
    }

    #[test]
    fn test_add_duplicate_authorized_key() {
        let temp_dir = TempDir::new().unwrap();
        let ssh_dir = temp_dir.path().join(".ssh");

        let manager = KeyManager::with_dir(ssh_dir);
        let key_pair = SshKeyPair::generate(KeyAlgorithm::Ed25519, "test@connecto").unwrap();

        manager.add_authorized_key(&key_pair.public_key).unwrap();
        manager.add_authorized_key(&key_pair.public_key).unwrap(); // Add again

        let keys = manager.list_authorized_keys().unwrap();
        assert_eq!(keys.len(), 1); // Should still be only 1
    }

    #[test]
    fn test_remove_authorized_key() {
        let temp_dir = TempDir::new().unwrap();
        let ssh_dir = temp_dir.path().join(".ssh");

        let manager = KeyManager::with_dir(ssh_dir);
        let key_pair1 = SshKeyPair::generate(KeyAlgorithm::Ed25519, "test1@connecto").unwrap();
        let key_pair2 = SshKeyPair::generate(KeyAlgorithm::Ed25519, "test2@connecto").unwrap();

        manager.add_authorized_key(&key_pair1.public_key).unwrap();
        manager.add_authorized_key(&key_pair2.public_key).unwrap();

        let removed = manager.remove_authorized_key(&key_pair1.public_key).unwrap();
        assert!(removed);

        let keys = manager.list_authorized_keys().unwrap();
        assert_eq!(keys.len(), 1);
        assert!(keys[0].contains("test2@connecto"));
    }

    #[test]
    fn test_list_empty_authorized_keys() {
        let temp_dir = TempDir::new().unwrap();
        let ssh_dir = temp_dir.path().join(".ssh");

        let manager = KeyManager::with_dir(ssh_dir);
        manager.ensure_ssh_dir().unwrap();

        let keys = manager.list_authorized_keys().unwrap();
        assert!(keys.is_empty());
    }
}
