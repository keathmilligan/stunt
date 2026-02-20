## Context

tunnel-mgr is a TUI application that manages SSH tunnel connections. Currently, SSH processes are spawned as child processes of the TUI via `tokio::process::Command`. The `Supervisor` monitors each child using `child.wait()` and handles reconnection with exponential backoff. All connection state is in-memory -- when the TUI exits, `App::shutdown()` kills every active SSH process, and on next launch all entries start as `Disconnected`.

This means users must keep the TUI running at all times to maintain tunnels, and any TUI restart (crash, intentional quit, terminal close) drops all connections.

The goal is to make SSH tunnels survive TUI restarts while keeping the architecture simple: no separate daemon, no IPC, no background service. The TUI remains the sole management interface.

## Goals / Non-Goals

**Goals:**
- SSH tunnel processes survive TUI exit and are adopted on next launch
- Session state file records active PIDs so the TUI can reconcile on startup
- User-configurable auto-restart per tunnel, only active while the TUI is running
- Suspended state prevents auto-restart from fighting user intent on manual disconnect
- Zero-config upgrade path: existing configs work unchanged

**Non-Goals:**
- Background daemon or separate supervisor process
- Auto-restart while the TUI is not running (tunnels persist but are unmonitored)
- Remote/distributed tunnel management
- Cross-platform detached process support beyond Unix (Windows support is out of scope for this change; `setsid` is Unix-only)
- Monitoring tunnels at the network/port level (liveness is PID-based only)

## Decisions

### 1. Detach SSH processes using `setsid` via `pre_exec`

**Decision:** Use `CommandExt::pre_exec` to call `libc::setsid()` before exec, creating a new session for each SSH process. This makes the SSH process independent of the TUI's process group and terminal session.

**Alternatives considered:**
- **Double-fork:** Classic Unix daemonization. More complex, harder to capture the final PID reliably, and unnecessary since `setsid` in a `pre_exec` hook achieves the same detachment.
- **`nohup` wrapper:** Spawning via `nohup ssh ...`. Adds an unnecessary intermediate process, complicates PID tracking, and `setsid` is more direct.
- **`process_group(0)` (std::os::unix):** Creates a new process group but not a new session. The process could still receive signals from the terminal. `setsid` provides stronger detachment.

**Rationale:** `pre_exec` + `setsid()` is the simplest single-call mechanism that fully detaches the child. The SSH process gets its own session, won't receive SIGHUP when the TUI's terminal closes, and its PID is directly available from `spawn()`.

### 2. PID-based liveness polling instead of `child.wait()`

**Decision:** Since detached processes are not children of the TUI, replace `child.wait()` with periodic PID liveness checks using `kill(pid, 0)` (signal 0 -- tests if the process exists without sending a signal). Poll every 2 seconds.

**Alternatives considered:**
- **`/proc/<pid>/stat` reading:** Linux-specific, doesn't work on macOS. `kill(pid, 0)` is POSIX-portable across Unix systems.
- **`waitid` / `pidfd_open`:** Linux-specific APIs. Not portable.
- **Longer poll interval (e.g., 10s):** Reduces CPU overhead but increases latency for detecting disconnects. 2 seconds is a reasonable balance -- fast enough for responsive UI, cheap enough for dozens of tunnels.

**Rationale:** `kill(pid, 0)` is the standard POSIX way to check process existence. It's a single syscall, works on Linux and macOS, and returns `ESRCH` if the process is gone. Combined with validating that the process is actually an `ssh` process (checking `/proc/<pid>/comm` on Linux or process name via `sysinfo`), this prevents PID-reuse false positives.

### 3. Session state file at `<data_dir>/tunnel-mgr/sessions.json`

**Decision:** Store runtime session state in a separate JSON file alongside the config. Each entry maps a tunnel UUID to its session record (PID, suspended flag, connected-at timestamp).

**Format: JSON** (not TOML) because this is runtime state that changes frequently and is machine-read/written, not user-edited.

**Alternatives considered:**
- **TOML (same as config):** TOML is good for human-edited config but verbose for runtime state. JSON is more natural for transient machine state.
- **SQLite:** Overkill for a handful of key-value records. Adds a dependency for no benefit.
- **PID files per tunnel (e.g., `<data_dir>/tunnel-mgr/pids/<uuid>.pid`):** Filesystem-as-database approach. More files to manage, harder to atomically update multiple entries, no benefit over a single JSON file.
- **Embed in `tunnels.toml`:** Mixes persistent config with transient runtime state. A config edit shouldn't require understanding session fields, and a crash shouldn't corrupt the config file.

**Rationale:** A single `sessions.json` file keeps runtime state separate from user configuration, is trivial to read/write atomically (write-tmp-rename), and is easy to inspect for debugging.

### 4. Auto-restart is TUI-only, not a background service

**Decision:** Auto-restart (reconnect on unexpected exit) only operates while the TUI is running. When the TUI is not running, detached SSH processes persist but are unmonitored. If an SSH process dies while the TUI is off, it stays dead until the next TUI launch reconciles state.

**Alternatives considered:**
- **Background daemon:** A separate long-running process that monitors and restarts tunnels. Significantly more complex (IPC, process management, startup integration, logging), and the user specifically requested against this approach.
- **systemd/launchd integration:** Platform-specific, heavy, and shifts complexity to the OS service manager.

**Rationale:** The TUI-only approach keeps the architecture simple. The primary value of persistent connections is surviving TUI restarts and brief disconnects -- not 24/7 unattended monitoring. Users who need always-on restart can simply leave the TUI running (which is no worse than today, and better because tunnels survive accidental closes).

### 5. Suspended state on manual disconnect of auto-restart tunnels

**Decision:** Add a `Suspended` variant to `ConnectionState`. When the user manually disconnects a tunnel that has `auto_restart: true`, the state transitions to `Suspended` instead of `Disconnected`. The supervisor will not attempt to reconnect a suspended tunnel. Suspended state is persisted in the session file so it survives TUI restarts.

**State transitions involving Suspended:**
- `Connected` + user disconnect + `auto_restart=true` → `Suspended`
- `Connected` + user disconnect + `auto_restart=false` → `Disconnected` (unchanged)
- `Suspended` + user connect → `Connecting` (clears suspended, resumes normal lifecycle)
- `Reconnecting` + user disconnect + `auto_restart=true` → `Suspended`
- TUI startup + session shows suspended → `Suspended` (no PID check needed, tunnel is intentionally stopped)

**Rationale:** Without Suspended, an auto-restart tunnel would immediately reconnect after the user disconnects it, making it impossible to intentionally stop. Suspended is the user's explicit "I want this off" override that the supervisor must respect.

### 6. Startup reconciliation sequence

**Decision:** On launch, before entering the event loop:
1. Load `tunnels.toml` (tunnel definitions) -- existing behavior
2. Load `sessions.json` (runtime state)
3. For each session record:
   - If `suspended: true` → set state to `Suspended`, do not check PID
   - If PID is alive (and is an `ssh` process) → set state to `Connected`, start a polling supervisor to monitor it
   - If PID is dead → set state to `Disconnected`, remove from session file
   - If tunnel UUID in session file doesn't exist in config → remove stale record
4. For tunnels with `auto_restart: true` that reconciled as `Disconnected` (SSH died while TUI was off) → automatically initiate connection
5. Write cleaned session file back to disk

**Rationale:** This is the core mechanism that makes persistent connections work. Step 4 means that auto-restart tunnels self-heal even across TUI restarts -- if the SSH process died while the TUI was off, the TUI reconnects it on launch.

### 7. Supervisor refactor: two modes (child and adopted)

**Decision:** The `Supervisor` will support two modes:
- **Spawn mode** (new connections): Spawns a detached SSH process, records the PID in the session file, then begins PID-polling to monitor it.
- **Adopt mode** (startup reconciliation): Takes an existing PID from the session file and begins PID-polling without spawning anything.

Both modes share the same polling loop and reconnect logic. The only difference is whether the supervisor spawns the initial process or inherits a PID.

**Alternatives considered:**
- **Separate `AdoptedSupervisor` struct:** Duplicates the monitoring/reconnect logic. Better to have one `Supervisor` with a parameterized start.

**Rationale:** Keeps the supervisor as a single abstraction. The spawn-vs-adopt distinction is only relevant at initialization; after that, all supervisors behave identically.

### 8. `auto_restart` field on `ServerEntry` with `false` default

**Decision:** Add `auto_restart: bool` to `ServerEntry`, defaulting to `false`, serialized with `#[serde(default)]`. Exposed in the form UI as a toggle field (the form doesn't have native checkboxes -- use a text-mode toggle like `[x]`/`[ ]` or `yes`/`no` cycling on keypress).

**Rationale:** Default `false` preserves existing behavior for all current users. The field is part of the tunnel definition (not runtime state) because it's a user preference about how the tunnel should behave, not ephemeral session data.

### 9. Graceful shutdown: stop monitoring, keep processes alive

**Decision:** On `Message::Quit`, the TUI:
1. Cancels all supervisor polling tasks (stops monitoring)
2. Does **not** kill any SSH processes
3. Leaves the session file intact
4. Exits cleanly

**Change from current behavior:** Today, `App::shutdown()` iterates supervisors and calls `cancel()`, which calls `child.kill()`. With detached processes, `cancel()` will only stop the polling task -- there is no child handle to kill.

**For explicit "disconnect all and quit":** A separate action (e.g., `Shift+Q` or a confirm dialog) could kill all processes and clear the session file, but this is not required for the initial implementation.

## Risks / Trade-offs

**[PID reuse false positive]** → After a long TUI absence, a recorded PID could be reused by a completely different process. **Mitigation:** Validate that the process at the recorded PID is actually an `ssh` process by checking the process name (e.g., `/proc/<pid>/comm` on Linux, `sysctl` on macOS). If it's not `ssh`, treat as dead.

**[Orphaned SSH processes]** → If the user deletes a tunnel definition while the TUI is off, its SSH process is never cleaned up. **Mitigation:** Startup reconciliation step removes session records for tunnel UUIDs that no longer exist in config, but does not kill the orphaned process (we don't own it anymore). Document that users should disconnect before deleting, or add a cleanup step that kills the orphan.

**[Poll latency]** → 2-second polling means up to 2 seconds before the UI reflects a disconnect. **Mitigation:** Acceptable for this use case. Users can see the state is slightly delayed. The poll interval could be made configurable later if needed.

**[No auto-restart when TUI is off]** → If an SSH process dies while the TUI isn't running, it stays dead until the next launch. **Mitigation:** Startup reconciliation with `auto_restart` step 4 handles this -- the tunnel is reconnected when the TUI starts. This is an explicit non-goal trade-off for architectural simplicity.

**[Unix-only `setsid`]** → `setsid` and `pre_exec` are Unix-specific (`std::os::unix::process::CommandExt`). **Mitigation:** This change is explicitly scoped to Unix. Windows support would require a different detach mechanism (`CREATE_NEW_PROCESS_GROUP`, `DETACHED_PROCESS` flags) and can be added later behind a `cfg(target_os)` gate.

**[Session file corruption on crash]** → If the TUI crashes mid-write of the session file, it could be left in a corrupt state. **Mitigation:** Use the same atomic write-tmp-then-rename strategy already used for `tunnels.toml`. On read failure, treat as empty (all tunnels start disconnected) and log a warning.

## Open Questions

- Should there be a "disconnect all and quit" keybinding (e.g., `Shift+Q`) in addition to the normal quit that leaves tunnels running? Or should quit always prompt?
- Should `auto_restart` tunnels that reconcile as disconnected on startup be connected immediately (current design) or require user confirmation?
- What's the right poll interval? 2 seconds is proposed but could be tuned. Should it be configurable?
