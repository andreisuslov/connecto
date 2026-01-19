$ErrorActionPreference = 'Stop'

$packageName   = 'connecto'
$version       = '0.1.0'
$toolsDir      = "$(Split-Path -parent $MyInvocation.MyCommand.Definition)"
$url64         = "https://github.com/andreisuslov/connecto/releases/download/v$version/connecto-windows-x86_64.zip"
$checksum      = 'PLACEHOLDER_CHECKSUM'
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
