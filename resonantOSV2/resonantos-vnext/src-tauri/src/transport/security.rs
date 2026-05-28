// Intent citation: .kiro/specs/unified-mesh-transport/design.md Section FR-8
// Security Layer — message padding, replay protection, identity verification

use super::trait_def::NodeId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Block size for message padding (prevents size-based metadata leakage).
pub const PADDING_BLOCK_SIZE: usize = 1024;

/// Pad a message payload to the next block boundary.
/// Prevents observers from inferring content type from message size.
pub fn pad_payload(payload: &[u8]) -> Vec<u8> {
    let current_len = payload.len();
    let padded_len = ((current_len / PADDING_BLOCK_SIZE) + 1) * PADDING_BLOCK_SIZE;
    let mut padded = payload.to_vec();
    padded.resize(padded_len, 0); // Zero-pad
    padded
}

/// Remove padding from a payload (find actual content length).
/// Assumes the last non-zero byte is the end of content.
/// For binary payloads that may contain zeros, a length prefix should be used instead.
pub fn unpad_payload(padded: &[u8], original_length: usize) -> Vec<u8> {
    padded[..original_length].to_vec()
}

/// Replay protection: tracks message sequence numbers per sender.
pub struct ReplayProtection {
    /// Last seen sequence number per sender.
    last_seen: HashMap<NodeId, u64>,
    /// Window of recently seen sequence numbers (for out-of-order tolerance).
    recent_window: HashMap<NodeId, Vec<u64>>,
    /// Window size for out-of-order tolerance.
    window_size: usize,
}

impl ReplayProtection {
    pub fn new(window_size: usize) -> Self {
        Self {
            last_seen: HashMap::new(),
            recent_window: HashMap::new(),
            window_size,
        }
    }

    /// Check if a message is a replay (already seen).
    /// Returns true if the message should be REJECTED (is a replay).
    pub fn is_replay(&self, sender: &NodeId, sequence: u64) -> bool {
        // Check if we've seen this exact sequence from this sender
        if let Some(window) = self.recent_window.get(sender) {
            if window.contains(&sequence) {
                return true; // Duplicate
            }
        }

        // Check if sequence is too old (below our window)
        if let Some(&last) = self.last_seen.get(sender) {
            if sequence <= last.saturating_sub(self.window_size as u64) {
                return true; // Too old, likely replay
            }
        }

        false
    }

    /// Record a message as seen (call after accepting it).
    pub fn record(&mut self, sender: NodeId, sequence: u64) {
        // Update last seen
        let last = self.last_seen.entry(sender).or_insert(0);
        if sequence > *last {
            *last = sequence;
        }

        // Add to recent window
        let window = self.recent_window.entry(sender).or_insert_with(Vec::new);
        window.push(sequence);

        // Trim window
        if window.len() > self.window_size {
            window.remove(0);
        }
    }

    /// Get the next expected sequence number for a sender.
    pub fn next_expected(&self, sender: &NodeId) -> u64 {
        self.last_seen.get(sender).map(|&s| s + 1).unwrap_or(1)
    }

    /// Reset state for a sender (e.g., they reconnected).
    pub fn reset_sender(&mut self, sender: &NodeId) {
        self.last_seen.remove(sender);
        self.recent_window.remove(sender);
    }
}

impl Default for ReplayProtection {
    fn default() -> Self {
        Self::new(100) // Window of 100 messages
    }
}

/// Sequence number generator for outgoing messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SequenceGenerator {
    /// Per-destination sequence counters.
    counters: HashMap<NodeId, u64>,
}

impl SequenceGenerator {
    pub fn new() -> Self {
        Self {
            counters: HashMap::new(),
        }
    }

    /// Get the next sequence number for a destination.
    pub fn next(&mut self, destination: &NodeId) -> u64 {
        let counter = self.counters.entry(*destination).or_insert(0);
        *counter += 1;
        *counter
    }
}

impl Default for SequenceGenerator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_padding() {
        let payload = vec![1, 2, 3, 4, 5]; // 5 bytes
        let padded = pad_payload(&payload);

        // Should be padded to next 1024 boundary
        assert_eq!(padded.len(), PADDING_BLOCK_SIZE);
        assert_eq!(&padded[..5], &[1, 2, 3, 4, 5]);
        assert!(padded[5..].iter().all(|&b| b == 0));
    }

    #[test]
    fn test_padding_exact_boundary() {
        let payload = vec![0u8; PADDING_BLOCK_SIZE]; // Exactly 1024 bytes
        let padded = pad_payload(&payload);

        // Should pad to NEXT boundary (2048), not stay at 1024
        assert_eq!(padded.len(), PADDING_BLOCK_SIZE * 2);
    }

    #[test]
    fn test_unpad() {
        let payload = vec![1, 2, 3, 4, 5];
        let padded = pad_payload(&payload);
        let unpadded = unpad_payload(&padded, 5);

        assert_eq!(unpadded, payload);
    }

    #[test]
    fn test_replay_detection() {
        let mut rp = ReplayProtection::default();
        let sender = uuid::Uuid::new_v4();

        // First message — not a replay
        assert!(!rp.is_replay(&sender, 1));
        rp.record(sender, 1);

        // Same sequence — replay!
        assert!(rp.is_replay(&sender, 1));

        // Next sequence — not a replay
        assert!(!rp.is_replay(&sender, 2));
        rp.record(sender, 2);
    }

    #[test]
    fn test_replay_out_of_order_tolerance() {
        let mut rp = ReplayProtection::new(10);
        let sender = uuid::Uuid::new_v4();

        // Record sequences 1-5
        for i in 1..=5 {
            rp.record(sender, i);
        }

        // Sequence 3 arrives again — replay
        assert!(rp.is_replay(&sender, 3));

        // Sequence 6 arrives (new) — not replay
        assert!(!rp.is_replay(&sender, 6));
    }

    #[test]
    fn test_replay_very_old_rejected() {
        let mut rp = ReplayProtection::new(10);
        let sender = uuid::Uuid::new_v4();

        // Record up to sequence 100
        for i in 1..=100 {
            rp.record(sender, i);
        }

        // Sequence 5 is way too old (below window of last 10)
        assert!(rp.is_replay(&sender, 5));

        // Sequence 101 is new
        assert!(!rp.is_replay(&sender, 101));
    }

    #[test]
    fn test_sequence_generator() {
        let mut gen = SequenceGenerator::new();
        let dest = uuid::Uuid::new_v4();

        assert_eq!(gen.next(&dest), 1);
        assert_eq!(gen.next(&dest), 2);
        assert_eq!(gen.next(&dest), 3);

        // Different destination starts at 1
        let dest2 = uuid::Uuid::new_v4();
        assert_eq!(gen.next(&dest2), 1);
    }

    #[test]
    fn test_reset_sender() {
        let mut rp = ReplayProtection::default();
        let sender = uuid::Uuid::new_v4();

        rp.record(sender, 1);
        rp.record(sender, 2);
        assert!(rp.is_replay(&sender, 1));

        rp.reset_sender(&sender);
        // After reset, sequence 1 is no longer considered replay
        assert!(!rp.is_replay(&sender, 1));
    }
}
