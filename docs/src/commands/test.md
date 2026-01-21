# test

Test SSH connection to a paired host.

## Usage

```bash
connecto test <HOST>
```

## Arguments

| Argument | Description |
|----------|-------------|
| `HOST` | Name of the paired host to test |

## Description

The `test` command verifies that SSH connectivity works to a paired host. It:

1. Looks up the host in `~/.ssh/config`
2. Attempts an SSH connection
3. Runs a simple command (`echo "Connecto test successful"`)
4. Reports success or failure

## Example

### Successful test

```bash
connecto test mydesktop
```

Output:
```
  CONNECTO TEST

→ Testing connection to: mydesktop

✓ Connection successful!
  → SSH to mydesktop is working.
```

### Failed test

```bash
connecto test mydesktop
```

Output:
```
  CONNECTO TEST

→ Testing connection to: mydesktop

✗ Connection failed!
  → Error: Connection refused

Troubleshooting:
  • Check if the host is online
  • Verify the IP address: connecto hosts
  • Update IP if changed: connecto update-ip mydesktop <new-ip>
```

## Common issues

| Error | Cause | Solution |
|-------|-------|----------|
| Connection refused | Host offline or SSH not running | Start the remote machine |
| Connection timed out | Wrong IP or network issue | Update IP with `connecto update-ip` |
| Permission denied | Key not in authorized_keys | Re-pair with `connecto pair` |
| Host key verification failed | Remote host changed | Remove from `~/.ssh/known_hosts` |

## Related commands

| Command | Description |
|---------|-------------|
| `connecto hosts` | List all paired hosts |
| `connecto update-ip` | Update host's IP address |
| `connecto pair` | Re-establish pairing |
