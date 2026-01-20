# config

Manage Connecto configuration.

## Usage

```bash
connecto config <SUBCOMMAND>
```

## Subcommands

| Subcommand | Description |
|------------|-------------|
| `add-subnet <CIDR>` | Add a subnet to scan automatically |
| `remove-subnet <CIDR>` | Remove a saved subnet |
| `list` | List all saved subnets |
| `path` | Show config file location |

---

## add-subnet

Add a subnet that will be scanned automatically.

```bash
connecto config add-subnet 10.0.2.0/24
```

Output:
```
✓ Added subnet: 10.0.2.0/24
```

Useful for VPN networks where mDNS doesn't work across subnets.

---

## remove-subnet

Remove a previously saved subnet.

```bash
connecto config remove-subnet 10.0.2.0/24
```

Output:
```
✓ Removed subnet: 10.0.2.0/24
```

---

## list

Show all configured subnets.

```bash
connecto config list
```

Output:
```
Configured subnets:
  • 10.0.2.0/24
  • 10.0.3.0/24
  • 192.168.100.0/24
```

---

## path

Show where the config file is stored.

```bash
connecto config path
```

Output:
```
Config file: /Users/john/.config/connecto/config.json
```

### Config File Locations

| Platform | Path |
|----------|------|
| macOS | `~/.config/connecto/config.json` |
| Linux | `~/.config/connecto/config.json` |
| Windows | `%APPDATA%\connecto\config.json` |

---

## Config File Format

The config file is JSON:

```json
{
  "subnets": [
    "10.0.2.0/24",
    "10.0.3.0/24"
  ]
}
```

You can edit it manually, but using the `connecto config` commands is recommended.

---

## Use Cases

### VPN Setup

When connecting to machines on a VPN:

```bash
# Save the VPN subnet once
connecto config add-subnet 10.0.2.0/24

# Now scans include that subnet automatically
connecto scan
```

### Multiple Office Networks

```bash
connecto config add-subnet 10.0.1.0/24   # Office A
connecto config add-subnet 10.0.2.0/24   # Office B
connecto config add-subnet 192.168.0.0/24 # Home
```

Scans will check all saved subnets regardless of which network you're on.

## Related Commands

| Command | Description |
|---------|-------------|
| `connecto scan` | Scan for devices |
| `connecto scan --subnet` | One-time subnet scan |
