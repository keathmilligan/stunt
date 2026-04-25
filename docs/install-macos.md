# `stunt` macOS Installation Guide

## Homebrew

```bash
brew tap keathmilligan/tap
brew install keathmilligan/tap/stunt
```

Stay up-to-date with `brew upgrade stunt`.

## Shell Installer

```bash
curl -fsSL https://packages.keathmilligan.net/stunt/install.sh | sh
```

This will install `stunt` into `~/.local/bin`.

## cargo

If you have Rust development tools installed:

```bash
cargo install stunt
```

## dmg Installer

Download the signed `.dmg` installer for your platform architecture (Apple Silicon `aarch64` or Intel `x86_64`) directly from the [GitHub Releases](https://github.com/keathmilligan/stunt/releases) page.

## Binary

Download the macOS binary archive for your architecture (Apple Silicon `aarch64` or Intel `x86_64`) from the [GitHub Releases](https://github.com/keathmilligan/stunt/releases) page.

Extract the `stunt` binary into a directory in your `PATH`.

