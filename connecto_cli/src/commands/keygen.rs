//! Keygen command - Generate SSH key pairs

use anyhow::Result;
use colored::Colorize;
use connecto_core::{
    discovery::get_hostname,
    keys::{KeyAlgorithm, KeyManager, SshKeyPair},
};

use super::{info, success, warn};

pub async fn run(name: String, comment: Option<String>, rsa: bool) -> Result<()> {
    println!();
    println!("{}", "  SSH KEY GENERATOR  ".on_bright_green().black().bold());
    println!();

    // Determine algorithm
    let algorithm = if rsa {
        warn("Using RSA-4096 (Ed25519 is recommended for better security and performance)");
        KeyAlgorithm::Rsa4096
    } else {
        info("Using Ed25519 (modern, secure, fast)");
        KeyAlgorithm::Ed25519
    };

    // Generate comment
    let key_comment = comment.unwrap_or_else(|| {
        let user = std::env::var("USER").unwrap_or_else(|_| "user".to_string());
        let hostname = get_hostname();
        format!("{}@{}", user, hostname)
    });

    info(&format!("Comment: {}", key_comment.cyan()));
    println!();

    // Generate key pair
    info("Generating key pair...");
    let key_pair = SshKeyPair::generate(algorithm, &key_comment)?;

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

    #[tokio::test]
    async fn test_key_generation() {
        // We can't fully test run() without mocking, but we can test the underlying functions
        let key_pair = SshKeyPair::generate(KeyAlgorithm::Ed25519, "test@test").unwrap();
        assert!(key_pair.public_key.starts_with("ssh-ed25519 "));
        assert!(key_pair.public_key.contains("test@test"));
    }

    #[test]
    fn test_algorithm_selection() {
        let ed25519 = KeyAlgorithm::Ed25519;
        let rsa = KeyAlgorithm::Rsa4096;

        assert_eq!(ed25519, KeyAlgorithm::default());
        assert_ne!(rsa, KeyAlgorithm::default());
    }
}
