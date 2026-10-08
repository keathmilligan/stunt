```
  ____  _____           _____
 / ___||_   _|_   _ _ _|_   _|
 \___ \  | | | | | | '_ \| |
  ___) | | | | |_| | | | | |
 |____/  |_|  \__,_|_| |_|_|
  Stupid Tunnel Tricks
```

**S**tupid **Tun**nel **T**ricks — a terminal user interface for defining, configuring, and managing SSH tunnels, Kubernetes port-forwards, sshuttle VPN sessions, and AWS SSM port-forwards.

---

[![CI](https://github.com/keathmilligan/stunt/actions/workflows/ci.yml/badge.svg)](https://github.com/keathmilligan/stunt/actions/workflows/ci.yml)
[![Release](https://github.com/keathmilligan/stunt/actions/workflows/release.yml/badge.svg)](https://github.com/keathmilligan/stunt/actions/workflows/release.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

[![macOS DMG](https://packages.keathmilligan.net/stunt/badges/macos-dmg.svg)](https://github.com/keathmilligan/stunt/releases/latest)
[![Windows MSI](https://packages.keathmilligan.net/stunt/badges/msi.svg)](https://github.com/keathmilligan/stunt/releases/latest)
[![Homebrew](https://packages.keathmilligan.net/stunt/badges/homebrew.svg)](https://github.com/keathmilligan/stunt/releases/latest)
[![Scoop](https://packages.keathmilligan.net/stunt/badges/scoop.svg)](https://github.com/keathmilligan/stunt/releases/latest)
[![apt](https://packages.keathmilligan.net/stunt/badges/apt.svg)](https://github.com/keathmilligan/stunt/releases/latest)
[![rpm](https://packages.keathmilligan.net/stunt/badges/rpm.svg)](https://github.com/keathmilligan/stunt/releases/latest)
[![crates.io](https://packages.keathmilligan.net/stunt/badges/crates-io.svg)](https://github.com/keathmilligan/stunt/releases/latest)
[![Install Scripts](https://packages.keathmilligan.net/stunt/badges/install-scripts.svg)](https://github.com/keathmilligan/stunt/releases/latest)

https://github.com/user-attachments/assets/4a80c80a-18e5-4c47-aaa1-9d4930e2eb6f

## Features

`stunt` allows you to define tunnels with SSH, Kubernetes, `sshuttle`, and AWS SSM that persist when you close the app. While it is running, `stunt` monitors and maintains active tunnel connections.

- SSH local/remote
- Kubernetes port-forwarding
- sshuttle
- AWS Systems Manager Session Manager port-forwarding (managed node or remote host)

## Installation

Download the latest release for your platform from the [releases page](https://github.com/keathmilligan/stunt/releases/latest), or install via a package manager:

### macOS (Homebrew)

```bash
brew tap keathmilligan/tap
brew install keathmilligan/tap/stunt
```

Stay up-to-date with `brew upgrade stunt`.

See the [macOS Install Guide](docs/install-macos.md) for other ways to install on macOS.

### Windows (winget)

In an elevated powershell session, run:

```powershell
winget install stunt
```

See the [Windows Install Guide](docs/install-windows.md) for other ways to install on Windows.

### Linux (shell installer)

```bash
curl -fsSL https://packages.keathmilligan.net/stunt/install.sh | sh
```

This will install `stunt` into `~/.local/bin`.

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
| `←` / `→` (or `Space`) | Change a selection field (e.g. **Resource Type**: Pod / Service / Deployment) |
| `Ctrl+A` | Add a new forward / port binding |
| `Ctrl+D` | Delete selected forward / port binding |
| `Ctrl+T` | Cycle SSH forward type (Local / Remote / Dynamic) while editing a forward |
| `Enter` | Confirm field / save entry |
| `Esc` | Cancel / go back |

> **Resource Type** is a selection field, not free text. Use `←` / `→`
> (or `Space`) to choose between **Pod**, **Service**, and **Deployment**.

## Configuration

Configuration is stored at:

| Platform | Path |
|----------|------|
| Linux | `~/.local/share/stunt/tunnels.toml` |
| macOS | `~/Library/Application Support/stunt/tunnels.toml` |
| Windows | `%APPDATA%\stunt\tunnels.toml` |

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

A Kubernetes entry wraps `kubectl port-forward`. Each entry targets a single
resource (`<resource_type>/<resource_name>`) and may declare **multiple port
bindings** — they are all forwarded by a single `kubectl port-forward`
invocation, exactly like passing several `<local>:<remote>` arguments on the
command line.

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

#### Example: replicating a `kubectl port-forward` command

The command:

```sh
kubectl -n my-platform port-forward \
  svc/my-platform-server-tls 18443:8443 19443:9443
```

is expressed as a single entry with **two port bindings**:

```toml
[[entries]]
type = "k8s"
name = "platform-server"
namespace = "my-platform"
resource_type = "service"             # svc/ → "service"
resource_name = "my-platform-server-tls"
auto_restart = false

  [[entries.forwards]]
  local_port = 18443
  remote_port = 8443

  [[entries.forwards]]
  local_port = 19443
  remote_port = 9443
```

In the TUI, create a Kubernetes entry, set **Resource Type** to `service`
(using `←` / `→`), enter the resource name and namespace, then add each port
binding with `Ctrl+A`. All bindings run under one `kubectl` process; the entry
is considered "connected" once the first local listener is reachable.

> **Note:** `svc` is shorthand for `service`. stunt uses the full
> `service` / `pod` / `deployment` forms.

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

### AWS SSM Entry

Press `n`, select **AWS SSM**, and fill in the target managed-node ID and ports.
Each entry opens one Session Manager port forward. Create separate entries for
additional ports. **Remote Host** is optional: leave it blank to connect to a
port on the target node, or set it to a hostname reachable from that node (for
example, an RDS endpoint).

```toml
[[entries]]
type = "ssm"
name = "prod-db"
target = "i-0123456789abcdef0"
profile = "prod"                  # optional; uses the AWS CLI credential chain
region = "us-east-1"              # optional; uses the AWS CLI region default
local_port = 15432
remote_port = 5432
remote_host = "db.internal"       # optional; omit to forward to the node itself
auto_restart = true               # optional, default false
```

Ports must be between 1 and 65535. The Session Manager plugin binds locally to
loopback; connect your client to `localhost:15432` in this example. Profile and
region can come from your AWS configuration or environment. STunT does not store
AWS credentials or perform interactive sign-in; authenticate in your shell first
(for example, `aws sso login --profile prod` for an SSO profile).

Install **AWS CLI v2** and the [Session Manager plugin](https://docs.aws.amazon.com/systems-manager/latest/userguide/session-manager-working-with-install-plugin.html)
on your PATH. The target must be an online SSM managed node, and your AWS identity
must have permission to start sessions using the corresponding forwarding document.
Managed-node forwarding requires SSM Agent 2.3.672.0 or later; remote-host forwarding
requires 3.1.1374.0 or later, plus connectivity and DNS resolution from the managed
node to the remote host. See [AWS port-forwarding prerequisites and examples](https://docs.aws.amazon.com/systems-manager/latest/userguide/session-manager-working-with-sessions-start.html).

STunT invokes `aws ssm start-session` with `AWS-StartPortForwardingSession` or
`AWS-StartPortForwardingSessionToRemoteHost`, captures its output, and checks the
local listener before showing **connected**. It holds the initial probe open for
three seconds to allow the plugin to report destination errors, then continues
watching stdout and stderr. `Connection to destination port failed` fails the
tunnel even if the plugin stays running; auto-restart retries with backoff when
enabled. This detects reported failures, not application health. A port already
in use is rejected.
Use `Enter` to disconnect; this also terminates the plugin process. Quitting
STunT leaves active tunnels running, and reopening it adopts saved sessions.
Auto-restart runs only while STunT is open. As with other providers, logs from an
adopted process are unavailable until the next reconnect, so destination-error
monitoring also requires disconnecting and reconnecting an adopted SSM tunnel.

## Architecture

- `config/`: tagged TOML entry models and configuration/session persistence.
- `app.rs`: application state, form validation, and event-driven state transitions.
- `ui/`: read-only rendering of entries, forms, status, and process logs.
- `tunnel/command.rs`: provider-specific command construction using argument arrays.
- `tunnel/supervisor.rs`: shared detached-process lifecycle, readiness, logs,
  reconnect backoff, and adoption of saved processes.
- `event/` and `main.rs`: terminal/tunnel event delivery and the application loop.

SSM uses the same supervisor and event flow as the other providers. The AWS CLI
handles authentication and launches the Session Manager plugin for the data channel.

## License

MIT
