$ErrorActionPreference = 'Stop'

$packageName = 'connecto'
$toolsDir    = "$(Split-Path -parent $MyInvocation.MyCommand.Definition)"

# Remove the executable
$exePath = Join-Path $toolsDir 'connecto.exe'
if (Test-Path $exePath) {
    Remove-Item $exePath -Force
    Write-Host "Removed $exePath"
}

Write-Host "$packageName has been uninstalled."
