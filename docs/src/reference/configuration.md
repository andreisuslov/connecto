# Configuration

Connecto stores configuration in platform-specific locations.

## Config file location

| Platform | Path |
|----------|------|
| macOS | `~/.config/connecto/config.json` |
| Linux | `~/.config/connecto/config.json` |
| Windows | `%APPDATA%\connecto\config.json` |

Find your config path:
```bash
connecto config path
```

## Config file format

```json
{
  "subnets": [
    "10.0.2.0/24",
    "192.168.100.0/24"
  ],
  "default_key": "/Users/john/.ssh/id_ed25519"
}
```

### Fields

| Field | Type | Description |
|-------|------|-------------|
| `subnets` | `string[]` | CIDR ranges to scan automatically |
| `default_key` | `string?` | Path to default SSH key for pairing (optional) |

## SSH Configuration

Connecto modifies `~/.ssh/config` when pairing. Each paired host gets an entry:

```
# Added by Connecto
Host mydesktop
    HostName 192.168.1.55
    User john
    IdentityFile ~/.ssh/connecto_mydesktop
    IdentitiesOnly yes
```

### Entry fields

| Field | Description |
|-------|-------------|
| `Host` | Alias used with `ssh` command |
| `HostName` | IP address or hostname |
| `User` | Remote username |
| `IdentityFile` | Path to private key |
| `IdentitiesOnly` | Use only the specified key |

## SSH Keys

Keys are stored in the SSH directory:

| Platform | Directory |
|----------|-----------|
| macOS/Linux | `~/.ssh/` |
| Windows | `%USERPROFILE%\.ssh\` |

### Key files

For each paired host:
- `~/.ssh/connecto_<hostname>` - Private key
- `~/.ssh/connecto_<hostname>.pub` - Public key

### Key type

Connecto generates Ed25519 keys by default:
- Modern elliptic curve cryptography
- 256-bit security level
- Small key size (compact `authorized_keys`)
- Fast generation and authentication

## Environment variables

| Variable | Description |
|----------|-------------|
| `HOME` | Home directory (Unix) - used to find `~/.ssh` |
| `USERPROFILE` | Home directory (Windows) - used to find `.ssh` |

## Ports

| Port | Protocol | Purpose |
|------|----------|---------|
| 5353 | UDP | mDNS discovery |
| 8099 | TCP | Pairing protocol |

## authorized_keys Format

When accepting a pairing, Connecto adds to `~/.ssh/authorized_keys`:

```
ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIG... connecto_laptop_2024-01-15
```

The comment includes:
- `connecto_` prefix for identification
- Source hostname
- Date of pairing
