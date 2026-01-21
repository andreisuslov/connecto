# keys

Manage SSH keys.

## GUI key management

The Connecto GUI provides a full-featured key management interface in the **Keys** tab:

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

## CLI key management

The CLI key management commands are planned for a future release.

### Planned features

#### List keys

```bash
connecto keys list
```

Show all Connecto-generated SSH keys.

#### Rotate keys

```bash
connecto keys rotate <HOST>
```

Generate a new key pair for a host and update the remote `authorized_keys`.

#### Key info

```bash
connecto keys info <HOST>
```

Display key details (type, fingerprint, creation date).

## Current CLI workarounds

### List Connecto keys

```bash
ls -la ~/.ssh/connecto_*
```

### View key fingerprint

```bash
ssh-keygen -lf ~/.ssh/connecto_mydesktop.pub
```

### Manual key rotation

1. Unpair the host: `connecto unpair mydesktop`
2. Re-pair: `connecto scan && connecto pair 0`

## Related commands

| Command | Description |
|---------|-------------|
| `connecto hosts` | List paired hosts |
| `connecto unpair` | Remove pairing |
| `connecto pair` | Establish new pairing |
