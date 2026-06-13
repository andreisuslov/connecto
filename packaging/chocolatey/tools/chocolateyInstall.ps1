$ErrorActionPreference = 'Stop'

$packageName   = 'connecto'
# Derived from the package metadata (connecto.nuspec <version>), so the
# download URL always matches the version being installed.
$version       = $env:ChocolateyPackageVersion
$toolsDir      = "$(Split-Path -parent $MyInvocation.MyCommand.Definition)"
$url64         = "https://github.com/andreisuslov/connecto/releases/download/v$version/connecto-windows-x86_64.zip"
# SHA-256 of the release zip above. Must be updated for every release before
# packing; see packaging/chocolatey/README.md for the exact commands.
$checksum      = 'REPLACE_WITH_CONNECTO_WINDOWS_X86_64_ZIP_SHA256'
$checksumType  = 'sha256'

$packageArgs = @{
    packageName    = $packageName
    unzipLocation  = $toolsDir
    url64bit       = $url64
    checksum64     = $checksum
    checksumType64 = $checksumType
}

Install-ChocolateyZipPackage @packageArgs

# Configure firewall rules for mDNS discovery
Write-Host "Configuring firewall rules for mDNS discovery..."

# Remove existing rules if they exist (to avoid duplicates)
Remove-NetFirewallRule -DisplayName "Connecto mDNS" -ErrorAction SilentlyContinue
Remove-NetFirewallRule -DisplayName "Connecto TCP" -ErrorAction SilentlyContinue

# Add firewall rules
New-NetFirewallRule -DisplayName "Connecto mDNS" -Direction Inbound -Protocol UDP -LocalPort 5353 -Action Allow | Out-Null
New-NetFirewallRule -DisplayName "Connecto TCP" -Direction Inbound -Protocol TCP -LocalPort 8099 -Action Allow | Out-Null

Write-Host "Firewall rules configured."
