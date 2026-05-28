// Intent citation: .kiro/specs/unified-mesh-transport/design.md
// Unified Mesh Transport — abstracts multiple networking technologies behind a single trait

pub mod trait_def;
pub mod manager;
pub mod registry;
pub mod selector;
pub mod failover;
pub mod metrics;
pub mod router;
pub mod adapters;
pub mod security;
pub mod commands;
pub mod qos;
