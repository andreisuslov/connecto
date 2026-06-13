# hosts

List all paired hosts.

## Usage

```bash
connecto hosts
```

## Description

The `hosts` command displays all devices you've paired with using Connecto.
It reads `~/.ssh/config` and lists the entries marked `# Added by connecto`;
hand-written host blocks are not shown.

## Example

```bash
connecto hosts
```

Output:
```
Paired hosts:

  • mydesktop → john@192.168.1.55
  • workstation → admin@10.0.2.100
  • laptop → alice@192.168.1.42

Connect with:
  → ssh <hostname>
```

## Output fields

| Field | Description |
|-------|-------------|
| Host alias | Name to use with the `ssh` command |
| User | Username for SSH connection |
| Address | IP address or hostname of the remote machine |

## Related commands

| Command | Description |
|---------|-------------|
| `connecto test <host>` | Test SSH connection |
| `connecto update-ip <host> <ip>` | Update host's IP address |
| `connecto unpair <host>` | Remove pairing |
| `connecto export` | Backup all pairings |
