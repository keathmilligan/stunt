//! Tunnel command construction and detached process spawning.

use std::process::Stdio;

use tokio::process::Command;

use crate::config::{K8sEntry, K8sPortForward, ServerEntry, SshuttleEntry};

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

/// Build a `tokio::process::Command` for a single `kubectl port-forward` binding.
///
/// The command form is:
/// ```text
/// kubectl port-forward [--context <ctx>] [-n <namespace>] <type>/<name> <bind>:<local>:<remote>
/// ```
pub fn build_kubectl_command(entry: &K8sEntry, forward: &K8sPortForward) -> Command {
    let mut cmd = Command::new("kubectl");

    cmd.arg("port-forward");

    if let Some(ref ctx) = entry.context {
        cmd.arg("--context").arg(ctx);
    }

    if let Some(ref ns) = entry.namespace {
        cmd.arg("-n").arg(ns);
    }

    cmd.arg(entry.resource_identifier());
    cmd.arg(forward.kubectl_arg());

    // Capture all stdio so kubectl doesn't interfere with the TUI
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::null());
    cmd.stderr(Stdio::piped());

    cmd
}

/// Build a `tokio::process::Command` for an `SshuttleEntry`.
///
/// The command form is:
/// ```text
/// sshuttle -r [user@]host[:port] [-e 'ssh -i <identity_file>'] <subnet>...
/// ```
pub fn build_sshuttle_command(entry: &SshuttleEntry) -> Command {
    let mut cmd = Command::new("sshuttle");

    // Assemble the remote argument: [user@]host[:port]
    let remote = {
        let host_part = match &entry.user {
            Some(user) => format!("{user}@{}", entry.host),
            None => entry.host.clone(),
        };
        match entry.port {
            Some(port) => format!("{host_part}:{port}"),
            None => host_part,
        }
    };
    cmd.arg("-r").arg(remote);

    // Optional identity file passed via -e 'ssh -i <path>'
    if let Some(ref identity_file) = entry.identity_file {
        cmd.arg("-e").arg(format!("ssh -i {identity_file}"));
    }

    // Positional subnet arguments
    for subnet in &entry.subnets {
        cmd.arg(subnet);
    }

    // Capture all stdio so sshuttle doesn't interfere with the TUI
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::null());
    cmd.stderr(Stdio::piped());

    cmd
}

/// Spawn the given command detached in a new session.
///
/// Calls `setsid()` via `pre_exec` so the process survives the TUI
/// exiting. Returns the PID of the spawned process. The child handle is
/// intentionally dropped — we monitor via PID polling, not `child.wait()`.
#[cfg(unix)]
pub fn spawn_detached_cmd(mut cmd: Command) -> anyhow::Result<u32> {
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
        .map_err(|e| anyhow::anyhow!("failed to spawn process: {e}"))?;

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
    use crate::config::{
        K8sEntry, K8sPortForward, K8sResourceType, ServerEntry, SshuttleEntry, TunnelForward,
    };
    use uuid::Uuid;

    #[test]
    fn test_build_ssh_command_basic() {
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
    fn test_build_ssh_command_with_all_options() {
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

    fn make_k8s_entry(
        context: Option<&str>,
        namespace: Option<&str>,
        resource_type: K8sResourceType,
        resource_name: &str,
    ) -> K8sEntry {
        K8sEntry {
            id: Uuid::new_v4(),
            name: "test".to_string(),
            context: context.map(|s| s.to_string()),
            namespace: namespace.map(|s| s.to_string()),
            resource_type,
            resource_name: resource_name.to_string(),
            forwards: vec![],
            auto_restart: false,
        }
    }

    fn make_k8s_fwd(local_port: u16, remote_port: u16) -> K8sPortForward {
        K8sPortForward {
            local_bind_address: "127.0.0.1".to_string(),
            local_port,
            remote_port,
        }
    }

    #[test]
    fn test_build_kubectl_command_all_options() {
        let entry = make_k8s_entry(
            Some("prod"),
            Some("default"),
            K8sResourceType::Deployment,
            "api",
        );
        let fwd = make_k8s_fwd(8080, 80);
        let cmd = build_kubectl_command(&entry, &fwd);

        let prog = cmd.as_std().get_program().to_string_lossy().to_string();
        let args: Vec<String> = cmd
            .as_std()
            .get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();

        assert_eq!(prog, "kubectl");
        assert!(args.contains(&"port-forward".to_string()));
        assert!(args.contains(&"--context".to_string()));
        assert!(args.contains(&"prod".to_string()));
        assert!(args.contains(&"-n".to_string()));
        assert!(args.contains(&"default".to_string()));
        assert!(args.contains(&"deployment/api".to_string()));
        assert!(args.contains(&"127.0.0.1:8080:80".to_string()));
    }

    #[test]
    fn test_build_kubectl_command_no_optional_fields() {
        let entry = make_k8s_entry(None, None, K8sResourceType::Service, "postgres");
        let fwd = make_k8s_fwd(5432, 5432);
        let cmd = build_kubectl_command(&entry, &fwd);

        let args: Vec<String> = cmd
            .as_std()
            .get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();

        assert!(!args.contains(&"--context".to_string()));
        assert!(!args.contains(&"-n".to_string()));
        assert!(args.contains(&"service/postgres".to_string()));
        assert!(args.contains(&"127.0.0.1:5432:5432".to_string()));
    }

    #[test]
    fn test_build_kubectl_command_pod_resource() {
        let entry = make_k8s_entry(
            None,
            Some("kube-system"),
            K8sResourceType::Pod,
            "my-pod-abc",
        );
        let fwd = make_k8s_fwd(9090, 9090);
        let cmd = build_kubectl_command(&entry, &fwd);

        let args: Vec<String> = cmd
            .as_std()
            .get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();

        assert!(args.contains(&"pod/my-pod-abc".to_string()));
        assert!(args.contains(&"-n".to_string()));
        assert!(args.contains(&"kube-system".to_string()));
    }

    fn make_sshuttle_entry(
        host: &str,
        port: Option<u16>,
        user: Option<&str>,
        identity_file: Option<&str>,
        subnets: &[&str],
    ) -> SshuttleEntry {
        SshuttleEntry {
            id: Uuid::new_v4(),
            name: "test-vpn".to_string(),
            host: host.to_string(),
            port,
            user: user.map(|s| s.to_string()),
            identity_file: identity_file.map(|s| s.to_string()),
            subnets: subnets.iter().map(|s| s.to_string()).collect(),
            auto_restart: false,
        }
    }

    #[test]
    fn test_build_sshuttle_command_minimal() {
        let entry = make_sshuttle_entry("bastion.example.com", None, None, None, &["10.0.0.0/8"]);
        let cmd = build_sshuttle_command(&entry);

        let prog = cmd.as_std().get_program().to_string_lossy().to_string();
        let args: Vec<String> = cmd
            .as_std()
            .get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();

        assert_eq!(prog, "sshuttle");
        assert!(args.contains(&"-r".to_string()));
        assert!(args.contains(&"bastion.example.com".to_string()));
        assert!(args.contains(&"10.0.0.0/8".to_string()));
        assert!(!args.contains(&"-e".to_string()));
    }

    #[test]
    fn test_build_sshuttle_command_full() {
        let entry = make_sshuttle_entry(
            "bastion.example.com",
            Some(2222),
            Some("alice"),
            Some("~/.ssh/id_ed25519"),
            &["10.0.0.0/8", "192.168.0.0/16"],
        );
        let cmd = build_sshuttle_command(&entry);

        let args: Vec<String> = cmd
            .as_std()
            .get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();

        // Remote arg should be user@host:port
        let r_idx = args.iter().position(|a| a == "-r").expect("-r not found");
        assert_eq!(args[r_idx + 1], "alice@bastion.example.com:2222");

        // Identity file via -e
        assert!(args.contains(&"-e".to_string()));
        let e_idx = args.iter().position(|a| a == "-e").unwrap();
        assert_eq!(args[e_idx + 1], "ssh -i ~/.ssh/id_ed25519");

        // Both subnets present
        assert!(args.contains(&"10.0.0.0/8".to_string()));
        assert!(args.contains(&"192.168.0.0/16".to_string()));
    }

    #[test]
    fn test_build_sshuttle_command_no_user_no_port() {
        let entry = make_sshuttle_entry("vpn.example.com", None, None, None, &["172.16.0.0/12"]);
        let cmd = build_sshuttle_command(&entry);

        let args: Vec<String> = cmd
            .as_std()
            .get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();

        let r_idx = args.iter().position(|a| a == "-r").expect("-r not found");
        // Should be bare hostname with no @ or :
        assert_eq!(args[r_idx + 1], "vpn.example.com");
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

        let pid = spawn_detached_cmd(cmd).expect("failed to spawn sleep");

        // Give the OS a moment to register the process
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        assert!(is_pid_alive(pid), "detached process should still be alive");

        // Clean up: kill the sleep process
        unsafe {
            libc::kill(pid as libc::pid_t, libc::SIGTERM);
        }
    }
}
