# export / import

Backup and restore paired hosts configuration.

## Export

### Usage

```bash
connecto export [OUTPUT]
```

### Arguments

| Argument | Description |
|----------|-------------|
| `OUTPUT` | Output file path (optional, prints to stdout if omitted) |

### Description

Exports all paired hosts to a JSON file for backup or transfer to another machine.

### Examples

**Export to file:**

```bash
connecto export ~/connecto-backup.json
```

**Export to stdout:**

```bash
connecto export
```

**Pipe to clipboard (macOS):**

```bash
connecto export | pbcopy
```

### Export format

```json
{
  "version": 1,
  "hosts": [
    {
      "host": "mydesktop",
      "hostname": "192.168.1.55",
      "user": "john",
      "identity_file": "~/.ssh/connecto_mydesktop"
    }
  ],
  "subnets": ["10.0.2.0/24", "10.0.3.0/24"]
}
```

Note: The export contains SSH config entries only, not the actual key files. To fully backup/restore, you should also copy the key files from `~/.ssh/`.

---

## Import

### Usage

```bash
connecto import <FILE>
```

### Arguments

| Argument | Description |
|----------|-------------|
| `FILE` | Path to the export JSON file |

### Description

Imports paired hosts from a previously exported JSON file. This:

1. Restores SSH key files
2. Adds entries to `~/.ssh/config`
3. Restores saved subnets to config

### Example

```bash
connecto import ~/connecto-backup.json
```

Output:
```
  CONNECTO IMPORT

→ Importing from: ~/connecto-backup.json

✓ Imported host: mydesktop
✓ Imported host: workstation
✓ Imported 2 subnets

Successfully imported 2 hosts.
```

### Handling conflicts

If a host already exists:
- Existing keys are preserved
- The import skips that host
- A warning is displayed

To replace an existing host, first unpair it:

```bash
connecto unpair mydesktop
connecto import backup.json
```

---

## Use cases

### Backup before reinstall

```bash
connecto export > ~/Dropbox/connecto-backup.json
# Reinstall OS
connecto import ~/Dropbox/connecto-backup.json
```

### Transfer to new machine

```bash
# On old machine
connecto export > /tmp/connecto.json
scp /tmp/connecto.json newmachine:/tmp/

# On new machine
connecto import /tmp/connecto.json
```

### Sync across machines

While not a true sync, you can share exports via cloud storage:

```bash
# Machine A
connecto export > ~/Dropbox/connecto.json

# Machine B
connecto import ~/Dropbox/connecto.json
```

## Security notes

- The export contains **references to private keys** (file paths), not the keys themselves
- The actual key files in `~/.ssh/` should be backed up separately
- For a complete backup, also copy the key files:

```bash
# Full backup
connecto export > connecto-backup.json
cp ~/.ssh/connecto_* ~/backup/
```

## Related commands

| Command | Description |
|---------|-------------|
| `connecto hosts` | List current pairings |
| `connecto config list` | List saved subnets |
