//! Tunnel process supervision — spawn, adopt, monitor, reconnect, cancel.
//!
//! The supervisor supports two modes:
//! - **Spawn mode**: Spawns a detached process, captures stdout/stderr, and monitors via
//!   `child.wait()`. Output lines are streamed to the app via `TunnelEvent::Output`.
//! - **Adopt mode**: Takes an existing PID and monitors via polling without spawning.
//!   No output capture is possible for adopted processes (they were spawned in a
//!   previous session).
//!
//! When the monitored process dies, the supervisor either reconnects (with
//! exponential backoff) if `auto_restart` is true, or sends a `Disconnected`
//! event and stops.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
#[cfg(unix)]
use std::time::Duration;

use tokio::process::Command;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

#[cfg(unix)]
use super::command::spawn_detached_cmd;
#[cfg(unix)]
use super::output::LogStream;
use super::pid::TunnelProcessType;
#[cfg(unix)]
use super::pid::is_live_tunnel;
use super::state::TunnelEvent;

/// PID polling interval — how often we check if the tunnel process is alive
/// (used only for adopted processes where we don't have a child handle).
#[cfg(unix)]
const POLL_INTERVAL: Duration = Duration::from_secs(2);

/// Stability threshold — how long the process must run before we consider
/// it "connected".
#[cfg(unix)]
const STABILITY_THRESHOLD: Duration = Duration::from_secs(3);

/// Maximum reconnect attempts before marking as failed.
#[cfg(unix)]
const MAX_RETRIES: u32 = 10;

/// Maximum backoff delay in seconds.
#[cfg(unix)]
const MAX_BACKOFF_SECS: u64 = 60;

/// Base backoff delay in seconds.
#[cfg(unix)]
const BASE_BACKOFF_SECS: u64 = 1;

/// A factory function that builds a fresh `Command` for reconnection attempts.
type CommandFactory = Box<dyn Fn() -> Command + Send + 'static>;

/// Manages the lifecycle of a tunnel connection (SSH, kubectl, or sshuttle).
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
    /// Spawn a new supervision task for a tunnel entry.
    ///
    /// The task will:
    /// 1. Spawn a detached process via the provided `command_factory`
    /// 2. Send `PidUpdate` with the new PID
    /// 3. Start capturing stdout/stderr, forwarding lines as `TunnelEvent::Output`
    /// 4. Wait for the stability threshold
    /// 5. Send `Connected` if stable
    /// 6. Wait for process exit; on death, reconnect with backoff if `auto_restart`
    /// 7. Send `Failed` after max retries
    pub fn spawn(
        entry_id: uuid::Uuid,
        auto_restart: bool,
        _process_type: TunnelProcessType,
        command_factory: CommandFactory,
        tx: mpsc::UnboundedSender<TunnelEvent>,
    ) -> Self {
        let cancel = CancellationToken::new();
        let cancel_clone = cancel.clone();
        let current_pid = Arc::new(AtomicU32::new(0));
        let pid_clone = current_pid.clone();
        let suspended = Arc::new(AtomicBool::new(false));
        let suspended_clone = suspended.clone();

        let handle = tokio::spawn(async move {
            #[cfg(unix)]
            Self::run_spawn(
                entry_id,
                auto_restart,
                command_factory,
                tx,
                cancel_clone,
                pid_clone,
                suspended_clone,
            )
            .await;
            #[cfg(not(unix))]
            let _ = (
                entry_id,
                auto_restart,
                _process_type,
                command_factory,
                tx,
                cancel_clone,
                pid_clone,
                suspended_clone,
            );
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
    /// Used during startup reconciliation when the TUI discovers a live
    /// process from a previous session. No output capture is possible for
    /// adopted processes.
    #[allow(dead_code)]
    pub fn adopt(
        entry_id: uuid::Uuid,
        pid: u32,
        auto_restart: bool,
        process_type: TunnelProcessType,
        command_factory: CommandFactory,
        tx: mpsc::UnboundedSender<TunnelEvent>,
    ) -> Self {
        let cancel = CancellationToken::new();
        let cancel_clone = cancel.clone();
        let current_pid = Arc::new(AtomicU32::new(pid));
        let pid_clone = current_pid.clone();
        let suspended = Arc::new(AtomicBool::new(false));
        let suspended_clone = suspended.clone();

        let handle = tokio::spawn(async move {
            #[cfg(unix)]
            Self::run_adopt(
                entry_id,
                pid,
                auto_restart,
                process_type,
                command_factory,
                tx,
                cancel_clone,
                pid_clone,
                suspended_clone,
            )
            .await;
            #[cfg(not(unix))]
            let _ = (
                entry_id,
                pid,
                auto_restart,
                process_type,
                command_factory,
                tx,
                cancel_clone,
                pid_clone,
                suspended_clone,
            );
        });

        Supervisor {
            cancel,
            _handle: handle,
            current_pid,
            suspended,
        }
    }

    /// Cancel the supervision task without killing the tunnel process.
    ///
    /// The detached process continues running. Used on graceful TUI quit.
    pub fn cancel(&self) {
        self.cancel.cancel();
    }

    /// Cancel the supervision task AND kill the tunnel process.
    ///
    /// Used when the user explicitly disconnects a tunnel.
    pub fn cancel_and_kill(&self) {
        self.cancel.cancel();
        #[cfg(unix)]
        {
            let pid = self.current_pid.load(Ordering::Relaxed);
            if pid > 0 {
                // Safety: SIGTERM is a standard signal. We only send it to a PID we spawned.
                unsafe {
                    libc::kill(pid as libc::pid_t, libc::SIGTERM);
                }
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

    /// Spawn a child process, set up output capture, and return (pid, exit_receiver).
    ///
    /// Stdout and stderr are each read line-by-line in background tasks that
    /// forward lines as `TunnelEvent::Output`.
    #[cfg(unix)]
    fn spawn_and_capture(
        entry_id: uuid::Uuid,
        command_factory: &CommandFactory,
        tx: &mpsc::UnboundedSender<TunnelEvent>,
        current_pid: &Arc<AtomicU32>,
    ) -> Result<(u32, mpsc::UnboundedReceiver<Option<i32>>), String> {
        use tokio::io::{AsyncBufReadExt, BufReader};

        let mut child = match spawn_detached_cmd(command_factory()) {
            Ok(child) => child,
            Err(e) => return Err(format!("failed to spawn process: {e}")),
        };

        let pid = child
            .id()
            .ok_or_else(|| "spawned process has no PID".to_string())?;
        current_pid.store(pid, Ordering::Relaxed);
        let _ = tx.send(TunnelEvent::PidUpdate { entry_id, pid });

        // Take stdout and stderr handles for async reading
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        // Spawn stdout reader
        if let Some(stdout) = stdout {
            let tx = tx.clone();
            tokio::spawn(async move {
                let reader = BufReader::new(stdout);
                let mut lines = reader.lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    if tx
                        .send(TunnelEvent::Output {
                            entry_id,
                            stream: LogStream::Stdout,
                            text: line,
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            });
        }

        // Spawn stderr reader
        if let Some(stderr) = stderr {
            let tx = tx.clone();
            tokio::spawn(async move {
                let reader = BufReader::new(stderr);
                let mut lines = reader.lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    if tx
                        .send(TunnelEvent::Output {
                            entry_id,
                            stream: LogStream::Stderr,
                            text: line,
                        })
                        .is_err()
                    {
                        break;
                    }
                }
            });
        }

        // Spawn a task that waits for the child to exit and sends the result
        let (exit_tx, exit_rx) = mpsc::unbounded_channel();
        let tx_exit = tx.clone();
        tokio::spawn(async move {
            let status = child.wait().await;
            let code = match status {
                Ok(s) => s.code(),
                Err(_) => None,
            };
            let _ = tx_exit.send(TunnelEvent::ExitStatus { entry_id, code });
            let _ = exit_tx.send(code);
        });

        Ok((pid, exit_rx))
    }

    /// Main supervision loop for spawn mode.
    ///
    /// Uses child.wait() for process monitoring and captures stdout/stderr.
    #[cfg(unix)]
    #[allow(clippy::too_many_arguments)]
    async fn run_spawn(
        entry_id: uuid::Uuid,
        auto_restart: bool,
        command_factory: CommandFactory,
        tx: mpsc::UnboundedSender<TunnelEvent>,
        cancel: CancellationToken,
        current_pid: Arc<AtomicU32>,
        suspended: Arc<AtomicBool>,
    ) {
        let mut attempt: u32 = 0;

        loop {
            // Spawn the process and start capturing output
            let (_pid, mut exit_rx) =
                match Self::spawn_and_capture(entry_id, &command_factory, &tx, &current_pid) {
                    Ok(result) => result,
                    Err(reason) => {
                        let _ = tx.send(TunnelEvent::Failed { entry_id, reason });
                        return;
                    }
                };

            // Phase 1: Wait for stability threshold
            let stable = tokio::select! {
                _ = cancel.cancelled() => return,
                _ = tokio::time::sleep(STABILITY_THRESHOLD) => {
                    // Process survived the threshold — check if it's still alive
                    // by seeing if exit_rx has a value ready
                    match exit_rx.try_recv() {
                        Ok(_code) => false, // process already exited
                        Err(mpsc::error::TryRecvError::Empty) => true, // still running
                        Err(mpsc::error::TryRecvError::Disconnected) => false, // channel closed
                    }
                }
                exit_code = exit_rx.recv() => {
                    // Process died before stability threshold
                    let _ = exit_code; // ExitStatus already sent by the waiter task
                    false
                }
            };

            if cancel.is_cancelled() {
                return;
            }

            if stable {
                // Reset backoff on successful connection
                attempt = 0;
                let _ = tx.send(TunnelEvent::Connected { entry_id });

                // Phase 2: Wait for process exit or cancellation
                tokio::select! {
                    _ = cancel.cancelled() => return,
                    _exit_code = exit_rx.recv() => {
                        // Process died — ExitStatus already sent by waiter task
                    }
                }

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
                _ = cancel.cancelled() => return,
                _ = tokio::time::sleep(Duration::from_secs(delay_secs)) => {}
            }

            // Check suspension again after backoff
            if suspended.load(Ordering::Relaxed) {
                return;
            }

            // Loop back to spawn a new process at the top
        }
    }

    /// Main supervision loop for adopt mode.
    ///
    /// Uses PID polling since we don't have a child handle for adopted processes.
    #[cfg(unix)]
    #[allow(clippy::too_many_arguments)]
    async fn run_adopt(
        entry_id: uuid::Uuid,
        pid: u32,
        auto_restart: bool,
        process_type: TunnelProcessType,
        command_factory: CommandFactory,
        tx: mpsc::UnboundedSender<TunnelEvent>,
        cancel: CancellationToken,
        current_pid: Arc<AtomicU32>,
        suspended: Arc<AtomicBool>,
    ) {
        // For adopted processes we use the legacy PID polling approach since
        // we don't have a child handle. Log a system message so the user
        // knows output capture isn't available.
        let _ = tx.send(TunnelEvent::Output {
            entry_id,
            stream: LogStream::System,
            text: format!("Adopted existing process (PID {pid}) — output capture unavailable"),
        });

        // Phase 1: Wait for stability via polling
        let stable = Self::wait_for_stability(pid, process_type, &cancel).await;

        if cancel.is_cancelled() {
            return;
        }

        if stable {
            let _ = tx.send(TunnelEvent::Connected { entry_id });

            // Phase 2: Poll until dead
            Self::poll_until_dead(pid, process_type, &cancel).await;

            if cancel.is_cancelled() {
                return;
            }

            let _ = tx.send(TunnelEvent::Disconnected { entry_id });
        } else {
            let _ = tx.send(TunnelEvent::Disconnected { entry_id });
        }

        // Check if suspended
        if suspended.load(Ordering::Relaxed) {
            return;
        }

        // Only auto-restart if configured
        if !auto_restart {
            return;
        }

        // Switch to spawn mode for reconnection (we'll have a child handle now)
        Self::run_spawn(
            entry_id,
            auto_restart,
            command_factory,
            tx,
            cancel,
            current_pid,
            suspended,
        )
        .await;
    }

    /// Poll the PID until it has been alive for the stability threshold.
    ///
    /// Returns `true` if the process survived the threshold, `false` if it died.
    #[cfg(unix)]
    async fn wait_for_stability(
        pid: u32,
        process_type: TunnelProcessType,
        cancel: &CancellationToken,
    ) -> bool {
        let deadline = tokio::time::Instant::now() + STABILITY_THRESHOLD;

        loop {
            if cancel.is_cancelled() {
                return false;
            }

            if !is_live_tunnel(pid, process_type) {
                return false;
            }

            if tokio::time::Instant::now() >= deadline {
                // Final liveness check
                return is_live_tunnel(pid, process_type);
            }

            tokio::select! {
                _ = cancel.cancelled() => return false,
                _ = tokio::time::sleep(POLL_INTERVAL) => {}
            }
        }
    }

    /// Poll until the PID is no longer alive, or cancellation.
    #[cfg(unix)]
    async fn poll_until_dead(
        pid: u32,
        process_type: TunnelProcessType,
        cancel: &CancellationToken,
    ) {
        loop {
            tokio::select! {
                _ = cancel.cancelled() => return,
                _ = tokio::time::sleep(POLL_INTERVAL) => {
                    if !is_live_tunnel(pid, process_type) {
                        return;
                    }
                }
            }
        }
    }
}
