# keys

Manage SSH keys (future feature).

## Status

This command is planned for a future release.

## Planned Features

### List Keys

```bash
connecto keys list
```

Show all Connecto-generated SSH keys.

### Rotate Keys

```bash
connecto keys rotate <HOST>
```

Generate a new key pair for a host and update the remote `authorized_keys`.

### Key Info

```bash
connecto keys info <HOST>
```

Display key details (type, fingerprint, creation date).

## Current Workarounds

### List Connecto Keys

```bash
ls -la ~/.ssh/connecto_*
```

### View Key Fingerprint

```bash
ssh-keygen -lf ~/.ssh/connecto_mydesktop.pub
```

### Manual Key Rotation

1. Unpair the host: `connecto unpair mydesktop`
2. Re-pair: `connecto scan && connecto pair 0`

## Related Commands

| Command | Description |
|---------|-------------|
| `connecto hosts` | List paired hosts |
| `connecto unpair` | Remove pairing |
| `connecto pair` | Establish new pairing |
