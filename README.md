```
  ____  _____            _____
 / ___||_   _|_   _ _ __|_   _|
 \___ \  | | | | | | '_ \| |
  ___) | | | | |_| | | | | |
 |____/  |_|  \__,_|_| |_|_|
  Stupid Tunnel Tricks
```

# STunT

[![CI](https://github.com/keathmilligan/stunt/actions/workflows/ci.yml/badge.svg)](https://github.com/keathmilligan/stunt/actions/workflows/ci.yml)
[![Release](https://github.com/keathmilligan/stunt/actions/workflows/release.yml/badge.svg)](https://github.com/keathmilligan/stunt/actions/workflows/release.yml)
[![macOS DMG](https://packages.keathmilligan.net/stunt/badges/macos-dmg.svg)](https://github.com/keathmilligan/stunt/releases/latest)
[![Windows MSI](https://packages.keathmilligan.net/stunt/badges/msi.svg)](https://github.com/keathmilligan/stunt/releases/latest)
[![Homebrew](https://packages.keathmilligan.net/stunt/badges/homebrew.svg)](https://github.com/keathmilligan/stunt/releases/latest)
[![Scoop](https://packages.keathmilligan.net/stunt/badges/scoop.svg)](https://github.com/keathmilligan/stunt/releases/latest)
[![apt](https://packages.keathmilligan.net/stunt/badges/apt.svg)](https://github.com/keathmilligan/stunt/releases/latest)
[![rpm](https://packages.keathmilligan.net/stunt/badges/rpm.svg)](https://github.com/keathmilligan/stunt/releases/latest)
[![crates.io](https://packages.keathmilligan.net/stunt/badges/crates-io.svg)](https://github.com/keathmilligan/stunt/releases/latest)
[![Install Scripts](https://packages.keathmilligan.net/stunt/badges/install-scripts.svg)](https://github.com/keathmilligan/stunt/releases/latest)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

**S**tupid **Tun**nel **T**ricks — a terminal user interface for defining, configuring, and managing SSH tunnel connections.

## Features

- Define local, remote, and dynamic (SOCKS) SSH port-forwarding tunnels
- Start, stop, and monitor tunnel connections from a single dashboard
- Persist tunnel configurations to disk (TOML)
- Auto-reconnect on connection failure
- Group and label tunnels for organization

## Requirements

- Rust 1.75+ (2024 edition)
- A working `ssh` client on `$PATH`

## Getting Started

```sh
# Clone the repository
git clone <repo-url> && cd stunt

# Build
cargo build --release

# Run
cargo run
```

Configuration files are stored in `~/.config/stunt/` by default.

## Usage

Launch the TUI:

```sh
stunt
```

### Key Bindings

| Key       | Action                        |
|-----------|-------------------------------|
| `n`       | New tunnel                    |
| `e`       | Edit selected tunnel          |
| `d`       | Delete selected tunnel        |
| `Enter`   | Connect / disconnect tunnel   |
| `j` / `k` or arrow keys | Navigate list                |
| `q`       | Quit                          |

## Tunnel Configuration

Tunnels are defined in TOML. Example:

```toml
[[tunnel]]
name = "postgres-prod"
host = "bastion.example.com"
user = "deploy"
local_port = 5432
remote_host = "db.internal"
remote_port = 5432
identity_file = "~/.ssh/id_ed25519"
```

## Development

```sh
# Run in debug mode
cargo run

# Run tests
cargo test

# Lint
cargo clippy -- -D warnings

# Format
cargo fmt
```

## License

MIT
