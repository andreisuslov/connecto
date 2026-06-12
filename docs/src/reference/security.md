# Security

Security model and best practices for Connecto.

## Trust model — read this first

Connecto's pairing protocol runs over **plaintext TCP on your local
network**. That is safe for the *secrecy* of your keys — only public keys
ever cross the wire, and private keys never leave the machine that generated
them — but it means the protocol itself cannot tell you *who* is on the other
end of the connection.

The important consequence:

> **Without `--verify`, `connecto listen` installs a key from any device on
> the network that completes the handshake.** The received key's fingerprint
> and verification code are printed so you can audit what was installed, but
> nothing is gated on them — by the time you read them, the key is already in
> `authorized_keys`.

If anyone untrusted can reach your machine on the pairing port (an office
LAN, a shared apartment network, a coffee shop), run the listener with
verification:

```bash
connecto listen --verify
```

With `--verify`, each pairing request shows the requesting device's name, the
received key's SHA-256 fingerprint, and a 6-digit verification code, and
**nothing is installed until you approve it**.

### Why the verification code works

The code is derived from the key material itself: the first 4 bytes of the
public key's SHA-256 fingerprint digest, reduced to 6 digits (see
[Protocol](./protocol.md#verification-code)). Each side computes it
independently — the pairing side from the key it *sent*, the listening side
from the key it *received*. They are never sent over the wire.

A man-in-the-middle that substitutes its own key therefore changes the code
shown on the listening side. If the codes on the two screens match, the key
you approved is the key the other device actually holds.

What the code does **not** do: it does not authenticate the device *name*
(mDNS names are unauthenticated and trivially spoofable), and it does not
help at all if you don't compare it — which is why `--verify` exists.

## Threat model

### Protected against

| Threat | Protection |
|--------|------------|
| Password guessing | SSH key authentication only |
| Private key theft in transit | Private keys never leave the device |
| Key substitution (MITM) during pairing | `--verify` code is bound to the received key — **only with `--verify`** |
| Network sniffing of the exchange | Only public keys are transmitted (safe to expose) |
| Half-completed exchanges | Keys are rolled back if the handshake fails after installation; sync installs only after both sides confirm |

### Not protected against

| Threat | Mitigation |
|--------|------------|
| Unsolicited pairing from the local network | Use `--verify`; only run `listen` when you intend to pair |
| Spoofed device names in scan results | Names are cosmetic — verify the code, not the name |
| Malicious network access after pairing | SSH itself protects the session; review `authorized_keys` |
| Physical device access | Use full-disk encryption |
| Compromised endpoints | Keep systems updated |

## Key security

### Key generation

- **Algorithm**: Ed25519 (elliptic curve), generated locally with the
  `ssh-key` crate
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

To use RSA-4096 with Connecto, pass `--rsa` when pairing:

```bash
connecto pair --rsa <target>
```

### Key storage

| Component | Location | Permissions |
|-----------|----------|-------------|
| Private key | `~/.ssh/connecto_*` | 600 (owner read/write) |
| Public key | `~/.ssh/connecto_*.pub` | 644 (world readable) |
| Authorized keys | `~/.ssh/authorized_keys` | 600 |

Files are written atomically (a crash mid-write cannot truncate
`authorized_keys` or `~/.ssh/config`).

### Key lifecycle

1. **Generation**: created fresh for each pairing, unless you configured an
   existing key with `--key` or `connecto config set-default-key`
2. **Distribution**: public key sent to the listener
3. **Storage**: private key saved locally, public key in the listener's
   `authorized_keys`
4. **Revocation**: `connecto unpair` removes the connecto-managed config
   entry and deletes the key files **only if they follow the `connecto_*`
   naming convention** — a personal key configured via `--key` or
   `set-default-key` is never deleted. The public key remains in the remote
   machine's `authorized_keys` until removed there (`connecto keys remove`).

## Network security

### Pairing protocol

The pairing protocol is **unencrypted**. That is a deliberate trade-off:

- Only public keys are transmitted — there is nothing secret to encrypt
- Authenticity is the real concern, and it is addressed by the `--verify`
  code (which is bound to the key material), not by the transport
- The default listener exits after one successful pairing, shrinking the
  window in which unsolicited peers can pair at all

### Ports used

| Port | Protocol | Purpose | Exposure |
|------|----------|---------|----------|
| 5353 | UDP | mDNS | Local network |
| 8099 | TCP | Pairing / sync | Local network |
| 22 | TCP | SSH | Configurable |

### Recommendations

1. **Use `--verify`** whenever the network has anyone you don't trust on it
2. **Firewall**: only allow 8099 during pairing
3. **VPN**: use a VPN for cross-internet pairing
4. **Monitoring**: log `authorized_keys` changes

## Best practices

### Before pairing

- Verify you're on a trusted network — or use `connecto listen --verify`
- Confirm the target IP is correct
- Ensure the listener is running on the intended machine

### During pairing (with `--verify`)

- Compare the 6-digit code on both screens before approving
- Check the device name and key comment look right (but remember only the
  code is cryptographically meaningful)

### After pairing

- Test the connection: `connecto test <host>`
- Verify SSH host key fingerprint on first connect
- Stop the listener if still running

### Ongoing

- Periodically review `~/.ssh/authorized_keys` (`connecto keys list`)
- Remove unused pairings: `connecto unpair <host>`
- Keep Connecto and SSH updated

## Auditing

### List paired hosts

```bash
connecto hosts
```

### List authorized keys

```bash
connecto keys list
```

or directly:

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
   connecto keys list
   connecto keys remove <number>
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
# Have target run: connecto listen --verify
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
