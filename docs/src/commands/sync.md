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
4. Both add each other's key to `~/.ssh/authorized_keys`
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

→ Generating Ed25519 key for sync...
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

### Using RSA instead of Ed25519

```bash
connecto sync --rsa
```

## How it works

1. **Both devices advertise**: Each device registers a sync service via mDNS
2. **Both devices search**: Each device also searches for other sync services
3. **Priority determines initiator**: Each device generates a random priority; the one that connects first becomes the initiator
4. **Key exchange**: Initiator sends `SyncHello` with its public key, responder replies with `SyncHelloAck` containing its key
5. **Mutual installation**: Both devices add the received key to their `authorized_keys`
6. **Confirmation**: Both send `SyncComplete` to confirm success

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
- **SyncComplete**: Final confirmation of success or failure

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

- Sync only with trusted devices on your local network
- The sync protocol requires both parties to actively participate
- Keys are generated fresh for each sync (unless `--key` is specified)
- Only run sync when you intend to exchange keys with another device
