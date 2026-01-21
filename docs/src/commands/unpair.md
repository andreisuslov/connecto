# unpair

Remove a paired host and delete its keys.

## Usage

```bash
connecto unpair <HOST>
```

## Arguments

| Argument | Description |
|----------|-------------|
| `HOST` | Name of the paired host to remove |

## Description

The `unpair` command removes a pairing established by Connecto:

1. Removes the host entry from `~/.ssh/config`
2. Deletes the private key (`~/.ssh/connecto_<host>`)
3. Deletes the public key (`~/.ssh/connecto_<host>.pub`)

## Example

```bash
connecto unpair mydesktop
```

Output:
```
  CONNECTO UNPAIR

→ Removing host: mydesktop

✓ Removed from ~/.ssh/config
✓ Deleted key: ~/.ssh/connecto_mydesktop
✓ Deleted key: ~/.ssh/connecto_mydesktop.pub

Host 'mydesktop' has been unpaired.
```

## Notes

- This only removes the local configuration
- The public key remains in the remote machine's `~/.ssh/authorized_keys`
- To fully revoke access, also remove the key from the remote machine

## Re-pairing

After unpairing, you can pair again:

```bash
connecto scan
connecto pair 0
```

A new key pair will be generated and exchanged.

## Related commands

| Command | Description |
|---------|-------------|
| `connecto hosts` | List all paired hosts |
| `connecto export` | Backup pairings before removing |
