## Why

Managing SSH tunnels by hand — remembering flags, port numbers, identity files, and which tunnels are running — is tedious and error-prone. A dedicated TUI gives a persistent, at-a-glance dashboard for defining tunnel configurations and controlling their connection state without leaving the terminal.

## What Changes

- Add a full-screen ratatui TUI that displays SSH server entries as multi-line rows in a scrollable list
- Each row shows server connection summary (host, port, user, identity file) and its associated local/remote tunnel forward definitions
- Arrow-key navigation scrolls by variable-height rows (4-5 lines each), with the selected row highlighted
- Key bindings for creating, editing, deleting tunnel entries and toggling connect/disconnect
- Tunnel definitions are persisted as TOML in the platform-appropriate user data directory (via the `dirs` crate)
- Tunnel connections are managed by spawning supervised `ssh` child processes with auto-reconnect on failure
- Support for local (`-L`), remote (`-R`), and dynamic/SOCKS (`-D`) port-forwarding types

## Capabilities

### New Capabilities

- `tunnel-config`: Tunnel and server definition data model, TOML serialization/deserialization, and persistent storage in the platform user data directory
- `tunnel-lifecycle`: Spawning, monitoring, reconnecting, and stopping `ssh` processes for each tunnel entry
- `tui-list-view`: Main scrollable list view with multi-line, variable-height rows, arrow-key navigation, row highlighting, and key-bound actions (new/edit/delete/connect/disconnect/quit)

### Modified Capabilities

_(none — greenfield project)_

## Impact

- **New crate dependencies:** ratatui, crossterm, tokio, serde, toml, dirs, thiserror, anyhow
- **External dependency:** a working `ssh` binary on `$PATH`
- **Filesystem:** reads/writes TOML config files under the platform user data directory (e.g., `~/.local/share/tunnel-mgr/` on Linux, `~/Library/Application Support/tunnel-mgr/` on macOS)
- **Processes:** spawns and supervises long-running `ssh` child processes
