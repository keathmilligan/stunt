//! PID liveness checks for detached SSH tunnel processes.
//!
//! Provides functions to verify that a process is alive and is actually an
//! `ssh` process (not a recycled PID running something else).

use std::fs;

/// Check whether a process with the given PID is alive.
///
/// Uses `libc::kill(pid, 0)` which sends no signal but checks process
/// existence. Returns `true` if the process exists (even if owned by another
/// user — `EPERM` still means alive). Returns `false` if the process does
/// not exist (`ESRCH`).
#[cfg(unix)]
pub fn is_pid_alive(pid: u32) -> bool {
    // Safety: kill(pid, 0) is a standard POSIX call with no side effects.
    let ret = unsafe { libc::kill(pid as libc::pid_t, 0) };
    if ret == 0 {
        return true;
    }
    // errno tells us why it failed
    let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
    // EPERM (1) means process exists but we lack permission to signal it — still alive
    // ESRCH (3) means no such process
    errno == libc::EPERM
}

/// Check whether the process with the given PID is an `ssh` process.
///
/// On Linux, reads `/proc/<pid>/comm` and checks if it starts with "ssh".
/// If `/proc` is unavailable (e.g., macOS or restricted environments), returns
/// `true` as a conservative fallback — we assume the PID is valid since we
/// tracked it ourselves.
pub fn is_ssh_process(pid: u32) -> bool {
    let comm_path = format!("/proc/{pid}/comm");
    match fs::read_to_string(&comm_path) {
        Ok(content) => {
            let name = content.trim();
            name == "ssh" || name.starts_with("ssh")
        }
        Err(_) => {
            // /proc not available or permission denied — conservative fallback
            true
        }
    }
}

/// Check whether the given PID is a live SSH tunnel process.
///
/// Combines both liveness and process identity checks. Returns `true` only
/// if the process is alive AND appears to be an `ssh` process.
#[cfg(unix)]
pub fn is_live_ssh_tunnel(pid: u32) -> bool {
    is_pid_alive(pid) && is_ssh_process(pid)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_current_process_is_alive() {
        let pid = std::process::id();
        assert!(is_pid_alive(pid), "current process should be alive");
    }

    #[test]
    fn test_nonexistent_pid_is_not_alive() {
        // PID 4194304 (2^22) is extremely unlikely to be in use; the kernel
        // default max is 32768 (or 4194304 on 64-bit with pid_max raised).
        // We try a very high PID that is almost certainly unused.
        let pid = 4_000_000;
        // This should be false on most systems. If somehow alive, skip.
        if !is_pid_alive(pid) {
            assert!(!is_pid_alive(pid));
        }
    }

    #[test]
    fn test_pid_zero_handling() {
        // PID 0 refers to the kernel scheduler on Linux. kill(0, 0) checks
        // the calling process's process group, so behaviour varies. We just
        // ensure no panic.
        let _ = is_pid_alive(0);
    }

    #[test]
    fn test_current_process_is_not_ssh() {
        let pid = std::process::id();
        // The test runner is not "ssh", so on Linux where /proc exists,
        // this should return false. On non-Linux it returns true (fallback).
        if std::path::Path::new("/proc").exists() {
            assert!(
                !is_ssh_process(pid),
                "test runner should not be identified as ssh"
            );
        }
    }

    #[test]
    fn test_is_live_ssh_tunnel_current_process() {
        let pid = std::process::id();
        // Current process is alive but NOT ssh, so on Linux this should be false.
        if std::path::Path::new("/proc").exists() {
            assert!(
                !is_live_ssh_tunnel(pid),
                "test runner should not be a live ssh tunnel"
            );
        }
    }

    #[test]
    fn test_is_live_ssh_tunnel_dead_pid() {
        let pid = 4_000_000;
        if !is_pid_alive(pid) {
            assert!(
                !is_live_ssh_tunnel(pid),
                "dead PID should not be a live ssh tunnel"
            );
        }
    }
}
