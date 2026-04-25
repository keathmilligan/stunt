//! Data model for tunnel configuration.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Top-level configuration containing all tunnel entries.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct Config {
    /// List of configured tunnel entries (SSH servers or Kubernetes workloads).
    #[serde(default)]
    pub entries: Vec<TunnelEntry>,
}

/// A tunnel entry — either an SSH server, a Kubernetes workload target, or an sshuttle VPN session.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum TunnelEntry {
    /// An SSH server with port-forward definitions.
    Ssh(ServerEntry),
    /// A Kubernetes workload target with port-forward bindings.
    K8s(K8sEntry),
    /// An sshuttle VPN-over-SSH session routing one or more subnets.
    Sshuttle(SshuttleEntry),
}

impl TunnelEntry {
    /// Returns the unique ID of this entry.
    pub fn id(&self) -> Uuid {
        match self {
            TunnelEntry::Ssh(e) => e.id,
            TunnelEntry::K8s(e) => e.id,
            TunnelEntry::Sshuttle(e) => e.id,
        }
    }

    /// Returns the display name of this entry.
    pub fn name(&self) -> &str {
        match self {
            TunnelEntry::Ssh(e) => &e.name,
            TunnelEntry::K8s(e) => &e.name,
            TunnelEntry::Sshuttle(e) => &e.name,
        }
    }

    /// Returns whether auto-restart is enabled for this entry.
    pub fn auto_restart(&self) -> bool {
        match self {
            TunnelEntry::Ssh(e) => e.auto_restart,
            TunnelEntry::K8s(e) => e.auto_restart,
            TunnelEntry::Sshuttle(e) => e.auto_restart,
        }
    }
}

/// An SSH server with connection details and tunnel forward definitions.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ServerEntry {
    /// Unique identifier for this entry.
    #[serde(default = "Uuid::new_v4")]
    pub id: Uuid,

    /// Display name for this server entry.
    pub name: String,

    /// SSH server hostname or IP address.
    pub host: String,

    /// SSH server port.
    #[serde(default = "default_ssh_port")]
    pub port: u16,

    /// SSH username (uses system default if omitted).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,

    /// Path to the SSH identity (private key) file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity_file: Option<String>,

    /// Tunnel forward definitions for this server.
    #[serde(default)]
    pub forwards: Vec<TunnelForward>,

    /// Whether to automatically restart this tunnel on unexpected disconnect.
    /// Only active while the TUI is running.
    #[serde(default)]
    pub auto_restart: bool,
}

/// A Kubernetes workload target with port-forward bindings.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct K8sEntry {
    /// Unique identifier for this entry.
    #[serde(default = "Uuid::new_v4")]
    pub id: Uuid,

    /// Display name for this entry.
    pub name: String,

    /// Path to an alternate kubeconfig file (uses default kubeconfig if omitted).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kubeconfig: Option<String>,

    /// kubeconfig context to use (uses default context if omitted).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,

    /// Kubernetes namespace (uses default namespace if omitted).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,

    /// The type of Kubernetes resource to port-forward to.
    pub resource_type: K8sResourceType,

    /// The name of the Kubernetes resource.
    pub resource_name: String,

    /// Port-forward bindings for this workload.
    #[serde(default)]
    pub forwards: Vec<K8sPortForward>,

    /// Whether to automatically restart this tunnel on unexpected disconnect.
    /// Only active while the TUI is running.
    #[serde(default)]
    pub auto_restart: bool,
}

impl K8sEntry {
    /// Returns the resource identifier string used in `kubectl port-forward`
    /// (e.g., `"deployment/api"`, `"service/postgres"`).
    pub fn resource_identifier(&self) -> String {
        format!("{}/{}", self.resource_type.as_str(), self.resource_name)
    }

    /// Returns a display label showing the entry name and resource identifier.
    #[allow(dead_code)]
    pub fn display_label(&self) -> String {
        format!("{} ({})", self.name, self.resource_identifier())
    }
}

/// The type of Kubernetes resource targeted by a port-forward.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum K8sResourceType {
    /// A pod resource.
    Pod,
    /// A service resource.
    Service,
    /// A deployment resource.
    Deployment,
}

impl K8sResourceType {
    /// Returns the lowercase string used in kubectl commands.
    pub fn as_str(self) -> &'static str {
        match self {
            K8sResourceType::Pod => "pod",
            K8sResourceType::Service => "service",
            K8sResourceType::Deployment => "deployment",
        }
    }
}

/// A single Kubernetes port-forward binding (local → remote port).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct K8sPortForward {
    /// Local address to bind (default: "127.0.0.1").
    #[serde(default = "default_bind_address")]
    pub local_bind_address: String,

    /// Local port to listen on.
    pub local_port: u16,

    /// Remote (container) port to forward to.
    pub remote_port: u16,
}

impl K8sPortForward {
    /// Returns the port-mapping argument for `kubectl port-forward`
    /// in the form `<bind>:<local>:<remote>`.
    pub fn kubectl_arg(&self) -> String {
        format!(
            "{}:{}:{}",
            self.local_bind_address, self.local_port, self.remote_port
        )
    }

    /// Returns a human-readable display string.
    pub fn display_address(&self) -> String {
        format!("{} -> :{}", self.local_port, self.remote_port)
    }
}

/// An sshuttle VPN-over-SSH session routing one or more subnets.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SshuttleEntry {
    /// Unique identifier for this entry.
    #[serde(default = "Uuid::new_v4")]
    pub id: Uuid,

    /// Display name for this entry.
    pub name: String,

    /// SSH server hostname or IP address.
    pub host: String,

    /// SSH server port (uses sshuttle default if omitted).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,

    /// SSH username (uses system default if omitted).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,

    /// Path to the SSH identity (private key) file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity_file: Option<String>,

    /// Subnets to route through the tunnel (e.g. `["10.0.0.0/8"]`).
    #[serde(default)]
    pub subnets: Vec<String>,

    /// Whether to automatically restart this tunnel on unexpected disconnect.
    /// Only active while the TUI is running.
    #[serde(default)]
    pub auto_restart: bool,
}

/// An SSH port-forwarding definition.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum TunnelForward {
    /// Local port forward (`ssh -L`).
    Local {
        /// Local address to bind (default: "127.0.0.1").
        #[serde(default = "default_bind_address")]
        bind_address: String,
        /// Local port to bind.
        bind_port: u16,
        /// Remote host to forward to.
        remote_host: String,
        /// Remote port to forward to.
        remote_port: u16,
    },
    /// Remote port forward (`ssh -R`).
    Remote {
        /// Remote address to bind (default: "127.0.0.1").
        #[serde(default = "default_bind_address")]
        bind_address: String,
        /// Remote port to bind.
        bind_port: u16,
        /// Local host to forward to.
        remote_host: String,
        /// Local port to forward to.
        remote_port: u16,
    },
    /// Dynamic SOCKS proxy (`ssh -D`).
    Dynamic {
        /// Local address to bind (default: "127.0.0.1").
        #[serde(default = "default_bind_address")]
        bind_address: String,
        /// Local port to bind.
        bind_port: u16,
    },
}

/// Default SSH port.
fn default_ssh_port() -> u16 {
    22
}

/// Default bind address for tunnel forwards.
fn default_bind_address() -> String {
    "127.0.0.1".to_string()
}

impl TunnelForward {
    /// Returns the ssh flag representation of this forward.
    pub fn to_ssh_flag(&self) -> String {
        match self {
            TunnelForward::Local {
                bind_address,
                bind_port,
                remote_host,
                remote_port,
            } => format!("-L {bind_address}:{bind_port}:{remote_host}:{remote_port}"),
            TunnelForward::Remote {
                bind_address,
                bind_port,
                remote_host,
                remote_port,
            } => format!("-R {bind_address}:{bind_port}:{remote_host}:{remote_port}"),
            TunnelForward::Dynamic {
                bind_address,
                bind_port,
            } => format!("-D {bind_address}:{bind_port}"),
        }
    }

    /// Returns a short type label for display.
    pub fn type_label(&self) -> &'static str {
        match self {
            TunnelForward::Local { .. } => "L",
            TunnelForward::Remote { .. } => "R",
            TunnelForward::Dynamic { .. } => "D",
        }
    }

    /// Returns a display string for the forward's addressing.
    pub fn display_address(&self) -> String {
        match self {
            TunnelForward::Local {
                bind_port,
                remote_host,
                remote_port,
                ..
            } => format!("{bind_port} -> {remote_host}:{remote_port}"),
            TunnelForward::Remote {
                bind_port,
                remote_host,
                remote_port,
                ..
            } => format!("{bind_port} <- {remote_host}:{remote_port}"),
            TunnelForward::Dynamic { bind_port, .. } => format!("{bind_port} (SOCKS)"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Existing SSH tests ───────────────────────────────────────────────

    #[test]
    fn test_minimal_server_entry_defaults() {
        let toml_str = r#"
            [[entries]]
            type = "ssh"
            name = "test"
            host = "example.com"
        "#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.entries.len(), 1);
        let TunnelEntry::Ssh(entry) = &config.entries[0] else {
            panic!("expected SSH entry");
        };
        assert_eq!(entry.port, 22);
        assert!(entry.user.is_none());
        assert!(entry.identity_file.is_none());
        assert!(entry.forwards.is_empty());
    }

    #[test]
    fn test_ssh_round_trip_serialization() {
        let config = Config {
            entries: vec![TunnelEntry::Ssh(ServerEntry {
                id: Uuid::new_v4(),
                name: "prod-db".to_string(),
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
            })],
        };

        let serialized = toml::to_string_pretty(&config).unwrap();
        let deserialized: Config = toml::from_str(&serialized).unwrap();
        assert_eq!(config, deserialized);
    }

    #[test]
    fn test_invalid_toml() {
        let result = toml::from_str::<Config>("this is not valid toml [[[");
        assert!(result.is_err());
    }

    #[test]
    fn test_ssh_flag_local() {
        let fwd = TunnelForward::Local {
            bind_address: "127.0.0.1".to_string(),
            bind_port: 5432,
            remote_host: "db.internal".to_string(),
            remote_port: 5432,
        };
        assert_eq!(fwd.to_ssh_flag(), "-L 127.0.0.1:5432:db.internal:5432");
    }

    #[test]
    fn test_ssh_flag_remote() {
        let fwd = TunnelForward::Remote {
            bind_address: "127.0.0.1".to_string(),
            bind_port: 8080,
            remote_host: "localhost".to_string(),
            remote_port: 3000,
        };
        assert_eq!(fwd.to_ssh_flag(), "-R 127.0.0.1:8080:localhost:3000");
    }

    #[test]
    fn test_ssh_flag_dynamic() {
        let fwd = TunnelForward::Dynamic {
            bind_address: "127.0.0.1".to_string(),
            bind_port: 1080,
        };
        assert_eq!(fwd.to_ssh_flag(), "-D 127.0.0.1:1080");
    }

    #[test]
    fn test_auto_restart_round_trip() {
        let config = Config {
            entries: vec![TunnelEntry::Ssh(ServerEntry {
                id: Uuid::new_v4(),
                name: "restart-test".to_string(),
                host: "example.com".to_string(),
                port: 22,
                user: None,
                identity_file: None,
                forwards: vec![],
                auto_restart: true,
            })],
        };
        let serialized = toml::to_string_pretty(&config).unwrap();
        let deserialized: Config = toml::from_str(&serialized).unwrap();
        assert_eq!(config, deserialized);
        let TunnelEntry::Ssh(entry) = &deserialized.entries[0] else {
            panic!("expected SSH entry");
        };
        assert!(entry.auto_restart);
    }

    #[test]
    fn test_auto_restart_defaults_false() {
        let toml_str = r#"
            [[entries]]
            type = "ssh"
            name = "no-restart"
            host = "example.com"
        "#;
        let config: Config = toml::from_str(toml_str).unwrap();
        let TunnelEntry::Ssh(entry) = &config.entries[0] else {
            panic!("expected SSH entry");
        };
        assert!(!entry.auto_restart);
    }

    // ── K8s model tests ─────────────────────────────────────────────────

    #[test]
    fn test_k8s_port_forward_defaults() {
        let fwd = K8sPortForward {
            local_bind_address: default_bind_address(),
            local_port: 8080,
            remote_port: 80,
        };
        assert_eq!(fwd.local_bind_address, "127.0.0.1");
        assert_eq!(fwd.kubectl_arg(), "127.0.0.1:8080:80");
        assert_eq!(fwd.display_address(), "8080 -> :80");
    }

    #[test]
    fn test_k8s_port_forward_round_trip() {
        let fwd = K8sPortForward {
            local_bind_address: "127.0.0.1".to_string(),
            local_port: 5432,
            remote_port: 5432,
        };
        let serialized = toml::to_string_pretty(&fwd).unwrap();
        let deserialized: K8sPortForward = toml::from_str(&serialized).unwrap();
        assert_eq!(fwd, deserialized);
    }

    #[test]
    fn test_k8s_entry_resource_identifier() {
        let entry = K8sEntry {
            id: Uuid::new_v4(),
            name: "api".to_string(),
            kubeconfig: None,
            context: None,
            namespace: None,
            resource_type: K8sResourceType::Deployment,
            resource_name: "api-server".to_string(),
            forwards: vec![],
            auto_restart: false,
        };
        assert_eq!(entry.resource_identifier(), "deployment/api-server");
        assert_eq!(entry.display_label(), "api (deployment/api-server)");
    }

    #[test]
    fn test_k8s_entry_round_trip() {
        let config = Config {
            entries: vec![TunnelEntry::K8s(K8sEntry {
                id: Uuid::new_v4(),
                name: "postgres-debug".to_string(),
                kubeconfig: None,
                context: Some("prod".to_string()),
                namespace: Some("default".to_string()),
                resource_type: K8sResourceType::Service,
                resource_name: "postgres".to_string(),
                forwards: vec![K8sPortForward {
                    local_bind_address: "127.0.0.1".to_string(),
                    local_port: 5432,
                    remote_port: 5432,
                }],
                auto_restart: true,
            })],
        };

        let serialized = toml::to_string_pretty(&config).unwrap();
        let deserialized: Config = toml::from_str(&serialized).unwrap();
        assert_eq!(config, deserialized);
    }

    #[test]
    fn test_tunnel_entry_mixed_round_trip() {
        let config = Config {
            entries: vec![
                TunnelEntry::Ssh(ServerEntry {
                    id: Uuid::new_v4(),
                    name: "bastion".to_string(),
                    host: "bastion.example.com".to_string(),
                    port: 22,
                    user: None,
                    identity_file: None,
                    forwards: vec![],
                    auto_restart: false,
                }),
                TunnelEntry::K8s(K8sEntry {
                    id: Uuid::new_v4(),
                    name: "api-debug".to_string(),
                    kubeconfig: None,
                    context: None,
                    namespace: Some("staging".to_string()),
                    resource_type: K8sResourceType::Deployment,
                    resource_name: "api".to_string(),
                    forwards: vec![K8sPortForward {
                        local_bind_address: "127.0.0.1".to_string(),
                        local_port: 8080,
                        remote_port: 80,
                    }],
                    auto_restart: false,
                }),
            ],
        };

        let serialized = toml::to_string_pretty(&config).unwrap();
        let deserialized: Config = toml::from_str(&serialized).unwrap();
        assert_eq!(config, deserialized);

        // Verify types preserved
        assert!(matches!(deserialized.entries[0], TunnelEntry::Ssh(_)));
        assert!(matches!(deserialized.entries[1], TunnelEntry::K8s(_)));
    }

    #[test]
    fn test_tunnel_entry_helpers() {
        let ssh = TunnelEntry::Ssh(ServerEntry {
            id: Uuid::new_v4(),
            name: "my-server".to_string(),
            host: "example.com".to_string(),
            port: 22,
            user: None,
            identity_file: None,
            forwards: vec![],
            auto_restart: true,
        });
        assert_eq!(ssh.name(), "my-server");
        assert!(ssh.auto_restart());

        let k8s = TunnelEntry::K8s(K8sEntry {
            id: Uuid::new_v4(),
            name: "k8s-entry".to_string(),
            kubeconfig: None,
            context: None,
            namespace: None,
            resource_type: K8sResourceType::Pod,
            resource_name: "my-pod".to_string(),
            forwards: vec![],
            auto_restart: false,
        });
        assert_eq!(k8s.name(), "k8s-entry");
        assert!(!k8s.auto_restart());
    }

    #[test]
    fn test_k8s_entry_with_kubeconfig_round_trip() {
        let config = Config {
            entries: vec![TunnelEntry::K8s(K8sEntry {
                id: Uuid::new_v4(),
                name: "alt-config-test".to_string(),
                kubeconfig: Some("~/.kube/alt-config".to_string()),
                context: Some("staging".to_string()),
                namespace: Some("default".to_string()),
                resource_type: K8sResourceType::Deployment,
                resource_name: "web".to_string(),
                forwards: vec![K8sPortForward {
                    local_bind_address: "127.0.0.1".to_string(),
                    local_port: 8080,
                    remote_port: 80,
                }],
                auto_restart: false,
            })],
        };

        let serialized = toml::to_string_pretty(&config).unwrap();
        assert!(serialized.contains("kubeconfig"));
        let deserialized: Config = toml::from_str(&serialized).unwrap();
        assert_eq!(config, deserialized);

        let TunnelEntry::K8s(entry) = &deserialized.entries[0] else {
            panic!("expected K8s entry");
        };
        assert_eq!(
            entry.kubeconfig.as_deref(),
            Some("~/.kube/alt-config")
        );
    }

    #[test]
    fn test_k8s_entry_without_kubeconfig_omits_field() {
        let config = Config {
            entries: vec![TunnelEntry::K8s(K8sEntry {
                id: Uuid::new_v4(),
                name: "no-kc".to_string(),
                kubeconfig: None,
                context: None,
                namespace: None,
                resource_type: K8sResourceType::Pod,
                resource_name: "my-pod".to_string(),
                forwards: vec![],
                auto_restart: false,
            })],
        };

        let serialized = toml::to_string_pretty(&config).unwrap();
        assert!(
            !serialized.contains("kubeconfig"),
            "kubeconfig should be omitted when None"
        );
        let deserialized: Config = toml::from_str(&serialized).unwrap();
        assert_eq!(config, deserialized);
    }

    #[test]
    fn test_k8s_resource_type_as_str() {
        assert_eq!(K8sResourceType::Pod.as_str(), "pod");
        assert_eq!(K8sResourceType::Service.as_str(), "service");
        assert_eq!(K8sResourceType::Deployment.as_str(), "deployment");
    }

    // ── SshuttleEntry tests ──────────────────────────────────────────────

    #[test]
    fn test_sshuttle_entry_minimal_defaults() {
        let toml_str = r#"
            [[entries]]
            type = "sshuttle"
            name = "vpn"
            host = "bastion.example.com"
            subnets = ["10.0.0.0/8"]
        "#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.entries.len(), 1);
        let TunnelEntry::Sshuttle(entry) = &config.entries[0] else {
            panic!("expected Sshuttle entry");
        };
        assert_eq!(entry.name, "vpn");
        assert_eq!(entry.host, "bastion.example.com");
        assert_eq!(entry.subnets, vec!["10.0.0.0/8"]);
        assert!(entry.port.is_none());
        assert!(entry.user.is_none());
        assert!(entry.identity_file.is_none());
        assert!(!entry.auto_restart);
    }

    #[test]
    fn test_sshuttle_entry_full_round_trip() {
        let config = Config {
            entries: vec![TunnelEntry::Sshuttle(SshuttleEntry {
                id: Uuid::new_v4(),
                name: "corp-vpn".to_string(),
                host: "bastion.example.com".to_string(),
                port: Some(2222),
                user: Some("alice".to_string()),
                identity_file: Some("~/.ssh/id_ed25519".to_string()),
                subnets: vec!["10.0.0.0/8".to_string(), "192.168.0.0/16".to_string()],
                auto_restart: true,
            })],
        };
        let serialized = toml::to_string_pretty(&config).unwrap();
        let deserialized: Config = toml::from_str(&serialized).unwrap();
        assert_eq!(config, deserialized);
    }

    #[test]
    fn test_mixed_ssh_k8s_sshuttle_round_trip() {
        let config = Config {
            entries: vec![
                TunnelEntry::Ssh(ServerEntry {
                    id: Uuid::new_v4(),
                    name: "bastion".to_string(),
                    host: "bastion.example.com".to_string(),
                    port: 22,
                    user: None,
                    identity_file: None,
                    forwards: vec![],
                    auto_restart: false,
                }),
                TunnelEntry::K8s(K8sEntry {
                    id: Uuid::new_v4(),
                    name: "api-debug".to_string(),
                    kubeconfig: None,
                    context: None,
                    namespace: None,
                    resource_type: K8sResourceType::Deployment,
                    resource_name: "api".to_string(),
                    forwards: vec![],
                    auto_restart: false,
                }),
                TunnelEntry::Sshuttle(SshuttleEntry {
                    id: Uuid::new_v4(),
                    name: "corp-vpn".to_string(),
                    host: "vpn.example.com".to_string(),
                    port: None,
                    user: None,
                    identity_file: None,
                    subnets: vec!["10.0.0.0/8".to_string()],
                    auto_restart: false,
                }),
            ],
        };
        let serialized = toml::to_string_pretty(&config).unwrap();
        let deserialized: Config = toml::from_str(&serialized).unwrap();
        assert_eq!(config, deserialized);
        assert!(matches!(deserialized.entries[0], TunnelEntry::Ssh(_)));
        assert!(matches!(deserialized.entries[1], TunnelEntry::K8s(_)));
        assert!(matches!(deserialized.entries[2], TunnelEntry::Sshuttle(_)));
    }

    #[test]
    fn test_tunnel_entry_helpers_sshuttle() {
        let entry = TunnelEntry::Sshuttle(SshuttleEntry {
            id: Uuid::new_v4(),
            name: "my-vpn".to_string(),
            host: "vpn.example.com".to_string(),
            port: None,
            user: None,
            identity_file: None,
            subnets: vec!["10.0.0.0/8".to_string()],
            auto_restart: true,
        });
        assert_eq!(entry.name(), "my-vpn");
        assert!(entry.auto_restart());
    }
}
