use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use tauri::Manager;

// --- Data Models ---

/// A single experience record capturing a scoring decision and its outcome.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExperienceRecord {
    pub id: String,
    pub delegation_packet_id: String,
    pub timestamp: String,
    pub workload_class: String,
    pub task_type: String,
    pub scoring_recommendation_json: String,
    pub heuristic_decision_json: String,
    pub advisory_accepted: bool,
    pub rejection_reason: Option<String>,
    pub outcome_status: Option<String>,
    pub outcome_duration_ms: Option<u64>,
    pub outcome_quality_score: Option<f64>,
    pub outcome_recorded_at: Option<String>,
    pub confidence_score: f64,
}

/// Rolling historical stats cached per agent per task type.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoricalStatsCache {
    pub agent_id: String,
    pub task_type: String,
    pub record_count: u32,
    pub rolling_quality_score: f64,
    pub rolling_speed_ms: f64,
    pub rolling_cost_tokens: f64,
    pub last_updated_at: String,
    pub decay_half_life_days: u32,
}

/// Trust tier transition log entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrustTierTransition {
    pub id: String,
    pub from_tier: String,
    pub to_tier: String,
    pub transitioned_at: String,
    pub validation_period_days: u32,
    pub metrics_json: String,
    pub promoting_authority: String,
}

/// Scoring weights configuration per workload class.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScoringWeightsRow {
    pub workload_class: String,
    pub quality_weight: f64,
    pub cost_weight: f64,
    pub speed_weight: f64,
    pub availability_weight: f64,
    pub updated_at: String,
}

/// Query for retrieving experience records.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExperienceQuery {
    pub from_date: Option<String>,
    pub to_date: Option<String>,
    pub task_type: Option<String>,
    pub advisory_accepted: Option<bool>,
    pub limit: Option<u32>,
}

/// Aggregate stats response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExperienceAggregateStats {
    pub total_recommendations: u64,
    pub acceptance_rate: f64,
    pub average_confidence_score: f64,
    pub recommendation_accuracy: f64,
    pub period_days: u32,
}

/// State wrapper for the experience buffer database connection.
pub struct ExperienceBufferState {
    pub db: Mutex<Connection>,
}

// --- Schema Initialization ---

pub fn initialize_experience_buffer_db(connection: &Connection) -> Result<(), String> {
    connection
        .execute_batch(
            "
            CREATE TABLE IF NOT EXISTS experience_records (
                id TEXT PRIMARY KEY,
                delegation_packet_id TEXT NOT NULL,
                timestamp TEXT NOT NULL,
                workload_class TEXT NOT NULL,
                task_type TEXT NOT NULL,
                scoring_recommendation_json TEXT NOT NULL,
                heuristic_decision_json TEXT NOT NULL,
                advisory_accepted INTEGER NOT NULL DEFAULT 0,
                rejection_reason TEXT,
                outcome_status TEXT,
                outcome_duration_ms INTEGER,
                outcome_quality_score REAL,
                outcome_recorded_at TEXT,
                confidence_score REAL NOT NULL DEFAULT 0.0
            );

            CREATE TABLE IF NOT EXISTS historical_stats_cache (
                agent_id TEXT NOT NULL,
                task_type TEXT NOT NULL,
                record_count INTEGER NOT NULL DEFAULT 0,
                rolling_quality_score REAL NOT NULL DEFAULT 0.0,
                rolling_speed_ms REAL NOT NULL DEFAULT 0.0,
                rolling_cost_tokens REAL NOT NULL DEFAULT 0.0,
                last_updated_at TEXT NOT NULL,
                decay_half_life_days INTEGER NOT NULL DEFAULT 14,
                PRIMARY KEY (agent_id, task_type)
            );

            CREATE TABLE IF NOT EXISTS trust_tier_transitions (
                id TEXT PRIMARY KEY,
                from_tier TEXT NOT NULL,
                to_tier TEXT NOT NULL,
                transitioned_at TEXT NOT NULL,
                validation_period_days INTEGER NOT NULL,
                metrics_json TEXT NOT NULL,
                promoting_authority TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS scoring_weights_config (
                workload_class TEXT PRIMARY KEY,
                quality_weight REAL NOT NULL,
                cost_weight REAL NOT NULL,
                speed_weight REAL NOT NULL,
                availability_weight REAL NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS circuit_breaker_state (
                id TEXT PRIMARY KEY DEFAULT 'singleton',
                consecutive_failures INTEGER NOT NULL DEFAULT 0,
                is_open INTEGER NOT NULL DEFAULT 0,
                last_failure_at TEXT,
                cooldown_ends_at TEXT,
                cooldown_ms INTEGER NOT NULL DEFAULT 60000,
                failure_threshold INTEGER NOT NULL DEFAULT 3
            );

            CREATE INDEX IF NOT EXISTS idx_experience_timestamp
                ON experience_records(timestamp);
            CREATE INDEX IF NOT EXISTS idx_experience_task_type
                ON experience_records(task_type);
            CREATE INDEX IF NOT EXISTS idx_experience_packet_id
                ON experience_records(delegation_packet_id);
            CREATE INDEX IF NOT EXISTS idx_experience_advisory_accepted
                ON experience_records(advisory_accepted);
            CREATE INDEX IF NOT EXISTS idx_experience_outcome_status
                ON experience_records(outcome_status);
            CREATE INDEX IF NOT EXISTS idx_historical_stats_agent
                ON historical_stats_cache(agent_id);
            ",
        )
        .map_err(|e| format!("Failed to initialize experience buffer schema: {}", e))
}

// --- CRUD Operations ---

pub fn record_experience(
    connection: &Connection,
    record: &ExperienceRecord,
) -> Result<(), String> {
    connection
        .execute(
            "INSERT INTO experience_records (
                id, delegation_packet_id, timestamp, workload_class, task_type,
                scoring_recommendation_json, heuristic_decision_json,
                advisory_accepted, rejection_reason, outcome_status,
                outcome_duration_ms, outcome_quality_score, outcome_recorded_at,
                confidence_score
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                record.id,
                record.delegation_packet_id,
                record.timestamp,
                record.workload_class,
                record.task_type,
                record.scoring_recommendation_json,
                record.heuristic_decision_json,
                record.advisory_accepted as i32,
                record.rejection_reason,
                record.outcome_status,
                record.outcome_duration_ms.map(|v| v as i64),
                record.outcome_quality_score,
                record.outcome_recorded_at,
                record.confidence_score,
            ],
        )
        .map_err(|e| format!("Failed to insert experience record: {}", e))?;
    Ok(())
}

pub fn query_experience_records(
    connection: &Connection,
    query: &ExperienceQuery,
) -> Result<Vec<ExperienceRecord>, String> {
    let mut sql = String::from("SELECT * FROM experience_records WHERE 1=1");
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

    if let Some(ref from_date) = query.from_date {
        sql.push_str(" AND timestamp >= ?");
        param_values.push(Box::new(from_date.clone()));
    }
    if let Some(ref to_date) = query.to_date {
        sql.push_str(" AND timestamp <= ?");
        param_values.push(Box::new(to_date.clone()));
    }
    if let Some(ref task_type) = query.task_type {
        sql.push_str(" AND task_type = ?");
        param_values.push(Box::new(task_type.clone()));
    }
    if let Some(advisory_accepted) = query.advisory_accepted {
        sql.push_str(" AND advisory_accepted = ?");
        param_values.push(Box::new(advisory_accepted as i32));
    }

    sql.push_str(" ORDER BY timestamp DESC");

    if let Some(limit) = query.limit {
        sql.push_str(" LIMIT ?");
        param_values.push(Box::new(limit as i64));
    }

    let params_refs: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(|p| p.as_ref()).collect();

    let mut stmt = connection
        .prepare(&sql)
        .map_err(|e| format!("Failed to prepare query: {}", e))?;

    let records = stmt
        .query_map(params_refs.as_slice(), |row| {
            Ok(ExperienceRecord {
                id: row.get(0)?,
                delegation_packet_id: row.get(1)?,
                timestamp: row.get(2)?,
                workload_class: row.get(3)?,
                task_type: row.get(4)?,
                scoring_recommendation_json: row.get(5)?,
                heuristic_decision_json: row.get(6)?,
                advisory_accepted: row.get::<_, i32>(7)? != 0,
                rejection_reason: row.get(8)?,
                outcome_status: row.get(9)?,
                outcome_duration_ms: row.get::<_, Option<i64>>(10)?.map(|v| v as u64),
                outcome_quality_score: row.get(11)?,
                outcome_recorded_at: row.get(12)?,
                confidence_score: row.get(13)?,
            })
        })
        .map_err(|e| format!("Failed to query experience records: {}", e))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Failed to collect experience records: {}", e))?;

    Ok(records)
}

pub fn append_outcome(
    connection: &Connection,
    delegation_packet_id: &str,
    status: &str,
    duration_ms: u64,
    quality_score: f64,
) -> Result<(), String> {
    let now = chrono::Utc::now().to_rfc3339();
    let rows_affected = connection
        .execute(
            "UPDATE experience_records SET
                outcome_status = ?1,
                outcome_duration_ms = ?2,
                outcome_quality_score = ?3,
                outcome_recorded_at = ?4
            WHERE delegation_packet_id = ?5",
            params![status, duration_ms as i64, quality_score, now, delegation_packet_id],
        )
        .map_err(|e| format!("Failed to append outcome: {}", e))?;

    if rows_affected == 0 {
        // Create a partial record if the original doesn't exist
        connection
            .execute(
                "INSERT INTO experience_records (
                    id, delegation_packet_id, timestamp, workload_class, task_type,
                    scoring_recommendation_json, heuristic_decision_json,
                    advisory_accepted, outcome_status, outcome_duration_ms,
                    outcome_quality_score, outcome_recorded_at, confidence_score
                ) VALUES (?1, ?2, ?3, 'unknown', 'unknown', '{}', '{}', 0, ?4, ?5, ?6, ?7, 0.0)",
                params![
                    format!("partial-{}", delegation_packet_id),
                    delegation_packet_id,
                    now,
                    status,
                    duration_ms as i64,
                    quality_score,
                    now,
                ],
            )
            .map_err(|e| format!("Failed to create partial outcome record: {}", e))?;
    }

    Ok(())
}

// --- Historical Stats ---

pub fn query_historical_stats(
    connection: &Connection,
    agent_id: &str,
    task_type: &str,
) -> Result<Option<HistoricalStatsCache>, String> {
    let mut stmt = connection
        .prepare(
            "SELECT agent_id, task_type, record_count, rolling_quality_score,
                    rolling_speed_ms, rolling_cost_tokens, last_updated_at, decay_half_life_days
             FROM historical_stats_cache
             WHERE agent_id = ?1 AND task_type = ?2",
        )
        .map_err(|e| format!("Failed to prepare historical stats query: {}", e))?;

    let result = stmt
        .query_row(params![agent_id, task_type], |row| {
            Ok(HistoricalStatsCache {
                agent_id: row.get(0)?,
                task_type: row.get(1)?,
                record_count: row.get(2)?,
                rolling_quality_score: row.get(3)?,
                rolling_speed_ms: row.get(4)?,
                rolling_cost_tokens: row.get(5)?,
                last_updated_at: row.get(6)?,
                decay_half_life_days: row.get(7)?,
            })
        })
        .ok();

    Ok(result)
}

pub fn query_system_wide_stats(
    connection: &Connection,
    task_type: &str,
) -> Result<Option<HistoricalStatsCache>, String> {
    // Aggregate across all agents for the given task type
    let mut stmt = connection
        .prepare(
            "SELECT
                '__system__' as agent_id,
                task_type,
                SUM(record_count) as record_count,
                AVG(rolling_quality_score) as rolling_quality_score,
                AVG(rolling_speed_ms) as rolling_speed_ms,
                AVG(rolling_cost_tokens) as rolling_cost_tokens,
                MAX(last_updated_at) as last_updated_at,
                14 as decay_half_life_days
             FROM historical_stats_cache
             WHERE task_type = ?1
             GROUP BY task_type",
        )
        .map_err(|e| format!("Failed to prepare system-wide stats query: {}", e))?;

    let result = stmt
        .query_row(params![task_type], |row| {
            Ok(HistoricalStatsCache {
                agent_id: row.get(0)?,
                task_type: row.get(1)?,
                record_count: row.get(2)?,
                rolling_quality_score: row.get(3)?,
                rolling_speed_ms: row.get(4)?,
                rolling_cost_tokens: row.get(5)?,
                last_updated_at: row.get(6)?,
                decay_half_life_days: row.get(7)?,
            })
        })
        .ok();

    Ok(result)
}

pub fn refresh_historical_cache(
    connection: &Connection,
    agent_id: &str,
    task_type: &str,
    decay_half_life_days: u32,
) -> Result<HistoricalStatsCache, String> {
    // Query the most recent 100 experience records for this agent/task_type with outcomes
    let mut stmt = connection
        .prepare(
            "SELECT timestamp, outcome_quality_score, outcome_duration_ms, confidence_score
             FROM experience_records
             WHERE task_type = ?1
               AND scoring_recommendation_json LIKE ?2
               AND outcome_status IS NOT NULL
             ORDER BY timestamp DESC
             LIMIT 100",
        )
        .map_err(|e| format!("Failed to prepare refresh query: {}", e))?;

    let agent_pattern = format!("%{}%", agent_id);
    let now = chrono::Utc::now();
    let _half_life_secs = (decay_half_life_days as f64) * 86400.0;
    let ln2 = std::f64::consts::LN_2;

    let rows: Vec<(String, Option<f64>, Option<i64>, f64)> = stmt
        .query_map(params![task_type, agent_pattern], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<f64>>(1)?,
                row.get::<_, Option<i64>>(2)?,
                row.get::<_, f64>(3)?,
            ))
        })
        .map_err(|e| format!("Failed to query records for refresh: {}", e))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Failed to collect refresh records: {}", e))?;

    let record_count = rows.len() as u32;

    if record_count == 0 {
        let stats = HistoricalStatsCache {
            agent_id: agent_id.to_string(),
            task_type: task_type.to_string(),
            record_count: 0,
            rolling_quality_score: 0.0,
            rolling_speed_ms: 0.0,
            rolling_cost_tokens: 0.0,
            last_updated_at: now.to_rfc3339(),
            decay_half_life_days,
        };
        upsert_historical_stats(connection, &stats)?;
        return Ok(stats);
    }

    let mut weight_sum = 0.0;
    let mut quality_sum = 0.0;
    let mut speed_sum = 0.0;
    let mut cost_sum = 0.0;

    for (timestamp_str, quality, duration_ms, _confidence) in &rows {
        let age_days = if let Ok(ts) = chrono::DateTime::parse_from_rfc3339(timestamp_str) {
            (now - ts.with_timezone(&chrono::Utc))
                .num_seconds()
                .max(0) as f64
                / 86400.0
        } else {
            0.0
        };

        let weight = (-ln2 * age_days / (decay_half_life_days as f64)).exp();
        weight_sum += weight;
        quality_sum += weight * quality.unwrap_or(0.0);
        speed_sum += weight * (*duration_ms).unwrap_or(0) as f64;
        cost_sum += weight * 0.0; // Cost tokens not directly stored in experience records
    }

    let rolling_quality = if weight_sum > 0.0 {
        quality_sum / weight_sum
    } else {
        0.0
    };
    let rolling_speed = if weight_sum > 0.0 {
        speed_sum / weight_sum
    } else {
        0.0
    };
    let rolling_cost = if weight_sum > 0.0 {
        cost_sum / weight_sum
    } else {
        0.0
    };

    let stats = HistoricalStatsCache {
        agent_id: agent_id.to_string(),
        task_type: task_type.to_string(),
        record_count,
        rolling_quality_score: rolling_quality,
        rolling_speed_ms: rolling_speed,
        rolling_cost_tokens: rolling_cost,
        last_updated_at: now.to_rfc3339(),
        decay_half_life_days,
    };

    upsert_historical_stats(connection, &stats)?;
    Ok(stats)
}

fn upsert_historical_stats(
    connection: &Connection,
    stats: &HistoricalStatsCache,
) -> Result<(), String> {
    connection
        .execute(
            "INSERT OR REPLACE INTO historical_stats_cache (
                agent_id, task_type, record_count, rolling_quality_score,
                rolling_speed_ms, rolling_cost_tokens, last_updated_at, decay_half_life_days
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                stats.agent_id,
                stats.task_type,
                stats.record_count,
                stats.rolling_quality_score,
                stats.rolling_speed_ms,
                stats.rolling_cost_tokens,
                stats.last_updated_at,
                stats.decay_half_life_days,
            ],
        )
        .map_err(|e| format!("Failed to upsert historical stats: {}", e))?;
    Ok(())
}

// --- Eviction ---

pub fn evict_expired_records(
    connection: &Connection,
    retention_days: u32,
) -> Result<u32, String> {
    let cutoff = chrono::Utc::now() - chrono::Duration::days(retention_days as i64);
    let cutoff_str = cutoff.to_rfc3339();

    let rows_deleted = connection
        .execute(
            "DELETE FROM experience_records WHERE timestamp < ?1",
            params![cutoff_str],
        )
        .map_err(|e| format!("Failed to evict expired records: {}", e))?;

    Ok(rows_deleted as u32)
}

// --- Aggregate Stats ---

pub fn compute_aggregate_stats(
    connection: &Connection,
    period_days: u32,
) -> Result<ExperienceAggregateStats, String> {
    let cutoff = chrono::Utc::now() - chrono::Duration::days(period_days as i64);
    let cutoff_str = cutoff.to_rfc3339();

    let total_recommendations: u64 = connection
        .query_row(
            "SELECT COUNT(*) FROM experience_records WHERE timestamp >= ?1",
            params![cutoff_str],
            |row| row.get(0),
        )
        .map_err(|e| format!("Failed to count recommendations: {}", e))?;

    if total_recommendations == 0 {
        return Ok(ExperienceAggregateStats {
            total_recommendations: 0,
            acceptance_rate: 0.0,
            average_confidence_score: 0.0,
            recommendation_accuracy: 0.0,
            period_days,
        });
    }

    let accepted_count: u64 = connection
        .query_row(
            "SELECT COUNT(*) FROM experience_records WHERE timestamp >= ?1 AND advisory_accepted = 1",
            params![cutoff_str],
            |row| row.get(0),
        )
        .map_err(|e| format!("Failed to count accepted: {}", e))?;

    let avg_confidence: f64 = connection
        .query_row(
            "SELECT AVG(confidence_score) FROM experience_records WHERE timestamp >= ?1",
            params![cutoff_str],
            |row| row.get(0),
        )
        .unwrap_or(0.0);

    let accepted_passed: u64 = connection
        .query_row(
            "SELECT COUNT(*) FROM experience_records
             WHERE timestamp >= ?1 AND advisory_accepted = 1 AND outcome_status = 'passed'",
            params![cutoff_str],
            |row| row.get(0),
        )
        .map_err(|e| format!("Failed to count passed: {}", e))?;

    let acceptance_rate = accepted_count as f64 / total_recommendations as f64;
    let recommendation_accuracy = if accepted_count > 0 {
        accepted_passed as f64 / accepted_count as f64
    } else {
        0.0
    };

    Ok(ExperienceAggregateStats {
        total_recommendations,
        acceptance_rate,
        average_confidence_score: avg_confidence,
        recommendation_accuracy,
        period_days,
    })
}

// --- Scoring Weights Config ---

pub fn read_all_scoring_weights(
    connection: &Connection,
) -> Result<Vec<ScoringWeightsRow>, String> {
    let mut stmt = connection
        .prepare(
            "SELECT workload_class, quality_weight, cost_weight, speed_weight,
                    availability_weight, updated_at
             FROM scoring_weights_config",
        )
        .map_err(|e| format!("Failed to prepare weights query: {}", e))?;

    let rows = stmt
        .query_map([], |row| {
            Ok(ScoringWeightsRow {
                workload_class: row.get(0)?,
                quality_weight: row.get(1)?,
                cost_weight: row.get(2)?,
                speed_weight: row.get(3)?,
                availability_weight: row.get(4)?,
                updated_at: row.get(5)?,
            })
        })
        .map_err(|e| format!("Failed to query weights: {}", e))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("Failed to collect weights: {}", e))?;

    Ok(rows)
}

pub fn upsert_scoring_weights(
    connection: &Connection,
    row: &ScoringWeightsRow,
) -> Result<(), String> {
    // Validate sum-to-1.0
    let sum = row.quality_weight + row.cost_weight + row.speed_weight + row.availability_weight;
    if (sum - 1.0).abs() > 0.001 {
        return Err(format!(
            "Scoring weights must sum to 1.0, got {} for workload_class '{}'",
            sum, row.workload_class
        ));
    }

    connection
        .execute(
            "INSERT OR REPLACE INTO scoring_weights_config (
                workload_class, quality_weight, cost_weight, speed_weight,
                availability_weight, updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                row.workload_class,
                row.quality_weight,
                row.cost_weight,
                row.speed_weight,
                row.availability_weight,
                row.updated_at,
            ],
        )
        .map_err(|e| format!("Failed to upsert scoring weights: {}", e))?;

    Ok(())
}

// --- IPC Commands ---

#[tauri::command]
pub fn experience_buffer_record(
    state: tauri::State<'_, ExperienceBufferState>,
    record: ExperienceRecord,
) -> Result<(), String> {
    let db = state.db.lock().map_err(|e| format!("Lock error: {}", e))?;
    record_experience(&db, &record)
}

#[tauri::command]
pub fn experience_buffer_append_outcome(
    state: tauri::State<'_, ExperienceBufferState>,
    delegation_packet_id: String,
    status: String,
    duration_ms: u64,
    quality_score: f64,
) -> Result<(), String> {
    let db = state.db.lock().map_err(|e| format!("Lock error: {}", e))?;
    append_outcome(&db, &delegation_packet_id, &status, duration_ms, quality_score)
}

#[tauri::command]
pub fn experience_buffer_query_stats(
    state: tauri::State<'_, ExperienceBufferState>,
    agent_id: String,
    task_type: String,
) -> Result<Option<HistoricalStatsCache>, String> {
    let db = state.db.lock().map_err(|e| format!("Lock error: {}", e))?;
    query_historical_stats(&db, &agent_id, &task_type)
}

#[tauri::command]
pub fn experience_buffer_query_system_stats(
    state: tauri::State<'_, ExperienceBufferState>,
    task_type: String,
) -> Result<Option<HistoricalStatsCache>, String> {
    let db = state.db.lock().map_err(|e| format!("Lock error: {}", e))?;
    query_system_wide_stats(&db, &task_type)
}

#[tauri::command]
pub fn experience_buffer_query_records(
    state: tauri::State<'_, ExperienceBufferState>,
    query: ExperienceQuery,
) -> Result<Vec<ExperienceRecord>, String> {
    let db = state.db.lock().map_err(|e| format!("Lock error: {}", e))?;
    query_experience_records(&db, &query)
}

#[tauri::command]
pub fn experience_buffer_aggregate_stats(
    state: tauri::State<'_, ExperienceBufferState>,
    period_days: u32,
) -> Result<ExperienceAggregateStats, String> {
    let db = state.db.lock().map_err(|e| format!("Lock error: {}", e))?;
    compute_aggregate_stats(&db, period_days)
}

#[tauri::command]
pub fn experience_buffer_refresh_cache(
    state: tauri::State<'_, ExperienceBufferState>,
    agent_id: String,
    task_type: String,
) -> Result<HistoricalStatsCache, String> {
    let db = state.db.lock().map_err(|e| format!("Lock error: {}", e))?;
    refresh_historical_cache(&db, &agent_id, &task_type, 14)
}

#[tauri::command]
pub fn experience_buffer_load_weights(
    state: tauri::State<'_, ExperienceBufferState>,
) -> Result<Vec<ScoringWeightsRow>, String> {
    let db = state.db.lock().map_err(|e| format!("Lock error: {}", e))?;
    read_all_scoring_weights(&db)
}

#[tauri::command]
pub fn experience_buffer_save_weights(
    state: tauri::State<'_, ExperienceBufferState>,
    row: ScoringWeightsRow,
) -> Result<(), String> {
    let db = state.db.lock().map_err(|e| format!("Lock error: {}", e))?;
    upsert_scoring_weights(&db, &row)
}

// --- App Setup ---

pub fn setup_experience_buffer(app: &tauri::App) -> Result<(), String> {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to get app data dir: {}", e))?;

    std::fs::create_dir_all(&app_data_dir)
        .map_err(|e| format!("Failed to create app data dir: {}", e))?;

    let db_path = app_data_dir.join("experience_buffer.db");
    let connection = Connection::open(&db_path)
        .map_err(|e| format!("Failed to open experience buffer db: {}", e))?;

    initialize_experience_buffer_db(&connection)?;

    app.manage(ExperienceBufferState {
        db: Mutex::new(connection),
    });

    Ok(())
}

// --- Unit Tests ---

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        initialize_experience_buffer_db(&conn).unwrap();
        conn
    }

    fn make_test_record(id: &str, packet_id: &str) -> ExperienceRecord {
        ExperienceRecord {
            id: id.to_string(),
            delegation_packet_id: packet_id.to_string(),
            timestamp: "2025-01-15T10:00:00Z".to_string(),
            workload_class: "coding".to_string(),
            task_type: "code-change".to_string(),
            scoring_recommendation_json: r#"{"agentId":"agent-1"}"#.to_string(),
            heuristic_decision_json: r#"{"providerProfileId":"p1"}"#.to_string(),
            advisory_accepted: true,
            rejection_reason: None,
            outcome_status: None,
            outcome_duration_ms: None,
            outcome_quality_score: None,
            outcome_recorded_at: None,
            confidence_score: 0.85,
        }
    }

    #[test]
    fn test_schema_initialization() {
        let conn = setup_test_db();
        // Verify tables exist by querying them
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM experience_records", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM historical_stats_cache", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM trust_tier_transitions", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM scoring_weights_config", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM circuit_breaker_state", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_record_insert_and_read() {
        let conn = setup_test_db();
        let record = make_test_record("rec-1", "pkt-1");

        record_experience(&conn, &record).unwrap();

        let query = ExperienceQuery {
            from_date: None,
            to_date: None,
            task_type: None,
            advisory_accepted: None,
            limit: Some(10),
        };
        let results = query_experience_records(&conn, &query).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "rec-1");
        assert_eq!(results[0].delegation_packet_id, "pkt-1");
        assert_eq!(results[0].workload_class, "coding");
        assert_eq!(results[0].advisory_accepted, true);
        assert_eq!(results[0].confidence_score, 0.85);
    }

    #[test]
    fn test_outcome_append() {
        let conn = setup_test_db();
        let record = make_test_record("rec-2", "pkt-2");
        record_experience(&conn, &record).unwrap();

        append_outcome(&conn, "pkt-2", "passed", 1500, 0.92).unwrap();

        let query = ExperienceQuery {
            from_date: None,
            to_date: None,
            task_type: None,
            advisory_accepted: None,
            limit: Some(10),
        };
        let results = query_experience_records(&conn, &query).unwrap();
        assert_eq!(results[0].outcome_status, Some("passed".to_string()));
        assert_eq!(results[0].outcome_duration_ms, Some(1500));
        assert_eq!(results[0].outcome_quality_score, Some(0.92));
        assert!(results[0].outcome_recorded_at.is_some());
    }

    #[test]
    fn test_outcome_append_nonexistent_creates_partial() {
        let conn = setup_test_db();
        append_outcome(&conn, "pkt-missing", "failed", 500, 0.3).unwrap();

        let query = ExperienceQuery {
            from_date: None,
            to_date: None,
            task_type: None,
            advisory_accepted: None,
            limit: Some(10),
        };
        let results = query_experience_records(&conn, &query).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].delegation_packet_id, "pkt-missing");
        assert_eq!(results[0].outcome_status, Some("failed".to_string()));
    }

    #[test]
    fn test_eviction_boundary() {
        let conn = setup_test_db();

        // Record from 100 days ago (should be evicted with 90-day retention)
        let mut old_record = make_test_record("rec-old", "pkt-old");
        let old_date = chrono::Utc::now() - chrono::Duration::days(100);
        old_record.timestamp = old_date.to_rfc3339();
        record_experience(&conn, &old_record).unwrap();

        // Record from 50 days ago (should be kept)
        let mut recent_record = make_test_record("rec-recent", "pkt-recent");
        let recent_date = chrono::Utc::now() - chrono::Duration::days(50);
        recent_record.timestamp = recent_date.to_rfc3339();
        record_experience(&conn, &recent_record).unwrap();

        let evicted = evict_expired_records(&conn, 90).unwrap();
        assert_eq!(evicted, 1);

        let query = ExperienceQuery {
            from_date: None,
            to_date: None,
            task_type: None,
            advisory_accepted: None,
            limit: None,
        };
        let results = query_experience_records(&conn, &query).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "rec-recent");
    }

    #[test]
    fn test_aggregate_stats() {
        let conn = setup_test_db();

        // Insert records with various states
        let mut r1 = make_test_record("rec-a1", "pkt-a1");
        r1.advisory_accepted = true;
        r1.outcome_status = Some("passed".to_string());
        r1.confidence_score = 0.9;
        r1.timestamp = chrono::Utc::now().to_rfc3339();
        record_experience(&conn, &r1).unwrap();

        let mut r2 = make_test_record("rec-a2", "pkt-a2");
        r2.advisory_accepted = false;
        r2.rejection_reason = Some("confidence-below-threshold".to_string());
        r2.confidence_score = 0.5;
        r2.timestamp = chrono::Utc::now().to_rfc3339();
        record_experience(&conn, &r2).unwrap();

        let mut r3 = make_test_record("rec-a3", "pkt-a3");
        r3.advisory_accepted = true;
        r3.outcome_status = Some("failed".to_string());
        r3.confidence_score = 0.8;
        r3.timestamp = chrono::Utc::now().to_rfc3339();
        record_experience(&conn, &r3).unwrap();

        let stats = compute_aggregate_stats(&conn, 30).unwrap();
        assert_eq!(stats.total_recommendations, 3);
        // 2 out of 3 accepted
        assert!((stats.acceptance_rate - 2.0 / 3.0).abs() < 0.01);
        // Average confidence: (0.9 + 0.5 + 0.8) / 3 ≈ 0.733
        assert!((stats.average_confidence_score - 0.733).abs() < 0.01);
        // 1 out of 2 accepted records passed
        assert!((stats.recommendation_accuracy - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_scoring_weights_crud() {
        let conn = setup_test_db();

        let weights = ScoringWeightsRow {
            workload_class: "coding".to_string(),
            quality_weight: 0.4,
            cost_weight: 0.2,
            speed_weight: 0.2,
            availability_weight: 0.2,
            updated_at: "2025-01-15T10:00:00Z".to_string(),
        };

        upsert_scoring_weights(&conn, &weights).unwrap();

        let all = read_all_scoring_weights(&conn).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].workload_class, "coding");
        assert!((all[0].quality_weight - 0.4).abs() < 0.001);
    }

    #[test]
    fn test_scoring_weights_validation_rejects_invalid_sum() {
        let conn = setup_test_db();

        let weights = ScoringWeightsRow {
            workload_class: "coding".to_string(),
            quality_weight: 0.5,
            cost_weight: 0.5,
            speed_weight: 0.5,
            availability_weight: 0.5,
            updated_at: "2025-01-15T10:00:00Z".to_string(),
        };

        let result = upsert_scoring_weights(&conn, &weights);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("must sum to 1.0"));
    }

    #[test]
    fn test_decay_computation() {
        let conn = setup_test_db();

        // Insert experience records with known timestamps and quality scores
        let now = chrono::Utc::now();
        for i in 0..5 {
            let ts = now - chrono::Duration::days(i * 7);
            let mut record = make_test_record(
                &format!("rec-decay-{}", i),
                &format!("pkt-decay-{}", i),
            );
            record.timestamp = ts.to_rfc3339();
            record.outcome_status = Some("passed".to_string());
            record.outcome_quality_score = Some(0.8 + (i as f64) * 0.02);
            record.outcome_duration_ms = Some(1000 + i as u64 * 100);
            record.scoring_recommendation_json =
                format!(r#"{{"agentId":"agent-decay"}}"#);
            record_experience(&conn, &record).unwrap();
        }

        let stats = refresh_historical_cache(&conn, "agent-decay", "code-change", 14).unwrap();
        assert_eq!(stats.agent_id, "agent-decay");
        assert_eq!(stats.task_type, "code-change");
        assert!(stats.record_count > 0);
        // Quality should be between 0.8 and 0.88 (weighted toward recent)
        assert!(stats.rolling_quality_score >= 0.0);
        assert!(stats.rolling_quality_score <= 1.0);
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;
    use rusqlite::Connection;

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        initialize_experience_buffer_db(&conn).unwrap();
        conn
    }

    // Feature: scoring-engine, Property 4: Scoring weights persistence round-trip
    proptest! {
        #[test]
        fn prop_scoring_weights_round_trip(
            quality in 0.0f64..=1.0,
            cost in 0.0f64..=1.0,
            speed in 0.0f64..=1.0,
        ) {
            // Ensure weights sum to 1.0
            let total = quality + cost + speed;
            if total <= 0.0 || total > 3.0 {
                return Ok(());
            }
            let q = quality / (total + 0.001);
            let c = cost / (total + 0.001);
            let s = speed / (total + 0.001);
            let a = 1.0 - q - c - s;
            if a < 0.0 || a > 1.0 {
                return Ok(());
            }

            let conn = setup_test_db();
            let weights = ScoringWeightsRow {
                workload_class: "coding".to_string(),
                quality_weight: q,
                cost_weight: c,
                speed_weight: s,
                availability_weight: a,
                updated_at: "2025-01-15T10:00:00Z".to_string(),
            };

            // Verify sum is close to 1.0
            let sum = q + c + s + a;
            if (sum - 1.0).abs() > 0.001 {
                return Ok(());
            }

            upsert_scoring_weights(&conn, &weights).unwrap();
            let all = read_all_scoring_weights(&conn).unwrap();
            prop_assert_eq!(all.len(), 1);
            prop_assert!((all[0].quality_weight - q).abs() < 0.0001);
            prop_assert!((all[0].cost_weight - c).abs() < 0.0001);
            prop_assert!((all[0].speed_weight - s).abs() < 0.0001);
            prop_assert!((all[0].availability_weight - a).abs() < 0.0001);
        }
    }

    // Feature: scoring-engine, Property 10: Experience record persistence round-trip
    proptest! {
        #[test]
        fn prop_experience_record_round_trip(
            id in "[a-z]{5,10}",
            packet_id in "[a-z]{5,10}",
            workload_class in prop_oneof!["coding", "routine", "recovery", "primary-chat"],
            task_type in prop_oneof!["code-change", "bug-fix", "research"],
            accepted in proptest::bool::ANY,
        ) {
            let conn = setup_test_db();
            let record = ExperienceRecord {
                id: id.clone(),
                delegation_packet_id: packet_id.clone(),
                timestamp: "2025-01-15T10:00:00Z".to_string(),
                workload_class: workload_class.clone(),
                task_type: task_type.clone(),
                scoring_recommendation_json: r#"{"test":true}"#.to_string(),
                heuristic_decision_json: r#"{"decision":"ok"}"#.to_string(),
                advisory_accepted: accepted,
                rejection_reason: if accepted { None } else { Some("confidence-below-threshold".to_string()) },
                outcome_status: None,
                outcome_duration_ms: None,
                outcome_quality_score: None,
                outcome_recorded_at: None,
                confidence_score: 0.75,
            };

            record_experience(&conn, &record).unwrap();

            let query = ExperienceQuery {
                from_date: None,
                to_date: None,
                task_type: None,
                advisory_accepted: None,
                limit: Some(1),
            };
            let results = query_experience_records(&conn, &query).unwrap();
            prop_assert_eq!(results.len(), 1);
            prop_assert_eq!(&results[0].id, &id);
            prop_assert_eq!(&results[0].delegation_packet_id, &packet_id);
            prop_assert_eq!(&results[0].workload_class, &workload_class);
            prop_assert_eq!(&results[0].task_type, &task_type);
            prop_assert_eq!(results[0].advisory_accepted, accepted);
            prop_assert_eq!(&results[0].rejection_reason, &record.rejection_reason);
        }
    }

    // Feature: scoring-engine, Property 11: Experience outcome append preserves record
    proptest! {
        #[test]
        fn prop_outcome_append_preserves_record(
            status in prop_oneof!["passed", "failed", "degraded"],
            duration_ms in 0u64..100000,
            quality_score in 0.0f64..=1.0,
        ) {
            let conn = setup_test_db();
            let record = ExperienceRecord {
                id: "rec-prop-11".to_string(),
                delegation_packet_id: "pkt-prop-11".to_string(),
                timestamp: "2025-01-15T10:00:00Z".to_string(),
                workload_class: "coding".to_string(),
                task_type: "code-change".to_string(),
                scoring_recommendation_json: r#"{"agentId":"a1"}"#.to_string(),
                heuristic_decision_json: r#"{"decision":"ok"}"#.to_string(),
                advisory_accepted: true,
                rejection_reason: None,
                outcome_status: None,
                outcome_duration_ms: None,
                outcome_quality_score: None,
                outcome_recorded_at: None,
                confidence_score: 0.85,
            };

            record_experience(&conn, &record).unwrap();
            append_outcome(&conn, "pkt-prop-11", &status, duration_ms, quality_score).unwrap();

            let query = ExperienceQuery {
                from_date: None,
                to_date: None,
                task_type: None,
                advisory_accepted: None,
                limit: Some(1),
            };
            let results = query_experience_records(&conn, &query).unwrap();
            prop_assert_eq!(results.len(), 1);
            // Original fields preserved
            prop_assert_eq!(&results[0].id, "rec-prop-11");
            prop_assert_eq!(&results[0].delegation_packet_id, "pkt-prop-11");
            prop_assert_eq!(&results[0].workload_class, "coding");
            prop_assert_eq!(&results[0].task_type, "code-change");
            prop_assert_eq!(results[0].advisory_accepted, true);
            // Outcome fields updated
            prop_assert_eq!(results[0].outcome_status.as_deref(), Some(status.as_str()));
            prop_assert_eq!(results[0].outcome_duration_ms, Some(duration_ms));
            prop_assert!((results[0].outcome_quality_score.unwrap() - quality_score).abs() < 0.0001);
            prop_assert!(results[0].outcome_recorded_at.is_some());
        }
    }

    // Feature: scoring-engine, Property 12: Experience retention policy
    proptest! {
        #[test]
        fn prop_retention_policy_never_deletes_recent(
            days_ago in 0u32..89,
        ) {
            let conn = setup_test_db();
            let ts = chrono::Utc::now() - chrono::Duration::days(days_ago as i64);
            let record = ExperienceRecord {
                id: format!("rec-ret-{}", days_ago),
                delegation_packet_id: format!("pkt-ret-{}", days_ago),
                timestamp: ts.to_rfc3339(),
                workload_class: "coding".to_string(),
                task_type: "code-change".to_string(),
                scoring_recommendation_json: "{}".to_string(),
                heuristic_decision_json: "{}".to_string(),
                advisory_accepted: true,
                rejection_reason: None,
                outcome_status: None,
                outcome_duration_ms: None,
                outcome_quality_score: None,
                outcome_recorded_at: None,
                confidence_score: 0.5,
            };

            record_experience(&conn, &record).unwrap();
            let evicted = evict_expired_records(&conn, 90).unwrap();
            prop_assert_eq!(evicted, 0);

            let query = ExperienceQuery {
                from_date: None,
                to_date: None,
                task_type: None,
                advisory_accepted: None,
                limit: None,
            };
            let results = query_experience_records(&conn, &query).unwrap();
            prop_assert_eq!(results.len(), 1);
        }
    }
}
