# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Connecto is an AirDrop-like SSH key pairing tool: `connecto listen` on the target machine, `connecto scan` + `connecto pair <n>` on the client, and SSH "just works". It is a Rust workspace with three crates:

- **connecto_core** — library: discovery, key management, pairing protocol, sync, SSH config editing, fallbacks
- **connecto_cli** — the `connecto` binary (clap)
- **connecto_gui** — Tauri 1.x desktop app with a React/Vite/Tailwind v4 frontend

## Commands

```bash
# Build / test core + CLI (most common during development)
cargo build -p connecto_core -p connecto_cli
cargo test -p connecto_core -p connecto_cli

# Run a single test
cargo test -p connecto_core test_name
cargo test -p connecto_core --test integration_tests   # integration tests only

# Run the CLI
cargo run -p connecto_cli -- scan

# Lint — CI enforces both; clippy warnings fail the build
cargo fmt --all -- --check
cargo clippy -p connecto_core -p connecto_cli --all-targets -- -D warnings

# Bluetooth support is feature-gated (off by default)
cargo build -p connecto_cli --features bluetooth
cargo test -p connecto_core --features bluetooth
```

**Full workspace builds require the GUI frontend first** — `tauri-build` needs `connecto_gui/dist` to exist:

```bash
cd connecto_gui && npm ci && npm run build   # then: cargo build --workspace
```

GUI development: `npm run dev` is wired into `tauri dev` via `beforeDevCommand` (devPath `http://localhost:5173`).

Docs are an mdBook in `docs/` (`mdbook serve docs`), published to GitHub Pages via `.github/workflows/docs.yml`.

CI (`.github/workflows/ci.yml`) is scoped to `-p connecto_core -p connecto_cli` (build + test on all 3 OSes, clippy/rustfmt lint job), plus a dedicated `bluetooth` job on Ubuntu that clippy-checks and tests with `--features bluetooth`. The GUI builds only in the path-filtered `.github/workflows/gui.yml` (triggers on `connecto_gui/**` changes), so core/CLI changes don't pay the Tauri/npm cost.

Releases are tag-triggered (`.github/workflows/release.yml`); the tag must match the workspace version in `Cargo.toml`, and the workflow stamps `package.json`/`tauri.conf.json`/`connecto.nuspec` at build time. Artifacts: `connecto-macos-{x86_64,aarch64}.tar.gz`, `connecto-x86_64-unknown-linux-gnu.tar.gz`, `connecto-windows-x86_64.zip` (each with a `.sha256`). Distribution: Homebrew tap (`andreisuslov/connecto`), `install.ps1` for Windows (installs to `%LOCALAPPDATA%\connecto`), `packaging/chocolatey/`.

## Architecture

### connecto_core modules

- **discovery** — mDNS advertise/browse via `mdns-sd`. Service type `_connecto._tcp.local.`, default port 8099. `SubnetScanner` does direct TCP probing of CIDR ranges for when mDNS can't cross subnets (VPN scenarios); probes speak the real protocol (`Hello`/`HelloAck`) using `PROTOCOL_VERSION`.
- **protocol** — the pairing handshake: newline-delimited JSON `Message` enum over plain TCP, framed by the shared `write_message`/`read_message` helpers (64 KiB line cap, 30s read timeout). Flow: `Hello`/`HelloAck` → `KeyExchange` → `KeyAccepted` → `PairingComplete`, with `Error {code, message}` on failure. Version policy lives in `negotiate_version` (`MIN_SUPPORTED_VERSION..=PROTOCOL_VERSION`, effective = min). The 6-digit verification code is derived from the public key's SHA-256 fingerprint (`verification_code_for_key`) independently on both sides — never sent over the wire. `HandshakeServer` supports an `ApprovalCallback` (the CLI's `--verify`) consulted before the key is installed, and rolls the install back if the post-install confirmation fails.
- **keys** — key generation (Ed25519 default, RSA-4096 via `--rsa`) using the `ssh-key` crate; `KeyManager` installs received public keys into `~/.ssh/authorized_keys` (atomic writes, dedup) and exposes `fingerprint_sha256`.
- **sync** — bidirectional pairing (both machines exchange keys simultaneously). Separate mDNS service type `_connecto-sync._tcp.local.`; each run publishes a random priority as a TXT property (self-recognition). Responder-side arbitration (`initiator_wins`: strict `(priority, name)` ordering) makes simultaneous mutual initiation converge. Keys are installed only after both sides confirm (`SyncComplete` both ways), with rollback on a failed confirm.
- **ssh_config** — THE single parser/writer for connecto-managed `~/.ssh/config` blocks. Owns the `CONNECTO_MARKER` constant (`# Added by connecto`), `HostEntry`, and `SshConfig::{list_hosts, add_host, remove_host, update_hostname}`. Only marker-preceded blocks are ever touched; user-authored blocks are invisible to it. **No other code may parse or write ~/.ssh/config.**
- **paths** — the one home/`~/.ssh` resolution strategy (`home_dir`, `ssh_dir`, `expand_tilde`); use it instead of reading `HOME`/`USERPROFILE` directly.
- **device_name** — `sanitize_device_name`, the single sanitizer for key file names and SSH aliases.
- **user_config** — persistent user `Config` (saved subnets, `default_key`), stored via `ProjectDirs("com","connecto","connecto")` as `config.json`. Shared by CLI and GUI. The scan device cache uses the same qualifiers' cache dir (`devices.json`), not `/tmp`.
- **fsutil** — `write_atomic` (temp file + rename) used for authorized_keys, ssh config, and user config writes.
- **fallback** — ad-hoc WiFi network creation/joining when normal networking fails. `fallback.rs` holds the cross-platform `AdHocNetwork` orchestration (sanitize, lifecycle, Drop restore) over a private `AdHocBackend` trait; per-OS command primitives live in `fallback/{macos,linux,windows}.rs`. On macOS ≥ 14.4 creating ad-hoc networks is impossible (`airport` is a no-op stub) — the backend fails fast with manual instructions. Backends always restore previous WiFi/DHCP state.
- **bluetooth** (feature `bluetooth`) — BLE discovery fallback. Scanning works on all platforms via `btleplug`; advertising is Linux-only via `bluer` (the feature activates `bluer` in Cargo.toml). Custom GATT service whose characteristic encodes IP/port/name, so BLE is only a discovery transport — pairing still happens over TCP.

Long-running operations communicate through `tokio::sync::mpsc` channels of event enums (`DiscoveryEvent`, `ServerEvent`, `SyncEvent`); the CLI and GUI subscribe and render these events. `HandshakeServer::run` shuts down when every event receiver is dropped.

### connecto_cli

`main.rs` defines the clap command tree and the `SilentExit` error type; every subcommand implementation lives in `src/commands/`. **Exit-code pattern**: failure sites print their rich colored diagnostics themselves and return `SilentExit`; `main` recognizes it via downcast, skips re-printing, and exits 1. All commands exit non-zero on failure — never print a red ✗ and return `Ok(())`.

Host-management commands (`hosts`, `unpair`, `update-ip`, `export`, `import`) are in `commands/hosts.rs` and go exclusively through `connecto_core::SshConfig`; they take the `SshConfig` as a parameter so tests run against temp dirs. `unpair` deletes key files only when they follow the `connecto_*`/`connecto_sync_*` naming convention; anything else is left in place with a message. Key resolution for `pair`/`sync`/`keygen` is the shared `commands::resolve_key_pair` (`--key` > config `default_key` > generate); `keygen` deliberately passes an empty config so the default key never applies. Shared output helpers (`success`/`error`/`info`/`warn`/`spinner`) live in `commands/mod.rs`.

`commands/ssh.rs` manages the OS SSH server with per-platform submodules in `commands/ssh/{macos,linux,windows}.rs` (compile-time gated; Linux doubles as the generic Unix fallback).

### connecto_gui

Tauri commands in `src-tauri/src/commands.rs` wrap connecto_core (shared state in `state.rs`); the React frontend in `src/` calls them via `@tauri-apps/api`. Note `connecto_gui/Cargo.toml` points its lib/bin at `src-tauri/src/`, so the crate root is not `src/`.

## Cross-platform constraints

All code must compile on macOS, Linux, and Windows — CI builds and tests all three. Platform-specific behavior goes behind `cfg(target_os)` / `cfg(unix)` / `cfg(windows)`; when adding such code, keep the other platforms compiling (stub or error, don't omit). Linux builds need system packages for the GUI (webkit2gtk, gtk-3, etc. — see `.github/workflows/gui.yml`) and `libdbus-1-dev` for the bluetooth feature (see the CI bluetooth job).
