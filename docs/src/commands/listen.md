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
| `--verify` | Require interactive approval of a verification code before installing a key |
| `-c, --continuous` | Keep listening after successful pairing |
| `--adhoc` | Create an ad-hoc WiFi network (bypasses router, for isolated networks) |
| `--bluetooth` | Enable Bluetooth Low Energy advertising (Linux only; requires a build with the `bluetooth` feature) |

## Examples

### Basic usage

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

### With verification (recommended on shared networks)

```bash
connecto listen --verify
```

Each pairing request must be approved before anything is installed:

```
Pairing approval required
  • Device:      my-laptop
  • Key comment: user@my-laptop
  • Fingerprint: SHA256:nThbg6kXUpJWGl7E1IGOCspRomTxdCARLviKw6E5SY8
  • Code:        627765
  Compare the code with the one shown on the pairing device.
Approve this pairing? [y/N]
```

The code is derived from the received key material on both sides
independently, so matching codes rule out a swapped key — see
[Security](../reference/security.md).

### Custom name and port

```bash
connecto listen --name workstation --port 9000
```

### Continuous mode

Keep listening for multiple pairings:

```bash
connecto listen --continuous
```

### Ad-hoc WiFi network

On networks with client isolation (devices can't see each other), `--adhoc`
creates a direct device-to-device WiFi network:

```bash
connecto listen --adhoc
```

Platform notes:

- **macOS**: modern macOS (14.4 and later) cannot create ad-hoc networks from
  the command line at all — Apple turned the `airport` utility into a no-op.
  Connecto detects this and fails fast with instructions for creating the
  network manually (Option-click the WiFi menu → Create Network).
- **Linux**: uses `nmcli` (with an `iw` fallback).
- **Windows**: uses `netsh wlan hostednetwork` (requires Administrator); the
  network password is printed.

Your previous WiFi network and DHCP configuration are restored when the
listener exits, including on Ctrl+C.

## What happens during pairing

1. Client connects and the two sides negotiate a protocol version
2. Client sends its public key
3. The listener validates the key and derives its fingerprint and 6-digit
   verification code
4. With `--verify`, the listener prompts for approval — nothing is installed
   if you decline. Without `--verify`, the key is installed automatically and
   the fingerprint/code of what was installed are printed.
5. Listener adds the key to `~/.ssh/authorized_keys` and sends back its
   username
6. Listener exits (or continues if `--continuous`)

If the final confirmation cannot be delivered after the key was installed,
the listener rolls the installation back. Probe connections (e.g. from
`connecto scan`) do not consume the one-shot session — the listener keeps
waiting until a pairing actually completes.

See [Protocol](../reference/protocol.md) for the full message flow.

## VPN/Cross-Subnet Detection

When a pairing comes from a different subnet, the listener displays a helpful message:

```
✓ Successfully paired with mac-laptop!
  → They can now SSH to this machine.

VPN/Cross-subnet connection detected!
  → Tell mac-laptop to save your subnet for future scans:
    connecto config add-subnet 10.0.1.0/24
```

## Security notes

- **Without `--verify`, any device on the network that completes the
  handshake gets its key installed.** Use `--verify` on any network with
  untrusted devices — see [Security](../reference/security.md).
- Only run `listen` when you intend to pair
- The listener only accepts SSH public keys (not arbitrary data)
- Stop the listener when done to prevent unwanted pairings

## Exit status

Like all connecto commands, `listen` exits non-zero when it fails (e.g. the
port cannot be bound), so it is safe to use in scripts.
