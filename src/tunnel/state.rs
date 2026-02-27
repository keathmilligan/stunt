//! Connection state machine and tunnel event types.

use uuid::Uuid;

/// Connection state for a server entry.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ConnectionState {
    /// Not connected, idle.
    #[default]
    Disconnected,
    /// SSH process has been spawned, waiting for stability threshold.
    Connecting,
    /// SSH process running and stable.
    Connected,
    /// Connection lost, waiting for backoff before retry.
    Reconnecting,
    /// Connection failed after max retries or immediate failure.
    Failed,
    /// Auto-restart tunnel manually disconnected by user; suppresses auto-reconnect.
    Suspended,
}

impl ConnectionState {
    /// Returns a short display label for the state.
    pub fn label(&self) -> &'static str {
        match self {
            ConnectionState::Disconnected => "disconnected",
            ConnectionState::Connecting => "connecting",
            ConnectionState::Connected => "connected",
            ConnectionState::Reconnecting => "reconnecting",
            ConnectionState::Failed => "failed",
            ConnectionState::Suspended => "suspended",
        }
    }

    /// Returns true if this state represents an active or pending connection.
    pub fn is_active(&self) -> bool {
        matches!(
            self,
            ConnectionState::Connecting
                | ConnectionState::Connected
                | ConnectionState::Reconnecting
        )
    }
}

/// Events sent from tunnel supervision tasks to the app.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum TunnelEvent {
    /// SSH process has been running past the stability threshold.
    Connected { entry_id: Uuid },
    /// SSH process exited unexpectedly.
    Disconnected { entry_id: Uuid },
    /// Connection attempt failed or max retries exhausted.
    Failed { entry_id: Uuid, reason: String },
    /// Entering reconnect backoff.
    Reconnecting {
        entry_id: Uuid,
        attempt: u32,
        delay_secs: u64,
    },
    /// The supervisor spawned or respawned a process; carries the new PID.
    PidUpdate { entry_id: Uuid, pid: u32 },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connection_state_labels() {
        assert_eq!(ConnectionState::Disconnected.label(), "disconnected");
        assert_eq!(ConnectionState::Connecting.label(), "connecting");
        assert_eq!(ConnectionState::Connected.label(), "connected");
        assert_eq!(ConnectionState::Reconnecting.label(), "reconnecting");
        assert_eq!(ConnectionState::Failed.label(), "failed");
        assert_eq!(ConnectionState::Suspended.label(), "suspended");
    }

    #[test]
    fn test_is_active() {
        assert!(!ConnectionState::Disconnected.is_active());
        assert!(ConnectionState::Connecting.is_active());
        assert!(ConnectionState::Connected.is_active());
        assert!(ConnectionState::Reconnecting.is_active());
        assert!(!ConnectionState::Failed.is_active());
        assert!(!ConnectionState::Suspended.is_active());
    }

    #[test]
    fn test_default_state() {
        assert_eq!(ConnectionState::default(), ConnectionState::Disconnected);
    }
}
