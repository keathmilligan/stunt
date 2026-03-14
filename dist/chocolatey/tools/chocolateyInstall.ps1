$ErrorActionPreference = 'Stop'

$version  = '__VERSION__'
$repo     = 'keathmilligan/stunt'
$msiName  = "stunt-${version}-x86_64.msi"
$url      = "https://github.com/${repo}/releases/download/v${version}/${msiName}"
$checksum = '__SHA256_MSI__'

$packageArgs = @{
  packageName    = 'stunt'
  fileType       = 'MSI'
  url64bit       = $url
  checksum64     = $checksum
  checksumType64 = 'sha256'
  silentArgs     = '/qn /norestart'
  validExitCodes = @(0, 3010)
}

Install-ChocolateyPackage @packageArgs
