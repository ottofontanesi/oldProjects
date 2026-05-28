// Intent citation: .kiro/specs/node-persistence-layer/tasks.md Task 6
// SettingsStore — CRUD operations for the settings table with in-memory cache

use rusqlite::params;

use super::error::PersistenceError;
use super::manager::PersistenceManager;

impl PersistenceManager {
    /// Get a setting by key. Returns cached value if available.
    pub async fn get_setting(&self, key: &str) -> Result<Option<serde_json::Value>, PersistenceError> {
        // Check cache first
        if let Some(entry) = self.settings_cache.get(key) {
            return Ok(Some(entry.value().clone()));
        }

        // Cache miss — query DB
        let conn = self.writer.lock().await;
        let mut stmt = conn.prepare("SELECT value_json FROM settings WHERE key = ?1")?;

        let result = stmt.query_row(params![key], |row| {
            let json_str: String = row.get(0)?;
            Ok(json_str)
        });

        match result {
            Ok(json_str) => {
                let value: serde_json::Value = serde_json::from_str(&json_str)?;
                // Populate cache
                self.settings_cache.insert(key.to_string(), value.clone());
                Ok(Some(value))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(PersistenceError::Sqlite(e)),
        }
    }

    /// Set a setting (write-through: updates cache and DB atomically).
    pub async fn set_setting(&self, key: &str, value: serde_json::Value) -> Result<(), PersistenceError> {
        let key_owned = key.to_string();
        let value_json = serde_json::to_string(&value)?;
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;

        self.retry_write(move |conn| {
            conn.execute(
                "INSERT OR REPLACE INTO settings (key, value_json, updated_at_ms) VALUES (?1, ?2, ?3)",
                params![key_owned, value_json, now_ms],
            )?;
            Ok(())
        })
        .await?;

        // Update cache after successful write
        self.settings_cache.insert(key.to_string(), value);
        Ok(())
    }

    /// Delete a setting.
    pub async fn delete_setting(&self, key: &str) -> Result<(), PersistenceError> {
        let key_owned = key.to_string();
        self.retry_write(move |conn| {
            conn.execute("DELETE FROM settings WHERE key = ?1", params![key_owned])?;
            Ok(())
        })
        .await?;

        // Remove from cache
        self.settings_cache.remove(key);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn test_set_and_get_setting() {
        let pm = PersistenceManager::initialize_in_memory().unwrap();

        pm.set_setting("theme", json!("dark")).await.unwrap();

        let value = pm.get_setting("theme").await.unwrap();
        assert_eq!(value, Some(json!("dark")));
    }

    #[tokio::test]
    async fn test_get_nonexistent_setting() {
        let pm = PersistenceManager::initialize_in_memory().unwrap();

        let value = pm.get_setting("nonexistent").await.unwrap();
        assert_eq!(value, None);
    }

    #[tokio::test]
    async fn test_overwrite_setting() {
        let pm = PersistenceManager::initialize_in_memory().unwrap();

        pm.set_setting("count", json!(1)).await.unwrap();
        pm.set_setting("count", json!(42)).await.unwrap();

        let value = pm.get_setting("count").await.unwrap();
        assert_eq!(value, Some(json!(42)));
    }

    #[tokio::test]
    async fn test_delete_setting() {
        let pm = PersistenceManager::initialize_in_memory().unwrap();

        pm.set_setting("temp", json!("value")).await.unwrap();
        pm.delete_setting("temp").await.unwrap();

        let value = pm.get_setting("temp").await.unwrap();
        assert_eq!(value, None);
    }

    #[tokio::test]
    async fn test_complex_json_setting() {
        let pm = PersistenceManager::initialize_in_memory().unwrap();

        let complex = json!({
            "providers": ["openai", "anthropic"],
            "max_tokens": 4096,
            "nested": {
                "enabled": true,
                "list": [1, 2, 3]
            }
        });

        pm.set_setting("config", complex.clone()).await.unwrap();

        let loaded = pm.get_setting("config").await.unwrap().unwrap();
        assert_eq!(loaded, complex);
    }

    #[tokio::test]
    async fn test_cache_coherence() {
        let pm = PersistenceManager::initialize_in_memory().unwrap();

        // Set value — should be in cache
        pm.set_setting("key", json!("v1")).await.unwrap();
        assert_eq!(pm.get_setting("key").await.unwrap(), Some(json!("v1")));

        // Update — cache should reflect new value
        pm.set_setting("key", json!("v2")).await.unwrap();
        assert_eq!(pm.get_setting("key").await.unwrap(), Some(json!("v2")));

        // Delete — cache should be empty
        pm.delete_setting("key").await.unwrap();
        assert_eq!(pm.get_setting("key").await.unwrap(), None);
    }
}
