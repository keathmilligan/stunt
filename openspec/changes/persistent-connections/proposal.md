## Why

SSH tunnels managed by tunnel-mgr die whenever the TUI exits. This forces users to keep the TUI running continuously just to maintain connectivity, and any restart (intentional or crash) drops every active tunnel. Users need tunnels that survive TUI restarts so they can treat the TUI as a management interface rather than a process host.

## What Changes

- **Detached SSH process spawning**: SSH tunnel processes are spawned as OS-independent processes (via `setsid` / process group separation) so they are not children of the TUI and survive TUI exit.
- **Session state persistence**: A session state file tracks which tunnels are active, their PIDs, and metadata (e.g., suspended flag). Written on connect/disconnect/state change, read on startup.
- **Startup state reconciliation**: On launch, the TUI reads the session file, checks whether each recorded SSH process is still alive (e.g., `kill(pid, 0)` or `/proc/<pid>` check), and sets connection state accordingly -- `Connected` if alive, `Disconnected` if dead. Dead entries are cleaned from the session file.
- **In-process monitoring**: While running, the TUI monitors detached SSH processes by periodically polling their PID liveness (since they are no longer child processes, `child.wait()` is not available). The existing supervisor/tokio-task pattern is adapted to poll rather than wait.
- **Auto-restart on disconnect config option**: A new boolean field `auto_restart` on `ServerEntry` (exposed as a checkbox in the form UI). When enabled and the TUI detects that the SSH process has exited, it automatically spawns a replacement. Auto-restart only operates while the TUI is running.
- **Suspended state for auto-restart tunnels**: When a user manually disconnects an auto-restart tunnel, it enters a `Suspended` state that suppresses auto-reconnection. The tunnel stays suspended (and recorded in the session file) until the user explicitly connects it again. This prevents the TUI from fighting the user's intent.
- **Graceful TUI exit**: On quit, the TUI stops monitoring but does **not** kill SSH processes. They continue running independently. The session file is left intact so the next launch can reconcile.

## Capabilities

### New Capabilities
- `session-persistence`: Tracks active tunnel sessions (PIDs, connection metadata, suspended state) in a state file that survives TUI restarts. Provides startup reconciliation to detect live/dead SSH processes by PID liveness checks.

### Modified Capabilities
- `tunnel-config`: Adds `auto_restart` boolean field to `ServerEntry` for opt-in automatic reconnection on disconnect.
- `tunnel-lifecycle`: SSH processes spawned detached from the TUI process tree. Supervisor adapted from child-wait to PID-polling for monitoring detached processes. New `Suspended` connection state for auto-restart tunnels that have been manually disconnected. Auto-restart with exponential backoff only operates while the TUI is running.
- `tui-list-view`: Adds form field for the `auto_restart` checkbox. Adds rendering for the `Suspended` connection state (color, label). Updates status bar to reflect suspended count.

## Impact

- **tunnel/supervisor.rs**: Rewrite monitoring from `child.wait()` to PID-polling since SSH processes are no longer children. Reconnect logic made conditional on `auto_restart` flag. Must respect `Suspended` state.
- **tunnel/command.rs**: SSH processes spawned detached via `setsid` (Unix) or equivalent, so they outlive the TUI. PID captured and recorded to session file.
- **tunnel/state.rs**: New `Suspended` variant in `ConnectionState`. Updated state transition rules (manual disconnect of auto-restart tunnel -> Suspended; explicit connect from Suspended -> Connecting).
- **config/model.rs**: New `auto_restart: bool` field on `ServerEntry` (default `false`) with serde support.
- **config/storage.rs**: New session state file (separate from `tunnels.toml`) for runtime state -- PIDs, suspended flags. Likely at `<data_dir>/tunnel-mgr/sessions.toml` or similar.
- **app.rs**: Startup reconciliation logic. Modified shutdown to leave SSH processes running. State transitions for `Suspended`. Connect/disconnect adapted for detached processes.
- **ui/mod.rs**: Form checkbox for `auto_restart`. `Suspended` state color/label rendering. Status bar updated.
- **main.rs**: Startup sequence reads session file and reconciles state before entering the event loop.
- **No new daemon or background service**: All monitoring and auto-restart happens inside the TUI process while it is running. When the TUI is not running, tunnels persist but are unmonitored.
- **Breaking change**: None. Existing configs without `auto_restart` default to `false`, preserving current behavior. Existing tunnels will be spawned detached going forward, but behavior is otherwise identical.
