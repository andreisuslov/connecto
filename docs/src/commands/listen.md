# listen

Start a listener to accept pairing requests.

## Usage

```bash
connecto listen [OPTIONS]
```

## Description

The `listen` command starts a pairing listener on the current machine. It:

1. Advertises the device via mDNS on the local network
2. Waits for incoming pairing requests on TCP port 8099
3. Accepts public keys and adds them to `~/.ssh/authorized_keys`
4. Exits after successful pairing (unless `--continuous` is used)

## Options

| Option | Description |
|--------|-------------|
| `-p, --port <PORT>` | Port to listen on (default: 8099) |
| `-n, --name <NAME>` | Device name to advertise (default: hostname) |
| `-c, --continuous` | Keep listening after successful pairing |

## Examples

### Basic Usage

```bash
connecto listen
```

Output:
```
  CONNECTO LISTENER

→ Device name: mydesktop
→ Port: 8099

Local IP addresses:
  • 192.168.1.55

✓ mDNS service registered - device is now discoverable

Listening for pairing requests on port 8099...
```

### Custom Name and Port

```bash
connecto listen --name workstation --port 9000
```

### Continuous Mode

Keep listening for multiple pairings:

```bash
connecto listen --continuous
```

## What Happens During Pairing

1. Client connects and sends their public key
2. Listener adds the key to `~/.ssh/authorized_keys`
3. Listener sends back its hostname and username
4. Both sides confirm success
5. Listener exits (or continues if `--continuous`)

## VPN/Cross-Subnet Detection

When a pairing comes from a different subnet, the listener displays a helpful message:

```
✓ Successfully paired with mac-laptop!
  → They can now SSH to this machine.

VPN/Cross-subnet connection detected!
  → Tell mac-laptop to save your subnet for future scans:
    connecto config add-subnet 10.0.1.0/24
```

## Security Notes

- Only run `listen` when you intend to pair
- The listener only accepts SSH public keys (not arbitrary data)
- Keys are added to `authorized_keys` with a comment identifying Connecto
- Stop the listener when done to prevent unwanted pairings
