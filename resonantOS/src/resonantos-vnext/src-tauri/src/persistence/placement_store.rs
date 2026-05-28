// Intent citation: .kiro/specs/node-persistence-layer/tasks.md Task 5
// PlacementStore — CRUD operations for the placements table

use rusqlite::params;
use serde::{Deserialize, Serialize};

use super::error::PersistenceError;
use super::manager::PersistenceManager;

/// Persisted representation of a placement plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlacementPlan {
    pub plan_id: String,
    pub created_at_ms: u64,
    pub plan_json: String,
    pub utility_score: f64,
}

/// Validate that a string is valid JSON.
fn validate_json(input: &str) -> Result<(), PersistenceError> {
    serde_json::from_str::<serde_json::Value>(input)
        .map(|_| ())
        .map_err(|e| PersistenceError::InvalidJson(format!("JSON validation failed: {}", e)))
}

impl PersistenceManager {
    /// Insert a new placement plan, marking it active and deactivating the previous.
    pub async fn save_plan(&self, plan: &PlacementPlan) -> Result<(), PersistenceError> {
        validate_json(&plan.plan_json)?;

        let plan_id = plan.plan_id.clone();
        let created_at_ms = plan.created_at_ms as i64;
        let plan_json = plan.plan_json.clone();
        let utility_score = plan.utility_score;

        self.retry_write(move |conn| {
            let tx = conn.unchecked_transaction()?;
            tx.execute("UPDATE placements SET is_active = 0 WHERE is_active = 1", [])?;
            tx.execute(
                "INSERT INTO placements (plan_id, created_at_ms, plan_json, utility_score, is_active)
                 VALUES (?1, ?2, ?3, ?4, 1)",
                params![plan_id, created_at_ms, plan_json, utility_score],
            )?;
            tx.commit()?;
            Ok(())
        })
        .await
    }

    /// Load the current active plan (if any).
    pub async fn load_active_plan(&self) -> Result<Option<PlacementPlan>, PersistenceError> {
        let conn = self.writer.lock().await;
        let mut stmt = conn.prepare(
            "SELECT plan_id, created_at_ms, plan_json, utility_score FROM placements WHERE is_active = 1 LIMIT 1"
        )?;

        let result = stmt.query_row([], |row| {
            Ok(PlacementPlan {
                plan_id: row.get(0)?,
                created_at_ms: row.get::<_, i64>(1)? as u64,
                plan_json: row.get(2)?,
                utility_score: row.get(3)?,
            })
        });

        match result {
            Ok(plan) => Ok(Some(plan)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(PersistenceError::Sqlite(e)),
        }
    }

    /// Enforce retention: keep only the last N plans (by created_at_ms).
    /// Returns the number of rows deleted.
    pub async fn enforce_plan_retention(&self, keep_count: usize) -> Result<u64, PersistenceError> {
        let keep = keep_count as i64;
        self.retry_write(move |conn| {
            let deleted = conn.execute(
                "DELETE FROM placements WHERE plan_id NOT IN (SELECT plan_id FROM placements ORDER BY created_at_ms DESC LIMIT ?1)",
                params![keep],
            )?;
            Ok(deleted as u64)
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_plan(id: &str, created_at_ms: u64, score: f64) -> PlacementPlan {
        PlacementPlan {
            plan_id: id.to_string(),
            created_at_ms,
            plan_json: format!(r#"{{"plan_id": "{}", "assignments": []}}"#, id),
            utility_score: score,
        }
    }

    #[tokio::test]
    async fn test_save_and_load_plan() {
        let pm = PersistenceManager::initialize_in_memory().unwrap();
        let plan = make_plan("plan-1", 1000, 0.85);

        pm.save_plan(&plan).await.unwrap();

        let loaded = pm.load_active_plan().await.unwrap();
        assert!(loaded.is_some());
        let loaded = loaded.unwrap();
        assert_eq!(loaded.plan_id, "plan-1");
        assert_eq!(loaded.utility_score, 0.85);
    }

    #[tokio::test]
    async fn test_only_one_active_plan() {
        let pm = PersistenceManager::initialize_in_memory().unwrap();

        pm.save_plan(&make_plan("plan-1", 1000, 0.8)).await.unwrap();
        pm.save_plan(&make_plan("plan-2", 2000, 0.9)).await.unwrap();
        pm.save_plan(&make_plan("plan-3", 3000, 0.95)).await.unwrap();

        let active = pm.load_active_plan().await.unwrap().unwrap();
        assert_eq!(active.plan_id, "plan-3");

        // Verify only one is active
        let conn = pm.writer.lock().await;
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM placements WHERE is_active = 1", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn test_no_active_plan_returns_none() {
        let pm = PersistenceManager::initialize_in_memory().unwrap();
        let loaded = pm.load_active_plan().await.unwrap();
        assert!(loaded.is_none());
    }

    #[tokio::test]
    async fn test_enforce_plan_retention() {
        let pm = PersistenceManager::initialize_in_memory().unwrap();

        for i in 0..15 {
            pm.save_plan(&make_plan(&format!("plan-{}", i), i * 1000, 0.5 + i as f64 * 0.01))
                .await
                .unwrap();
        }

        let deleted = pm.enforce_plan_retention(10).await.unwrap();
        assert_eq!(deleted, 5);

        // Verify 10 remain
        let conn = pm.writer.lock().await;
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM placements", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 10);
    }

    #[tokio::test]
    async fn test_reject_invalid_json_plan() {
        let pm = PersistenceManager::initialize_in_memory().unwrap();
        let plan = PlacementPlan {
            plan_id: "bad-plan".to_string(),
            created_at_ms: 1000,
            plan_json: "not json!!!".to_string(),
            utility_score: 0.5,
        };

        let result = pm.save_plan(&plan).await;
        assert!(matches!(result, Err(PersistenceError::InvalidJson(_))));
    }
}
