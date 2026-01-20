# hosts

List all paired hosts.

## Usage

```bash
connecto hosts
```

## Description

The `hosts` command displays all devices you've paired with using Connecto. It reads from `~/.ssh/config` and shows hosts that have Connecto-generated keys.

## Example

```bash
connecto hosts
```

Output:
```
  PAIRED HOSTS

[0] mydesktop
    → Host: 192.168.1.55
    → User: john
    → Key:  ~/.ssh/connecto_mydesktop

[1] workstation
    → Host: 10.0.2.100
    → User: admin
    → Key:  ~/.ssh/connecto_workstation

[2] laptop
    → Host: 192.168.1.42
    → User: alice
    → Key:  ~/.ssh/connecto_laptop
```

## Output Fields

| Field | Description |
|-------|-------------|
| Host | IP address or hostname of the remote machine |
| User | Username for SSH connection |
| Key | Path to the private key file |

## Related Commands

| Command | Description |
|---------|-------------|
| `connecto test <host>` | Test SSH connection |
| `connecto update-ip <host> <ip>` | Update host's IP address |
| `connecto unpair <host>` | Remove pairing |
| `connecto export` | Backup all pairings |
