## ADDED Requirements

### Requirement: Session state file storage
The system SHALL store runtime session state in a JSON file named `sessions.json` inside the same `tunnel-mgr` subdirectory of the platform user data directory used for `tunnels.toml`. The session file SHALL be separate from the tunnel configuration file.

#### Scenario: Session file location on Linux
- **WHEN** running on Linux
- **THEN** the session file path is `~/.local/share/tunnel-mgr/sessions.json`

#### Scenario: Session file does not exist on startup
- **WHEN** the app starts and no `sessions.json` exists
- **THEN** the system treats all tunnel entries as `Disconnected` and proceeds normally without error

#### Scenario: Session file is corrupt or unreadable
- **WHEN** the app starts and `sessions.json` exists but cannot be parsed
- **THEN** the system logs a warning, treats all entries as `Disconnected`, and continues startup

### Requirement: Session record data model
Each session record SHALL contain: the tunnel entry UUID (as the key), the PID of the SSH process (integer), a suspended flag (boolean), and a connected-at timestamp (ISO 8601 string). The session file SHALL be a JSON object mapping UUID strings to session record objects.

#### Scenario: Session record for an active tunnel
- **WHEN** a tunnel is connected with PID 12345
- **THEN** the session file contains an entry mapping the tunnel's UUID to `{ "pid": 12345, "suspended": false, "connected_at": "2026-02-19T10:30:00Z" }`

#### Scenario: Session record for a suspended tunnel
- **WHEN** a tunnel is suspended by the user
- **THEN** the session file contains an entry mapping the tunnel's UUID to `{ "pid": null, "suspended": true, "connected_at": null }`

### Requirement: Atomic session file writes
The system SHALL write session state changes atomically using the same write-to-temp-then-rename strategy used for `tunnels.toml`. The system SHALL update the session file on every connection state change (connect, disconnect, suspend, process exit detection).

#### Scenario: Session file updated on connect
- **WHEN** a tunnel transitions to Connected with a known PID
- **THEN** the session file is atomically updated to include the new session record

#### Scenario: Session file updated on disconnect
- **WHEN** a tunnel is disconnected or its process is detected as dead
- **THEN** the session record for that tunnel is removed from the session file atomically

#### Scenario: Crash during session file write
- **WHEN** the TUI crashes mid-write of the session file
- **THEN** the original session file remains intact (temp file is orphaned)

### Requirement: Startup state reconciliation
On startup, after loading the tunnel configuration, the system SHALL load the session file and reconcile connection state for each recorded session. For each session record, the system SHALL:
1. If the tunnel UUID does not exist in the loaded config, remove the stale session record.
2. If the session record has `suspended: true`, set the tunnel's state to `Suspended`.
3. If the session record has a PID, check whether the process is alive AND is an `ssh` process. If alive, set state to `Connected` and begin monitoring. If dead, set state to `Disconnected` and remove the session record.
4. For tunnels with `auto_restart: true` that reconciled as `Disconnected` (process died while TUI was off), automatically initiate a new connection.
5. Write the cleaned session file back to disk.

#### Scenario: Reconcile live SSH process
- **WHEN** the session file records PID 12345 for tunnel "prod-db" and PID 12345 is a running `ssh` process
- **THEN** the tunnel's state is set to `Connected` and a polling supervisor is started to monitor PID 12345

#### Scenario: Reconcile dead SSH process
- **WHEN** the session file records PID 12345 for tunnel "prod-db" and PID 12345 does not exist
- **THEN** the tunnel's state is set to `Disconnected` and the session record is removed

#### Scenario: Reconcile dead process with auto-restart
- **WHEN** the session file records a dead PID for tunnel "prod-db" and the tunnel has `auto_restart: true`
- **THEN** the tunnel's state is set to `Disconnected`, the stale session record is removed, and a new connection is automatically initiated

#### Scenario: Reconcile suspended tunnel
- **WHEN** the session file records `suspended: true` for tunnel "staging-api"
- **THEN** the tunnel's state is set to `Suspended` regardless of any PID value, and no connection is attempted

#### Scenario: Stale session record for deleted tunnel
- **WHEN** the session file contains a UUID that does not match any entry in `tunnels.toml`
- **THEN** the stale session record is removed from the session file

### Requirement: PID liveness check
The system SHALL determine whether a recorded PID is alive using `kill(pid, 0)` (POSIX signal 0). If the process exists, the system SHALL additionally verify that the process is an `ssh` process to guard against PID reuse. If `kill` returns `ESRCH`, the process is considered dead. If `kill` returns `EPERM`, the process is considered alive (exists but owned by another user).

#### Scenario: PID is alive and is ssh
- **WHEN** `kill(pid, 0)` succeeds and the process name is `ssh`
- **THEN** the PID is considered a live SSH tunnel process

#### Scenario: PID is alive but not ssh
- **WHEN** `kill(pid, 0)` succeeds but the process name is not `ssh`
- **THEN** the PID is considered dead (PID reuse detected) and the session record is removed

#### Scenario: PID does not exist
- **WHEN** `kill(pid, 0)` returns `ESRCH`
- **THEN** the PID is considered dead

#### Scenario: PID exists but permission denied
- **WHEN** `kill(pid, 0)` returns `EPERM`
- **THEN** the PID is considered alive (process exists but belongs to another user, which is unexpected but safe to treat as alive)
