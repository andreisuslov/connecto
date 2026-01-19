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
