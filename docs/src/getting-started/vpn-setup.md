# VPN Setup

When devices are on different subnets (common with VPNs), mDNS discovery won't work across subnets. Connecto provides a simple solution: save the remote subnet once, and scans will include it automatically.

## The Problem

```
┌─────────────────────────────────────────────────────────────┐
│                         VPN Network                          │
├─────────────────────────┬───────────────────────────────────┤
│   Subnet A: 10.0.1.0/24 │   Subnet B: 10.0.2.0/24          │
│                         │                                   │
│   ┌─────────────┐       │       ┌─────────────┐            │
│   │   Your Mac  │       │       │   Windows   │            │
│   │  10.0.1.50  │  ✗ mDNS ✗     │  10.0.2.100 │            │
│   └─────────────┘       │       └─────────────┘            │
│                         │                                   │
└─────────────────────────┴───────────────────────────────────┘
```

mDNS broadcasts don't cross subnet boundaries, so `connecto scan` won't find devices on other subnets.

## The Solution

### Step 1: Find the Remote Subnet

Ask your colleague or check your VPN documentation for the subnet. Common formats:
- `10.0.2.0/24` (256 addresses)
- `192.168.100.0/24`
- `172.16.5.0/24`

### Step 2: Save the Subnet

```bash
connecto config add-subnet 10.0.2.0/24
```

You can add multiple subnets:

```bash
connecto config add-subnet 10.0.3.0/24
connecto config add-subnet 192.168.100.0/24
```

### Step 3: Scan and Pair

Now `connecto scan` will automatically include saved subnets:

```bash
connecto scan
```

```
  CONNECTO SCANNER

→ Scanning for devices...

✓ Found 1 device(s):

[0] windows-workstation (10.0.2.100:8099)
```

Pair as usual:

```bash
connecto pair 0
```

## Managing Subnets

### List Saved Subnets

```bash
connecto config list
```

```
Configured subnets:
  • 10.0.2.0/24
  • 10.0.3.0/24
```

### Remove a Subnet

```bash
connecto config remove-subnet 10.0.3.0/24
```

### Config File Location

```bash
connecto config path
```

The config file is stored at:
- **macOS/Linux**: `~/.config/connecto/config.json`
- **Windows**: `%APPDATA%\connecto\config.json`

## One-Time Subnet Scan

If you don't want to save a subnet permanently, use the `--subnet` flag:

```bash
connecto scan --subnet 10.0.2.0/24
```

You can specify multiple subnets:

```bash
connecto scan -s 10.0.2.0/24 -s 10.0.3.0/24
```

## Listener VPN Hint

When someone pairs from a different subnet, the listener shows a helpful message:

```
✓ Successfully paired with mac-laptop!
  → They can now SSH to this machine.

VPN/Cross-subnet connection detected!
  → Tell mac-laptop to save your subnet for future scans:
    connecto config add-subnet 10.0.1.0/24
```

## Direct Pairing

If you know the exact IP, skip scanning entirely:

```bash
connecto pair 10.0.2.100:8099
```

## Troubleshooting

### Scan takes too long

Scanning large subnets can take time. Connecto scans up to 100 IPs concurrently with 500ms timeout per IP.

For faster scans, use a smaller subnet if possible:
- `/24` = 254 IPs (~2-3 seconds)
- `/22` = 1022 IPs (~5-10 seconds)
- `/16` = 65534 IPs (not recommended)

### Connection refused

Ensure:
1. The target is running `connecto listen`
2. Firewall allows TCP 8099
3. VPN is connected and routing works

Test connectivity:

```bash
# Check if port is open
nc -zv 10.0.2.100 8099
```
