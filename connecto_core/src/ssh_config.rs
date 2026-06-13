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
        let (lines, _eol) = self.read_file()?;
        Ok(parse_blocks(&lines).into_iter().map(|b| b.entry).collect())
    }

    /// Add a connecto-managed host entry
    ///
    /// If a connecto-managed block with the same alias already exists, its
    /// block is replaced in place and [`AddOutcome::Replaced`] is returned;
    /// any extra option lines the user added inside the managed block (e.g.
    /// `Port`, `ProxyJump`) are preserved after the managed lines. Otherwise
    /// a new block is appended. Creates the config file (and its parent
    /// directory, mode `0o700` on Unix) if missing. User-authored blocks
    /// with the same alias are never modified.
    pub fn add_host(&self, entry: &HostEntry) -> Result<AddOutcome> {
        self.ensure_parent_dir()?;

        let content = if self.path.exists() {
            fs::read_to_string(&self.path)?
        } else {
            String::new()
        };
        let eol = detect_eol(&content);
        let lines: Vec<String> = content.lines().map(String::from).collect();
        let blocks = parse_blocks(&lines);

        if let Some(block) = blocks.iter().find(|b| b.entry.host == entry.host) {
            // Keep any lines in the old block that are not one of the
            // managed keys (marker, Host, HostName, User, IdentityFile) so
            // user customizations survive a re-pair.
            let extra: Vec<String> = lines[block.marker_idx..block.end_idx]
                .iter()
                .skip(2) // marker line + Host line
                .filter(|line| {
                    let trimmed = line.trim();
                    key_value(trimmed, "HostName").is_none()
                        && key_value(trimmed, "User").is_none()
                        && key_value(trimmed, "IdentityFile").is_none()
                })
                .cloned()
                .collect();

            let mut new_lines: Vec<String> = Vec::with_capacity(lines.len() + 5);
            new_lines.extend_from_slice(&lines[..block.marker_idx]);
            new_lines.extend(block_lines(entry));
            new_lines.extend(extra);
            new_lines.extend_from_slice(&lines[block.end_idx..]);
            self.write_lines(&new_lines, eol)?;
            debug!(host = %entry.host, "Replaced connecto-managed SSH config entry");
            return Ok(AddOutcome::Replaced);
        }

        let mut new_content = content;
        let formatted = format_entry(entry);
        if eol == "\n" {
            new_content.push_str(&formatted);
        } else {
            new_content.push_str(&formatted.replace('\n', eol));
        }
        fsutil::write_atomic(&self.path, &new_content)?;
        debug!(host = %entry.host, "Added connecto-managed SSH config entry");
        Ok(AddOutcome::Added)
    }

    /// Remove connecto-managed host entries by exact alias
    ///
    /// Removes every matching managed block (including the marker line and
    /// the blank separator line before each) and returns the first removed
    /// entry so callers can decide what to do with the associated key files.
    /// Duplicate same-alias blocks (e.g. from hand edits or imports) are all
    /// removed, so the alias no longer resolves afterwards. Returns
    /// `Ok(None)` if no connecto-managed block matches; user-authored blocks
    /// are never removed, even if they have the same alias.
    pub fn remove_host(&self, host: &str) -> Result<Option<HostEntry>> {
        if !self.path.exists() {
            return Ok(None);
        }
        let (lines, eol) = self.read_file()?;
        let blocks = parse_blocks(&lines);

        let matching: Vec<Block> = blocks
            .into_iter()
            .filter(|b| b.entry.host == host)
            .collect();
        let first = match matching.first() {
            Some(b) => b.entry.clone(),
            None => return Ok(None),
        };

        let mut keep = vec![true; lines.len()];
        for block in &matching {
            // Also drop the blank separator line the writer places before
            // the marker.
            let mut start = block.marker_idx;
            if start > 0 && lines[start - 1].trim().is_empty() {
                start -= 1;
            }
            for flag in keep.iter_mut().take(block.end_idx).skip(start) {
                *flag = false;
            }
        }

        let new_lines: Vec<String> = lines
            .into_iter()
            .zip(keep)
            .filter_map(|(line, keep)| keep.then_some(line))
            .collect();
        self.write_lines(&new_lines, eol)?;
        debug!(host, "Removed connecto-managed SSH config entry");
        Ok(Some(first))
    }

    /// Update the `HostName` of a connecto-managed entry by exact alias
    ///
    /// Returns the old hostname, or `Ok(None)` if no connecto-managed block
    /// matches. User-authored blocks are never modified.
    pub fn update_hostname(&self, host: &str, new_hostname: &str) -> Result<Option<String>> {
        if !self.path.exists() {
            return Ok(None);
        }
        let (mut lines, eol) = self.read_file()?;
        let blocks = parse_blocks(&lines);

        let block = match blocks.into_iter().find(|b| b.entry.host == host) {
            Some(b) => b,
            None => return Ok(None),
        };

        let new_line = format!("    HostName {}", quote_value(new_hostname));
        match block.hostname_idx {
            Some(idx) => lines[idx] = new_line,
            // The writer always emits a HostName line; tolerate a hand-edited
            // block without one by inserting it right after the Host line.
            None => lines.insert(block.marker_idx + 2, new_line),
        }
        self.write_lines(&lines, eol)?;
        debug!(host, new_hostname, "Updated connecto-managed HostName");
        Ok(Some(block.entry.hostname))
    }

    /// Read the config file as a list of lines plus its dominant line ending
    ///
    /// The line ending is reused on write so a CRLF config stays CRLF.
    fn read_file(&self) -> Result<(Vec<String>, &'static str)> {
        let content = fs::read_to_string(&self.path)?;
        let eol = detect_eol(&content);
        Ok((content.lines().map(String::from).collect(), eol))
    }

    /// Write lines back atomically, with a trailing newline, using the
    /// file's original line ending
    fn write_lines(&self, lines: &[String], eol: &str) -> Result<()> {
        let mut content = lines.join(eol);
        if !content.is_empty() {
            content.push_str(eol);
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

/// Detect the dominant line ending of a config file
///
/// Returns `"\r\n"` only when CRLF endings outnumber bare-LF ones, so a new
/// or empty file (and a mixed file with no CRLF majority) defaults to `"\n"`.
fn detect_eol(content: &str) -> &'static str {
    let crlf = content.matches("\r\n").count();
    let lf = content.matches('\n').count() - crlf;
    if crlf > lf {
        "\r\n"
    } else {
        "\n"
    }
}

/// Quote a value for an ssh_config line if it contains whitespace
///
/// ssh_config splits arguments on whitespace unless they are enclosed in
/// double quotes, so e.g. an `IdentityFile` path with spaces must be quoted.
fn quote_value(value: &str) -> String {
    if value.contains(char::is_whitespace) {
        format!("\"{}\"", value)
    } else {
        value.to_string()
    }
}

/// Format a managed entry in the exact four-line format used on disk
///
/// This format must round-trip with entries already written by older
/// connecto versions, so do not change it. Values containing whitespace are
/// double-quoted per ssh_config rules.
fn format_entry(entry: &HostEntry) -> String {
    format!(
        "\n{}\nHost {}\n    HostName {}\n    User {}\n    IdentityFile {}\n",
        CONNECTO_MARKER,
        entry.host,
        quote_value(&entry.hostname),
        quote_value(&entry.user),
        quote_value(&entry.identity_file)
    )
}

/// The lines of a managed block (marker + four config lines)
fn block_lines(entry: &HostEntry) -> Vec<String> {
    vec![
        CONNECTO_MARKER.to_string(),
        format!("Host {}", entry.host),
        format!("    HostName {}", quote_value(&entry.hostname)),
        format!("    User {}", quote_value(&entry.user)),
        format!("    IdentityFile {}", quote_value(&entry.identity_file)),
    ]
}

/// Parse all connecto-managed blocks out of the config lines
///
/// A managed block is a [`CONNECTO_MARKER`] line immediately followed by a
/// `Host` line with a single non-wildcard alias. The block ends at the first
/// blank line, the next marker, the next `Host`/`Match` section start
/// (case-insensitive, at any indentation, like ssh itself), or end of file —
/// trailing blocks at EOF are flushed. Indented option lines belong to the
/// block. Un-indented lines are treated conservatively: they belong to the
/// block only if they are a managed key (`HostName`/`User`/`IdentityFile`)
/// not yet seen in the block (legacy flat-format blocks); anything else —
/// e.g. a stray top-level directive right after the block — ends it, so
/// remove/replace never destroys adjacent user-authored configuration.
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
            let raw = &lines[j];
            let trimmed = raw.trim();
            if trimmed.is_empty() || trimmed == CONNECTO_MARKER || is_section_start(trimmed) {
                break;
            }
            let indented = raw.starts_with(char::is_whitespace);
            if let Some(value) = key_value(trimmed, "HostName") {
                if !indented && hostname_idx.is_some() {
                    break;
                }
                entry.hostname = value.to_string();
                hostname_idx = Some(j);
            } else if let Some(value) = key_value(trimmed, "User") {
                if !indented && !entry.user.is_empty() {
                    break;
                }
                entry.user = value.to_string();
            } else if let Some(value) = key_value(trimmed, "IdentityFile") {
                if !indented && !entry.identity_file.is_empty() {
                    break;
                }
                entry.identity_file = value.to_string();
            } else if !indented {
                // Un-indented and not a managed key: top-level user config,
                // not part of the block.
                break;
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

/// Whether a trimmed line starts a new top-level section
///
/// ssh_config has exactly two section keywords, `Host` and `Match`, and
/// matches keywords case-insensitively with whitespace or `=` after the
/// keyword. Either one always terminates the preceding block.
fn is_section_start(trimmed: &str) -> bool {
    let keyword = trimmed
        .split(|c: char| c.is_whitespace() || c == '=')
        .next()
        .unwrap_or("");
    keyword.eq_ignore_ascii_case("Host") || keyword.eq_ignore_ascii_case("Match")
}

/// If `trimmed` is `<keyword> <value>`, return the value
///
/// A value enclosed in double quotes (ssh_config quoting for values with
/// whitespace) is returned without the quotes.
fn key_value<'a>(trimmed: &'a str, keyword: &str) -> Option<&'a str> {
    let rest = trimmed.strip_prefix(keyword)?;
    if !rest.starts_with(char::is_whitespace) {
        return None;
    }
    let value = unquote_value(rest.trim());
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

/// Strip one pair of surrounding double quotes, if present
fn unquote_value(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .unwrap_or(value)
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

    // --- Finding [6]: block termination must not absorb adjacent user config ---

    /// A `Match` block directly after a managed block (no blank separator)
    /// must never be parsed into, updated, replaced, or removed with it.
    #[test]
    fn test_match_block_after_managed_block_is_never_touched() {
        let temp_dir = TempDir::new().unwrap();
        let config = config_in(&temp_dir);
        fs::create_dir_all(config.path().parent().unwrap()).unwrap();

        let match_block =
            "Match host secret-server\n    User root\n    IdentityFile /k/secret_personal\n";
        let managed = "# Added by connecto\nHost mydev\n    HostName 1.2.3.4\n    User dev\n    IdentityFile /k/connecto_mydev\n";
        fs::write(config.path(), format!("{managed}{match_block}")).unwrap();

        // The Match block's options must not leak into the managed entry.
        let hosts = config.list_hosts().unwrap();
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].user, "dev");
        assert_eq!(hosts[0].identity_file, "/k/connecto_mydev");

        // update_hostname leaves the Match block byte-intact.
        config
            .update_hostname("mydev", "10.0.0.9")
            .unwrap()
            .unwrap();
        assert!(fs::read_to_string(config.path())
            .unwrap()
            .contains(match_block));

        // Replace (re-pair) leaves the Match block byte-intact.
        let mut updated = entry("mydev");
        updated.identity_file = "/k/connecto_mydev".to_string();
        assert_eq!(config.add_host(&updated).unwrap(), AddOutcome::Replaced);
        assert!(fs::read_to_string(config.path())
            .unwrap()
            .contains(match_block));

        // Remove leaves exactly the Match block behind.
        config.remove_host("mydev").unwrap().unwrap();
        assert_eq!(fs::read_to_string(config.path()).unwrap(), match_block);
    }

    /// ssh keywords are case-insensitive: a lowercase `match`/`host` line
    /// must also terminate a managed block.
    #[test]
    fn test_lowercase_section_keywords_terminate_block() {
        let temp_dir = TempDir::new().unwrap();
        let config = config_in(&temp_dir);
        fs::create_dir_all(config.path().parent().unwrap()).unwrap();

        let tail = "match all\n    User root\nhost legacy\n    HostName old.example.com\n";
        fs::write(
            config.path(),
            format!(
                "# Added by connecto\nHost lc\n    HostName 1.2.3.4\n    User dev\n    IdentityFile /k/connecto_lc\n{tail}"
            ),
        )
        .unwrap();

        let hosts = config.list_hosts().unwrap();
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].user, "dev");

        config.remove_host("lc").unwrap().unwrap();
        assert_eq!(fs::read_to_string(config.path()).unwrap(), tail);
    }

    /// An un-indented stray top-level line (e.g. a global `IdentityFile`)
    /// directly after a complete managed block is not absorbed.
    #[test]
    fn test_unindented_stray_line_after_block_is_not_absorbed() {
        let temp_dir = TempDir::new().unwrap();
        let config = config_in(&temp_dir);
        fs::create_dir_all(config.path().parent().unwrap()).unwrap();

        let stray = "IdentityFile /k/global_personal\n";
        fs::write(
            config.path(),
            format!(
                "# Added by connecto\nHost dev2\n    HostName 1.2.3.4\n    User dev\n    IdentityFile /k/connecto_dev2\n{stray}"
            ),
        )
        .unwrap();

        let hosts = config.list_hosts().unwrap();
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].identity_file, "/k/connecto_dev2");

        config.remove_host("dev2").unwrap().unwrap();
        assert_eq!(fs::read_to_string(config.path()).unwrap(), stray);
    }

    // --- Finding [8]: replace preserves user-added options in the block ---

    #[test]
    fn test_replace_preserves_user_added_options_in_block() {
        let temp_dir = TempDir::new().unwrap();
        let config = config_in(&temp_dir);
        fs::create_dir_all(config.path().parent().unwrap()).unwrap();

        fs::write(
            config.path(),
            "# Added by connecto\nHost custom\n    HostName 1.2.3.4\n    User dev\n    IdentityFile /k/connecto_custom\n    Port 2222\n    ProxyJump bastion\n",
        )
        .unwrap();

        // Re-pair with a changed IP.
        let mut updated = entry("custom");
        updated.hostname = "10.0.0.7".to_string();
        assert_eq!(config.add_host(&updated).unwrap(), AddOutcome::Replaced);

        let content = fs::read_to_string(config.path()).unwrap();
        assert!(
            content.contains("    Port 2222"),
            "Port dropped:\n{content}"
        );
        assert!(
            content.contains("    ProxyJump bastion"),
            "ProxyJump dropped:\n{content}"
        );
        assert_eq!(content.matches(CONNECTO_MARKER).count(), 1);

        let hosts = config.list_hosts().unwrap();
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].hostname, "10.0.0.7");

        // A second replace must not duplicate the preserved options.
        assert_eq!(config.add_host(&updated).unwrap(), AddOutcome::Replaced);
        let content = fs::read_to_string(config.path()).unwrap();
        assert_eq!(content.matches("Port 2222").count(), 1);
        assert_eq!(content.matches("ProxyJump bastion").count(), 1);
    }

    // --- Finding [9]: remove_host removes all duplicate same-alias blocks ---

    #[test]
    fn test_remove_host_removes_all_duplicate_blocks() {
        let temp_dir = TempDir::new().unwrap();
        let config = config_in(&temp_dir);
        fs::create_dir_all(config.path().parent().unwrap()).unwrap();

        fs::write(
            config.path(),
            "# Added by connecto\nHost dup\n    HostName 1.1.1.1\n    User a\n    IdentityFile /k/dup1\n\n# Added by connecto\nHost dup\n    HostName 2.2.2.2\n    User b\n    IdentityFile /k/dup2\n",
        )
        .unwrap();
        assert_eq!(config.list_hosts().unwrap().len(), 2);

        // The first removed entry is returned, but BOTH blocks are removed.
        let removed = config.remove_host("dup").unwrap().unwrap();
        assert_eq!(removed.hostname, "1.1.1.1");

        assert!(config.list_hosts().unwrap().is_empty());
        let content = fs::read_to_string(config.path()).unwrap();
        assert!(!content.contains("dup"), "duplicate survived:\n{content}");
        assert!(!content.contains(CONNECTO_MARKER));
    }

    // --- Finding [10]: CRLF configs keep their line endings ---

    #[test]
    fn test_crlf_round_trip_preserves_line_endings() {
        let temp_dir = TempDir::new().unwrap();
        let config = config_in(&temp_dir);
        fs::create_dir_all(config.path().parent().unwrap()).unwrap();

        let user_block = "Host user\r\n    HostName example.com\r\n";
        fs::write(
            config.path(),
            format!(
                "{user_block}\r\n# Added by connecto\r\nHost crlf\r\n    HostName 1.2.3.4\r\n    User u\r\n    IdentityFile /k/connecto_crlf\r\n"
            ),
        )
        .unwrap();

        // Append a new entry: the whole file, including the new block, must
        // stay CRLF.
        config.add_host(&entry("extra")).unwrap();
        let content = fs::read_to_string(config.path()).unwrap();
        assert!(
            !content.replace("\r\n", "").contains('\n'),
            "bare LF introduced:\n{content:?}"
        );

        // Remove both managed entries: the untouched user block survives
        // byte-for-byte.
        config.remove_host("extra").unwrap().unwrap();
        config.remove_host("crlf").unwrap().unwrap();
        assert_eq!(fs::read_to_string(config.path()).unwrap(), user_block);

        // update_hostname also preserves CRLF.
        config.add_host(&entry("crlf2")).unwrap();
        config.update_hostname("crlf2", "9.9.9.9").unwrap().unwrap();
        let content = fs::read_to_string(config.path()).unwrap();
        assert!(content.starts_with(user_block));
        assert!(!content.replace("\r\n", "").contains('\n'));
    }

    // --- Finding [11]: IdentityFile paths with spaces are quoted ---

    #[test]
    fn test_identity_file_with_spaces_is_quoted_and_round_trips() {
        let temp_dir = TempDir::new().unwrap();
        let config = config_in(&temp_dir);

        let spacey = "/home/alice/my keys/connecto_spacey";
        let mut e = entry("spacey");
        e.identity_file = spacey.to_string();
        config.add_host(&e).unwrap();

        // The written line is double-quoted so ssh parses it as one value.
        let content = fs::read_to_string(config.path()).unwrap();
        assert!(
            content.contains("    IdentityFile \"/home/alice/my keys/connecto_spacey\""),
            "IdentityFile not quoted:\n{content}"
        );

        // The parser strips the quotes back off.
        let hosts = config.list_hosts().unwrap();
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].identity_file, spacey);

        // Replace and remove still find/round-trip the quoted entry.
        assert_eq!(config.add_host(&e).unwrap(), AddOutcome::Replaced);
        let removed = config.remove_host("spacey").unwrap().unwrap();
        assert_eq!(removed.identity_file, spacey);

        // Values without whitespace stay unquoted (legacy on-disk format).
        config.add_host(&entry("plain")).unwrap();
        let content = fs::read_to_string(config.path()).unwrap();
        assert!(!content.contains('"'));
    }
}
