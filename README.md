```
  ____  _____           _____
 / ___||_   _|_   _ _ _|_   _|
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

https://github.com/user-attachments/assets/4a80c80a-18e5-4c47-aaa1-9d4930e2eb6f

## Features

`stunt` allows you to define tunnels with SSH, Kubernetes and `sshuttle` that persist when you close the app. While it is running, `stunt` monitors and maintains active tunnel connections.

- SSH local/remote
- Kubernetes port-forwarding
- sshuttle

## Installation

Download the latest release for your platform from the [releases page](https://github.com/keathmilligan/stunt/releases/latest), or install via a package manager:

### macOS (Homebrew)

```bash
brew tap keathmilligan/tap
brew install keathmilligan/tap/unfk
```

Stay up-to-date with `brew upgrade unfk`.

See the [macOS Install Guide](docs/install-macos.md) for other ways to install on macOS.

### Windows (PowerShell)

In an elevated powershell session, run:

```powershell
irm https://packages.keathmilligan.net/unfk/install.ps1 | iex
```

See the [Windows Install Guide](docs/install-windows.md) for other ways to install on Windows.

### Linux (shell installer)

```bash
curl -fsSL https://packages.keathmilligan.net/unfk/install.sh | sh
```

This will install `unfk` into `~/.local/bin`.

See the [Linux Install Guide](docs/install-linux.md) for other ways to install on Linux.

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

## License

MIT
