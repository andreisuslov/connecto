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

If the subnet was not in the config, the command says so and exits non-zero.

---

## set-default-key

Set a default SSH key to use for all pairings (`connecto pair` and
`connecto sync`).

```bash
connecto config set-default-key ~/.ssh/id_ed25519
```

Output:
```
✓ Default key set: /Users/john/.ssh/id_ed25519
  → All future pairings will use this key.
```

Both the private key and its `.pub` file must exist; otherwise the command
fails with a non-zero exit status.

This is useful when you want to:
- Reuse your existing SSH key across all devices
- Use a single key for easier management
- Avoid generating multiple Connecto-specific keys

Note: `connecto unpair` never deletes keys that don't follow the
`connecto_*` naming convention, so your personal default key is safe.

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

Show all configuration (saved subnets and the default key, if set).

```bash
connecto config list
```

Output:
```
Configured subnets:
  • 10.0.2.0/24
  • 10.0.3.0/24
  • 192.168.100.0/24

Default SSH key:
  • /Users/john/.ssh/id_ed25519
```

---

## path

Show where the config file is stored.

```bash
connecto config path
```

Output:
```
/Users/john/Library/Application Support/com.connecto.connecto/config.json
```

### Config file locations

| Platform | Path |
|----------|------|
| macOS | `~/Library/Application Support/com.connecto.connecto/config.json` |
| Linux | `~/.config/connecto/config.json` |
| Windows | `%APPDATA%\connecto\connecto\config\config.json` |

---

## Config file format

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

## Use cases

### VPN Setup

When connecting to machines on a VPN:

```bash
# Save the VPN subnet once
connecto config add-subnet 10.0.2.0/24

# Now scans include that subnet automatically
connecto scan
```

### Multiple office networks

```bash
connecto config add-subnet 10.0.1.0/24   # Office A
connecto config add-subnet 10.0.2.0/24   # Office B
connecto config add-subnet 192.168.0.0/24 # Home
```

Scans will check all saved subnets regardless of which network you're on.

## Related commands

| Command | Description |
|---------|-------------|
| `connecto scan` | Scan for devices |
| `connecto scan --subnet` | One-time subnet scan |
