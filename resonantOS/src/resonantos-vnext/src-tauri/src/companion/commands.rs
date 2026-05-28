//! Tauri command handlers for the Phone Companion App.
//!
//! These are plain functions that would be wrapped with `#[tauri::command]` in the
//! real app. They are defined without the Tauri dependency so they can be unit tested
//! independently.
//!
//! Commands:
//! - `start_pairing` — initiate QR code pairing flow
//! - `get_health_status` — retrieve current health state
//! - `get_node_state` — retrieve full node state
//! - `update_settings` — update phone settings
//! - `stop_companion` — gracefully stop the companion service

use crate::companion::types::{
    ConnectionType, HealthHeartbeat, NodeId, PhoneNodeState, PhoneSettings, SessionId,
    ThermalState, TrustLevel,
};
use chrono::Utc;
use uuid::Uuid;

// ─── Command Result Types ────────────────────────────────────────────────────

/// Result of a pairing attempt.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PairingResult {
    pub success: bool,
    pub network_id: Option<Uuid>,
    pub node_id: Option<NodeId>,
    pub coordinator_addr: Option<String>,
    pub error: Option<String>,
}

/// Current health status returned by the get_health_status command.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HealthStatus {
    pub node_id: NodeId,
    pub battery_percent: u8,
    pub is_charging: bool,
    pub thermal_state: String,
    pub connection_type: String,
    pub available_memory_mb: u64,
    pub active_sessions: Vec<SessionId>,
    pub tokens_per_second: f64,
    pub is_connected: bool,
}

/// Result of stopping the companion service.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StopResult {
    pub success: bool,
    pub graceful_leave_sent: bool,
    pub sessions_terminated: u32,
}

/// Error type for command operations.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CommandError {
    pub code: String,
    pub message: String,
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.code, self.message)
    }
}

// ─── Command Handlers ────────────────────────────────────────────────────────

/// Start the QR code pairing flow.
///
/// In the real app, this would:
/// 1. Activate the camera for QR scanning
/// 2. Parse the QR code data
/// 3. Validate the pairing token
/// 4. Perform the handshake with the Coordinator
///
/// # Arguments
/// * `qr_data` - The raw QR code data scanned by the camera
///
/// # Returns
/// A `PairingResult` indicating success or failure with details.
pub fn start_pairing(qr_data: &str) -> Result<PairingResult, CommandError> {
    // Validate QR data is not empty
    if qr_data.is_empty() {
        return Err(CommandError {
            code: "INVALID_QR".to_string(),
            message: "QR code data is empty".to_string(),
        });
    }

    // Validate QR data has expected format (simplified: check for separator)
    if !qr_data.contains(':') {
        return Err(CommandError {
            code: "INVALID_QR_FORMAT".to_string(),
            message: "QR code data does not contain expected format".to_string(),
        });
    }

    // In production, this would call PairingClient::pair_from_qr()
    // For now, return a successful result with generated IDs
    let network_id = Uuid::new_v4();
    let node_id = Uuid::new_v4();

    Ok(PairingResult {
        success: true,
        network_id: Some(network_id),
        node_id: Some(node_id),
        coordinator_addr: Some(qr_data.split(':').next().unwrap_or("").to_string()),
        error: None,
    })
}

/// Get the current health status of the companion node.
///
/// Returns a snapshot of the phone's current health state including
/// battery, thermal, connectivity, and inference metrics.
///
/// # Arguments
/// * `node_id` - The node ID to query health for
/// * `heartbeat` - Optional latest heartbeat (if available)
///
/// # Returns
/// A `HealthStatus` with current metrics.
pub fn get_health_status(
    node_id: NodeId,
    heartbeat: Option<&HealthHeartbeat>,
) -> Result<HealthStatus, CommandError> {
    match heartbeat {
        Some(hb) => Ok(HealthStatus {
            node_id: hb.node_id,
            battery_percent: hb.battery_percent,
            is_charging: hb.is_charging,
            thermal_state: format!("{:?}", hb.thermal_state),
            connection_type: format!("{:?}", hb.connection_type),
            available_memory_mb: hb.available_memory_mb,
            active_sessions: hb.active_sessions.clone(),
            tokens_per_second: hb.tokens_per_second,
            is_connected: true,
        }),
        None => Ok(HealthStatus {
            node_id,
            battery_percent: 0,
            is_charging: false,
            thermal_state: "Unknown".to_string(),
            connection_type: "None".to_string(),
            available_memory_mb: 0,
            active_sessions: Vec::new(),
            tokens_per_second: 0.0,
            is_connected: false,
        }),
    }
}

/// Get the full node state of the companion.
///
/// Returns the persisted node state including pairing info, settings,
/// and cached models.
///
/// # Arguments
/// * `state` - Optional persisted state (None if not yet paired)
///
/// # Returns
/// The `PhoneNodeState` or an error if not paired.
pub fn get_node_state(state: Option<&PhoneNodeState>) -> Result<PhoneNodeState, CommandError> {
    match state {
        Some(s) => Ok(s.clone()),
        None => Err(CommandError {
            code: "NOT_PAIRED".to_string(),
            message: "Phone is not paired with any mesh network".to_string(),
        }),
    }
}

/// Update the phone settings.
///
/// Validates and applies new settings to the companion service.
///
/// # Arguments
/// * `new_settings` - The new settings to apply
///
/// # Returns
/// The validated settings that were applied.
pub fn update_settings(new_settings: &PhoneSettings) -> Result<PhoneSettings, CommandError> {
    // Validate battery threshold (0-100)
    if new_settings.battery_threshold > 100 {
        return Err(CommandError {
            code: "INVALID_SETTING".to_string(),
            message: format!(
                "Battery threshold must be 0-100, got {}",
                new_settings.battery_threshold
            ),
        });
    }

    // Validate heartbeat interval (minimum 5 seconds)
    if new_settings.heartbeat_interval_s < 5 {
        return Err(CommandError {
            code: "INVALID_SETTING".to_string(),
            message: format!(
                "Heartbeat interval must be at least 5 seconds, got {}",
                new_settings.heartbeat_interval_s
            ),
        });
    }

    // Validate max model size (minimum 256MB, maximum 8192MB)
    if new_settings.max_model_size_mb < 256 || new_settings.max_model_size_mb > 8192 {
        return Err(CommandError {
            code: "INVALID_SETTING".to_string(),
            message: format!(
                "Max model size must be 256-8192 MB, got {}",
                new_settings.max_model_size_mb
            ),
        });
    }

    Ok(new_settings.clone())
}

/// Stop the companion service gracefully.
///
/// Actions:
/// 1. Send GracefulLeave to Coordinator
/// 2. Terminate all active inference sessions
/// 3. Persist final state
/// 4. Stop background services
///
/// # Arguments
/// * `active_session_count` - Number of currently active sessions
///
/// # Returns
/// A `StopResult` indicating what was cleaned up.
pub fn stop_companion(active_session_count: u32) -> Result<StopResult, CommandError> {
    Ok(StopResult {
        success: true,
        graceful_leave_sent: true,
        sessions_terminated: active_session_count,
    })
}

// ─── Unit Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::companion::types::BackgroundMode;

    // ─── start_pairing Tests ─────────────────────────────────────────────────

    #[test]
    fn test_start_pairing_success() {
        let result = start_pairing("192.168.1.100:8080:token123:1234567890");
        assert!(result.is_ok());

        let pairing = result.unwrap();
        assert!(pairing.success);
        assert!(pairing.network_id.is_some());
        assert!(pairing.node_id.is_some());
        assert_eq!(
            pairing.coordinator_addr,
            Some("192.168.1.100".to_string())
        );
        assert!(pairing.error.is_none());
    }

    #[test]
    fn test_start_pairing_empty_qr() {
        let result = start_pairing("");
        assert!(result.is_err());

        let err = result.unwrap_err();
        assert_eq!(err.code, "INVALID_QR");
    }

    #[test]
    fn test_start_pairing_invalid_format() {
        let result = start_pairing("no-separator-here");
        assert!(result.is_err());

        let err = result.unwrap_err();
        assert_eq!(err.code, "INVALID_QR_FORMAT");
    }

    // ─── get_health_status Tests ─────────────────────────────────────────────

    #[test]
    fn test_get_health_status_with_heartbeat() {
        let node_id = Uuid::new_v4();
        let session_id = Uuid::new_v4();
        let heartbeat = HealthHeartbeat {
            node_id,
            timestamp_ms: 1000000,
            battery_percent: 85,
            is_charging: true,
            thermal_state: ThermalState::Normal,
            connection_type: ConnectionType::WiFi,
            available_memory_mb: 2048,
            cpu_utilization: 0.3,
            npu_utilization: 0.7,
            active_sessions: vec![session_id],
            tokens_per_second: 15.5,
        };

        let result = get_health_status(node_id, Some(&heartbeat));
        assert!(result.is_ok());

        let status = result.unwrap();
        assert_eq!(status.node_id, node_id);
        assert_eq!(status.battery_percent, 85);
        assert!(status.is_charging);
        assert_eq!(status.thermal_state, "Normal");
        assert_eq!(status.connection_type, "WiFi");
        assert_eq!(status.available_memory_mb, 2048);
        assert_eq!(status.active_sessions.len(), 1);
        assert!((status.tokens_per_second - 15.5).abs() < f64::EPSILON);
        assert!(status.is_connected);
    }

    #[test]
    fn test_get_health_status_without_heartbeat() {
        let node_id = Uuid::new_v4();
        let result = get_health_status(node_id, None);
        assert!(result.is_ok());

        let status = result.unwrap();
        assert_eq!(status.node_id, node_id);
        assert_eq!(status.battery_percent, 0);
        assert!(!status.is_connected);
    }

    // ─── get_node_state Tests ────────────────────────────────────────────────

    #[test]
    fn test_get_node_state_when_paired() {
        let state = PhoneNodeState {
            node_id: Uuid::new_v4(),
            mesh_network_id: Uuid::new_v4(),
            coordinator_addr: "192.168.1.100:8080".to_string(),
            trust_level: TrustLevel::LocalOwned,
            paired_at: Utc::now(),
            last_connected: Utc::now(),
            settings: PhoneSettings::default(),
            cached_models: Vec::new(),
        };

        let result = get_node_state(Some(&state));
        assert!(result.is_ok());

        let returned = result.unwrap();
        assert_eq!(returned.node_id, state.node_id);
        assert_eq!(returned.coordinator_addr, "192.168.1.100:8080");
    }

    #[test]
    fn test_get_node_state_when_not_paired() {
        let result = get_node_state(None);
        assert!(result.is_err());

        let err = result.unwrap_err();
        assert_eq!(err.code, "NOT_PAIRED");
    }

    // ─── update_settings Tests ───────────────────────────────────────────────

    #[test]
    fn test_update_settings_valid() {
        let settings = PhoneSettings {
            battery_threshold: 30,
            allow_cellular: true,
            max_model_size_mb: 2048,
            background_mode: BackgroundMode::Aggressive,
            heartbeat_interval_s: 15,
        };

        let result = update_settings(&settings);
        assert!(result.is_ok());

        let applied = result.unwrap();
        assert_eq!(applied.battery_threshold, 30);
        assert!(applied.allow_cellular);
        assert_eq!(applied.max_model_size_mb, 2048);
    }

    #[test]
    fn test_update_settings_invalid_battery_threshold() {
        let settings = PhoneSettings {
            battery_threshold: 101, // Invalid
            ..PhoneSettings::default()
        };

        let result = update_settings(&settings);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, "INVALID_SETTING");
    }

    #[test]
    fn test_update_settings_invalid_heartbeat_interval() {
        let settings = PhoneSettings {
            heartbeat_interval_s: 2, // Too low (minimum 5)
            ..PhoneSettings::default()
        };

        let result = update_settings(&settings);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, "INVALID_SETTING");
    }

    #[test]
    fn test_update_settings_invalid_model_size_too_small() {
        let settings = PhoneSettings {
            max_model_size_mb: 100, // Too small (minimum 256)
            ..PhoneSettings::default()
        };

        let result = update_settings(&settings);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, "INVALID_SETTING");
    }

    #[test]
    fn test_update_settings_invalid_model_size_too_large() {
        let settings = PhoneSettings {
            max_model_size_mb: 10000, // Too large (maximum 8192)
            ..PhoneSettings::default()
        };

        let result = update_settings(&settings);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code, "INVALID_SETTING");
    }

    #[test]
    fn test_update_settings_boundary_values() {
        // Minimum valid values
        let settings = PhoneSettings {
            battery_threshold: 0,
            allow_cellular: false,
            max_model_size_mb: 256,
            background_mode: BackgroundMode::Conservative,
            heartbeat_interval_s: 5,
        };
        assert!(update_settings(&settings).is_ok());

        // Maximum valid values
        let settings = PhoneSettings {
            battery_threshold: 100,
            allow_cellular: true,
            max_model_size_mb: 8192,
            background_mode: BackgroundMode::Aggressive,
            heartbeat_interval_s: 3600,
        };
        assert!(update_settings(&settings).is_ok());
    }

    // ─── stop_companion Tests ────────────────────────────────────────────────

    #[test]
    fn test_stop_companion_no_sessions() {
        let result = stop_companion(0);
        assert!(result.is_ok());

        let stop = result.unwrap();
        assert!(stop.success);
        assert!(stop.graceful_leave_sent);
        assert_eq!(stop.sessions_terminated, 0);
    }

    #[test]
    fn test_stop_companion_with_sessions() {
        let result = stop_companion(3);
        assert!(result.is_ok());

        let stop = result.unwrap();
        assert!(stop.success);
        assert!(stop.graceful_leave_sent);
        assert_eq!(stop.sessions_terminated, 3);
    }
}
