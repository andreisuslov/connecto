# update-ip

Update the IP address for a paired host.

## Usage

```bash
connecto update-ip <HOST> <IP>
```

## Arguments

| Argument | Description |
|----------|-------------|
| `HOST` | Name of the paired host |
| `IP` | New IP address |

## Description

The `update-ip` command changes the IP address for a paired host in `~/.ssh/config`. This is useful when:

- A device gets a new DHCP lease
- You're switching between networks (home/office)
- The VPN assigns a different IP

The SSH keys remain valid - only the IP changes.

`update-ip` only modifies entries marked `# Added by connecto`. Hand-written
host blocks are never rewritten, even if they share the same alias.

## Example

```bash
connecto update-ip mydesktop 10.0.2.50
```

Output:
```
✓ Updated 'mydesktop' IP: 192.168.1.55 → 10.0.2.50
```

## Finding the New IP

### On the Remote Machine

```bash
# Linux/macOS
ip addr show | grep inet

# Windows
ipconfig
```

### Using Connecto scan

If the remote is running `connecto listen`:

```bash
connecto scan
```

The scan results show the current IP.

## Notes

- The SSH keys are not affected
- You don't need to re-pair after updating the IP
- Consider using static IPs or hostnames for frequently-changing devices

## Exit status

`update-ip` exits non-zero if the host is not found among the
connecto-managed entries (or no SSH config exists).

## Related commands

| Command | Description |
|---------|-------------|
| `connecto hosts` | View current IP addresses |
| `connecto test` | Verify connection after update |
