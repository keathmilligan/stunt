# `stunt` Linux Installation Guide

Choose the best installation option for your distro.

## Shell Installer (all distros)

```bash
curl -fsSL https://packages.keathmilligan.net/stunt/install.sh | sh
```

This will install `stunt` into `~/.local/bin`.

## cargo (all distros)

If you have Rust development tools installed:

```bash
cargo install stunt
```

## Homebrew (all distros)

Homebrew is also supported on Linux. If you have it installed:

```bash
brew tap keathmilligan/tap
brew install keathmilligan/tap/stunt
```

## apt (Debian / Ubuntu)

```bash
curl -fsSL https://packages.keathmilligan.net/gpg.key | sudo gpg --dearmor -o /etc/apt/keyrings/keathmilligan.gpg
echo "deb [signed-by=/etc/apt/keyrings/keathmilligan.gpg] https://packages.keathmilligan.net/apt stable main" | sudo tee /etc/apt/sources.list.d/keathmilligan.list
sudo apt update
sudo apt install stunt
```

Stay up to date with:

```
sudo apt upgrade stunt
```

## dnf / rpm (Fedora / RHEL / CentOS)

```bash
sudo curl -o /etc/yum.repos.d/keathmilligan.repo https://packages.keathmilligan.net/rpm/keathmilligan.repo
sudo dnf install stunt
```

Stay up to date with:

```
sudo dnf upgrade stunt
```

## AUR (Arch Linux)

```bash
yay -S stunt-bin
```

## Binary

Download the linux binary archive for your architecture (Intel `x86_64` or ARM `aarch64`) from the [GitHub Releases](https://github.com/keathmilligan/stunt/releases) page.

Extract the `stunt` binary into a directory in your `PATH`.

