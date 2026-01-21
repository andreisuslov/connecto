# Connecto

**AirDrop-like SSH key pairing for your terminal.**

Connecto eliminates the hassle of manual SSH key setup. Instead of copying IP addresses and managing keys by hand, simply run `connecto listen` on one machine and `connecto pair` on another. Done.

## Features

- **mDNS Discovery** - Automatically discover devices on your local network
- **VPN Support** - Save subnets for cross-network discovery
- **Zero-config Pairing** - Exchange SSH keys with a single command
- **Auto SSH Config** - `ssh hostname` just works after pairing
- **Modern Cryptography** - Uses Ed25519 by default (RSA-4096 also supported)
- **Cross-platform** - Works on Linux, macOS, and Windows

## How it works

```
┌─────────────────┐                    ┌─────────────────┐
│  Target Machine │                    │  Client Machine │
│                 │                    │                 │
│  connecto listen│◄───── mDNS ───────►│  connecto scan  │
│                 │                    │                 │
│                 │◄── TCP/8099 ──────►│  connecto pair 0│
│                 │                    │                 │
│  authorized_keys│                    │  ~/.ssh/config  │
│     updated     │                    │    updated      │
└─────────────────┘                    └─────────────────┘
                                              │
                                              ▼
                                       ssh mydesktop ✓
```

1. **Target** runs `connecto listen` - advertises via mDNS
2. **Client** runs `connecto scan` - discovers available devices
3. **Client** runs `connecto pair 0` - exchanges SSH keys
4. **Done** - `ssh hostname` just works

## Quick example

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

## Next steps

- [Installation](./getting-started/installation.md) - Install Connecto on your system
- [Quick Start](./getting-started/quickstart.md) - Get up and running in minutes
- [VPN Setup](./getting-started/vpn-setup.md) - Configure for VPN/cross-subnet use
