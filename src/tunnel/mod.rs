//! Tunnel definition, lifecycle, and process management.

mod command;
pub mod pid;
mod state;
mod supervisor;

#[cfg(unix)]
pub use command::spawn_detached_cmd;
pub use command::{build_kubectl_command, build_ssh_command};
pub use pid::TunnelProcessType;
#[cfg(unix)]
pub use pid::{is_live_ssh_tunnel, is_live_tunnel};
pub use state::{ConnectionState, TunnelEvent};
pub use supervisor::Supervisor;

/// Check that the `ssh` binary is available on PATH.
///
/// Returns `Ok(())` if found, or an error with a descriptive message.
pub fn check_ssh_available() -> anyhow::Result<()> {
    match std::process::Command::new("ssh")
        .arg("-V")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
    {
        Ok(_) => Ok(()),
        Err(_) => anyhow::bail!(
            "ssh binary not found on PATH. Please install OpenSSH and ensure 'ssh' is available."
        ),
    }
}

/// Check that the `kubectl` binary is available on PATH.
///
/// Returns `true` if found, `false` if not. Unlike `check_ssh_available`, this
/// returns a bool rather than a `Result` because kubectl absence is a warning,
/// not a fatal error — the app still starts and SSH tunnels remain functional.
pub fn check_kubectl_available() -> bool {
    std::process::Command::new("kubectl")
        .arg("version")
        .arg("--client")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok()
}
