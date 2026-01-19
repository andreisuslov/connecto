//! Keys command - Manage authorized keys

use crate::KeysAction;
use anyhow::{anyhow, Result};
use colored::Colorize;
use connecto_core::keys::KeyManager;
use dialoguer::{theme::ColorfulTheme, Confirm};

use super::{error, info, success, warn};

pub async fn run(action: Option<KeysAction>) -> Result<()> {
    let key_manager = KeyManager::new()?;

    match action {
        None | Some(KeysAction::List) => list_keys(&key_manager).await,
        Some(KeysAction::Remove { target }) => remove_key(&key_manager, &target).await,
    }
}

async fn list_keys(key_manager: &KeyManager) -> Result<()> {
    println!();
    println!("{}", "  AUTHORIZED KEYS  ".on_bright_yellow().black().bold());
    println!();

    let keys = key_manager.list_authorized_keys()?;

    if keys.is_empty() {
        info("No authorized keys found.");
        println!();
        println!(
            "{}",
            "Run 'connecto listen' on this machine to allow other devices to pair.".dimmed()
        );
        println!();
        return Ok(());
    }

    println!("{} authorized key(s) found:", keys.len());
    println!();

    for (i, key) in keys.iter().enumerate() {
        let parts: Vec<&str> = key.split_whitespace().collect();

        let num = format!("[{}]", i + 1).yellow().bold();
        let key_type = parts.first().unwrap_or(&"unknown");
        let comment = if parts.len() > 2 {
            parts[2..].join(" ")
        } else {
            "no comment".to_string()
        };

        // Truncate key data for display
        let key_preview = if parts.len() > 1 {
            let key_data = parts[1];
            if key_data.len() > 20 {
                format!("{}...{}", &key_data[..10], &key_data[key_data.len()-10..])
            } else {
                key_data.to_string()
            }
        } else {
            "invalid".to_string()
        };

        println!(
            "{} {} {} {}",
            num,
            key_type.cyan(),
            key_preview.dimmed(),
            comment.green()
        );
    }

    println!();
    println!(
        "{}",
        format!(
            "To remove a key: {}",
            "connecto keys remove <number>".cyan()
        )
        .dimmed()
    );
    println!();

    Ok(())
}

async fn remove_key(key_manager: &KeyManager, target: &str) -> Result<()> {
    println!();
    println!("{}", "  REMOVE KEY  ".on_bright_red().white().bold());
    println!();

    let keys = key_manager.list_authorized_keys()?;

    if keys.is_empty() {
        info("No authorized keys to remove.");
        return Ok(());
    }

    // Try to parse as number first
    let key_to_remove = if let Ok(index) = target.parse::<usize>() {
        if index == 0 || index > keys.len() {
            return Err(anyhow!(
                "Invalid key number {}. Valid range: 1-{}",
                index,
                keys.len()
            ));
        }
        keys[index - 1].clone()
    } else {
        // Search by pattern (comment or key type)
        let matches: Vec<&String> = keys
            .iter()
            .filter(|k| k.to_lowercase().contains(&target.to_lowercase()))
            .collect();

        match matches.len() {
            0 => return Err(anyhow!("No keys matching '{}' found", target)),
            1 => matches[0].clone(),
            _ => {
                warn(&format!("Multiple keys match '{}'. Please be more specific:", target));
                for (i, key) in matches.iter().enumerate() {
                    let parts: Vec<&str> = key.split_whitespace().collect();
                    let comment = if parts.len() > 2 {
                        parts[2..].join(" ")
                    } else {
                        "no comment".to_string()
                    };
                    println!("  {} {}", format!("[{}]", i + 1).yellow(), comment);
                }
                return Ok(());
            }
        }
    };

    // Show what we're about to remove
    let parts: Vec<&str> = key_to_remove.split_whitespace().collect();
    let key_type = parts.first().unwrap_or(&"unknown");
    let comment = if parts.len() > 2 {
        parts[2..].join(" ")
    } else {
        "no comment".to_string()
    };

    println!("About to remove:");
    println!(
        "  {} {} - {}",
        "•".red(),
        key_type.cyan(),
        comment.green()
    );
    println!();

    // Confirm
    let confirmed = Confirm::with_theme(&ColorfulTheme::default())
        .with_prompt("Are you sure you want to remove this key?")
        .default(false)
        .interact()?;

    if !confirmed {
        info("Operation cancelled.");
        return Ok(());
    }

    // Remove the key
    let removed = key_manager.remove_authorized_key(&key_to_remove)?;

    if removed {
        success("Key removed successfully.");
    } else {
        error("Failed to remove key.");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_module_compiles() {
        assert!(true);
    }
}
