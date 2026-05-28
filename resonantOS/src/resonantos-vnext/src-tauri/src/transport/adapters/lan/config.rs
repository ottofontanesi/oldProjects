// Intent citation: .kiro/specs/lan-transport-adapter/design.md — Configuration
// LAN adapter configuration with all tunable parameters.

use std::time::Duration;

/// Configuration for the LAN transport adapter.
///
/// All fields have sensible defaults for a typical gigabit LAN environment.
#[derive(Debug, Clone)]
pub struct LanAdapterConfig {
    /// TCP listener port (default: 9741).
    pub listen_port: u16,

    /// mDNS service type for browsing/advertising (default: "_resonantos._tcp.local").
    pub mdns_service_type: String,

    /// Interval between heartbeat pings to each connected peer (default: 10s).
    pub heartbeat_interval: Duration,

    /// Number of consecutive missed heartbeats before marking a peer offline (default: 3).
    pub heartbeat_timeout_count: u8,

    /// Timeout for establishing a new TCP connection (default: 2s).
    pub connect_timeout: Duration,

    /// Maximum allowed frame size in bytes (default: 64MB).
    pub max_message_size: u64,

    /// Duration of inactivity before sending a keepalive ping (default: 60s).
    pub idle_keepalive: Duration,

    /// Duration after which an unreachable peer's connection is cleaned up (default: 5min).
    pub stale_peer_timeout: Duration,

    /// Maximum number of retry attempts for connections and mDNS registration (default: 3).
    pub max_retry_attempts: u8,

    /// Timeout for reading a complete frame from a TCP stream (default: 10s).
    pub frame_read_timeout: Duration,

    /// Base duration for mDNS registration retry exponential backoff (default: 1s).
    /// Retries: 1s, 2s, 4s.
    pub mdns_retry_backoff_base: Duration,

    /// Base duration for connection retry exponential backoff (default: 100ms).
    /// Retries: 100ms, 200ms, 400ms.
    pub connect_retry_backoff_base: Duration,
}

impl Default for LanAdapterConfig {
    fn default() -> Self {
        Self {
            listen_port: 9741,
            mdns_service_type: "_resonantos._tcp.local".to_string(),
            heartbeat_interval: Duration::from_secs(10),
            heartbeat_timeout_count: 3,
            connect_timeout: Duration::from_secs(2),
            max_message_size: 64 * 1024 * 1024, // 64MB
            idle_keepalive: Duration::from_secs(60),
            stale_peer_timeout: Duration::from_secs(5 * 60), // 5 minutes
            max_retry_attempts: 3,
            frame_read_timeout: Duration::from_secs(10),
            mdns_retry_backoff_base: Duration::from_secs(1),
            connect_retry_backoff_base: Duration::from_millis(100),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_values() {
        let config = LanAdapterConfig::default();

        assert_eq!(config.listen_port, 9741);
        assert_eq!(config.mdns_service_type, "_resonantos._tcp.local");
        assert_eq!(config.heartbeat_interval, Duration::from_secs(10));
        assert_eq!(config.heartbeat_timeout_count, 3);
        assert_eq!(config.connect_timeout, Duration::from_secs(2));
        assert_eq!(config.max_message_size, 64 * 1024 * 1024);
        assert_eq!(config.idle_keepalive, Duration::from_secs(60));
        assert_eq!(config.stale_peer_timeout, Duration::from_secs(300));
        assert_eq!(config.max_retry_attempts, 3);
        assert_eq!(config.frame_read_timeout, Duration::from_secs(10));
        assert_eq!(config.mdns_retry_backoff_base, Duration::from_secs(1));
        assert_eq!(config.connect_retry_backoff_base, Duration::from_millis(100));
    }

    #[test]
    fn test_config_custom_values() {
        let config = LanAdapterConfig {
            listen_port: 8080,
            heartbeat_interval: Duration::from_secs(5),
            max_retry_attempts: 5,
            ..Default::default()
        };

        assert_eq!(config.listen_port, 8080);
        assert_eq!(config.heartbeat_interval, Duration::from_secs(5));
        assert_eq!(config.max_retry_attempts, 5);
        // Other fields remain default
        assert_eq!(config.mdns_service_type, "_resonantos._tcp.local");
    }
}
