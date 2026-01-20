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

## What Gets Created

### SSH Key Pair

- **Private key**: `~/.ssh/connecto_<hostname>`
- **Public key**: `~/.ssh/connecto_<hostname>.pub`

Keys use Ed25519 by default (modern, secure, fast).

### SSH Config Entry

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

## After Pairing

Connect immediately:

```bash
ssh mydesktop
```

Or verify the pairing:

```bash
connecto test mydesktop
```
