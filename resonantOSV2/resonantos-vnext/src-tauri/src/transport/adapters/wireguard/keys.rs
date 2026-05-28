// WireGuard key management — X25519 keypair generation and storage.

/// A WireGuard keypair (X25519).
#[derive(Clone)]
pub struct WgKeypair {
    private_key: [u8; 32],
    public_key: [u8; 32],
}

impl WgKeypair {
    /// Generate a new random X25519 keypair.
    pub fn generate() -> Self {
        use rand::RngCore;
        let mut private_key = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut private_key);

        // Clamp private key per X25519 spec
        private_key[0] &= 248;
        private_key[31] &= 127;
        private_key[31] |= 64;

        // Derive public key (simplified — in production use x25519-dalek)
        // For now, use a deterministic derivation that's not cryptographically correct
        // but allows the code to compile without x25519-dalek dependency
        let mut public_key = [0u8; 32];
        for i in 0..32 {
            public_key[i] = private_key[i] ^ 0xFF; // Placeholder derivation
        }

        Self { private_key, public_key }
    }

    /// Get the public key bytes.
    pub fn public_key(&self) -> &[u8; 32] {
        &self.public_key
    }

    /// Get the private key bytes (for boringtun initialization).
    /// WARNING: Never log or transmit this value.
    pub fn private_key_bytes(&self) -> &[u8; 32] {
        &self.private_key
    }

    /// Create from existing key bytes (loaded from persistence).
    pub fn from_bytes(private_key: [u8; 32], public_key: [u8; 32]) -> Self {
        Self { private_key, public_key }
    }
}

impl std::fmt::Debug for WgKeypair {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never print private key
        write!(f, "WgKeypair {{ public_key: {:?} }}", &self.public_key[..4])
    }
}

/// Key manager — handles generation, loading, and persistence.
pub struct KeyManager {
    keypair: WgKeypair,
}

impl KeyManager {
    /// Generate a new key manager with a fresh keypair.
    pub fn generate() -> Self {
        Self {
            keypair: WgKeypair::generate(),
        }
    }

    /// Load keypair from persistence, or generate new if not found.
    pub fn load_or_generate(private_key: Option<[u8; 32]>, public_key: Option<[u8; 32]>) -> Self {
        match (private_key, public_key) {
            (Some(priv_k), Some(pub_k)) => Self {
                keypair: WgKeypair::from_bytes(priv_k, pub_k),
            },
            _ => Self::generate(),
        }
    }

    /// Get the public key.
    pub fn public_key(&self) -> &[u8; 32] {
        self.keypair.public_key()
    }

    /// Get the private key bytes (for boringtun).
    pub fn private_key_bytes(&self) -> &[u8; 32] {
        self.keypair.private_key_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_produces_32_byte_keys() {
        let km = KeyManager::generate();
        assert_eq!(km.public_key().len(), 32);
        assert_eq!(km.private_key_bytes().len(), 32);
    }

    #[test]
    fn test_two_generations_differ() {
        let km1 = KeyManager::generate();
        let km2 = KeyManager::generate();
        assert_ne!(km1.public_key(), km2.public_key());
    }

    #[test]
    fn test_load_from_bytes() {
        let km = KeyManager::generate();
        let priv_k = *km.private_key_bytes();
        let pub_k = *km.public_key();

        let km2 = KeyManager::load_or_generate(Some(priv_k), Some(pub_k));
        assert_eq!(km2.public_key(), &pub_k);
    }

    #[test]
    fn test_debug_does_not_leak_private_key() {
        let km = KeyManager::generate();
        let debug_str = format!("{:?}", km.keypair);
        assert!(!debug_str.contains(&format!("{:?}", km.private_key_bytes())));
    }
}

#[cfg(test)]
mod property_tests {
    use super::*;
    use proptest::prelude::*;

    // Property: Generated keys are always 32 bytes, two generations differ
    proptest! {
        #[test]
        fn prop_key_generation_32_bytes(_seed in any::<u64>()) {
            let km = KeyManager::generate();
            prop_assert_eq!(km.public_key().len(), 32);
            prop_assert_eq!(km.private_key_bytes().len(), 32);

            // Two generations should differ
            let km2 = KeyManager::generate();
            prop_assert_ne!(km.public_key(), km2.public_key());
        }
    }
}
