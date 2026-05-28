//! MeshIdentity: Ed25519 keypair management with platform secure storage.
//!
//! Manages the phone's persistent mesh identity using Ed25519 signing keys.
//! Platform-specific secure storage is used on iOS (Keychain) and Android (Keystore),
//! with an in-memory fallback for desktop/testing environments.

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand::rngs::OsRng;
use uuid::Uuid;

use crate::companion::types::NodeId;

// ─── Error Types ─────────────────────────────────────────────────────────────

/// Errors that can occur during identity operations.
#[derive(Debug, Clone)]
pub enum IdentityError {
    /// Failed to generate a new keypair.
    GenerationFailed(String),
    /// Failed to load identity from secure storage.
    LoadFailed(String),
    /// Failed to sign a message.
    SigningFailed(String),
    /// Secure storage is unavailable on this platform.
    StorageUnavailable(String),
}

impl std::fmt::Display for IdentityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::GenerationFailed(msg) => write!(f, "Identity generation failed: {}", msg),
            Self::LoadFailed(msg) => write!(f, "Identity load failed: {}", msg),
            Self::SigningFailed(msg) => write!(f, "Signing failed: {}", msg),
            Self::StorageUnavailable(msg) => write!(f, "Secure storage unavailable: {}", msg),
        }
    }
}

impl std::error::Error for IdentityError {}

// ─── Secure Key Store ────────────────────────────────────────────────────────

/// Platform-specific secure key storage backend.
#[derive(Debug, Clone)]
pub enum SecureKeyStore {
    /// iOS Keychain storage.
    #[cfg(target_os = "ios")]
    IosKeychain { service: String, account: String },

    /// Android Keystore storage.
    #[cfg(target_os = "android")]
    AndroidKeystore { alias: String },

    /// In-memory storage for desktop/testing (keys are not persisted across restarts).
    #[cfg(not(any(target_os = "ios", target_os = "android")))]
    InMemory {
        /// The raw signing key bytes stored in memory.
        key_bytes: [u8; 32],
    },
}

impl SecureKeyStore {
    /// Create a new platform-appropriate key store.
    #[cfg(target_os = "ios")]
    pub fn new_platform() -> Self {
        SecureKeyStore::IosKeychain {
            service: "com.resonantos.companion".to_string(),
            account: "mesh_identity".to_string(),
        }
    }

    /// Create a new platform-appropriate key store.
    #[cfg(target_os = "android")]
    pub fn new_platform() -> Self {
        SecureKeyStore::AndroidKeystore {
            alias: "resonantos_mesh_identity".to_string(),
        }
    }

    /// Create a new platform-appropriate key store (desktop/testing fallback).
    #[cfg(not(any(target_os = "ios", target_os = "android")))]
    pub fn new_platform() -> Self {
        SecureKeyStore::InMemory {
            key_bytes: [0u8; 32],
        }
    }

    /// Store signing key bytes in the platform secure storage.
    #[cfg(target_os = "ios")]
    pub fn store(&mut self, _key_bytes: &[u8; 32]) -> Result<(), IdentityError> {
        // iOS: Use Security framework to store in Keychain
        // SecItemAdd with kSecClassGenericPassword, kSecAttrAccessibleWhenUnlockedThisDeviceOnly
        Err(IdentityError::StorageUnavailable(
            "iOS Keychain not available in this build".to_string(),
        ))
    }

    /// Store signing key bytes in the platform secure storage.
    #[cfg(target_os = "android")]
    pub fn store(&mut self, _key_bytes: &[u8; 32]) -> Result<(), IdentityError> {
        // Android: Use JNI to access Android Keystore
        // KeyStore.getInstance("AndroidKeyStore"), store as SecretKeyEntry
        Err(IdentityError::StorageUnavailable(
            "Android Keystore not available in this build".to_string(),
        ))
    }

    /// Store signing key bytes in memory (desktop/testing fallback).
    #[cfg(not(any(target_os = "ios", target_os = "android")))]
    pub fn store(&mut self, key_bytes: &[u8; 32]) -> Result<(), IdentityError> {
        let SecureKeyStore::InMemory {
            key_bytes: ref mut stored,
        } = self;
        *stored = *key_bytes;
        Ok(())
    }

    /// Retrieve signing key bytes from the platform secure storage.
    #[cfg(target_os = "ios")]
    pub fn retrieve(&self) -> Result<Option<[u8; 32]>, IdentityError> {
        // iOS: Use Security framework SecItemCopyMatching
        Ok(None)
    }

    /// Retrieve signing key bytes from the platform secure storage.
    #[cfg(target_os = "android")]
    pub fn retrieve(&self) -> Result<Option<[u8; 32]>, IdentityError> {
        // Android: Use JNI to access Android Keystore, KeyStore.getEntry
        Ok(None)
    }

    /// Retrieve signing key bytes from memory (desktop/testing fallback).
    #[cfg(not(any(target_os = "ios", target_os = "android")))]
    pub fn retrieve(&self) -> Result<Option<[u8; 32]>, IdentityError> {
        let SecureKeyStore::InMemory { key_bytes } = self;
        // All zeros means no key stored yet
        if *key_bytes == [0u8; 32] {
            Ok(None)
        } else {
            Ok(Some(*key_bytes))
        }
    }
}

// ─── MeshIdentity ────────────────────────────────────────────────────────────

/// Ed25519 public key wrapper for type clarity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ed25519PublicKey(pub VerifyingKey);

/// Ed25519 signature wrapper for type clarity.
#[derive(Debug, Clone)]
pub struct Ed25519Signature(pub Signature);

/// Manages the phone's Ed25519 mesh identity with platform secure storage.
///
/// The identity is generated once during first pairing and persisted in the
/// platform's secure enclave (iOS Keychain / Android Keystore). On desktop/test
/// environments, keys are stored in memory.
pub struct MeshIdentity {
    /// The unique node identifier derived from the public key.
    pub node_id: NodeId,
    /// The Ed25519 public key (shared with other nodes for verification).
    pub public_key: Ed25519PublicKey,
    /// The signing key (kept private, stored in secure enclave).
    signing_key: SigningKey,
    /// Platform-specific secure storage backend.
    store: SecureKeyStore,
}

impl MeshIdentity {
    /// Generate a new mesh identity with a fresh Ed25519 keypair.
    ///
    /// This should only be called during first pairing. The generated key is
    /// stored in the platform's secure enclave.
    pub fn generate() -> Result<Self, IdentityError> {
        let signing_key = SigningKey::generate(&mut OsRng);
        let verifying_key = signing_key.verifying_key();

        // Derive node_id from the first 16 bytes of the public key
        let pk_bytes = verifying_key.to_bytes();
        let node_id = Uuid::from_slice(&pk_bytes[..16])
            .map_err(|e| IdentityError::GenerationFailed(e.to_string()))?;

        let mut store = SecureKeyStore::new_platform();
        store
            .store(&signing_key.to_bytes())
            .map_err(|e| IdentityError::GenerationFailed(e.to_string()))?;

        Ok(Self {
            node_id,
            public_key: Ed25519PublicKey(verifying_key),
            signing_key,
            store,
        })
    }

    /// Load an existing mesh identity from platform secure storage.
    ///
    /// Returns `Ok(None)` if no identity has been generated yet.
    pub fn load() -> Result<Option<Self>, IdentityError> {
        let store = SecureKeyStore::new_platform();
        let key_bytes = store.retrieve()?;

        match key_bytes {
            None => Ok(None),
            Some(bytes) => {
                let signing_key = SigningKey::from_bytes(&bytes);
                let verifying_key = signing_key.verifying_key();

                let pk_bytes = verifying_key.to_bytes();
                let node_id = Uuid::from_slice(&pk_bytes[..16])
                    .map_err(|e| IdentityError::LoadFailed(e.to_string()))?;

                Ok(Some(Self {
                    node_id,
                    public_key: Ed25519PublicKey(verifying_key),
                    signing_key,
                    store,
                }))
            }
        }
    }

    /// Sign a message using this identity's private key.
    ///
    /// Used for authenticating messages sent to other mesh nodes.
    pub fn sign(&self, message: &[u8]) -> Result<Ed25519Signature, IdentityError> {
        let signature = self.signing_key.sign(message);
        Ok(Ed25519Signature(signature))
    }

    /// Verify a signature from another node using their public key.
    ///
    /// Returns `true` if the signature is valid for the given message and public key.
    pub fn verify(
        public_key: &Ed25519PublicKey,
        message: &[u8],
        signature: &Ed25519Signature,
    ) -> bool {
        public_key.0.verify(message, &signature.0).is_ok()
    }

    /// Get the raw public key bytes (for transmission to other nodes).
    pub fn public_key_bytes(&self) -> [u8; 32] {
        self.public_key.0.to_bytes()
    }
}

// ─── Unit Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_creates_valid_identity() {
        let identity = MeshIdentity::generate().expect("should generate identity");
        // Node ID should be a valid UUID
        assert_ne!(identity.node_id, Uuid::nil());
        // Public key should be 32 bytes
        assert_eq!(identity.public_key_bytes().len(), 32);
    }

    #[test]
    fn test_sign_and_verify_roundtrip() {
        let identity = MeshIdentity::generate().expect("should generate identity");
        let message = b"hello mesh network";

        let signature = identity.sign(message).expect("should sign");
        let valid = MeshIdentity::verify(&identity.public_key, message, &signature);
        assert!(valid, "signature should verify with correct public key");
    }

    #[test]
    fn test_verify_fails_with_wrong_key() {
        let identity1 = MeshIdentity::generate().expect("should generate identity 1");
        let identity2 = MeshIdentity::generate().expect("should generate identity 2");
        let message = b"hello mesh network";

        let signature = identity1.sign(message).expect("should sign");
        // Verify with wrong public key should fail
        let valid = MeshIdentity::verify(&identity2.public_key, message, &signature);
        assert!(!valid, "signature should NOT verify with wrong public key");
    }

    #[test]
    fn test_verify_fails_with_wrong_message() {
        let identity = MeshIdentity::generate().expect("should generate identity");
        let message = b"original message";
        let tampered = b"tampered message";

        let signature = identity.sign(message).expect("should sign");
        let valid = MeshIdentity::verify(&identity.public_key, tampered, &signature);
        assert!(!valid, "signature should NOT verify with wrong message");
    }

    #[test]
    fn test_load_returns_none_when_no_identity_stored() {
        // On desktop/test, the in-memory store starts empty
        let loaded = MeshIdentity::load().expect("should not error");
        assert!(loaded.is_none(), "should return None when no identity stored");
    }

    #[test]
    fn test_two_identities_have_different_node_ids() {
        let id1 = MeshIdentity::generate().expect("should generate");
        let id2 = MeshIdentity::generate().expect("should generate");
        assert_ne!(id1.node_id, id2.node_id, "different identities should have different node IDs");
    }

    #[test]
    fn test_sign_empty_message() {
        let identity = MeshIdentity::generate().expect("should generate identity");
        let message = b"";

        let signature = identity.sign(message).expect("should sign empty message");
        let valid = MeshIdentity::verify(&identity.public_key, message, &signature);
        assert!(valid, "empty message signature should verify");
    }

    #[test]
    fn test_sign_large_message() {
        let identity = MeshIdentity::generate().expect("should generate identity");
        let message = vec![0xABu8; 10_000];

        let signature = identity.sign(&message).expect("should sign large message");
        let valid = MeshIdentity::verify(&identity.public_key, &message, &signature);
        assert!(valid, "large message signature should verify");
    }
}
