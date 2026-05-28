// Intent citation: .kiro/specs/network-onboarding-wizard/design.md Section 2.6
// Phone Pairing — QR generation, listener, handshake, token expiry, subnet verification

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ─── Pairing Types ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairingInitData {
    pub pairing_token: String,
    pub desktop_lan_address: String,
    pub network_id: Uuid,
    pub protocol_version: u32,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub qr_code_data: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum BackgroundMode {
    Aggressive,
    Balanced,
    Conservative,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhoneSettingsData {
    pub battery_threshold: u8,
    pub allow_cellular: bool,
    pub max_model_size_b: f64,
    pub background_mode: BackgroundMode,
}

impl Default for PhoneSettingsData {
    fn default() -> Self {
        Self {
            battery_threshold: 20,
            allow_cellular: false,
            max_model_size_b: 3.0,
            background_mode: BackgroundMode::Balanced,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PhoneOs {
    Android,
    Ios,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ConnectionType {
    Wifi,
    Cellular,
    Ethernet,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PhoneCapabilities {
    pub os: PhoneOs,
    pub os_version: String,
    pub npu: Option<String>,
    pub ram_gb: f64,
    pub battery_percent: u8,
    pub is_charging: bool,
    pub connection_type: ConnectionType,
    pub app_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairingHandshake {
    pub phone_capabilities: PhoneCapabilities,
    pub pairing_token: String,
    pub phone_node_id: Uuid,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PairingStatus {
    WaitingForConnection,
    Connected { phone_capabilities: PhoneCapabilities },
    Completed { phone_node_id: Uuid },
    Expired,
    Failed { reason: String },
}

#[derive(Debug, Clone, PartialEq)]
pub enum PairingError {
    TokenExpired,
    TokenMismatch,
    WrongNetwork { phone_subnet: String, desktop_subnet: String },
    AlreadyUsed,
    ListenerFailed { reason: String },
}

impl std::fmt::Display for PairingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TokenExpired => write!(f, "Pairing token has expired (5-minute window)"),
            Self::TokenMismatch => write!(f, "Pairing token does not match"),
            Self::WrongNetwork { phone_subnet, desktop_subnet } => {
                write!(f, "Phone ({}) is not on the same network as desktop ({})", phone_subnet, desktop_subnet)
            }
            Self::AlreadyUsed => write!(f, "This pairing token has already been used"),
            Self::ListenerFailed { reason } => write!(f, "Pairing listener failed: {}", reason),
        }
    }
}

// ─── Pairing Manager ─────────────────────────────────────────────────────────

/// Manages phone pairing: QR generation, token validation, handshake.
pub struct PairingManager {
    /// Current pairing session (if any).
    current_session: Option<PairingSession>,
    /// Pairing port (default: 9743).
    pub pairing_port: u16,
    /// Token expiry duration (default: 5 minutes).
    pub token_expiry_minutes: u32,
    /// Protocol version.
    pub protocol_version: u32,
}

struct PairingSession {
    init_data: PairingInitData,
    consumed: bool,
    status: PairingStatus,
}

impl PairingManager {
    pub fn new() -> Self {
        Self {
            current_session: None,
            pairing_port: 9743,
            token_expiry_minutes: 5,
            protocol_version: 1,
        }
    }

    /// Generate a new pairing QR code with a fresh token.
    pub fn generate_pairing_qr(&mut self, desktop_lan_address: &str) -> PairingInitData {
        // Generate 128-bit random token
        let token_bytes: [u8; 16] = rand::random();
        let pairing_token = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(&token_bytes);

        let network_id = Uuid::new_v4();
        let created_at = Utc::now();
        let expires_at = created_at + Duration::minutes(self.token_expiry_minutes as i64);

        // QR data format: resonantos://pair?token=X&addr=Y&net=Z&v=1
        let qr_code_data = format!(
            "resonantos://pair?token={}&addr={}&net={}&v={}",
            pairing_token, desktop_lan_address, network_id, self.protocol_version
        );

        let init_data = PairingInitData {
            pairing_token: pairing_token.clone(),
            desktop_lan_address: desktop_lan_address.to_string(),
            network_id,
            protocol_version: self.protocol_version,
            created_at,
            expires_at,
            qr_code_data,
        };

        self.current_session = Some(PairingSession {
            init_data: init_data.clone(),
            consumed: false,
            status: PairingStatus::WaitingForConnection,
        });

        init_data
    }

    /// Validate a pairing handshake from a phone.
    pub fn validate_handshake(
        &mut self,
        handshake: &PairingHandshake,
        phone_ip: &str,
        desktop_subnet: &str,
    ) -> Result<Uuid, PairingError> {
        let session = self
            .current_session
            .as_mut()
            .ok_or(PairingError::ListenerFailed {
                reason: "No active pairing session".to_string(),
            })?;

        // Check if already consumed (single-use)
        if session.consumed {
            return Err(PairingError::AlreadyUsed);
        }

        // Check token expiry
        if Utc::now() > session.init_data.expires_at {
            session.status = PairingStatus::Expired;
            return Err(PairingError::TokenExpired);
        }

        // Verify token matches
        if handshake.pairing_token != session.init_data.pairing_token {
            return Err(PairingError::TokenMismatch);
        }

        // Verify same network (subnet check)
        let phone_subnet = extract_subnet(phone_ip);
        if phone_subnet != desktop_subnet {
            return Err(PairingError::WrongNetwork {
                phone_subnet,
                desktop_subnet: desktop_subnet.to_string(),
            });
        }

        // All checks passed — mark as consumed and complete
        session.consumed = true;
        session.status = PairingStatus::Completed {
            phone_node_id: handshake.phone_node_id,
        };

        Ok(handshake.phone_node_id)
    }

    /// Get current pairing status.
    pub fn status(&self) -> PairingStatus {
        match &self.current_session {
            Some(session) => {
                if !session.consumed && Utc::now() > session.init_data.expires_at {
                    PairingStatus::Expired
                } else {
                    session.status.clone()
                }
            }
            None => PairingStatus::WaitingForConnection,
        }
    }

    /// Check if the current token is expired.
    pub fn is_expired(&self) -> bool {
        self.current_session
            .as_ref()
            .map(|s| Utc::now() > s.init_data.expires_at)
            .unwrap_or(true)
    }

    /// Regenerate QR code (creates new token, invalidates old one).
    pub fn regenerate(&mut self, desktop_lan_address: &str) -> PairingInitData {
        self.generate_pairing_qr(desktop_lan_address)
    }
}

/// Extract subnet from IP address (first 3 octets).
fn extract_subnet(ip: &str) -> String {
    let parts: Vec<&str> = ip.split('.').collect();
    if parts.len() >= 3 {
        format!("{}.{}.{}", parts[0], parts[1], parts[2])
    } else {
        ip.to_string()
    }
}

use base64::Engine;

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_handshake(token: &str) -> PairingHandshake {
        PairingHandshake {
            phone_capabilities: PhoneCapabilities {
                os: PhoneOs::Android,
                os_version: "14".to_string(),
                npu: Some("Hexagon".to_string()),
                ram_gb: 8.0,
                battery_percent: 85,
                is_charging: false,
                connection_type: ConnectionType::Wifi,
                app_version: "1.0.0".to_string(),
            },
            pairing_token: token.to_string(),
            phone_node_id: Uuid::new_v4(),
        }
    }

    #[test]
    fn test_generate_qr() {
        let mut manager = PairingManager::new();
        let init = manager.generate_pairing_qr("192.168.1.10");

        assert!(!init.pairing_token.is_empty());
        assert!(init.qr_code_data.contains("resonantos://pair"));
        assert!(init.qr_code_data.contains(&init.pairing_token));
        assert!(init.expires_at > init.created_at);
    }

    #[test]
    fn test_valid_handshake() {
        let mut manager = PairingManager::new();
        let init = manager.generate_pairing_qr("192.168.1.10");

        let handshake = make_handshake(&init.pairing_token);
        let result = manager.validate_handshake(&handshake, "192.168.1.50", "192.168.1");
        assert!(result.is_ok());
    }

    #[test]
    fn test_expired_token_rejected() {
        let mut manager = PairingManager::new();
        manager.token_expiry_minutes = 0; // Expire immediately
        let init = manager.generate_pairing_qr("192.168.1.10");

        // Manually expire
        if let Some(session) = &mut manager.current_session {
            session.init_data.expires_at = Utc::now() - Duration::seconds(1);
        }

        let handshake = make_handshake(&init.pairing_token);
        let result = manager.validate_handshake(&handshake, "192.168.1.50", "192.168.1");
        assert_eq!(result, Err(PairingError::TokenExpired));
    }

    #[test]
    fn test_wrong_network_rejected() {
        let mut manager = PairingManager::new();
        let init = manager.generate_pairing_qr("192.168.1.10");

        let handshake = make_handshake(&init.pairing_token);
        // Phone on different subnet
        let result = manager.validate_handshake(&handshake, "10.0.0.50", "192.168.1");
        assert!(matches!(result, Err(PairingError::WrongNetwork { .. })));
    }

    #[test]
    fn test_token_single_use() {
        let mut manager = PairingManager::new();
        let init = manager.generate_pairing_qr("192.168.1.10");

        let handshake = make_handshake(&init.pairing_token);
        let r1 = manager.validate_handshake(&handshake, "192.168.1.50", "192.168.1");
        assert!(r1.is_ok());

        // Second use should fail
        let r2 = manager.validate_handshake(&handshake, "192.168.1.50", "192.168.1");
        assert_eq!(r2, Err(PairingError::AlreadyUsed));
    }

    #[test]
    fn test_wrong_token_rejected() {
        let mut manager = PairingManager::new();
        manager.generate_pairing_qr("192.168.1.10");

        let handshake = make_handshake("wrong-token");
        let result = manager.validate_handshake(&handshake, "192.168.1.50", "192.168.1");
        assert_eq!(result, Err(PairingError::TokenMismatch));
    }

    #[test]
    fn test_subnet_extraction() {
        assert_eq!(extract_subnet("192.168.1.100"), "192.168.1");
        assert_eq!(extract_subnet("10.0.0.5"), "10.0.0");
    }
}
