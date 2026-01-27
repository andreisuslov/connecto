#!/usr/bin/env pwsh
# Connecto installer for Windows
# Usage: irm https://raw.githubusercontent.com/andreisuslov/connecto/main/install.ps1 | iex

$ErrorActionPreference = "Stop"

$repo = "andreisuslov/connecto"
$installDir = "$env:LOCALAPPDATA\connecto"

# Check for admin rights (needed for firewall rules)
$isAdmin = ([Security.Principal.WindowsPrincipal] [Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)

Write-Host "Installing Connecto..." -ForegroundColor Cyan

# Get latest release
$release = Invoke-RestMethod "https://api.github.com/repos/$repo/releases/latest"
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
$zipPath = "$env:TEMP\connecto.zip"
Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $zipPath
Expand-Archive -Path $zipPath -DestinationPath $installDir -Force
Remove-Item $zipPath

# Add to PATH if not already there
# Use Machine PATH when running as Admin (persists for all users including Admin sessions)
# Use User PATH when running as regular user
$pathUpdated = $false
if ($isAdmin) {
    $machinePath = [Environment]::GetEnvironmentVariable("PATH", "Machine")
    if ($machinePath -notlike "*$installDir*") {
        [Environment]::SetEnvironmentVariable("PATH", "$machinePath;$installDir", "Machine")
        $pathUpdated = $true
        Write-Host "Added $installDir to system PATH" -ForegroundColor Green
    }
} else {
    $userPath = [Environment]::GetEnvironmentVariable("PATH", "User")
    if ($userPath -notlike "*$installDir*") {
        [Environment]::SetEnvironmentVariable("PATH", "$userPath;$installDir", "User")
        $pathUpdated = $true
        Write-Host "Added $installDir to user PATH" -ForegroundColor Green
    }
}

# Refresh PATH in current session by re-reading from registry
$machinePath = [Environment]::GetEnvironmentVariable("PATH", "Machine")
$userPath = [Environment]::GetEnvironmentVariable("PATH", "User")
$env:PATH = "$machinePath;$userPath"

# Broadcast WM_SETTINGCHANGE to notify other applications
if ($pathUpdated) {
    Add-Type -TypeDefinition @"
        using System;
        using System.Runtime.InteropServices;
        public class Environment {
            [DllImport("user32.dll", SetLastError = true, CharSet = CharSet.Auto)]
            public static extern IntPtr SendMessageTimeout(
                IntPtr hWnd, uint Msg, UIntPtr wParam, string lParam,
                uint fuFlags, uint uTimeout, out UIntPtr lpdwResult);
        }
"@
    $HWND_BROADCAST = [IntPtr]0xffff
    $WM_SETTINGCHANGE = 0x1a
    $result = [UIntPtr]::Zero
    [Environment]::SendMessageTimeout($HWND_BROADCAST, $WM_SETTINGCHANGE, [UIntPtr]::Zero, "Environment", 2, 5000, [ref]$result) | Out-Null
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

Write-Host ""
Write-Host "Connecto $version installed successfully!" -ForegroundColor Green
Write-Host "Run 'connecto --help' to get started."
