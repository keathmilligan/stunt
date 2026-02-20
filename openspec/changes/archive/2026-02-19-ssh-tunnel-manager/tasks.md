## 1. Project Scaffolding

- [x] 1.1 Initialize Cargo project with `cargo init` and configure Cargo.toml with dependencies (ratatui, crossterm, tokio, serde, toml, dirs, thiserror, anyhow)
- [x] 1.2 Create module structure: `src/main.rs`, `src/app.rs`, `src/ui/`, `src/tunnel/`, `src/config/`, `src/event/`
- [x] 1.3 Add a `.gitignore` for Rust build artifacts

## 2. Data Model & Config (tunnel-config)

- [x] 2.1 Define `ServerEntry` struct with fields: name, host, port (default 22), user (Option), identity_file (Option), forwards (Vec<TunnelForward>)
- [x] 2.2 Define `TunnelForward` enum with variants: Local (bind_address, bind_port, remote_host, remote_port), Remote (bind_address, bind_port, remote_host, remote_port), Dynamic (bind_address, bind_port)
- [x] 2.3 Implement serde Serialize/Deserialize for `ServerEntry` and `TunnelForward` with TOML-friendly `[[server]]` / `[[server.forwards]]` schema and `type` tag field
- [x] 2.4 Implement config file path resolution using `dirs::data_dir()` + `tunnel-mgr/tunnels.toml`
- [x] 2.5 Implement `load()` — read and deserialize TOML from disk, create directory/file if missing, return empty list on first run
- [x] 2.6 Implement `save()` — atomic write via tempfile + rename, with `.bak` of previous file
- [x] 2.7 Write unit tests for round-trip serialization, missing optional fields, invalid TOML, and atomic save behavior

## 3. SSH Command & Process Spawning (tunnel-lifecycle)

- [x] 3.1 Implement `build_ssh_command()` that constructs a `tokio::process::Command` from a `ServerEntry` (host, -p, -l, -i, -N, -o ExitOnForwardFailure=yes, -L/-R/-D flags)
- [x] 3.2 Define `ConnectionState` enum: Disconnected, Connecting, Connected, Reconnecting, Failed
- [x] 3.3 Define `TunnelEvent` message type for communication between supervision tasks and the app (Disconnected, Connected, Failed, etc.)
- [x] 3.4 Implement spawn logic that starts the ssh child process with stdin/stdout/stderr captured and transitions state to Connecting
- [x] 3.5 Implement supervision tokio task: monitor child via `child.wait()`, detect exit, send `TunnelEvent` to app channel, handle reconnect with exponential backoff (1s base, 2x, capped 60s)
- [x] 3.6 Implement cancellation of supervision task via `CancellationToken` on user disconnect, with SIGTERM to child process
- [x] 3.7 Implement startup check for `ssh` binary on PATH (exit with clear error if missing)
- [x] 3.8 Write unit tests for command building and state transition logic

## 4. Event System

- [x] 4.1 Define `AppEvent` enum covering terminal input events (key, resize) and tunnel status events (`TunnelEvent`)
- [x] 4.2 Implement terminal event reader using crossterm `EventStream` feeding into a `tokio::sync::mpsc` channel
- [x] 4.3 Wire up the main `tokio::select!` loop that multiplexes terminal events and tunnel events

## 5. App State & TEA Loop

- [x] 5.1 Define `App` struct holding: server entry list, selected index, scroll offset, connection states, transient status message, and running flag
- [x] 5.2 Define `Message` enum for all user actions (NavigateUp, NavigateDown, NewEntry, EditEntry, DeleteEntry, ToggleConnect, Quit) and tunnel events
- [x] 5.3 Implement `update()` — match on Message and mutate App state accordingly
- [x] 5.4 Implement connect/disconnect action handlers that spawn/cancel supervision tasks and update connection state
- [x] 5.5 Implement delete handler that disconnects if active, removes entry, adjusts selection, and persists config
- [x] 5.6 Wire up `main.rs`: init terminal, load config, create App, run event loop, restore terminal on exit

## 6. UI Rendering (tui-list-view)

- [x] 6.1 Implement three-region Layout (title bar 1 line, list viewport Min(0), status bar 1 line)
- [x] 6.2 Render title bar: "tunnel-mgr" left-aligned, key hints right-aligned, bold on colored background
- [x] 6.3 Render status bar: entry count, connected count, failed count, transient message, distinct background color
- [x] 6.4 Implement multi-line row rendering for each server entry: header line (name, state indicator, host:port, user, identity) + one line per forward (type label + addressing)
- [x] 6.5 Apply semantic color palette: green (Connected), red (Failed), yellow (Connecting/Reconnecting), dim/gray (Disconnected), cyan (forward type labels), bold white (server name)
- [x] 6.6 Implement row selection highlight (reverse or blue background for selected row)
- [x] 6.7 Implement variable-height scroll logic: compute cumulative row heights, adjust scroll offset to keep selected row fully visible
- [x] 6.8 Integrate all rendering into a top-level `ui()` function called from the main loop

## 7. Input Forms (new/edit)

- [x] 7.1 Implement a modal input form for creating a new server entry (fields: name, host, port, user, identity_file)
- [x] 7.2 Implement forward definition sub-form within the entry form (type selector, addressing fields)
- [x] 7.3 Implement edit mode that pre-fills the form with the selected entry's current values
- [x] 7.4 Wire form submission to persist config and return to list view

## 8. Integration & Polish

- [x] 8.1 Ensure clean shutdown: terminate all ssh processes on quit
- [x] 8.2 Handle terminal resize events and re-render layout
- [x] 8.3 Verify the full workflow end-to-end: start app → create entry → connect → disconnect → edit → delete → quit
- [x] 8.4 Run `cargo clippy -- -D warnings` and `cargo fmt` — fix all issues
