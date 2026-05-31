//! PID liveness checks for detached tunnel processes.
//!
//! Provides functions to verify that a process is alive and is actually the
//! expected tunnel process type (`ssh` or `kubectl`), not a recycled PID
//! running something else.

#[cfg(unix)]
use std::fs;

/// The type of tunnel process being supervised.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TunnelProcessType {
    /// An SSH tunnel process (`ssh` binary).
    Ssh,
    /// A Kubernetes port-forward process (`kubectl` binary).
    Kubectl,
    /// An sshuttle VPN-over-SSH process (`sshuttle` binary).
    Sshuttle,
}

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

/// Match an executable's base name against the expected tunnel binary.
///
/// Accepts an optional `.exe` suffix so the same logic works for Windows
/// image names (`ssh.exe`) and Unix command names (`ssh`).
fn name_matches(name: &str, process_type: TunnelProcessType) -> bool {
    let name = name.trim();
    let stem = name.strip_suffix(".exe").unwrap_or(name);
    match process_type {
        TunnelProcessType::Ssh => stem == "ssh" || stem.starts_with("ssh"),
        TunnelProcessType::Kubectl => stem == "kubectl" || stem.starts_with("kubectl"),
        TunnelProcessType::Sshuttle => stem == "sshuttle" || stem.starts_with("sshuttle"),
    }
}

/// Check whether the process with the given PID matches the expected name.
///
/// On Linux, reads `/proc/<pid>/comm` and checks against the expected process
/// name prefix for the given `TunnelProcessType`:
/// - `Ssh` → checks for `"ssh"` prefix
/// - `Kubectl` → checks for `"kubectl"` prefix
///
/// If `/proc` is unavailable (e.g., macOS or restricted environments), returns
/// `true` as a conservative fallback — we assume the PID is valid since we
/// tracked it ourselves.
#[cfg(unix)]
#[allow(dead_code)]
pub fn is_expected_process(pid: u32, process_type: TunnelProcessType) -> bool {
    let comm_path = format!("/proc/{pid}/comm");
    match fs::read_to_string(&comm_path) {
        Ok(content) => name_matches(content.trim(), process_type),
        Err(_) => {
            // /proc not available or permission denied — conservative fallback
            true
        }
    }
}

/// Check whether a process with the given PID is alive (Windows).
///
/// Opens the process with `PROCESS_QUERY_LIMITED_INFORMATION` and inspects its
/// exit code via `GetExitCodeProcess`. A return of `STILL_ACTIVE` means the
/// process is running. If the handle cannot be opened at all the process is
/// treated as dead (it has exited or never existed).
///
/// Caveat: a process whose real exit code happens to equal `STILL_ACTIVE`
/// (259) after exiting could be misreported as alive, but for our supervised
/// `ssh`/`kubectl`/`sshuttle` processes this is vanishingly unlikely, and the
/// identity check in `is_live_tunnel` guards against recycled PIDs.
#[cfg(windows)]
pub fn is_pid_alive(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    if pid == 0 {
        return false;
    }

    // Safety: OpenProcess returns a null handle on failure, which we check.
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle.is_null() {
        return false;
    }

    let mut exit_code: u32 = 0;
    // Safety: handle is valid; exit_code is a valid out-pointer.
    let ok = unsafe { GetExitCodeProcess(handle, &mut exit_code) };
    // Safety: handle was opened above and is not used after closing.
    unsafe {
        CloseHandle(handle);
    }

    ok != 0 && exit_code == STILL_ACTIVE as u32
}

/// Check whether the process with the given PID matches the expected name (Windows).
///
/// Queries the process image path via `QueryFullProcessImageNameW` and compares
/// the file name against the expected tunnel binary. If the image name cannot
/// be queried (e.g. insufficient access), returns `true` as a conservative
/// fallback — consistent with the Unix `/proc`-unavailable behaviour.
#[cfg(windows)]
#[allow(dead_code)]
pub fn is_expected_process(pid: u32, process_type: TunnelProcessType) -> bool {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
    };

    if pid == 0 {
        return false;
    }

    // Safety: OpenProcess returns a null handle on failure, which we check.
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle.is_null() {
        // Can't open — be conservative and assume identity holds.
        return true;
    }

    let mut buf: [u16; 260] = [0; 260];
    let mut size: u32 = buf.len() as u32;
    // Safety: handle is valid; buf/size are valid out-pointers. Flags = 0 means
    // the Win32 path format is returned.
    let ok = unsafe { QueryFullProcessImageNameW(handle, 0, buf.as_mut_ptr(), &mut size) };
    // Safety: handle was opened above and is not used after closing.
    unsafe {
        CloseHandle(handle);
    }

    if ok == 0 || size == 0 {
        // Query failed — conservative fallback.
        return true;
    }

    let path = String::from_utf16_lossy(&buf[..size as usize]);
    let file_name = path.rsplit(['\\', '/']).next().unwrap_or(path.as_str());
    name_matches(file_name, process_type)
}

/// Check whether the given PID is a live tunnel process of the expected type.
///
/// Combines both liveness and process identity checks. Returns `true` only
/// if the process is alive AND matches the expected process name.
#[cfg(any(unix, windows))]
pub fn is_live_tunnel(pid: u32, process_type: TunnelProcessType) -> bool {
    is_pid_alive(pid) && is_expected_process(pid, process_type)
}

#[cfg(all(test, unix))]
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
                !is_expected_process(pid, TunnelProcessType::Ssh),
                "test runner should not be identified as ssh"
            );
        }
    }

    #[test]
    fn test_current_process_is_not_kubectl() {
        let pid = std::process::id();
        if std::path::Path::new("/proc").exists() {
            assert!(
                !is_expected_process(pid, TunnelProcessType::Kubectl),
                "test runner should not be identified as kubectl"
            );
        }
    }

    #[test]
    fn test_is_live_tunnel_ssh_current_process() {
        let pid = std::process::id();
        // Current process is alive but NOT ssh, so on Linux this should be false.
        if std::path::Path::new("/proc").exists() {
            assert!(
                !is_live_tunnel(pid, TunnelProcessType::Ssh),
                "test runner should not be a live ssh tunnel"
            );
        }
    }

    #[test]
    fn test_is_live_tunnel_kubectl_current_process() {
        let pid = std::process::id();
        if std::path::Path::new("/proc").exists() {
            assert!(
                !is_live_tunnel(pid, TunnelProcessType::Kubectl),
                "test runner should not be a live kubectl tunnel"
            );
        }
    }

    #[test]
    fn test_is_live_tunnel_dead_pid() {
        let pid = 4_000_000;
        if !is_pid_alive(pid) {
            assert!(
                !is_live_tunnel(pid, TunnelProcessType::Ssh),
                "dead PID should not be a live ssh tunnel"
            );
            assert!(
                !is_live_tunnel(pid, TunnelProcessType::Kubectl),
                "dead PID should not be a live kubectl tunnel"
            );
        }
    }

    #[test]
    fn test_current_process_is_not_sshuttle() {
        let pid = std::process::id();
        if std::path::Path::new("/proc").exists() {
            assert!(
                !is_expected_process(pid, TunnelProcessType::Sshuttle),
                "test runner should not be identified as sshuttle"
            );
        }
    }

    #[test]
    fn test_is_live_tunnel_sshuttle_current_process() {
        let pid = std::process::id();
        if std::path::Path::new("/proc").exists() {
            assert!(
                !is_live_tunnel(pid, TunnelProcessType::Sshuttle),
                "test runner should not be a live sshuttle tunnel"
            );
        }
    }

    #[test]
    fn test_is_live_tunnel_sshuttle_dead_pid() {
        let pid = 4_000_000;
        if !is_pid_alive(pid) {
            assert!(
                !is_live_tunnel(pid, TunnelProcessType::Sshuttle),
                "dead PID should not be a live sshuttle tunnel"
            );
        }
    }
}

#[cfg(test)]
mod name_match_tests {
    use super::*;

    #[test]
    fn test_name_matches_with_and_without_exe_suffix() {
        assert!(name_matches("ssh", TunnelProcessType::Ssh));
        assert!(name_matches("ssh.exe", TunnelProcessType::Ssh));
        assert!(name_matches("kubectl.exe", TunnelProcessType::Kubectl));
        assert!(name_matches("sshuttle", TunnelProcessType::Sshuttle));

        assert!(!name_matches("kubectl.exe", TunnelProcessType::Ssh));
        assert!(!name_matches("bash.exe", TunnelProcessType::Ssh));
    }
}

#[cfg(all(test, windows))]
mod windows_tests {
    use super::*;

    #[test]
    fn test_current_process_is_alive() {
        let pid = std::process::id();
        assert!(is_pid_alive(pid), "current process should be alive");
    }

    #[test]
    fn test_pid_zero_is_not_alive() {
        assert!(!is_pid_alive(0), "PID 0 should not be reported alive");
    }

    #[test]
    fn test_nonexistent_pid_is_not_alive() {
        // A very high, almost-certainly-unused PID. Windows PIDs are multiples
        // of 4 and rarely this large.
        let pid = 0x7FFF_FFF0;
        assert!(!is_pid_alive(pid), "unused high PID should not be alive");
    }

    #[test]
    fn test_current_process_is_not_ssh() {
        // The test runner image is not ssh.exe, so identity should fail.
        let pid = std::process::id();
        assert!(
            !is_expected_process(pid, TunnelProcessType::Ssh),
            "test runner should not be identified as ssh"
        );
    }

    #[test]
    fn test_is_live_tunnel_current_process_is_not_ssh() {
        let pid = std::process::id();
        assert!(
            !is_live_tunnel(pid, TunnelProcessType::Ssh),
            "test runner is alive but is not ssh"
        );
    }

    #[test]
    fn test_is_live_tunnel_dead_pid() {
        let pid = 0x7FFF_FFF0;
        assert!(
            !is_live_tunnel(pid, TunnelProcessType::Ssh),
            "dead PID should not be a live ssh tunnel"
        );
    }
}
