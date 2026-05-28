// Intent citation: .kiro/specs/node-persistence-layer/tasks.md Task 1.1
// Persistence module — SQLite-backed durable storage for all node state

pub mod error;
pub mod manager;
pub mod models;
pub mod migrations;
pub mod node_store;
pub mod checkpoint_store;
pub mod placement_store;
pub mod settings_store;
pub mod workflow_store;
pub mod cleanup;

// Re-exports for convenience
pub use error::PersistenceError;
pub use manager::{HealthStatus, PersistenceManager};
pub use models::{
    CleanupReport, DbSizeReport, PersistedCheckpoint, PersistedWorkflow,
    WorkflowPersistenceStatus,
};
pub use placement_store::PlacementPlan;

#[cfg(test)]
mod property_tests;

#[cfg(test)]
mod integration_tests;
