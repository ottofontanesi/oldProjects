// Intent citation: .kiro/specs/node-persistence-layer/tasks.md Task 1.3
// Persistence data models

use serde::{Deserialize, Serialize};

/// Persisted representation of a workflow checkpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedCheckpoint {
    pub checkpoint_id: String,
    pub workflow_id: String,
    pub step_index: u32,
    pub state_json: String,
    pub created_at_ms: u64,
    pub expires_at_ms: u64,
}

/// Persisted representation of a workflow.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedWorkflow {
    pub workflow_id: String,
    pub status: WorkflowPersistenceStatus,
    pub dag_json: String,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub owner_node_id: String,
}

/// Workflow status as stored in the database.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum WorkflowPersistenceStatus {
    Pending,
    Running,
    Completed,
    Failed,
}

/// Report from a cleanup run.
#[derive(Debug, Clone)]
pub struct CleanupReport {
    pub expired_checkpoints_deleted: u64,
    pub stale_nodes_deleted: u64,
    pub old_plans_deleted: u64,
    pub vacuum_run: bool,
}

/// Database size report.
#[derive(Debug, Clone)]
pub struct DbSizeReport {
    pub size_bytes: u64,
    pub free_pages_percent: f64,
    pub approaching_limit: bool,
}
