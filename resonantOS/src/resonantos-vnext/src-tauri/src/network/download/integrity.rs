// Intent citation: .kiro/specs/model-download-engine/design.md — IntegrityVerifier
// Streaming SHA256 integrity verification for downloaded files.

use sha2::{Digest, Sha256};

/// Streaming SHA256 integrity verifier.
/// Computes the hash incrementally as data arrives, avoiding a second pass over the file.
pub struct IntegrityVerifier {
    hasher: Sha256,
}

impl IntegrityVerifier {
    /// Create a new verifier with a fresh SHA256 hasher.
    pub fn new() -> Self {
        Self {
            hasher: Sha256::new(),
        }
    }

    /// Feed bytes into the hasher incrementally.
    pub fn update(&mut self, data: &[u8]) {
        self.hasher.update(data);
    }

    /// Finalize the hash and return the hex-encoded SHA256 digest.
    /// Consumes the verifier (cannot be reused after finalization).
    pub fn finalize(self) -> String {
        let result = self.hasher.finalize();
        hex_encode(&result)
    }

    /// Constant-time comparison of computed vs expected hash.
    /// Both are compared in lowercase to handle mixed-case inputs.
    pub fn verify(computed: &str, expected: &str) -> bool {
        let computed_lower = computed.to_lowercase();
        let expected_lower = expected.to_lowercase();

        if computed_lower.len() != expected_lower.len() {
            return false;
        }

        // Constant-time comparison to prevent timing attacks
        let mut diff = 0u8;
        for (a, b) in computed_lower.bytes().zip(expected_lower.bytes()) {
            diff |= a ^ b;
        }
        diff == 0
    }
}

impl Default for IntegrityVerifier {
    fn default() -> Self {
        Self::new()
    }
}

/// Encode bytes as lowercase hex string.
fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_input_hash() {
        let verifier = IntegrityVerifier::new();
        let hash = verifier.finalize();
        // SHA256 of empty input
        assert_eq!(
            hash,
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn test_known_hash() {
        let mut verifier = IntegrityVerifier::new();
        verifier.update(b"hello world");
        let hash = verifier.finalize();
        assert_eq!(
            hash,
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
    }

    #[test]
    fn test_streaming_equals_batch() {
        let data = b"the quick brown fox jumps over the lazy dog";

        // Batch
        let mut batch_verifier = IntegrityVerifier::new();
        batch_verifier.update(data);
        let batch_hash = batch_verifier.finalize();

        // Streaming (chunk by chunk)
        let mut stream_verifier = IntegrityVerifier::new();
        for chunk in data.chunks(5) {
            stream_verifier.update(chunk);
        }
        let stream_hash = stream_verifier.finalize();

        assert_eq!(batch_hash, stream_hash);
    }

    #[test]
    fn test_verify_matching() {
        let hash = "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9";
        assert!(IntegrityVerifier::verify(hash, hash));
    }

    #[test]
    fn test_verify_case_insensitive() {
        let lower = "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9";
        let upper = "B94D27B9934D3E08A52E52D7DA7DABFAC484EFE37A5380EE9088F7ACE2EFCDE9";
        assert!(IntegrityVerifier::verify(lower, upper));
    }

    #[test]
    fn test_verify_mismatch() {
        let hash_a = "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9";
        let hash_b = "0000000000000000000000000000000000000000000000000000000000000000";
        assert!(!IntegrityVerifier::verify(hash_a, hash_b));
    }

    #[test]
    fn test_verify_different_lengths() {
        assert!(!IntegrityVerifier::verify("abc", "abcd"));
        assert!(!IntegrityVerifier::verify("abcd", "abc"));
    }
}

#[cfg(test)]
mod property_tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        /// **Validates: Requirements 4.1, 4.2**
        /// Property 4: Integrity Guarantee — for any byte sequence, streaming hash
        /// equals batch hash; mismatched expected hash always fails verification.
        #[test]
        fn streaming_hash_equals_batch_hash(data in proptest::collection::vec(any::<u8>(), 0..4096)) {
            // Batch: feed all at once
            let mut batch = IntegrityVerifier::new();
            batch.update(&data);
            let batch_hash = batch.finalize();

            // Streaming: feed in random-sized chunks
            let mut stream = IntegrityVerifier::new();
            for chunk in data.chunks(7) { // Arbitrary chunk size
                stream.update(chunk);
            }
            let stream_hash = stream.finalize();

            // Streaming must equal batch
            prop_assert_eq!(&batch_hash, &stream_hash);

            // Verify: matching hash passes
            prop_assert!(IntegrityVerifier::verify(&batch_hash, &stream_hash));

            // Verify: mismatched hash always fails
            let wrong_hash = "0000000000000000000000000000000000000000000000000000000000000000";
            if batch_hash != wrong_hash {
                prop_assert!(!IntegrityVerifier::verify(&batch_hash, wrong_hash));
            }
        }
    }
}
