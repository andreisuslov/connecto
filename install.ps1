#!/usr/bin/env pwsh
# Connecto installer for Windows
# Usage: irm https://raw.githubusercontent.com/andreisuslov/ssh/main/install.ps1 | iex

$ErrorActionPreference = "Stop"

$repo = "andreisuslov/ssh"
$installDir = "$env:LOCALAPPDATA\connecto"

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
$userPath = [Environment]::GetEnvironmentVariable("PATH", "User")
if ($userPath -notlike "*$installDir*") {
    [Environment]::SetEnvironmentVariable("PATH", "$userPath;$installDir", "User")
    $env:PATH += ";$installDir"
    Write-Host "Added $installDir to PATH" -ForegroundColor Green
}

Write-Host ""
Write-Host "Connecto $version installed successfully!" -ForegroundColor Green
Write-Host "Run 'connecto --help' to get started."
Write-Host ""
Write-Host "Note: Restart your terminal for PATH changes to take effect."
