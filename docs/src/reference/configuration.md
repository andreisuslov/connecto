# Configuration

Connecto stores configuration in platform-specific locations.

## Config File Location

| Platform | Path |
|----------|------|
| macOS | `~/.config/connecto/config.json` |
| Linux | `~/.config/connecto/config.json` |
| Windows | `%APPDATA%\connecto\config.json` |

Find your config path:
```bash
connecto config path
```

## Config File Format

```json
{
  "subnets": [
    "10.0.2.0/24",
    "192.168.100.0/24"
  ]
}
```

### Fields

| Field | Type | Description |
|-------|------|-------------|
| `subnets` | `string[]` | CIDR ranges to scan automatically |

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

### Entry Fields

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

### Key Files

For each paired host:
- `~/.ssh/connecto_<hostname>` - Private key
- `~/.ssh/connecto_<hostname>.pub` - Public key

### Key Type

Connecto generates Ed25519 keys by default:
- Modern elliptic curve cryptography
- 256-bit security level
- Small key size (compact `authorized_keys`)
- Fast generation and authentication

## Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `CONNECTO_PORT` | Override default port | `8099` |
| `CONNECTO_NAME` | Override device name | hostname |
| `HOME` | Home directory (Unix) | - |
| `USERPROFILE` | Home directory (Windows) | - |

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
