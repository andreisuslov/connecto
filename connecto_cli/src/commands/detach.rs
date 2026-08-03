//! Detached process spawning for `connecto listen --detach`.
//!
//! Running `connecto listen` over SSH is a natural thing to want: you are already
//! on the target machine and you want to add another key to it. But on Windows the
//! listener dies the moment the SSH session closes, which surfaces as a confusing
//! "connection actively refused" on the pairing side even though the listener
//! reported `Server started on 0.0.0.0:8099` seconds earlier.
//!
//! Windows OpenSSH terminates the processes belonging to a session when the
//! connection drops. Measured on Windows Server 2025, both of the obvious escapes
//! fail - a child spawned with `DETACHED_PROCESS`, and one spawned with
//! `DETACHED_PROCESS | CREATE_BREAKAWAY_FROM_JOB`, are both gone once the session
//! ends, even though `CreateProcess` itself succeeds in both cases. Creating the
//! process through WMI does survive, because `Win32_Process.Create` is executed by
//! the WMI service, so the new process never belongs to the SSH session at all.
//!
//! On Unix the problem does not arise in the same way; putting the child in its own
//! process group is enough to detach it from the terminal.

use anyhow::{anyhow, Result};
use std::path::PathBuf;

/// Rebuild the `listen` argument vector, minus `--detach`.
///
/// Reconstructing from the parsed values rather than replaying the original argv
/// keeps quoting predictable and guarantees `--detach` cannot be passed through to
/// the child, which would fork forever.
pub fn listen_argv(
    port: u16,
    name: &Option<String>,
    verify: bool,
    continuous: bool,
    adhoc: bool,
    bluetooth: bool,
) -> Vec<String> {
    let mut args = vec!["listen".to_string(), "--port".to_string(), port.to_string()];
    if let Some(n) = name {
        args.push("--name".to_string());
        args.push(n.clone());
    }
    if verify {
        args.push("--verify".to_string());
    }
    if continuous {
        args.push("--continuous".to_string());
    }
    if adhoc {
        args.push("--adhoc".to_string());
    }
    if bluetooth {
        args.push("--bluetooth".to_string());
    }
    args
}

/// Quote one argument for a Windows command line.
#[cfg(windows)]
fn quote_win(arg: &str) -> String {
    if !arg.is_empty() && !arg.contains([' ', '\t', '"']) {
        return arg.to_string();
    }
    let mut out = String::from("\"");
    let mut backslashes = 0usize;
    for c in arg.chars() {
        match c {
            '\\' => {
                backslashes += 1;
                out.push('\\');
            }
            '"' => {
                // Escape the run of backslashes preceding the quote, then the quote.
                for _ in 0..=backslashes {
                    out.push('\\');
                }
                backslashes = 0;
                out.push('"');
            }
            _ => {
                backslashes = 0;
                out.push(c);
            }
        }
    }
    // Backslashes immediately before the closing quote must also be doubled.
    for _ in 0..backslashes {
        out.push('\\');
    }
    out.push('"');
    out
}

/// Spawn `exe args...` fully detached from the current session. Returns its PID.
pub fn spawn_detached(exe: PathBuf, args: &[String]) -> Result<u32> {
    #[cfg(windows)]
    {
        spawn_detached_windows(exe, args)
    }
    #[cfg(not(windows))]
    {
        spawn_detached_unix(exe, args)
    }
}

#[cfg(windows)]
fn spawn_detached_windows(exe: PathBuf, args: &[String]) -> Result<u32> {
    use std::process::Command;

    let mut parts = vec![quote_win(&exe.to_string_lossy())];
    parts.extend(args.iter().map(|a| quote_win(a)));
    let command_line = parts.join(" ");

    // Single-quoted PowerShell string: the only escape needed is doubling `'`.
    let escaped = command_line.replace('\'', "''");
    let script = format!(
        "$ErrorActionPreference='Stop'; \
         $r = Invoke-CimMethod -ClassName Win32_Process -MethodName Create \
              -Arguments @{{ CommandLine = '{escaped}' }}; \
         if ($r.ReturnValue -ne 0) {{ Write-Error \"Win32_Process.Create returned $($r.ReturnValue)\"; exit 1 }} \
         else {{ Write-Output $r.ProcessId }}"
    );

    let output = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .output()
        .map_err(|e| anyhow!("could not run powershell to detach the listener: {e}"))?;

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!(
            "detaching the listener failed: {}",
            err.trim().lines().next().unwrap_or("unknown error")
        ));
    }

    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse::<u32>()
        .map_err(|_| anyhow!("detached the listener but could not read back its process id"))
}

#[cfg(not(windows))]
fn spawn_detached_unix(exe: PathBuf, args: &[String]) -> Result<u32> {
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};

    let child = Command::new(exe)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .map_err(|e| anyhow!("could not start the detached listener: {e}"))?;

    Ok(child.id())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn argv_defaults_are_minimal() {
        let a = listen_argv(8099, &None, false, false, false, false);
        assert_eq!(a, vec!["listen", "--port", "8099"]);
    }

    #[test]
    fn argv_includes_every_flag_that_was_set() {
        let a = listen_argv(9000, &Some("box one".into()), true, true, true, true);
        assert_eq!(
            a,
            vec![
                "listen",
                "--port",
                "9000",
                "--name",
                "box one",
                "--verify",
                "--continuous",
                "--adhoc",
                "--bluetooth",
            ]
        );
    }

    #[test]
    fn argv_never_propagates_detach() {
        // A child that inherited --detach would spawn another child forever.
        let a = listen_argv(8099, &Some("x".into()), true, true, true, true);
        assert!(!a.iter().any(|s| s == "--detach"));
    }

    #[cfg(windows)]
    #[test]
    fn quoting_leaves_simple_arguments_alone() {
        assert_eq!(quote_win("listen"), "listen");
        assert_eq!(quote_win("--port"), "--port");
    }

    #[cfg(windows)]
    #[test]
    fn quoting_wraps_paths_containing_spaces() {
        assert_eq!(
            quote_win(r"C:\Program Files\connecto\connecto.exe"),
            "\"C:\\Program Files\\connecto\\connecto.exe\""
        );
    }

    #[cfg(windows)]
    #[test]
    fn quoting_escapes_embedded_quotes() {
        assert_eq!(quote_win(r#"a"b"#), r#""a\"b""#);
    }

    #[cfg(windows)]
    #[test]
    fn quoting_leaves_a_trailing_backslash_alone_when_no_quoting_is_needed() {
        // Nothing to escape: with no spaces or quotes the argument is passed through
        // verbatim, so the trailing backslash is harmless.
        assert_eq!(quote_win(r"c:\dir\"), r"c:\dir\");
    }

    #[cfg(windows)]
    #[test]
    fn quoting_doubles_trailing_backslashes_inside_quotes() {
        // This is the case that actually matters: once the argument is wrapped in
        // quotes, a trailing backslash would escape the closing quote unless doubled.
        assert_eq!(quote_win(r"c:\my dir\"), "\"c:\\my dir\\\\\"");
    }
}
