//! SSH command construction and detached process spawning.

use std::process::Stdio;

use tokio::process::Command;

use crate::config::ServerEntry;

/// Build a `tokio::process::Command` from a `ServerEntry`.
///
/// The command includes:
/// - `-N` (no remote command)
/// - `-o ExitOnForwardFailure=yes`
/// - `-p <port>` (if not 22)
/// - `-l <user>` (if set)
/// - `-i <identity_file>` (if set)
/// - `-L`/`-R`/`-D` flags for each tunnel forward
pub fn build_ssh_command(entry: &ServerEntry) -> Command {
    let mut cmd = Command::new("ssh");

    cmd.arg("-N");
    cmd.arg("-o").arg("ExitOnForwardFailure=yes");

    if entry.port != 22 {
        cmd.arg("-p").arg(entry.port.to_string());
    }

    if let Some(ref user) = entry.user {
        cmd.arg("-l").arg(user);
    }

    if let Some(ref identity_file) = entry.identity_file {
        cmd.arg("-i").arg(identity_file);
    }

    for fwd in &entry.forwards {
        let flag = fwd.to_ssh_flag();
        // Split "-L addr:port:host:port" into flag and value
        let mut parts = flag.splitn(2, ' ');
        if let (Some(flag_part), Some(value_part)) = (parts.next(), parts.next()) {
            cmd.arg(flag_part).arg(value_part);
        }
    }

    cmd.arg(&entry.host);

    // Capture all stdio so ssh doesn't interfere with the TUI
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::null());
    cmd.stderr(Stdio::piped());

    cmd
}

/// Spawn the SSH command detached in a new session.
///
/// Calls `setsid()` via `pre_exec` so the SSH process survives the TUI
/// exiting. Returns the PID of the spawned process. The child handle is
/// intentionally dropped — we monitor via PID polling, not `child.wait()`.
#[cfg(unix)]
pub fn spawn_detached(entry: &ServerEntry) -> anyhow::Result<u32> {
    let mut cmd = build_ssh_command(entry);

    // Safety: setsid() is async-signal-safe and has no preconditions beyond
    // the child not already being a session leader (which a freshly forked
    // child never is).
    unsafe {
        cmd.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }

    let child = cmd
        .spawn()
        .map_err(|e| anyhow::anyhow!("failed to spawn ssh: {e}"))?;

    let pid = child
        .id()
        .ok_or_else(|| anyhow::anyhow!("spawned process has no PID"))?;

    // Intentionally drop child handle — process is detached via setsid().
    // We track it by PID from here on.
    drop(child);

    Ok(pid)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ServerEntry, TunnelForward};
    use uuid::Uuid;

    #[test]
    fn test_build_command_basic() {
        let entry = ServerEntry {
            id: Uuid::new_v4(),
            name: "test".to_string(),
            host: "example.com".to_string(),
            port: 22,
            user: None,
            identity_file: None,
            forwards: vec![],
            auto_restart: false,
        };
        let cmd = build_ssh_command(&entry);
        let prog = cmd.as_std().get_program().to_string_lossy().to_string();
        let args: Vec<String> = cmd
            .as_std()
            .get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();

        assert_eq!(prog, "ssh");
        assert!(args.contains(&"-N".to_string()));
        assert!(args.contains(&"ExitOnForwardFailure=yes".to_string()));
        assert!(args.contains(&"example.com".to_string()));
        // port 22 should not produce a -p flag
        assert!(!args.contains(&"-p".to_string()));
    }

    #[test]
    fn test_build_command_with_all_options() {
        let entry = ServerEntry {
            id: Uuid::new_v4(),
            name: "full".to_string(),
            host: "bastion.example.com".to_string(),
            port: 2222,
            user: Some("deploy".to_string()),
            identity_file: Some("~/.ssh/id_ed25519".to_string()),
            forwards: vec![
                TunnelForward::Local {
                    bind_address: "127.0.0.1".to_string(),
                    bind_port: 5432,
                    remote_host: "db.internal".to_string(),
                    remote_port: 5432,
                },
                TunnelForward::Remote {
                    bind_address: "127.0.0.1".to_string(),
                    bind_port: 8080,
                    remote_host: "localhost".to_string(),
                    remote_port: 3000,
                },
                TunnelForward::Dynamic {
                    bind_address: "127.0.0.1".to_string(),
                    bind_port: 1080,
                },
            ],
            auto_restart: false,
        };
        let cmd = build_ssh_command(&entry);
        let args: Vec<String> = cmd
            .as_std()
            .get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();

        assert!(args.contains(&"-p".to_string()));
        assert!(args.contains(&"2222".to_string()));
        assert!(args.contains(&"-l".to_string()));
        assert!(args.contains(&"deploy".to_string()));
        assert!(args.contains(&"-i".to_string()));
        assert!(args.contains(&"~/.ssh/id_ed25519".to_string()));
        assert!(args.contains(&"-L".to_string()));
        assert!(args.contains(&"-R".to_string()));
        assert!(args.contains(&"-D".to_string()));
    }

    /// Integration test: spawn a detached `sleep` process and verify the PID
    /// is alive after dropping the child handle.
    #[cfg(unix)]
    #[tokio::test]
    async fn test_spawn_detached_process_survives_drop() {
        use crate::tunnel::pid::is_pid_alive;

        // Spawn a detached sleep process (not ssh, just testing the detach pattern)
        let mut cmd = tokio::process::Command::new("sleep");
        cmd.arg("10");
        cmd.stdin(Stdio::null());
        cmd.stdout(Stdio::null());
        cmd.stderr(Stdio::null());

        unsafe {
            cmd.pre_exec(|| {
                if libc::setsid() == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }

        let child = cmd.spawn().expect("failed to spawn sleep");
        let pid = child.id().expect("no PID");

        // Drop the child handle — the process should survive because of setsid()
        drop(child);

        // Give the OS a moment to register the process
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        assert!(is_pid_alive(pid), "detached process should still be alive");

        // Clean up: kill the sleep process
        unsafe {
            libc::kill(pid as libc::pid_t, libc::SIGTERM);
        }
    }
}
