//! Process-level integration tests using a fake CLI and plugin, without AWS.

use super::*;
use std::net::TcpListener;

const FIXTURE: &str = "tunnel::supervisor::tests::ssm_process_fixture";

fn fixture_command(port: u16) -> Command {
    let mut cmd = Command::new(std::env::current_exe().unwrap());
    cmd.args(["--ignored", "--exact", FIXTURE, "--nocapture"])
        .env("STUNT_TEST_SSM_PORT", port.to_string())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    cmd
}

#[test]
#[ignore = "subprocess fixture invoked by lifecycle tests"]
fn ssm_process_fixture() {
    let Ok(port) = std::env::var("STUNT_TEST_SSM_PORT") else {
        return;
    };
    let port: u16 = port.parse().unwrap();
    if std::env::var_os("STUNT_TEST_SSM_PLUGIN").is_some() {
        let listener = TcpListener::bind(("127.0.0.1", port)).unwrap();
        println!("Port opened by fake Session Manager plugin");
        let mut first = true;
        for stream in listener.incoming() {
            let mut stream = stream.unwrap();
            if first && let Ok(delay) = std::env::var("STUNT_TEST_SSM_FAIL_MS") {
                let delay = delay.parse::<u64>().unwrap();
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(delay));
                    let error = "Connection to destination port failed, check SSM Agent logs.";
                    if std::env::var_os("STUNT_TEST_SSM_STDERR").is_some() {
                        eprintln!("{error}");
                    } else {
                        println!("{error}");
                    }
                });
            }
            first = false;
            std::thread::spawn(move || {
                use std::io::Read;
                let mut byte = [0];
                while stream.read(&mut byte).is_ok_and(|count| count > 0) {}
            });
        }
    } else {
        // Match the real CLI's process name for Linux adoption checks.
        #[cfg(target_os = "linux")]
        unsafe {
            libc::prctl(libc::PR_SET_NAME, c"aws".as_ptr(), 0, 0, 0);
        }
        let mut cmd = fixture_command(port).into_std();
        cmd.env("STUNT_TEST_SSM_PLUGIN", "1")
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit());
        let mut plugin = cmd.spawn().unwrap();
        println!("Fake CLI launched plugin");
        plugin.wait().unwrap();
    }
}

async fn wait_event(
    rx: &mut mpsc::UnboundedReceiver<TunnelEvent>,
    matches: impl Fn(&TunnelEvent) -> bool,
) {
    tokio::time::timeout(Duration::from_secs(10), async {
        while let Some(event) = rx.recv().await {
            if matches(&event) {
                return;
            }
        }
        panic!("event channel closed");
    })
    .await
    .expect("expected tunnel event");
}

struct ProcessCleanup(u32);
impl Drop for ProcessCleanup {
    fn drop(&mut self) {
        if self.0 != 0 {
            // Safety: this PID is the detached fixture's process-group leader.
            unsafe {
                libc::kill(-(self.0 as libc::pid_t), libc::SIGKILL);
            }
        }
    }
}

#[tokio::test]
async fn ssm_process_survives_quit_and_adoption_disconnect_closes_plugin() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let (tx, mut rx) = mpsc::unbounded_channel();
    let id = uuid::Uuid::new_v4();
    let supervisor = Supervisor::spawn(
        id,
        false,
        TunnelProcessType::Ssm,
        Box::new(move || fixture_command(port)),
        Some(ReadinessProbe::new("127.0.0.1", port)),
        tx.clone(),
    );
    wait_event(&mut rx, |e| matches!(e, TunnelEvent::PidUpdate { .. })).await;
    let _cleanup = ProcessCleanup(supervisor.pid());
    wait_event(&mut rx, |e| matches!(e, TunnelEvent::Connected { .. })).await;
    let pid = supervisor.pid();
    supervisor.cancel();
    supervisor._handle.await.unwrap();
    assert!(std::net::TcpStream::connect(("127.0.0.1", port)).is_ok());

    let adopted = Supervisor::adopt(
        id,
        pid,
        false,
        TunnelProcessType::Ssm,
        Box::new(move || fixture_command(port)),
        Some(ReadinessProbe::new("127.0.0.1", port)),
        tx,
    );
    wait_event(&mut rx, |e| matches!(e, TunnelEvent::Connected { .. })).await;
    adopted.cancel_and_kill();
    adopted._handle.await.unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        while std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("disconnect must close the plugin listener");
}

#[tokio::test]
async fn ssm_rejects_an_occupied_port_before_spawning() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let supervisor = Supervisor::spawn(
        uuid::Uuid::new_v4(),
        false,
        TunnelProcessType::Ssm,
        Box::new(|| panic!("must not spawn when the local port is occupied")),
        Some(ReadinessProbe::new("127.0.0.1", port)),
        tx,
    );
    wait_event(
        &mut rx,
        |e| matches!(e, TunnelEvent::Failed { reason, .. } if reason.contains("unavailable")),
    )
    .await;
    supervisor._handle.await.unwrap();
}

#[tokio::test]
async fn ssm_reports_failure_output_and_retries_unexpected_exit() {
    let (tx, mut rx) = mpsc::unbounded_channel();
    let supervisor = Supervisor::spawn(
        uuid::Uuid::new_v4(),
        true,
        TunnelProcessType::Ssm,
        Box::new(|| {
            let mut cmd = Command::new("sh");
            cmd.args(["-c", "echo TargetNotConnected >&2; exit 7"])
                .stderr(std::process::Stdio::piped());
            cmd
        }),
        None,
        tx,
    );
    let mut saw_error = false;
    let mut saw_exit = false;
    tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(event) = rx.recv().await {
            match event {
                TunnelEvent::Output { text, .. } if text == "TargetNotConnected" => {
                    saw_error = true
                }
                TunnelEvent::ExitStatus { code: Some(7), .. } => saw_exit = true,
                TunnelEvent::Reconnecting { .. } if saw_error && saw_exit => break,
                _ => {}
            }
        }
    })
    .await
    .unwrap();
    supervisor.cancel_and_kill();
    supervisor._handle.await.unwrap();
    assert!(saw_error && saw_exit);
}

async fn assert_destination_error(delay_ms: u64, stderr: bool) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let (tx, mut rx) = mpsc::unbounded_channel();
    let supervisor = Supervisor::spawn(
        uuid::Uuid::new_v4(),
        false,
        TunnelProcessType::Ssm,
        Box::new(move || {
            let mut cmd = fixture_command(port);
            cmd.env("STUNT_TEST_SSM_FAIL_MS", delay_ms.to_string());
            if stderr {
                cmd.env("STUNT_TEST_SSM_STDERR", "1");
            }
            cmd
        }),
        Some(ReadinessProbe::new("127.0.0.1", port)),
        tx,
    );
    wait_event(&mut rx, |e| matches!(e, TunnelEvent::PidUpdate { .. })).await;
    let _cleanup = ProcessCleanup(supervisor.pid());
    let mut connected = false;
    let mut logged = false;
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            match rx.recv().await.unwrap() {
                TunnelEvent::Connected { .. } => connected = true,
                TunnelEvent::Output { stream, text, .. }
                    if text.starts_with("Connection to destination port failed") =>
                {
                    assert_eq!(
                        stream,
                        if stderr {
                            LogStream::Stderr
                        } else {
                            LogStream::Stdout
                        }
                    );
                    logged = true;
                }
                TunnelEvent::Failed { reason, .. } => {
                    assert!(reason.contains("Connection to destination port failed"));
                    break;
                }
                _ => {}
            }
        }
    })
    .await
    .expect("a live plugin's destination error must fail the tunnel");
    supervisor._handle.await.unwrap();
    assert!(logged);
    assert_eq!(
        connected,
        delay_ms > 3000,
        "startup errors must never become Connected"
    );
    assert!(std::net::TcpStream::connect(("127.0.0.1", port)).is_err());
}

#[tokio::test]
async fn ssm_destination_error_on_stdout_prevents_connected() {
    assert_destination_error(100, false).await;
}

#[tokio::test]
async fn ssm_destination_error_on_stderr_prevents_connected() {
    assert_destination_error(100, true).await;
}

#[tokio::test]
async fn ssm_destination_error_after_connected_fails_live_process() {
    assert_destination_error(4000, false).await;
}

#[tokio::test]
async fn ssm_destination_error_reconnects_with_fresh_error_state() {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let attempts = Arc::new(AtomicU32::new(0));
    let count = attempts.clone();
    let (tx, mut rx) = mpsc::unbounded_channel();
    let supervisor = Supervisor::spawn(
        uuid::Uuid::new_v4(),
        true,
        TunnelProcessType::Ssm,
        Box::new(move || {
            let mut cmd = fixture_command(port);
            if count.fetch_add(1, Ordering::Relaxed) == 0 {
                cmd.env("STUNT_TEST_SSM_FAIL_MS", "100");
            }
            cmd
        }),
        Some(ReadinessProbe::new("127.0.0.1", port)),
        tx,
    );
    wait_event(&mut rx, |e| matches!(e, TunnelEvent::PidUpdate { .. })).await;
    let _cleanup_first = ProcessCleanup(supervisor.pid());
    wait_event(&mut rx, |e| matches!(e, TunnelEvent::Reconnecting { .. })).await;
    wait_event(&mut rx, |e| matches!(e, TunnelEvent::PidUpdate { .. })).await;
    let _cleanup_second = ProcessCleanup(supervisor.pid());
    wait_event(&mut rx, |e| matches!(e, TunnelEvent::Connected { .. })).await;
    assert_eq!(attempts.load(Ordering::Relaxed), 2);
    supervisor.cancel_and_kill();
    supervisor._handle.await.unwrap();
}
