use chrono::{Datelike, Utc};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

/// A single cost record written after each provider API call.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CostRecord {
    pub id: String,
    pub recorded_at: String,
    pub agent_id: String,
    pub task_type: String,
    pub provider_id: String,
    pub model: String,
    pub cost_posture: String,
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
    pub estimated_cost_usd: f64,
    pub duration_ms: Option<u32>,
}

/// Pre-aggregated daily summary for fast dashboard queries.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CostAggregation {
    pub period: String,
    pub period_type: String,
    pub agent_id: String,
    pub task_type: String,
    pub total_prompt_tokens: u64,
    pub total_completion_tokens: u64,
    pub total_tokens: u64,
    pub total_estimated_cost_usd: f64,
    pub record_count: u32,
}

/// Projected monthly spend based on 7-day rolling average.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CostProjection {
    pub daily_average_usd: f64,
    pub projected_monthly_usd: f64,
    pub rolling_window_days: u32,
    pub computed_at: String,
}

/// Query parameters for the cost dashboard.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CostLedgerQuery {
    pub period_type: Option<String>,
    pub agent_id: Option<String>,
    pub task_type: Option<String>,
    pub from_date: Option<String>,
    pub to_date: Option<String>,
    pub limit: Option<u32>,
}

/// Dashboard response combining aggregations and projection.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CostDashboardData {
    pub aggregations: Vec<CostAggregation>,
    pub projection: CostProjection,
    pub recent_records: Vec<CostRecord>,
}

/// Initialize the cost ledger database schema.
pub fn initialize_cost_ledger_db(connection: &Connection) -> Result<(), String> {
    connection
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS cost_records (
                id TEXT PRIMARY KEY,
                recorded_at TEXT NOT NULL,
                agent_id TEXT NOT NULL,
                task_type TEXT NOT NULL,
                provider_id TEXT NOT NULL,
                model TEXT NOT NULL,
                cost_posture TEXT NOT NULL,
                prompt_tokens INTEGER NOT NULL DEFAULT 0,
                completion_tokens INTEGER NOT NULL DEFAULT 0,
                total_tokens INTEGER NOT NULL DEFAULT 0,
                estimated_cost_usd REAL NOT NULL DEFAULT 0.0,
                duration_ms INTEGER
            );

            CREATE TABLE IF NOT EXISTS cost_aggregations (
                period TEXT NOT NULL,
                period_type TEXT NOT NULL,
                agent_id TEXT NOT NULL,
                task_type TEXT NOT NULL,
                total_prompt_tokens INTEGER NOT NULL DEFAULT 0,
                total_completion_tokens INTEGER NOT NULL DEFAULT 0,
                total_tokens INTEGER NOT NULL DEFAULT 0,
                total_estimated_cost_usd REAL NOT NULL DEFAULT 0.0,
                record_count INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (period, period_type, agent_id, task_type)
            );

            CREATE INDEX IF NOT EXISTS idx_cost_records_agent ON cost_records(agent_id);
            CREATE INDEX IF NOT EXISTS idx_cost_records_recorded_at ON cost_records(recorded_at);
            CREATE INDEX IF NOT EXISTS idx_cost_records_task_type ON cost_records(task_type);
            CREATE INDEX IF NOT EXISTS idx_cost_aggregations_period ON cost_aggregations(period, period_type);",
        )
        .map_err(|e| format!("Failed to initialize cost ledger schema: {}", e))
}

/// Estimate cost in USD based on cost posture and token counts.
/// - free-local: 0.0
/// - subscription: 0.0 (included in subscription)
/// - paid-api: $0.002 per 1K tokens (input), $0.006 per 1K tokens (output)
/// - emergency-only: $0.01 per 1K tokens (input), $0.03 per 1K tokens (output)
pub fn estimate_cost_usd(cost_posture: &str, prompt_tokens: u32, completion_tokens: u32) -> f64 {
    match cost_posture {
        "free-local" => 0.0,
        "subscription" => 0.0,
        "paid-api" => {
            let input_cost = (prompt_tokens as f64 / 1000.0) * 0.002;
            let output_cost = (completion_tokens as f64 / 1000.0) * 0.006;
            input_cost + output_cost
        }
        "emergency-only" => {
            let input_cost = (prompt_tokens as f64 / 1000.0) * 0.01;
            let output_cost = (completion_tokens as f64 / 1000.0) * 0.03;
            input_cost + output_cost
        }
        _ => 0.0, // Unknown posture defaults to zero
    }
}

/// Compute the ISO week string for a given date (e.g., "2026-W24").
fn iso_week_string(date_str: &str) -> String {
    // Parse the date and compute ISO week
    if let Ok(date) = chrono::NaiveDate::parse_from_str(&date_str[..10], "%Y-%m-%d") {
        let iso_week = date.iso_week();
        format!("{}-W{:02}", iso_week.year(), iso_week.week())
    } else {
        // Fallback: use the date as-is
        date_str[..10].to_string()
    }
}

/// Record a cost entry: insert the record and upsert daily/weekly aggregation rows.
pub fn record_cost_entry(connection: &Connection, record: &CostRecord) -> Result<(), String> {
    // Insert the raw cost record
    connection
        .execute(
            "INSERT OR REPLACE INTO cost_records (
                id, recorded_at, agent_id, task_type, provider_id, model,
                cost_posture, prompt_tokens, completion_tokens, total_tokens,
                estimated_cost_usd, duration_ms
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                record.id,
                record.recorded_at,
                record.agent_id,
                record.task_type,
                record.provider_id,
                record.model,
                record.cost_posture,
                record.prompt_tokens,
                record.completion_tokens,
                record.total_tokens,
                record.estimated_cost_usd,
                record.duration_ms,
            ],
        )
        .map_err(|e| format!("Failed to insert cost record: {}", e))?;

    // Extract the day portion (first 10 chars of recorded_at)
    let day_period = if record.recorded_at.len() >= 10 {
        record.recorded_at[..10].to_string()
    } else {
        record.recorded_at.clone()
    };

    // Upsert daily aggregation
    upsert_aggregation(
        connection,
        &day_period,
        "day",
        &record.agent_id,
        &record.task_type,
        record.prompt_tokens as u64,
        record.completion_tokens as u64,
        record.total_tokens as u64,
        record.estimated_cost_usd,
    )?;

    // Upsert weekly aggregation
    let week_period = iso_week_string(&record.recorded_at);
    upsert_aggregation(
        connection,
        &week_period,
        "week",
        &record.agent_id,
        &record.task_type,
        record.prompt_tokens as u64,
        record.completion_tokens as u64,
        record.total_tokens as u64,
        record.estimated_cost_usd,
    )?;

    Ok(())
}

/// Upsert an aggregation row (INSERT or UPDATE existing).
fn upsert_aggregation(
    connection: &Connection,
    period: &str,
    period_type: &str,
    agent_id: &str,
    task_type: &str,
    prompt_tokens: u64,
    completion_tokens: u64,
    total_tokens: u64,
    estimated_cost_usd: f64,
) -> Result<(), String> {
    connection
        .execute(
            "INSERT INTO cost_aggregations (
                period, period_type, agent_id, task_type,
                total_prompt_tokens, total_completion_tokens, total_tokens,
                total_estimated_cost_usd, record_count
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 1)
            ON CONFLICT(period, period_type, agent_id, task_type) DO UPDATE SET
                total_prompt_tokens = total_prompt_tokens + ?5,
                total_completion_tokens = total_completion_tokens + ?6,
                total_tokens = total_tokens + ?7,
                total_estimated_cost_usd = total_estimated_cost_usd + ?8,
                record_count = record_count + 1",
            params![
                period,
                period_type,
                agent_id,
                task_type,
                prompt_tokens,
                completion_tokens,
                total_tokens,
                estimated_cost_usd,
            ],
        )
        .map_err(|e| format!("Failed to upsert aggregation: {}", e))?;
    Ok(())
}

/// Query the cost dashboard: read aggregations by period/agent/task_type with date range filtering.
pub fn query_cost_dashboard(
    connection: &Connection,
    query: &CostLedgerQuery,
) -> Result<CostDashboardData, String> {
    let aggregations = query_aggregations(connection, query)?;
    let projection = cost_ledger_projection_from_db(connection)?;
    let recent_records = query_recent_records(connection, query)?;

    Ok(CostDashboardData {
        aggregations,
        projection,
        recent_records,
    })
}

/// Query aggregation rows with optional filters.
fn query_aggregations(
    connection: &Connection,
    query: &CostLedgerQuery,
) -> Result<Vec<CostAggregation>, String> {
    let mut sql = String::from(
        "SELECT period, period_type, agent_id, task_type,
                total_prompt_tokens, total_completion_tokens, total_tokens,
                total_estimated_cost_usd, record_count
         FROM cost_aggregations WHERE 1=1",
    );
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

    if let Some(ref period_type) = query.period_type {
        sql.push_str(" AND period_type = ?");
        param_values.push(Box::new(period_type.clone()));
    }
    if let Some(ref agent_id) = query.agent_id {
        sql.push_str(" AND agent_id = ?");
        param_values.push(Box::new(agent_id.clone()));
    }
    if let Some(ref task_type) = query.task_type {
        sql.push_str(" AND task_type = ?");
        param_values.push(Box::new(task_type.clone()));
    }
    if let Some(ref from_date) = query.from_date {
        sql.push_str(" AND period >= ?");
        param_values.push(Box::new(from_date.clone()));
    }
    if let Some(ref to_date) = query.to_date {
        sql.push_str(" AND period <= ?");
        param_values.push(Box::new(to_date.clone()));
    }

    sql.push_str(" ORDER BY period DESC");

    if let Some(limit) = query.limit {
        sql.push_str(&format!(" LIMIT {}", limit));
    }

    let params_refs: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(|p| p.as_ref()).collect();

    let mut stmt = connection
        .prepare(&sql)
        .map_err(|e| format!("Failed to prepare aggregation query: {}", e))?;

    let rows = stmt
        .query_map(params_refs.as_slice(), |row| {
            Ok(CostAggregation {
                period: row.get(0)?,
                period_type: row.get(1)?,
                agent_id: row.get(2)?,
                task_type: row.get(3)?,
                total_prompt_tokens: row.get(4)?,
                total_completion_tokens: row.get(5)?,
                total_tokens: row.get(6)?,
                total_estimated_cost_usd: row.get(7)?,
                record_count: row.get(8)?,
            })
        })
        .map_err(|e| format!("Failed to query aggregations: {}", e))?;

    let mut results = Vec::new();
    for row in rows {
        results.push(row.map_err(|e| format!("Failed to read aggregation row: {}", e))?);
    }
    Ok(results)
}

/// Query recent cost records with optional filters.
fn query_recent_records(
    connection: &Connection,
    query: &CostLedgerQuery,
) -> Result<Vec<CostRecord>, String> {
    let mut sql = String::from(
        "SELECT id, recorded_at, agent_id, task_type, provider_id, model,
                cost_posture, prompt_tokens, completion_tokens, total_tokens,
                estimated_cost_usd, duration_ms
         FROM cost_records WHERE 1=1",
    );
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

    if let Some(ref agent_id) = query.agent_id {
        sql.push_str(" AND agent_id = ?");
        param_values.push(Box::new(agent_id.clone()));
    }
    if let Some(ref task_type) = query.task_type {
        sql.push_str(" AND task_type = ?");
        param_values.push(Box::new(task_type.clone()));
    }
    if let Some(ref from_date) = query.from_date {
        sql.push_str(" AND recorded_at >= ?");
        param_values.push(Box::new(from_date.clone()));
    }
    if let Some(ref to_date) = query.to_date {
        sql.push_str(" AND recorded_at <= ?");
        param_values.push(Box::new(to_date.clone()));
    }

    sql.push_str(" ORDER BY recorded_at DESC");

    let limit = query.limit.unwrap_or(50);
    sql.push_str(&format!(" LIMIT {}", limit));

    let params_refs: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(|p| p.as_ref()).collect();

    let mut stmt = connection
        .prepare(&sql)
        .map_err(|e| format!("Failed to prepare records query: {}", e))?;

    let rows = stmt
        .query_map(params_refs.as_slice(), |row| {
            Ok(CostRecord {
                id: row.get(0)?,
                recorded_at: row.get(1)?,
                agent_id: row.get(2)?,
                task_type: row.get(3)?,
                provider_id: row.get(4)?,
                model: row.get(5)?,
                cost_posture: row.get(6)?,
                prompt_tokens: row.get(7)?,
                completion_tokens: row.get(8)?,
                total_tokens: row.get(9)?,
                estimated_cost_usd: row.get(10)?,
                duration_ms: row.get(11)?,
            })
        })
        .map_err(|e| format!("Failed to query records: {}", e))?;

    let mut results = Vec::new();
    for row in rows {
        results.push(row.map_err(|e| format!("Failed to read record row: {}", e))?);
    }
    Ok(results)
}

/// Compute 7-day rolling average × 30.44 for monthly projection.
pub fn cost_ledger_projection_from_db(connection: &Connection) -> Result<CostProjection, String> {
    let mut stmt = connection
        .prepare(
            "SELECT COALESCE(SUM(total_estimated_cost_usd), 0.0)
             FROM cost_aggregations
             WHERE period_type = 'day'
               AND period >= date('now', '-7 days')",
        )
        .map_err(|e| format!("Failed to prepare projection query: {}", e))?;

    let sum_7_days: f64 = stmt
        .query_row([], |row| row.get(0))
        .map_err(|e| format!("Failed to compute projection: {}", e))?;

    let daily_average = sum_7_days / 7.0;
    let projected_monthly = daily_average * 30.44;

    Ok(CostProjection {
        daily_average_usd: daily_average,
        projected_monthly_usd: projected_monthly,
        rolling_window_days: 7,
        computed_at: Utc::now().to_rfc3339(),
    })
}

/// Compute projection from a set of daily totals (pure function for testing).
pub fn compute_projection(daily_totals: &[f64]) -> CostProjection {
    let sum: f64 = daily_totals.iter().sum();
    let _days = if daily_totals.is_empty() {
        7.0
    } else {
        daily_totals.len().min(7) as f64
    };
    let daily_average = sum / 7.0;
    let projected_monthly = daily_average * 30.44;

    CostProjection {
        daily_average_usd: daily_average,
        projected_monthly_usd: projected_monthly,
        rolling_window_days: 7,
        computed_at: Utc::now().to_rfc3339(),
    }
}

/// Event listener setup for `cost-record-created` Tauri event.
/// This should be called during app setup to register the listener.
pub fn setup_cost_record_event_listener(app_handle: tauri::AppHandle, db_path: std::path::PathBuf) {
    use tauri::Listener;
    app_handle.listen("cost-record-created", move |event| {
        let payload_str = event.payload();
        if let Ok(record) = serde_json::from_str::<CostRecord>(payload_str) {
            // Non-blocking write: open connection and write
            if let Ok(conn) = Connection::open(&db_path) {
                let _ = record_cost_entry(&conn, &record);
            }
        }
    });
}

/// IPC command: record a cost entry.
#[tauri::command]
pub fn cost_ledger_record(
    app: tauri::AppHandle,
    record: CostRecord,
) -> Result<(), String> {
    use tauri::Manager;
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to get app data dir: {}", e))?;
    let db_path = app_data_dir.join("cost_ledger.db");
    let conn = Connection::open(&db_path)
        .map_err(|e| format!("Failed to open cost ledger db: {}", e))?;
    initialize_cost_ledger_db(&conn)?;
    record_cost_entry(&conn, &record)
}

/// IPC command: query cost dashboard data.
#[tauri::command]
pub fn cost_ledger_query(
    app: tauri::AppHandle,
    query: CostLedgerQuery,
) -> Result<CostDashboardData, String> {
    use tauri::Manager;
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to get app data dir: {}", e))?;
    let db_path = app_data_dir.join("cost_ledger.db");
    let conn = Connection::open(&db_path)
        .map_err(|e| format!("Failed to open cost ledger db: {}", e))?;
    initialize_cost_ledger_db(&conn)?;
    query_cost_dashboard(&conn, &query)
}

/// IPC command: get cost projection.
#[tauri::command]
pub fn cost_ledger_projection(
    app: tauri::AppHandle,
) -> Result<CostProjection, String> {
    use tauri::Manager;
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to get app data dir: {}", e))?;
    let db_path = app_data_dir.join("cost_ledger.db");
    let conn = Connection::open(&db_path)
        .map_err(|e| format!("Failed to open cost ledger db: {}", e))?;
    initialize_cost_ledger_db(&conn)?;
    cost_ledger_projection_from_db(&conn)
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn create_test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        initialize_cost_ledger_db(&conn).unwrap();
        conn
    }

    fn make_cost_record(
        id: &str,
        agent_id: &str,
        task_type: &str,
        cost_posture: &str,
        prompt_tokens: u32,
        completion_tokens: u32,
    ) -> CostRecord {
        let total = prompt_tokens + completion_tokens;
        let cost = estimate_cost_usd(cost_posture, prompt_tokens, completion_tokens);
        CostRecord {
            id: id.to_string(),
            recorded_at: "2026-06-15T10:00:00Z".to_string(),
            agent_id: agent_id.to_string(),
            task_type: task_type.to_string(),
            provider_id: "provider-1".to_string(),
            model: "gpt-4".to_string(),
            cost_posture: cost_posture.to_string(),
            prompt_tokens,
            completion_tokens,
            total_tokens: total,
            estimated_cost_usd: cost,
            duration_ms: Some(100),
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]

        // Feature: data-infrastructure, Property 6: Cost record persistence round-trip
        // **Validates: Requirements 4.2, 4.3**
        #[test]
        fn prop_cost_record_round_trip(
            agent_id in "[a-z]{3,8}",
            task_type in "(coding|research|planning|review)",
            prompt_tokens in 0u32..100000,
            completion_tokens in 0u32..100000,
        ) {
            let conn = create_test_db();
            let id = format!("test-{}-{}", agent_id, prompt_tokens);
            let record = make_cost_record(
                &id, &agent_id, &task_type, "paid-api",
                prompt_tokens, completion_tokens,
            );

            record_cost_entry(&conn, &record).unwrap();

            // Read back by ID
            let mut stmt = conn.prepare(
                "SELECT id, recorded_at, agent_id, task_type, provider_id, model,
                        cost_posture, prompt_tokens, completion_tokens, total_tokens,
                        estimated_cost_usd, duration_ms
                 FROM cost_records WHERE id = ?1"
            ).unwrap();

            let read_back: CostRecord = stmt.query_row(params![id], |row| {
                Ok(CostRecord {
                    id: row.get(0)?,
                    recorded_at: row.get(1)?,
                    agent_id: row.get(2)?,
                    task_type: row.get(3)?,
                    provider_id: row.get(4)?,
                    model: row.get(5)?,
                    cost_posture: row.get(6)?,
                    prompt_tokens: row.get(7)?,
                    completion_tokens: row.get(8)?,
                    total_tokens: row.get(9)?,
                    estimated_cost_usd: row.get(10)?,
                    duration_ms: row.get(11)?,
                })
            }).unwrap();

            prop_assert_eq!(&read_back.id, &record.id);
            prop_assert_eq!(&read_back.agent_id, &record.agent_id);
            prop_assert_eq!(&read_back.task_type, &record.task_type);
            prop_assert_eq!(read_back.prompt_tokens, record.prompt_tokens);
            prop_assert_eq!(read_back.completion_tokens, record.completion_tokens);
            prop_assert_eq!(read_back.total_tokens, record.total_tokens);
            prop_assert!((read_back.estimated_cost_usd - record.estimated_cost_usd).abs() < 1e-10);
        }

        // Feature: data-infrastructure, Property 7: Cost aggregation correctness
        // **Validates: Requirements 4.1, 4.4**
        #[test]
        fn prop_cost_aggregation_correctness(
            num_records in 1usize..10,
            prompt_tokens in prop::collection::vec(100u32..5000, 1..10),
            completion_tokens in prop::collection::vec(50u32..3000, 1..10),
        ) {
            let conn = create_test_db();
            let agent_id = "test-agent";
            let task_type = "coding";
            let num = num_records.min(prompt_tokens.len()).min(completion_tokens.len());

            let mut expected_total_tokens: u64 = 0;
            let mut expected_count: u32 = 0;

            for i in 0..num {
                let record = make_cost_record(
                    &format!("rec-{}", i),
                    agent_id,
                    task_type,
                    "paid-api",
                    prompt_tokens[i],
                    completion_tokens[i],
                );
                expected_total_tokens += record.total_tokens as u64;
                expected_count += 1;
                record_cost_entry(&conn, &record).unwrap();
            }

            // Check aggregation for the day
            let mut stmt = conn.prepare(
                "SELECT total_tokens, record_count FROM cost_aggregations
                 WHERE period = '2026-06-15' AND period_type = 'day'
                   AND agent_id = ?1 AND task_type = ?2"
            ).unwrap();

            let (agg_tokens, agg_count): (u64, u32) = stmt.query_row(
                params![agent_id, task_type],
                |row| Ok((row.get(0)?, row.get(1)?))
            ).unwrap();

            prop_assert_eq!(agg_tokens, expected_total_tokens,
                "Aggregation total_tokens mismatch");
            prop_assert_eq!(agg_count, expected_count,
                "Aggregation record_count mismatch");
        }

        // Feature: data-infrastructure, Property 8: Monthly projection uses 7-day rolling average
        // **Validates: Requirements 5.3**
        #[test]
        fn prop_monthly_projection(
            daily_totals in prop::collection::vec(0.0f64..100.0, 1..8),
        ) {
            let projection = compute_projection(&daily_totals);

            let sum: f64 = daily_totals.iter().sum();
            let expected_daily_avg = sum / 7.0;
            let expected_monthly = expected_daily_avg * 30.44;

            prop_assert!(
                (projection.daily_average_usd - expected_daily_avg).abs() < 1e-10,
                "Daily average mismatch: {} vs {}", projection.daily_average_usd, expected_daily_avg
            );
            prop_assert!(
                (projection.projected_monthly_usd - expected_monthly).abs() < 1e-6,
                "Monthly projection mismatch: {} vs {}", projection.projected_monthly_usd, expected_monthly
            );
            prop_assert_eq!(projection.rolling_window_days, 7);
        }

        // Feature: data-infrastructure, Property 9: Cost posture derivation is deterministic
        // **Validates: Requirements 5.1**
        #[test]
        fn prop_cost_posture_deterministic(
            prompt_tokens in 0u32..100000,
            completion_tokens in 0u32..100000,
            posture_idx in 0usize..4,
        ) {
            let postures = ["free-local", "subscription", "paid-api", "emergency-only"];
            let posture = postures[posture_idx];

            let cost1 = estimate_cost_usd(posture, prompt_tokens, completion_tokens);
            let cost2 = estimate_cost_usd(posture, prompt_tokens, completion_tokens);

            // Deterministic: same inputs produce same output
            prop_assert_eq!(cost1.to_bits(), cost2.to_bits(),
                "Cost estimation must be deterministic");

            // Verify correct rates
            match posture {
                "free-local" => {
                    prop_assert_eq!(cost1, 0.0, "free-local must be zero cost");
                }
                "subscription" => {
                    prop_assert_eq!(cost1, 0.0, "subscription must be zero cost");
                }
                "paid-api" => {
                    let expected = (prompt_tokens as f64 / 1000.0) * 0.002
                        + (completion_tokens as f64 / 1000.0) * 0.006;
                    prop_assert!((cost1 - expected).abs() < 1e-10,
                        "paid-api cost mismatch: {} vs {}", cost1, expected);
                }
                "emergency-only" => {
                    let expected = (prompt_tokens as f64 / 1000.0) * 0.01
                        + (completion_tokens as f64 / 1000.0) * 0.03;
                    prop_assert!((cost1 - expected).abs() < 1e-10,
                        "emergency-only cost mismatch: {} vs {}", cost1, expected);
                }
                _ => {}
            }
        }
    }
}
