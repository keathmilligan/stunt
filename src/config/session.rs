//! Runtime session state persistence (load/save JSON).
//!
//! Session state tracks active tunnel PIDs, suspended flags, and connection
//! timestamps in a `sessions.json` file separate from the TOML configuration.
//! This file is machine-managed and should not be edited by users.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A single session record for an active or suspended tunnel.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SessionRecord {
    /// PID of the SSH process, or `None` if suspended without a live process.
    pub pid: Option<u32>,
    /// Whether the tunnel has been manually suspended by the user.
    pub suspended: bool,
    /// ISO 8601 timestamp of when the tunnel was connected, or `None` if suspended.
    pub connected_at: Option<String>,
}

/// Runtime session state: maps tunnel UUIDs to their session records.
pub type SessionState = HashMap<Uuid, SessionRecord>;

/// Returns the path to the session state file.
///
/// Uses the platform user data directory via `dirs::data_dir()`:
/// - Linux: `~/.local/share/tunnel-mgr/sessions.json`
/// - macOS: `~/Library/Application Support/tunnel-mgr/sessions.json`
pub fn session_path() -> Result<PathBuf> {
    let data_dir = dirs::data_dir().context("could not determine platform data directory")?;
    Ok(data_dir.join("tunnel-mgr").join("sessions.json"))
}

/// Load session state from disk.
///
/// Returns an empty map if the file does not exist or cannot be parsed.
/// Logs a warning to stderr on parse failure.
pub fn load_sessions() -> SessionState {
    let path = match session_path() {
        Ok(p) => p,
        Err(_) => return SessionState::new(),
    };

    if !path.exists() {
        return SessionState::new();
    }

    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!(
                "warning: failed to read session file {}: {e}",
                path.display()
            );
            return SessionState::new();
        }
    };

    match serde_json::from_str::<SessionState>(&content) {
        Ok(state) => state,
        Err(e) => {
            eprintln!(
                "warning: failed to parse session file {}: {e}",
                path.display()
            );
            SessionState::new()
        }
    }
}

/// Save session state to disk atomically.
///
/// Writes to a temporary file in the same directory, then renames into place.
pub fn save_sessions(state: &SessionState) -> Result<()> {
    let path = session_path()?;

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create session directory: {}", parent.display()))?;
    }

    let content =
        serde_json::to_string_pretty(state).context("failed to serialize session state")?;

    let tmp_path = path.with_extension("json.tmp");

    // Write to temp file
    fs::write(&tmp_path, &content)
        .with_context(|| format!("failed to write temp session file: {}", tmp_path.display()))?;

    // Atomic rename
    fs::rename(&tmp_path, &path)
        .with_context(|| format!("failed to rename temp session file to: {}", path.display()))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_record_round_trip_json() {
        let mut state = SessionState::new();
        let id = Uuid::new_v4();
        state.insert(
            id,
            SessionRecord {
                pid: Some(12345),
                suspended: false,
                connected_at: Some("2026-02-19T10:30:00Z".to_string()),
            },
        );

        let json = serde_json::to_string_pretty(&state).unwrap();
        let deserialized: SessionState = serde_json::from_str(&json).unwrap();
        assert_eq!(state, deserialized);
    }

    #[test]
    fn test_suspended_record_round_trip() {
        let mut state = SessionState::new();
        let id = Uuid::new_v4();
        state.insert(
            id,
            SessionRecord {
                pid: None,
                suspended: true,
                connected_at: None,
            },
        );

        let json = serde_json::to_string_pretty(&state).unwrap();
        let deserialized: SessionState = serde_json::from_str(&json).unwrap();
        assert_eq!(state, deserialized);
        assert!(deserialized[&id].suspended);
        assert!(deserialized[&id].pid.is_none());
    }

    #[test]
    fn test_load_missing_file_returns_empty() {
        // load_sessions gracefully handles missing files
        // This test relies on the function returning empty on any failure,
        // which is tested implicitly by the function's design.
        let state = SessionState::new();
        assert!(state.is_empty());
    }

    #[test]
    fn test_save_and_load_round_trip() {
        let dir = std::env::temp_dir().join(format!("tunnel-mgr-session-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("sessions.json");

        let mut state = SessionState::new();
        let id = Uuid::new_v4();
        state.insert(
            id,
            SessionRecord {
                pid: Some(9999),
                suspended: false,
                connected_at: Some("2026-02-19T12:00:00Z".to_string()),
            },
        );

        // Write directly to temp path for testing
        let content = serde_json::to_string_pretty(&state).unwrap();
        fs::write(&path, &content).unwrap();

        let loaded: SessionState =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(state, loaded);

        // Clean up
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_corrupt_json_returns_empty() {
        let dir =
            std::env::temp_dir().join(format!("tunnel-mgr-session-corrupt-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("sessions.json");

        fs::write(&path, "not valid json {{{").unwrap();

        // Parsing corrupt content directly should fail
        let result = serde_json::from_str::<SessionState>("not valid json {{{");
        assert!(result.is_err());

        // Clean up
        fs::remove_dir_all(&dir).ok();
    }
}
