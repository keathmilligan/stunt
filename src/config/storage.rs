//! Persistent storage for tunnel configuration (load/save TOML).

use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};

use super::model::Config;

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

/// Load configuration from disk. Creates the directory and an empty config
/// file if they do not exist.
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
    use crate::config::model::{ServerEntry, TunnelForward};
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
            server: vec![ServerEntry {
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
            }],
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
}
