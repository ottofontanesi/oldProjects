// Mock transport manager for integration tests.
//
// Captures all sent messages, supports failure injection per-node.

use super::mock_node::NodeId;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

/// A captured message from the mock transport.
#[derive(Debug, Clone)]
pub struct CapturedMessage {
    pub source: NodeId,
    pub target: NodeId,
    pub payload: Vec<u8>,
    pub channel: String,
}

/// Mock transport error.
#[derive(Debug, Clone, PartialEq)]
pub enum MockTransportError {
    Unreachable { target: NodeId },
    Timeout,
}

/// Mock transport manager implementing message capture and failure injection.
pub struct MockTransportManager {
    pub latency_ms: f64,
    pub failure_rate: f64,
    pub messages: Arc<Mutex<Vec<CapturedMessage>>>,
    pub failed_nodes: Arc<Mutex<HashSet<NodeId>>>,
    pub secondary_available: bool,
}

impl MockTransportManager {
    pub fn new() -> Self {
        Self {
            latency_ms: 5.0,
            failure_rate: 0.0,
            messages: Arc::new(Mutex::new(Vec::new())),
            failed_nodes: Arc::new(Mutex::new(HashSet::new())),
            secondary_available: true,
        }
    }

    /// Send a message to a target node.
    /// Returns error if the target is in the failed_nodes set.
    pub fn send(
        &self,
        source: NodeId,
        target: NodeId,
        payload: Vec<u8>,
        channel: &str,
    ) -> Result<(), MockTransportError> {
        let failed = self.failed_nodes.lock().unwrap();
        if failed.contains(&target) {
            // Try secondary if available
            if self.secondary_available && !failed.contains(&source) {
                // Secondary path succeeds — record with "secondary" channel
                drop(failed);
                let msg = CapturedMessage {
                    source,
                    target,
                    payload,
                    channel: format!("{}_secondary", channel),
                };
                self.messages.lock().unwrap().push(msg);
                return Ok(());
            }
            return Err(MockTransportError::Unreachable { target });
        }
        drop(failed);

        let msg = CapturedMessage {
            source,
            target,
            payload,
            channel: channel.to_string(),
        };
        self.messages.lock().unwrap().push(msg);
        Ok(())
    }

    /// Inject a transport failure for a specific node.
    pub fn inject_failure(&self, node_id: NodeId) {
        self.failed_nodes.lock().unwrap().insert(node_id);
    }

    /// Recover a previously failed node.
    pub fn recover(&self, node_id: NodeId) {
        self.failed_nodes.lock().unwrap().remove(&node_id);
    }

    /// Get all captured messages.
    pub fn captured_messages(&self) -> Vec<CapturedMessage> {
        self.messages.lock().unwrap().clone()
    }

    /// Clear captured messages.
    pub fn clear_messages(&self) {
        self.messages.lock().unwrap().clear();
    }

    /// Check if a node is currently failed.
    pub fn is_failed(&self, node_id: &NodeId) -> bool {
        self.failed_nodes.lock().unwrap().contains(node_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn test_send_captures_message() {
        let transport = MockTransportManager::new();
        let src = Uuid::new_v4();
        let dst = Uuid::new_v4();

        let result = transport.send(src, dst, vec![1, 2, 3], "test");
        assert!(result.is_ok());
        assert_eq!(transport.captured_messages().len(), 1);
        assert_eq!(transport.captured_messages()[0].channel, "test");
    }

    #[test]
    fn test_failed_node_returns_error_or_secondary() {
        let transport = MockTransportManager::new();
        let src = Uuid::new_v4();
        let dst = Uuid::new_v4();

        transport.inject_failure(dst);

        // With secondary available, should succeed via secondary
        let result = transport.send(src, dst, vec![1], "test");
        assert!(result.is_ok());
        let msgs = transport.captured_messages();
        assert_eq!(msgs[0].channel, "test_secondary");
    }

    #[test]
    fn test_failed_node_no_secondary() {
        let mut transport = MockTransportManager::new();
        transport.secondary_available = false;
        let src = Uuid::new_v4();
        let dst = Uuid::new_v4();

        transport.inject_failure(dst);
        let result = transport.send(src, dst, vec![1], "test");
        assert!(matches!(result, Err(MockTransportError::Unreachable { .. })));
    }

    #[test]
    fn test_recover_allows_send() {
        let mut transport = MockTransportManager::new();
        transport.secondary_available = false;
        let src = Uuid::new_v4();
        let dst = Uuid::new_v4();

        transport.inject_failure(dst);
        assert!(transport.send(src, dst, vec![1], "test").is_err());

        transport.recover(dst);
        assert!(transport.send(src, dst, vec![1], "test").is_ok());
    }
}
