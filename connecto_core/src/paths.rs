//! Cross-platform path resolution
//!
//! Provides the single home-directory resolution strategy used throughout
//! Connecto, so the CLI and GUI agree on where `~/.ssh` and friends live on
//! every platform.

use crate::error::{ConnectoError, Result};
use directories::UserDirs;
use std::path::PathBuf;

/// Get the current user's home directory
///
/// Tries [`directories::UserDirs`] first, then falls back to the `HOME` and
/// `USERPROFILE` environment variables. Returns an error if none of these
/// yields a home directory.
pub fn home_dir() -> Result<PathBuf> {
    if let Some(dirs) = UserDirs::new() {
        return Ok(dirs.home_dir().to_path_buf());
    }

    for var in ["HOME", "USERPROFILE"] {
        if let Ok(value) = std::env::var(var) {
            if !value.is_empty() {
                return Ok(PathBuf::from(value));
            }
        }
    }

    Err(ConnectoError::Io(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        "Could not determine home directory (no user dirs, HOME, or USERPROFILE)",
    )))
}

/// Get the current user's SSH directory (`~/.ssh`)
pub fn ssh_dir() -> Result<PathBuf> {
    Ok(home_dir()?.join(".ssh"))
}

/// Expand a leading tilde in a path string
///
/// A bare `~` or a leading `~/` (or `~\` on Windows-style paths) is replaced
/// with the home directory from [`home_dir`]. All other paths, including
/// `~user/...` forms, are passed through unchanged.
pub fn expand_tilde(path: &str) -> Result<PathBuf> {
    if path == "~" {
        return home_dir();
    }
    if let Some(rest) = path.strip_prefix("~/").or_else(|| path.strip_prefix("~\\")) {
        return Ok(home_dir()?.join(rest));
    }
    Ok(PathBuf::from(path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_home_dir_resolves() {
        let home = home_dir().unwrap();
        assert!(!home.as_os_str().is_empty());
    }

    #[test]
    fn test_ssh_dir_is_under_home() {
        let ssh = ssh_dir().unwrap();
        assert!(ssh.starts_with(home_dir().unwrap()));
        assert!(ssh.ends_with(".ssh"));
    }

    #[test]
    fn test_expand_tilde_bare() {
        assert_eq!(expand_tilde("~").unwrap(), home_dir().unwrap());
    }

    #[test]
    fn test_expand_tilde_prefix() {
        let expanded = expand_tilde("~/.ssh/config").unwrap();
        assert_eq!(expanded, home_dir().unwrap().join(".ssh/config"));
    }

    #[test]
    fn test_expand_tilde_absolute_unchanged() {
        assert_eq!(
            expand_tilde("/etc/ssh/ssh_config").unwrap(),
            PathBuf::from("/etc/ssh/ssh_config")
        );
    }

    #[test]
    fn test_expand_tilde_relative_unchanged() {
        assert_eq!(expand_tilde("foo/bar").unwrap(), PathBuf::from("foo/bar"));
    }

    #[test]
    fn test_expand_tilde_user_form_unchanged() {
        // ~user expansion is not supported; pass through unchanged.
        assert_eq!(
            expand_tilde("~bob/.ssh").unwrap(),
            PathBuf::from("~bob/.ssh")
        );
    }
}
