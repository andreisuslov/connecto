$ErrorActionPreference = 'Stop'

$packageName = 'connecto'
$toolsDir    = "$(Split-Path -parent $MyInvocation.MyCommand.Definition)"

# Remove the executable
$exePath = Join-Path $toolsDir 'connecto.exe'
if (Test-Path $exePath) {
    Remove-Item $exePath -Force
    Write-Host "Removed $exePath"
}

# Remove firewall rules
Remove-NetFirewallRule -DisplayName "Connecto mDNS" -ErrorAction SilentlyContinue
Remove-NetFirewallRule -DisplayName "Connecto TCP" -ErrorAction SilentlyContinue
Write-Host "Removed firewall rules."

Write-Host "$packageName has been uninstalled."
