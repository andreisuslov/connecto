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

## Example

```bash
connecto update-ip mydesktop 10.0.2.50
```

Output:
```
  CONNECTO UPDATE-IP

→ Updating IP for: mydesktop
→ Old IP: 192.168.1.55
→ New IP: 10.0.2.50

✓ Updated successfully!

You can now connect with:

  ssh mydesktop
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

## Related commands

| Command | Description |
|---------|-------------|
| `connecto hosts` | View current IP addresses |
| `connecto test` | Verify connection after update |
