# Windows Installation Guide

## PowerShell Installer

In an elevated powershell session, run:

```powershell
irm https://packages.keathmilligan.net/unfk/install.ps1 | iex
```

## cargo

If you have Rust development tools installed you can install with `cargo`:

```bash
cargo install unfk
```

## Scoop (Windows)

```powershell
scoop bucket add keathmilligan https://github.com/keathmilligan/scoop-bucket
scoop install unfk
```

## Windows MSI

Download the signed `.msi` installer directly from the [GitHub Releases](https://github.com/keathmilligan/unfk/releases) page.

## Binary

Download the Windows binary archive for your architecture (Intel `x86_64` or ARM `aarch64`) from the [GitHub Releases](https://github.com/keathmilligan/unfk/releases) page.

Extract the `unfk` binary into a directory in your `PATH`.

