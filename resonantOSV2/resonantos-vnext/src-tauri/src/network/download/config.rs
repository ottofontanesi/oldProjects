// Intent citation: .kiro/specs/model-download-engine/design.md — DownloadConfig
// Configuration for the download engine with sensible defaults.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Configuration for the DownloadManager.
/// All fields have sensible defaults for typical desktop usage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadConfig {
    /// Maximum number of concurrent active downloads (default: 3).
    pub max_concurrent: u32,
    /// Global bandwidth limit in bytes per second. None = unlimited.
    pub bandwidth_limit_bps: Option<u64>,
    /// Maximum retry attempts for transient errors (default: 4).
    pub max_retries: u32,
    /// Base delay for exponential backoff in milliseconds (default: 1000).
    pub retry_backoff_base_ms: u64,
    /// Directory for temporary .part files during download.
    pub temp_dir: PathBuf,
    /// Final directory for completed model files.
    pub model_dir: PathBuf,
    /// Minimum free disk space to maintain in MB (default: 1024 = 1GB).
    pub min_disk_space_mb: u64,
    /// Interval between progress events in milliseconds (default: 500).
    pub progress_interval_ms: u64,
    /// HTTP connection timeout in seconds (default: 30).
    pub connect_timeout_secs: u64,
    /// Maximum HTTP redirects to follow (default: 5).
    pub max_redirects: u32,
}

impl Default for DownloadConfig {
    fn default() -> Self {
        Self {
            max_concurrent: 3,
            bandwidth_limit_bps: None,
            max_retries: 4,
            retry_backoff_base_ms: 1000,
            temp_dir: default_temp_dir(),
            model_dir: default_model_dir(),
            min_disk_space_mb: 1024,
            progress_interval_ms: 500,
            connect_timeout_secs: 30,
            max_redirects: 5,
        }
    }
}

/// Default temp directory for partial downloads.
fn default_temp_dir() -> PathBuf {
    dirs_fallback("downloads")
}

/// Default directory for completed model files.
fn default_model_dir() -> PathBuf {
    dirs_fallback("models")
}

/// Get a subdirectory under the app data folder, with fallback to current dir.
fn dirs_fallback(subdir: &str) -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        if let Ok(appdata) = std::env::var("APPDATA") {
            return PathBuf::from(appdata)
                .join("resonantos-vnext")
                .join(subdir);
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home)
                .join(".local")
                .join("share")
                .join("resonantos-vnext")
                .join(subdir);
        }
    }
    PathBuf::from(".").join("resonantos-vnext").join(subdir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = DownloadConfig::default();
        assert_eq!(config.max_concurrent, 3);
        assert_eq!(config.bandwidth_limit_bps, None);
        assert_eq!(config.max_retries, 4);
        assert_eq!(config.retry_backoff_base_ms, 1000);
        assert_eq!(config.min_disk_space_mb, 1024);
        assert_eq!(config.progress_interval_ms, 500);
        assert_eq!(config.connect_timeout_secs, 30);
        assert_eq!(config.max_redirects, 5);
    }

    #[test]
    fn test_config_serialization() {
        let config = DownloadConfig::default();
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: DownloadConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.max_concurrent, config.max_concurrent);
        assert_eq!(deserialized.bandwidth_limit_bps, config.bandwidth_limit_bps);
    }

    #[test]
    fn test_custom_config() {
        let config = DownloadConfig {
            max_concurrent: 5,
            bandwidth_limit_bps: Some(10_000_000),
            max_retries: 2,
            retry_backoff_base_ms: 500,
            temp_dir: PathBuf::from("/custom/temp"),
            model_dir: PathBuf::from("/custom/models"),
            min_disk_space_mb: 2048,
            progress_interval_ms: 1000,
            connect_timeout_secs: 60,
            max_redirects: 3,
        };
        assert_eq!(config.max_concurrent, 5);
        assert_eq!(config.bandwidth_limit_bps, Some(10_000_000));
    }
}
