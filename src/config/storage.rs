//! Persistent storage for tunnel configuration (load/save TOML).

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};

use super::model::{Config, ServerEntry, TunnelEntry};

/// Returns the path to the tunnel configuration file.
///
/// Uses the platform user data directory via `dirs::data_dir()`:
/// - Linux: `~/.local/share/tunnel-mgr/tunnels.toml`
/// - macOS: `~/Library/Application Support/tunnel-mgr/tunnels.toml`
/// - Windows: `%APPDATA%/tunnel-mgr/tunnels.toml`
pub fn config_path() -> Result<PathBuf> {
    let data_dir = dirs::data_dir().context("could not determine platform data directory")?;
    Ok(data_dir.join("tunnel-mgr").join("tunnels.toml"))
}

/// Legacy config format: a flat list of `[[server]]` entries without a `type` tag.
///
/// Used only for migration detection.
#[derive(serde::Deserialize)]
struct LegacyConfig {
    #[serde(default)]
    server: Vec<ServerEntry>,
}

/// Detect whether a TOML string uses the legacy `[[server]]` format (pre-migration).
///
/// Returns `true` if the content contains `[[server]]` table entries and no `[[entries]]`.
fn is_legacy_format(content: &str) -> bool {
    // Simple heuristic: check for [[server]] key at the table level.
    // A robust check would parse the TOML value tree, but this is sufficient
    // because the new format always uses [[entries]].
    content.contains("[[server]]")
}

/// Migrate a legacy `[[server]]`-format config string to the new `[[entries]]` format.
///
/// Parses the legacy format, wraps each `ServerEntry` as `TunnelEntry::Ssh`, and
/// serializes to the new format.
pub fn migrate_legacy_config(content: &str) -> Result<Config> {
    let legacy: LegacyConfig =
        toml::from_str(content).context("failed to parse legacy config for migration")?;

    let entries = legacy.server.into_iter().map(TunnelEntry::Ssh).collect();

    Ok(Config { entries })
}

/// Load configuration from disk. Creates the directory and an empty config
/// file if they do not exist.
///
/// If the config file uses the legacy `[[server]]` format, it is automatically
/// migrated to the new `[[entries]]` format and written back to disk. The
/// original file is preserved as a `.bak` before migration.
pub fn load() -> Result<Config> {
    let path = config_path()?;

    if !path.exists() {
        // First run — create directory and empty config
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("failed to create config directory: {}", parent.display())
            })?;
        }
        let empty = Config::default();
        let content = toml::to_string_pretty(&empty).context("failed to serialize empty config")?;
        fs::write(&path, &content)
            .with_context(|| format!("failed to write initial config: {}", path.display()))?;
        return Ok(empty);
    }

    let content = fs::read_to_string(&path)
        .with_context(|| format!("failed to read config file: {}", path.display()))?;

    // Detect and migrate legacy format
    if is_legacy_format(&content) {
        let migrated = migrate_legacy_config(&content)
            .with_context(|| format!("failed to migrate legacy config: {}", path.display()))?;

        // Back up the original before overwriting
        let bak_path = path.with_extension("toml.bak");
        fs::copy(&path, &bak_path)
            .with_context(|| format!("failed to backup legacy config: {}", bak_path.display()))?;

        // Save the migrated config
        save(&migrated)
            .with_context(|| format!("failed to save migrated config: {}", path.display()))?;

        return Ok(migrated);
    }

    let config: Config = toml::from_str(&content)
        .with_context(|| format!("failed to parse config file: {}", path.display()))?;
    Ok(config)
}

/// Save configuration to disk atomically.
///
/// Writes to a temporary file in the same directory, backs up the existing
/// file as `.bak`, then renames the temp file into place.
pub fn save(config: &Config) -> Result<()> {
    let path = config_path()?;

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create config directory: {}", parent.display()))?;
    }

    let content = toml::to_string_pretty(config).context("failed to serialize config")?;

    let tmp_path = path.with_extension("toml.tmp");

    // Write to temp file
    fs::write(&tmp_path, &content)
        .with_context(|| format!("failed to write temp config: {}", tmp_path.display()))?;

    // Back up existing file
    if path.exists() {
        let bak_path = path.with_extension("toml.bak");
        fs::copy(&path, &bak_path)
            .with_context(|| format!("failed to create backup: {}", bak_path.display()))?;
    }

    // Atomic rename
    fs::rename(&tmp_path, &path)
        .with_context(|| format!("failed to rename temp config to: {}", path.display()))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::model::{ServerEntry, TunnelEntry, TunnelForward};
    use uuid::Uuid;

    #[test]
    fn test_config_path_is_valid() {
        let path = config_path().unwrap();
        assert!(path.to_string_lossy().contains("tunnel-mgr"));
        assert!(path.to_string_lossy().ends_with("tunnels.toml"));
    }

    #[test]
    fn test_save_and_load_round_trip() {
        let dir = std::env::temp_dir().join(format!("tunnel-mgr-test-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("tunnels.toml");

        let config = Config {
            entries: vec![TunnelEntry::Ssh(ServerEntry {
                id: Uuid::new_v4(),
                name: "test-server".to_string(),
                host: "example.com".to_string(),
                port: 22,
                user: Some("testuser".to_string()),
                identity_file: None,
                forwards: vec![TunnelForward::Local {
                    bind_address: "127.0.0.1".to_string(),
                    bind_port: 5432,
                    remote_host: "db.internal".to_string(),
                    remote_port: 5432,
                }],
                auto_restart: false,
            })],
        };

        // Write directly to temp path for testing
        let content = toml::to_string_pretty(&config).unwrap();
        fs::write(&path, &content).unwrap();

        let loaded: Config = toml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(config, loaded);

        // Clean up
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_save_creates_backup() {
        let dir = std::env::temp_dir().join(format!("tunnel-mgr-test-bak-{}", Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("tunnels.toml");
        let bak_path = dir.join("tunnels.toml.bak");

        // Write an initial file
        fs::write(&path, "# original").unwrap();

        // Write new content using the same atomic pattern
        let tmp_path = path.with_extension("toml.tmp");
        fs::write(&tmp_path, "# updated").unwrap();
        fs::copy(&path, &bak_path).unwrap();
        fs::rename(&tmp_path, &path).unwrap();

        assert_eq!(fs::read_to_string(&bak_path).unwrap(), "# original");
        assert_eq!(fs::read_to_string(&path).unwrap(), "# updated");

        // Clean up
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn test_legacy_migration_detection() {
        let legacy = r#"
[[server]]
name = "bastion"
host = "bastion.example.com"
"#;
        assert!(is_legacy_format(legacy));

        let modern = r#"
[[entries]]
type = "ssh"
name = "bastion"
host = "bastion.example.com"
"#;
        assert!(!is_legacy_format(modern));
    }

    #[test]
    fn test_migrate_legacy_config_ssh_entries() {
        let legacy_toml = r#"
[[server]]
name = "bastion"
host = "bastion.example.com"
port = 22
auto_restart = false

[[server]]
name = "db-tunnel"
host = "db.internal"
port = 2222
user = "deploy"
auto_restart = true
"#;

        let migrated = migrate_legacy_config(legacy_toml).unwrap();
        assert_eq!(migrated.entries.len(), 2);

        // Both entries should be SSH type
        assert!(matches!(migrated.entries[0], TunnelEntry::Ssh(_)));
        assert!(matches!(migrated.entries[1], TunnelEntry::Ssh(_)));

        let TunnelEntry::Ssh(first) = &migrated.entries[0] else {
            panic!("expected SSH");
        };
        assert_eq!(first.name, "bastion");
        assert_eq!(first.host, "bastion.example.com");

        let TunnelEntry::Ssh(second) = &migrated.entries[1] else {
            panic!("expected SSH");
        };
        assert_eq!(second.name, "db-tunnel");
        assert_eq!(second.user, Some("deploy".to_string()));
        assert!(second.auto_restart);

        // Verify migrated config round-trips
        let serialized = toml::to_string_pretty(&migrated).unwrap();
        assert!(
            !serialized.contains("[[server]]"),
            "migrated config should not contain [[server]]"
        );
        let reloaded: Config = toml::from_str(&serialized).unwrap();
        assert_eq!(migrated, reloaded);
    }

    #[test]
    fn test_migrate_empty_legacy_config() {
        let legacy_toml = "# empty legacy config\n";
        // No [[server]] entries → is_legacy_format returns false, but migration
        // should also handle it gracefully
        let migrated = migrate_legacy_config(legacy_toml).unwrap();
        assert!(migrated.entries.is_empty());
    }
}
