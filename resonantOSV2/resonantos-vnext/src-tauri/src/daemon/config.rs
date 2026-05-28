// Headless node daemon configuration (TOML-based).

use std::path::PathBuf;

/// Full daemon configuration.
#[derive(Debug, Clone)]
pub struct NodeConfig {
    pub network: NetworkConfig,
    pub hardware: HardwareConfig,
    pub models: ModelsConfig,
    pub daemon: DaemonConfig,
    pub low_power: LowPowerConfig,
}

#[derive(Debug, Clone)]
pub struct NetworkConfig {
    pub listen_port: u16,
    pub peers: Vec<String>,
    pub transport: TransportMode,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TransportMode {
    Lan,
    Wireguard,
    Both,
}

#[derive(Debug, Clone)]
pub struct HardwareConfig {
    pub max_memory_mb: u64,
    pub max_vram_mb: u64,
    pub gpu_layers: GpuLayerMode,
}

#[derive(Debug, Clone, PartialEq)]
pub enum GpuLayerMode {
    Auto,
    None,
    Max,
    Fixed(u32),
}

#[derive(Debug, Clone)]
pub struct ModelsConfig {
    pub directory: PathBuf,
    pub max_loaded: usize,
    pub auto_download: bool,
}

#[derive(Debug, Clone)]
pub struct DaemonConfig {
    pub log_file: PathBuf,
    pub log_level: String,
    pub api_port: u16,
    pub low_power: bool,
}

#[derive(Debug, Clone)]
pub struct LowPowerConfig {
    pub max_models: usize,
    pub battery_pause_threshold: u8,
    pub reduce_heartbeat: bool,
}

impl Default for NodeConfig {
    fn default() -> Self {
        let home = dirs_home();
        Self {
            network: NetworkConfig {
                listen_port: 9741,
                peers: Vec::new(),
                transport: TransportMode::Lan,
            },
            hardware: HardwareConfig {
                max_memory_mb: 0,
                max_vram_mb: 0,
                gpu_layers: GpuLayerMode::Auto,
            },
            models: ModelsConfig {
                directory: home.join("models"),
                max_loaded: 2,
                auto_download: true,
            },
            daemon: DaemonConfig {
                log_file: home.join("logs").join("node.log"),
                log_level: "info".to_string(),
                api_port: 9742,
                low_power: false,
            },
            low_power: LowPowerConfig {
                max_models: 1,
                battery_pause_threshold: 20,
                reduce_heartbeat: true,
            },
        }
    }
}

impl NodeConfig {
    /// Load from TOML file, falling back to defaults for missing fields.
    pub fn load(path: &std::path::Path) -> Self {
        if !path.exists() {
            return Self::default();
        }
        // In production: parse TOML with toml crate
        // For now: return defaults (TOML parsing added when toml crate is available)
        Self::default()
    }

    /// Apply CLI overrides.
    pub fn apply_overrides(&mut self, overrides: &CliOverrides) {
        if let Some(port) = overrides.port {
            self.network.listen_port = port;
        }
        if !overrides.peers.is_empty() {
            self.network.peers = overrides.peers.clone();
        }
        if let Some(ref dir) = overrides.models_dir {
            self.models.directory = dir.clone();
        }
        if overrides.low_power {
            self.daemon.low_power = true;
        }
        if let Some(ref level) = overrides.log_level {
            self.daemon.log_level = level.clone();
        }
    }

    /// Get effective heartbeat interval based on low-power mode.
    pub fn heartbeat_interval_secs(&self) -> u64 {
        if self.daemon.low_power && self.low_power.reduce_heartbeat {
            300
        } else {
            60
        }
    }

    /// Get effective max models based on low-power mode.
    pub fn effective_max_models(&self) -> usize {
        if self.daemon.low_power {
            self.low_power.max_models
        } else {
            self.models.max_loaded
        }
    }
}

/// CLI argument overrides.
#[derive(Debug, Clone, Default)]
pub struct CliOverrides {
    pub port: Option<u16>,
    pub peers: Vec<String>,
    pub models_dir: Option<PathBuf>,
    pub low_power: bool,
    pub log_level: Option<String>,
    pub config_path: Option<PathBuf>,
    pub daemon_mode: bool,
    pub status_query: bool,
    pub shutdown_request: bool,
}

fn dirs_home() -> PathBuf {
    dirs_resonantos()
}

fn dirs_resonantos() -> PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    PathBuf::from(home).join(".resonantos")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = NodeConfig::default();
        assert_eq!(config.network.listen_port, 9741);
        assert_eq!(config.daemon.api_port, 9742);
        assert_eq!(config.models.max_loaded, 2);
        assert!(!config.daemon.low_power);
    }

    #[test]
    fn test_heartbeat_interval_normal() {
        let config = NodeConfig::default();
        assert_eq!(config.heartbeat_interval_secs(), 60);
    }

    #[test]
    fn test_heartbeat_interval_low_power() {
        let mut config = NodeConfig::default();
        config.daemon.low_power = true;
        assert_eq!(config.heartbeat_interval_secs(), 300);
    }

    #[test]
    fn test_effective_max_models_low_power() {
        let mut config = NodeConfig::default();
        config.daemon.low_power = true;
        assert_eq!(config.effective_max_models(), 1);
    }

    #[test]
    fn test_apply_overrides() {
        let mut config = NodeConfig::default();
        let overrides = CliOverrides {
            port: Some(8888),
            low_power: true,
            ..Default::default()
        };
        config.apply_overrides(&overrides);
        assert_eq!(config.network.listen_port, 8888);
        assert!(config.daemon.low_power);
    }

    #[test]
    fn test_load_missing_file_returns_defaults() {
        let config = NodeConfig::load(std::path::Path::new("/nonexistent/node.toml"));
        assert_eq!(config.network.listen_port, 9741);
    }
}
