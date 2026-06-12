//! SSH Key management module
//!
//! Handles generation, parsing, and storage of SSH keys

use crate::error::{ConnectoError, Result};
use crate::fsutil;
use directories::UserDirs;
use ssh_key::{Algorithm, HashAlg, LineEnding, PrivateKey, PublicKey};
use std::fs;
use std::path::PathBuf;

/// Supported SSH key algorithms
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum KeyAlgorithm {
    #[default]
    Ed25519,
    Rsa4096,
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
        PublicKey::from_openssh(&key_data).map_err(|e| ConnectoError::KeyParsing(e.to_string()))
    }

    /// Load an existing SSH key pair from files
    pub fn load_from_file(private_key_path: &str) -> Result<Self> {
        // Read private key
        let private_key = fs::read_to_string(private_key_path).map_err(ConnectoError::Io)?;

        // Read public key (same path + .pub)
        let public_key_path = format!("{}.pub", private_key_path);
        let public_key = fs::read_to_string(&public_key_path)
            .map_err(ConnectoError::Io)?
            .trim()
            .to_string();

        // Parse private key to determine algorithm
        let parsed_private = PrivateKey::from_openssh(&private_key)
            .map_err(|e| ConnectoError::KeyParsing(e.to_string()))?;

        let algorithm = match parsed_private.algorithm() {
            Algorithm::Ed25519 => KeyAlgorithm::Ed25519,
            Algorithm::Rsa { .. } => KeyAlgorithm::Rsa4096,
            _ => {
                return Err(ConnectoError::KeyParsing(
                    "Unsupported key algorithm".to_string(),
                ))
            }
        };

        // Extract comment from public key
        let parts: Vec<&str> = public_key.split_whitespace().collect();
        let comment = if parts.len() > 2 {
            parts[2..].join(" ")
        } else {
            String::new()
        };

        Ok(Self {
            private_key,
            public_key,
            algorithm,
            comment,
        })
    }
}

/// Extract the base64 key blob (the second whitespace-separated field) from a
/// public key or authorized_keys line
fn key_blob(line: &str) -> Option<&str> {
    let mut fields = line.split_whitespace();
    fields.next()?;
    fields.next()
}

/// Check if the current process is running with Administrator privileges
///
/// This is the single Windows elevation check shared across the workspace
/// (key management here, `connecto ssh` in the CLI). The result is computed
/// once per process — elevation cannot change at runtime — so the PowerShell
/// probe is not re-spawned on every key operation.
#[cfg(target_os = "windows")]
pub fn is_windows_admin() -> bool {
    use std::process::Command;
    use std::sync::OnceLock;

    static IS_ADMIN: OnceLock<bool> = OnceLock::new();

    *IS_ADMIN.get_or_init(|| {
        let output = Command::new("powershell")
            .args([
                "-Command",
                "([Security.Principal.WindowsPrincipal] [Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)",
            ])
            .output();

        match output {
            Ok(out) => String::from_utf8_lossy(&out.stdout).trim() == "True",
            Err(_) => false,
        }
    })
}

/// Compute the SHA-256 fingerprint of a public key in OpenSSH format
///
/// Returns the fingerprint in the standard `SHA256:<base64>` form, matching
/// the output of `ssh-keygen -lf`. The input may include a trailing comment.
pub fn fingerprint_sha256(public_key_openssh: &str) -> Result<String> {
    let public_key = SshKeyPair::parse_public_key(public_key_openssh)?;
    Ok(public_key.fingerprint(HashAlg::Sha256).to_string())
}

/// Manager for SSH key files on disk
pub struct KeyManager {
    ssh_dir: PathBuf,
    /// Whether a custom directory was provided (used on Windows to skip admin path handling in tests)
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    use_custom_dir: bool,
}

impl KeyManager {
    /// Create a new KeyManager with the default SSH directory
    pub fn new() -> Result<Self> {
        let ssh_dir = Self::default_ssh_dir()?;
        Ok(Self {
            ssh_dir,
            use_custom_dir: false,
        })
    }

    /// Create a KeyManager with a custom SSH directory (useful for testing)
    pub fn with_dir(ssh_dir: PathBuf) -> Self {
        Self {
            ssh_dir,
            use_custom_dir: true,
        }
    }

    /// Get the default SSH directory path
    pub fn default_ssh_dir() -> Result<PathBuf> {
        UserDirs::new()
            .map(|dirs| dirs.home_dir().join(".ssh"))
            .ok_or_else(|| {
                ConnectoError::Io(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "Could not determine home directory",
                ))
            })
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
    ///
    /// Both files are written atomically; on Unix the private key is created
    /// with mode `0o600` before any content touches disk.
    pub fn save_key_pair(&self, key_pair: &SshKeyPair, name: &str) -> Result<(PathBuf, PathBuf)> {
        self.ensure_ssh_dir()?;

        let private_path = self.ssh_dir.join(name);
        let public_path = self.ssh_dir.join(format!("{}.pub", name));

        fsutil::write_atomic(&private_path, &key_pair.private_key)?;
        fsutil::write_atomic(&public_path, &key_pair.public_key)?;

        Ok((private_path, public_path))
    }

    /// Get the path to authorized_keys file
    /// On Windows, admin users require a different path (unless using a custom directory)
    pub fn authorized_keys_path(&self) -> PathBuf {
        #[cfg(target_os = "windows")]
        {
            // Only use admin path for default SSH directory, not custom dirs (e.g., in tests)
            if !self.use_custom_dir && is_windows_admin() {
                // Windows OpenSSH Server uses a special path for admin users
                PathBuf::from(r"C:\ProgramData\ssh\administrators_authorized_keys")
            } else {
                self.ssh_dir.join("authorized_keys")
            }
        }

        #[cfg(not(target_os = "windows"))]
        {
            self.ssh_dir.join("authorized_keys")
        }
    }

    /// Add a public key to authorized_keys
    ///
    /// The file is rewritten atomically (created `0o600` on Unix). Keys are
    /// deduplicated by exact key-blob comparison, so a key whose blob is a
    /// prefix of an existing one is still added.
    pub fn add_authorized_key(&self, public_key: &str) -> Result<()> {
        self.ensure_ssh_dir()?;

        let auth_keys_path = self.authorized_keys_path();

        // Ensure parent directory exists (needed for Windows admin path)
        if let Some(parent) = auth_keys_path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent)?;
            }
        }

        let existing = if auth_keys_path.exists() {
            fs::read_to_string(&auth_keys_path)?
        } else {
            String::new()
        };

        // Skip if the exact key blob is already authorized
        if let Some(new_blob) = key_blob(public_key) {
            if existing
                .lines()
                .any(|line| key_blob(line) == Some(new_blob))
            {
                return Ok(());
            }
        }

        let mut contents = existing;
        if !contents.is_empty() && !contents.ends_with('\n') {
            contents.push('\n');
        }
        contents.push_str(public_key);
        contents.push('\n');

        fsutil::write_atomic(&auth_keys_path, &contents)?;

        #[cfg(target_os = "windows")]
        {
            // Set proper ACL for Windows admin authorized_keys file
            // The file must be owned by Administrators or SYSTEM and not writable by others
            if is_windows_admin() {
                Self::set_windows_admin_key_permissions(&auth_keys_path)?;
            }
        }

        Ok(())
    }

    /// Set proper ACL permissions on Windows admin authorized_keys file
    ///
    /// Windows OpenSSH refuses an `administrators_authorized_keys` file with
    /// lax permissions, so inheritance is stripped and full control granted to
    /// Administrators and SYSTEM only. SIDs are used instead of account names
    /// so this also works on localized Windows installs
    /// (`*S-1-5-32-544` = Administrators, `*S-1-5-18` = SYSTEM).
    #[cfg(target_os = "windows")]
    fn set_windows_admin_key_permissions(path: &std::path::Path) -> Result<()> {
        use std::process::Command;

        let path_str = path.to_string_lossy().into_owned();

        // icacls works on all Windows versions (including Server 2012 R2):
        // 1. Reset and disable inheritance
        // 2. Grant Administrators full control
        // 3. Grant SYSTEM full control
        let icacls_args: [&[&str]; 3] = [
            &["/inheritance:r"],
            &["/grant", "*S-1-5-32-544:F"],
            &["/grant", "*S-1-5-18:F"],
        ];

        for args in icacls_args {
            let output = Command::new("icacls")
                .arg(&path_str)
                .args(args)
                .output()
                .map_err(|e| {
                    ConnectoError::Io(std::io::Error::new(
                        e.kind(),
                        format!("Failed to run icacls {:?} on {}: {}", args, path_str, e),
                    ))
                })?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr);
                return Err(ConnectoError::Io(std::io::Error::other(format!(
                    "icacls {:?} on {} failed: {}",
                    args,
                    path_str,
                    stderr.trim()
                ))));
            }
        }

        Ok(())
    }

    /// Remove a public key from authorized_keys
    ///
    /// Matching compares the base64 key blob (second whitespace-separated
    /// field) of each line for exact equality, so a key whose blob is a
    /// substring of another key's blob never removes the wrong line. Returns
    /// `Ok(false)` without rewriting the file when no line matched; otherwise
    /// the file is rewritten atomically.
    pub fn remove_authorized_key(&self, public_key: &str) -> Result<bool> {
        let auth_keys_path = self.authorized_keys_path();

        if !auth_keys_path.exists() {
            return Ok(false);
        }

        let target_blob = key_blob(public_key)
            .ok_or_else(|| ConnectoError::KeyParsing("Invalid key format".to_string()))?;

        let content = fs::read_to_string(&auth_keys_path)?;
        let retained: Vec<&str> = content
            .lines()
            .filter(|line| key_blob(line) != Some(target_blob))
            .collect();

        if retained.len() == content.lines().count() {
            // Nothing matched; leave the file untouched
            return Ok(false);
        }

        fsutil::write_atomic(&auth_keys_path, &(retained.join("\n") + "\n"))?;
        Ok(true)
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
    fn test_fingerprint_sha256_of_generated_key() {
        let key_pair = SshKeyPair::generate(KeyAlgorithm::Ed25519, "test@connecto").unwrap();

        let fingerprint = fingerprint_sha256(&key_pair.public_key).unwrap();
        assert!(fingerprint.starts_with("SHA256:"));

        // Stable across calls for the same key
        let again = fingerprint_sha256(&key_pair.public_key).unwrap();
        assert_eq!(fingerprint, again);
    }

    #[test]
    fn test_fingerprint_sha256_known_fixture() {
        // Fixture generated with ssh-keygen; expected value from `ssh-keygen -lf`.
        let public_key =
            "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIOe+ZEWqatUUD1EaBS1DRn/J0hKQPSiW0fLQ7a7Af96S fixture@connecto";
        let fingerprint = fingerprint_sha256(public_key).unwrap();
        assert_eq!(
            fingerprint,
            "SHA256:mTiqtUQvo0mB/zDzQbxahaBBq+KJ62pV8ECXptWqzLg"
        );
    }

    #[test]
    fn test_fingerprint_sha256_invalid_key() {
        assert!(fingerprint_sha256("not-a-key").is_err());
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

        let (private_path, public_path) =
            manager.save_key_pair(&key_pair, "connecto_test").unwrap();

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

        let removed = manager
            .remove_authorized_key(&key_pair1.public_key)
            .unwrap();
        assert!(removed);

        let keys = manager.list_authorized_keys().unwrap();
        assert_eq!(keys.len(), 1);
        assert!(keys[0].contains("test2@connecto"));
    }

    #[test]
    fn test_remove_authorized_key_requires_exact_blob_match() {
        let temp_dir = TempDir::new().unwrap();
        let ssh_dir = temp_dir.path().join(".ssh");
        let manager = KeyManager::with_dir(ssh_dir);

        // The first key's blob is a strict prefix of the second key's blob;
        // substring matching would remove both.
        let short_key = "ssh-ed25519 AAAAshort short@host";
        let long_key = "ssh-ed25519 AAAAshortANDLONGER long@host";

        manager.add_authorized_key(short_key).unwrap();
        manager.add_authorized_key(long_key).unwrap();
        assert_eq!(manager.list_authorized_keys().unwrap().len(), 2);

        let removed = manager.remove_authorized_key(short_key).unwrap();
        assert!(removed);

        let keys = manager.list_authorized_keys().unwrap();
        assert_eq!(keys.len(), 1);
        assert!(keys[0].contains("AAAAshortANDLONGER"));
    }

    #[test]
    fn test_add_authorized_key_distinguishes_blob_prefix() {
        let temp_dir = TempDir::new().unwrap();
        let ssh_dir = temp_dir.path().join(".ssh");
        let manager = KeyManager::with_dir(ssh_dir);

        // Adding a key whose blob is a prefix of an existing blob must not be
        // treated as a duplicate.
        manager
            .add_authorized_key("ssh-ed25519 AAAAshortANDLONGER long@host")
            .unwrap();
        manager
            .add_authorized_key("ssh-ed25519 AAAAshort short@host")
            .unwrap();

        assert_eq!(manager.list_authorized_keys().unwrap().len(), 2);
    }

    #[test]
    fn test_remove_authorized_key_no_match_leaves_file_untouched() {
        let temp_dir = TempDir::new().unwrap();
        let ssh_dir = temp_dir.path().join(".ssh");
        let manager = KeyManager::with_dir(ssh_dir);

        let key_pair = SshKeyPair::generate(KeyAlgorithm::Ed25519, "kept@connecto").unwrap();
        let other = SshKeyPair::generate(KeyAlgorithm::Ed25519, "absent@connecto").unwrap();

        manager.add_authorized_key(&key_pair.public_key).unwrap();

        let path = manager.authorized_keys_path();
        let content_before = fs::read_to_string(&path).unwrap();
        let mtime_before = fs::metadata(&path).unwrap().modified().unwrap();

        let removed = manager.remove_authorized_key(&other.public_key).unwrap();
        assert!(!removed);

        // The file must not have been rewritten at all
        assert_eq!(fs::read_to_string(&path).unwrap(), content_before);
        assert_eq!(
            fs::metadata(&path).unwrap().modified().unwrap(),
            mtime_before
        );
    }

    #[test]
    fn test_remove_authorized_key_preserves_other_lines() {
        let temp_dir = TempDir::new().unwrap();
        let ssh_dir = temp_dir.path().join(".ssh");
        let manager = KeyManager::with_dir(ssh_dir);

        let first = SshKeyPair::generate(KeyAlgorithm::Ed25519, "first@connecto").unwrap();
        let second = SshKeyPair::generate(KeyAlgorithm::Ed25519, "second@connecto").unwrap();
        let third = SshKeyPair::generate(KeyAlgorithm::Ed25519, "third@connecto").unwrap();

        manager.add_authorized_key(&first.public_key).unwrap();
        manager.add_authorized_key(&second.public_key).unwrap();
        manager.add_authorized_key(&third.public_key).unwrap();

        assert!(manager.remove_authorized_key(&second.public_key).unwrap());

        let keys = manager.list_authorized_keys().unwrap();
        assert_eq!(keys.len(), 2);
        assert!(keys[0].contains("first@connecto"));
        assert!(keys[1].contains("third@connecto"));
    }

    #[cfg(unix)]
    #[test]
    fn test_authorized_keys_file_has_0600_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let temp_dir = TempDir::new().unwrap();
        let ssh_dir = temp_dir.path().join(".ssh");
        let manager = KeyManager::with_dir(ssh_dir);

        let key_pair = SshKeyPair::generate(KeyAlgorithm::Ed25519, "perm@connecto").unwrap();
        manager.add_authorized_key(&key_pair.public_key).unwrap();

        let mode = fs::metadata(manager.authorized_keys_path())
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn test_save_key_pair_private_key_has_0600_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let temp_dir = TempDir::new().unwrap();
        let ssh_dir = temp_dir.path().join(".ssh");
        let manager = KeyManager::with_dir(ssh_dir);

        let key_pair = SshKeyPair::generate(KeyAlgorithm::Ed25519, "perm@connecto").unwrap();
        let (private_path, _) = manager.save_key_pair(&key_pair, "connecto_perm").unwrap();

        let mode = fs::metadata(&private_path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    #[test]
    fn test_key_blob_extraction() {
        assert_eq!(
            key_blob("ssh-ed25519 AAAAC3Nza comment@host"),
            Some("AAAAC3Nza")
        );
        assert_eq!(key_blob("ssh-ed25519 AAAAC3Nza"), Some("AAAAC3Nza"));
        assert_eq!(key_blob("ssh-ed25519"), None);
        assert_eq!(key_blob(""), None);
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
