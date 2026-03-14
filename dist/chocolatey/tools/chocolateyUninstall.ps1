$ErrorActionPreference = 'Stop'

# The MSI installer registers the product for clean uninstall via Programs and Features.
# Chocolatey's Uninstall-ChocolateyPackage will invoke msiexec /x against the cached MSI.
# If the MSI is not in the cache (e.g. after a manual install), fall back to product GUID.

$packageArgs = @{
  packageName    = 'stunt'
  fileType       = 'MSI'
  silentArgs     = '/qn /norestart'
  validExitCodes = @(0, 3010, 1605)  # 1605 = product not installed (safe to ignore)
}

Uninstall-ChocolateyPackage @packageArgs
