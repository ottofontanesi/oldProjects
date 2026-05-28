// Intent citation: .kiro/specs/unified-mesh-transport/design.md Section 2.1
// MeshTransport trait — the core abstraction all transport adapters implement

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Unique identifier for a transport adapter.
pub type TransportId = String;

/// Unique identifier for a node (same as network::registry::NodeId).
pub type NodeId = uuid::Uuid;

// ─── Message Types ───────────────────────────────────────────────────────────

/// Priority levels for transport messages.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum MessagePriority {
    /// Metrics, announcements — any available path.
    Low = 0,
    /// Requests, responses — normal delivery.
    Normal = 1,
    /// Inference activations, time-sensitive — lowest latency path.
    Critical = 2,
}

/// What kind of request this message carries (affects path selection).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum RequestType {
    /// Split inference activation forwarding — lowest latency.
    InferenceActivation,
    /// Normal inference request — low latency + sufficient bandwidth.
    InferenceRequest,
    /// Normal inference response.
    InferenceResponse,
    /// Large model file transfer — highest bandwidth.
    ModelTransfer,
    /// Heartbeat/keepalive — any path (cheapest).
    Heartbeat,
    /// Metric probe — any path.
    MetricProbe,
    /// KV-cache data — high bandwidth, moderate latency.
    KvCacheData,
    /// Broadcast announcement — any path.
    Announcement,
    /// Phase 15 extension: orchestrator → worker step dispatch.
    AgentStepDispatch,
    /// Phase 15 extension: worker → orchestrator step result.
    AgentStepResult,
    /// Phase 15 extension: inter-step data transfer.
    AgentStepData,
}

/// A message to be sent via the transport layer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TransportMessage {
    pub message_id: uuid::Uuid,
    pub priority: MessagePriority,
    pub request_type: RequestType,
    pub payload: Vec<u8>,
    pub payload_size: u64,
    pub created_at_ms: u64,
    /// Remaining hops (starts at 5, decremented on each relay).
    pub ttl_hops: u8,
    /// Nodes this message has already visited (for loop detection).
    pub visited_nodes: Vec<NodeId>,
}

impl TransportMessage {
    pub fn new(payload: Vec<u8>, priority: MessagePriority, request_type: RequestType) -> Self {
        let size = payload.len() as u64;
        Self {
            message_id: uuid::Uuid::new_v4(),
            priority,
            request_type,
            payload,
            payload_size: size,
            created_at_ms: 0, // Set by caller
            ttl_hops: 5,
            visited_nodes: Vec::new(),
        }
    }
}

/// An incoming message received from the transport.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncomingMessage {
    pub message: TransportMessage,
    pub source_node: NodeId,
    pub transport_id: TransportId,
    pub received_at_ms: u64,
}

/// A peer discovered via a transport adapter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredPeer {
    pub node_id: NodeId,
    pub transport_id: TransportId,
    pub address: String,
    pub initial_latency_ms: Option<f64>,
    pub discovered_at_ms: u64,
}

// ─── Transport Capabilities ──────────────────────────────────────────────────

/// What a transport adapter can do.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransportCapabilities {
    pub max_message_size_bytes: u64,
    pub supports_broadcast: bool,
    pub supports_multi_hop: bool,
    pub typical_latency_range: (f64, f64),
    pub typical_bandwidth_range: (f64, f64),
    pub encryption: EncryptionType,
    pub reliability_class: ReliabilityClass,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum EncryptionType {
    Tls13,
    NaclBox,
    WireGuardNative,
    ReticulumNative,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ReliabilityClass {
    /// TCP-based, guaranteed delivery.
    Reliable,
    /// Retries but may drop under load.
    SemiReliable,
    /// No delivery guarantee (LoRa, UDP).
    BestEffort,
}

/// Health status of a transport adapter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransportHealth {
    pub transport_id: TransportId,
    pub is_healthy: bool,
    pub peers_reachable: u32,
    pub last_successful_send_ms: Option<u64>,
    pub error_rate_percent: f64,
    pub details: String,
}

// ─── Errors ──────────────────────────────────────────────────────────────────

/// Errors that can occur during transport operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TransportError {
    Timeout { target: NodeId, timeout_ms: u64 },
    Unreachable { target: NodeId },
    MessageTooLarge { size: u64, max: u64 },
    EncryptionFailed { reason: String },
    AdapterCrashed { transport_id: TransportId },
    RoutingLoop { node: NodeId },
    TtlExpired { message_id: uuid::Uuid },
    NotConnected,
    InternalError { reason: String },
}

impl std::fmt::Display for TransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Timeout { target, timeout_ms } => write!(f, "Timeout sending to {} ({}ms)", target, timeout_ms),
            Self::Unreachable { target } => write!(f, "Node {} is unreachable", target),
            Self::MessageTooLarge { size, max } => write!(f, "Message too large: {} bytes (max {})", size, max),
            Self::EncryptionFailed { reason } => write!(f, "Encryption failed: {}", reason),
            Self::AdapterCrashed { transport_id } => write!(f, "Adapter '{}' crashed", transport_id),
            Self::RoutingLoop { node } => write!(f, "Routing loop detected at node {}", node),
            Self::TtlExpired { message_id } => write!(f, "TTL expired for message {}", message_id),
            Self::NotConnected => write!(f, "Transport not connected"),
            Self::InternalError { reason } => write!(f, "Internal transport error: {}", reason),
        }
    }
}

// ─── Bandwidth Estimate ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BandwidthEstimate {
    pub estimated_mbps: f64,
    pub measured_at_ms: u64,
    pub confidence: f64,
}

// ─── The Trait ───────────────────────────────────────────────────────────────

/// The core trait that all transport adapters must implement.
/// This is the abstraction boundary — upper layers (optimizer, inference router)
/// interact only with this trait, never with specific transport implementations.
pub trait MeshTransport: Send + Sync {
    /// Unique identifier for this transport.
    fn id(&self) -> &TransportId;

    /// Human-readable name.
    fn name(&self) -> &str;

    /// What this transport can do.
    fn capabilities(&self) -> TransportCapabilities;

    /// Discover peers reachable via this transport.
    fn discover_peers(&self) -> Vec<DiscoveredPeer>;

    /// Send a message to a specific node.
    fn send(&self, target: &NodeId, message: &TransportMessage) -> Result<(), TransportError>;

    /// Send a message to all reachable nodes.
    /// Returns the number of nodes the message was sent to.
    fn broadcast(&self, message: &TransportMessage) -> Result<u32, TransportError>;

    /// Measure latency (RTT) to a specific peer.
    fn measure_latency(&self, peer: &NodeId) -> Result<Duration, TransportError>;

    /// Get estimated bandwidth to a peer.
    fn get_bandwidth(&self, peer: &NodeId) -> Result<BandwidthEstimate, TransportError>;

    /// Get reliability score for a peer [0.0, 1.0].
    fn get_reliability(&self, peer: &NodeId) -> Result<f64, TransportError>;

    /// Check if this transport is healthy and operational.
    fn health_check(&self) -> TransportHealth;

    /// Graceful shutdown.
    fn shutdown(&self) -> Result<(), TransportError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_creation() {
        let msg = TransportMessage::new(
            vec![1, 2, 3, 4],
            MessagePriority::Critical,
            RequestType::InferenceActivation,
        );
        assert_eq!(msg.payload_size, 4);
        assert_eq!(msg.ttl_hops, 5);
        assert!(msg.visited_nodes.is_empty());
        assert_eq!(msg.priority, MessagePriority::Critical);
    }

    #[test]
    fn test_priority_ordering() {
        assert!(MessagePriority::Critical > MessagePriority::Normal);
        assert!(MessagePriority::Normal > MessagePriority::Low);
    }

    #[test]
    fn test_transport_error_display() {
        let err = TransportError::Timeout {
            target: uuid::Uuid::new_v4(),
            timeout_ms: 5000,
        };
        let msg = format!("{}", err);
        assert!(msg.contains("Timeout"));
        assert!(msg.contains("5000ms"));
    }

    #[test]
    fn test_trait_is_object_safe() {
        // This compiles only if the trait is object-safe
        fn _accepts_dyn(_t: &dyn MeshTransport) {}
    }
}
