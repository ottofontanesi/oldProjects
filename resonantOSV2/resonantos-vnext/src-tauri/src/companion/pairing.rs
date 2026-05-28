//! PairingClient: QR code parsing and handshake with Coordinator.
//!
//! Handles the phone side of the QR code pairing flow (counterpart to desktop's
//! `wizard/pairing.rs`). Implements:
//! - QR data parsing to extract pairing token and coordinator address
//! - Token expiry validation (5-minute window)
//! - Subnet verification (first three octets must match)
//! - Handshake message construction with full phone capabilities
//! - Reconnection using stored MeshIdentity

use std::time::{SystemTime, UNIX_EPOCH};

use uuid::Uuid;

use crate::companion::identity::MeshIdentity;
use crate::companion::types::{ConnectionType, NodeId, TrustLevel};

// ─── Error Types ─────────────────────────────────────────────────────────────

/// Errors that can occur during pairing operations.
#[derive(Debug, Clone, PartialEq)]
pub enum PairingClientError {
    /// The pairing token has expired (older than 5 minutes).
    TokenExpired,
    /// The phone and desktop are not on the same subnet.
    SubnetMismatch { phone: String, desktop: String },
    /// The coordinator is not reachable over any transport.
    NetworkUnreachable,
    /// The QR code data is malformed or cannot be parsed.
    InvalidQrData(String),
    /// The coordinator rejected the handshake.
    HandshakeRejected(String),
}

impl std::fmt::Display for PairingClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TokenExpired => write!(f, "Pairing token has expired (>5 minutes old)"),
            Self::SubnetMismatch { phone, desktop } => {
                write!(
                    f,
                    "Subnet mismatch: phone={}, desktop={}",
                    phone, desktop
                )
            }
            Self::NetworkUnreachable => write!(f, "Coordinator is not reachable"),
            Self::InvalidQrData(msg) => write!(f, "Invalid QR data: {}", msg),
            Self::HandshakeRejected(msg) => write!(f, "Handshake rejected: {}", msg),
        }
    }
}

impl std::error::Error for PairingClientError {}

// ─── Phone Capabilities ──────────────────────────────────────────────────────

/// Capabilities reported by the phone during pairing handshake.
#[derive(Debug, Clone)]
pub struct PhoneCapabilities {
    /// Operating system (e.g., "iOS 17.4", "Android 14").
    pub os: String,
    /// NPU type description (e.g., "Apple Neural Engine Gen 5").
    pub npu: String,
    /// Total RAM in MB.
    pub ram_mb: u64,
    /// Current battery percentage (0-100).
    pub battery_percent: u8,
    /// Current connection type.
    pub connection_type: ConnectionType,
}

// ─── QR Data ─────────────────────────────────────────────────────────────────

/// Parsed QR code data from the desktop coordinator.
#[derive(Debug, Clone)]
pub struct QrCodeData {
    /// The single-use pairing token.
    pub token: String,
    /// The coordinator's network address (ip:port).
    pub coordinator_addr: String,
    /// The desktop's subnet (for verification).
    pub desktop_subnet: String,
    /// Timestamp when the token was created (Unix epoch seconds).
    pub created_at_secs: u64,
    /// The mesh network ID to join.
    pub network_id: Uuid,
}

// ─── Handshake Message ───────────────────────────────────────────────────────

/// The handshake message sent from phone to coordinator during pairing.
#[derive(Debug, Clone)]
pub struct HandshakeMessage {
    /// The pairing token from the QR code.
    pub pairing_token: String,
    /// The phone's node ID (derived from Ed25519 public key).
    pub phone_node_id: NodeId,
    /// The phone's capabilities.
    pub capabilities: PhoneCapabilities,
}

// ─── Pairing Result ──────────────────────────────────────────────────────────

/// Successful pairing result returned after handshake completion.
#[derive(Debug, Clone)]
pub struct PairingResult {
    /// The mesh network ID the phone joined.
    pub network_id: Uuid,
    /// The coordinator's address for future communication.
    pub coordinator_addr: String,
    /// The node ID assigned to this phone.
    pub assigned_node_id: Uuid,
    /// Trust level (always LocalOwned after owner pairing).
    pub trust_level: TrustLevel,
}

// ─── PairingClient ───────────────────────────────────────────────────────────

/// Phone-side pairing client.
///
/// Handles QR code parsing, token validation, subnet verification,
/// and handshake construction for mesh network registration.
pub struct PairingClient {
    /// The phone's mesh identity (Ed25519 keypair).
    identity: MeshIdentity,
    /// The phone's capabilities to report during pairing.
    capabilities: PhoneCapabilities,
}

impl PairingClient {
    /// Create a new PairingClient with the given identity and capabilities.
    pub fn new(identity: MeshIdentity, capabilities: PhoneCapabilities) -> Self {
        Self {
            identity,
            capabilities,
        }
    }

    /// Parse QR code data and initiate pairing handshake.
    ///
    /// Steps:
    /// 1. Parse QR data string into structured fields
    /// 2. Validate token expiry (must be within 5 minutes)
    /// 3. Verify subnet match (first three octets)
    /// 4. Construct and return handshake message
    ///
    /// # Arguments
    /// * `qr_data` - Raw QR code string scanned by the phone camera
    /// * `phone_ip` - The phone's current IP address (for subnet verification)
    ///
    /// # Returns
    /// A `PairingResult` on success, or a `PairingClientError` on failure.
    pub fn pair_from_qr(
        &self,
        qr_data: &str,
        phone_ip: &str,
    ) -> Result<PairingResult, PairingClientError> {
        // Step 1: Parse QR data
        let qr = Self::parse_qr_data(qr_data)?;

        // Step 2: Validate token expiry
        Self::validate_token_expiry(qr.created_at_secs)?;

        // Step 3: Verify subnet match
        Self::verify_subnet(phone_ip, &qr.desktop_subnet)?;

        // Step 4: Construct handshake message (in a real implementation,
        // this would be sent over the transport layer)
        let _handshake = self.build_handshake(&qr);

        // In a real implementation, we'd send the handshake and wait for response.
        // For now, return a successful pairing result.
        Ok(PairingResult {
            network_id: qr.network_id,
            coordinator_addr: qr.coordinator_addr,
            assigned_node_id: self.identity.node_id,
            trust_level: TrustLevel::LocalOwned,
        })
    }

    /// Re-authenticate with Coordinator using stored identity (no QR needed).
    ///
    /// Used after network interruptions or app restarts to rejoin the mesh
    /// without requiring a new QR code scan.
    ///
    /// # Arguments
    /// * `coordinator_addr` - The coordinator's address from stored state
    ///
    /// # Returns
    /// `Ok(())` on successful reconnection, or a `PairingClientError`.
    pub fn reconnect(&self, coordinator_addr: &str) -> Result<PairingResult, PairingClientError> {
        // Validate coordinator address is not empty
        if coordinator_addr.is_empty() {
            return Err(PairingClientError::NetworkUnreachable);
        }

        // Sign a reconnection challenge with our identity to prove we are who we claim
        let challenge = format!("reconnect:{}:{}", self.identity.node_id, coordinator_addr);
        let _signature = self
            .identity
            .sign(challenge.as_bytes())
            .map_err(|_| PairingClientError::NetworkUnreachable)?;

        // In a real implementation, we'd send the signed challenge to the coordinator
        // and wait for acknowledgment. For now, return success.
        Ok(PairingResult {
            network_id: Uuid::new_v4(), // Would come from coordinator response
            coordinator_addr: coordinator_addr.to_string(),
            assigned_node_id: self.identity.node_id,
            trust_level: TrustLevel::LocalOwned,
        })
    }

    /// Parse QR code data string into structured fields.
    ///
    /// Expected format: `resonant://<coordinator_addr>?token=<token>&subnet=<subnet>&ts=<timestamp>&net=<network_id>`
    pub fn parse_qr_data(qr_data: &str) -> Result<QrCodeData, PairingClientError> {
        // Check for the resonant:// prefix
        let data = qr_data
            .strip_prefix("resonant://")
            .ok_or_else(|| PairingClientError::InvalidQrData("Missing resonant:// prefix".to_string()))?;

        // Split on '?' to get coordinator address and query params
        let parts: Vec<&str> = data.splitn(2, '?').collect();
        if parts.len() != 2 {
            return Err(PairingClientError::InvalidQrData(
                "Missing query parameters".to_string(),
            ));
        }

        let coordinator_addr = parts[0].to_string();
        if coordinator_addr.is_empty() {
            return Err(PairingClientError::InvalidQrData(
                "Empty coordinator address".to_string(),
            ));
        }

        let query = parts[1];

        // Parse query parameters
        let mut token = None;
        let mut subnet = None;
        let mut ts = None;
        let mut net = None;

        for param in query.split('&') {
            let kv: Vec<&str> = param.splitn(2, '=').collect();
            if kv.len() != 2 {
                continue;
            }
            match kv[0] {
                "token" => token = Some(kv[1].to_string()),
                "subnet" => subnet = Some(kv[1].to_string()),
                "ts" => ts = Some(kv[1].to_string()),
                "net" => net = Some(kv[1].to_string()),
                _ => {}
            }
        }

        let token = token.ok_or_else(|| {
            PairingClientError::InvalidQrData("Missing token parameter".to_string())
        })?;
        let subnet = subnet.ok_or_else(|| {
            PairingClientError::InvalidQrData("Missing subnet parameter".to_string())
        })?;
        let ts_str = ts.ok_or_else(|| {
            PairingClientError::InvalidQrData("Missing ts parameter".to_string())
        })?;
        let net_str = net.ok_or_else(|| {
            PairingClientError::InvalidQrData("Missing net parameter".to_string())
        })?;

        let created_at_secs = ts_str.parse::<u64>().map_err(|_| {
            PairingClientError::InvalidQrData("Invalid timestamp".to_string())
        })?;

        let network_id = Uuid::parse_str(&net_str).map_err(|_| {
            PairingClientError::InvalidQrData("Invalid network ID".to_string())
        })?;

        Ok(QrCodeData {
            token,
            coordinator_addr,
            desktop_subnet: subnet,
            created_at_secs,
            network_id,
        })
    }

    /// Validate that the pairing token has not expired.
    ///
    /// Tokens are valid for 5 minutes (300 seconds) from creation.
    pub fn validate_token_expiry(created_at_secs: u64) -> Result<(), PairingClientError> {
        let now_secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let elapsed = now_secs.saturating_sub(created_at_secs);

        if elapsed > 300 {
            return Err(PairingClientError::TokenExpired);
        }

        Ok(())
    }

    /// Validate token expiry using an explicit "now" timestamp (for testing).
    ///
    /// Tokens are valid for 5 minutes (300 seconds) from creation.
    pub fn validate_token_expiry_at(
        created_at_secs: u64,
        now_secs: u64,
    ) -> Result<(), PairingClientError> {
        let elapsed = now_secs.saturating_sub(created_at_secs);

        if elapsed > 300 {
            return Err(PairingClientError::TokenExpired);
        }

        Ok(())
    }

    /// Verify that the phone and desktop are on the same subnet.
    ///
    /// Compares the first three octets of the phone IP against the desktop subnet.
    pub fn verify_subnet(phone_ip: &str, desktop_subnet: &str) -> Result<(), PairingClientError> {
        let phone_prefix = Self::extract_subnet_prefix(phone_ip).ok_or_else(|| {
            PairingClientError::InvalidQrData(format!("Invalid phone IP: {}", phone_ip))
        })?;

        let desktop_prefix =
            Self::extract_subnet_prefix(desktop_subnet).ok_or_else(|| {
                PairingClientError::InvalidQrData(format!(
                    "Invalid desktop subnet: {}",
                    desktop_subnet
                ))
            })?;

        if phone_prefix != desktop_prefix {
            return Err(PairingClientError::SubnetMismatch {
                phone: phone_prefix,
                desktop: desktop_prefix,
            });
        }

        Ok(())
    }

    /// Extract the first three octets from an IPv4 address string.
    ///
    /// Accepts formats like "192.168.1.100" or "192.168.1" (already a prefix).
    pub fn extract_subnet_prefix(ip: &str) -> Option<String> {
        let octets: Vec<&str> = ip.split('.').collect();
        if octets.len() < 3 {
            return None;
        }

        // Validate that the first three parts are valid octets
        for octet_str in &octets[..3] {
            let val: u16 = octet_str.parse().ok()?;
            if val > 255 {
                return None;
            }
        }

        Some(format!("{}.{}.{}", octets[0], octets[1], octets[2]))
    }

    /// Build the handshake message to send to the coordinator.
    fn build_handshake(&self, qr: &QrCodeData) -> HandshakeMessage {
        HandshakeMessage {
            pairing_token: qr.token.clone(),
            phone_node_id: self.identity.node_id,
            capabilities: self.capabilities.clone(),
        }
    }
}

// ─── Unit Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_capabilities() -> PhoneCapabilities {
        PhoneCapabilities {
            os: "iOS 17.4".to_string(),
            npu: "Apple Neural Engine Gen 5".to_string(),
            ram_mb: 6144,
            battery_percent: 85,
            connection_type: ConnectionType::WiFi,
        }
    }

    fn make_test_client() -> PairingClient {
        let identity = MeshIdentity::generate().expect("should generate identity");
        let capabilities = make_test_capabilities();
        PairingClient::new(identity, capabilities)
    }

    fn current_timestamp() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }

    fn make_valid_qr(ts: u64) -> String {
        let net_id = Uuid::new_v4();
        format!(
            "resonant://192.168.1.10:8080?token=abc123&subnet=192.168.1.1&ts={}&net={}",
            ts, net_id
        )
    }

    // ─── QR Parsing Tests ────────────────────────────────────────────────────

    #[test]
    fn test_parse_valid_qr_data() {
        let net_id = Uuid::new_v4();
        let qr = format!(
            "resonant://192.168.1.10:8080?token=mytoken&subnet=192.168.1.1&ts=1000000&net={}",
            net_id
        );

        let parsed = PairingClient::parse_qr_data(&qr).unwrap();
        assert_eq!(parsed.token, "mytoken");
        assert_eq!(parsed.coordinator_addr, "192.168.1.10:8080");
        assert_eq!(parsed.desktop_subnet, "192.168.1.1");
        assert_eq!(parsed.created_at_secs, 1000000);
        assert_eq!(parsed.network_id, net_id);
    }

    #[test]
    fn test_parse_qr_missing_prefix() {
        let result = PairingClient::parse_qr_data("http://192.168.1.10?token=abc");
        assert!(matches!(result, Err(PairingClientError::InvalidQrData(_))));
    }

    #[test]
    fn test_parse_qr_missing_token() {
        let qr = "resonant://192.168.1.10:8080?subnet=192.168.1.1&ts=1000&net=00000000-0000-0000-0000-000000000000";
        let result = PairingClient::parse_qr_data(qr);
        assert!(matches!(result, Err(PairingClientError::InvalidQrData(_))));
    }

    #[test]
    fn test_parse_qr_missing_query_params() {
        let result = PairingClient::parse_qr_data("resonant://192.168.1.10:8080");
        assert!(matches!(result, Err(PairingClientError::InvalidQrData(_))));
    }

    #[test]
    fn test_parse_qr_invalid_timestamp() {
        let qr = "resonant://192.168.1.10:8080?token=abc&subnet=192.168.1.1&ts=notanumber&net=00000000-0000-0000-0000-000000000000";
        let result = PairingClient::parse_qr_data(qr);
        assert!(matches!(result, Err(PairingClientError::InvalidQrData(_))));
    }

    #[test]
    fn test_parse_qr_invalid_network_id() {
        let qr = "resonant://192.168.1.10:8080?token=abc&subnet=192.168.1.1&ts=1000&net=not-a-uuid";
        let result = PairingClient::parse_qr_data(qr);
        assert!(matches!(result, Err(PairingClientError::InvalidQrData(_))));
    }

    // ─── Token Expiry Tests ──────────────────────────────────────────────────

    #[test]
    fn test_token_valid_within_5_minutes() {
        // Token created 4 minutes ago (240 seconds)
        let now = 1000;
        let created = now - 240;
        assert!(PairingClient::validate_token_expiry_at(created, now).is_ok());
    }

    #[test]
    fn test_token_valid_at_exactly_5_minutes() {
        // Token created exactly 300 seconds ago
        let now = 1000;
        let created = now - 300;
        assert!(PairingClient::validate_token_expiry_at(created, now).is_ok());
    }

    #[test]
    fn test_token_expired_over_5_minutes() {
        // Token created 301 seconds ago
        let now = 1000;
        let created = now - 301;
        let result = PairingClient::validate_token_expiry_at(created, now);
        assert_eq!(result, Err(PairingClientError::TokenExpired));
    }

    #[test]
    fn test_token_valid_just_created() {
        // Token created right now
        let now = 1000;
        let created = now;
        assert!(PairingClient::validate_token_expiry_at(created, now).is_ok());
    }

    #[test]
    fn test_token_future_timestamp_is_valid() {
        // Token created "in the future" (clock skew) — elapsed is 0 due to saturating_sub
        let now = 1000;
        let created = now + 100;
        assert!(PairingClient::validate_token_expiry_at(created, now).is_ok());
    }

    // ─── Subnet Verification Tests ──────────────────────────────────────────

    #[test]
    fn test_subnet_match_same_network() {
        let result = PairingClient::verify_subnet("192.168.1.100", "192.168.1.1");
        assert!(result.is_ok());
    }

    #[test]
    fn test_subnet_mismatch_different_network() {
        let result = PairingClient::verify_subnet("192.168.1.100", "10.0.0.1");
        assert!(matches!(
            result,
            Err(PairingClientError::SubnetMismatch { .. })
        ));
    }

    #[test]
    fn test_subnet_mismatch_third_octet_differs() {
        let result = PairingClient::verify_subnet("192.168.2.100", "192.168.1.1");
        assert!(matches!(
            result,
            Err(PairingClientError::SubnetMismatch { .. })
        ));
    }

    #[test]
    fn test_subnet_match_different_host() {
        // Same subnet, different host part
        let result = PairingClient::verify_subnet("10.0.1.55", "10.0.1.200");
        assert!(result.is_ok());
    }

    #[test]
    fn test_subnet_invalid_phone_ip() {
        let result = PairingClient::verify_subnet("invalid", "192.168.1.1");
        assert!(matches!(result, Err(PairingClientError::InvalidQrData(_))));
    }

    #[test]
    fn test_extract_subnet_prefix_valid() {
        assert_eq!(
            PairingClient::extract_subnet_prefix("192.168.1.100"),
            Some("192.168.1".to_string())
        );
    }

    #[test]
    fn test_extract_subnet_prefix_three_octets() {
        assert_eq!(
            PairingClient::extract_subnet_prefix("10.0.0"),
            Some("10.0.0".to_string())
        );
    }

    #[test]
    fn test_extract_subnet_prefix_too_few_octets() {
        assert_eq!(PairingClient::extract_subnet_prefix("192.168"), None);
    }

    #[test]
    fn test_extract_subnet_prefix_invalid_octet() {
        assert_eq!(PairingClient::extract_subnet_prefix("256.168.1.1"), None);
    }

    // ─── Full Pairing Flow Tests ─────────────────────────────────────────────

    #[test]
    fn test_pair_from_qr_success() {
        let client = make_test_client();
        let ts = current_timestamp();
        let qr = make_valid_qr(ts);

        let result = client.pair_from_qr(&qr, "192.168.1.50");
        assert!(result.is_ok());

        let pairing = result.unwrap();
        assert_eq!(pairing.trust_level, TrustLevel::LocalOwned);
        assert_eq!(pairing.coordinator_addr, "192.168.1.10:8080");
    }

    #[test]
    fn test_pair_from_qr_expired_token() {
        let client = make_test_client();
        // Token created 10 minutes ago
        let ts = current_timestamp() - 600;
        let qr = make_valid_qr(ts);

        let result = client.pair_from_qr(&qr, "192.168.1.50");
        assert!(matches!(result, Err(PairingClientError::TokenExpired)));
    }

    #[test]
    fn test_pair_from_qr_subnet_mismatch() {
        let client = make_test_client();
        let ts = current_timestamp();
        let qr = make_valid_qr(ts);

        // Phone is on a different subnet
        let result = client.pair_from_qr(&qr, "10.0.0.50");
        assert!(matches!(
            result,
            Err(PairingClientError::SubnetMismatch { .. })
        ));
    }

    #[test]
    fn test_pair_from_qr_invalid_data() {
        let client = make_test_client();
        let result = client.pair_from_qr("garbage data", "192.168.1.50");
        assert!(matches!(result, Err(PairingClientError::InvalidQrData(_))));
    }

    // ─── Reconnection Tests ──────────────────────────────────────────────────

    #[test]
    fn test_reconnect_success() {
        let client = make_test_client();
        let result = client.reconnect("192.168.1.10:8080");
        assert!(result.is_ok());

        let pairing = result.unwrap();
        assert_eq!(pairing.coordinator_addr, "192.168.1.10:8080");
        assert_eq!(pairing.trust_level, TrustLevel::LocalOwned);
    }

    #[test]
    fn test_reconnect_empty_address() {
        let client = make_test_client();
        let result = client.reconnect("");
        assert!(matches!(result, Err(PairingClientError::NetworkUnreachable)));
    }

    // ─── Handshake Message Tests ─────────────────────────────────────────────

    #[test]
    fn test_handshake_contains_all_fields() {
        let identity = MeshIdentity::generate().expect("should generate");
        let capabilities = PhoneCapabilities {
            os: "Android 14".to_string(),
            npu: "Qualcomm Hexagon v73".to_string(),
            ram_mb: 8192,
            battery_percent: 72,
            connection_type: ConnectionType::WiFi,
        };
        let client = PairingClient::new(identity, capabilities.clone());

        let qr = QrCodeData {
            token: "test-token-123".to_string(),
            coordinator_addr: "192.168.1.10:8080".to_string(),
            desktop_subnet: "192.168.1.1".to_string(),
            created_at_secs: current_timestamp(),
            network_id: Uuid::new_v4(),
        };

        let handshake = client.build_handshake(&qr);

        // Verify all fields are present
        assert_eq!(handshake.pairing_token, "test-token-123");
        assert_ne!(handshake.phone_node_id, Uuid::nil());
        assert_eq!(handshake.capabilities.os, "Android 14");
        assert_eq!(handshake.capabilities.npu, "Qualcomm Hexagon v73");
        assert_eq!(handshake.capabilities.ram_mb, 8192);
        assert_eq!(handshake.capabilities.battery_percent, 72);
        assert_eq!(handshake.capabilities.connection_type, ConnectionType::WiFi);
    }
}
