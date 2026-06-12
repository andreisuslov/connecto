//! Filesystem utilities
//!
//! Provides atomic file writes for modules that persist user-facing state
//! (SSH config entries, user configuration). Writing to a temporary file and
//! renaming it over the target guarantees readers never observe a partially
//! written file, even if the process is interrupted mid-write.

use crate::error::Result;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

/// Write `contents` to `path` atomically
///
/// The data is first written to a temporary file in the same directory as
/// `path` (so the final rename never crosses a filesystem boundary), then
/// renamed over the target. On Unix the temporary file is created with mode
/// `0o600` *before* any content is written, so sensitive data never touches
/// disk with looser permissions. On Windows, where renaming over an existing
/// file can fail, the existing target is removed first if the rename fails.
pub fn write_atomic(path: &Path, contents: &str) -> Result<()> {
    let parent = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => PathBuf::from("."),
    };

    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".to_string());
    let tmp_path = parent.join(format!(
        ".{}.connecto-tmp.{}.{}",
        file_name,
        std::process::id(),
        rand::random::<u32>()
    ));

    let result = write_and_rename(&tmp_path, path, contents);
    if result.is_err() {
        // Best-effort cleanup; the original error is what matters.
        let _ = fs::remove_file(&tmp_path);
    }
    result
}

fn write_and_rename(tmp_path: &Path, path: &Path, contents: &str) -> Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }

    let mut file = options.open(tmp_path)?;
    file.write_all(contents.as_bytes())?;
    file.sync_all()?;
    drop(file);

    match fs::rename(tmp_path, path) {
        Ok(()) => Ok(()),
        Err(rename_err) => {
            // Windows cannot always rename over an existing file; remove the
            // target and retry once.
            #[cfg(windows)]
            {
                if path.exists() {
                    fs::remove_file(path)?;
                    fs::rename(tmp_path, path)?;
                    return Ok(());
                }
            }
            Err(rename_err.into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_write_atomic_creates_file() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("config");

        write_atomic(&path, "hello world").unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "hello world");
    }

    #[test]
    fn test_write_atomic_overwrites_existing() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("config");

        write_atomic(&path, "first").unwrap();
        write_atomic(&path, "second").unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "second");
    }

    #[test]
    fn test_write_atomic_empty_contents() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("empty");

        write_atomic(&path, "").unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "");
    }

    #[test]
    fn test_write_atomic_leaves_no_temp_files() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("config");

        write_atomic(&path, "data").unwrap();

        let entries: Vec<_> = fs::read_dir(temp_dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0], "config");
    }

    #[test]
    fn test_write_atomic_missing_parent_fails() {
        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("does-not-exist").join("config");

        assert!(write_atomic(&path, "data").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn test_write_atomic_sets_0600_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("secret");

        write_atomic(&path, "private").unwrap();

        let mode = fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}
