# Capability: Tunnel Lifecycle

## Purpose
Defines SSH process construction, spawning, connection state management, exponential backoff reconnection, process supervision, and startup validation.

## Requirements

### Requirement: Build ssh command from server entry
The system SHALL construct an `ssh` command-line invocation from a `ServerEntry` by assembling the host, port (`-p`), user (`-l`), identity file (`-i`), and all tunnel forward flags (`-L`, `-R`, `-D`). The command SHALL include `-N` (no remote command) and `-o ExitOnForwardFailure=yes`.

#### Scenario: Server with local and remote forwards
- **WHEN** a server entry has host "bastion.example.com", user "deploy", and two forwards (one local, one remote)
- **THEN** the constructed command is `ssh -N -o ExitOnForwardFailure=yes -l deploy -L <local_spec> -R <remote_spec> bastion.example.com`

#### Scenario: Server with only dynamic forward
- **WHEN** a server entry has host "proxy.example.com", port 2222, and one dynamic forward on port 1080
- **THEN** the constructed command is `ssh -N -o ExitOnForwardFailure=yes -p 2222 -D 1080 proxy.example.com`

#### Scenario: Server with identity file
- **WHEN** a server entry specifies identity_file as `~/.ssh/id_ed25519`
- **THEN** the constructed command includes `-i ~/.ssh/id_ed25519`

### Requirement: Spawn ssh process
The system SHALL spawn the `ssh` command as an async child process using `tokio::process::Command`. The process SHALL be spawned with stdin, stdout, and stderr captured (not inherited) so it does not interfere with the TUI.

#### Scenario: Successful spawn
- **WHEN** `ssh` is available on PATH and the command is valid
- **THEN** a child process is spawned and the server entry's connection state transitions to Connecting

#### Scenario: ssh binary not found
- **WHEN** `ssh` is not available on PATH
- **THEN** the spawn fails with a descriptive error and the connection state transitions to Failed

### Requirement: Connection state machine
Each server entry SHALL maintain a connection state with the following values: `Disconnected`, `Connecting`, `Connected`, `Reconnecting`, `Failed`. State transitions SHALL follow these rules:
- `Disconnected` → `Connecting` (on user connect action)
- `Connecting` → `Connected` (after process has been running for a stability threshold, e.g. 3 seconds)
- `Connecting` → `Failed` (if process exits immediately)
- `Connected` → `Reconnecting` (if process exits unexpectedly)
- `Reconnecting` → `Connecting` (after backoff delay)
- `Reconnecting` → `Failed` (after max retries exhausted)
- Any state → `Disconnected` (on user disconnect action)

#### Scenario: Successful connection
- **WHEN** the user triggers connect and the ssh process starts and remains running for the stability threshold
- **THEN** the state transitions from Disconnected → Connecting → Connected

#### Scenario: Immediate failure
- **WHEN** the ssh process exits within the stability threshold after spawn
- **THEN** the state transitions from Connecting → Failed

#### Scenario: Unexpected disconnect with reconnect
- **WHEN** a Connected server's ssh process exits unexpectedly
- **THEN** the state transitions to Reconnecting, waits for the backoff delay, then transitions to Connecting and spawns a new process

#### Scenario: Max retries exhausted
- **WHEN** reconnection attempts exceed the max retry limit
- **THEN** the state transitions to Failed and no further automatic reconnection is attempted

#### Scenario: User-initiated disconnect
- **WHEN** the user triggers disconnect on a Connected or Reconnecting entry
- **THEN** the ssh process is killed (SIGTERM), any pending reconnect is cancelled, and the state transitions to Disconnected

### Requirement: Exponential backoff on reconnect
The system SHALL use exponential backoff for reconnection delays, starting at 1 second and doubling on each consecutive failure, capped at 60 seconds. The backoff counter SHALL reset to 1 second after a successful connection (state reaches Connected).

#### Scenario: Successive reconnect delays
- **WHEN** a server fails to connect three times in a row
- **THEN** the delays are 1s, 2s, 4s before the fourth attempt

#### Scenario: Backoff cap
- **WHEN** the computed backoff exceeds 60 seconds
- **THEN** the delay is capped at 60 seconds

#### Scenario: Backoff reset after success
- **WHEN** a reconnection succeeds and the state reaches Connected, then later the connection drops again
- **THEN** the backoff restarts from 1 second

### Requirement: Process supervision via tokio task
Each server entry with an active connection SHALL have a dedicated tokio task that monitors the ssh child process. The task SHALL detect process exit via `child.wait()`, send a status update message to the app's event channel, and handle reconnect logic. The task SHALL be cancellable via a `tokio::sync::watch` or `CancellationToken` when the user disconnects.

#### Scenario: Process exits while monitored
- **WHEN** the ssh child process exits with a non-zero status
- **THEN** the supervision task sends a `TunnelEvent::Disconnected { entry_id, exit_code }` message to the app event channel

#### Scenario: User cancels supervision
- **WHEN** the user disconnects a server entry
- **THEN** the supervision task receives a cancellation signal, kills the child process, and terminates cleanly

### Requirement: Startup ssh availability check
The system SHALL verify that the `ssh` binary is available on PATH at application startup. If `ssh` is not found, the system SHALL display a clear error message and exit.

#### Scenario: ssh available
- **WHEN** the app starts and `ssh` is found on PATH
- **THEN** startup continues normally

#### Scenario: ssh not available
- **WHEN** the app starts and `ssh` is not found on PATH
- **THEN** the app prints an error message to stderr and exits with a non-zero status code
