use chrono::Utc;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

/// The fixed set of trusted agent identifiers.
pub const TRUSTED_AGENT_SET: &[&str] = &[
    "strategist.core",
    "setup.core",
    "logician.core",
];

/// Maximum number of fact records in the store.
pub const MAX_STORE_SIZE: usize = 50;

/// Maximum token count for fact content.
pub const MAX_CONTENT_TOKENS: usize = 200;

/// A single fact record in the federated memory store.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FactRecord {
    pub id: String,
    pub source_agent: String,
    pub timestamp: String,
    pub category: String,
    pub content: String,
    pub confidence: f64,
    pub ttl_seconds: u64,
}

/// Query parameters for fact retrieval.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FactQuery {
    pub category: Option<String>,
    pub source_agent: Option<String>,
    pub min_confidence: Option<f64>,
    pub max_age_seconds: Option<u64>,
    pub limit: Option<u32>,
}

/// Write request for a new fact.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FactWriteRequest {
    pub agent_id: String,
    pub category: String,
    pub content: String,
    pub confidence: f64,
    pub ttl_seconds: u64,
}

/// Result of a write operation.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FactWriteResult {
    pub id: String,
    pub accepted: bool,
    pub error: Option<String>,
    pub evicted_ids: Vec<String>,
}

/// Request for reading a fact by ID.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FactReadByIdRequest {
    pub agent_id: String,
    pub fact_id: String,
}

/// Status of the federated memory service.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FederatedMemoryStatus {
    pub total_facts: u32,
    pub capacity: u32,
    pub trusted_agents: Vec<String>,
    pub available: bool,
}

/// Initialize the federated memory database schema.
pub fn initialize_federated_memory_db(connection: &Connection) -> Result<(), String> {
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS facts (
                id TEXT PRIMARY KEY,
                source_agent TEXT NOT NULL,
                timestamp TEXT NOT NULL,
                category TEXT NOT NULL CHECK(category IN ('system-config', 'provider-state', 'user-preference', 'architecture-decision')),
                content TEXT NOT NULL,
                token_count INTEGER NOT NULL,
                confidence REAL NOT NULL CHECK(confidence >= 0.0 AND confidence <= 1.0),
                ttl_seconds INTEGER NOT NULL,
                expires_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS access_log (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                timestamp TEXT NOT NULL,
                agent_id TEXT NOT NULL,
                operation TEXT NOT NULL,
                fact_id TEXT,
                success INTEGER NOT NULL DEFAULT 1
            );

            CREATE TABLE IF NOT EXISTS trusted_agent_promotions (
                agent_id TEXT PRIMARY KEY,
                promoted_by TEXT NOT NULL,
                promoted_at TEXT NOT NULL,
                validation_period_end TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_facts_category ON facts(category);
            CREATE INDEX IF NOT EXISTS idx_facts_source_agent ON facts(source_agent);
            CREATE INDEX IF NOT EXISTS idx_facts_expires_at ON facts(expires_at);
            CREATE INDEX IF NOT EXISTS idx_facts_timestamp ON facts(timestamp DESC);
            CREATE INDEX IF NOT EXISTS idx_access_log_timestamp ON access_log(timestamp);",
        )
        .map_err(|e| format!("Failed to initialize federated memory schema: {}", e))
}

/// Validate that an agent is in the trusted agent set.
pub fn validate_trusted_agent(agent_id: &str) -> Result<(), String> {
    if TRUSTED_AGENT_SET.contains(&agent_id) {
        Ok(())
    } else {
        // Check if agent has been promoted and is within validation period
        Err(format!(
            "Agent '{}' is not in the Trusted_Agent_Set",
            agent_id
        ))
    }
}

/// Validate trusted agent with promotion check against the database.
pub fn validate_trusted_agent_with_db(
    connection: &Connection,
    agent_id: &str,
) -> Result<(), String> {
    // First check the hardcoded set
    if TRUSTED_AGENT_SET.contains(&agent_id) {
        return Ok(());
    }

    // Check promotions table for agents within their validation period
    let now = Utc::now().to_rfc3339();
    let count: u32 = connection
        .query_row(
            "SELECT COUNT(*) FROM trusted_agent_promotions
             WHERE agent_id = ?1 AND validation_period_end > ?2",
            params![agent_id, now],
            |row| row.get(0),
        )
        .unwrap_or(0);

    if count > 0 {
        Ok(())
    } else {
        Err(format!(
            "Agent '{}' is not in the Trusted_Agent_Set",
            agent_id
        ))
    }
}

/// Estimate token count using whitespace-split heuristic.
/// Approximation: word_count × 4 / 3 (roughly 0.75 words per token).
pub fn estimate_token_count(content: &str) -> usize {
    let word_count = content.split_whitespace().count();
    (word_count * 4) / 3
}

/// Log an unauthorized access attempt to the access_log table.
pub fn log_unauthorized_access(
    connection: &Connection,
    agent_id: &str,
    operation: &str,
    fact_id: Option<&str>,
) -> Result<(), String> {
    let timestamp = Utc::now().to_rfc3339();
    connection
        .execute(
            "INSERT INTO access_log (timestamp, agent_id, operation, fact_id, success)
             VALUES (?1, ?2, ?3, ?4, 0)",
            params![timestamp, agent_id, operation, fact_id],
        )
        .map_err(|e| format!("Failed to log unauthorized access: {}", e))?;
    Ok(())
}

/// Log a successful access to the access_log table.
fn log_access(
    connection: &Connection,
    agent_id: &str,
    operation: &str,
    fact_id: Option<&str>,
) -> Result<(), String> {
    let timestamp = Utc::now().to_rfc3339();
    connection
        .execute(
            "INSERT INTO access_log (timestamp, agent_id, operation, fact_id, success)
             VALUES (?1, ?2, ?3, ?4, 1)",
            params![timestamp, agent_id, operation, fact_id],
        )
        .map_err(|e| format!("Failed to log access: {}", e))?;
    Ok(())
}

/// Get the current count of facts in the store.
pub fn get_fact_count(connection: &Connection) -> Result<usize, String> {
    let count: u32 = connection
        .query_row("SELECT COUNT(*) FROM facts", [], |row| row.get(0))
        .map_err(|e| format!("Failed to count facts: {}", e))?;
    Ok(count as usize)
}

/// Evict records to make room for new entries.
/// Returns the IDs of evicted records.
/// Policy: delete expired-TTL first (oldest expiry), then oldest non-expired.
pub fn evict_if_at_capacity(connection: &Connection) -> Result<Vec<String>, String> {
    let count = get_fact_count(connection)?;
    if count < MAX_STORE_SIZE {
        return Ok(Vec::new());
    }

    let mut evicted_ids = Vec::new();
    let now = Utc::now().to_rfc3339();

    // First: evict expired records (oldest expiry first)
    let expired_ids: Vec<String> = {
        let mut stmt = connection
            .prepare(
                "SELECT id FROM facts WHERE expires_at < ?1 ORDER BY expires_at ASC LIMIT 1",
            )
            .map_err(|e| format!("Failed to query expired facts: {}", e))?;
        let rows = stmt
            .query_map(params![now], |row| row.get(0))
            .map_err(|e| format!("Failed to read expired facts: {}", e))?;
        let mut ids = Vec::new();
        for row in rows {
            ids.push(row.map_err(|e| format!("Failed to read expired id: {}", e))?);
        }
        ids
    };

    for id in &expired_ids {
        connection
            .execute("DELETE FROM facts WHERE id = ?1", params![id])
            .map_err(|e| format!("Failed to evict expired fact: {}", e))?;
        evicted_ids.push(id.clone());
    }

    // Check if we still need to evict
    let count = get_fact_count(connection)?;
    if count < MAX_STORE_SIZE {
        return Ok(evicted_ids);
    }

    // Second: evict oldest non-expired records
    let oldest_ids: Vec<String> = {
        let mut stmt = connection
            .prepare("SELECT id FROM facts ORDER BY timestamp ASC LIMIT 1")
            .map_err(|e| format!("Failed to query oldest facts: {}", e))?;
        let rows = stmt
            .query_map([], |row| row.get(0))
            .map_err(|e| format!("Failed to read oldest facts: {}", e))?;
        let mut ids = Vec::new();
        for row in rows {
            ids.push(row.map_err(|e| format!("Failed to read oldest id: {}", e))?);
        }
        ids
    };

    for id in &oldest_ids {
        connection
            .execute("DELETE FROM facts WHERE id = ?1", params![id])
            .map_err(|e| format!("Failed to evict oldest fact: {}", e))?;
        evicted_ids.push(id.clone());
    }

    Ok(evicted_ids)
}

/// Write a fact to the federated memory store.
pub fn federated_memory_write(
    connection: &Connection,
    request: &FactWriteRequest,
) -> Result<FactWriteResult, String> {
    // Validate access control
    if let Err(e) = validate_trusted_agent_with_db(connection, &request.agent_id) {
        log_unauthorized_access(connection, &request.agent_id, "unauthorized-write", None)?;
        return Ok(FactWriteResult {
            id: String::new(),
            accepted: false,
            error: Some(e),
            evicted_ids: Vec::new(),
        });
    }

    // Validate token limit
    let token_count = estimate_token_count(&request.content);
    if token_count > MAX_CONTENT_TOKENS {
        return Ok(FactWriteResult {
            id: String::new(),
            accepted: false,
            error: Some(format!(
                "Content exceeds 200 token limit (estimated: {} tokens)",
                token_count
            )),
            evicted_ids: Vec::new(),
        });
    }

    // Clamp confidence to [0.0, 1.0]
    let confidence = request.confidence.clamp(0.0, 1.0);

    // Validate category
    let valid_categories = [
        "system-config",
        "provider-state",
        "user-preference",
        "architecture-decision",
    ];
    if !valid_categories.contains(&request.category.as_str()) {
        return Ok(FactWriteResult {
            id: String::new(),
            accepted: false,
            error: Some(format!(
                "Invalid category '{}'. Valid categories: {:?}",
                request.category, valid_categories
            )),
            evicted_ids: Vec::new(),
        });
    }

    // Evict if at capacity
    let evicted_ids = evict_if_at_capacity(connection)?;

    // Generate ID and timestamps
    let id = format!(
        "fact-{}-{}",
        request.agent_id.replace('.', "-"),
        Utc::now().timestamp_millis()
    );
    let timestamp = Utc::now().to_rfc3339();
    let expires_at = (Utc::now()
        + chrono::Duration::seconds(request.ttl_seconds as i64))
        .to_rfc3339();

    // Insert the fact
    connection
        .execute(
            "INSERT INTO facts (id, source_agent, timestamp, category, content, token_count, confidence, ttl_seconds, expires_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                id,
                request.agent_id,
                timestamp,
                request.category,
                request.content,
                token_count as u32,
                confidence,
                request.ttl_seconds,
                expires_at,
            ],
        )
        .map_err(|e| format!("Failed to insert fact: {}", e))?;

    // Log successful access
    log_access(connection, &request.agent_id, "write", Some(&id))?;

    Ok(FactWriteResult {
        id,
        accepted: true,
        error: None,
        evicted_ids,
    })
}

/// Query facts with filters.
pub fn federated_memory_query_db(
    connection: &Connection,
    agent_id: &str,
    query: &FactQuery,
) -> Result<Vec<FactRecord>, String> {
    // Validate access control
    if let Err(e) = validate_trusted_agent_with_db(connection, agent_id) {
        log_unauthorized_access(connection, agent_id, "unauthorized-read", None)?;
        return Err(e);
    }

    let mut sql = String::from(
        "SELECT id, source_agent, timestamp, category, content, confidence, ttl_seconds
         FROM facts WHERE 1=1",
    );
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

    if let Some(ref category) = query.category {
        sql.push_str(" AND category = ?");
        param_values.push(Box::new(category.clone()));
    }
    if let Some(ref source_agent) = query.source_agent {
        sql.push_str(" AND source_agent = ?");
        param_values.push(Box::new(source_agent.clone()));
    }
    if let Some(min_confidence) = query.min_confidence {
        sql.push_str(" AND confidence >= ?");
        param_values.push(Box::new(min_confidence));
    }
    if let Some(max_age_seconds) = query.max_age_seconds {
        let cutoff = (Utc::now() - chrono::Duration::seconds(max_age_seconds as i64)).to_rfc3339();
        sql.push_str(" AND timestamp >= ?");
        param_values.push(Box::new(cutoff));
    }

    sql.push_str(" ORDER BY timestamp DESC");

    if let Some(limit) = query.limit {
        sql.push_str(&format!(" LIMIT {}", limit));
    }

    let params_refs: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(|p| p.as_ref()).collect();

    let mut stmt = connection
        .prepare(&sql)
        .map_err(|e| format!("Failed to prepare fact query: {}", e))?;

    let rows = stmt
        .query_map(params_refs.as_slice(), |row| {
            Ok(FactRecord {
                id: row.get(0)?,
                source_agent: row.get(1)?,
                timestamp: row.get(2)?,
                category: row.get(3)?,
                content: row.get(4)?,
                confidence: row.get(5)?,
                ttl_seconds: row.get(6)?,
            })
        })
        .map_err(|e| format!("Failed to query facts: {}", e))?;

    let mut results = Vec::new();
    for row in rows {
        results.push(row.map_err(|e| format!("Failed to read fact row: {}", e))?);
    }

    // Log successful access
    log_access(connection, agent_id, "read", None)?;

    Ok(results)
}

/// Read a single fact by ID with access control.
pub fn federated_memory_read_by_id_db(
    connection: &Connection,
    agent_id: &str,
    fact_id: &str,
) -> Result<Option<FactRecord>, String> {
    // Validate access control
    if let Err(e) = validate_trusted_agent_with_db(connection, agent_id) {
        log_unauthorized_access(connection, agent_id, "unauthorized-read", Some(fact_id))?;
        return Err(e);
    }

    let mut stmt = connection
        .prepare(
            "SELECT id, source_agent, timestamp, category, content, confidence, ttl_seconds
             FROM facts WHERE id = ?1",
        )
        .map_err(|e| format!("Failed to prepare read-by-id query: {}", e))?;

    let result = stmt
        .query_row(params![fact_id], |row| {
            Ok(FactRecord {
                id: row.get(0)?,
                source_agent: row.get(1)?,
                timestamp: row.get(2)?,
                category: row.get(3)?,
                content: row.get(4)?,
                confidence: row.get(5)?,
                ttl_seconds: row.get(6)?,
            })
        })
        .ok();

    // Log successful access
    log_access(connection, agent_id, "read", Some(fact_id))?;

    Ok(result)
}

/// Promote an agent to the trusted set with a 30-day validation period.
pub fn promote_agent(
    connection: &Connection,
    agent_id: &str,
    promoted_by: &str,
) -> Result<(), String> {
    // Validate that the promoter is trusted
    validate_trusted_agent_with_db(connection, promoted_by)?;

    let now = Utc::now();
    let promoted_at = now.to_rfc3339();
    let validation_period_end = (now + chrono::Duration::days(30)).to_rfc3339();

    connection
        .execute(
            "INSERT OR REPLACE INTO trusted_agent_promotions (agent_id, promoted_by, promoted_at, validation_period_end)
             VALUES (?1, ?2, ?3, ?4)",
            params![agent_id, promoted_by, promoted_at, validation_period_end],
        )
        .map_err(|e| format!("Failed to promote agent: {}", e))?;

    // Log the promotion event
    log_access(connection, promoted_by, "promote", Some(agent_id))?;

    Ok(())
}

/// IPC command: write a fact to federated memory.
#[tauri::command]
pub fn federated_memory_write_cmd(
    app: tauri::AppHandle,
    request: FactWriteRequest,
) -> Result<FactWriteResult, String> {
    use tauri::Manager;
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to get app data dir: {}", e))?;
    let db_path = app_data_dir.join("federated_memory.db");
    let conn = Connection::open(&db_path)
        .map_err(|e| format!("Failed to open federated memory db: {}", e))?;
    initialize_federated_memory_db(&conn)?;
    federated_memory_write(&conn, &request)
}

/// IPC command: query facts from federated memory.
#[tauri::command]
pub fn federated_memory_query_cmd(
    app: tauri::AppHandle,
    agent_id: String,
    query: FactQuery,
) -> Result<Vec<FactRecord>, String> {
    use tauri::Manager;
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to get app data dir: {}", e))?;
    let db_path = app_data_dir.join("federated_memory.db");
    let conn = Connection::open(&db_path)
        .map_err(|e| format!("Failed to open federated memory db: {}", e))?;
    initialize_federated_memory_db(&conn)?;
    federated_memory_query_db(&conn, &agent_id, &query)
}

/// IPC command: read a single fact by ID.
#[tauri::command]
pub fn federated_memory_read_by_id_cmd(
    app: tauri::AppHandle,
    request: FactReadByIdRequest,
) -> Result<Option<FactRecord>, String> {
    use tauri::Manager;
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to get app data dir: {}", e))?;
    let db_path = app_data_dir.join("federated_memory.db");
    let conn = Connection::open(&db_path)
        .map_err(|e| format!("Failed to open federated memory db: {}", e))?;
    initialize_federated_memory_db(&conn)?;
    federated_memory_read_by_id_db(&conn, &request.agent_id, &request.fact_id)
}

/// IPC command: get federated memory status.
#[tauri::command]
pub fn federated_memory_status_cmd(
    app: tauri::AppHandle,
) -> Result<FederatedMemoryStatus, String> {
    use tauri::Manager;
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to get app data dir: {}", e))?;
    let db_path = app_data_dir.join("federated_memory.db");
    let conn = Connection::open(&db_path)
        .map_err(|e| format!("Failed to open federated memory db: {}", e))?;
    initialize_federated_memory_db(&conn)?;

    let total_facts = get_fact_count(&conn)? as u32;
    let trusted_agents: Vec<String> = TRUSTED_AGENT_SET.iter().map(|s| s.to_string()).collect();

    Ok(FederatedMemoryStatus {
        total_facts,
        capacity: MAX_STORE_SIZE as u32,
        trusted_agents,
        available: true,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn create_test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        initialize_federated_memory_db(&conn).unwrap();
        conn
    }

    fn write_fact_for_agent(
        conn: &Connection,
        agent_id: &str,
        category: &str,
        content: &str,
        confidence: f64,
        ttl_seconds: u64,
    ) -> FactWriteResult {
        let request = FactWriteRequest {
            agent_id: agent_id.to_string(),
            category: category.to_string(),
            content: content.to_string(),
            confidence,
            ttl_seconds,
        };
        federated_memory_write(conn, &request).unwrap()
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]

        // Feature: data-infrastructure, Property 10: Fact record storage round-trip preserves all fields
        // **Validates: Requirements 7.1, 9.5**
        #[test]
        fn prop_fact_record_round_trip(
            category_idx in 0usize..4,
            confidence in 0.0f64..1.0,
            ttl_seconds in 60u64..86400,
            word_count in 1usize..50,
        ) {
            let conn = create_test_db();
            let categories = ["system-config", "provider-state", "user-preference", "architecture-decision"];
            let category = categories[category_idx];
            let agent_id = "strategist.core";

            // Generate content within token limit
            let content: String = (0..word_count).map(|i| format!("word{}", i)).collect::<Vec<_>>().join(" ");

            // Skip if content exceeds token limit
            let tokens = estimate_token_count(&content);
            prop_assume!(tokens <= MAX_CONTENT_TOKENS);

            let result = write_fact_for_agent(&conn, agent_id, category, &content, confidence, ttl_seconds);
            prop_assert!(result.accepted, "Write should be accepted");
            prop_assert!(result.error.is_none());

            // Read back by ID
            let read_back = federated_memory_read_by_id_db(&conn, agent_id, &result.id).unwrap();
            prop_assert!(read_back.is_some(), "Should find fact by ID");

            let fact = read_back.unwrap();
            prop_assert_eq!(&fact.id, &result.id);
            prop_assert_eq!(&fact.source_agent, agent_id);
            prop_assert_eq!(&fact.category, category);
            prop_assert_eq!(&fact.content, &content);
            prop_assert!((fact.confidence - confidence.clamp(0.0, 1.0)).abs() < 1e-10);
            prop_assert_eq!(fact.ttl_seconds, ttl_seconds);
        }

        // Feature: data-infrastructure, Property 11: Store size invariant — never exceeds 50 records
        // **Validates: Requirements 7.2**
        #[test]
        fn prop_store_size_invariant(
            num_writes in 45usize..60,
        ) {
            let conn = create_test_db();
            let agent_id = "strategist.core";

            for i in 0..num_writes {
                let content = format!("fact number {}", i);
                let request = FactWriteRequest {
                    agent_id: agent_id.to_string(),
                    category: "system-config".to_string(),
                    content,
                    confidence: 0.8,
                    ttl_seconds: 3600,
                };
                let result = federated_memory_write(&conn, &request).unwrap();
                prop_assert!(result.accepted, "Write {} should be accepted", i);

                // Verify store size never exceeds MAX_STORE_SIZE
                let count = get_fact_count(&conn).unwrap();
                prop_assert!(
                    count <= MAX_STORE_SIZE,
                    "Store size {} exceeds max {} after write {}",
                    count, MAX_STORE_SIZE, i
                );
            }
        }

        // Feature: data-infrastructure, Property 12: Content exceeding 200 tokens is rejected
        // **Validates: Requirements 7.4**
        #[test]
        fn prop_content_token_limit_rejection(
            word_count in 200usize..500,
        ) {
            let conn = create_test_db();
            let agent_id = "strategist.core";

            // Generate content that exceeds 200 tokens
            let content: String = (0..word_count).map(|i| format!("word{}", i)).collect::<Vec<_>>().join(" ");
            let tokens = estimate_token_count(&content);

            // Only test if actually over limit
            prop_assume!(tokens > MAX_CONTENT_TOKENS);

            let request = FactWriteRequest {
                agent_id: agent_id.to_string(),
                category: "system-config".to_string(),
                content: content.clone(),
                confidence: 0.9,
                ttl_seconds: 3600,
            };
            let result = federated_memory_write(&conn, &request).unwrap();

            prop_assert!(!result.accepted, "Write should be rejected for {} tokens", tokens);
            prop_assert!(result.error.is_some(), "Error message should be present");
            prop_assert!(
                result.error.as_ref().unwrap().contains("200 token limit"),
                "Error should mention token limit"
            );

            // Store should remain unchanged
            let count = get_fact_count(&conn).unwrap();
            prop_assert_eq!(count, 0, "Store should be empty after rejected write");
        }

        // Feature: data-infrastructure, Property 13: Access control enforcement
        // **Validates: Requirements 8.1, 8.2, 8.3**
        #[test]
        fn prop_access_control_enforcement(
            agent_idx in 0usize..6,
        ) {
            let conn = create_test_db();
            let agents = [
                "strategist.core",  // trusted
                "setup.core",       // trusted
                "logician.core",    // trusted
                "rogue.agent",      // untrusted
                "hacker.bot",       // untrusted
                "unknown.service",  // untrusted
            ];
            let agent_id = agents[agent_idx];
            let is_trusted = agent_idx < 3;

            // Test write access
            let request = FactWriteRequest {
                agent_id: agent_id.to_string(),
                category: "system-config".to_string(),
                content: "test fact".to_string(),
                confidence: 0.8,
                ttl_seconds: 3600,
            };
            let write_result = federated_memory_write(&conn, &request).unwrap();

            if is_trusted {
                prop_assert!(write_result.accepted,
                    "Trusted agent {} should be able to write", agent_id);
            } else {
                prop_assert!(!write_result.accepted,
                    "Untrusted agent {} should not be able to write", agent_id);
                prop_assert!(write_result.error.is_some());
                prop_assert!(write_result.error.as_ref().unwrap().contains("not in the Trusted_Agent_Set"));
            }

            // Test read access
            let query = FactQuery {
                category: None,
                source_agent: None,
                min_confidence: None,
                max_age_seconds: None,
                limit: None,
            };
            let read_result = federated_memory_query_db(&conn, agent_id, &query);

            if is_trusted {
                prop_assert!(read_result.is_ok(),
                    "Trusted agent {} should be able to read", agent_id);
            } else {
                prop_assert!(read_result.is_err(),
                    "Untrusted agent {} should not be able to read", agent_id);
            }

            // Verify unauthorized access is logged
            if !is_trusted {
                let log_count: u32 = conn.query_row(
                    "SELECT COUNT(*) FROM access_log WHERE agent_id = ?1 AND operation LIKE 'unauthorized%'",
                    params![agent_id],
                    |row| row.get(0),
                ).unwrap();
                prop_assert!(log_count > 0,
                    "Unauthorized access by {} should be logged", agent_id);
            }
        }

        // Feature: data-infrastructure, Property 14: Query filtering returns only matching facts
        // **Validates: Requirements 9.1, 9.2**
        #[test]
        fn prop_query_filtering(
            num_facts in 3usize..10,
            filter_category_idx in 0usize..4,
            min_confidence in 0.0f64..1.0,
        ) {
            let conn = create_test_db();
            let agent_id = "strategist.core";
            let categories = ["system-config", "provider-state", "user-preference", "architecture-decision"];

            // Write facts with varying categories and confidences
            for i in 0..num_facts {
                let cat = categories[i % 4];
                let conf = (i as f64) / (num_facts as f64);
                let content = format!("fact {}", i);
                write_fact_for_agent(&conn, agent_id, cat, &content, conf, 3600);
                // Small delay to ensure distinct timestamps
                std::thread::sleep(std::time::Duration::from_millis(2));
            }

            let filter_category = categories[filter_category_idx];

            // Query with category filter
            let query = FactQuery {
                category: Some(filter_category.to_string()),
                source_agent: None,
                min_confidence: Some(min_confidence),
                max_age_seconds: None,
                limit: None,
            };
            let results = federated_memory_query_db(&conn, agent_id, &query).unwrap();

            // Verify all results match the filter criteria
            for fact in &results {
                prop_assert_eq!(&fact.category, filter_category,
                    "All results should match category filter");
                prop_assert!(fact.confidence >= min_confidence,
                    "All results should meet min_confidence: {} >= {}",
                    fact.confidence, min_confidence);
            }

            // Verify results are sorted by timestamp DESC
            for i in 1..results.len() {
                prop_assert!(
                    results[i - 1].timestamp >= results[i].timestamp,
                    "Results should be sorted by timestamp DESC"
                );
            }
        }
    }
}
