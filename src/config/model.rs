//! Data model for tunnel configuration.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Top-level configuration containing all server entries.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct Config {
    /// List of configured SSH server entries.
    #[serde(default)]
    pub server: Vec<ServerEntry>,
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
    #[serde(default = "default_port")]
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
fn default_port() -> u16 {
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

    #[test]
    fn test_minimal_server_entry_defaults() {
        let toml_str = r#"
            [[server]]
            name = "test"
            host = "example.com"
        "#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.server.len(), 1);
        let entry = &config.server[0];
        assert_eq!(entry.port, 22);
        assert!(entry.user.is_none());
        assert!(entry.identity_file.is_none());
        assert!(entry.forwards.is_empty());
    }

    #[test]
    fn test_round_trip_serialization() {
        let config = Config {
            server: vec![ServerEntry {
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
            }],
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
            server: vec![ServerEntry {
                id: Uuid::new_v4(),
                name: "restart-test".to_string(),
                host: "example.com".to_string(),
                port: 22,
                user: None,
                identity_file: None,
                forwards: vec![],
                auto_restart: true,
            }],
        };
        let serialized = toml::to_string_pretty(&config).unwrap();
        assert!(serialized.contains("auto_restart = true"));
        let deserialized: Config = toml::from_str(&serialized).unwrap();
        assert_eq!(config, deserialized);
        assert!(deserialized.server[0].auto_restart);
    }

    #[test]
    fn test_auto_restart_defaults_false() {
        let toml_str = r#"
            [[server]]
            name = "no-restart"
            host = "example.com"
        "#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert!(!config.server[0].auto_restart);
    }
}
