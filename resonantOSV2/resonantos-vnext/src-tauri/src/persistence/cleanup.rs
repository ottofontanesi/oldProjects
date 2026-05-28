// Intent citation: .kiro/specs/node-persistence-layer/tasks.md Task 8.2
// Cleanup — retention policies, expired data removal, VACUUM

use super::error::PersistenceError;
use super::manager::PersistenceManager;
use super::models::{CleanupReport, DbSizeReport};

impl PersistenceManager {
    /// Run all cleanup tasks (expired checkpoints, stale nodes, plan retention, VACUUM).
    pub async fn run_cleanup(&self) -> Result<CleanupReport, PersistenceError> {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let expired_checkpoints_deleted = self.cleanup_expired_checkpoints(now_ms).await?;
        let stale_nodes_deleted = self.cleanup_stale_nodes(30, now_ms).await?;
        let old_plans_deleted = self.enforce_plan_retention(10).await?;

        // Check if VACUUM is needed
        let vacuum_run = self.maybe_vacuum().await?;

        Ok(CleanupReport {
            expired_checkpoints_deleted,
            stale_nodes_deleted,
            old_plans_deleted,
            vacuum_run,
        })
    }

    /// Check database size and warn if approaching limit.
    pub async fn check_db_size(&self) -> Result<DbSizeReport, PersistenceError> {
        let path_str = self.db_path.to_string_lossy();
        if path_str == ":memory:" {
            return Ok(DbSizeReport {
                size_bytes: 0,
                free_pages_percent: 0.0,
                approaching_limit: false,
            });
        }

        let size_bytes = std::fs::metadata(&*self.db_path)
            .map(|m| m.len())
            .unwrap_or(0);

        let conn = self.writer.lock().await;
        let free_pages: i64 = conn
            .query_row("PRAGMA freelist_count", [], |row| row.get(0))
            .unwrap_or(0);
        let total_pages: i64 = conn
            .query_row("PRAGMA page_count", [], |row| row.get(0))
            .unwrap_or(1);

        let free_pages_percent = if total_pages > 0 {
            (free_pages as f64 / total_pages as f64) * 100.0
        } else {
            0.0
        };

        let approaching_limit = size_bytes > 80 * 1024 * 1024; // 80MB warning threshold

        if approaching_limit {
            eprintln!(
                "Warning: database size ({} MB) approaching 100MB limit",
                size_bytes / (1024 * 1024)
            );
        }

        // Update health with size
        {
            let mut health = self.health.lock().await;
            health.db_size_bytes = size_bytes;
        }

        Ok(DbSizeReport {
            size_bytes,
            free_pages_percent,
            approaching_limit,
        })
    }

    /// Run VACUUM if free pages exceed 20% of total.
    async fn maybe_vacuum(&self) -> Result<bool, PersistenceError> {
        let conn = self.writer.lock().await;
        let free_pages: i64 = conn
            .query_row("PRAGMA freelist_count", [], |row| row.get(0))
            .unwrap_or(0);
        let total_pages: i64 = conn
            .query_row("PRAGMA page_count", [], |row| row.get(0))
            .unwrap_or(1);

        if total_pages > 0 && (free_pages as f64 / total_pages as f64) > 0.20 {
            conn.execute_batch("VACUUM;")?;
            Ok(true)
        } else {
            Ok(false)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_run_cleanup_empty_db() {
        let pm = PersistenceManager::initialize_in_memory().unwrap();
        let report = pm.run_cleanup().await.unwrap();

        assert_eq!(report.expired_checkpoints_deleted, 0);
        assert_eq!(report.stale_nodes_deleted, 0);
        assert_eq!(report.old_plans_deleted, 0);
    }

    #[tokio::test]
    async fn test_check_db_size_in_memory() {
        let pm = PersistenceManager::initialize_in_memory().unwrap();
        let report = pm.check_db_size().await.unwrap();

        assert_eq!(report.size_bytes, 0);
        assert!(!report.approaching_limit);
    }
}
