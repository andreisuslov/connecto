# Quick start

Get SSH access between two machines in under a minute.

## Prerequisites

- Connecto installed on both machines ([Installation](./installation.md))
- Both machines on the same network (or [VPN setup](./vpn-setup.md))

## Step 1: Start the Listener

On the **target machine** (the one you want to SSH into):

```bash
connecto listen
```

You'll see:

```
  CONNECTO LISTENER

→ Device name: mydesktop
→ Port: 8099

Local IP addresses:
  • 192.168.1.55

✓ mDNS service registered - device is now discoverable

Listening for pairing requests on port 8099...
```

## Step 2: Scan for Devices

On the **client machine** (the one you want to SSH from):

```bash
connecto scan
```

You'll see:

```
  CONNECTO SCANNER

→ Scanning for devices...

✓ Found 1 device(s):

[0] mydesktop (192.168.1.55:8099)

To pair with a device, run: connecto pair <number>
```

## Step 3: Pair

Still on the client machine:

```bash
connecto pair 0
```

You'll see:

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

## Step 4: Connect!

```bash
ssh mydesktop
```

That's it! The listener exits automatically after successful pairing.

## What just happened?

1. **Listener** advertised itself via mDNS on your local network
2. **Scanner** discovered the listener and displayed it
3. **Pair** command:
   - Generated a new Ed25519 SSH key pair
   - Sent the public key to the listener
   - Listener added it to `~/.ssh/authorized_keys`
   - Client saved the private key and updated `~/.ssh/config`

## Next steps

- [List paired hosts](../commands/hosts.md): `connecto hosts`
- [Test connection](../commands/test.md): `connecto test mydesktop`
- [Update IP](../commands/update-ip.md): `connecto update-ip mydesktop 10.0.0.5`
- [Remove pairing](../commands/unpair.md): `connecto unpair mydesktop`
