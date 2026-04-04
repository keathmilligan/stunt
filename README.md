```
  ____  _____            _____
 / ___||_   _|_   _ _ __|_   _|
 \___ \  | | | | | | '_ \| |
  ___) | | | | |_| | | | | |
 |____/  |_|  \__,_|_| |_|_|
  Stupid Tunnel Tricks
```

**S**tupid **Tun**nel **T**ricks — a terminal user interface for defining, configuring, and managing SSH tunnel connections, Kubernetes port-forwards, and sshuttle VPN sessions.

---
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

https://github.com/user-attachments/assets/176c1ad8-4c27-4890-a080-060098b0db67

## Features

`stunt` allows you to define tunnels with SSH, Kubernetes and `sshuttle` that persist when you close the app. 

### SSH Tunnels

- Local port forwards (`ssh -L`) — expose a remote service on a local port
- Remote port forwards (`ssh -R`) — expose a local service on a remote port
- Dynamic SOCKS proxy (`ssh -D`) — route traffic through a remote host
- Optional SSH username and identity file per entry
- Custom SSH port support

### Kubernetes Port-Forwards

- Forward to pods, services, or deployments via `kubectl port-forward`
- Optional kubeconfig context and namespace per entry
- Multiple port bindings per workload entry

### sshuttle VPN Sessions

- Route entire subnets through a remote host via [sshuttle](https://github.com/sshuttle/sshuttle)
- Multiple subnets per entry (comma-separated in the form)
- Optional SSH port, username, and identity file per entry
- Linux and macOS only (sshuttle is not supported on Windows)

### Connection Management

- Start and stop tunnels from a single dashboard
- Per-entry connection state: connecting, connected, reconnecting, failed, suspended
- Auto-reconnect with exponential backoff (up to 10 retries, max 60s delay)
- Suspended state — manual disconnect of an auto-restart tunnel suppresses reconnection
- Session state persisted across restarts (PIDs tracked in `sessions.json`)
- Adopts existing tunnel processes on startup if they are still alive
- Warning when `kubectl` or `sshuttle` is unavailable but entries of that type are configured

### Configuration

- Configuration stored as TOML (`tunnels.toml`) in the platform data directory
- Atomic saves with `.bak` backup on every write
- Automatic migration of legacy `[[server]]` format to current `[[entries]]` format
- Multiple port-forward definitions per SSH and K8s entry

### UI

- Full-screen TUI built with [ratatui](https://ratatui.rs/)
- Create and edit entries with an in-app form (no editor required)
- Type-selection step when creating a new entry (SSH, K8s, or sshuttle)
- In-line forward sub-form with type cycling (`Ctrl+T`)
- Status bar with transient feedback messages

## Requirements

- Rust 1.85+ (2024 edition)
- A working `ssh` client on `$PATH` (for SSH tunnels)
- `kubectl` on `$PATH` (for Kubernetes port-forwards)
- `sshuttle` on `$PATH` (for sshuttle VPN sessions — Linux/macOS only)

## Installation

Download the latest release for your platform from the [releases page](https://github.com/keathmilligan/stunt/releases/latest), or install via a package manager:

```sh
# Homebrew (macOS / Linux)
brew install keathmilligan/tap/stunt

# Scoop (Windows)
scoop bucket add keathmilligan https://github.com/keathmilligan/scoop-bucket
scoop install stunt

# apt (Debian / Ubuntu)
# See release page for repo setup instructions

# rpm (Fedora / RHEL)
# See release page for repo setup instructions

# cargo
cargo install stunt
```

## Building from Source

```sh
git clone https://github.com/keathmilligan/stunt.git && cd stunt
cargo build --release
# Binary at: target/release/stunt
```

## Usage

```sh
stunt
```

### Key Bindings

#### Normal Mode

| Key | Action |
|-----|--------|
| `n` | New entry |
| `e` | Edit selected entry |
| `d` | Delete selected entry |
| `Enter` | Connect / disconnect selected entry |
| `j` / `k` or arrow keys | Navigate list |
| `q` / `Ctrl+C` | Quit |

#### Form Mode

| Key | Action |
|-----|--------|
| `Tab` / `Down` | Next field |
| `Shift+Tab` / `Up` | Previous field |
| `Ctrl+A` | Add a new forward |
| `Ctrl+D` | Delete selected forward |
| `Ctrl+T` | Cycle forward type (Local / Remote / Dynamic for SSH; Pod / Service / Deployment for K8s) |
| `Enter` | Confirm field / save entry |
| `Esc` | Cancel / go back |

## Configuration

Configuration is stored at:

| Platform | Path |
|----------|------|
| Linux | `~/.local/share/tunnel-mgr/tunnels.toml` |
| macOS | `~/Library/Application Support/tunnel-mgr/tunnels.toml` |
| Windows | `%APPDATA%\tunnel-mgr\tunnels.toml` |

### SSH Entry

```toml
[[entries]]
type = "ssh"
name = "prod-db"
host = "bastion.example.com"
port = 22                         # optional, default 22
user = "deploy"                   # optional
identity_file = "~/.ssh/id_ed25519"  # optional
auto_restart = true               # optional, default false

  [[entries.forwards]]
  type = "local"
  bind_port = 5432
  remote_host = "db.internal"
  remote_port = 5432

  [[entries.forwards]]
  type = "dynamic"
  bind_port = 1080
```

### Kubernetes Entry

```toml
[[entries]]
type = "k8s"
name = "api-debug"
context = "prod"           # optional, uses current context if omitted
namespace = "default"      # optional
resource_type = "deployment"
resource_name = "api-server"
auto_restart = false

  [[entries.forwards]]
  local_port = 8080
  remote_port = 80
```

Supported `resource_type` values: `pod`, `service`, `deployment`.

### sshuttle Entry

```toml
[[entries]]
type = "sshuttle"
name = "corp-vpn"
host = "bastion.example.com"
subnets = ["10.0.0.0/8", "192.168.0.0/16"]
port = 22                         # optional, uses sshuttle default if omitted
user = "alice"                    # optional
identity_file = "~/.ssh/id_ed25519"  # optional
auto_restart = true               # optional, default false
```

## Development

```sh
# Build
cargo build

# Run tests
cargo test

# Lint
cargo clippy -- -D warnings

# Format
cargo fmt
```

## License

MIT
