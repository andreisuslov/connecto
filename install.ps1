#!/usr/bin/env pwsh
# Connecto installer for Windows
# Usage: irm https://raw.githubusercontent.com/andreisuslov/connecto/main/install.ps1 | iex

$ErrorActionPreference = "Stop"

# Enable TLS 1.2 for older Windows systems (required for GitHub)
[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

$repo = "andreisuslov/connecto"
$installDir = "$env:LOCALAPPDATA\connecto"

# Check for admin rights (needed for firewall rules)
$isAdmin = ([Security.Principal.WindowsPrincipal] [Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)

Write-Host "Installing Connecto..." -ForegroundColor Cyan

# Get latest release
$release = Invoke-RestMethod "https://api.github.com/repos/$repo/releases/latest" -UseBasicParsing
$version = $release.tag_name
$asset = $release.assets | Where-Object { $_.name -eq "connecto-windows-x86_64.zip" }

if (-not $asset) {
    Write-Error "Could not find Windows release asset"
    exit 1
}

Write-Host "Downloading Connecto $version..."

# Create install directory
New-Item -ItemType Directory -Force -Path $installDir | Out-Null

# Download and extract
$zipPath = Join-Path $env:TEMP ("connecto-" + [guid]::NewGuid().ToString("N") + ".zip")
Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $zipPath -UseBasicParsing

# Extract synchronously.
# Shell.Application's CopyHere is asynchronous, so deleting the zip immediately after
# could truncate the extraction, and it silently no-ops where the shell COM object is
# unavailable (Server Core, some service/non-interactive contexts). Both failure modes
# leave a missing or partial connecto.exe while the installer still reports success.
try {
    if (Get-Command Expand-Archive -ErrorAction SilentlyContinue) {
        Expand-Archive -LiteralPath $zipPath -DestinationPath $installDir -Force
    } else {
        # PowerShell 3/4: Expand-Archive doesn't exist, but .NET 4.5's ZipFile does.
        Add-Type -AssemblyName System.IO.Compression.FileSystem
        $staging = Join-Path $env:TEMP ("connecto-extract-" + [guid]::NewGuid().ToString("N"))
        [System.IO.Compression.ZipFile]::ExtractToDirectory($zipPath, $staging)
        Copy-Item -Path (Join-Path $staging "*") -Destination $installDir -Recurse -Force
        Remove-Item $staging -Recurse -Force -ErrorAction SilentlyContinue
    }
} finally {
    Remove-Item $zipPath -Force -ErrorAction SilentlyContinue
}

# Verify the binary actually landed and runs before claiming success
$exePath = Join-Path $installDir "connecto.exe"

if (-not (Test-Path -LiteralPath $exePath)) {
    Write-Error "Install failed: $exePath was not created. The archive did not extract correctly."
    exit 1
}

if ((Get-Item -LiteralPath $exePath).Length -eq 0) {
    Write-Error "Install failed: $exePath is empty. The archive extracted incompletely."
    exit 1
}

$prevEap = $ErrorActionPreference
$ErrorActionPreference = "Continue"
$global:LASTEXITCODE = 0
try {
    # Catches a binary that exists but cannot run at all - truncated download,
    # wrong architecture, or blocked/quarantined by endpoint security.
    $versionOutput = (& $exePath --version 2>&1 | Out-String).Trim()
    $versionExit = $LASTEXITCODE
} catch {
    $versionOutput = $_.Exception.Message
    $versionExit = -1
}
$ErrorActionPreference = $prevEap

if ($versionExit -ne 0) {
    Write-Error "Install failed: '$exePath --version' exited with $versionExit. Output: $versionOutput"
    exit 1
}

# Add to PATH, at the FRONT.
# Appending meant any stale connecto shim (an old .cmd/.bat/.ps1, or a previous install
# in another directory) earlier in PATH would keep winning resolution forever, so
# `connecto` would run the wrong thing even after a successful install.
# Use Machine PATH when running as Admin (persists for all users including Admin sessions)
# Use User PATH when running as regular user
function Set-ConnectoPath {
    param([string]$Scope, [string]$Dir)

    $current = [Environment]::GetEnvironmentVariable("PATH", $Scope)
    $parts = @()
    if ($current) {
        $parts = @($current -split ';' | Where-Object { $_ -and ($_.TrimEnd('\') -ne $Dir.TrimEnd('\')) })
    }
    $new = (@($Dir) + $parts) -join ';'

    if ($new -ne $current) {
        [Environment]::SetEnvironmentVariable("PATH", $new, $Scope)
        return $true
    }
    return $false
}

$pathUpdated = $false
if ($isAdmin) {
    $pathUpdated = Set-ConnectoPath -Scope "Machine" -Dir $installDir
    if ($pathUpdated) { Write-Host "Added $installDir to the front of system PATH" -ForegroundColor Green }
} else {
    $pathUpdated = Set-ConnectoPath -Scope "User" -Dir $installDir
    if ($pathUpdated) { Write-Host "Added $installDir to the front of user PATH" -ForegroundColor Green }
}

# Refresh PATH in current session by re-reading from registry,
# keeping the install dir first so this session resolves the new binary too.
$machinePath = [Environment]::GetEnvironmentVariable("PATH", "Machine")
$userPath = [Environment]::GetEnvironmentVariable("PATH", "User")
$sessionParts = @("$machinePath;$userPath" -split ';' | Where-Object { $_ -and ($_.TrimEnd('\') -ne $installDir.TrimEnd('\')) })
$env:PATH = (@($installDir) + $sessionParts) -join ';'

# Broadcast WM_SETTINGCHANGE to notify other applications
if ($pathUpdated) {
    try {
        Add-Type -TypeDefinition @"
            using System;
            using System.Runtime.InteropServices;
            public class ConnectoNativeMethods {
                [DllImport("user32.dll", SetLastError = true, CharSet = CharSet.Auto)]
                public static extern IntPtr SendMessageTimeout(
                    IntPtr hWnd, uint Msg, UIntPtr wParam, string lParam,
                    uint fuFlags, uint uTimeout, out UIntPtr lpdwResult);
            }
"@ -ErrorAction SilentlyContinue
        $HWND_BROADCAST = [IntPtr]0xffff
        $WM_SETTINGCHANGE = 0x1a
        $result = [UIntPtr]::Zero
        [ConnectoNativeMethods]::SendMessageTimeout($HWND_BROADCAST, $WM_SETTINGCHANGE, [UIntPtr]::Zero, "Environment", 2, 5000, [ref]$result) | Out-Null
    } catch {
        # Silently ignore if broadcast fails - PATH is already updated
    }
}

# Configure firewall rules for mDNS discovery
if ($isAdmin) {
    Write-Host "Configuring firewall rules..." -ForegroundColor Cyan

    # Remove existing rules if they exist (to avoid duplicates)
    Remove-NetFirewallRule -DisplayName "Connecto mDNS" -ErrorAction SilentlyContinue
    Remove-NetFirewallRule -DisplayName "Connecto TCP" -ErrorAction SilentlyContinue

    # Add firewall rules
    New-NetFirewallRule -DisplayName "Connecto mDNS" -Direction Inbound -Protocol UDP -LocalPort 5353 -Action Allow | Out-Null
    New-NetFirewallRule -DisplayName "Connecto TCP" -Direction Inbound -Protocol TCP -LocalPort 8099 -Action Allow | Out-Null

    Write-Host "Firewall rules configured." -ForegroundColor Green
} else {
    Write-Host ""
    Write-Host "Warning: Run as Administrator to configure firewall rules for mDNS discovery." -ForegroundColor Yellow
    Write-Host "Without firewall rules, 'connecto scan' may not discover devices." -ForegroundColor Yellow
    Write-Host ""
    Write-Host "To add firewall rules manually, run PowerShell as Administrator and execute:" -ForegroundColor Yellow
    Write-Host "  New-NetFirewallRule -DisplayName 'Connecto mDNS' -Direction Inbound -Protocol UDP -LocalPort 5353 -Action Allow" -ForegroundColor Gray
    Write-Host "  New-NetFirewallRule -DisplayName 'Connecto TCP' -Direction Inbound -Protocol TCP -LocalPort 8099 -Action Allow" -ForegroundColor Gray
}

# Warn if something else on PATH still shadows the install.
# A stale shim resolves and exits silently, which looks exactly like connecto being broken.
$resolved = Get-Command connecto -ErrorAction SilentlyContinue
$isShadowed = $false
if ($resolved) {
    # A function or alias beats PATH entirely and is reloaded by the profile on every new
    # session, so it survives "just open a new shell". Those have no .Source, so check the
    # command type first rather than only comparing paths.
    if ($resolved.CommandType -ne "Application") {
        $isShadowed = $true
    } elseif ($resolved.Source -and ($resolved.Source -ne $exePath)) {
        $isShadowed = $true
    }
}

if ($isShadowed) {
    $what = if ($resolved.Source) { $resolved.Source } else { "$($resolved.CommandType) '$($resolved.Name)' (defined in your PowerShell profile or session)" }
    Write-Host ""
    Write-Host "Warning: 'connecto' does not resolve to this install:" -ForegroundColor Yellow
    Write-Host "  resolves to: $what" -ForegroundColor Yellow
    Write-Host "  installed:   $exePath" -ForegroundColor Yellow
    Write-Host "A stale shim, function, or alias will shadow this binary and may exit silently." -ForegroundColor Yellow
    Write-Host "Remove it, or invoke the full path directly:" -ForegroundColor Yellow
    Write-Host "  & '$exePath' --help" -ForegroundColor Gray
    Write-Host "All matches:" -ForegroundColor Yellow
    Get-Command connecto -All -ErrorAction SilentlyContinue |
        ForEach-Object { Write-Host ("  " + $_.CommandType + "  " + $(if ($_.Source) { $_.Source } else { $_.Name })) -ForegroundColor Gray }
}

Write-Host ""
Write-Host "Connecto $version installed successfully! (verified: $versionOutput)" -ForegroundColor Green
Write-Host "Run 'connecto --help' to get started."
