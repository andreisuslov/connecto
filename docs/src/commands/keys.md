# keys

Manage SSH keys.

## CLI key management

### List authorized keys

List the keys in this machine's `~/.ssh/authorized_keys` (the keys that are
allowed to SSH in):

```bash
connecto keys          # same as 'connecto keys list'
connecto keys list
```

Output:
```
  AUTHORIZED KEYS

2 authorized key(s) found:

[1] ssh-ed25519 AAAAC3NzaC...IGOCspRomTx alice@laptop
[2] ssh-ed25519 AAAAC3NzaC...w6E5SY8nThb bob@desktop

To remove a key: connecto keys remove <number>
```

### Remove an authorized key

Remove a key by its number from the list, or by a search pattern matched
against the key's comment/type:

```bash
connecto keys remove 2
connecto keys remove alice@laptop
```

You are shown the key and asked to confirm before it is removed. If a pattern
matches multiple keys, the matches are listed and nothing is removed — be
more specific.

This revokes that device's SSH access to this machine.

### Generate a key pair

```bash
connecto keygen [OPTIONS]
```

| Option | Description |
|--------|-------------|
| `-n, --name <NAME>` | Key file name in `~/.ssh/` (default: `connecto_key`) |
| `-c, --comment <TEXT>` | Key comment (default: `user@hostname`) |
| `--rsa` | Generate RSA-4096 instead of Ed25519 |

`keygen` always generates a fresh key — a configured default key
(`connecto config set-default-key`) does not apply here.

```bash
connecto keygen --name connecto_work --comment "work laptop"
```

## GUI key management

The Connecto GUI provides a key management interface in the **Keys** tab:

### Authorized keys

View and manage SSH keys that are authorized to connect to this machine. You can:
- View key algorithm, fingerprint, and comment
- Remove keys to revoke access

### Local keys

View and manage SSH key pairs stored in `~/.ssh/`:
- **List keys**: See all local key pairs with algorithm, comment, and fingerprint
- **Copy path**: Copy the public key path to clipboard
- **Rename**: Rename key files (both private and public)
- **Delete**: Remove key pairs permanently

### Generate new key

Create new SSH key pairs:
- Choose between Ed25519 (default) and RSA-4096
- Set custom key name and comment
- Keys are saved to `~/.ssh/`

## Useful shell commands

### List Connecto-generated key files

```bash
ls -la ~/.ssh/connecto_*
```

### View key fingerprint

```bash
ssh-keygen -lf ~/.ssh/connecto_mydesktop.pub
```

### Key rotation

1. Unpair the host: `connecto unpair mydesktop`
2. Re-pair: `connecto scan && connecto pair 0`

## Related commands

| Command | Description |
|---------|-------------|
| `connecto hosts` | List paired hosts |
| `connecto unpair` | Remove pairing |
| `connecto pair` | Establish new pairing |
