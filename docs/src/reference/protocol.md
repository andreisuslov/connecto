# Protocol

Technical details of the Connecto pairing protocol.

## Overview

Connecto uses a simple TCP-based protocol for key exchange:

```
┌────────────┐                    ┌────────────┐
│   Client   │                    │  Listener  │
│  (pair)    │                    │  (listen)  │
└─────┬──────┘                    └──────┬─────┘
      │                                   │
      │──── TCP Connect (port 8099) ─────>│
      │                                   │
      │──── HELLO <version> ─────────────>│
      │                                   │
      │<──── HELLO <version> ─────────────│
      │                                   │
      │──── PUBKEY <ssh-public-key> ─────>│
      │                                   │
      │<──── OK <hostname> <user> ────────│
      │                                   │
      │──── BYE ─────────────────────────>│
      │                                   │
      ×─────── Connection Closed ─────────×
```

## Message format

All messages are newline-terminated strings:

```
COMMAND [ARGUMENTS...]\n
```

## Messages

### HELLO

Version handshake.

```
HELLO 1
```

| Field | Description |
|-------|-------------|
| Version | Protocol version (currently `1`) |

### PUBKEY

Client sends their SSH public key.

```
PUBKEY ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIG... user@hostname
```

| Field | Description |
|-------|-------------|
| Key | Full SSH public key in OpenSSH format |

### OK

Listener confirms successful pairing.

```
OK mydesktop john
```

| Field | Description |
|-------|-------------|
| Hostname | Listener's hostname |
| User | Username for SSH connection |

### ERR

Error response.

```
ERR Invalid public key format
```

| Field | Description |
|-------|-------------|
| Message | Human-readable error description |

### BYE

Connection closing.

```
BYE
```

## Discovery

### mDNS

Connecto advertises via mDNS:

| Field | Value |
|-------|-------|
| Service Type | `_connecto._tcp` |
| Port | 8099 |
| TXT Records | `version=1` |

Devices respond to mDNS queries on UDP port 5353.

### Subnet scanning

For cross-subnet discovery, Connecto scans IP ranges:

1. Generate list of IPs from CIDR (e.g., `10.0.2.0/24` → 254 IPs)
2. Attempt TCP connection to port 8099 on each IP
3. 100 concurrent connections, 500ms timeout each
4. Valid listeners respond to HELLO

## Security considerations

### What's protected

- **Authentication**: SSH keys provide cryptographic authentication
- **Integrity**: SSH protocol ensures connection integrity
- **Authorization**: Keys only added with physical/network access

### What's not protected

- **Initial Exchange**: The pairing protocol itself is unencrypted
- **Network Eavesdropping**: Public keys are sent in plaintext (this is safe - they're public)
- **Man-in-the-Middle**: No certificate verification during pairing

### Recommendations

- Only run `listen` on trusted networks
- Verify the IP address before pairing
- Use SSH host key verification after pairing
- Review `authorized_keys` periodically

## Wire format example

Complete pairing session:

```
CLIENT: HELLO 1
SERVER: HELLO 1
CLIENT: PUBKEY ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIKx... user@laptop
SERVER: OK desktop john
CLIENT: BYE
[connection closed]
```

## Future considerations

Potential protocol enhancements:
- TLS encryption for the pairing channel
- Challenge-response verification
- QR code / out-of-band verification
- Key rotation protocol
