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

1. Runs `ssh` against the host alias (so it uses your `~/.ssh/config` entry)
2. Uses `BatchMode` (no password prompts) and a 5-second connection timeout
3. Executes a trivial echo command and checks the response
4. Reports success or failure

## Example

### Successful test

```bash
connecto test mydesktop
```

Output:
```
→ Testing connection to mydesktop...
✓ Connection successful!
```

### Failed test

```bash
connecto test mydesktop
```

Output:
```
→ Testing connection to mydesktop...
✗ Connection failed.

Troubleshooting:
  • Check if the host is online
  • Verify the IP is correct: connecto hosts
  • Update IP if changed: connecto update-ip mydesktop <new-ip>
```

A failed test exits non-zero, so it can gate scripts:

```bash
connecto test mydesktop && rsync -a project/ mydesktop:project/
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
