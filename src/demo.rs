//! Demo mode — synthetic tunnel entries and simulated lifecycle events.
//!
//! Provides hardcoded fixture entries and a per-tunnel simulation loop that
//! emits [`TunnelEvent`] variants through the existing event channel, allowing
//! the TUI to render realistic tunnel activity without any real processes.

use std::time::Duration;

use rand::rngs::SmallRng;
use rand::{RngExt, SeedableRng};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::config::{
    K8sEntry, K8sPortForward, K8sResourceType, ServerEntry, SshuttleEntry, TunnelEntry,
    TunnelForward,
};
use crate::tunnel::TunnelEvent;

/// Returns a fixed set of synthetic tunnel entries for demo mode.
///
/// Covers all three tunnel types with varied configurations:
/// - 2 SSH entries (local, remote, and dynamic forwards)
/// - 2 K8s entries (Deployment and Service with port-forward bindings)
/// - 2 sshuttle entries (single and multi-subnet)
///
/// A subset of entries have `auto_restart: true` so the reconnect cycle
/// is exercised during the simulation.
pub fn demo_entries() -> Vec<TunnelEntry> {
    vec![
        // SSH: 4 forwards (local + remote + dynamic), auto_restart enabled
        TunnelEntry::Ssh(ServerEntry {
            id: Uuid::new_v4(),
            name: "Production DB".to_string(),
            host: "bastion.example.com".to_string(),
            port: 22,
            user: Some("deploy".to_string()),
            identity_file: Some("~/.ssh/id_ed25519".to_string()),
            forwards: vec![
                TunnelForward::Local {
                    bind_address: "127.0.0.1".to_string(),
                    bind_port: 5432,
                    remote_host: "db.internal".to_string(),
                    remote_port: 5432,
                },
                TunnelForward::Local {
                    bind_address: "127.0.0.1".to_string(),
                    bind_port: 6379,
                    remote_host: "redis.internal".to_string(),
                    remote_port: 6379,
                },
                TunnelForward::Local {
                    bind_address: "127.0.0.1".to_string(),
                    bind_port: 27017,
                    remote_host: "mongo.internal".to_string(),
                    remote_port: 27017,
                },
                TunnelForward::Remote {
                    bind_address: "127.0.0.1".to_string(),
                    bind_port: 9200,
                    remote_host: "localhost".to_string(),
                    remote_port: 9200,
                },
            ],
            auto_restart: true,
        }),
        // SSH: 3 forwards (remote + local + dynamic)
        TunnelEntry::Ssh(ServerEntry {
            id: Uuid::new_v4(),
            name: "Dev API Gateway".to_string(),
            host: "dev-gw.example.com".to_string(),
            port: 2222,
            user: Some("admin".to_string()),
            identity_file: None,
            forwards: vec![
                TunnelForward::Remote {
                    bind_address: "127.0.0.1".to_string(),
                    bind_port: 8080,
                    remote_host: "localhost".to_string(),
                    remote_port: 3000,
                },
                TunnelForward::Local {
                    bind_address: "127.0.0.1".to_string(),
                    bind_port: 8443,
                    remote_host: "api.internal".to_string(),
                    remote_port: 443,
                },
                TunnelForward::Dynamic {
                    bind_address: "127.0.0.1".to_string(),
                    bind_port: 1080,
                },
            ],
            auto_restart: true,
        }),
        // K8s: Deployment with 4 forwards, auto_restart enabled
        TunnelEntry::K8s(K8sEntry {
            id: Uuid::new_v4(),
            name: "Staging Pods".to_string(),
            context: Some("staging".to_string()),
            namespace: Some("app".to_string()),
            resource_type: K8sResourceType::Deployment,
            resource_name: "api-server".to_string(),
            forwards: vec![
                K8sPortForward {
                    local_bind_address: "127.0.0.1".to_string(),
                    local_port: 8080,
                    remote_port: 80,
                },
                K8sPortForward {
                    local_bind_address: "127.0.0.1".to_string(),
                    local_port: 8443,
                    remote_port: 443,
                },
                K8sPortForward {
                    local_bind_address: "127.0.0.1".to_string(),
                    local_port: 9090,
                    remote_port: 9090,
                },
                K8sPortForward {
                    local_bind_address: "127.0.0.1".to_string(),
                    local_port: 9100,
                    remote_port: 9100,
                },
            ],
            auto_restart: true,
        }),
        // K8s: Service with 3 forwards
        TunnelEntry::K8s(K8sEntry {
            id: Uuid::new_v4(),
            name: "Monitoring Stack".to_string(),
            context: Some("prod".to_string()),
            namespace: Some("monitoring".to_string()),
            resource_type: K8sResourceType::Service,
            resource_name: "grafana".to_string(),
            forwards: vec![
                K8sPortForward {
                    local_bind_address: "127.0.0.1".to_string(),
                    local_port: 3000,
                    remote_port: 3000,
                },
                K8sPortForward {
                    local_bind_address: "127.0.0.1".to_string(),
                    local_port: 9090,
                    remote_port: 9090,
                },
                K8sPortForward {
                    local_bind_address: "127.0.0.1".to_string(),
                    local_port: 9093,
                    remote_port: 9093,
                },
            ],
            auto_restart: false,
        }),
        // sshuttle: 2 subnets, auto_restart enabled
        TunnelEntry::Sshuttle(SshuttleEntry {
            id: Uuid::new_v4(),
            name: "Corp VPN".to_string(),
            host: "vpn.example.com".to_string(),
            port: Some(2222),
            user: Some("alice".to_string()),
            identity_file: Some("~/.ssh/corp_key".to_string()),
            subnets: vec!["10.0.0.0/8".to_string(), "172.16.0.0/12".to_string()],
            auto_restart: true,
        }),
        // sshuttle: 1 subnet
        TunnelEntry::Sshuttle(SshuttleEntry {
            id: Uuid::new_v4(),
            name: "Lab Network".to_string(),
            host: "lab-gw.internal".to_string(),
            port: None,
            user: None,
            identity_file: None,
            subnets: vec!["192.168.50.0/24".to_string()],
            auto_restart: false,
        }),
        // SSH: minimal — no forwards, no identity
        TunnelEntry::Ssh(ServerEntry {
            id: Uuid::new_v4(),
            name: "Jump Host".to_string(),
            host: "jump.example.com".to_string(),
            port: 22,
            user: Some("ops".to_string()),
            identity_file: None,
            forwards: vec![],
            auto_restart: false,
        }),
        // K8s: Pod resource, single port binding
        TunnelEntry::K8s(K8sEntry {
            id: Uuid::new_v4(),
            name: "Debug Pod".to_string(),
            context: Some("dev".to_string()),
            namespace: Some("debug".to_string()),
            resource_type: K8sResourceType::Pod,
            resource_name: "debug-shell-xyz".to_string(),
            forwards: vec![K8sPortForward {
                local_bind_address: "127.0.0.1".to_string(),
                local_port: 2345,
                remote_port: 2345,
            }],
            auto_restart: false,
        }),
        // SSH: 2 local forwards only
        TunnelEntry::Ssh(ServerEntry {
            id: Uuid::new_v4(),
            name: "Analytics Cluster".to_string(),
            host: "analytics.example.com".to_string(),
            port: 22,
            user: Some("analyst".to_string()),
            identity_file: Some("~/.ssh/id_rsa".to_string()),
            forwards: vec![
                TunnelForward::Local {
                    bind_address: "127.0.0.1".to_string(),
                    bind_port: 9000,
                    remote_host: "clickhouse.internal".to_string(),
                    remote_port: 9000,
                },
                TunnelForward::Local {
                    bind_address: "127.0.0.1".to_string(),
                    bind_port: 9001,
                    remote_host: "clickhouse.internal".to_string(),
                    remote_port: 9001,
                },
            ],
            auto_restart: true,
        }),
        // sshuttle: 3 subnets
        TunnelEntry::Sshuttle(SshuttleEntry {
            id: Uuid::new_v4(),
            name: "Multi-Region VPN".to_string(),
            host: "vpn2.example.com".to_string(),
            port: None,
            user: Some("netops".to_string()),
            identity_file: Some("~/.ssh/netops_key".to_string()),
            subnets: vec![
                "10.0.0.0/8".to_string(),
                "172.16.0.0/12".to_string(),
                "192.168.0.0/16".to_string(),
            ],
            auto_restart: true,
        }),
    ]
}

/// Start the demo simulation for all entries.
///
/// Spawns one tokio task per entry that cycles through realistic tunnel
/// lifecycle transitions, emitting [`TunnelEvent`] variants through `tx`.
///
/// Returns a [`CancellationToken`] that, when cancelled, stops all
/// simulation tasks.
pub fn start_demo(
    entries: &[TunnelEntry],
    tx: mpsc::UnboundedSender<TunnelEvent>,
) -> CancellationToken {
    let cancel = CancellationToken::new();

    for entry in entries {
        let entry_id = entry.id();
        let auto_restart = entry.auto_restart();
        let tx = tx.clone();
        let cancel = cancel.clone();

        tokio::spawn(async move {
            run_demo_tunnel(entry_id, auto_restart, tx, cancel).await;
        });
    }

    cancel
}

/// Simulate the lifecycle of a single tunnel.
///
/// Runs in a loop:
/// 1. Staggered startup delay (0–3 s)
/// 2. Connecting phase (2–5 s)
/// 3. Connected hold (15–60 s)
/// 4. Random event: disconnect (70%) or failure (30%)
/// 5. If auto_restart and disconnect: reconnect sequence then loop
/// 6. If failure: hold in Failed state (10–20 s) then restart cycle
async fn run_demo_tunnel(
    entry_id: Uuid,
    auto_restart: bool,
    tx: mpsc::UnboundedSender<TunnelEvent>,
    cancel: CancellationToken,
) {
    let mut rng = SmallRng::from_rng(&mut rand::rng());

    // Staggered initial delay so tunnels don't all start at once
    let stagger = Duration::from_millis(rng.random_range(0..3000));
    if cancel_sleep(stagger, &cancel).await {
        return;
    }

    loop {
        // ── Connecting ─────────────────────────────────────────────
        // The App will set Connecting state when it receives PidUpdate
        // with a fake PID. We send PidUpdate to trigger the Connecting
        // visual, then after a delay send Connected.
        let fake_pid: u32 = rng.random_range(10000..60000);
        if tx
            .send(TunnelEvent::PidUpdate {
                entry_id,
                pid: fake_pid,
            })
            .is_err()
        {
            return;
        }

        let connecting_dur = Duration::from_millis(rng.random_range(2000..5000));
        if cancel_sleep(connecting_dur, &cancel).await {
            return;
        }

        // ── Connected ──────────────────────────────────────────────
        if tx.send(TunnelEvent::Connected { entry_id }).is_err() {
            return;
        }

        let connected_dur = Duration::from_millis(rng.random_range(15000..60000));
        if cancel_sleep(connected_dur, &cancel).await {
            return;
        }

        // ── Random event: disconnect or failure ────────────────────
        let roll: f64 = rng.random_range(0.0..1.0);

        if roll < 0.3 {
            // ── Permanent failure path ─────────────────────────────
            if tx
                .send(TunnelEvent::Failed {
                    entry_id,
                    reason: "connection refused (simulated)".to_string(),
                })
                .is_err()
            {
                return;
            }

            let failed_dur = Duration::from_millis(rng.random_range(10000..20000));
            if cancel_sleep(failed_dur, &cancel).await {
                return;
            }

            // After the failed hold, restart the entire cycle
            continue;
        }

        // ── Disconnect path ────────────────────────────────────────
        if tx.send(TunnelEvent::Disconnected { entry_id }).is_err() {
            return;
        }

        if auto_restart {
            // Reconnect sequence: 1–2 attempts before reconnecting
            let attempts = if rng.random_range(0.0..1.0) < 0.2 {
                2
            } else {
                1
            };

            for attempt in 1..=attempts {
                let delay_secs: u64 = rng.random_range(2..5);
                if tx
                    .send(TunnelEvent::Reconnecting {
                        entry_id,
                        attempt,
                        delay_secs,
                    })
                    .is_err()
                {
                    return;
                }

                let reconnect_dur = Duration::from_secs(delay_secs);
                if cancel_sleep(reconnect_dur, &cancel).await {
                    return;
                }
            }

            // Loop back to the Connecting phase at top of loop
            continue;
        }

        // Non-auto_restart: stay disconnected for a while, then restart cycle
        let rest_dur = Duration::from_millis(rng.random_range(8000..15000));
        if cancel_sleep(rest_dur, &cancel).await {
            return;
        }
    }
}

/// Sleep for `duration`, returning `true` if cancelled before waking.
async fn cancel_sleep(duration: Duration, cancel: &CancellationToken) -> bool {
    tokio::select! {
        _ = cancel.cancelled() => true,
        _ = tokio::time::sleep(duration) => false,
    }
}
