# Troubleshooting

Common issues and solutions.

## Discovery Issues

### "No devices found" during scan

**Causes:**
1. Listener not running
2. Firewall blocking mDNS or TCP
3. Different subnets (VPN scenario)

**Solutions:**

1. Verify listener is running:
   ```bash
   # On target machine
   connecto listen
   ```

2. Check firewall:
   ```bash
   # Test mDNS (macOS/Linux)
   dns-sd -B _connecto._tcp

   # Test TCP port
   nc -zv <target-ip> 8099
   ```

3. For VPN/cross-subnet:
   ```bash
   connecto config add-subnet 10.0.2.0/24
   connecto scan
   ```

### Scan finds device but can't connect

**Causes:**
1. Firewall allows mDNS but blocks TCP
2. Listener crashed after advertising

**Solutions:**
1. Restart listener: `connecto listen`
2. Check TCP connectivity: `nc -zv <ip> 8099`
3. Review firewall rules for TCP 8099

---

## Pairing Issues

### "Connection refused"

**Causes:**
1. Listener not running
2. Wrong port
3. Firewall blocking TCP

**Solutions:**
```bash
# Verify listener is running
ps aux | grep connecto

# Check if port is listening
lsof -i :8099  # macOS/Linux
netstat -an | findstr 8099  # Windows

# Test connection
nc -zv <ip> 8099
```

### "Connection timed out"

**Causes:**
1. Wrong IP address
2. Network routing issue
3. Firewall dropping packets

**Solutions:**
```bash
# Verify IP is reachable
ping <ip>

# Check route
traceroute <ip>  # macOS/Linux
tracert <ip>     # Windows
```

### "Permission denied" after pairing

**Causes:**
1. Key not added to authorized_keys
2. Wrong username
3. SSH config issue

**Solutions:**
```bash
# On target machine, verify key was added
grep connecto ~/.ssh/authorized_keys

# Check permissions
ls -la ~/.ssh/
# Should be: authorized_keys 600, .ssh dir 700

# Test with verbose SSH
ssh -v <host>
```

---

## SSH Issues

### "Host key verification failed"

The remote host's SSH server key changed.

**Solutions:**
```bash
# Remove old key
ssh-keygen -R <ip>

# Connect again (will prompt to accept new key)
ssh <host>
```

### "Too many authentication failures"

SSH agent offering too many keys.

**Solutions:**
```bash
# Connect with specific key only
ssh -o IdentitiesOnly=yes -i ~/.ssh/connecto_<host> <host>

# Or update ~/.ssh/config (Connecto does this automatically):
# IdentitiesOnly yes
```

### Can't connect after IP change

**Solutions:**
```bash
# Update the IP
connecto update-ip <host> <new-ip>

# Verify
connecto test <host>
```

---

## Platform-Specific Issues

### macOS

**mDNS not working:**
```bash
# Check mDNS daemon
sudo launchctl list | grep mDNS

# Restart mDNS
sudo killall -HUP mDNSResponder
```

**Firewall prompts:**
- Allow "connecto" in System Preferences → Security & Privacy → Firewall

### Windows

**Firewall blocking Connecto:**
```powershell
# Add firewall rules
New-NetFirewallRule -DisplayName "Connecto mDNS" -Direction Inbound -Protocol UDP -LocalPort 5353 -Action Allow
New-NetFirewallRule -DisplayName "Connecto TCP" -Direction Inbound -Protocol TCP -LocalPort 8099 -Action Allow
```

**OpenSSH not installed:**
```powershell
# Check if OpenSSH is available
Get-WindowsCapability -Online | ? Name -like 'OpenSSH*'

# Install OpenSSH Client
Add-WindowsCapability -Online -Name OpenSSH.Client~~~~0.0.1.0
```

**SSH service not running:**
```powershell
# Start SSH agent
Start-Service ssh-agent
Set-Service ssh-agent -StartupType Automatic
```

### Linux

**mDNS/Avahi issues:**
```bash
# Check Avahi daemon
systemctl status avahi-daemon

# Restart Avahi
sudo systemctl restart avahi-daemon

# Install if missing
sudo apt install avahi-daemon  # Debian/Ubuntu
sudo dnf install avahi         # Fedora
```

**SELinux blocking SSH:**
```bash
# Check SELinux status
getenforce

# Temporarily disable (for testing)
sudo setenforce 0

# Check audit log
sudo ausearch -m avc -ts recent
```

---

## Config Issues

### Config file corrupted

**Symptoms:** Commands fail with JSON parse errors

**Solution:**
```bash
# Find config location
connecto config path

# Reset config (backup first)
mv ~/.config/connecto/config.json ~/.config/connecto/config.json.bak
```

### SSH config conflicts

**Symptoms:** SSH uses wrong key or settings

**Solution:**
```bash
# Check for duplicate entries
grep -n "Host <hostname>" ~/.ssh/config

# Remove duplicates, keep Connecto entry
# Or manually merge settings
```

---

## Getting Help

### Verbose Output

Most commands support verbose mode (planned feature):
```bash
connecto scan -v
connecto pair -v 0
```

### Debug Information

Collect for bug reports:
```bash
# Version
connecto --version

# Config
connecto config list
connecto config path

# SSH config (remove sensitive info)
grep -A5 "# Added by Connecto" ~/.ssh/config

# System info
uname -a  # macOS/Linux
systeminfo | findstr /B /C:"OS"  # Windows
```

### Reporting Bugs

Report issues at: [github.com/andreisuslov/connecto/issues](https://github.com/andreisuslov/connecto/issues)

Include:
1. Connecto version
2. Operating system
3. Steps to reproduce
4. Error messages
5. Relevant config (sanitized)
