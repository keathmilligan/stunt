```
  ____  _____            _____
 / ___||_   _|_   _ _ __|_   _|
 \___ \  | | | | | | '_ \| |
  ___) | | | | |_| | | | | |
 |____/  |_|  \__,_|_| |_|_|
  Stupid Tunnel Tricks
```

# STunT

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

TBD
