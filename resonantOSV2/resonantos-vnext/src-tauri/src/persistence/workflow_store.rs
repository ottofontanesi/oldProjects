// Intent citation: .kiro/specs/node-persistence-layer/tasks.md Task 7
// WorkflowStore — CRUD operations for the workflows table

use rusqlite::params;

use super::error::PersistenceError;
use super::manager::PersistenceManager;
use super::models::{PersistedWorkflow, WorkflowPersistenceStatus};

/// Validate that a string is valid JSON.
fn validate_json(input: &str) -> Result<(), PersistenceError> {
    serde_json::from_str::<serde_json::Value>(input)
        .map(|_| ())
        .map_err(|e| PersistenceError::InvalidJson(format!("JSON validation failed: {}", e)))
}

impl WorkflowPersistenceStatus {
    /// Convert to database string representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }

    /// Parse from database string representation.
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "pending" => Some(Self::Pending),
            "running" => Some(Self::Running),
            "completed" => Some(Self::Completed),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }
}

impl PersistenceManager {
    /// Upsert workflow metadata.
    pub async fn upsert_workflow(&self, workflow: &PersistedWorkflow) -> Result<(), PersistenceError> {
        validate_json(&workflow.dag_json)?;

        let workflow_id = workflow.workflow_id.clone();
        let status = workflow.status.as_str().to_string();
        let dag_json = workflow.dag_json.clone();
        let created_at_ms = workflow.created_at_ms as i64;
        let updated_at_ms = workflow.updated_at_ms as i64;
        let owner_node_id = workflow.owner_node_id.clone();

        self.retry_write(move |conn| {
            conn.execute(
                "INSERT OR REPLACE INTO workflows (workflow_id, status, dag_json, created_at_ms, updated_at_ms, owner_node_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    workflow_id,
                    status,
                    dag_json,
                    created_at_ms,
                    updated_at_ms,
                    owner_node_id,
                ],
            )?;
            Ok(())
        })
        .await
    }

    /// Load workflows with status "running" for recovery.
    pub async fn load_running_workflows(&self) -> Result<Vec<PersistedWorkflow>, PersistenceError> {
        let conn = self.writer.lock().await;
        let mut stmt = conn.prepare(
            "SELECT workflow_id, status, dag_json, created_at_ms, updated_at_ms, owner_node_id
             FROM workflows WHERE status = 'running'"
        )?;

        let rows = stmt.query_map([], |row| {
            let status_str: String = row.get(1)?;
            Ok((
                row.get::<_, String>(0)?,
                status_str,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, String>(5)?,
            ))
        })?;

        let mut workflows = Vec::new();
        for row in rows {
            match row {
                Ok((workflow_id, status_str, dag_json, created_at_ms, updated_at_ms, owner_node_id)) => {
                    if let Some(status) = WorkflowPersistenceStatus::from_str(&status_str) {
                        workflows.push(PersistedWorkflow {
                            workflow_id,
                            status,
                            dag_json,
                            created_at_ms: created_at_ms as u64,
                            updated_at_ms: updated_at_ms as u64,
                            owner_node_id,
                        });
                    }
                }
                Err(e) => eprintln!("Warning: skipping workflow row with error: {}", e),
            }
        }

        Ok(workflows)
    }

    /// Mark stale running workflows (older than max_age_hours) as failed.
    /// Returns the number of rows updated.
    pub async fn timeout_stale_workflows(&self, max_age_hours: u64, now_ms: u64) -> Result<u64, PersistenceError> {
        let cutoff_ms = now_ms as i64 - (max_age_hours as i64 * 3600 * 1000);
        let now = now_ms as i64;

        self.retry_write(move |conn| {
            let updated = conn.execute(
                "UPDATE workflows SET status = 'failed', updated_at_ms = ?1 WHERE status = 'running' AND updated_at_ms < ?2",
                params![now, cutoff_ms],
            )?;
            Ok(updated as u64)
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_workflow(id: &str, status: WorkflowPersistenceStatus, updated_at_ms: u64) -> PersistedWorkflow {
        PersistedWorkflow {
            workflow_id: id.to_string(),
            status,
            dag_json: r#"{"steps": [{"id": "step1", "action": "compute"}]}"#.to_string(),
            created_at_ms: 1000,
            updated_at_ms,
            owner_node_id: "node-1".to_string(),
        }
    }

    #[tokio::test]
    async fn test_upsert_and_load_workflow() {
        let pm = PersistenceManager::initialize_in_memory().unwrap();
        let wf = make_workflow("wf-1", WorkflowPersistenceStatus::Running, 5000);

        pm.upsert_workflow(&wf).await.unwrap();

        let loaded = pm.load_running_workflows().await.unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].workflow_id, "wf-1");
        assert_eq!(loaded[0].status, WorkflowPersistenceStatus::Running);
        assert_eq!(loaded[0].owner_node_id, "node-1");
    }

    #[tokio::test]
    async fn test_load_running_filters_by_status() {
        let pm = PersistenceManager::initialize_in_memory().unwrap();

        pm.upsert_workflow(&make_workflow("wf-1", WorkflowPersistenceStatus::Running, 5000)).await.unwrap();
        pm.upsert_workflow(&make_workflow("wf-2", WorkflowPersistenceStatus::Pending, 5000)).await.unwrap();
        pm.upsert_workflow(&make_workflow("wf-3", WorkflowPersistenceStatus::Completed, 5000)).await.unwrap();
        pm.upsert_workflow(&make_workflow("wf-4", WorkflowPersistenceStatus::Failed, 5000)).await.unwrap();
        pm.upsert_workflow(&make_workflow("wf-5", WorkflowPersistenceStatus::Running, 6000)).await.unwrap();

        let running = pm.load_running_workflows().await.unwrap();
        assert_eq!(running.len(), 2);

        let ids: Vec<&str> = running.iter().map(|w| w.workflow_id.as_str()).collect();
        assert!(ids.contains(&"wf-1"));
        assert!(ids.contains(&"wf-5"));
    }

    #[tokio::test]
    async fn test_timeout_stale_workflows() {
        let pm = PersistenceManager::initialize_in_memory().unwrap();

        // Workflow updated 25 hours ago (stale)
        pm.upsert_workflow(&make_workflow("wf-old", WorkflowPersistenceStatus::Running, 1000)).await.unwrap();
        // Workflow updated recently (fresh)
        pm.upsert_workflow(&make_workflow("wf-new", WorkflowPersistenceStatus::Running, 90_000_000)).await.unwrap();
        // Completed workflow (should not be affected)
        pm.upsert_workflow(&make_workflow("wf-done", WorkflowPersistenceStatus::Completed, 1000)).await.unwrap();

        // now = 100_000_000, max_age = 24 hours = 86_400_000 ms
        // cutoff = 100_000_000 - 86_400_000 = 13_600_000
        // wf-old (updated_at 1000) < 13_600_000 → should be timed out
        // wf-new (updated_at 90_000_000) >= 13_600_000 → should remain running
        let timed_out = pm.timeout_stale_workflows(24, 100_000_000).await.unwrap();
        assert_eq!(timed_out, 1);

        let running = pm.load_running_workflows().await.unwrap();
        assert_eq!(running.len(), 1);
        assert_eq!(running[0].workflow_id, "wf-new");
    }

    #[tokio::test]
    async fn test_upsert_updates_existing_workflow() {
        let pm = PersistenceManager::initialize_in_memory().unwrap();

        let wf = make_workflow("wf-1", WorkflowPersistenceStatus::Running, 5000);
        pm.upsert_workflow(&wf).await.unwrap();

        // Update status
        let updated = PersistedWorkflow {
            status: WorkflowPersistenceStatus::Completed,
            updated_at_ms: 10000,
            ..wf
        };
        pm.upsert_workflow(&updated).await.unwrap();

        let running = pm.load_running_workflows().await.unwrap();
        assert_eq!(running.len(), 0); // No longer running
    }

    #[tokio::test]
    async fn test_reject_invalid_json_workflow() {
        let pm = PersistenceManager::initialize_in_memory().unwrap();
        let wf = PersistedWorkflow {
            workflow_id: "wf-bad".to_string(),
            status: WorkflowPersistenceStatus::Running,
            dag_json: "invalid json {{".to_string(),
            created_at_ms: 1000,
            updated_at_ms: 1000,
            owner_node_id: "node-1".to_string(),
        };

        let result = pm.upsert_workflow(&wf).await;
        assert!(matches!(result, Err(PersistenceError::InvalidJson(_))));
    }
}
