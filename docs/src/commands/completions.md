# completions

Generate shell completion scripts.

## Usage

```bash
connecto completions <SHELL>
```

## Arguments

| Argument | Description |
|----------|-------------|
| `SHELL` | Target shell: `bash`, `zsh`, `fish`, or `powershell` |

## Description

Generates tab-completion scripts for your shell. After installation, pressing Tab will complete Connecto commands and options.

## Installation

### Bash

```bash
# Add to ~/.bashrc
connecto completions bash >> ~/.bashrc

# Or install system-wide
sudo connecto completions bash > /etc/bash_completion.d/connecto
```

Restart your shell or run:
```bash
source ~/.bashrc
```

### Zsh

```bash
# Add to ~/.zshrc
connecto completions zsh >> ~/.zshrc
```

Or for Oh My Zsh:
```bash
connecto completions zsh > ~/.oh-my-zsh/completions/_connecto
```

Restart your shell or run:
```bash
source ~/.zshrc
```

### Fish

```bash
connecto completions fish > ~/.config/fish/completions/connecto.fish
```

Completions are available immediately in new shells.

### PowerShell

```powershell
# Add to your profile
connecto completions powershell >> $PROFILE

# Reload profile
. $PROFILE
```

To find your profile path:
```powershell
echo $PROFILE
```

## Example usage

After installation:

```bash
connecto <Tab>
# Shows: config  export  hosts  import  listen  pair  scan  test  unpair  update-ip

connecto config <Tab>
# Shows: add-subnet  list  path  remove-subnet

connecto scan --<Tab>
# Shows: --subnet  --timeout
```

## Troubleshooting

### Bash completions not working

Ensure bash-completion is installed:
```bash
# macOS
brew install bash-completion

# Ubuntu/Debian
apt install bash-completion
```

### Zsh completions not working

Ensure completion system is initialized. Add to `~/.zshrc`:
```zsh
autoload -Uz compinit && compinit
```

### Fish completions not working

Check that the completions directory exists:
```bash
mkdir -p ~/.config/fish/completions
```
