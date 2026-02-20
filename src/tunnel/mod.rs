//! Tunnel definition, lifecycle, and process management.

mod command;
pub mod pid;
mod state;
mod supervisor;

pub use pid::is_live_ssh_tunnel;
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
