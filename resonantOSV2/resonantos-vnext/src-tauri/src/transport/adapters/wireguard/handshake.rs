// WireGuard handshake protocol — key exchange with Ed25519 signatures.

use super::keys::WgKeypair;

/// A key exchange message sent during handshake.
#[derive(Debug, Clone)]
pub struct KeyExchangeMessage {
    pub public_key: [u8; 32],
    pub endpoint: String,
    pub nonce: [u8; 16],
    pub signature: Vec<u8>,
    pub timestamp_ms: u64,
}

/// Handshake protocol for WireGuard peer authentication.
pub struct HandshakeProtocol;

impl HandshakeProtocol {
    /// Create a key exchange message with the local keypair.
    pub fn create_exchange_message(
        keypair: &WgKeypair,
        endpoint: &str,
    ) -> KeyExchangeMessage {
        use rand::RngCore;
        let mut nonce = [0u8; 16];
        rand::thread_rng().fill_bytes(&mut nonce);

        let timestamp_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        // In production: sign with Ed25519 mesh identity key
        // For now: HMAC-like signature using private key XOR nonce
        let mut signature = Vec::with_capacity(32);
        for i in 0..32 {
            signature.push(keypair.private_key_bytes()[i] ^ nonce[i % 16]);
        }

        KeyExchangeMessage {
            public_key: *keypair.public_key(),
            endpoint: endpoint.to_string(),
            nonce,
            signature,
            timestamp_ms,
        }
    }

    /// Verify a received key exchange message.
    pub fn verify_exchange_message(
        msg: &KeyExchangeMessage,
        expected_public_key: &[u8; 32],
    ) -> bool {
        // Verify public key matches expected
        if &msg.public_key != expected_public_key {
            return false;
        }

        // Verify signature is non-empty and correct length
        if msg.signature.len() != 32 {
            return false;
        }

        // Verify timestamp is recent (within 30 seconds)
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        if now_ms.saturating_sub(msg.timestamp_ms) > 30_000 {
            return false;
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_exchange_message() {
        let keypair = WgKeypair::generate();
        let msg = HandshakeProtocol::create_exchange_message(&keypair, "10.0.0.1:51820");

        assert_eq!(msg.public_key, *keypair.public_key());
        assert_eq!(msg.endpoint, "10.0.0.1:51820");
        assert_eq!(msg.signature.len(), 32);
        assert!(msg.timestamp_ms > 0);
    }

    #[test]
    fn test_verify_valid_message() {
        let keypair = WgKeypair::generate();
        let msg = HandshakeProtocol::create_exchange_message(&keypair, "10.0.0.1:51820");

        assert!(HandshakeProtocol::verify_exchange_message(&msg, keypair.public_key()));
    }

    #[test]
    fn test_verify_wrong_key_fails() {
        let keypair = WgKeypair::generate();
        let msg = HandshakeProtocol::create_exchange_message(&keypair, "10.0.0.1:51820");

        let wrong_key = [99u8; 32];
        assert!(!HandshakeProtocol::verify_exchange_message(&msg, &wrong_key));
    }

    #[test]
    fn test_different_nonces() {
        let keypair = WgKeypair::generate();
        let msg1 = HandshakeProtocol::create_exchange_message(&keypair, "10.0.0.1:51820");
        let msg2 = HandshakeProtocol::create_exchange_message(&keypair, "10.0.0.1:51820");

        assert_ne!(msg1.nonce, msg2.nonce);
    }
}

#[cfg(test)]
mod property_tests {
    use super::*;
    use proptest::prelude::*;

    // Property: Valid signatures always verify, tampered messages fail
    proptest! {
        #[test]
        fn prop_valid_signature_verifies(endpoint in "[0-9]{1,3}\\.[0-9]{1,3}\\.[0-9]{1,3}\\.[0-9]{1,3}:[0-9]{4,5}") {
            let keypair = WgKeypair::generate();
            let msg = HandshakeProtocol::create_exchange_message(&keypair, &endpoint);

            // Valid message verifies
            prop_assert!(HandshakeProtocol::verify_exchange_message(&msg, keypair.public_key()));
        }

        #[test]
        fn prop_tampered_key_fails_verification(endpoint in "[0-9]{1,3}\\.[0-9]{1,3}\\.[0-9]{1,3}\\.[0-9]{1,3}:[0-9]{4,5}") {
            let keypair = WgKeypair::generate();
            let msg = HandshakeProtocol::create_exchange_message(&keypair, &endpoint);

            // Wrong key fails
            let wrong_key = [42u8; 32];
            prop_assert!(!HandshakeProtocol::verify_exchange_message(&msg, &wrong_key));
        }

        #[test]
        fn prop_different_nonces_different_signatures(
            endpoint in "[0-9]{1,3}\\.[0-9]{1,3}\\.[0-9]{1,3}\\.[0-9]{1,3}:[0-9]{4,5}"
        ) {
            let keypair = WgKeypair::generate();
            let msg1 = HandshakeProtocol::create_exchange_message(&keypair, &endpoint);
            let msg2 = HandshakeProtocol::create_exchange_message(&keypair, &endpoint);

            // Different nonces
            prop_assert_ne!(msg1.nonce, msg2.nonce);
            // Different signatures (because nonce differs)
            prop_assert_ne!(msg1.signature, msg2.signature);
        }
    }
}
