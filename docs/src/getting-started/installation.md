# Installation

Connecto can be installed on macOS, Linux, and Windows.

## macOS

### Homebrew (Recommended)

```bash
brew install andreisuslov/connecto/connecto
```

### Binary download

Download the latest release from [GitHub Releases](https://github.com/andreisuslov/connecto/releases):

```bash
# Apple Silicon (M1/M2/M3)
curl -LO https://github.com/andreisuslov/connecto/releases/latest/download/connecto-aarch64-apple-darwin.tar.gz
tar xzf connecto-aarch64-apple-darwin.tar.gz
sudo mv connecto /usr/local/bin/

# Intel Mac
curl -LO https://github.com/andreisuslov/connecto/releases/latest/download/connecto-x86_64-apple-darwin.tar.gz
tar xzf connecto-x86_64-apple-darwin.tar.gz
sudo mv connecto /usr/local/bin/
```

## Windows

### PowerShell (Recommended)

Run in PowerShell as Administrator:

```powershell
irm https://raw.githubusercontent.com/andreisuslov/connecto/main/install.ps1 | iex
```

This will:
- Download the latest release
- Install to `C:\Program Files\Connecto`
- Add to PATH
- Configure firewall rules for mDNS and the Connecto port

### Chocolatey

```cmd
choco install connecto
```

### Manual installation

1. Download `connecto-x86_64-pc-windows-msvc.zip` from [GitHub Releases](https://github.com/andreisuslov/connecto/releases)
2. Extract to `C:\Program Files\Connecto`
3. Add to PATH

## Linux

### Binary download

```bash
# x86_64
curl -LO https://github.com/andreisuslov/connecto/releases/latest/download/connecto-x86_64-unknown-linux-gnu.tar.gz
tar xzf connecto-x86_64-unknown-linux-gnu.tar.gz
sudo mv connecto /usr/local/bin/
```

### From source

Requires Rust 1.70+:

```bash
git clone https://github.com/andreisuslov/connecto
cd connecto
cargo install --path connecto_cli
```

## Verify installation

```bash
connecto --version
```

## Shell completions

Enable tab completion for your shell:

```bash
# Bash
connecto completions bash >> ~/.bashrc

# Zsh
connecto completions zsh >> ~/.zshrc

# Fish
connecto completions fish > ~/.config/fish/completions/connecto.fish

# PowerShell
connecto completions powershell >> $PROFILE
```

Restart your shell or source the config file.

## Firewall configuration

Connecto uses:
- **UDP 5353** for mDNS discovery
- **TCP 8099** for the pairing protocol

### Linux (iptables)

```bash
sudo iptables -A INPUT -p udp --dport 5353 -j ACCEPT
sudo iptables -A INPUT -p tcp --dport 8099 -j ACCEPT
```

### Linux (firewalld)

```bash
sudo firewall-cmd --add-port=5353/udp --permanent
sudo firewall-cmd --add-port=8099/tcp --permanent
sudo firewall-cmd --reload
```

### macOS

macOS typically allows these connections by default. If needed, add rules in System Preferences > Security & Privacy > Firewall.

### Windows

The PowerShell installer automatically configures firewall rules. For manual setup:

```powershell
New-NetFirewallRule -DisplayName "Connecto mDNS" -Direction Inbound -Protocol UDP -LocalPort 5353 -Action Allow
New-NetFirewallRule -DisplayName "Connecto TCP" -Direction Inbound -Protocol TCP -LocalPort 8099 -Action Allow
```
