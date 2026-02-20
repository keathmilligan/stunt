## Context

This is a greenfield Rust TUI application. There is no existing code — only an AGENTS.md establishing the tech stack (Rust, ratatui/crossterm, tokio, serde/toml) and a README outlining features and key bindings. The app manages SSH tunnels by wrapping the system `ssh` binary, so it has no SSH library dependency but does require a working `ssh` on `$PATH`.

The user's primary interaction model is a single-screen scrollable list of SSH server entries, where each entry is a multi-line row (~4-5 lines) showing connection details and tunnel forwards. Navigation, CRUD, and connect/disconnect are all keyboard-driven.

## Goals / Non-Goals

**Goals:**

- A single-binary TUI that starts instantly and renders a responsive list of tunnel entries
- Persistent TOML storage in the platform user data directory (`dirs` crate)
- Spawn and supervise one `ssh` process per server entry, with automatic reconnect on failure
- Support local (`-L`), remote (`-R`), and dynamic/SOCKS (`-D`) port forwards
- Arrow-key scrolling that moves by whole rows (variable height), not by single lines
- A title/menu bar at the top showing the app name and available key bindings, and a status bar at the bottom showing connection summary and contextual info
- Consistent use of color to convey state and improve readability (e.g., green for connected, red for failed, yellow for connecting)

**Non-Goals:**

- Built-in SSH library (libssh2, russh, etc.) — we delegate to the system `ssh`
- Multi-pane or tabbed UI — the initial version is a single list view
- SSH key generation or agent management
- Remote configuration sync or cloud storage
- Windows support in the initial release (crossterm supports it, but `ssh` availability varies)

## Decisions

### 1. Elm Architecture (TEA) for app loop

**Choice:** Model → Update → View loop driven by an enum of `Message` variants.

**Rationale:** ratatui is an immediate-mode rendering library with no built-in state management. TEA gives a predictable, testable state machine. The `App` struct holds all state, input events map to `Message` values, `update()` applies them, and `ui()` renders the current state without mutation.

**Alternatives considered:**
- Component-based (tui-realm): Adds abstraction overhead and a learning curve for a single-view app.
- Ad-hoc mutable state: Harder to reason about and test.

### 2. One `ssh` child process per server entry

**Choice:** Each server entry spawns a single `ssh` process with all of its tunnel forwards passed as `-L`/`-R`/`-D` flags.

**Rationale:** SSH multiplexes multiple forwards over one connection. This minimizes process count, avoids duplicate authentication prompts, and makes connect/disconnect a per-server operation. If one forward in a set fails, the entire connection is torn down and retried.

**Alternatives considered:**
- One process per forward: Simpler error isolation but more processes, more auth prompts, and harder to reason about "server connected" state.
- ControlMaster with separate forward commands: More complex setup and harder to supervise portably.

### 3. Tokio for async process supervision

**Choice:** Use `tokio::process::Command` to spawn SSH, with a per-server tokio task that monitors the child and handles reconnect.

**Rationale:** ratatui itself is synchronous, but tunnel lifecycle (spawn, wait, reconnect delay, kill) is inherently async. Tokio is the standard async runtime, and `tokio::process` gives us non-blocking wait on child exit. The main loop uses `tokio::select!` to multiplex terminal events and tunnel status updates.

**Alternatives considered:**
- Synchronous threads with `std::process`: Viable but more manual synchronization and no `select!`.
- async-std: Less ecosystem support for process management.

### 4. Platform user data directory via `dirs`

**Choice:** Store configuration at `dirs::data_dir()/tunnel-mgr/tunnels.toml`.

**Rationale:** `dirs` maps to the correct platform convention (`~/.local/share/` on Linux, `~/Library/Application Support/` on macOS, `%APPDATA%` on Windows). A single `tunnels.toml` file is sufficient at this scale — no need for a database or per-tunnel files.

**Alternatives considered:**
- `dirs::config_dir()`: The data is user-created tunnel definitions, which semantically fits "data" more than "config". Either is defensible, but `data_dir` avoids clobbering XDG config conventions.
- Per-tunnel files: More complex file management for no clear benefit at this scale.

### 5. Variable-height row scrolling

**Choice:** The list widget tracks a `selected` index and a `scroll_offset` measured in rows (not lines). On render, row heights are computed from content (number of tunnel forwards), and the viewport is adjusted to keep the selected row fully visible.

**Rationale:** Rows are 4-5 lines each (1 line server header + 1 line per forward + border/spacing). Standard list widgets scroll by fixed-height items. We need a custom scroll calculation that accumulates row heights to determine which rows fit in the viewport and where to scroll.

**Alternatives considered:**
- Fixed-height rows with truncated forwards: Loses information; the whole point is seeing all forwards at a glance.
- Virtual scrolling by line offset: More complex to map back to row selection and highlighting.

### 6. Screen layout: title bar, list viewport, status bar

**Choice:** The terminal screen is divided into three vertical regions using ratatui `Layout` constraints:

1. **Title bar** (1 line, fixed) — App name on the left, key binding hints on the right (e.g., `[n]ew  [e]dit  [d]elete  [Enter] connect  [q]uit`). Rendered with a bold/highlighted style on a colored background to visually anchor the top of the screen.
2. **List viewport** (remaining space, `Min(0)`) — The scrollable server entry list.
3. **Status bar** (1 line, fixed) — Contextual info: total entries, connected/disconnected count, and any transient messages (e.g., "Saved", "Connection failed: timeout"). Rendered with a distinct background color.

**Rationale:** A title bar with key hints reduces the learning curve — users can see available actions without consulting docs. The status bar provides at-a-glance aggregate state and feedback. Both are cheap (1 line each) and keep the viewport maximized for tunnel entries.

**Alternatives considered:**
- No chrome (full-screen list only): Functional but forces users to memorize key bindings and provides no connection summary.
- Popup help overlay: Discoverable only if you know the help key; always-visible hints are better for a small key set.

### 7. Color scheme for state and readability

**Choice:** Use a semantic color palette applied via ratatui `Style`:

| Element | Color | Purpose |
|---|---|---|
| Server name / header | White, bold | Primary text, high contrast |
| Connected status | Green | Immediately signals healthy state |
| Disconnected status | Dim / gray | Recedes visually, low priority |
| Connecting / reconnecting | Yellow | Transient state, draws attention |
| Failed / error | Red | Urgent, needs user action |
| Forward type labels (`L`, `R`, `D`) | Cyan | Distinguishes forward metadata from addresses |
| Selected row background | Highlighted (reverse or blue bg) | Clear selection indicator |
| Title bar | Bold on accent background | Visual anchor |
| Status bar | Dim foreground on dark background | Present but not distracting |

**Rationale:** Color-coding connection state lets users scan the list and instantly identify which servers need attention. The palette follows common terminal conventions (green = good, red = bad, yellow = warning) and works on both dark and light terminal themes by using named colors rather than fixed RGB values.

**Alternatives considered:**
- Monochrome with text-only status indicators: Functional but slower to scan visually.
- User-configurable themes: Nice-to-have for a future iteration, not worth the complexity now.

### 8. Data model: `ServerEntry` containing `Vec<TunnelForward>`

**Choice:** Top-level config is a list of `ServerEntry` structs. Each entry has connection fields (host, port, user, identity_file) and a vector of `TunnelForward` values. `TunnelForward` is an enum with `Local`, `Remote`, and `Dynamic` variants, each carrying the relevant addressing fields.

**Rationale:** This mirrors the user's mental model (the README and user description both frame it as "servers with their tunnels") and maps directly to how `ssh` is invoked — one command per server, with multiple `-L`/`-R`/`-D` flags.

## Risks / Trade-offs

- **`ssh` not on PATH** → The app checks for `ssh` at startup and shows a clear error if missing. No graceful degradation — the app is useless without it.
- **ssh prompts (password, host key confirmation) hijack the terminal** → Document that tunnels should use key-based auth with known hosts. In a future iteration, consider passing `-o BatchMode=yes` and surfacing errors in the TUI rather than letting ssh steal the tty.
- **Reconnect storms** → If a server is persistently unreachable, the supervisor task could spin. Mitigate with exponential backoff (capped at 60s) and a max-retry limit before marking the entry as failed.
- **Large number of entries** → The list is rendered fully on each frame. For hundreds of entries this is fine; for thousands it would need virtualization. Not a realistic concern for SSH tunnel definitions.
- **TOML file corruption** → Write via atomic rename (write to tempfile, then rename). Keep a `.bak` copy of the previous version.
