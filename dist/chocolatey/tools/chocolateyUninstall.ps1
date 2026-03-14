$ErrorActionPreference = 'Stop'

# No custom uninstall logic needed for portable ZIP packages.
# Chocolatey automatically removes extracted files from the tools directory
# and cleans up shims from the bin folder.
