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
| `set-default-key <PATH>` | Set default SSH key for pairing |
| `clear-default-key` | Clear the default SSH key |
| `list` | List all configuration |
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

## set-default-key

Set a default SSH key to use for all pairings.

```bash
connecto config set-default-key ~/.ssh/id_ed25519
```

Output:
```
✓ Default key set: /Users/john/.ssh/id_ed25519
  → All future pairings will use this key.
```

This is useful when you want to:
- Reuse your existing SSH key across all devices
- Use a single key for easier management
- Avoid generating multiple Connecto-specific keys

---

## clear-default-key

Remove the default SSH key setting.

```bash
connecto config clear-default-key
```

Output:
```
✓ Default key cleared.
  → Pairings will generate new keys again.
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
  ],
  "default_key": "/Users/john/.ssh/id_ed25519"
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
