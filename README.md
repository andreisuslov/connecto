# Connecto

**AirDrop-like SSH key pairing for your terminal.**

Connecto eliminates the hassle of manual SSH key setup. Instead of copying IP addresses and managing keys by hand, simply run `connecto listen` on one machine and `connecto pair` on another.

## Quick Start

**On the target machine** (where you want to SSH into):
```bash
connecto listen
```

**On the client machine** (where you want to SSH from):
```bash
connecto scan
connecto pair 0
ssh mydesktop  # It just works!
```

## Installation

```bash
# macOS
brew install andreisuslov/connecto/connecto

# Windows (PowerShell as Admin)
irm https://raw.githubusercontent.com/andreisuslov/connecto/main/install.ps1 | iex

# From source
cargo install --path connecto_cli
```

## Documentation

Full documentation is available at **[andreisuslov.github.io/connecto](https://andreisuslov.github.io/connecto/)**

- [Installation Guide](https://andreisuslov.github.io/connecto/getting-started/installation.html)
- [Quick Start](https://andreisuslov.github.io/connecto/getting-started/quickstart.html)
- [VPN/Cross-Subnet Setup](https://andreisuslov.github.io/connecto/getting-started/vpn-setup.html)
- [All Commands](https://andreisuslov.github.io/connecto/commands/listen.html)
- [Security](https://andreisuslov.github.io/connecto/reference/security.html)

## SSH Server Setup

Before other machines can SSH into your device, the SSH server must be enabled:

```bash
connecto ssh on      # Enable SSH server
connecto ssh status  # Check SSH server status
connecto ssh off     # Disable SSH server
```

**Platform-specific notes:**
- **Windows**: Run PowerShell as Administrator
- **macOS/Linux**: Run with `sudo`

## Features

- **mDNS Discovery** - Auto-discover devices on your network
- **VPN Support** - Cross-subnet scanning
- **Zero-config Pairing** - One command key exchange
- **Auto SSH Config** - `ssh hostname` just works
- **Cross-platform** - macOS, Linux, Windows
- **SSH Server Management** - Enable/disable SSH server with one command on all platforms

## License

MIT
