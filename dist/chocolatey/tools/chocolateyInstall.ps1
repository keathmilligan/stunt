$ErrorActionPreference = 'Stop'

$version  = '__VERSION__'
$repo     = 'keathmilligan/stunt'
$zipName  = "stunt-${version}-x86_64-pc-windows-msvc.zip"
$url      = "https://github.com/${repo}/releases/download/v${version}/${zipName}"
$checksum = '__SHA256_ZIP__'

$packageArgs = @{
  packageName    = 'stunt'
  unzipLocation  = $(Split-Path -Parent $MyInvocation.MyCommand.Definition)
  url64bit       = $url
  checksum64     = $checksum
  checksumType64 = 'sha256'
}

Install-ChocolateyZipPackage @packageArgs
