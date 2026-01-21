# pair

Pair with a discovered device or direct IP.

## Usage

```bash
connecto pair <TARGET>
```

## Arguments

| Argument | Description |
|----------|-------------|
| `TARGET` | Device number from scan, or direct IP:port |

## Options

| Option | Description |
|--------|-------------|
| `-k, --key <PATH>` | Use existing SSH key instead of generating new |
| `-c, --comment <TEXT>` | Custom key comment |
| `--rsa` | Generate RSA-4096 instead of Ed25519 |

## Description

The `pair` command establishes SSH key-based authentication with a remote device:

1. Generates a new Ed25519 SSH key pair
2. Sends the public key to the target device
3. Saves the private key to `~/.ssh/connecto_<hostname>`
4. Updates `~/.ssh/config` for easy `ssh hostname` access

## Examples

### Pair by Device Number

After running `connecto scan`:

```bash
connecto pair 0
```

Output:
```
  CONNECTO PAIRING

→ Connecting to 192.168.1.55:8099...
→ Using Ed25519 key (modern, secure, fast)

✓ Pairing successful!

Key saved:
  • Private: /home/user/.ssh/connecto_mydesktop
  • Public:  /home/user/.ssh/connecto_mydesktop.pub

✓ Added to ~/.ssh/config as 'mydesktop'

You can now connect with:

  ssh mydesktop
```

### Pair by IP Address

Skip scanning and pair directly:

```bash
connecto pair 192.168.1.55:8099
```

Or with just the IP (uses default port 8099):

```bash
connecto pair 192.168.1.55
```

## What gets created

### SSH key pair

- **Private key**: `~/.ssh/connecto_<hostname>`
- **Public key**: `~/.ssh/connecto_<hostname>.pub`

Keys use Ed25519 by default (modern, secure, fast).

### SSH config entry

An entry is added to `~/.ssh/config`:

```
# Added by Connecto
Host mydesktop
    HostName 192.168.1.55
    User john
    IdentityFile ~/.ssh/connecto_mydesktop
    IdentitiesOnly yes
```

This allows simple `ssh mydesktop` without specifying user, IP, or key.

## Re-pairing

If you pair with a device that already has an entry:

1. The old key files are overwritten
2. The SSH config entry is updated
3. A new key exchange occurs

This is useful when:
- The remote machine was reinstalled
- You want to refresh the keys
- The IP address changed

## Using existing keys

Instead of generating a new key for each pairing, you can use an existing SSH key.

### One-time usage

```bash
connecto pair 0 --key ~/.ssh/id_ed25519
```

### Set default key

Set a default key for all future pairings:

```bash
connecto config set-default-key ~/.ssh/id_ed25519
```

Now all `connecto pair` commands will use this key automatically.

### Clear default key

Return to generating new keys:

```bash
connecto config clear-default-key
```

### Priority order

When pairing, Connecto looks for keys in this order:
1. `--key` flag (if specified)
2. Config default key (if set)
3. Generate new key (default behavior)

## After pairing

Connect immediately:

```bash
ssh mydesktop
```

Or verify the pairing:

```bash
connecto test mydesktop
```
