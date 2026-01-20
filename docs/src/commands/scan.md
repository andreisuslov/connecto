# scan

Discover devices running `connecto listen`.

## Usage

```bash
connecto scan [OPTIONS]
```

## Description

The `scan` command discovers devices on your network that are running `connecto listen`. It uses:

1. **mDNS discovery** - Finds devices advertising the `_connecto._tcp` service
2. **Subnet scanning** - Scans saved subnets and optionally specified subnets

## Options

| Option | Description |
|--------|-------------|
| `-s, --subnet <CIDR>` | Additional subnet to scan (can be repeated) |
| `-t, --timeout <MS>` | Scan timeout in milliseconds (default: 3000) |

## Examples

### Basic Scan

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

### Scan Additional Subnet

```bash
connecto scan --subnet 10.0.2.0/24
```

### Scan Multiple Subnets

```bash
connecto scan -s 10.0.2.0/24 -s 10.0.3.0/24
```

## Discovery Methods

### mDNS Discovery

mDNS (multicast DNS) automatically finds devices on the same subnet. No configuration needed.

**Limitations:**
- Only works within the same subnet
- May be blocked by some network configurations

### Subnet Scanning

For VPN or cross-subnet scenarios, Connecto scans IP ranges directly.

**Saved subnets** are automatically included:
```bash
connecto config add-subnet 10.0.2.0/24
connecto scan  # Now includes 10.0.2.0/24
```

**One-time subnets** can be specified with `--subnet`:
```bash
connecto scan --subnet 10.0.2.0/24
```

## Scan Performance

| Subnet Size | IPs | Approximate Time |
|-------------|-----|------------------|
| /24 | 254 | 2-3 seconds |
| /22 | 1,022 | 5-10 seconds |
| /16 | 65,534 | Not recommended |

Connecto scans up to 100 IPs concurrently with a 500ms timeout per IP.

## No Devices Found?

If no devices are found:

1. Ensure the target is running `connecto listen`
2. Check firewall allows TCP 8099 and UDP 5353
3. For VPN, add the remote subnet: `connecto config add-subnet <CIDR>`
4. Try direct pairing: `connecto pair <ip>:8099`

See [Troubleshooting](../reference/troubleshooting.md) for more help.
