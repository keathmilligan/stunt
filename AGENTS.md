# Agent Instructions

## Project Overview

tunnel-mgr is a terminal user interface (TUI) application written in Rust that allows users to define, configure, and manage SSH tunnel connections and their connection state.

## Tech Stack

- **Language:** Rust
- **TUI Framework:** [ratatui](https://ratatui.rs/) with [crossterm](https://docs.rs/crossterm) as the backend
- **Async Runtime:** [tokio](https://tokio.rs/)
- **Serialization:** [serde](https://serde.rs/) + [toml](https://docs.rs/toml) for configuration files
- **SSH:** Tunnel connections are managed by spawning and supervising `ssh` processes

## Project Structure

```
tunnel-mgr/
├── src/
│   ├── main.rs          # Entry point, CLI arg parsing, app bootstrap
│   ├── app.rs           # Core App state and update loop
│   ├── ui/              # UI components and rendering
│   ├── tunnel/          # Tunnel definition, lifecycle, and process management
│   ├── config/          # Configuration loading, saving, and validation
│   └── event/           # Terminal event handling (key, mouse, resize)
├── Cargo.toml
├── Cargo.lock
├── README.md
├── AGENTS.md
└── openspec/            # OpenSpec workflow artifacts
```

## Conventions

### Rust

- Use `thiserror` for library/domain error types and `anyhow` for top-level error propagation.
- Prefer exhaustive `match` over wildcard patterns where practical.
- Keep UI rendering logic (`ui/`) strictly separate from business logic (`tunnel/`, `config/`).
- All public types and functions must have doc comments.
- Use `clippy` with default lints. Fix all warnings before committing.

### Architecture

- The app follows the **Elm / TEA (The Elm Architecture)** pattern: `Model -> Update -> View`.
  - `App` holds the application model (state).
  - Input events produce messages that drive state transitions.
  - The `ui/` module renders the current state; it never mutates state directly.
- Tunnel processes are managed asynchronously via tokio tasks.
- Configuration is stored as TOML files in a user-configurable directory (default: `~/.config/tunnel-mgr/`).

### Testing

- Run the full test suite with `cargo test`.
- Run lints with `cargo clippy -- -D warnings`.
- Format code with `cargo fmt`.
- Prefer integration tests for tunnel lifecycle logic; use unit tests for config parsing and state transitions.

### Git

- Write concise, imperative commit messages (e.g., "add tunnel edit form", "fix reconnect on SIGHUP").
- Do not commit generated files or build artifacts.
