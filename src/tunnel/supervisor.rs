//! Tunnel process supervision — spawn, adopt, monitor, reconnect, cancel.
//!
//! The supervisor supports two modes:
//! - **Spawn mode**: Spawns a detached SSH process, records the PID, then monitors via polling.
//! - **Adopt mode**: Takes an existing PID and monitors via polling without spawning.
//!
//! Both modes share the same PID-polling loop. When the monitored process dies,
//! the supervisor either reconnects (with exponential backoff) if `auto_restart`
//! is true, or sends a `Disconnected` event and stops.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Duration;

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::command::spawn_detached;
use super::pid::is_live_ssh_tunnel;
use super::state::TunnelEvent;
use crate::config::ServerEntry;

/// PID polling interval — how often we check if the SSH process is alive.
const POLL_INTERVAL: Duration = Duration::from_secs(2);

/// Stability threshold — how long the ssh process must run before we consider
/// it "connected".
const STABILITY_THRESHOLD: Duration = Duration::from_secs(3);

/// Maximum reconnect attempts before marking as failed.
const MAX_RETRIES: u32 = 10;

/// Maximum backoff delay in seconds.
const MAX_BACKOFF_SECS: u64 = 60;

/// Base backoff delay in seconds.
const BASE_BACKOFF_SECS: u64 = 1;

/// Manages the lifecycle of an SSH tunnel connection for a single server entry.
pub struct Supervisor {
    /// Cancellation token to signal the supervision task to stop.
    cancel: CancellationToken,
    /// Handle to the spawned tokio task.
    _handle: tokio::task::JoinHandle<()>,
    /// The current PID being monitored (0 if not yet spawned).
    current_pid: Arc<AtomicU32>,
    /// Whether the tunnel has been suspended (user manually disconnected an auto-restart tunnel).
    suspended: Arc<AtomicBool>,
}

impl Supervisor {
    /// Spawn a new supervision task for the given server entry.
    ///
    /// The task will:
    /// 1. Spawn a detached ssh process via `setsid()`
    /// 2. Send `PidUpdate` with the new PID
    /// 3. Wait for the stability threshold via PID polling
    /// 4. Send `Connected` if stable
    /// 5. Continue polling; on death, reconnect with backoff if `auto_restart`
    /// 6. Send `Failed` after max retries
    pub fn spawn(entry: ServerEntry, tx: mpsc::UnboundedSender<TunnelEvent>) -> Self {
        let cancel = CancellationToken::new();
        let cancel_clone = cancel.clone();
        let current_pid = Arc::new(AtomicU32::new(0));
        let pid_clone = current_pid.clone();
        let suspended = Arc::new(AtomicBool::new(false));
        let suspended_clone = suspended.clone();

        let handle = tokio::spawn(async move {
            Self::run_spawn(entry, tx, cancel_clone, pid_clone, suspended_clone).await;
        });

        Supervisor {
            cancel,
            _handle: handle,
            current_pid,
            suspended,
        }
    }

    /// Adopt an existing PID and start monitoring via PID polling.
    ///
    /// Used during startup reconciliation when the TUI discovers a live SSH
    /// process from a previous session.
    pub fn adopt(entry: ServerEntry, pid: u32, tx: mpsc::UnboundedSender<TunnelEvent>) -> Self {
        let cancel = CancellationToken::new();
        let cancel_clone = cancel.clone();
        let current_pid = Arc::new(AtomicU32::new(pid));
        let pid_clone = current_pid.clone();
        let suspended = Arc::new(AtomicBool::new(false));
        let suspended_clone = suspended.clone();

        let handle = tokio::spawn(async move {
            Self::run_adopt(entry, pid, tx, cancel_clone, pid_clone, suspended_clone).await;
        });

        Supervisor {
            cancel,
            _handle: handle,
            current_pid,
            suspended,
        }
    }

    /// Cancel the supervision task without killing the SSH process.
    ///
    /// The detached SSH process continues running. Used on graceful TUI quit.
    pub fn cancel(&self) {
        self.cancel.cancel();
    }

    /// Cancel the supervision task AND kill the SSH process.
    ///
    /// Used when the user explicitly disconnects a tunnel.
    pub fn cancel_and_kill(&self) {
        self.cancel.cancel();
        let pid = self.current_pid.load(Ordering::Relaxed);
        if pid > 0 {
            // Safety: SIGTERM is a standard signal. We only send it to a PID we spawned.
            unsafe {
                libc::kill(pid as libc::pid_t, libc::SIGTERM);
            }
        }
    }

    /// Get the PID currently being monitored, or 0 if none.
    #[allow(dead_code)]
    pub fn pid(&self) -> u32 {
        self.current_pid.load(Ordering::Relaxed)
    }

    /// Mark this supervisor as suspended (user manually disconnected).
    ///
    /// A suspended supervisor will not attempt reconnection.
    pub fn set_suspended(&self, val: bool) {
        self.suspended.store(val, Ordering::Relaxed);
    }

    /// Main supervision loop for spawn mode.
    async fn run_spawn(
        entry: ServerEntry,
        tx: mpsc::UnboundedSender<TunnelEvent>,
        cancel: CancellationToken,
        current_pid: Arc<AtomicU32>,
        suspended: Arc<AtomicBool>,
    ) {
        let entry_id = entry.id;

        // Initial spawn
        let pid = match spawn_detached(&entry) {
            Ok(pid) => pid,
            Err(e) => {
                let _ = tx.send(TunnelEvent::Failed {
                    entry_id,
                    reason: format!("failed to spawn ssh: {e}"),
                });
                return;
            }
        };

        current_pid.store(pid, Ordering::Relaxed);
        let _ = tx.send(TunnelEvent::PidUpdate { entry_id, pid });

        // Enter the shared monitoring loop
        Self::monitor_loop(entry, pid, tx, cancel, current_pid, suspended).await;
    }

    /// Main supervision loop for adopt mode.
    async fn run_adopt(
        entry: ServerEntry,
        pid: u32,
        tx: mpsc::UnboundedSender<TunnelEvent>,
        cancel: CancellationToken,
        current_pid: Arc<AtomicU32>,
        suspended: Arc<AtomicBool>,
    ) {
        // Process is already running — go straight to monitoring
        Self::monitor_loop(entry, pid, tx, cancel, current_pid, suspended).await;
    }

    /// Shared PID-polling monitor loop.
    ///
    /// Waits for the stability threshold, sends `Connected`, then polls until
    /// the process dies. On death, handles reconnection logic.
    async fn monitor_loop(
        entry: ServerEntry,
        initial_pid: u32,
        tx: mpsc::UnboundedSender<TunnelEvent>,
        cancel: CancellationToken,
        current_pid: Arc<AtomicU32>,
        suspended: Arc<AtomicBool>,
    ) {
        let entry_id = entry.id;
        let auto_restart = entry.auto_restart;
        let mut pid = initial_pid;
        let mut attempt: u32 = 0;

        loop {
            // Phase 1: Wait for stability threshold via polling
            let stable = Self::wait_for_stability(pid, &cancel).await;

            if cancel.is_cancelled() {
                return;
            }

            if stable {
                // Reset backoff on successful connection
                attempt = 0;
                let _ = tx.send(TunnelEvent::Connected { entry_id });

                // Phase 2: Poll until process dies or cancellation
                Self::poll_until_dead(pid, &cancel).await;

                if cancel.is_cancelled() {
                    return;
                }

                // Process died
                let _ = tx.send(TunnelEvent::Disconnected { entry_id });
            } else {
                // Process died before stability threshold
                let _ = tx.send(TunnelEvent::Disconnected { entry_id });
            }

            // Check if suspended — if so, stop reconnect loop
            if suspended.load(Ordering::Relaxed) {
                return;
            }

            // Only auto-restart if configured
            if !auto_restart {
                return;
            }

            // Reconnect logic with exponential backoff
            attempt += 1;
            if attempt > MAX_RETRIES {
                let _ = tx.send(TunnelEvent::Failed {
                    entry_id,
                    reason: format!("max retries ({MAX_RETRIES}) exhausted"),
                });
                return;
            }

            let delay_secs =
                (BASE_BACKOFF_SECS * 2u64.saturating_pow(attempt - 1)).min(MAX_BACKOFF_SECS);
            let _ = tx.send(TunnelEvent::Reconnecting {
                entry_id,
                attempt,
                delay_secs,
            });

            // Wait for backoff or cancellation
            tokio::select! {
                _ = cancel.cancelled() => {
                    return;
                }
                _ = tokio::time::sleep(Duration::from_secs(delay_secs)) => {}
            }

            // Check suspension again after backoff
            if suspended.load(Ordering::Relaxed) {
                return;
            }

            // Respawn
            pid = match spawn_detached(&entry) {
                Ok(new_pid) => new_pid,
                Err(e) => {
                    let _ = tx.send(TunnelEvent::Failed {
                        entry_id,
                        reason: format!("failed to respawn ssh: {e}"),
                    });
                    return;
                }
            };

            current_pid.store(pid, Ordering::Relaxed);
            let _ = tx.send(TunnelEvent::PidUpdate { entry_id, pid });
        }
    }

    /// Poll the PID until it has been alive for the stability threshold.
    ///
    /// Returns `true` if the process survived the threshold, `false` if it died.
    async fn wait_for_stability(pid: u32, cancel: &CancellationToken) -> bool {
        let deadline = tokio::time::Instant::now() + STABILITY_THRESHOLD;

        loop {
            if cancel.is_cancelled() {
                return false;
            }

            if !is_live_ssh_tunnel(pid) {
                return false;
            }

            if tokio::time::Instant::now() >= deadline {
                // Final liveness check
                return is_live_ssh_tunnel(pid);
            }

            tokio::select! {
                _ = cancel.cancelled() => return false,
                _ = tokio::time::sleep(POLL_INTERVAL) => {}
            }
        }
    }

    /// Poll until the PID is no longer alive, or cancellation.
    async fn poll_until_dead(pid: u32, cancel: &CancellationToken) {
        loop {
            tokio::select! {
                _ = cancel.cancelled() => return,
                _ = tokio::time::sleep(POLL_INTERVAL) => {
                    if !is_live_ssh_tunnel(pid) {
                        return;
                    }
                }
            }
        }
    }
}
