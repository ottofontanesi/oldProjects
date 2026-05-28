// Intent citation: .kiro/specs/node-persistence-layer/tasks.md Task 4
// CheckpointStore — CRUD operations for the checkpoints table

use rusqlite::params;

use super::error::PersistenceError;
use super::manager::PersistenceManager;
use super::models::PersistedCheckpoint;

/// Validate that a string is valid JSON.
fn validate_json(input: &str) -> Result<(), PersistenceError> {
    serde_json::from_str::<serde_json::Value>(input)
        .map(|_| ())
        .map_err(|e| PersistenceError::InvalidJson(format!("JSON validation failed: {}", e)))
}

impl PersistenceManager {
    /// Insert a workflow checkpoint.
    pub async fn save_checkpoint(&self, checkpoint: &PersistedCheckpoint) -> Result<(), PersistenceError> {
        validate_json(&checkpoint.state_json)?;

        let checkpoint_id = checkpoint.checkpoint_id.clone();
        let workflow_id = checkpoint.workflow_id.clone();
        let step_index = checkpoint.step_index as i64;
        let state_json = checkpoint.state_json.clone();
        let created_at_ms = checkpoint.created_at_ms as i64;
        let expires_at_ms = checkpoint.expires_at_ms as i64;

        self.retry_write(move |conn| {
            conn.execute(
                "INSERT OR REPLACE INTO checkpoints (checkpoint_id, workflow_id, step_index, state_json, created_at_ms, expires_at_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    checkpoint_id,
                    workflow_id,
                    step_index,
                    state_json,
                    created_at_ms,
                    expires_at_ms,
                ],
            )?;
            Ok(())
        })
        .await
    }

    /// Load all unexpired checkpoints (where expires_at_ms >= now_ms).
    pub async fn load_unexpired_checkpoints(&self, now_ms: u64) -> Result<Vec<PersistedCheckpoint>, PersistenceError> {
        let conn = self.writer.lock().await;
        let mut stmt = conn.prepare(
            "SELECT checkpoint_id, workflow_id, step_index, state_json, created_at_ms, expires_at_ms
             FROM checkpoints WHERE expires_at_ms >= ?1"
        )?;

        let now = now_ms as i64;
        let rows = stmt.query_map(params![now], |row| {
            Ok(PersistedCheckpoint {
                checkpoint_id: row.get(0)?,
                workflow_id: row.get(1)?,
                step_index: row.get::<_, i64>(2)? as u32,
                state_json: row.get(3)?,
                created_at_ms: row.get::<_, i64>(4)? as u64,
                expires_at_ms: row.get::<_, i64>(5)? as u64,
            })
        })?;

        let mut checkpoints = Vec::new();
        for row in rows {
            match row {
                Ok(cp) => checkpoints.push(cp),
                Err(e) => eprintln!("Warning: skipping checkpoint row with error: {}", e),
            }
        }

        Ok(checkpoints)
    }

    /// Delete expired checkpoints (where expires_at_ms < now_ms).
    /// Returns the number of rows deleted.
    pub async fn cleanup_expired_checkpoints(&self, now_ms: u64) -> Result<u64, PersistenceError> {
        let now = now_ms as i64;
        self.retry_write(move |conn| {
            let deleted = conn.execute(
                "DELETE FROM checkpoints WHERE expires_at_ms < ?1",
                params![now],
            )?;
            Ok(deleted as u64)
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_checkpoint(id: &str, workflow_id: &str, expires_at_ms: u64) -> PersistedCheckpoint {
        PersistedCheckpoint {
            checkpoint_id: id.to_string(),
            workflow_id: workflow_id.to_string(),
            step_index: 1,
            state_json: r#"{"step": "processing", "data": [1,2,3]}"#.to_string(),
            created_at_ms: 1000,
            expires_at_ms,
        }
    }

    #[tokio::test]
    async fn test_save_and_load_checkpoint() {
        let pm = PersistenceManager::initialize_in_memory().unwrap();
        let cp = make_checkpoint("cp-1", "wf-1", 5000);

        pm.save_checkpoint(&cp).await.unwrap();

        let loaded = pm.load_unexpired_checkpoints(1000).await.unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].checkpoint_id, "cp-1");
        assert_eq!(loaded[0].workflow_id, "wf-1");
        assert_eq!(loaded[0].step_index, 1);
    }

    #[tokio::test]
    async fn test_load_filters_expired() {
        let pm = PersistenceManager::initialize_in_memory().unwrap();

        pm.save_checkpoint(&make_checkpoint("cp-1", "wf-1", 3000)).await.unwrap();
        pm.save_checkpoint(&make_checkpoint("cp-2", "wf-1", 7000)).await.unwrap();
        pm.save_checkpoint(&make_checkpoint("cp-3", "wf-2", 5000)).await.unwrap();

        // At time 5000, cp-1 (expires 3000) is expired, cp-2 and cp-3 are valid
        let loaded = pm.load_unexpired_checkpoints(5000).await.unwrap();
        assert_eq!(loaded.len(), 2);

        let ids: Vec<&str> = loaded.iter().map(|c| c.checkpoint_id.as_str()).collect();
        assert!(ids.contains(&"cp-2"));
        assert!(ids.contains(&"cp-3"));
    }

    #[tokio::test]
    async fn test_cleanup_expired_checkpoints() {
        let pm = PersistenceManager::initialize_in_memory().unwrap();

        pm.save_checkpoint(&make_checkpoint("cp-1", "wf-1", 3000)).await.unwrap();
        pm.save_checkpoint(&make_checkpoint("cp-2", "wf-1", 7000)).await.unwrap();
        pm.save_checkpoint(&make_checkpoint("cp-3", "wf-2", 2000)).await.unwrap();

        let deleted = pm.cleanup_expired_checkpoints(5000).await.unwrap();
        assert_eq!(deleted, 2); // cp-1 and cp-3

        let remaining = pm.load_unexpired_checkpoints(0).await.unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].checkpoint_id, "cp-2");
    }

    #[tokio::test]
    async fn test_reject_invalid_json_checkpoint() {
        let pm = PersistenceManager::initialize_in_memory().unwrap();
        let cp = PersistedCheckpoint {
            checkpoint_id: "cp-bad".to_string(),
            workflow_id: "wf-1".to_string(),
            step_index: 0,
            state_json: "not valid json {{{".to_string(),
            created_at_ms: 1000,
            expires_at_ms: 5000,
        };

        let result = pm.save_checkpoint(&cp).await;
        assert!(matches!(result, Err(PersistenceError::InvalidJson(_))));
    }
}
