# scan

Discover devices running `connecto listen`.

## Usage

```bash
connecto scan [OPTIONS]
```

## Description

The `scan` command discovers devices on your network that are running `connecto listen`. It tries discovery methods in order, falling back when one finds nothing:

1. **mDNS discovery** - finds devices advertising the `_connecto._tcp` service
2. **Subnet scanning** - scans your local subnets, saved subnets, and any `--subnet` arguments
3. **Ad-hoc network scan** - looks for Connecto ad-hoc WiFi networks (created with `connecto listen --adhoc`)
4. **Bluetooth LE** - with `--bluetooth`, scans for BLE-advertised devices (requires a build with the `bluetooth` feature)

## Options

| Option | Description |
|--------|-------------|
| `-t, --timeout <SECONDS>` | Scan timeout in seconds (default: 5) |
| `-s, --subnet <CIDR>` | Additional subnet to scan (can be repeated) |
| `--bluetooth` | Enable Bluetooth Low Energy scanning as a fallback |

## Examples

### Basic scan

```bash
connecto scan
```

Output:
```
  CONNECTO SCANNER

→ Scanning for devices...

✓ Found 2 device(s):

[0] mydesktop (192.168.1.55:8099)
[1] workstation (192.168.1.100:8099)

To pair with a device, run: connecto pair <number>
```

### Scan additional subnet

```bash
connecto scan --subnet 10.0.2.0/24
```

### Scan multiple subnets

```bash
connecto scan -s 10.0.2.0/24 -s 10.0.3.0/24
```

## Discovery methods

### mDNS Discovery

mDNS (multicast DNS) automatically finds devices on the same subnet. No configuration needed.

**Limitations:**
- Only works within the same subnet
- May be blocked by some network configurations

### Subnet scanning

For VPN or cross-subnet scenarios, Connecto scans IP ranges directly. Each
responding host is probed with a real protocol handshake, so only actual
Connecto listeners show up — see [Protocol](../reference/protocol.md#subnet-scanning).

**Saved subnets** are automatically included:
```bash
connecto config add-subnet 10.0.2.0/24
connecto scan  # Now includes 10.0.2.0/24
```

**One-time subnets** can be specified with `--subnet`:
```bash
connecto scan --subnet 10.0.2.0/24
```

### Ad-hoc networks

If nothing is found, Connecto looks for ad-hoc WiFi networks created by
`connecto listen --adhoc`. When one is found, Connecto may briefly join it to
probe for the listening host — your previous WiFi network is always restored
before the results are printed, so pair by rejoining the ad-hoc network when
you're ready.

### Network isolation hint

If you have a working network address but nothing answered, your router is
likely isolating clients from each other (AP/client isolation). The scan
output explains how to work around it with a direct WiFi network.

## Device cache

Scan results are cached so that `connecto pair <number>` can resolve device
numbers. The cache lives in your per-user cache directory (e.g.
`~/Library/Caches/com.connecto.connecto/devices.json` on macOS,
`~/.cache/connecto/devices.json` on Linux) — not in a world-writable
location like `/tmp`.

Device numbers start at `0` and refer to the most recent scan.

## Scan performance

| Subnet Size | IPs | Approximate Time |
|-------------|-----|------------------|
| /24 | 254 | 2-3 seconds |
| /22 | 1,022 | 5-10 seconds |
| /16 | 65,534 | Not recommended |

Connecto scans up to 100 IPs concurrently with a 500ms timeout per IP.

## No devices found?

If no devices are found:

1. Ensure the target is running `connecto listen`
2. Check firewall allows TCP 8099 and UDP 5353
3. For VPN, add the remote subnet: `connecto config add-subnet <CIDR>`
4. Try direct pairing: `connecto pair <ip>:8099`

See [Troubleshooting](../reference/troubleshooting.md) for more help.
