# Protocol

Technical details of the Connecto pairing and sync protocols.

## Overview

Connecto runs two related protocols over plain TCP (default port 8099):

- **Pairing** (one-directional): `connecto pair` sends a public key to `connecto listen`
- **Sync** (bidirectional): two `connecto sync` instances exchange public keys

Both use the same wire format and the same `Message` set, defined in
`connecto_core::protocol`.

## Wire format

Every message is a single JSON object terminated by a newline:

```
{"type":"<MessageType>", ...fields}\n
```

- Messages are serialized with a `type` tag identifying the variant
- A single message may be at most **64 KiB** including the newline; longer
  lines are rejected and the connection is dropped
- Every protocol read is bounded by a **30-second timeout** (subnet-scan
  probes use a shorter 2-second timeout); a peer that goes silent cannot
  park a connection forever

## Version negotiation

| Constant | Value |
|----------|-------|
| `PROTOCOL_VERSION` | 1 |
| `MIN_SUPPORTED_VERSION` | 1 |

A peer is accepted when its advertised version lies in
`MIN_SUPPORTED_VERSION..=PROTOCOL_VERSION`. Each side replies with its own
version, so after the hello exchange both sides operate at the **minimum** of
the two versions. Versions outside the supported range are rejected with an
`Error` message (code 1); a newer peer is expected to retry at a version the
older side supports.

## Pairing flow

```
┌────────────┐                                ┌────────────┐
│   Client   │                                │  Listener  │
│  (pair)    │                                │  (listen)  │
└─────┬──────┘                                └──────┬─────┘
      │                                              │
      │──── TCP connect (port 8099) ────────────────>│
      │                                              │
      │──── Hello {version, device_name} ───────────>│
      │                                              │  version check
      │<─── HelloAck {version, device_name} ─────────│
      │                                              │
      │──── KeyExchange {public_key, comment} ──────>│
      │                                              │  validate key
      │                                              │  derive fingerprint + code
      │                                              │  approval (--verify)
      │                                              │  install key
      │<─── KeyAccepted {message} ───────────────────│
      │                                              │
      │<─── PairingComplete {ssh_user} ──────────────│
      │                                              │
      ×──────────── Connection closed ───────────────×
```

On the listening side, the received key is validated and its fingerprint and
verification code are derived **before** anything is installed. With
`--verify`, the operator must approve the request before installation; a
rejection sends `Error` (code 4) and installs nothing. If sending
`KeyAccepted`/`PairingComplete` fails after the key was installed, the
listener **rolls the installation back** so a half-completed exchange leaves
no SSH access behind.

## Messages

### Hello

Sent by the client to open a pairing handshake.

```json
{"type":"Hello","version":1,"device_name":"my-laptop"}
```

### HelloAck

The listener's reply. The `verification_code` field is a legacy v1 field and
is always `null`: a code invented by the listener and sent over the wire
proves nothing. The real verification code is derived from key material on
both sides (see below).

```json
{"type":"HelloAck","version":1,"device_name":"mydesktop","verification_code":null}
```

### KeyExchange

The client's SSH public key in OpenSSH format.

```json
{"type":"KeyExchange","public_key":"ssh-ed25519 AAAAC3... user@my-laptop","comment":"user@my-laptop"}
```

### KeyAccepted

Confirms the key was added to `authorized_keys`.

```json
{"type":"KeyAccepted","message":"Key added to authorized_keys"}
```

### PairingComplete

Carries the username to SSH in as.

```json
{"type":"PairingComplete","ssh_user":"john"}
```

### Error

Sent instead of the normal reply when something goes wrong; the connection is
closed afterwards.

```json
{"type":"Error","code":4,"message":"Pairing rejected by user"}
```

| Code | Meaning |
|------|---------|
| 1 | Unsupported protocol version |
| 2 | Expected `Hello` (wrong opening message) |
| 3 | Expected `KeyExchange` (wrong follow-up message) |
| 4 | Pairing rejected by the user (`--verify` prompt declined) |
| 5 | Invalid public key |

## Verification code

The 6-digit verification code shown during pairing is derived from the public
key itself:

1. Parse the OpenSSH public key
2. Compute its SHA-256 fingerprint digest
3. Interpret the first 4 bytes of the raw digest as a big-endian `u32`
4. Reduce modulo 1,000,000 and zero-pad to six digits

Both sides derive the code **independently**: the pairing side from the key
it *sent*, the listening side from the key it *received*. The codes match
only if the same key crossed the wire, so a man-in-the-middle that
substitutes its own key changes the code displayed on the listening side.
With `connecto listen --verify`, installation is gated on the operator
confirming this code; without it the code is printed but not enforced (see
[Security](./security.md)).

## Sync flow

Sync is bidirectional: both devices run `connecto sync`, advertise on a
dedicated mDNS service type, browse for each other, and exchange keys over a
single connection.

```
┌────────────┐                                       ┌────────────┐
│ Initiator  │                                       │ Responder  │
└─────┬──────┘                                       └──────┬─────┘
      │                                                     │
      │── SyncHello {version, device_name,                  │
      │      initiator_priority, public_key,                │
      │      key_comment, ssh_user} ──────────────────────>│
      │                                                     │ version check
      │                                                     │ arbitration
      │<─ SyncHelloAck {version, device_name, public_key,   │
      │      key_comment, ssh_user, accept_sync} ──────────│
      │                                                     │
      │── SyncComplete {success:true} ────────────────────>│
      │                                                     │ install initiator's key
      │<─ SyncComplete {success:true} ─────────────────────│
      │                                                     │
      │ install responder's key                             │
      ×──────────────── Connection closed ──────────────────×
```

Key properties:

- **Priority arbitration**: each run generates a random `u64` priority. The
  responder accepts an incoming `SyncHello` only if the initiator's
  `(priority, device name)` pair strictly outranks its own; otherwise it
  replies `accept_sync: false` and keeps listening, because its own
  initiator role outranks the peer's and will be accepted on the other side.
  Exactly one direction wins, so running `connecto sync` on both devices
  simultaneously converges instead of deadlocking.
- **Self-identification**: each device publishes its per-run priority as an
  mDNS TXT property (`priority`), and skips advertisements that match both
  its own device name and its own priority — so a device never tries to sync
  with itself, while two distinct devices that happen to share a name still
  find each other.
- **Install after confirmation**: neither side installs the peer's key until
  the protocol confirms the exchange. The responder installs only after the
  initiator's `SyncComplete`; the initiator installs only after the
  responder's `SyncComplete`. If the responder's key installation fails, it
  reports `SyncComplete {success: false}` so the initiator does not install
  either; if the responder's confirmation cannot be sent after it installed,
  it rolls the installation back.

## Discovery

### mDNS

| Protocol | Service type | TXT records |
|----------|--------------|-------------|
| Pairing | `_connecto._tcp.local.` | none |
| Sync | `_connecto-sync._tcp.local.` | `priority=<random u64>` |

Devices respond to mDNS queries on UDP port 5353.

### Subnet scanning

For cross-subnet discovery (VPNs, mDNS-blocking networks), Connecto scans IP
ranges directly:

1. Generate the list of IPs from CIDR (e.g., `10.0.2.0/24` → 254 IPs);
   local `10.x.x.x` addresses are widened to a /22
2. Attempt a TCP connection to port 8099 on each IP
3. Up to 100 concurrent connections, 500ms connect timeout each
4. Each open port is probed with a real `Hello` → `HelloAck` exchange
   (2-second read timeout), so only actual Connecto listeners are reported

A probe stops after `HelloAck` — it never sends a key — so being scanned
cannot pair anything. The one-shot listener keeps accepting connections until
a pairing actually completes, so probes do not consume the session.

## Security considerations

See [Security](./security.md) for the full trust model. In short:

- The pairing channel is **plaintext TCP** — but only *public* keys cross it;
  private keys never leave the machine that generated them
- Without `--verify`, a listener auto-accepts any peer that completes the
  handshake; with `--verify`, installation is gated on a code derived from
  the received key material
- mDNS device names are not authenticated; verify the code, not the name
