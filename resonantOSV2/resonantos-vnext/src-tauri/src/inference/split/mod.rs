// Intent citation: .kiro/specs/split-inference-protocol/design.md
// Split Inference — coordination layer for running models across multiple nodes

pub mod codec;
pub mod assigner;
pub mod coordinator;
pub mod worker;
pub mod sync_protocol;
pub mod kv_cache;
pub mod failure;
pub mod backend;
pub mod segment_config;
pub mod segment_plan;
pub mod virtual_queue;
pub mod segment_optimizer;

/// Session identifier for a split inference group.
pub type SessionId = uuid::Uuid;
/// Node identifier (same as transport/network).
pub type NodeId = uuid::Uuid;
/// Model identifier.
pub type ModelId = String;
pub mod protocol;
