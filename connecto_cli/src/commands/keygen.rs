//! Keygen command - Generate SSH key pairs

use anyhow::Result;
use colored::Colorize;
use connecto_core::{keys::KeyManager, Config};

use super::{info, resolve_key_pair, success};

pub async fn run(name: String, comment: Option<String>, rsa: bool) -> Result<()> {
    println!();
    println!(
        "{}",
        "  SSH KEY GENERATOR  ".on_bright_green().black().bold()
    );
    println!();

    // keygen always generates a fresh key, so it deliberately passes an empty
    // config: a configured default_key must not apply here.
    let (key_pair, _) = resolve_key_pair(None, rsa, comment.as_deref(), &Config::default())?;

    info(&format!("Comment: {}", key_pair.comment.cyan()));
    println!();

    // Save key pair
    let key_manager = KeyManager::new()?;
    let (private_path, public_path) = key_manager.save_key_pair(&key_pair, &name)?;

    println!();
    success("Key pair generated successfully!");
    println!();

    println!("{}", "Files created:".bold());
    println!(
        "  {} Private key: {}",
        "•".green(),
        private_path.display().to_string().cyan()
    );
    println!(
        "  {} Public key:  {}",
        "•".green(),
        public_path.display().to_string().cyan()
    );
    println!();

    // Show public key
    println!("{}", "Public key:".bold());
    println!("{}", key_pair.public_key.dimmed());
    println!();

    // Show usage hints
    println!("{}", "Usage:".bold());
    println!(
        "  {} Copy to remote: {}",
        "→".cyan(),
        format!("ssh-copy-id -i {} user@host", public_path.display()).dimmed()
    );
    println!(
        "  {} Or use Connecto: {}",
        "→".cyan(),
        "connecto pair <device>".dimmed()
    );
    println!();

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use connecto_core::keys::{KeyAlgorithm, SshKeyPair};

    #[tokio::test]
    async fn test_key_generation() {
        // We can't fully test run() without mocking, but we can test the underlying functions
        let key_pair = SshKeyPair::generate(KeyAlgorithm::Ed25519, "test@test").unwrap();
        assert!(key_pair.public_key.starts_with("ssh-ed25519 "));
        assert!(key_pair.public_key.contains("test@test"));
    }

    #[test]
    fn test_run_ignores_default_key_config() {
        // resolve_key_pair with an empty config must always generate.
        let (key_pair, used_existing) =
            resolve_key_pair(None, false, Some("gen@test"), &Config::default()).unwrap();
        assert!(used_existing.is_none());
        assert_eq!(key_pair.comment, "gen@test");
    }
}
