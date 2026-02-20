## 1. Config Model: Add auto_restart field

- [x] 1.1 Add `auto_restart: bool` field to `ServerEntry` in `config/model.rs` with `#[serde(default)]` (defaults to `false`)
- [x] 1.2 Update `ServerEntry` construction sites in `app.rs` (`submit_form`, `new`) to include `auto_restart` field
- [x] 1.3 Add unit test for round-trip serialization with `auto_restart: true` and `auto_restart` omitted (defaults to false)

## 2. Connection State: Add Suspended variant

- [x] 2.1 Add `Suspended` variant to `ConnectionState` enum in `tunnel/state.rs`
- [x] 2.2 Add label `"suspended"` and update `is_active()` to return `false` for `Suspended`
- [x] 2.3 Update all exhaustive `match` arms on `ConnectionState` across the codebase (ui rendering, app event handling)
- [x] 2.4 Add unit tests for `Suspended` label and `is_active()` behavior

## 3. Session State File: Data model and persistence

- [x] 3.1 Create `config/session.rs` module with `SessionRecord` struct (pid: `Option<u32>`, suspended: `bool`, connected_at: `Option<String>`) and `SessionState` type alias (`HashMap<Uuid, SessionRecord>`)
- [x] 3.2 Implement `load_sessions()` to read `sessions.json` from the data directory, returning empty map on missing/corrupt file with a warning log
- [x] 3.3 Implement `save_sessions()` with atomic write-tmp-then-rename matching the pattern in `config/storage.rs`
- [x] 3.4 Register `config/session.rs` in `config/mod.rs` and re-export public types
- [x] 3.5 Add unit tests: round-trip JSON serialization, missing file returns empty, corrupt file returns empty

## 4. PID Liveness Check

- [x] 4.1 Create `tunnel/pid.rs` module with `is_pid_alive(pid: u32) -> bool` using `libc::kill(pid, 0)` and handling `ESRCH`/`EPERM` return codes
- [x] 4.2 Implement `is_ssh_process(pid: u32) -> bool` that checks `/proc/<pid>/comm` on Linux (reading the file for "ssh") with a fallback that returns `true` if `/proc` is unavailable
- [x] 4.3 Implement `is_live_ssh_tunnel(pid: u32) -> bool` combining both checks: alive AND is ssh process
- [x] 4.4 Register `tunnel/pid.rs` in `tunnel/mod.rs`
- [x] 4.5 Add unit tests for `is_pid_alive` with current process PID (alive) and PID 0/nonexistent PID (dead or permission)

## 5. Detached SSH Process Spawning

- [x] 5.1 Modify `build_ssh_command()` in `tunnel/command.rs` to add `pre_exec` hook calling `libc::setsid()` via `CommandExt::pre_exec` (behind `#[cfg(unix)]`)
- [x] 5.2 After `spawn()`, extract the child PID via `child.id()` before dropping the child handle (since the process is detached, we don't hold the child)
- [x] 5.3 Add integration test that spawns a detached `sleep` command and verifies the PID is alive after dropping the child handle

## 6. Supervisor Refactor: Spawn mode and Adopt mode

- [x] 6.1 Refactor `Supervisor::spawn()` to use the detached spawning from task 5, capture the PID, and begin PID-polling instead of `child.wait()`
- [x] 6.2 Add `Supervisor::adopt()` constructor that takes an existing PID and starts PID-polling without spawning
- [x] 6.3 Implement the PID-polling loop: check `is_live_ssh_tunnel(pid)` every 2 seconds, send `TunnelEvent::Disconnected` when dead
- [x] 6.4 On disconnect detection, check `auto_restart` flag: if true, apply backoff and respawn (detached); if false, send disconnect event and stop
- [x] 6.5 Respect `Suspended` state: add a mechanism (e.g., watch channel or flag) for the supervisor to check if the tunnel has been suspended, and stop reconnect loop if so
- [x] 6.6 On reconnect (respawn), record new PID and send updated session state
- [x] 6.7 Update `Supervisor::cancel()` to only stop the polling task (do NOT kill the SSH process)
- [x] 6.8 Add a `Supervisor::cancel_and_kill()` method that stops polling AND kills the SSH process (for explicit user disconnect)
- [x] 6.9 Add `Supervisor::pid()` getter to expose the current monitored PID

## 7. Session State Integration in App

- [x] 7.1 Add `sessions: SessionState` field to `App` struct and load sessions in `App::new()`
- [x] 7.2 Update `App::connect()` to write a session record (PID, connected_at) after spawning
- [x] 7.3 Update `App::disconnect()` to: if `auto_restart` is true, set state to `Suspended` and update session record; if false, remove session record and set state to `Disconnected`
- [x] 7.4 Update `App::handle_tunnel_event()` for `Disconnected` events to remove session record (when not auto-restarting) or update PID (when supervisor respawns)
- [x] 7.5 Update `App::handle_tunnel_event()` for `Connected` events to update session record with new PID
- [x] 7.6 Call `save_sessions()` after every session state mutation

## 8. Startup Reconciliation

- [x] 8.1 Implement `App::reconcile_sessions()` that iterates loaded session records and applies reconciliation logic per the spec (check PID liveness, set states, clean stale records)
- [x] 8.2 For sessions with `suspended: true`, set `ConnectionState::Suspended` without PID check
- [x] 8.3 For live SSH PIDs, set `Connected` and call `Supervisor::adopt()` to start monitoring
- [x] 8.4 For dead PIDs with `auto_restart: true`, initiate a new connection automatically
- [x] 8.5 For dead PIDs without `auto_restart`, set `Disconnected` and remove session record
- [x] 8.6 Remove session records whose UUID doesn't match any entry in config
- [x] 8.7 Save cleaned session state after reconciliation
- [x] 8.8 Call `reconcile_sessions()` in `App::new()` after loading config and sessions

## 9. Graceful Shutdown: Keep processes alive

- [x] 9.1 Modify `App::shutdown()` to call `Supervisor::cancel()` (stop polling only) instead of killing SSH processes
- [x] 9.2 Leave the session file intact on quit (do not clear it)
- [x] 9.3 Remove the `Message::Quit` handler's implicit disconnect-all behavior — just set `running = false`

## 10. UI: Auto-restart form field

- [x] 10.1 Add "Auto Restart" field to `FormState.fields` in `NewEntry` and `EditEntry` handlers, displaying as `yes`/`no` toggle
- [x] 10.2 Handle toggle input: pressing space or enter on the Auto Restart field cycles between `yes` and `no`
- [x] 10.3 Read `auto_restart` field value in `submit_form()` and set it on the `ServerEntry`
- [x] 10.4 Pre-fill the Auto Restart field from the existing entry's `auto_restart` value in `EditEntry`

## 11. UI: Suspended state rendering

- [x] 11.1 Add color for `Suspended` state in `ui/mod.rs` — use magenta or a distinct color separate from Disconnected (gray) and Failed (red)
- [x] 11.2 Add `"suspended"` label rendering in the server row state indicator
- [x] 11.3 Update status bar to include suspended count (e.g., "1 suspended") alongside connected/failed counts
- [x] 11.4 Update the `ToggleConnect` behavior for `Suspended` state: pressing Enter on a Suspended entry should initiate a connection (clear suspended, transition to Connecting)

## 12. Tests and validation

- [x] 12.1 Run `cargo clippy -- -D warnings` and fix all warnings
- [x] 12.2 Run `cargo test` and fix any failures
- [x] 12.3 Run `cargo fmt` to ensure formatting
- [ ] 12.4 Manual smoke test: start TUI, connect a tunnel, quit, restart, verify tunnel is shown as connected
