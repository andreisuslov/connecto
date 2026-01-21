# Security

Security model and best practices for Connecto.

## Threat model

### Protected against

| Threat | Protection |
|--------|------------|
| Password guessing | SSH key authentication only |
| Credential theft | Private keys never leave the device |
| Replay attacks | SSH protocol cryptographic protection |
| Network sniffing | SSH encrypts all traffic after pairing |

### Not protected against

| Threat | Mitigation |
|--------|------------|
| Malicious network access | Only pair on trusted networks |
| Physical device access | Use full-disk encryption |
| Compromised endpoints | Keep systems updated |

## Key security

### Key generation

- **Algorithm**: Ed25519 (elliptic curve)
- **Security level**: 128-bit equivalent
- **Key size**: 256-bit private, 256-bit public

Ed25519 advantages:

- No known practical attacks
- Resistant to timing attacks
- Small, fast signatures
- Widely supported (OpenSSH 6.5+)

### When to prefer RSA-4096

While Ed25519 is the default and recommended for most users, RSA-4096 may be preferred in certain scenarios:

| Reason | Details |
|--------|---------|
| **Legacy compatibility** | Systems running OpenSSH < 6.5 (pre-2014) or older embedded devices may not support Ed25519 |
| **Hardware security modules** | Some older HSMs, smart cards, and hardware tokens only support RSA keys |
| **Compliance requirements** | Certain regulatory frameworks (e.g., older FIPS 140-2 configurations, some government standards) may mandate RSA |
| **Conservative cryptographic choice** | RSA has 40+ years of cryptanalysis; some organizations prefer battle-tested algorithms |
| **Cross-platform interoperability** | Better support across legacy SSH implementations, older libraries, and enterprise software |

RSA-4096 trade-offs:

- **Slower**: key generation, signing, and verification are significantly slower than Ed25519
- **Larger keys**: 4096-bit keys vs 256-bit (affects storage and transmission)
- **More complex implementation**: higher risk of implementation flaws (padding oracles, timing attacks)

To use RSA-4096 with Connecto, specify the key type during pairing:

```bash
connecto pair --key-type rsa <target>
```

### Key storage

| Component | Location | Permissions |
|-----------|----------|-------------|
| Private key | `~/.ssh/connecto_*` | 600 (owner read/write) |
| Public key | `~/.ssh/connecto_*.pub` | 644 (world readable) |
| Authorized keys | `~/.ssh/authorized_keys` | 600 |

### Key lifecycle

1. **Generation**: Created fresh for each pairing
2. **Distribution**: Public key sent to listener
3. **Storage**: Private key saved locally, public key in `authorized_keys`
4. **Revocation**: `connecto unpair` removes local keys; manual removal from `authorized_keys`

## Network security

### Pairing protocol

The pairing protocol is **unencrypted** but designed to be safe:

- Only public keys are transmitted (safe to expose)
- Connection requires network access (implicit trust boundary)
- Short-lived listener (exits after pairing)

### Ports used

| Port | Protocol | Purpose | Exposure |
|------|----------|---------|----------|
| 5353 | UDP | mDNS | Local network |
| 8099 | TCP | Pairing | Local network |
| 22 | TCP | SSH | Configurable |

### Recommendations

1. **Firewall**: Only allow 8099 during pairing
2. **VPN**: Use VPN for cross-internet pairing
3. **Monitoring**: Log `authorized_keys` changes

## Best practices

### Before pairing

-Verify you're on a trusted network
-Confirm the target IP is correct
-Ensure the listener is running on the intended machine

### After pairing

- Test the connection: `connecto test <host>`
- Verify SSH host key fingerprint on first connect
- Stop the listener if still running

### Ongoing

- Periodically review `~/.ssh/authorized_keys`
- Remove unused pairings: `connecto unpair <host>`
- Keep Connecto and SSH updated

## Auditing

### List Connecto keys

```bash
connecto hosts
```

### View authorized_keys

```bash
grep connecto ~/.ssh/authorized_keys
```

### Check key fingerprints

```bash
for key in ~/.ssh/connecto_*.pub; do
  echo "=== $key ==="
  ssh-keygen -lf "$key"
done
```

### SSH connection logs

```bash
# macOS
log show --predicate 'process == "sshd"' --last 1h

# Linux
journalctl -u sshd --since "1 hour ago"

# Windows
Get-EventLog -LogName Security -InstanceId 4624 |
  Where-Object { $_.Message -like "*ssh*" }
```

## Incident response

### Suspected compromise

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

### Key rotation

To rotate keys for a host:

```bash
connecto unpair mydesktop
# Have target run: connecto listen
connecto scan
connecto pair 0
```

## Comparison

### vs password authentication

| Aspect | Password | Connecto (SSH keys) |
|--------|----------|---------------------|
| Brute force | Vulnerable | Immune |
| Credential reuse | Common | Impossible |
| Phishing | Possible | Difficult |
| Setup complexity | Low | Low (with Connecto) |

### vs manual SSH keys

| Aspect | Manual | Connecto |
|--------|--------|----------|
| Key generation | Manual | Automatic |
| Key distribution | Copy/paste | Protocol |
| Config setup | Manual | Automatic |
| Discovery | Manual | mDNS |
