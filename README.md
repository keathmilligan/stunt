# tunnel-mgr

A terminal user interface for defining, configuring, and managing SSH tunnel connections.

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
git clone <repo-url> && cd tunnel-mgr

# Build
cargo build --release

# Run
cargo run
```

Configuration files are stored in `~/.config/tunnel-mgr/` by default.

## Usage

Launch the TUI:

```sh
tunnel-mgr
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
