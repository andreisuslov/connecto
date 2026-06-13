# sync

Bidirectional SSH key pairing between two devices.

## Usage

```bash
connecto sync [OPTIONS]
```

## Description

The `sync` command enables two devices to simultaneously exchange SSH keys so both can SSH to each other. Unlike the `listen` + `pair` workflow which is one-directional (client can SSH to target), `sync` establishes bidirectional access.

Both devices run `connecto sync` at the same time, and they:

1. Advertise via mDNS (`_connecto-sync._tcp.local.`)
2. Scan for sync peers on the network
3. When found, exchange SSH public keys
4. Both add each other's key to `~/.ssh/authorized_keys` (only after both sides confirm the exchange)
5. Both can now SSH to each other

## Options

| Option | Description |
|--------|-------------|
| `-p, --port <PORT>` | Port to use for sync (default: 8099) |
| `-n, --name <NAME>` | Custom device name (default: hostname) |
| `-t, --timeout <SECS>` | Peer search timeout in seconds (default: 60) |
| `--rsa` | Use RSA-4096 key instead of Ed25519 |
| `-k, --key <PATH>` | Use existing SSH key instead of generating new one |

## Examples

### Basic usage

Run on both devices simultaneously:

```bash
# On Device A
connecto sync

# On Device B (at the same time)
connecto sync
```

Output on Device A:
```
  CONNECTO SYNC

→ Device name: device-a
→ Port: 8099
→ Timeout: 60s

Local IP addresses:
  • 192.168.1.100

→ Using Ed25519 key (modern, secure, fast)
→ Key saved: /Users/alice/.ssh/connecto_sync_device-a

Waiting for sync peer...
Run 'connecto sync' on another device on the same network
Press Ctrl+C to cancel

→ Found peer: Device B (192.168.1.101:8099)
→ Connected to Device B
→ Received key from Device B: bob@device-b
→ Our key was accepted by peer

✓ Sync completed with Device B!
  → Bidirectional SSH access established.
  → You can SSH to them, and they can SSH to you.

Sync Summary:
  • Peer: Device B
  • User: bob
  • Address: 192.168.1.101:8099

Next steps:
  → SSH to peer: ssh device-b

✓ Sync successful!
```

### With custom timeout

For slower networks:

```bash
connecto sync --timeout 120
```

### Using an existing key

```bash
connecto sync --key ~/.ssh/my_existing_key
```

`sync` resolves keys the same way `pair` does: `--key` flag first, then the
key configured via `connecto config set-default-key`, then a freshly
generated key. The `~/.ssh/config` entry written for the peer always points
at the key file that actually exists on disk.

### Using RSA instead of Ed25519

```bash
connecto sync --rsa
```

## How it works

1. **Both devices advertise**: each device registers a sync service via mDNS,
   publishing a random per-run priority as a TXT property (this is also how a
   device recognizes — and skips — its own advertisement)
2. **Both devices search**: each device also searches for other sync services
3. **Priority arbitration picks one direction**: whichever device connects
   sends `SyncHello` with its priority. The responder accepts only if the
   initiator's `(priority, device name)` pair strictly outranks its own;
   otherwise it declines and keeps listening, knowing its own outgoing
   attempt outranks the peer's. Exactly one direction wins, so starting
   `connecto sync` on both devices at the same time converges instead of
   hanging.
4. **Key exchange**: the winning initiator's `SyncHello` carries its public
   key; the responder replies with `SyncHelloAck` containing its own key
5. **Confirm, then install**: both sides exchange `SyncComplete` and only
   install the peer's key after the other side has confirmed — an aborted
   exchange leaves no key behind
6. **SSH config**: each side writes a connecto-managed `~/.ssh/config` entry
   for the peer

See [Protocol](../reference/protocol.md#sync-flow) for the exact message flow.

## Comparison with listen + pair

| Aspect | listen + pair | sync |
|--------|--------------|------|
| Direction | One-way | Bidirectional |
| Workflow | Run `listen` on target, `pair` on client | Run `sync` on both |
| Result | Client can SSH to target | Both can SSH to each other |
| Use case | Setting up access to a server | Two peers that need mutual access |

## Protocol messages

The sync protocol uses these message types:

- **SyncHello**: Contains version, device name, priority, public key, and SSH user
- **SyncHelloAck**: Response with the peer's public key and acceptance status
- **SyncComplete**: Final confirmation of success or failure (sent by both sides)

## Troubleshooting

### Timeout waiting for sync peer

- Ensure both devices are on the same network
- Check that mDNS/Bonjour is not blocked by firewall
- Try increasing the timeout: `connecto sync --timeout 120`

### Connection refused

- Make sure both devices start sync around the same time
- Check that port 8099 is not in use by another service
- Try a different port: `connecto sync --port 9000`

### Keys not being added

- Check write permissions on `~/.ssh/authorized_keys`
- Ensure `~/.ssh` directory exists with proper permissions (700)

## Security notes

- Sync only with trusted devices on your local network — sync has no
  equivalent of `listen --verify`, so any reachable peer that wins
  arbitration completes the exchange
- The sync protocol requires both parties to actively participate
- Keys are installed only after both sides confirm; a failed install on one
  side prevents the other side from installing too
- Keys are generated fresh for each sync (unless `--key` or a default key is
  configured)
- Only run sync when you intend to exchange keys with another device

## Exit status

`sync` exits non-zero when the sync fails or times out.
