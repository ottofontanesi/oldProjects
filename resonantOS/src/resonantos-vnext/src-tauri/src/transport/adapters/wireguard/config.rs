// WireGuard adapter configuration.

/// Configuration for the WireGuard transport adapter.
#[derive(Debug, Clone)]
pub struct WireGuardConfig {
    /// UDP listen port for WireGuard traffic.
    pub listen_port: u16,
    /// Keepalive interval in seconds.
    pub keepalive_interval_secs: u64,
    /// Handshake timeout in seconds.
    pub handshake_timeout_secs: u64,
    /// Maximum concurrent tunnels.
    pub max_tunnels: usize,
    /// Peer timeout (seconds without data → Offline).
    pub peer_timeout_secs: u64,
    /// Suspect timeout (seconds without data → Suspect).
    pub suspect_timeout_secs: u64,
    /// MTU for WireGuard packets.
    pub mtu: u16,
    /// Maximum message size in bytes.
    pub max_message_size: usize,
    /// Number of handshake retries before giving up.
    pub handshake_retries: u32,
    /// WireGuard interface name (for display).
    pub interface_name: String,
}

impl Default for WireGuardConfig {
    fn default() -> Self {
        Self {
            listen_port: 51820,
            keepalive_interval_secs: 25,
            handshake_timeout_secs: 5,
            max_tunnels: 20,
            peer_timeout_secs: 120,
            suspect_timeout_secs: 60,
            mtu: 1420,
            max_message_size: 64 * 1024 * 1024, // 64MB
            handshake_retries: 3,
            interface_name: "wg0".to_string(),
        }
    }
}

impl WireGuardConfig {
    /// Validate configuration values.
    pub fn validate(&self) -> Result<(), String> {
        if self.listen_port == 0 {
            return Err("listen_port must be > 0".to_string());
        }
        if self.max_tunnels == 0 {
            return Err("max_tunnels must be > 0".to_string());
        }
        if self.suspect_timeout_secs >= self.peer_timeout_secs {
            return Err("suspect_timeout must be < peer_timeout".to_string());
        }
        if self.mtu < 1280 {
            return Err("MTU must be >= 1280 (IPv6 minimum)".to_string());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_valid() {
        let config = WireGuardConfig::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_invalid_port() {
        let mut config = WireGuardConfig::default();
        config.listen_port = 0;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_invalid_timeout_order() {
        let mut config = WireGuardConfig::default();
        config.suspect_timeout_secs = 200;
        config.peer_timeout_secs = 100;
        assert!(config.validate().is_err());
    }
}
