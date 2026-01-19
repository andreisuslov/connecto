# Connecto

**AirDrop-like SSH key pairing for your terminal.**

Connecto eliminates the hassle of manual SSH key setup. Instead of copying IP addresses and managing keys by hand, simply run `connecto listen` on one machine and `connecto pair` on another. Done.

## Features

- **mDNS Discovery**: Automatically discover devices on your local network
- **VPN Support**: Save subnets for cross-network discovery
- **Zero-config Pairing**: Exchange SSH keys with a single command
- **Auto SSH Config**: `ssh hostname` just works after pairing
- **Modern Cryptography**: Uses Ed25519 by default (RSA-4096 also supported)
- **Cross-platform**: Works on Linux, macOS, and Windows

## Quick Start

### On the Target Machine (where you want to SSH into)

```bash
connecto listen
```

### On the Client Machine (where you want to SSH from)

```bash
connecto scan
connecto pair 0
```

That's it! Now connect with:

```bash
ssh mydesktop
```

## VPN / Cross-Subnet Setup

If devices are on different subnets (e.g., VPN), save the remote subnet once:

```bash
# One-time setup
connecto config add-subnet 10.105.225.0/24

# Now scan finds devices on that subnet automatically
connecto scan
```

## Installation

### macOS (Homebrew)

```bash
brew install andreisuslov/connecto/connecto
```

### Windows (PowerShell)

```powershell
irm https://raw.githubusercontent.com/andreisuslov/connecto/main/install.ps1 | iex
```

### Windows (Chocolatey)

```cmd
choco install connecto
```

### From Source

```bash
git clone https://github.com/andreisuslov/connecto
cd connecto
cargo install --path connecto_cli
```

### Binary Releases

Download pre-built binaries from the [Releases](https://github.com/andreisuslov/connecto/releases) page.

## CLI Commands

### `connecto listen`

Start listening for pairing requests. Exits after one pairing by default.

```bash
connecto listen [OPTIONS]

Options:
  -p, --port <PORT>   Port to listen on [default: 8099]
  -n, --name <NAME>   Custom device name (defaults to hostname)
  -c, --continuous    Keep listening after first pairing
      --verify        Require verification code
  -v, --verbose       Enable verbose output
```

### `connecto scan`

Scan the local network for devices running Connecto.

```bash
connecto scan [OPTIONS]

Options:
  -t, --timeout <TIMEOUT>    Scan duration in seconds [default: 5]
  -s, --subnet <SUBNET>      Additional subnet to scan (can be repeated)
  -v, --verbose              Enable verbose output
```

### `connecto pair`

Pair with a discovered device. Automatically adds host to `~/.ssh/config`.

```bash
connecto pair <TARGET> [OPTIONS]

Arguments:
  <TARGET>  Device number from scan (0-indexed), or IP:port

Options:
  -c, --comment <COMMENT>  Custom key comment (defaults to user@hostname)
      --rsa                Generate RSA key instead of Ed25519
  -v, --verbose            Enable verbose output
```

### `connecto hosts`

List all paired hosts.

```bash
connecto hosts
```

```
Paired hosts:

  • mydesktop → user@192.168.1.55
  • workstation → admin@10.0.0.10

Connect with:
  → ssh <hostname>
```

### `connecto config`

Manage saved subnets for VPN/cross-subnet scanning.

```bash
connecto config add-subnet <CIDR>     # Add subnet (e.g., 10.0.0.0/24)
connecto config remove-subnet <CIDR>  # Remove subnet
connecto config list                  # Show saved subnets
connecto config path                  # Show config file location
```

### `connecto keys`

Manage authorized keys on this machine.

```bash
connecto keys list    # List all authorized keys
connecto keys remove  # Remove a key by number or pattern
```

### `connecto keygen`

Generate a new SSH key pair.

```bash
connecto keygen [OPTIONS]

Options:
  -n, --name <NAME>        Key name [default: connecto_key]
  -c, --comment <COMMENT>  Key comment
      --rsa                Generate RSA key instead of Ed25519
```

## How It Works

### Same Network (mDNS)

1. **Listen**: Target advertises via mDNS on port 8099
2. **Scan**: Client discovers devices via mDNS
3. **Pair**: Client sends public key, target adds to `~/.ssh/authorized_keys`
4. **Done**: Client's `~/.ssh/config` is auto-configured

### Different Networks (VPN)

1. **Save subnet**: `connecto config add-subnet 10.x.x.0/24`
2. **Listen**: Target starts listener
3. **Scan**: Client scans local subnets + saved subnets
4. **Pair**: Same as above

## Protocol

Connecto uses a simple JSON-over-TCP protocol:

1. **Hello**: Client sends version and device name
2. **HelloAck**: Server responds with version, name, and optional verification code
3. **KeyExchange**: Client sends its public key
4. **KeyAccepted**: Server confirms key installation
5. **PairingComplete**: Server sends SSH user for connection

## Security

- Keys are generated locally; only public keys are transmitted
- The pairing service only listens when explicitly started
- Optional verification codes prevent unauthorized pairing
- All communication happens over the local/VPN network

## Building from Source

### Requirements

- Rust 1.70+

### Build Commands

```bash
# Build
cargo build --release

# Run tests
cargo test --workspace

# Install
cargo install --path connecto_cli
```

## License

MIT License - see [LICENSE](LICENSE) for details.

## Contributing

Contributions are welcome! Please feel free to submit issues and pull requests.
