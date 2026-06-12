//! CLI command implementations

pub mod hosts;
pub mod keygen;
pub mod keys;
pub mod listen;
pub mod pair;
pub mod scan;
pub mod ssh;
pub mod sync;

use anyhow::{anyhow, Result};
use colored::Colorize;
use connecto_core::{
    discovery::get_hostname,
    keys::{KeyAlgorithm, SshKeyPair},
    paths, Config,
};
use indicatif::{ProgressBar, ProgressStyle};
use std::path::PathBuf;
use std::time::Duration;

/// Print a success message
pub fn success(msg: &str) {
    println!("{} {}", "✓".green().bold(), msg);
}

/// Print an error message
pub fn error(msg: &str) {
    eprintln!("{} {}", "✗".red().bold(), msg);
}

/// Print an info message
pub fn info(msg: &str) {
    println!("{} {}", "→".cyan().bold(), msg);
}

/// Print a warning message
pub fn warn(msg: &str) {
    println!("{} {}", "!".yellow().bold(), msg);
}

/// Create the standard connecto spinner (already ticking)
///
/// `color` is an indicatif style color name (e.g. "cyan", "magenta").
pub fn spinner(color: &str, msg: &'static str) -> ProgressBar {
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏")
            .template(&format!("{{spinner:.{}}} {{msg}}", color))
            .unwrap(),
    );
    pb.set_message(msg);
    pb.enable_steady_tick(Duration::from_millis(80));
    pb
}

/// Default key comment: `user@host`, with `USERNAME` fallback for Windows
pub fn default_key_comment(host: &str) -> String {
    let user = std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "user".to_string());
    format!("{}@{}", user, host)
}

/// Resolve the SSH key pair a command should use
///
/// Priority: `--key` flag > `config.default_key` > generate a new key.
/// Existing key paths get tilde expansion and are validated (both the private
/// key and its `.pub` file must exist). Returns the key pair plus the path of
/// the existing key when one was used; `None` means the key was freshly
/// generated and still needs to be saved by the caller.
pub fn resolve_key_pair(
    key_flag: Option<&str>,
    rsa: bool,
    comment: Option<&str>,
    config: &Config,
) -> Result<(SshKeyPair, Option<PathBuf>)> {
    let existing = key_flag
        .map(str::to_string)
        .or_else(|| config.default_key.clone());

    if let Some(raw_path) = existing {
        let key_path = paths::expand_tilde(&raw_path)?;
        if !key_path.exists() {
            return Err(anyhow!("Key file not found: {}", key_path.display()));
        }
        let pub_path = PathBuf::from(format!("{}.pub", key_path.display()));
        if !pub_path.exists() {
            return Err(anyhow!(
                "Public key not found: {} (both private and public key files are required)",
                pub_path.display()
            ));
        }

        info(&format!(
            "Using existing key: {}",
            key_path.display().to_string().cyan()
        ));
        let key_pair = SshKeyPair::load_from_file(&key_path.to_string_lossy())?;
        return Ok((key_pair, Some(key_path)));
    }

    let algorithm = if rsa {
        warn("Using RSA-4096 (Ed25519 is recommended for better security)");
        KeyAlgorithm::Rsa4096
    } else {
        info("Using Ed25519 key (modern, secure, fast)");
        KeyAlgorithm::Ed25519
    };

    let key_comment = comment
        .map(str::to_string)
        .unwrap_or_else(|| default_key_comment(&get_hostname()));

    let key_pair = SshKeyPair::generate(algorithm, &key_comment)?;
    Ok((key_pair, None))
}

#[cfg(test)]
mod tests {
    use super::*;
    use connecto_core::keys::KeyManager;
    use tempfile::TempDir;

    #[test]
    fn test_default_key_comment_has_user_and_host() {
        let comment = default_key_comment("myhost");
        assert!(comment.ends_with("@myhost"));
        assert!(!comment.starts_with('@'));
    }

    #[test]
    fn test_resolve_key_pair_generates_when_nothing_configured() {
        let (key_pair, path) =
            resolve_key_pair(None, false, Some("t@t"), &Config::default()).unwrap();
        assert!(path.is_none());
        assert!(key_pair.public_key.starts_with("ssh-ed25519 "));
        assert_eq!(key_pair.comment, "t@t");
    }

    #[test]
    fn test_resolve_key_pair_prefers_key_flag_over_default_key() {
        let temp = TempDir::new().unwrap();
        let manager = KeyManager::with_dir(temp.path().join(".ssh"));
        let generated = SshKeyPair::generate(KeyAlgorithm::Ed25519, "exist@host").unwrap();
        let (priv_path, _) = manager
            .save_key_pair(&generated, "connecto_existing")
            .unwrap();

        let mut config = Config::default();
        config.set_default_key("/nonexistent/should-not-be-used");

        let (key_pair, used) =
            resolve_key_pair(Some(&priv_path.display().to_string()), false, None, &config).unwrap();
        assert_eq!(used.as_deref(), Some(priv_path.as_path()));
        assert_eq!(key_pair.comment, "exist@host");
    }

    #[test]
    fn test_resolve_key_pair_uses_config_default_key() {
        let temp = TempDir::new().unwrap();
        let manager = KeyManager::with_dir(temp.path().join(".ssh"));
        let generated = SshKeyPair::generate(KeyAlgorithm::Ed25519, "default@host").unwrap();
        let (priv_path, _) = manager
            .save_key_pair(&generated, "connecto_default")
            .unwrap();

        let mut config = Config::default();
        config.set_default_key(&priv_path.display().to_string());

        let (key_pair, used) = resolve_key_pair(None, false, None, &config).unwrap();
        assert_eq!(used.as_deref(), Some(priv_path.as_path()));
        assert_eq!(key_pair.comment, "default@host");
    }

    #[test]
    fn test_resolve_key_pair_missing_private_key_errors() {
        let err = resolve_key_pair(
            Some("/definitely/missing/key"),
            false,
            None,
            &Config::default(),
        )
        .unwrap_err();
        assert!(err.to_string().contains("Key file not found"));
    }

    #[test]
    fn test_resolve_key_pair_missing_public_key_errors() {
        let temp = TempDir::new().unwrap();
        let priv_only = temp.path().join("key_without_pub");
        std::fs::write(&priv_only, "private").unwrap();

        let err = resolve_key_pair(
            Some(&priv_only.display().to_string()),
            false,
            None,
            &Config::default(),
        )
        .unwrap_err();
        assert!(err.to_string().contains("Public key not found"));
    }
}
