//! Connecto-managed `~/.ssh/config` entries
//!
//! This module is the single parser/writer for the SSH config blocks that
//! Connecto creates. Every managed block is preceded by the
//! [`CONNECTO_MARKER`] comment line and written in a fixed four-line format:
//!
//! ```text
//! # Added by connecto
//! Host my-device
//!     HostName 192.168.1.10
//!     User alice
//!     IdentityFile /home/alice/.ssh/connecto_my_device
//! ```
//!
//! Only blocks preceded by the marker are ever listed, replaced, updated, or
//! removed; user-authored blocks are never touched, even if they share an
//! alias with a managed entry.

use crate::error::Result;
use crate::{fsutil, paths};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use tracing::debug;

/// Comment line that precedes every connecto-managed SSH config block
pub const CONNECTO_MARKER: &str = "# Added by connecto";

/// A connecto-managed SSH config entry
///
/// Fields that are missing from a (possibly hand-edited) block are returned
/// as empty strings rather than dropping the entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostEntry {
    /// SSH host alias (the `Host` line)
    pub host: String,
    /// Target address (the `HostName` line)
    pub hostname: String,
    /// Remote user (the `User` line)
    pub user: String,
    /// Path to the private key (the `IdentityFile` line)
    pub identity_file: String,
}

/// Outcome of [`SshConfig::add_host`]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddOutcome {
    /// A new block was appended to the config
    Added,
    /// An existing connecto-managed block with the same alias was replaced
    Replaced,
}

/// Reader/writer for connecto-managed entries in an SSH config file
#[derive(Debug, Clone)]
pub struct SshConfig {
    path: PathBuf,
}

/// A parsed connecto-managed block within the config file
struct Block {
    /// Index of the marker line
    marker_idx: usize,
    /// One past the last line belonging to the block
    end_idx: usize,
    /// Index of the `HostName` line within the block, if present
    hostname_idx: Option<usize>,
    /// The parsed entry
    entry: HostEntry,
}

impl SshConfig {
    /// Get the default SSH config path (`~/.ssh/config`)
    pub fn default_path() -> Result<PathBuf> {
        Ok(paths::ssh_dir()?.join("config"))
    }

    /// Create an `SshConfig` for an explicit config file path (useful for testing)
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// Create an `SshConfig` for the default config path
    pub fn at_default() -> Result<Self> {
        Ok(Self::new(Self::default_path()?))
    }

    /// Get the config file path this instance operates on
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// List all connecto-managed host entries
    ///
    /// Only blocks preceded by the [`CONNECTO_MARKER`] line are returned.
    /// Returns an empty list if the config file does not exist.
    pub fn list_hosts(&self) -> Result<Vec<HostEntry>> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }
        let lines = self.read_lines()?;
        Ok(parse_blocks(&lines).into_iter().map(|b| b.entry).collect())
    }

    /// Add a connecto-managed host entry
    ///
    /// If a connecto-managed block with the same alias already exists, its
    /// block is replaced in place and [`AddOutcome::Replaced`] is returned;
    /// otherwise a new block is appended. Creates the config file (and its
    /// parent directory, mode `0o700` on Unix) if missing. User-authored
    /// blocks with the same alias are never modified.
    pub fn add_host(&self, entry: &HostEntry) -> Result<AddOutcome> {
        self.ensure_parent_dir()?;

        let content = if self.path.exists() {
            fs::read_to_string(&self.path)?
        } else {
            String::new()
        };
        let lines: Vec<String> = content.lines().map(String::from).collect();
        let blocks = parse_blocks(&lines);

        if let Some(block) = blocks.iter().find(|b| b.entry.host == entry.host) {
            let mut new_lines: Vec<String> = Vec::with_capacity(lines.len() + 5);
            new_lines.extend_from_slice(&lines[..block.marker_idx]);
            new_lines.extend(block_lines(entry));
            new_lines.extend_from_slice(&lines[block.end_idx..]);
            self.write_lines(&new_lines)?;
            debug!(host = %entry.host, "Replaced connecto-managed SSH config entry");
            return Ok(AddOutcome::Replaced);
        }

        let mut new_content = content;
        new_content.push_str(&format_entry(entry));
        fsutil::write_atomic(&self.path, &new_content)?;
        debug!(host = %entry.host, "Added connecto-managed SSH config entry");
        Ok(AddOutcome::Added)
    }

    /// Remove a connecto-managed host entry by exact alias
    ///
    /// Removes the block (including the marker line and the blank separator
    /// line before it) and returns the removed entry so callers can decide
    /// what to do with the associated key files. Returns `Ok(None)` if no
    /// connecto-managed block matches; user-authored blocks are never
    /// removed, even if they have the same alias.
    pub fn remove_host(&self, host: &str) -> Result<Option<HostEntry>> {
        if !self.path.exists() {
            return Ok(None);
        }
        let lines = self.read_lines()?;
        let blocks = parse_blocks(&lines);

        let block = match blocks.into_iter().find(|b| b.entry.host == host) {
            Some(b) => b,
            None => return Ok(None),
        };

        // Also drop the blank separator line the writer places before the marker.
        let mut start = block.marker_idx;
        if start > 0 && lines[start - 1].trim().is_empty() {
            start -= 1;
        }

        let mut new_lines: Vec<String> = Vec::with_capacity(lines.len());
        new_lines.extend_from_slice(&lines[..start]);
        new_lines.extend_from_slice(&lines[block.end_idx..]);
        self.write_lines(&new_lines)?;
        debug!(host, "Removed connecto-managed SSH config entry");
        Ok(Some(block.entry))
    }

    /// Update the `HostName` of a connecto-managed entry by exact alias
    ///
    /// Returns the old hostname, or `Ok(None)` if no connecto-managed block
    /// matches. User-authored blocks are never modified.
    pub fn update_hostname(&self, host: &str, new_hostname: &str) -> Result<Option<String>> {
        if !self.path.exists() {
            return Ok(None);
        }
        let mut lines = self.read_lines()?;
        let blocks = parse_blocks(&lines);

        let block = match blocks.into_iter().find(|b| b.entry.host == host) {
            Some(b) => b,
            None => return Ok(None),
        };

        let new_line = format!("    HostName {}", new_hostname);
        match block.hostname_idx {
            Some(idx) => lines[idx] = new_line,
            // The writer always emits a HostName line; tolerate a hand-edited
            // block without one by inserting it right after the Host line.
            None => lines.insert(block.marker_idx + 2, new_line),
        }
        self.write_lines(&lines)?;
        debug!(host, new_hostname, "Updated connecto-managed HostName");
        Ok(Some(block.entry.hostname))
    }

    /// Read the config file as a list of lines
    fn read_lines(&self) -> Result<Vec<String>> {
        let content = fs::read_to_string(&self.path)?;
        Ok(content.lines().map(String::from).collect())
    }

    /// Write lines back atomically, with a trailing newline
    fn write_lines(&self, lines: &[String]) -> Result<()> {
        let mut content = lines.join("\n");
        if !content.is_empty() {
            content.push('\n');
        }
        fsutil::write_atomic(&self.path, &content)
    }

    /// Ensure the config file's parent directory exists (mode `0o700` on Unix)
    fn ensure_parent_dir(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            if !parent.as_os_str().is_empty() && !parent.exists() {
                fs::create_dir_all(parent)?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
                }
            }
        }
        Ok(())
    }
}

/// Format a managed entry in the exact four-line format used on disk
///
/// This format must round-trip with entries already written by older
/// connecto versions, so do not change it.
fn format_entry(entry: &HostEntry) -> String {
    format!(
        "\n{}\nHost {}\n    HostName {}\n    User {}\n    IdentityFile {}\n",
        CONNECTO_MARKER, entry.host, entry.hostname, entry.user, entry.identity_file
    )
}

/// The lines of a managed block (marker + four config lines)
fn block_lines(entry: &HostEntry) -> Vec<String> {
    vec![
        CONNECTO_MARKER.to_string(),
        format!("Host {}", entry.host),
        format!("    HostName {}", entry.hostname),
        format!("    User {}", entry.user),
        format!("    IdentityFile {}", entry.identity_file),
    ]
}

/// Parse all connecto-managed blocks out of the config lines
///
/// A managed block is a [`CONNECTO_MARKER`] line immediately followed by a
/// `Host` line with a single non-wildcard alias. The block extends until a
/// blank line, the next `Host` line, the next marker, or end of file —
/// trailing blocks at EOF are flushed. Keys may be indented or unindented,
/// and entries missing `IdentityFile` (or other keys) are still emitted with
/// those fields empty.
fn parse_blocks(lines: &[String]) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        if lines[i].trim() != CONNECTO_MARKER {
            i += 1;
            continue;
        }
        let marker_idx = i;

        // The writer always places the Host line directly after the marker.
        let alias = match lines.get(i + 1).and_then(|l| host_alias(l.trim())) {
            Some(a) => a.to_string(),
            None => {
                // Dangling marker (no Host line, or a wildcard pattern):
                // not a managed block.
                i += 1;
                continue;
            }
        };

        let mut entry = HostEntry {
            host: alias,
            hostname: String::new(),
            user: String::new(),
            identity_file: String::new(),
        };
        let mut hostname_idx = None;

        let mut j = i + 2;
        while j < lines.len() {
            let trimmed = lines[j].trim();
            if trimmed.is_empty() || trimmed == CONNECTO_MARKER || is_host_line(trimmed) {
                break;
            }
            if let Some(value) = key_value(trimmed, "HostName") {
                entry.hostname = value.to_string();
                hostname_idx = Some(j);
            } else if let Some(value) = key_value(trimmed, "User") {
                entry.user = value.to_string();
            } else if let Some(value) = key_value(trimmed, "IdentityFile") {
                entry.identity_file = value.to_string();
            }
            j += 1;
        }

        blocks.push(Block {
            marker_idx,
            end_idx: j,
            hostname_idx,
            entry,
        });
        i = j;
    }

    blocks
}

/// Extract the alias from a trimmed `Host` line
///
/// Returns `None` for wildcard patterns (`Host *`, `Host ?…`) — those are
/// never connecto-managed.
fn host_alias(trimmed: &str) -> Option<&str> {
    let rest = trimmed.strip_prefix("Host")?;
    if !rest.starts_with(char::is_whitespace) {
        return None;
    }
    let alias = rest.trim();
    if alias.is_empty() || alias.contains('*') || alias.contains('?') {
        return None;
    }
    Some(alias)
}

/// Whether a trimmed line starts a new `Host` block (wildcard or not)
fn is_host_line(trimmed: &str) -> bool {
    trimmed == "Host"
        || trimmed
            .strip_prefix("Host")
            .is_some_and(|rest| rest.starts_with(char::is_whitespace))
}

/// If `trimmed` is `<keyword> <value>`, return the value
fn key_value<'a>(trimmed: &'a str, keyword: &str) -> Option<&'a str> {
    let rest = trimmed.strip_prefix(keyword)?;
    if !rest.starts_with(char::is_whitespace) {
        return None;
    }
    let value = rest.trim();
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn entry(host: &str) -> HostEntry {
        HostEntry {
            host: host.to_string(),
            hostname: "192.168.1.10".to_string(),
            user: "alice".to_string(),
            identity_file: format!("/home/alice/.ssh/connecto_{}", host),
        }
    }

    fn config_in(temp_dir: &TempDir) -> SshConfig {
        SshConfig::new(temp_dir.path().join(".ssh").join("config"))
    }

    #[test]
    fn test_list_hosts_missing_file() {
        let temp_dir = TempDir::new().unwrap();
        let config = config_in(&temp_dir);

        assert!(config.list_hosts().unwrap().is_empty());
    }

    #[test]
    fn test_round_trip_add_list_update_remove() {
        let temp_dir = TempDir::new().unwrap();
        let config = config_in(&temp_dir);
        let e = entry("my-device");

        assert_eq!(config.add_host(&e).unwrap(), AddOutcome::Added);

        let hosts = config.list_hosts().unwrap();
        assert_eq!(hosts, vec![e.clone()]);

        let old = config
            .update_hostname("my-device", "10.0.0.5")
            .unwrap()
            .unwrap();
        assert_eq!(old, "192.168.1.10");
        assert_eq!(config.list_hosts().unwrap()[0].hostname, "10.0.0.5");

        let removed = config.remove_host("my-device").unwrap().unwrap();
        assert_eq!(removed.hostname, "10.0.0.5");
        assert_eq!(removed.identity_file, e.identity_file);
        assert!(config.list_hosts().unwrap().is_empty());
    }

    #[test]
    fn test_add_creates_file_and_dir_from_scratch() {
        let temp_dir = TempDir::new().unwrap();
        let config = config_in(&temp_dir);

        assert!(!temp_dir.path().join(".ssh").exists());
        config.add_host(&entry("fresh")).unwrap();
        assert!(config.path().exists());

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let dir_mode = fs::metadata(temp_dir.path().join(".ssh"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(dir_mode & 0o777, 0o700);
            let file_mode = fs::metadata(config.path()).unwrap().permissions().mode();
            assert_eq!(file_mode & 0o777, 0o600);
        }
    }

    #[test]
    fn test_add_uses_exact_on_disk_format() {
        let temp_dir = TempDir::new().unwrap();
        let config = config_in(&temp_dir);
        let e = entry("exact");

        config.add_host(&e).unwrap();

        let content = fs::read_to_string(config.path()).unwrap();
        let expected = format!(
            "\n# Added by connecto\nHost exact\n    HostName 192.168.1.10\n    User alice\n    IdentityFile {}\n",
            e.identity_file
        );
        assert_eq!(content, expected);
    }

    #[test]
    fn test_add_replaces_existing_connecto_entry_in_place() {
        let temp_dir = TempDir::new().unwrap();
        let config = config_in(&temp_dir);

        config.add_host(&entry("first")).unwrap();
        config.add_host(&entry("second")).unwrap();

        let mut updated = entry("first");
        updated.hostname = "172.16.0.2".to_string();
        updated.user = "bob".to_string();
        assert_eq!(config.add_host(&updated).unwrap(), AddOutcome::Replaced);

        let hosts = config.list_hosts().unwrap();
        assert_eq!(hosts.len(), 2);
        // Replaced in place: "first" keeps its position before "second".
        assert_eq!(hosts[0], updated);
        assert_eq!(hosts[1], entry("second"));

        let content = fs::read_to_string(config.path()).unwrap();
        assert_eq!(content.matches(CONNECTO_MARKER).count(), 2);
        assert_eq!(content.matches("Host first").count(), 1);
    }

    #[test]
    fn test_foreign_block_with_same_alias_is_never_touched() {
        let temp_dir = TempDir::new().unwrap();
        let config = config_in(&temp_dir);
        fs::create_dir_all(config.path().parent().unwrap()).unwrap();

        let foreign = "Host myhost\n    HostName example.com\n    User carol\n";
        fs::write(config.path(), foreign).unwrap();

        // Not listed.
        assert!(config.list_hosts().unwrap().is_empty());

        // Not removed (this was the old data-loss bug).
        assert!(config.remove_host("myhost").unwrap().is_none());
        assert_eq!(fs::read_to_string(config.path()).unwrap(), foreign);

        // Not updated.
        assert!(config
            .update_hostname("myhost", "10.0.0.1")
            .unwrap()
            .is_none());
        assert_eq!(fs::read_to_string(config.path()).unwrap(), foreign);

        // Adding a managed entry with the same alias appends a new block...
        assert_eq!(
            config.add_host(&entry("myhost")).unwrap(),
            AddOutcome::Added
        );
        let hosts = config.list_hosts().unwrap();
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0], entry("myhost"));

        // ...and removing it leaves the user-authored block intact.
        assert!(config.remove_host("myhost").unwrap().is_some());
        let content = fs::read_to_string(config.path()).unwrap();
        assert!(content.contains("HostName example.com"));
        assert!(content.contains("User carol"));
        assert!(!content.contains(CONNECTO_MARKER));
    }

    #[test]
    fn test_trailing_block_at_eof_is_flushed() {
        let temp_dir = TempDir::new().unwrap();
        let config = config_in(&temp_dir);
        fs::create_dir_all(config.path().parent().unwrap()).unwrap();

        // No trailing newline, no blank line after the block.
        fs::write(
            config.path(),
            "# Added by connecto\nHost tail\n    HostName 10.1.1.1\n    User dave\n    IdentityFile /k/tail",
        )
        .unwrap();

        let hosts = config.list_hosts().unwrap();
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].host, "tail");
        assert_eq!(hosts[0].hostname, "10.1.1.1");
        assert_eq!(hosts[0].user, "dave");
        assert_eq!(hosts[0].identity_file, "/k/tail");
    }

    #[test]
    fn test_entry_without_identity_file_is_emitted() {
        let temp_dir = TempDir::new().unwrap();
        let config = config_in(&temp_dir);
        fs::create_dir_all(config.path().parent().unwrap()).unwrap();

        fs::write(
            config.path(),
            "# Added by connecto\nHost nokey\n    HostName 10.2.2.2\n    User erin\n\nHost other\n    HostName example.org\n",
        )
        .unwrap();

        let hosts = config.list_hosts().unwrap();
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].host, "nokey");
        assert_eq!(hosts[0].identity_file, "");
    }

    #[test]
    fn test_unindented_keys_are_parsed() {
        let temp_dir = TempDir::new().unwrap();
        let config = config_in(&temp_dir);
        fs::create_dir_all(config.path().parent().unwrap()).unwrap();

        fs::write(
            config.path(),
            "# Added by connecto\nHost flat\nHostName 10.3.3.3\nUser frank\nIdentityFile /k/flat\n",
        )
        .unwrap();

        let hosts = config.list_hosts().unwrap();
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].hostname, "10.3.3.3");
        assert_eq!(hosts[0].user, "frank");
        assert_eq!(hosts[0].identity_file, "/k/flat");
    }

    #[test]
    fn test_wildcard_host_is_never_managed() {
        let temp_dir = TempDir::new().unwrap();
        let config = config_in(&temp_dir);
        fs::create_dir_all(config.path().parent().unwrap()).unwrap();

        fs::write(
            config.path(),
            "# Added by connecto\nHost *\n    User root\n\n# Added by connecto\nHost real\n    HostName 10.4.4.4\n    User gina\n    IdentityFile /k/real\n",
        )
        .unwrap();

        let hosts = config.list_hosts().unwrap();
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].host, "real");

        assert!(config.remove_host("*").unwrap().is_none());
    }

    #[test]
    fn test_remove_middle_entry_keeps_neighbors() {
        let temp_dir = TempDir::new().unwrap();
        let config = config_in(&temp_dir);

        config.add_host(&entry("alpha")).unwrap();
        config.add_host(&entry("beta")).unwrap();
        config.add_host(&entry("gamma")).unwrap();

        let removed = config.remove_host("beta").unwrap().unwrap();
        assert_eq!(removed.host, "beta");

        let hosts = config.list_hosts().unwrap();
        assert_eq!(hosts.len(), 2);
        assert_eq!(hosts[0].host, "alpha");
        assert_eq!(hosts[1].host, "gamma");

        // No leftover marker or double blank lines from the removed block.
        let content = fs::read_to_string(config.path()).unwrap();
        assert_eq!(content.matches(CONNECTO_MARKER).count(), 2);
        assert!(!content.contains("\n\n\n"));
        assert!(!content.contains("beta"));
    }

    #[test]
    fn test_remove_is_idempotent() {
        let temp_dir = TempDir::new().unwrap();
        let config = config_in(&temp_dir);

        config.add_host(&entry("once")).unwrap();
        assert!(config.remove_host("once").unwrap().is_some());
        assert!(config.remove_host("once").unwrap().is_none());

        // Missing file is also a no-op.
        let missing = SshConfig::new(temp_dir.path().join("nope").join("config"));
        assert!(missing.remove_host("once").unwrap().is_none());
    }

    #[test]
    fn test_remove_does_not_match_alias_prefix() {
        let temp_dir = TempDir::new().unwrap();
        let config = config_in(&temp_dir);

        config.add_host(&entry("dev")).unwrap();
        config.add_host(&entry("dev-2")).unwrap();

        assert!(config.remove_host("dev").unwrap().is_some());
        let hosts = config.list_hosts().unwrap();
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].host, "dev-2");
    }

    #[test]
    fn test_update_hostname_missing_host_returns_none() {
        let temp_dir = TempDir::new().unwrap();
        let config = config_in(&temp_dir);

        config.add_host(&entry("known")).unwrap();
        assert!(config
            .update_hostname("unknown", "10.0.0.9")
            .unwrap()
            .is_none());
    }

    #[test]
    fn test_dangling_marker_is_ignored() {
        let temp_dir = TempDir::new().unwrap();
        let config = config_in(&temp_dir);
        fs::create_dir_all(config.path().parent().unwrap()).unwrap();

        fs::write(
            config.path(),
            "# Added by connecto\n\n# Added by connecto\nHost ok\n    HostName 10.5.5.5\n    User hank\n    IdentityFile /k/ok\n",
        )
        .unwrap();

        let hosts = config.list_hosts().unwrap();
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].host, "ok");
    }

    #[test]
    fn test_host_entry_serde_round_trip() {
        let e = entry("serde");
        let json = serde_json::to_string(&e).unwrap();
        assert!(json.contains("\"identity_file\""));
        let back: HostEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(back, e);
    }
}
