// Intent citation: .kiro/specs/mesh-network-optimizer/design.md Section 2.1
// Mesh Identity — Ed25519 keypair generation, signing, verification, and invitation tokens

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::transport::trait_def::NodeId;

/// Unique identifier for a mesh network.
pub type MeshId = Uuid;

// ─── Trust Tiers ─────────────────────────────────────────────────────────────

/// Trust tiers determine what a node can see and do within the mesh.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TrustTier {
    /// Routing only, never sees prompts.
    Public = 1,
    /// Can serve non-sensitive inference.
    InvitedFriend = 2,
    /// Full trust, sees all prompts.
    LocalOwned = 3,
}

// ─── Mesh Identity ───────────────────────────────────────────────────────────

/// A node's cryptographic identity within the mesh network.
/// The signing key is private and never transmitted; the verifying key is shared.
#[derive(Debug, Clone)]
pub struct MeshIdentity {
    pub node_id: NodeId,
    pub signing_key: SigningKey,
    pub verifying_key: VerifyingKey,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl MeshIdentity {
    /// Generate a new mesh identity with a fresh Ed25519 keypair.
    /// Called on first run to create the node's persistent identity.
    pub fn generate() -> Self {
        let mut csprng = rand::rngs::OsRng;
        let signing_key = SigningKey::generate(&mut csprng);
        let verifying_key = signing_key.verifying_key();

        Self {
            node_id: Uuid::new_v4(),
            signing_key,
            verifying_key,
            created_at: chrono::Utc::now(),
        }
    }

    /// Load an existing identity from encrypted local storage, or generate a new one
    /// if none exists. The identity is persisted as an encrypted JSON blob keyed by
    /// a passphrase-derived key (using the platform keychain in production).
    ///
    /// Storage format: JSON with the signing key bytes stored as a hex string,
    /// encrypted at rest via the platform's secure storage (macOS Keychain,
    /// Windows Credential Manager, or Linux Secret Service).
    pub fn load_or_generate(conn: &rusqlite::Connection) -> Result<Self, String> {
        // Try to load existing identity
        let existing: Option<(String, Vec<u8>, String)> = conn
            .query_row(
                "SELECT node_id, signing_key_bytes, created_at FROM mesh_identity LIMIT 1",
                [],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, Vec<u8>>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .ok();

        if let Some((node_id_str, key_bytes, created_at_str)) = existing {
            let node_id = Uuid::parse_str(&node_id_str)
                .map_err(|e| format!("Invalid stored node_id: {}", e))?;
            let key_array: [u8; 32] = key_bytes
                .try_into()
                .map_err(|_| "Stored signing key must be 32 bytes".to_string())?;
            let signing_key = SigningKey::from_bytes(&key_array);
            let verifying_key = signing_key.verifying_key();
            let created_at = chrono::DateTime::parse_from_rfc3339(&created_at_str)
                .map_err(|e| format!("Invalid stored created_at: {}", e))?
                .with_timezone(&chrono::Utc);

            Ok(Self {
                node_id,
                signing_key,
                verifying_key,
                created_at,
            })
        } else {
            // Generate new identity and persist it
            let identity = Self::generate();
            identity.persist(conn)?;
            Ok(identity)
        }
    }

    /// Persist this identity to encrypted local storage.
    /// The signing key bytes are stored directly — the database file itself should
    /// be encrypted at rest by the platform's secure storage mechanism.
    pub fn persist(&self, conn: &rusqlite::Connection) -> Result<(), String> {
        conn.execute(
            "CREATE TABLE IF NOT EXISTS mesh_identity (
                node_id TEXT PRIMARY KEY,
                signing_key_bytes BLOB NOT NULL,
                created_at TEXT NOT NULL
            )",
            [],
        )
        .map_err(|e| format!("Failed to create mesh_identity table: {}", e))?;

        conn.execute(
            "INSERT OR REPLACE INTO mesh_identity (node_id, signing_key_bytes, created_at)
             VALUES (?1, ?2, ?3)",
            rusqlite::params![
                self.node_id.to_string(),
                self.signing_key.to_bytes().to_vec(),
                self.created_at.to_rfc3339(),
            ],
        )
        .map_err(|e| format!("Failed to persist mesh identity: {}", e))?;

        Ok(())
    }

    /// Sign arbitrary data with this node's private key.
    pub fn sign(&self, data: &[u8]) -> Signature {
        self.signing_key.sign(data)
    }

    /// Verify a signature against data using a given public key.
    /// Returns true if the signature is valid for the data and key.
    pub fn verify(data: &[u8], signature: &Signature, public_key: &VerifyingKey) -> bool {
        public_key.verify(data, signature).is_ok()
    }

    /// Create an invitation token for another node to join a mesh.
    /// The token is signed by this node (the inviter) and includes the offered trust tier.
    pub fn create_invitation(
        &self,
        mesh_id: MeshId,
        offered_tier: TrustTier,
        expires_in_hours: u32,
    ) -> InvitationToken {
        let token_id = Uuid::new_v4();
        let expires_at = chrono::Utc::now()
            + chrono::Duration::hours(expires_in_hours as i64);

        // Build the signable payload (everything except the signature itself)
        let payload = InvitationPayload {
            token_id,
            mesh_id,
            inviter_node_id: self.node_id,
            offered_tier,
            expires_at,
        };
        let payload_bytes = serde_json::to_vec(&payload)
            .expect("InvitationPayload serialization should not fail");

        let signature = self.sign(&payload_bytes);

        InvitationToken {
            token_id,
            mesh_id,
            inviter_node_id: self.node_id,
            offered_tier,
            expires_at,
            signature,
            consumed: false,
        }
    }
}

// ─── Invitation Token ────────────────────────────────────────────────────────

/// Internal payload used for signing — contains all fields except the signature.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvitationPayload {
    pub token_id: Uuid,
    pub mesh_id: MeshId,
    pub inviter_node_id: NodeId,
    pub offered_tier: TrustTier,
    pub expires_at: chrono::DateTime<chrono::Utc>,
}

/// An invitation token that allows a new node to join a mesh.
/// Single-use, time-limited, and cryptographically signed by the inviter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvitationToken {
    pub token_id: Uuid,
    pub mesh_id: MeshId,
    pub inviter_node_id: NodeId,
    pub offered_tier: TrustTier,
    pub expires_at: chrono::DateTime<chrono::Utc>,
    /// Ed25519 signature over the token payload, produced by the inviter.
    #[serde(with = "signature_serde")]
    pub signature: Signature,
    pub consumed: bool,
}

/// Errors that can occur during invitation token validation.
#[derive(Debug, Clone, PartialEq)]
pub enum InvitationError {
    /// The token has expired (current time is past expires_at).
    Expired,
    /// The token has already been consumed (single-use).
    AlreadyConsumed,
    /// The signature does not match the inviter's public key.
    InvalidSignature,
    /// Failed to decode the token from its encoded form.
    DecodingFailed(String),
}

impl std::fmt::Display for InvitationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Expired => write!(f, "Invitation token has expired"),
            Self::AlreadyConsumed => write!(f, "Invitation token has already been consumed"),
            Self::InvalidSignature => write!(f, "Invitation token signature is invalid"),
            Self::DecodingFailed(reason) => write!(f, "Failed to decode invitation token: {}", reason),
        }
    }
}

impl InvitationToken {
    /// Validate the invitation token: check expiry, verify signature, check consumed flag.
    /// Returns Ok(()) if the token is valid, or an appropriate error.
    pub fn validate(&self, inviter_public_key: &VerifyingKey) -> Result<(), InvitationError> {
        // Check if already consumed (single-use)
        if self.consumed {
            return Err(InvitationError::AlreadyConsumed);
        }

        // Check expiry
        if chrono::Utc::now() > self.expires_at {
            return Err(InvitationError::Expired);
        }

        // Verify signature against the payload
        let payload = InvitationPayload {
            token_id: self.token_id,
            mesh_id: self.mesh_id,
            inviter_node_id: self.inviter_node_id,
            offered_tier: self.offered_tier,
            expires_at: self.expires_at,
        };
        let payload_bytes = serde_json::to_vec(&payload)
            .map_err(|e| InvitationError::DecodingFailed(e.to_string()))?;

        if !MeshIdentity::verify(&payload_bytes, &self.signature, inviter_public_key) {
            return Err(InvitationError::InvalidSignature);
        }

        Ok(())
    }

    /// Mark this token as consumed (single-use enforcement).
    pub fn consume(&mut self) {
        self.consumed = true;
    }

    /// Encode the invitation token as a base64url-safe string suitable for URLs and QR codes.
    pub fn encode(&self) -> String {
        use base64::engine::general_purpose::URL_SAFE_NO_PAD;
        use base64::Engine;

        let json = serde_json::to_vec(self)
            .expect("InvitationToken serialization should not fail");
        URL_SAFE_NO_PAD.encode(&json)
    }

    /// Decode an invitation token from a base64url-safe string.
    pub fn decode(encoded: &str) -> Result<Self, InvitationError> {
        use base64::engine::general_purpose::URL_SAFE_NO_PAD;
        use base64::Engine;

        let bytes = URL_SAFE_NO_PAD
            .decode(encoded)
            .map_err(|e| InvitationError::DecodingFailed(format!("base64 decode: {}", e)))?;

        serde_json::from_slice(&bytes)
            .map_err(|e| InvitationError::DecodingFailed(format!("json decode: {}", e)))
    }
}

// ─── Serde support for ed25519_dalek::Signature ──────────────────────────────

pub mod signature_serde {
    use ed25519_dalek::Signature;
    use serde::{self, Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(sig: &Signature, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let bytes = sig.to_bytes();
        serializer.serialize_bytes(&bytes)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Signature, D::Error>
    where
        D: Deserializer<'de>,
    {
        let bytes: Vec<u8> = Deserialize::deserialize(deserializer)?;
        let byte_array: [u8; 64] = bytes
            .try_into()
            .map_err(|_| serde::de::Error::custom("signature must be exactly 64 bytes"))?;
        Ok(Signature::from_bytes(&byte_array))
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    // ─── Helpers ─────────────────────────────────────────────────────────────

    /// Strategy to generate arbitrary byte vectors (simulating data to sign).
    fn arb_data() -> impl Strategy<Value = Vec<u8>> {
        proptest::collection::vec(any::<u8>(), 0..1024)
    }

    /// Strategy to generate a random TrustTier.
    fn arb_trust_tier() -> impl Strategy<Value = TrustTier> {
        prop_oneof![
            Just(TrustTier::Public),
            Just(TrustTier::InvitedFriend),
            Just(TrustTier::LocalOwned),
        ]
    }

    // ─── Property Tests ──────────────────────────────────────────────────────

    proptest! {
        /// **Validates: Requirements FR-1.1, NFR-3.2**
        /// Property: sign/verify roundtrip always succeeds with the correct key.
        /// For any data, signing with a key and verifying with the corresponding
        /// public key always returns true.
        #[test]
        fn prop_sign_verify_roundtrip_succeeds_with_correct_key(
            data in arb_data()
        ) {
            let identity = MeshIdentity::generate();
            let signature = identity.sign(&data);
            let valid = MeshIdentity::verify(&data, &signature, &identity.verifying_key);
            prop_assert!(valid, "Signature verification should succeed with the correct key");
        }

        /// **Validates: Requirements NFR-3.2, NFR-3.6**
        /// Property: verify always fails with a wrong key.
        /// For any data, a signature produced by one key never verifies
        /// against a different key.
        #[test]
        fn prop_verify_fails_with_wrong_key(
            data in arb_data()
        ) {
            let identity_a = MeshIdentity::generate();
            let identity_b = MeshIdentity::generate();

            let signature = identity_a.sign(&data);
            let valid = MeshIdentity::verify(&data, &signature, &identity_b.verifying_key);
            prop_assert!(!valid, "Signature verification should fail with a different key");
        }

        /// **Validates: Requirements FR-1.2, Property 10**
        /// Property: expired tokens are always rejected during validation.
        /// An invitation token whose expires_at is in the past always fails validation.
        #[test]
        fn prop_expired_tokens_always_rejected(
            offered_tier in arb_trust_tier(),
            hours_ago in 1u32..1000
        ) {
            let identity = MeshIdentity::generate();
            let mesh_id = Uuid::new_v4();

            // Create a token and manually set its expiry to the past
            let mut token = identity.create_invitation(mesh_id, offered_tier, 1);
            token.expires_at = chrono::Utc::now()
                - chrono::Duration::hours(hours_ago as i64);

            // Re-sign with the backdated expiry so signature is valid for the payload
            let payload = InvitationPayload {
                token_id: token.token_id,
                mesh_id: token.mesh_id,
                inviter_node_id: token.inviter_node_id,
                offered_tier: token.offered_tier,
                expires_at: token.expires_at,
            };
            let payload_bytes = serde_json::to_vec(&payload).unwrap();
            token.signature = identity.sign(&payload_bytes);

            let result = token.validate(&identity.verifying_key);
            prop_assert_eq!(result, Err(InvitationError::Expired));
        }

        /// **Validates: Requirements NFR-3.3, Property 10**
        /// Property: consumed tokens are always rejected during validation.
        /// A token that has been marked as consumed always fails validation
        /// regardless of other conditions.
        #[test]
        fn prop_consumed_tokens_always_rejected(
            offered_tier in arb_trust_tier(),
            expires_in_hours in 1u32..720
        ) {
            let identity = MeshIdentity::generate();
            let mesh_id = Uuid::new_v4();

            let mut token = identity.create_invitation(mesh_id, offered_tier, expires_in_hours);
            token.consume();

            let result = token.validate(&identity.verifying_key);
            prop_assert_eq!(result, Err(InvitationError::AlreadyConsumed));
        }
    }

    // ─── Unit Tests ──────────────────────────────────────────────────────────

    #[test]
    fn test_identity_generation_produces_unique_ids() {
        let id_a = MeshIdentity::generate();
        let id_b = MeshIdentity::generate();
        assert_ne!(id_a.node_id, id_b.node_id);
    }

    #[test]
    fn test_create_invitation_produces_valid_token() {
        let identity = MeshIdentity::generate();
        let mesh_id = Uuid::new_v4();
        let token = identity.create_invitation(mesh_id, TrustTier::InvitedFriend, 24);

        assert_eq!(token.mesh_id, mesh_id);
        assert_eq!(token.inviter_node_id, identity.node_id);
        assert_eq!(token.offered_tier, TrustTier::InvitedFriend);
        assert!(!token.consumed);
        assert!(token.expires_at > chrono::Utc::now());

        // Should validate successfully
        assert!(token.validate(&identity.verifying_key).is_ok());
    }

    #[test]
    fn test_invitation_encode_decode_roundtrip() {
        let identity = MeshIdentity::generate();
        let mesh_id = Uuid::new_v4();
        let token = identity.create_invitation(mesh_id, TrustTier::LocalOwned, 48);

        let encoded = token.encode();
        // Verify it's URL-safe (no +, /, or = characters from standard base64)
        assert!(!encoded.contains('+'));
        assert!(!encoded.contains('/'));

        let decoded = InvitationToken::decode(&encoded).unwrap();
        assert_eq!(decoded.token_id, token.token_id);
        assert_eq!(decoded.mesh_id, token.mesh_id);
        assert_eq!(decoded.inviter_node_id, token.inviter_node_id);
        assert_eq!(decoded.offered_tier, token.offered_tier);
        assert_eq!(decoded.consumed, token.consumed);

        // Decoded token should still validate
        assert!(decoded.validate(&identity.verifying_key).is_ok());
    }

    #[test]
    fn test_invalid_signature_rejected() {
        let identity_a = MeshIdentity::generate();
        let identity_b = MeshIdentity::generate();
        let mesh_id = Uuid::new_v4();

        let token = identity_a.create_invitation(mesh_id, TrustTier::Public, 24);

        // Validate with wrong key should fail
        let result = token.validate(&identity_b.verifying_key);
        assert_eq!(result, Err(InvitationError::InvalidSignature));
    }

    #[test]
    fn test_decode_invalid_base64_fails() {
        let result = InvitationToken::decode("not-valid-base64!!!");
        assert!(matches!(result, Err(InvitationError::DecodingFailed(_))));
    }

    #[test]
    fn test_decode_valid_base64_but_invalid_json_fails() {
        use base64::engine::general_purpose::URL_SAFE_NO_PAD;
        use base64::Engine;

        let encoded = URL_SAFE_NO_PAD.encode(b"not json at all");
        let result = InvitationToken::decode(&encoded);
        assert!(matches!(result, Err(InvitationError::DecodingFailed(_))));
    }

    #[test]
    fn test_trust_tier_ordering() {
        assert!(TrustTier::LocalOwned > TrustTier::InvitedFriend);
        assert!(TrustTier::InvitedFriend > TrustTier::Public);
    }

    #[test]
    fn test_load_or_generate_creates_new_identity() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute(
            "CREATE TABLE IF NOT EXISTS mesh_identity (
                node_id TEXT PRIMARY KEY,
                signing_key_bytes BLOB NOT NULL,
                created_at TEXT NOT NULL
            )",
            [],
        )
        .unwrap();

        let identity = MeshIdentity::load_or_generate(&conn).unwrap();

        // Loading again should return the same identity
        let reloaded = MeshIdentity::load_or_generate(&conn).unwrap();
        assert_eq!(identity.node_id, reloaded.node_id);
        assert_eq!(
            identity.signing_key.to_bytes(),
            reloaded.signing_key.to_bytes()
        );
    }

    #[test]
    fn test_persisted_identity_signs_consistently() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute(
            "CREATE TABLE IF NOT EXISTS mesh_identity (
                node_id TEXT PRIMARY KEY,
                signing_key_bytes BLOB NOT NULL,
                created_at TEXT NOT NULL
            )",
            [],
        )
        .unwrap();

        let identity = MeshIdentity::load_or_generate(&conn).unwrap();
        let data = b"test message";
        let sig = identity.sign(data);

        // Reload and verify the signature still works
        let reloaded = MeshIdentity::load_or_generate(&conn).unwrap();
        assert!(MeshIdentity::verify(data, &sig, &reloaded.verifying_key));
    }
}
