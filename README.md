# Connecto

**AirDrop-like SSH key pairing for your terminal.**

Connecto eliminates the hassle of manual SSH key setup. Instead of copying IP addresses and managing keys by hand, simply run `connecto listen` on one machine and `connecto pair` on another. Done.

## Features

- **mDNS Discovery**: Automatically discover devices on your local network
- **Zero-config Pairing**: Exchange SSH keys with a single command
- **Modern Cryptography**: Uses Ed25519 by default (RSA-4096 also supported)
- **Cross-platform**: Works on Linux, macOS, and Windows
- **GUI & CLI**: Use the terminal or the graphical interface

## Quick Start

### On the Target Machine (where you want to SSH into)

```bash
connecto listen
```

```
  CONNECTO LISTENER

-> Device name: MyDesktop
-> Port: 8099

Local IP addresses:
  * 192.168.1.55

 mDNS service registered - device is now discoverable

Listening for pairing requests on port 8099...
Press Ctrl+C to stop
```

### On the Client Machine (where you want to SSH from)

```bash
connecto scan
```

```
  CONNECTO SCANNER

-> Scanning for 5 seconds...

 Found 1 device(s):

[1] MyDesktop (mydesktop) (192.168.1.55:8099)

To pair with a device, run: connecto pair <number>
```

```bash
connecto pair 1
```

```
  CONNECTO PAIRING

-> Connecting to 192.168.1.55:8099...
-> Using Ed25519 key (modern, secure, fast)

 Pairing successful!

Key saved:
  * Private: /home/user/.ssh/connecto_mydesktop
  * Public:  /home/user/.ssh/connecto_mydesktop.pub

You can now connect with:

  ssh -i /home/user/.ssh/connecto_mydesktop user@192.168.1.55
```

## Installation

### From Source

```bash
# Clone the repository
git clone https://github.com/yourusername/connecto
cd connecto

# Build and install
cargo install --path connecto_cli
```

### Binary Releases

Download pre-built binaries from the [Releases](https://github.com/yourusername/connecto/releases) page.

## CLI Commands

### `connecto listen`

Start listening for pairing requests.

```bash
connecto listen [OPTIONS]

Options:
  -p, --port <PORT>   Port to listen on [default: 8099]
  -n, --name <NAME>   Custom device name (defaults to hostname)
      --verify        Require verification code
      --once          Handle only one pairing request and exit
  -v, --verbose       Enable verbose output
```

### `connecto scan`

Scan the local network for devices running Connecto.

```bash
connecto scan [OPTIONS]

Options:
  -t, --timeout <TIMEOUT>  How long to scan in seconds [default: 5]
  -v, --verbose            Enable verbose output
```

### `connecto pair`

Pair with a discovered device.

```bash
connecto pair <TARGET> [OPTIONS]

Arguments:
  <TARGET>  Device number from scan results, or IP:port address

Options:
  -c, --comment <COMMENT>  Custom key comment (defaults to user@hostname)
      --rsa                Generate RSA key instead of Ed25519
  -v, --verbose            Enable verbose output
```

### `connecto keys`

Manage authorized keys on this machine.

```bash
connecto keys [COMMAND]

Commands:
  list    List all authorized keys
  remove  Remove a key by number or pattern
```

### `connecto keygen`

Generate a new SSH key pair.

```bash
connecto keygen [OPTIONS]

Options:
  -n, --name <NAME>        Key name (stored in ~/.ssh/) [default: connecto_key]
  -c, --comment <COMMENT>  Key comment
      --rsa                Generate RSA key instead of Ed25519
```

## Architecture

Connecto is built as a Rust workspace with three components:

```
connecto/
├── connecto_core/    # Core library (mDNS, SSH keys, protocol)
├── connecto_cli/     # CLI application
└── connecto_gui/     # Tauri GUI application
```

### connecto_core

The heart of Connecto, providing:

- **discovery**: mDNS-based device discovery using `mdns-sd`
- **keys**: SSH key generation and management using `ssh-key`
- **protocol**: JSON-based handshake protocol over TCP

### connecto_cli

A terminal interface built with:

- `clap` for argument parsing
- `colored` for terminal colors
- `indicatif` for progress spinners
- `dialoguer` for interactive prompts

### connecto_gui

A cross-platform GUI built with Tauri, featuring:

- Device scanning and pairing
- Key management
- Modern, dark-themed interface

## Protocol

Connecto uses a simple JSON-over-TCP protocol for key exchange:

1. **Hello**: Client sends version and device name
2. **HelloAck**: Server responds with version, name, and optional verification code
3. **KeyExchange**: Client sends its public key
4. **KeyAccepted**: Server confirms key installation
5. **PairingComplete**: Server sends SSH user for connection command

## Security Considerations

- Keys are generated locally and never leave the device (only public keys are transmitted)
- The pairing service only listens when explicitly started
- Optional verification codes can prevent unauthorized pairing
- All communication happens over the local network

## Building from Source

### Requirements

- Rust 1.70+
- For GUI: WebKitGTK development libraries (Linux)

### Build Commands

```bash
# Build everything
cargo build --release

# Run tests
cargo test --workspace

# Build only CLI
cargo build -p connecto_cli --release

# Build GUI (requires system dependencies)
# Uncomment connecto_gui in Cargo.toml first
cargo build -p connecto_gui --release
```

### Linux GUI Dependencies

For the GUI on Linux, install:

```bash
# Ubuntu/Debian
sudo apt install libwebkit2gtk-4.0-dev libgtk-3-dev libappindicator3-dev

# Fedora
sudo dnf install webkit2gtk3-devel gtk3-devel libappindicator-gtk3-devel

# Arch
sudo pacman -S webkit2gtk gtk3 libappindicator-gtk3
```

## License

MIT License - see [LICENSE](LICENSE) for details.

## Contributing

Contributions are welcome! Please feel free to submit issues and pull requests.
