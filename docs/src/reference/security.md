# Security

Security model and best practices for Connecto.

## Threat Model

### Protected Against

| Threat | Protection |
|--------|------------|
| Password guessing | SSH key authentication only |
| Credential theft | Private keys never leave the device |
| Replay attacks | SSH protocol cryptographic protection |
| Network sniffing | SSH encrypts all traffic after pairing |

### Not Protected Against

| Threat | Mitigation |
|--------|------------|
| Malicious network access | Only pair on trusted networks |
| Physical device access | Use full-disk encryption |
| Compromised endpoints | Keep systems updated |

## Key Security

### Key Generation

- **Algorithm**: Ed25519 (elliptic curve)
- **Security level**: 128-bit equivalent
- **Key size**: 256-bit private, 256-bit public

Ed25519 advantages:
- No known practical attacks
- Resistant to timing attacks
- Small, fast signatures
- Widely supported (OpenSSH 6.5+)

### Key Storage

| Component | Location | Permissions |
|-----------|----------|-------------|
| Private key | `~/.ssh/connecto_*` | 600 (owner read/write) |
| Public key | `~/.ssh/connecto_*.pub` | 644 (world readable) |
| Authorized keys | `~/.ssh/authorized_keys` | 600 |

### Key Lifecycle

1. **Generation**: Created fresh for each pairing
2. **Distribution**: Public key sent to listener
3. **Storage**: Private key saved locally, public key in `authorized_keys`
4. **Revocation**: `connecto unpair` removes local keys; manual removal from `authorized_keys`

## Network Security

### Pairing Protocol

The pairing protocol is **unencrypted** but designed to be safe:

- Only public keys are transmitted (safe to expose)
- Connection requires network access (implicit trust boundary)
- Short-lived listener (exits after pairing)

### Ports Used

| Port | Protocol | Purpose | Exposure |
|------|----------|---------|----------|
| 5353 | UDP | mDNS | Local network |
| 8099 | TCP | Pairing | Local network |
| 22 | TCP | SSH | Configurable |

### Recommendations

1. **Firewall**: Only allow 8099 during pairing
2. **VPN**: Use VPN for cross-internet pairing
3. **Monitoring**: Log `authorized_keys` changes

## Best Practices

### Before Pairing

- [ ] Verify you're on a trusted network
- [ ] Confirm the target IP is correct
- [ ] Ensure the listener is running on the intended machine

### After Pairing

- [ ] Test the connection: `connecto test <host>`
- [ ] Verify SSH host key fingerprint on first connect
- [ ] Stop the listener if still running

### Ongoing

- [ ] Periodically review `~/.ssh/authorized_keys`
- [ ] Remove unused pairings: `connecto unpair <host>`
- [ ] Keep Connecto and SSH updated

## Auditing

### List Connecto Keys

```bash
connecto hosts
```

### View authorized_keys

```bash
grep connecto ~/.ssh/authorized_keys
```

### Check Key Fingerprints

```bash
for key in ~/.ssh/connecto_*.pub; do
  echo "=== $key ==="
  ssh-keygen -lf "$key"
done
```

### SSH Connection Logs

```bash
# macOS
log show --predicate 'process == "sshd"' --last 1h

# Linux
journalctl -u sshd --since "1 hour ago"

# Windows
Get-EventLog -LogName Security -InstanceId 4624 |
  Where-Object { $_.Message -like "*ssh*" }
```

## Incident Response

### Suspected Compromise

1. **Immediately**: Remove unauthorized keys
   ```bash
   # Edit authorized_keys
   nano ~/.ssh/authorized_keys
   ```

2. **Audit**: Check all Connecto pairings
   ```bash
   connecto hosts
   ```

3. **Revoke**: Remove suspicious pairings
   ```bash
   connecto unpair <suspicious-host>
   ```

4. **Investigate**: Check SSH logs for unauthorized access

### Key Rotation

To rotate keys for a host:

```bash
connecto unpair mydesktop
# Have target run: connecto listen
connecto scan
connecto pair 0
```

## Comparison

### vs Password Authentication

| Aspect | Password | Connecto (SSH Keys) |
|--------|----------|---------------------|
| Brute force | Vulnerable | Immune |
| Credential reuse | Common | Impossible |
| Phishing | Possible | Difficult |
| Setup complexity | Low | Low (with Connecto) |

### vs Manual SSH Keys

| Aspect | Manual | Connecto |
|--------|--------|----------|
| Key generation | Manual | Automatic |
| Key distribution | Copy/paste | Protocol |
| Config setup | Manual | Automatic |
| Discovery | Manual | mDNS |
