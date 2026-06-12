# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Connecto is an AirDrop-like SSH key pairing tool: `connecto listen` on the target machine, `connecto scan` + `connecto pair <n>` on the client, and SSH "just works". It is a Rust workspace with three crates:

- **connecto_core** — library: discovery, key management, pairing protocol, sync, fallbacks
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
cargo clippy --workspace -- -D warnings

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

Releases are tag-triggered (`.github/workflows/release.yml`). Distribution: Homebrew tap (`andreisuslov/connecto`), `install.ps1` for Windows, `packaging/chocolatey/`.

## Architecture

### connecto_core modules

- **discovery** — mDNS advertise/browse via `mdns-sd`. Service type `_connecto._tcp.local.`, default port 8099. `SubnetScanner` does direct TCP probing of CIDR ranges for when mDNS can't cross subnets (VPN scenarios).
- **protocol** — the pairing handshake: newline-delimited JSON `Message` enum over plain TCP. Flow: `Hello`/`HelloAck` (version check, optional verification code) → `KeyExchange` → `KeyAccepted` → `PairingComplete`. `HandshakeServer` runs on the listening side, `HandshakeClient` on the pairing side. Bump `PROTOCOL_VERSION` for incompatible changes.
- **keys** — key generation (Ed25519 default, RSA-4096 via `--rsa`) using the `ssh-key` crate; `KeyManager` installs received public keys into `~/.ssh/authorized_keys`.
- **sync** — bidirectional pairing (both machines exchange keys simultaneously). Uses a separate mDNS service type `_connecto-sync._tcp.local.` and random priorities to break the initiator tie.
- **fallback** — ad-hoc WiFi network creation/joining when normal networking fails. Heavily `cfg(target_os = ...)`-gated; shells out to platform tools (`networksetup` on macOS, etc.).
- **bluetooth** (feature `bluetooth`) — BLE discovery fallback. Scanning works on all platforms via `btleplug`; advertising is Linux-only via `bluer`. Custom GATT service whose characteristic encodes IP/port/name, so BLE is only a discovery transport — pairing still happens over TCP.

Long-running operations communicate through `tokio::sync::mpsc` channels of event enums (`DiscoveryEvent`, `ServerEvent`, `SyncEvent`, `BluetoothEvent`); the CLI and GUI subscribe and render these events.

### connecto_cli

`main.rs` defines the clap command tree; networked subcommands live in `src/commands/` (listen, scan, pair, sync, keys, keygen, ssh). Host-management commands (`hosts`, `unpair`, `update-ip`, `export`, `import`) are implemented directly in `main.rs` and work by text-parsing `~/.ssh/config`, relying on the `# Added by connecto` marker comment that `pair` writes. That marker is load-bearing — changing the SSH config entry format breaks all of these commands. App config (saved subnets, default key) lives in `src/config.rs`.

`commands/ssh.rs` manages the OS SSH server (systemd/launchd/Windows OpenSSH) and is platform-gated like `fallback`.

### connecto_gui

Tauri commands in `src-tauri/src/commands.rs` wrap connecto_core (shared state in `state.rs`); the React frontend in `src/` calls them via `@tauri-apps/api`. Note `connecto_gui/Cargo.toml` points its lib/bin at `src-tauri/src/`, so the crate root is not `src/`.

## Cross-platform constraints

All code must compile on macOS, Linux, and Windows — CI builds and tests all three. Platform-specific behavior goes behind `cfg(target_os)` / `cfg(unix)` / `cfg(windows)`; when adding such code, keep the other platforms compiling (stub or error, don't omit). Linux builds need system packages for the GUI (webkit2gtk, gtk-3, etc. — see `.github/workflows/ci.yml`).
