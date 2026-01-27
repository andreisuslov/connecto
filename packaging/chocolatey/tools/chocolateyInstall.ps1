$ErrorActionPreference = 'Stop'

$packageName   = 'connecto'
$version       = '0.3.0'
$toolsDir      = "$(Split-Path -parent $MyInvocation.MyCommand.Definition)"
$url64         = "https://github.com/andreisuslov/connecto/releases/download/v$version/connecto-windows-x86_64.zip"
$checksum      = 'c88870df1f446b1815d06d699ac6d5c5f2b7da1c9d53eb89e147313308519a27'
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
