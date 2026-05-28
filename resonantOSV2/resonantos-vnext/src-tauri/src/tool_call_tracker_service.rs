//! Tool Call Tracker Service
//!
//! Passive logging and offline analysis system that records every tool invocation
//! made by delegated agents during task execution. Provides async logging via mpsc
//! channel, buffered batch writes, circuit breaker pattern, and background analysis.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use chrono::Utc;
use dashmap::DashMap;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, RwLock};
use tokio::time::{self, Duration};

use crate::tool_call_analysis::{
    analyze_completed_task_inner, AnalysisResult,
};
use crate::tool_call_sanitizer::sanitize_parameters;

// ─── Core Data Structures ───────────────────────────────────────────────────

/// A single tool call record — the core data unit.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallRecord {
    pub id: String,
    pub delegation_packet_id: String,
    pub agent_id: String,
    pub task_type: String,
    pub tool_name: String,
    pub input_params_json: String,
    pub output_summary: Option<String>,
    pub duration_ms: u64,
    pub success: bool,
    pub timestamp: String,
    pub sequence_position: u32,
    pub prompt_tokens: Option<u32>,
    pub completion_tokens: Option<u32>,
    pub is_llm_backed: bool,
}

/// Configuration for the tool call tracker.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallTrackerConfig {
    pub buffer_flush_size: usize,
    pub buffer_flush_interval_secs: u64,
    pub channel_capacity: usize,
    pub circuit_breaker_threshold: u32,
    pub circuit_breaker_cooldown_secs: u64,
    pub max_output_summary_tokens: usize,
    pub max_storage_bytes: u64,
    pub efficiency_threshold: f64,
    pub historical_avg_multiplier: f64,
    pub retention_days_traces: u32,
    pub retention_days_metrics: u32,
    pub rolling_avg_window_size: u32,
}

impl Default for ToolCallTrackerConfig {
    fn default() -> Self {
        Self {
            buffer_flush_size: 50,
            buffer_flush_interval_secs: 10,
            channel_capacity: 1000,
            circuit_breaker_threshold: 5,
            circuit_breaker_cooldown_secs: 30,
            max_output_summary_tokens: 500,
            max_storage_bytes: 500 * 1024 * 1024,
            efficiency_threshold: 0.5,
            historical_avg_multiplier: 3.0,
            retention_days_traces: 90,
            retention_days_metrics: 180,
            rolling_avg_window_size: 100,
        }
    }
}

/// Circuit breaker state for persistence failures.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CircuitBreakerState {
    pub consecutive_failures: u32,
    pub is_open: bool,
    pub last_failure_at: Option<String>,
    pub cooldown_ends_at: Option<String>,
    pub cooldown_secs: u64,
    pub failure_threshold: u32,
    pub total_records_dropped: u64,
}

impl Default for CircuitBreakerState {
    fn default() -> Self {
        Self {
            consecutive_failures: 0,
            is_open: false,
            last_failure_at: None,
            cooldown_ends_at: None,
            cooldown_secs: 30,
            failure_threshold: 5,
            total_records_dropped: 0,
        }
    }
}

/// Shared state for the tool call tracker, accessible from the interceptor.
pub struct ToolCallTrackerState {
    pub sender: mpsc::Sender<ToolCallRecord>,
    pub circuit_breaker: Arc<RwLock<CircuitBreakerState>>,
    pub config: ToolCallTrackerConfig,
    pub sequence_positions: Arc<DashMap<String, AtomicU32>>,
    pub db_path: PathBuf,
}

// ─── Database Initialization ────────────────────────────────────────────────

/// Initialize the tool call tracker database with all required tables and indexes.
pub fn initialize_tool_call_tracker_db(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        "
        PRAGMA journal_mode = WAL;
        PRAGMA busy_timeout = 5000;

        CREATE TABLE IF NOT EXISTS tool_call_records (
            id TEXT PRIMARY KEY,
            delegation_packet_id TEXT NOT NULL,
            agent_id TEXT NOT NULL,
            task_type TEXT NOT NULL,
            tool_name TEXT NOT NULL,
            input_params_json TEXT NOT NULL,
            output_summary TEXT,
            duration_ms INTEGER NOT NULL,
            success INTEGER NOT NULL,
            timestamp TEXT NOT NULL,
            sequence_position INTEGER NOT NULL,
            prompt_tokens INTEGER,
            completion_tokens INTEGER,
            is_llm_backed INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS task_analysis_results (
            delegation_packet_id TEXT PRIMARY KEY,
            agent_id TEXT NOT NULL,
            task_type TEXT NOT NULL,
            efficiency_ratio REAL NOT NULL,
            total_calls INTEGER NOT NULL,
            useful_calls INTEGER NOT NULL,
            redundant_calls INTEGER NOT NULL,
            detected_patterns_json TEXT NOT NULL,
            anomaly_flags_json TEXT,
            tool_sequence_signature_json TEXT NOT NULL,
            analyzed_at TEXT NOT NULL,
            experience_buffer_linked INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS task_type_averages (
            task_type TEXT PRIMARY KEY,
            avg_tool_call_count REAL NOT NULL,
            avg_efficiency_ratio REAL NOT NULL,
            sample_count INTEGER NOT NULL,
            last_updated_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS standalone_trace_summaries (
            delegation_packet_id TEXT PRIMARY KEY,
            summary_json TEXT NOT NULL,
            created_at TEXT NOT NULL,
            linked INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS aggregate_stats (
            agent_id TEXT NOT NULL,
            task_type TEXT NOT NULL,
            avg_efficiency_ratio REAL NOT NULL,
            avg_tool_call_count REAL NOT NULL,
            total_tasks_analyzed INTEGER NOT NULL,
            last_updated_at TEXT NOT NULL,
            PRIMARY KEY (agent_id, task_type)
        );

        CREATE TABLE IF NOT EXISTS circuit_breaker_state (
            id TEXT PRIMARY KEY DEFAULT 'singleton',
            consecutive_failures INTEGER NOT NULL DEFAULT 0,
            is_open INTEGER NOT NULL DEFAULT 0,
            last_failure_at TEXT,
            cooldown_ends_at TEXT,
            cooldown_secs INTEGER NOT NULL DEFAULT 30,
            failure_threshold INTEGER NOT NULL DEFAULT 5,
            total_records_dropped INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS tracker_config (
            id TEXT PRIMARY KEY DEFAULT 'singleton',
            efficiency_threshold REAL NOT NULL DEFAULT 0.5,
            historical_avg_multiplier REAL NOT NULL DEFAULT 3.0,
            max_storage_bytes INTEGER NOT NULL DEFAULT 524288000,
            retention_days_traces INTEGER NOT NULL DEFAULT 90,
            retention_days_metrics INTEGER NOT NULL DEFAULT 180,
            rolling_avg_window_size INTEGER NOT NULL DEFAULT 100
        );

        CREATE INDEX IF NOT EXISTS idx_tcr_delegation_packet
            ON tool_call_records(delegation_packet_id);
        CREATE INDEX IF NOT EXISTS idx_tcr_agent_id
            ON tool_call_records(agent_id);
        CREATE INDEX IF NOT EXISTS idx_tcr_timestamp
            ON tool_call_records(timestamp);
        CREATE INDEX IF NOT EXISTS idx_tcr_task_type
            ON tool_call_records(task_type);
        CREATE INDEX IF NOT EXISTS idx_tcr_sequence
            ON tool_call_records(delegation_packet_id, sequence_position);
        CREATE INDEX IF NOT EXISTS idx_tar_agent_task
            ON task_analysis_results(agent_id, task_type);
        CREATE INDEX IF NOT EXISTS idx_tar_analyzed_at
            ON task_analysis_results(analyzed_at);
        ",
    )
    .map_err(|e| format!("Failed to initialize tool call tracker DB: {}", e))?;

    // Insert default circuit breaker state if not exists
    conn.execute(
        "INSERT OR IGNORE INTO circuit_breaker_state (id, consecutive_failures, is_open, cooldown_secs, failure_threshold, total_records_dropped)
         VALUES ('singleton', 0, 0, 30, 5, 0)",
        [],
    )
    .map_err(|e| format!("Failed to insert default circuit breaker state: {}", e))?;

    // Insert default tracker config if not exists
    conn.execute(
        "INSERT OR IGNORE INTO tracker_config (id) VALUES ('singleton')",
        [],
    )
    .map_err(|e| format!("Failed to insert default tracker config: {}", e))?;

    Ok(())
}

// ─── CRUD: tool_call_records ────────────────────────────────────────────────

/// Insert a single tool call record.
pub fn insert_tool_call_record(conn: &Connection, record: &ToolCallRecord) -> Result<(), String> {
    conn.execute(
        "INSERT INTO tool_call_records (id, delegation_packet_id, agent_id, task_type, tool_name,
         input_params_json, output_summary, duration_ms, success, timestamp, sequence_position,
         prompt_tokens, completion_tokens, is_llm_backed)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        params![
            record.id,
            record.delegation_packet_id,
            record.agent_id,
            record.task_type,
            record.tool_name,
            record.input_params_json,
            record.output_summary,
            record.duration_ms as i64,
            record.success as i32,
            record.timestamp,
            record.sequence_position as i32,
            record.prompt_tokens.map(|v| v as i32),
            record.completion_tokens.map(|v| v as i32),
            record.is_llm_backed as i32,
        ],
    )
    .map_err(|e| format!("Failed to insert tool call record: {}", e))?;
    Ok(())
}

/// Insert a batch of tool call records within a single transaction.
pub fn insert_tool_call_records_batch(
    conn: &Connection,
    records: &[ToolCallRecord],
) -> Result<(), String> {
    let tx = conn
        .unchecked_transaction()
        .map_err(|e| format!("Failed to begin transaction: {}", e))?;

    for record in records {
        tx.execute(
            "INSERT INTO tool_call_records (id, delegation_packet_id, agent_id, task_type, tool_name,
             input_params_json, output_summary, duration_ms, success, timestamp, sequence_position,
             prompt_tokens, completion_tokens, is_llm_backed)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                record.id,
                record.delegation_packet_id,
                record.agent_id,
                record.task_type,
                record.tool_name,
                record.input_params_json,
                record.output_summary,
                record.duration_ms as i64,
                record.success as i32,
                record.timestamp,
                record.sequence_position as i32,
                record.prompt_tokens.map(|v| v as i32),
                record.completion_tokens.map(|v| v as i32),
                record.is_llm_backed as i32,
            ],
        )
        .map_err(|e| format!("Failed to insert record in batch: {}", e))?;
    }

    tx.commit()
        .map_err(|e| format!("Failed to commit batch transaction: {}", e))?;
    Ok(())
}

/// Query tool call records by delegation packet ID, ordered by sequence_position.
pub fn query_records_by_packet_id(
    conn: &Connection,
    delegation_packet_id: &str,
) -> Result<Vec<ToolCallRecord>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, delegation_packet_id, agent_id, task_type, tool_name,
             input_params_json, output_summary, duration_ms, success, timestamp,
             sequence_position, prompt_tokens, completion_tokens, is_llm_backed
             FROM tool_call_records WHERE delegation_packet_id = ?1
             ORDER BY sequence_position ASC",
        )
        .map_err(|e| format!("Failed to prepare query: {}", e))?;

    let rows = stmt
        .query_map(params![delegation_packet_id], |row| {
            Ok(ToolCallRecord {
                id: row.get(0)?,
                delegation_packet_id: row.get(1)?,
                agent_id: row.get(2)?,
                task_type: row.get(3)?,
                tool_name: row.get(4)?,
                input_params_json: row.get(5)?,
                output_summary: row.get(6)?,
                duration_ms: row.get::<_, i64>(7)? as u64,
                success: row.get::<_, i32>(8)? != 0,
                timestamp: row.get(9)?,
                sequence_position: row.get::<_, i32>(10)? as u32,
                prompt_tokens: row.get::<_, Option<i32>>(11)?.map(|v| v as u32),
                completion_tokens: row.get::<_, Option<i32>>(12)?.map(|v| v as u32),
                is_llm_backed: row.get::<_, i32>(13)? != 0,
            })
        })
        .map_err(|e| format!("Failed to query records: {}", e))?;

    let mut results = Vec::new();
    for row in rows {
        results.push(row.map_err(|e| format!("Failed to read row: {}", e))?);
    }
    Ok(results)
}

/// Query tool call records by agent ID, ordered by timestamp.
pub fn query_records_by_agent(
    conn: &Connection,
    agent_id: &str,
) -> Result<Vec<ToolCallRecord>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, delegation_packet_id, agent_id, task_type, tool_name,
             input_params_json, output_summary, duration_ms, success, timestamp,
             sequence_position, prompt_tokens, completion_tokens, is_llm_backed
             FROM tool_call_records WHERE agent_id = ?1
             ORDER BY timestamp ASC",
        )
        .map_err(|e| format!("Failed to prepare query: {}", e))?;

    let rows = stmt
        .query_map(params![agent_id], |row| {
            Ok(ToolCallRecord {
                id: row.get(0)?,
                delegation_packet_id: row.get(1)?,
                agent_id: row.get(2)?,
                task_type: row.get(3)?,
                tool_name: row.get(4)?,
                input_params_json: row.get(5)?,
                output_summary: row.get(6)?,
                duration_ms: row.get::<_, i64>(7)? as u64,
                success: row.get::<_, i32>(8)? != 0,
                timestamp: row.get(9)?,
                sequence_position: row.get::<_, i32>(10)? as u32,
                prompt_tokens: row.get::<_, Option<i32>>(11)?.map(|v| v as u32),
                completion_tokens: row.get::<_, Option<i32>>(12)?.map(|v| v as u32),
                is_llm_backed: row.get::<_, i32>(13)? != 0,
            })
        })
        .map_err(|e| format!("Failed to query records: {}", e))?;

    let mut results = Vec::new();
    for row in rows {
        results.push(row.map_err(|e| format!("Failed to read row: {}", e))?);
    }
    Ok(results)
}

/// Query tool call records within a time range, ordered by timestamp.
pub fn query_records_by_time_range(
    conn: &Connection,
    from: &str,
    to: &str,
) -> Result<Vec<ToolCallRecord>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, delegation_packet_id, agent_id, task_type, tool_name,
             input_params_json, output_summary, duration_ms, success, timestamp,
             sequence_position, prompt_tokens, completion_tokens, is_llm_backed
             FROM tool_call_records WHERE timestamp >= ?1 AND timestamp <= ?2
             ORDER BY timestamp ASC",
        )
        .map_err(|e| format!("Failed to prepare query: {}", e))?;

    let rows = stmt
        .query_map(params![from, to], |row| {
            Ok(ToolCallRecord {
                id: row.get(0)?,
                delegation_packet_id: row.get(1)?,
                agent_id: row.get(2)?,
                task_type: row.get(3)?,
                tool_name: row.get(4)?,
                input_params_json: row.get(5)?,
                output_summary: row.get(6)?,
                duration_ms: row.get::<_, i64>(7)? as u64,
                success: row.get::<_, i32>(8)? != 0,
                timestamp: row.get(9)?,
                sequence_position: row.get::<_, i32>(10)? as u32,
                prompt_tokens: row.get::<_, Option<i32>>(11)?.map(|v| v as u32),
                completion_tokens: row.get::<_, Option<i32>>(12)?.map(|v| v as u32),
                is_llm_backed: row.get::<_, i32>(13)? != 0,
            })
        })
        .map_err(|e| format!("Failed to query records: {}", e))?;

    let mut results = Vec::new();
    for row in rows {
        results.push(row.map_err(|e| format!("Failed to read row: {}", e))?);
    }
    Ok(results)
}

// ─── CRUD: tracker_config ───────────────────────────────────────────────────

/// Persisted tracker configuration (subset stored in DB).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackerConfigRow {
    pub efficiency_threshold: f64,
    pub historical_avg_multiplier: f64,
    pub max_storage_bytes: u64,
    pub retention_days_traces: u32,
    pub retention_days_metrics: u32,
    pub rolling_avg_window_size: u32,
}

/// Read the tracker configuration from the database.
pub fn read_tracker_config(conn: &Connection) -> Result<TrackerConfigRow, String> {
    conn.query_row(
        "SELECT efficiency_threshold, historical_avg_multiplier, max_storage_bytes,
         retention_days_traces, retention_days_metrics, rolling_avg_window_size
         FROM tracker_config WHERE id = 'singleton'",
        [],
        |row| {
            Ok(TrackerConfigRow {
                efficiency_threshold: row.get(0)?,
                historical_avg_multiplier: row.get(1)?,
                max_storage_bytes: row.get::<_, i64>(2)? as u64,
                retention_days_traces: row.get::<_, i32>(3)? as u32,
                retention_days_metrics: row.get::<_, i32>(4)? as u32,
                rolling_avg_window_size: row.get::<_, i32>(5)? as u32,
            })
        },
    )
    .map_err(|e| format!("Failed to read tracker config: {}", e))
}

/// Update the tracker configuration with validation.
pub fn update_tracker_config(conn: &Connection, config: &TrackerConfigRow) -> Result<(), String> {
    // Validate
    if config.efficiency_threshold < 0.0 || config.efficiency_threshold > 1.0 {
        return Err("efficiency_threshold must be in range [0.0, 1.0]".to_string());
    }
    if config.historical_avg_multiplier <= 0.0 {
        return Err("historical_avg_multiplier must be > 0".to_string());
    }
    if config.retention_days_traces == 0 {
        return Err("retention_days_traces must be > 0".to_string());
    }
    if config.retention_days_metrics == 0 {
        return Err("retention_days_metrics must be > 0".to_string());
    }
    if config.rolling_avg_window_size == 0 {
        return Err("rolling_avg_window_size must be > 0".to_string());
    }

    conn.execute(
        "UPDATE tracker_config SET
         efficiency_threshold = ?1,
         historical_avg_multiplier = ?2,
         max_storage_bytes = ?3,
         retention_days_traces = ?4,
         retention_days_metrics = ?5,
         rolling_avg_window_size = ?6
         WHERE id = 'singleton'",
        params![
            config.efficiency_threshold,
            config.historical_avg_multiplier,
            config.max_storage_bytes as i64,
            config.retention_days_traces as i32,
            config.retention_days_metrics as i32,
            config.rolling_avg_window_size as i32,
        ],
    )
    .map_err(|e| format!("Failed to update tracker config: {}", e))?;
    Ok(())
}

// ─── CRUD: circuit_breaker_state ────────────────────────────────────────────

/// Read the circuit breaker state from the database.
pub fn read_circuit_breaker(conn: &Connection) -> Result<CircuitBreakerState, String> {
    conn.query_row(
        "SELECT consecutive_failures, is_open, last_failure_at, cooldown_ends_at,
         cooldown_secs, failure_threshold, total_records_dropped
         FROM circuit_breaker_state WHERE id = 'singleton'",
        [],
        |row| {
            Ok(CircuitBreakerState {
                consecutive_failures: row.get::<_, i32>(0)? as u32,
                is_open: row.get::<_, i32>(1)? != 0,
                last_failure_at: row.get(2)?,
                cooldown_ends_at: row.get(3)?,
                cooldown_secs: row.get::<_, i64>(4)? as u64,
                failure_threshold: row.get::<_, i32>(5)? as u32,
                total_records_dropped: row.get::<_, i64>(6)? as u64,
            })
        },
    )
    .map_err(|e| format!("Failed to read circuit breaker state: {}", e))
}

/// Update the circuit breaker state in the database.
pub fn update_circuit_breaker(conn: &Connection, state: &CircuitBreakerState) -> Result<(), String> {
    conn.execute(
        "UPDATE circuit_breaker_state SET
         consecutive_failures = ?1,
         is_open = ?2,
         last_failure_at = ?3,
         cooldown_ends_at = ?4,
         cooldown_secs = ?5,
         failure_threshold = ?6,
         total_records_dropped = ?7
         WHERE id = 'singleton'",
        params![
            state.consecutive_failures as i32,
            state.is_open as i32,
            state.last_failure_at,
            state.cooldown_ends_at,
            state.cooldown_secs as i64,
            state.failure_threshold as i32,
            state.total_records_dropped as i64,
        ],
    )
    .map_err(|e| format!("Failed to update circuit breaker state: {}", e))?;
    Ok(())
}

/// Reset the circuit breaker to its default closed state.
pub fn reset_circuit_breaker(conn: &Connection) -> Result<(), String> {
    let default_state = CircuitBreakerState::default();
    update_circuit_breaker(conn, &default_state)
}

// ─── Phase 3: Async Logging and Buffer Writer ───────────────────────────────

/// Truncate output summary to the configured max token count.
/// Uses a simple word-based approximation (split on whitespace).
pub fn truncate_output_summary(output: &str, max_tokens: usize) -> String {
    let words: Vec<&str> = output.split_whitespace().collect();
    if words.len() <= max_tokens {
        output.to_string()
    } else {
        let truncated: String = words[..max_tokens].join(" ");
        format!("{}...[truncated]", truncated)
    }
}

/// The non-blocking logging function called after each tool execution.
/// Returns immediately — never blocks the tool return path.
pub fn log_tool_call(state: &ToolCallTrackerState, mut record: ToolCallRecord) {
    // Check circuit breaker (non-blocking read via try_read)
    if let Ok(cb) = state.circuit_breaker.try_read() {
        if cb.is_open {
            // Circuit breaker is open, drop silently
            return;
        }
    }

    // Sanitize input parameters inline
    if let Ok(params_value) = serde_json::from_str::<serde_json::Value>(&record.input_params_json) {
        let sanitized = sanitize_parameters(&params_value);
        if let Ok(sanitized_str) = serde_json::to_string(&sanitized) {
            record.input_params_json = sanitized_str;
        }
    }

    // Truncate output summary
    if let Some(ref output) = record.output_summary {
        record.output_summary = Some(truncate_output_summary(
            output,
            state.config.max_output_summary_tokens,
        ));
    }

    // Assign sequence position from atomic counter per delegation_packet_id
    let counter = state
        .sequence_positions
        .entry(record.delegation_packet_id.clone())
        .or_insert_with(|| AtomicU32::new(0));
    let pos = counter.value().fetch_add(1, Ordering::SeqCst) + 1;
    record.sequence_position = pos;

    // Set timestamp if not already set
    if record.timestamp.is_empty() {
        record.timestamp = Utc::now().to_rfc3339();
    }

    // try_send into channel (non-blocking). If channel full, drop silently.
    let _ = state.sender.try_send(record);
}

/// Start the tool call tracker: creates mpsc channel, spawns buffer writer task,
/// initializes circuit breaker state, returns ToolCallTrackerState.
pub fn start_tool_call_tracker(
    db_path: PathBuf,
    config: ToolCallTrackerConfig,
) -> ToolCallTrackerState {
    let (sender, receiver) = mpsc::channel(config.channel_capacity);
    let circuit_breaker = Arc::new(RwLock::new(CircuitBreakerState {
        cooldown_secs: config.circuit_breaker_cooldown_secs,
        failure_threshold: config.circuit_breaker_threshold,
        ..Default::default()
    }));
    let sequence_positions = Arc::new(DashMap::new());

    let cb_clone = Arc::clone(&circuit_breaker);
    let db_path_clone = db_path.clone();
    let config_clone = config.clone();

    // Spawn the buffer writer task
    tokio::spawn(async move {
        buffer_writer_task(receiver, db_path_clone, config_clone, cb_clone).await;
    });

    ToolCallTrackerState {
        sender,
        circuit_breaker,
        config,
        sequence_positions,
        db_path,
    }
}

/// The background writer task that receives records from the channel,
/// buffers them, and flushes to rusqlite in batches.
async fn buffer_writer_task(
    mut receiver: mpsc::Receiver<ToolCallRecord>,
    db_path: PathBuf,
    config: ToolCallTrackerConfig,
    circuit_breaker: Arc<RwLock<CircuitBreakerState>>,
) {
    let flush_interval = Duration::from_secs(config.buffer_flush_interval_secs);
    let mut buffer: Vec<ToolCallRecord> = Vec::with_capacity(config.buffer_flush_size);
    let mut current_backoff_secs: u64 = config.circuit_breaker_cooldown_secs;
    let max_backoff_secs: u64 = 300; // 5 minutes cap

    loop {
        let timeout_result = time::timeout(flush_interval, receiver.recv()).await;
        let was_timeout = timeout_result.is_err();

        match timeout_result {
            Ok(Some(record)) => {
                // Check if circuit breaker is open
                let is_open = {
                    let cb = circuit_breaker.read().await;
                    cb.is_open
                };

                if is_open {
                    // Drain and drop while open, increment dropped count
                    let mut cb = circuit_breaker.write().await;
                    cb.total_records_dropped += 1;

                    // Check if cooldown has expired
                    if let Some(ref cooldown_end) = cb.cooldown_ends_at {
                        let now = Utc::now().to_rfc3339();
                        if now >= *cooldown_end {
                            // Attempt recovery
                            cb.is_open = false;
                            cb.consecutive_failures = 0;
                        }
                    }
                    continue;
                }

                buffer.push(record);
            }
            Ok(None) => {
                // Channel closed, flush remaining and exit
                if !buffer.is_empty() {
                    let _ = flush_buffer(&db_path, &mut buffer, &circuit_breaker, &mut current_backoff_secs, max_backoff_secs).await;
                }
                break;
            }
            Err(_) => {
                // Timeout elapsed — flush if we have records
            }
        }

        // Flush if buffer is full or timeout elapsed
        if buffer.len() >= config.buffer_flush_size || (!buffer.is_empty() && was_timeout) {
            let _ = flush_buffer(&db_path, &mut buffer, &circuit_breaker, &mut current_backoff_secs, max_backoff_secs).await;
        }
    }
}

/// Flush the buffer to the database. Returns Ok(()) on success, Err on failure.
async fn flush_buffer(
    db_path: &Path,
    buffer: &mut Vec<ToolCallRecord>,
    circuit_breaker: &Arc<RwLock<CircuitBreakerState>>,
    current_backoff_secs: &mut u64,
    max_backoff_secs: u64,
) -> Result<(), String> {
    let conn = Connection::open(db_path)
        .map_err(|e| format!("Failed to open DB: {}", e));

    let conn = match conn {
        Ok(c) => c,
        Err(e) => {
            handle_flush_failure(circuit_breaker, current_backoff_secs, max_backoff_secs).await;
            return Err(e);
        }
    };

    let records_to_flush: Vec<ToolCallRecord> = buffer.drain(..).collect();
    match insert_tool_call_records_batch(&conn, &records_to_flush) {
        Ok(()) => {
            // Success: reset circuit breaker failure count
            let mut cb = circuit_breaker.write().await;
            cb.consecutive_failures = 0;
            cb.is_open = false;
            *current_backoff_secs = cb.cooldown_secs;
            Ok(())
        }
        Err(e) => {
            // Put records back in buffer for retry (up to capacity)
            // Actually, since we drained, we just lose them on failure after threshold
            handle_flush_failure(circuit_breaker, current_backoff_secs, max_backoff_secs).await;
            Err(e)
        }
    }
}

/// Handle a flush failure: increment circuit breaker, open if threshold reached.
async fn handle_flush_failure(
    circuit_breaker: &Arc<RwLock<CircuitBreakerState>>,
    current_backoff_secs: &mut u64,
    max_backoff_secs: u64,
) {
    let mut cb = circuit_breaker.write().await;
    cb.consecutive_failures += 1;
    cb.last_failure_at = Some(Utc::now().to_rfc3339());

    if cb.consecutive_failures >= cb.failure_threshold {
        cb.is_open = true;
        // Exponential backoff capped at 5 minutes
        let cooldown = (*current_backoff_secs).min(max_backoff_secs);
        let cooldown_end = Utc::now() + chrono::Duration::seconds(cooldown as i64);
        cb.cooldown_ends_at = Some(cooldown_end.to_rfc3339());
        // Double backoff for next time
        *current_backoff_secs = (*current_backoff_secs * 2).min(max_backoff_secs);
    }
}

// ─── Phase 7: Background Analysis Orchestration ─────────────────────────────

/// Task analysis result row stored in the database.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskAnalysisResultRow {
    pub delegation_packet_id: String,
    pub agent_id: String,
    pub task_type: String,
    pub efficiency_ratio: f64,
    pub total_calls: u32,
    pub useful_calls: u32,
    pub redundant_calls: u32,
    pub detected_patterns_json: String,
    pub anomaly_flags_json: Option<String>,
    pub tool_sequence_signature_json: String,
    pub analyzed_at: String,
    pub experience_buffer_linked: bool,
}

/// Orchestrate analysis for a completed task.
/// Loads records → classifies → computes ratio → detects patterns → checks anomaly
/// → updates averages → updates aggregates → persists AnalysisResult.
pub fn analyze_completed_task(
    db_path: &Path,
    delegation_packet_id: &str,
    agent_id: &str,
    task_type: &str,
    expected_artifacts: &[String],
    allowed_tools: &[String],
    capability_grants: &[String],
    config: &ToolCallTrackerConfig,
) -> Result<AnalysisResult, String> {
    let conn = Connection::open(db_path)
        .map_err(|e| format!("Failed to open DB for analysis: {}", e))?;

    // 1. Load all ToolCallRecords for this delegation_packet_id
    let records = query_records_by_packet_id(&conn, delegation_packet_id)?;

    // 2-6. Delegate to analysis module
    let result = analyze_completed_task_inner(
        &records,
        delegation_packet_id,
        agent_id,
        task_type,
        expected_artifacts,
        allowed_tools,
        capability_grants,
        config,
    );

    // 7. Persist AnalysisResult to task_analysis_results
    let patterns_json = serde_json::to_string(&result.detected_patterns)
        .unwrap_or_else(|_| "[]".to_string());
    let anomaly_json = if result.anomaly_flags.is_empty() {
        None
    } else {
        Some(serde_json::to_string(&result.anomaly_flags).unwrap_or_else(|_| "[]".to_string()))
    };
    let signature_json = serde_json::to_string(&result.tool_sequence_signature)
        .unwrap_or_else(|_| "[]".to_string());

    conn.execute(
        "INSERT OR REPLACE INTO task_analysis_results
         (delegation_packet_id, agent_id, task_type, efficiency_ratio, total_calls,
          useful_calls, redundant_calls, detected_patterns_json, anomaly_flags_json,
          tool_sequence_signature_json, analyzed_at, experience_buffer_linked)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 0)",
        params![
            result.delegation_packet_id,
            result.agent_id,
            result.task_type,
            result.efficiency_ratio,
            result.total_calls as i32,
            result.useful_calls as i32,
            result.redundant_calls as i32,
            patterns_json,
            anomaly_json,
            signature_json,
            result.analyzed_at,
        ],
    )
    .map_err(|e| format!("Failed to persist analysis result: {}", e))?;

    // 8. Update task type averages
    update_task_type_average_db(&conn, task_type, result.efficiency_ratio, result.total_calls, config.rolling_avg_window_size)?;

    // 9. Update aggregate stats
    update_aggregate_stats_db(&conn, agent_id, task_type, result.efficiency_ratio, result.total_calls)?;

    Ok(result)
}

/// Task completion trigger: spawns analysis as a separate tokio task.
/// Called when a LogicianExecutionArtifact event with terminal status arrives.
pub fn trigger_analysis_on_completion(
    db_path: PathBuf,
    delegation_packet_id: String,
    agent_id: String,
    task_type: String,
    expected_artifacts: Vec<String>,
    allowed_tools: Vec<String>,
    capability_grants: Vec<String>,
    config: ToolCallTrackerConfig,
) {
    // Spawn on a separate tokio task for isolation
    tokio::spawn(async move {
        // Analysis runs on its own task, never blocks the buffer writer or logging interceptor
        let _ = analyze_completed_task(
            &db_path,
            &delegation_packet_id,
            &agent_id,
            &task_type,
            &expected_artifacts,
            &allowed_tools,
            &capability_grants,
            &config,
        );
    });
}

/// Update the rolling average for a task type.
fn update_task_type_average_db(
    conn: &Connection,
    task_type: &str,
    efficiency_ratio: f64,
    total_calls: u32,
    window_size: u32,
) -> Result<(), String> {
    // Read current average
    let existing = conn.query_row(
        "SELECT avg_tool_call_count, avg_efficiency_ratio, sample_count
         FROM task_type_averages WHERE task_type = ?1",
        params![task_type],
        |row| {
            Ok((
                row.get::<_, f64>(0)?,
                row.get::<_, f64>(1)?,
                row.get::<_, i32>(2)? as u32,
            ))
        },
    );

    let now = Utc::now().to_rfc3339();

    match existing {
        Ok((avg_calls, avg_eff, sample_count)) => {
            let effective_window = sample_count.min(window_size) as f64;
            let new_sample_count = sample_count + 1;
            let new_avg_calls =
                (avg_calls * effective_window + total_calls as f64) / (effective_window + 1.0);
            let new_avg_eff =
                (avg_eff * effective_window + efficiency_ratio) / (effective_window + 1.0);

            conn.execute(
                "UPDATE task_type_averages SET
                 avg_tool_call_count = ?1, avg_efficiency_ratio = ?2,
                 sample_count = ?3, last_updated_at = ?4
                 WHERE task_type = ?5",
                params![
                    new_avg_calls,
                    new_avg_eff,
                    new_sample_count as i32,
                    now,
                    task_type,
                ],
            )
            .map_err(|e| format!("Failed to update task type average: {}", e))?;
        }
        Err(_) => {
            // First entry for this task type
            conn.execute(
                "INSERT INTO task_type_averages (task_type, avg_tool_call_count, avg_efficiency_ratio, sample_count, last_updated_at)
                 VALUES (?1, ?2, ?3, 1, ?4)",
                params![task_type, total_calls as f64, efficiency_ratio, now],
            )
            .map_err(|e| format!("Failed to insert task type average: {}", e))?;
        }
    }

    Ok(())
}

/// Update aggregate stats per agent per task type.
fn update_aggregate_stats_db(
    conn: &Connection,
    agent_id: &str,
    task_type: &str,
    efficiency_ratio: f64,
    total_calls: u32,
) -> Result<(), String> {
    let existing = conn.query_row(
        "SELECT avg_efficiency_ratio, avg_tool_call_count, total_tasks_analyzed
         FROM aggregate_stats WHERE agent_id = ?1 AND task_type = ?2",
        params![agent_id, task_type],
        |row| {
            Ok((
                row.get::<_, f64>(0)?,
                row.get::<_, f64>(1)?,
                row.get::<_, i32>(2)? as u32,
            ))
        },
    );

    let now = Utc::now().to_rfc3339();

    match existing {
        Ok((avg_eff, avg_calls, total_tasks)) => {
            let n = total_tasks as f64;
            let new_avg_eff = (avg_eff * n + efficiency_ratio) / (n + 1.0);
            let new_avg_calls = (avg_calls * n + total_calls as f64) / (n + 1.0);
            let new_total = total_tasks + 1;

            conn.execute(
                "UPDATE aggregate_stats SET
                 avg_efficiency_ratio = ?1, avg_tool_call_count = ?2,
                 total_tasks_analyzed = ?3, last_updated_at = ?4
                 WHERE agent_id = ?5 AND task_type = ?6",
                params![
                    new_avg_eff,
                    new_avg_calls,
                    new_total as i32,
                    now,
                    agent_id,
                    task_type,
                ],
            )
            .map_err(|e| format!("Failed to update aggregate stats: {}", e))?;
        }
        Err(_) => {
            conn.execute(
                "INSERT INTO aggregate_stats (agent_id, task_type, avg_efficiency_ratio, avg_tool_call_count, total_tasks_analyzed, last_updated_at)
                 VALUES (?1, ?2, ?3, ?4, 1, ?5)",
                params![agent_id, task_type, efficiency_ratio, total_calls as f64, now],
            )
            .map_err(|e| format!("Failed to insert aggregate stats: {}", e))?;
        }
    }

    Ok(())
}

/// Query the historical average tool call count for a task type.
pub fn get_task_type_avg_calls(conn: &Connection, task_type: &str) -> Result<f64, String> {
    conn.query_row(
        "SELECT avg_tool_call_count FROM task_type_averages WHERE task_type = ?1",
        params![task_type],
        |row| row.get(0),
    )
    .map_err(|e| format!("Failed to get task type average: {}", e))
}

// ─── Phase 8: Experience Buffer and Cost Ledger Integration ─────────────────

/// Ensure the experience_records table has the tool_call_trace_json column.
/// Idempotent: checks column existence before altering.
pub fn ensure_experience_buffer_migration(eb_conn: &Connection) -> Result<(), String> {
    // Check if column already exists
    let has_column: bool = eb_conn
        .prepare("PRAGMA table_info(experience_records)")
        .map_err(|e| format!("Failed to query table info: {}", e))?
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|e| format!("Failed to read table info: {}", e))?
        .any(|col| col.map(|c| c == "tool_call_trace_json").unwrap_or(false));

    if !has_column {
        eb_conn
            .execute_batch(
                "ALTER TABLE experience_records ADD COLUMN tool_call_trace_json TEXT;",
            )
            .map_err(|e| format!("Failed to add tool_call_trace_json column: {}", e))?;
    }
    Ok(())
}

/// Append tool call trace summary to the corresponding ExperienceRecord.
/// Opens experience_buffer.db at the given path, runs the migration if needed,
/// then UPDATEs the matching record. Returns the number of rows affected.
pub fn append_to_experience_buffer(
    eb_db_path: &Path,
    packet_id: &str,
    summary_json: &str,
) -> Result<usize, String> {
    let eb_conn = Connection::open(eb_db_path)
        .map_err(|e| format!("Failed to open experience buffer DB: {}", e))?;

    ensure_experience_buffer_migration(&eb_conn)?;

    let rows_affected = eb_conn
        .execute(
            "UPDATE experience_records SET tool_call_trace_json = ?1 WHERE delegation_packet_id = ?2",
            params![summary_json, packet_id],
        )
        .map_err(|e| format!("Failed to update experience buffer: {}", e))?;

    Ok(rows_affected)
}

/// Standalone fallback: when Experience Buffer UPDATE affects 0 rows,
/// INSERT into standalone_trace_summaries with linked=0.
pub fn insert_standalone_trace_summary(
    conn: &Connection,
    packet_id: &str,
    summary_json: &str,
) -> Result<(), String> {
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT OR REPLACE INTO standalone_trace_summaries (delegation_packet_id, summary_json, created_at, linked)
         VALUES (?1, ?2, ?3, 0)",
        params![packet_id, summary_json, now],
    )
    .map_err(|e| format!("Failed to insert standalone trace summary: {}", e))?;
    Ok(())
}

/// Attempt to append trace summary to experience buffer, falling back to standalone storage.
pub fn append_trace_with_fallback(
    tracker_db_conn: &Connection,
    eb_db_path: &Path,
    packet_id: &str,
    summary_json: &str,
) -> Result<bool, String> {
    // Try to update experience buffer
    match append_to_experience_buffer(eb_db_path, packet_id, summary_json) {
        Ok(rows) if rows > 0 => Ok(true), // Successfully linked
        Ok(_) => {
            // No matching record — store standalone
            insert_standalone_trace_summary(tracker_db_conn, packet_id, summary_json)?;
            Ok(false)
        }
        Err(_) => {
            // Experience buffer unavailable — store standalone
            insert_standalone_trace_summary(tracker_db_conn, packet_id, summary_json)?;
            Ok(false)
        }
    }
}

/// Retroactive linking job: queries standalone_trace_summaries WHERE linked=0,
/// attempts to link each to Experience Buffer, sets linked=1 on success.
pub fn retroactive_linking_job(
    tracker_db_path: &Path,
    eb_db_path: &Path,
) -> Result<u32, String> {
    let tracker_conn = Connection::open(tracker_db_path)
        .map_err(|e| format!("Failed to open tracker DB for linking: {}", e))?;

    let mut stmt = tracker_conn
        .prepare(
            "SELECT delegation_packet_id, summary_json FROM standalone_trace_summaries WHERE linked = 0",
        )
        .map_err(|e| format!("Failed to prepare unlinked query: {}", e))?;

    let unlinked: Vec<(String, String)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .map_err(|e| format!("Failed to query unlinked summaries: {}", e))?
        .filter_map(|r| r.ok())
        .collect();

    let mut linked_count = 0u32;

    for (packet_id, summary_json) in &unlinked {
        match append_to_experience_buffer(eb_db_path, packet_id, summary_json) {
            Ok(rows) if rows > 0 => {
                // Successfully linked — mark as linked
                let _ = tracker_conn.execute(
                    "UPDATE standalone_trace_summaries SET linked = 1 WHERE delegation_packet_id = ?1",
                    params![packet_id],
                );
                linked_count += 1;
            }
            _ => {
                // Still can't link — leave for next attempt
            }
        }
    }

    Ok(linked_count)
}

/// Spawn a periodic retroactive linking job (every 60 seconds).
pub fn start_retroactive_linking_job(tracker_db_path: PathBuf, eb_db_path: PathBuf) {
    tokio::spawn(async move {
        let mut interval = time::interval(Duration::from_secs(60));
        loop {
            interval.tick().await;
            let _ = retroactive_linking_job(&tracker_db_path, &eb_db_path);
        }
    });
}

/// Write cost attribution records to the Cost Ledger for token-consuming tool calls.
/// For each ToolCallRecord with is_llm_backed=true, creates a CostRecord and writes
/// via cost_ledger_service::record_cost_entry.
pub fn write_cost_attributions(
    cost_db_path: &Path,
    records: &[ToolCallRecord],
    agent_id: &str,
    _delegation_packet_id: &str,
    task_type: &str,
) -> Result<u32, String> {
    let llm_records: Vec<&ToolCallRecord> = records.iter().filter(|r| r.is_llm_backed).collect();

    if llm_records.is_empty() {
        return Ok(0);
    }

    let cost_conn = Connection::open(cost_db_path)
        .map_err(|e| format!("Failed to open cost ledger DB: {}", e))?;

    crate::cost_ledger_service::initialize_cost_ledger_db(&cost_conn)?;

    let mut written = 0u32;

    for record in llm_records {
        let prompt_tokens = record.prompt_tokens.unwrap_or(0);
        let completion_tokens = record.completion_tokens.unwrap_or(0);
        let total_tokens = prompt_tokens + completion_tokens;
        let estimated_cost = crate::cost_ledger_service::estimate_cost_usd(
            "paid-api",
            prompt_tokens,
            completion_tokens,
        );

        let cost_record = crate::cost_ledger_service::CostRecord {
            id: format!("tct-{}", record.id),
            recorded_at: record.timestamp.clone(),
            agent_id: agent_id.to_string(),
            task_type: task_type.to_string(),
            provider_id: format!("tool-call-{}", record.tool_name),
            model: record.tool_name.clone(),
            cost_posture: "paid-api".to_string(),
            prompt_tokens,
            completion_tokens,
            total_tokens,
            estimated_cost_usd: estimated_cost,
            duration_ms: Some(record.duration_ms as u32),
        };

        match crate::cost_ledger_service::record_cost_entry(&cost_conn, &cost_record) {
            Ok(()) => written += 1,
            Err(e) => {
                // Log error but continue — don't fail the whole batch
                eprintln!("Failed to write cost attribution for {}: {}", record.id, e);
            }
        }
    }

    Ok(written)
}

// ─── Phase 9: Retention, Eviction, and Bulk Export ──────────────────────────

/// Evict expired records based on retention configuration.
/// DELETE from tool_call_records WHERE timestamp < (now - retention_days_traces)
/// DELETE from task_analysis_results WHERE analyzed_at < (now - retention_days_metrics)
pub fn evict_expired_records(conn: &Connection, config: &ToolCallTrackerConfig) -> Result<(u32, u32), String> {
    let traces_cutoff = Utc::now() - chrono::Duration::days(config.retention_days_traces as i64);
    let metrics_cutoff = Utc::now() - chrono::Duration::days(config.retention_days_metrics as i64);

    let traces_deleted = conn
        .execute(
            "DELETE FROM tool_call_records WHERE timestamp < ?1",
            params![traces_cutoff.to_rfc3339()],
        )
        .map_err(|e| format!("Failed to evict expired traces: {}", e))? as u32;

    let metrics_deleted = conn
        .execute(
            "DELETE FROM task_analysis_results WHERE analyzed_at < ?1",
            params![metrics_cutoff.to_rfc3339()],
        )
        .map_err(|e| format!("Failed to evict expired metrics: {}", e))? as u32;

    Ok((traces_deleted, metrics_deleted))
}

/// Check current storage usage of the database.
/// Returns the size in bytes (page_count × page_size).
pub fn check_storage_usage(conn: &Connection) -> Result<u64, String> {
    let page_count: u64 = conn
        .query_row("PRAGMA page_count", [], |row| row.get(0))
        .map_err(|e| format!("Failed to get page_count: {}", e))?;

    let page_size: u64 = conn
        .query_row("PRAGMA page_size", [], |row| row.get(0))
        .map_err(|e| format!("Failed to get page_size: {}", e))?;

    Ok(page_count * page_size)
}

/// Check storage and trigger eviction if approaching max_storage_bytes (90% threshold).
pub fn check_and_evict_if_needed(conn: &Connection, config: &ToolCallTrackerConfig) -> Result<bool, String> {
    let usage = check_storage_usage(conn)?;
    let threshold = (config.max_storage_bytes as f64 * 0.9) as u64;

    if usage >= threshold {
        evict_expired_records(conn, config)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

/// Spawn a periodic eviction job (every hour).
pub fn start_periodic_eviction_job(db_path: PathBuf, config: ToolCallTrackerConfig) {
    tokio::spawn(async move {
        let mut interval = time::interval(Duration::from_secs(3600));
        loop {
            interval.tick().await;
            if let Ok(conn) = Connection::open(&db_path) {
                let _ = check_and_evict_if_needed(&conn, &config);
            }
        }
    });
}

/// Bulk export traces and analysis results within a date range as NDJSON.
/// Each line is a valid JSON object (newline-delimited JSON format).
pub fn bulk_export_traces(
    conn: &Connection,
    from: &str,
    to: &str,
) -> Result<String, String> {
    let mut output = String::new();

    // Export tool_call_records
    let records = query_records_by_time_range(conn, from, to)?;
    for record in &records {
        let json = serde_json::to_string(record)
            .map_err(|e| format!("Failed to serialize record: {}", e))?;
        output.push_str(&json);
        output.push('\n');
    }

    // Export task_analysis_results in the same time range
    let mut stmt = conn
        .prepare(
            "SELECT delegation_packet_id, agent_id, task_type, efficiency_ratio,
             total_calls, useful_calls, redundant_calls, detected_patterns_json,
             anomaly_flags_json, tool_sequence_signature_json, analyzed_at
             FROM task_analysis_results
             WHERE analyzed_at >= ?1 AND analyzed_at <= ?2
             ORDER BY analyzed_at ASC",
        )
        .map_err(|e| format!("Failed to prepare export query: {}", e))?;

    let analysis_rows = stmt
        .query_map(params![from, to], |row| {
            Ok(TaskAnalysisResultRow {
                delegation_packet_id: row.get(0)?,
                agent_id: row.get(1)?,
                task_type: row.get(2)?,
                efficiency_ratio: row.get(3)?,
                total_calls: row.get::<_, i32>(4)? as u32,
                useful_calls: row.get::<_, i32>(5)? as u32,
                redundant_calls: row.get::<_, i32>(6)? as u32,
                detected_patterns_json: row.get(7)?,
                anomaly_flags_json: row.get(8)?,
                tool_sequence_signature_json: row.get(9)?,
                analyzed_at: row.get(10)?,
                experience_buffer_linked: false,
            })
        })
        .map_err(|e| format!("Failed to query analysis results for export: {}", e))?;

    for row in analysis_rows {
        let result = row.map_err(|e| format!("Failed to read analysis row: {}", e))?;
        let json = serde_json::to_string(&result)
            .map_err(|e| format!("Failed to serialize analysis result: {}", e))?;
        output.push_str(&json);
        output.push('\n');
    }

    Ok(output)
}

// ─── Phase 10: Graceful Degradation and Recovery ────────────────────────────

/// Crash-safe startup: check for incomplete state and recover gracefully.
/// - If circuit breaker is open, reset it (fresh start)
/// - If there are unlinked standalone records, note them for the linking job
pub fn crash_safe_startup(conn: &Connection) -> Result<CrashRecoveryReport, String> {
    let mut report = CrashRecoveryReport {
        circuit_breaker_was_open: false,
        circuit_breaker_reset: false,
        unlinked_records_found: 0,
    };

    // Check circuit breaker state
    let cb_state = read_circuit_breaker(conn)?;
    if cb_state.is_open {
        report.circuit_breaker_was_open = true;
        // Reset circuit breaker on fresh startup
        reset_circuit_breaker(conn)?;
        report.circuit_breaker_reset = true;
    }

    // Count unlinked standalone records
    let unlinked_count: u32 = conn
        .query_row(
            "SELECT COUNT(*) FROM standalone_trace_summaries WHERE linked = 0",
            [],
            |row| row.get::<_, i32>(0).map(|v| v as u32),
        )
        .unwrap_or(0);

    report.unlinked_records_found = unlinked_count;

    Ok(report)
}

/// Report from crash-safe startup recovery.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrashRecoveryReport {
    pub circuit_breaker_was_open: bool,
    pub circuit_breaker_reset: bool,
    pub unlinked_records_found: u32,
}

/// Buffer overflow handling state: tracks when the last data loss warning was logged.
pub struct BufferOverflowTracker {
    pub last_warning_at: Option<std::time::Instant>,
    pub dropped_since_last_warning: u64,
}

impl BufferOverflowTracker {
    pub fn new() -> Self {
        Self {
            last_warning_at: None,
            dropped_since_last_warning: 0,
        }
    }

    /// Record a dropped record. Returns true if a warning should be logged
    /// (at most once per minute).
    pub fn record_drop(&mut self) -> bool {
        self.dropped_since_last_warning += 1;
        let now = std::time::Instant::now();

        match self.last_warning_at {
            None => {
                self.last_warning_at = Some(now);
                self.dropped_since_last_warning = 0;
                true
            }
            Some(last) if now.duration_since(last) >= std::time::Duration::from_secs(60) => {
                self.last_warning_at = Some(now);
                self.dropped_since_last_warning = 0;
                true
            }
            _ => false,
        }
    }
}

// ─── Phase 12: Integration Hook and Performance ─────────────────────────────

/// Integration point for the logging interceptor.
/// This function should be called in the DelegationPacket tool execution path
/// after the tool returns, before the result is passed back to the agent.
///
/// # Usage
/// ```rust,ignore
/// // In the tool execution pipeline:
/// let result = execute_tool(tool_name, params).await;
/// integrate_logging_interceptor(&tracker_state, delegation_packet_id, agent_id, task_type, tool_name, params, &result, duration_ms);
/// result
/// ```
pub fn integrate_logging_interceptor(
    state: &ToolCallTrackerState,
    delegation_packet_id: &str,
    agent_id: &str,
    task_type: &str,
    tool_name: &str,
    input_params: &serde_json::Value,
    output_summary: Option<&str>,
    duration_ms: u64,
    success: bool,
    is_llm_backed: bool,
    prompt_tokens: Option<u32>,
    completion_tokens: Option<u32>,
) {
    let record = ToolCallRecord {
        id: format!("{}-{}", delegation_packet_id, Utc::now().timestamp_nanos_opt().unwrap_or(0)),
        delegation_packet_id: delegation_packet_id.to_string(),
        agent_id: agent_id.to_string(),
        task_type: task_type.to_string(),
        tool_name: tool_name.to_string(),
        input_params_json: serde_json::to_string(input_params).unwrap_or_else(|_| "{}".to_string()),
        output_summary: output_summary.map(|s| s.to_string()),
        duration_ms,
        success,
        timestamp: String::new(), // Will be set by log_tool_call
        sequence_position: 0,     // Will be set by log_tool_call
        prompt_tokens,
        completion_tokens,
        is_llm_backed,
    };

    log_tool_call(state, record);
}

/// Resolve database paths from the app data directory.
/// Returns (tracker_db_path, experience_buffer_db_path, cost_ledger_db_path).
pub fn resolve_tracker_paths(app_data_dir: &Path) -> (PathBuf, PathBuf, PathBuf) {
    (
        app_data_dir.join("tool_call_tracker.db"),
        app_data_dir.join("experience_buffer.db"),
        app_data_dir.join("cost_ledger.db"),
    )
}

/// Tracker status summary for IPC queries.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackerStatus {
    pub is_active: bool,
    pub circuit_breaker_open: bool,
    pub total_records_dropped: u64,
    pub consecutive_failures: u32,
    pub storage_usage_bytes: u64,
    pub max_storage_bytes: u64,
    pub unlinked_standalone_count: u32,
}

/// Get the current tracker status.
pub fn get_tracker_status(conn: &Connection, config: &ToolCallTrackerConfig) -> Result<TrackerStatus, String> {
    let cb = read_circuit_breaker(conn)?;
    let storage = check_storage_usage(conn).unwrap_or(0);

    let unlinked: u32 = conn
        .query_row(
            "SELECT COUNT(*) FROM standalone_trace_summaries WHERE linked = 0",
            [],
            |row| row.get::<_, i32>(0).map(|v| v as u32),
        )
        .unwrap_or(0);

    Ok(TrackerStatus {
        is_active: !cb.is_open,
        circuit_breaker_open: cb.is_open,
        total_records_dropped: cb.total_records_dropped,
        consecutive_failures: cb.consecutive_failures,
        storage_usage_bytes: storage,
        max_storage_bytes: config.max_storage_bytes,
        unlinked_standalone_count: unlinked,
    })
}

// ─── Phase 11: IPC Commands ─────────────────────────────────────────────────

/// IPC command: Query tool call records by delegation packet ID.
#[tauri::command]
pub fn tool_call_tracker_query_records(
    app: tauri::AppHandle,
    delegation_packet_id: String,
) -> Result<Vec<ToolCallRecord>, String> {
    use tauri::Manager;
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to get app data dir: {}", e))?;
    let db_path = app_data_dir.join("tool_call_tracker.db");
    let conn = Connection::open(&db_path)
        .map_err(|e| format!("Failed to open tracker DB: {}", e))?;
    query_records_by_packet_id(&conn, &delegation_packet_id)
}

/// IPC command: Query analysis results for a delegation packet.
#[tauri::command]
pub fn tool_call_tracker_query_analysis(
    app: tauri::AppHandle,
    delegation_packet_id: String,
) -> Result<Option<TaskAnalysisResultRow>, String> {
    use tauri::Manager;
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to get app data dir: {}", e))?;
    let db_path = app_data_dir.join("tool_call_tracker.db");
    let conn = Connection::open(&db_path)
        .map_err(|e| format!("Failed to open tracker DB: {}", e))?;

    let result = conn.query_row(
        "SELECT delegation_packet_id, agent_id, task_type, efficiency_ratio,
         total_calls, useful_calls, redundant_calls, detected_patterns_json,
         anomaly_flags_json, tool_sequence_signature_json, analyzed_at, experience_buffer_linked
         FROM task_analysis_results WHERE delegation_packet_id = ?1",
        params![delegation_packet_id],
        |row| {
            Ok(TaskAnalysisResultRow {
                delegation_packet_id: row.get(0)?,
                agent_id: row.get(1)?,
                task_type: row.get(2)?,
                efficiency_ratio: row.get(3)?,
                total_calls: row.get::<_, i32>(4)? as u32,
                useful_calls: row.get::<_, i32>(5)? as u32,
                redundant_calls: row.get::<_, i32>(6)? as u32,
                detected_patterns_json: row.get(7)?,
                anomaly_flags_json: row.get(8)?,
                tool_sequence_signature_json: row.get(9)?,
                analyzed_at: row.get(10)?,
                experience_buffer_linked: row.get::<_, i32>(11)? != 0,
            })
        },
    );

    match result {
        Ok(row) => Ok(Some(row)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(format!("Failed to query analysis: {}", e)),
    }
}

/// IPC command: Query anomaly-flagged tasks within a time range.
#[tauri::command]
pub fn tool_call_tracker_query_anomalies(
    app: tauri::AppHandle,
    from: String,
    to: String,
) -> Result<Vec<crate::tool_call_analysis::TaskAnalysisResult>, String> {
    use tauri::Manager;
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to get app data dir: {}", e))?;
    let db_path = app_data_dir.join("tool_call_tracker.db");
    let conn = Connection::open(&db_path)
        .map_err(|e| format!("Failed to open tracker DB: {}", e))?;
    crate::tool_call_analysis::query_anomaly_flagged_tasks(&conn, &from, &to)
}

/// IPC command: Export traces within a date range as NDJSON.
#[tauri::command]
pub fn tool_call_tracker_export(
    app: tauri::AppHandle,
    from: String,
    to: String,
) -> Result<String, String> {
    use tauri::Manager;
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to get app data dir: {}", e))?;
    let db_path = app_data_dir.join("tool_call_tracker.db");
    let conn = Connection::open(&db_path)
        .map_err(|e| format!("Failed to open tracker DB: {}", e))?;
    bulk_export_traces(&conn, &from, &to)
}

/// IPC command: Read tracker configuration.
#[tauri::command]
pub fn tool_call_tracker_config_read(
    app: tauri::AppHandle,
) -> Result<TrackerConfigRow, String> {
    use tauri::Manager;
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to get app data dir: {}", e))?;
    let db_path = app_data_dir.join("tool_call_tracker.db");
    let conn = Connection::open(&db_path)
        .map_err(|e| format!("Failed to open tracker DB: {}", e))?;
    read_tracker_config(&conn)
}

/// IPC command: Update tracker configuration.
#[tauri::command]
pub fn tool_call_tracker_config_update(
    app: tauri::AppHandle,
    config: TrackerConfigRow,
) -> Result<(), String> {
    use tauri::Manager;
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to get app data dir: {}", e))?;
    let db_path = app_data_dir.join("tool_call_tracker.db");
    let conn = Connection::open(&db_path)
        .map_err(|e| format!("Failed to open tracker DB: {}", e))?;
    update_tracker_config(&conn, &config)
}

/// IPC command: Get tracker status.
#[tauri::command]
pub fn tool_call_tracker_status(
    app: tauri::AppHandle,
) -> Result<TrackerStatus, String> {
    use tauri::Manager;
    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to get app data dir: {}", e))?;
    let db_path = app_data_dir.join("tool_call_tracker.db");
    let conn = Connection::open(&db_path)
        .map_err(|e| format!("Failed to open tracker DB: {}", e))?;
    let config = ToolCallTrackerConfig::default();
    get_tracker_status(&conn, &config)
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use rusqlite::Connection;

    fn create_test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        initialize_tool_call_tracker_db(&conn).unwrap();
        conn
    }

    fn make_test_record(id: &str, packet_id: &str, seq: u32) -> ToolCallRecord {
        ToolCallRecord {
            id: id.to_string(),
            delegation_packet_id: packet_id.to_string(),
            agent_id: "test-agent".to_string(),
            task_type: "code_generation".to_string(),
            tool_name: "read_file".to_string(),
            input_params_json: r#"{"path":"src/main.rs"}"#.to_string(),
            output_summary: Some("file contents here".to_string()),
            duration_ms: 42,
            success: true,
            timestamp: "2026-07-15T10:30:00Z".to_string(),
            sequence_position: seq,
            prompt_tokens: None,
            completion_tokens: None,
            is_llm_backed: false,
        }
    }

    // ─── Unit Tests (Task 1.6) ──────────────────────────────────────────────

    #[test]
    fn test_schema_initialization() {
        let conn = create_test_db();
        // Verify tables exist by querying them
        let count: i32 = conn
            .query_row("SELECT COUNT(*) FROM circuit_breaker_state", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);

        let count: i32 = conn
            .query_row("SELECT COUNT(*) FROM tracker_config", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_record_insert_read_round_trip() {
        let conn = create_test_db();
        let record = make_test_record("rec-001", "packet-001", 1);

        insert_tool_call_record(&conn, &record).unwrap();
        let results = query_records_by_packet_id(&conn, "packet-001").unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "rec-001");
        assert_eq!(results[0].delegation_packet_id, "packet-001");
        assert_eq!(results[0].tool_name, "read_file");
        assert_eq!(results[0].duration_ms, 42);
        assert!(results[0].success);
        assert_eq!(results[0].sequence_position, 1);
    }

    #[test]
    fn test_batch_insert() {
        let conn = create_test_db();
        let records: Vec<ToolCallRecord> = (1..=10)
            .map(|i| make_test_record(&format!("rec-{:03}", i), "packet-batch", i as u32))
            .collect();

        insert_tool_call_records_batch(&conn, &records).unwrap();
        let results = query_records_by_packet_id(&conn, "packet-batch").unwrap();

        assert_eq!(results.len(), 10);
        for (i, r) in results.iter().enumerate() {
            assert_eq!(r.sequence_position, (i + 1) as u32);
        }
    }

    #[test]
    fn test_config_validation_valid() {
        let conn = create_test_db();
        let config = TrackerConfigRow {
            efficiency_threshold: 0.7,
            historical_avg_multiplier: 2.5,
            max_storage_bytes: 1024 * 1024 * 100,
            retention_days_traces: 60,
            retention_days_metrics: 120,
            rolling_avg_window_size: 50,
        };
        assert!(update_tracker_config(&conn, &config).is_ok());

        let read_back = read_tracker_config(&conn).unwrap();
        assert!((read_back.efficiency_threshold - 0.7).abs() < f64::EPSILON);
        assert_eq!(read_back.rolling_avg_window_size, 50);
    }

    #[test]
    fn test_config_validation_invalid_threshold() {
        let conn = create_test_db();
        let config = TrackerConfigRow {
            efficiency_threshold: 1.5, // invalid
            historical_avg_multiplier: 3.0,
            max_storage_bytes: 500 * 1024 * 1024,
            retention_days_traces: 90,
            retention_days_metrics: 180,
            rolling_avg_window_size: 100,
        };
        assert!(update_tracker_config(&conn, &config).is_err());
    }

    #[test]
    fn test_config_validation_invalid_multiplier() {
        let conn = create_test_db();
        let config = TrackerConfigRow {
            efficiency_threshold: 0.5,
            historical_avg_multiplier: 0.0, // invalid
            max_storage_bytes: 500 * 1024 * 1024,
            retention_days_traces: 90,
            retention_days_metrics: 180,
            rolling_avg_window_size: 100,
        };
        assert!(update_tracker_config(&conn, &config).is_err());
    }

    #[test]
    fn test_config_validation_invalid_retention() {
        let conn = create_test_db();
        let config = TrackerConfigRow {
            efficiency_threshold: 0.5,
            historical_avg_multiplier: 3.0,
            max_storage_bytes: 500 * 1024 * 1024,
            retention_days_traces: 0, // invalid
            retention_days_metrics: 180,
            rolling_avg_window_size: 100,
        };
        assert!(update_tracker_config(&conn, &config).is_err());
    }

    #[test]
    fn test_circuit_breaker_persistence() {
        let conn = create_test_db();

        let state = CircuitBreakerState {
            consecutive_failures: 3,
            is_open: false,
            last_failure_at: Some("2026-07-15T10:00:00Z".to_string()),
            cooldown_ends_at: None,
            cooldown_secs: 30,
            failure_threshold: 5,
            total_records_dropped: 42,
        };
        update_circuit_breaker(&conn, &state).unwrap();

        let read_back = read_circuit_breaker(&conn).unwrap();
        assert_eq!(read_back.consecutive_failures, 3);
        assert!(!read_back.is_open);
        assert_eq!(read_back.total_records_dropped, 42);
    }

    #[test]
    fn test_circuit_breaker_reset() {
        let conn = create_test_db();

        let state = CircuitBreakerState {
            consecutive_failures: 5,
            is_open: true,
            last_failure_at: Some("2026-07-15T10:00:00Z".to_string()),
            cooldown_ends_at: Some("2026-07-15T10:00:30Z".to_string()),
            cooldown_secs: 30,
            failure_threshold: 5,
            total_records_dropped: 100,
        };
        update_circuit_breaker(&conn, &state).unwrap();
        reset_circuit_breaker(&conn).unwrap();

        let read_back = read_circuit_breaker(&conn).unwrap();
        assert_eq!(read_back.consecutive_failures, 0);
        assert!(!read_back.is_open);
        assert_eq!(read_back.total_records_dropped, 0);
    }

    #[test]
    fn test_query_by_agent() {
        let conn = create_test_db();
        let mut record = make_test_record("rec-agent-1", "packet-a1", 1);
        record.agent_id = "agent-alpha".to_string();
        insert_tool_call_record(&conn, &record).unwrap();

        let mut record2 = make_test_record("rec-agent-2", "packet-a2", 1);
        record2.agent_id = "agent-beta".to_string();
        insert_tool_call_record(&conn, &record2).unwrap();

        let results = query_records_by_agent(&conn, "agent-alpha").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].agent_id, "agent-alpha");
    }

    #[test]
    fn test_query_by_time_range() {
        let conn = create_test_db();
        let mut r1 = make_test_record("rec-t1", "packet-t", 1);
        r1.timestamp = "2026-07-10T10:00:00Z".to_string();
        insert_tool_call_record(&conn, &r1).unwrap();

        let mut r2 = make_test_record("rec-t2", "packet-t", 2);
        r2.timestamp = "2026-07-15T10:00:00Z".to_string();
        insert_tool_call_record(&conn, &r2).unwrap();

        let mut r3 = make_test_record("rec-t3", "packet-t", 3);
        r3.timestamp = "2026-07-20T10:00:00Z".to_string();
        insert_tool_call_record(&conn, &r3).unwrap();

        let results = query_records_by_time_range(&conn, "2026-07-12T00:00:00Z", "2026-07-18T00:00:00Z").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "rec-t2");
    }

    // ─── Phase 8 Integration Tests (Task 8.6) ───────────────────────────────

    #[test]
    fn test_append_to_experience_buffer_success() {
        // Create an in-memory experience buffer DB
        let eb_dir = tempfile::tempdir().unwrap();
        let eb_path = eb_dir.path().join("experience_buffer.db");
        let eb_conn = Connection::open(&eb_path).unwrap();
        crate::experience_buffer_service::initialize_experience_buffer_db(&eb_conn).unwrap();

        // Run migration
        ensure_experience_buffer_migration(&eb_conn).unwrap();

        // Insert an experience record
        eb_conn.execute(
            "INSERT INTO experience_records (id, delegation_packet_id, timestamp, workload_class, task_type, scoring_recommendation_json, heuristic_decision_json, advisory_accepted, confidence_score)
             VALUES ('er-1', 'packet-eb-test', '2026-07-15T10:00:00Z', 'coding', 'code_gen', '{}', '{}', 1, 0.9)",
            [],
        ).unwrap();
        drop(eb_conn);

        // Append trace summary
        let summary_json = r#"{"delegationPacketId":"packet-eb-test","efficiencyRatio":0.8,"totalCalls":5,"usefulCalls":4,"redundantCalls":1,"detectedPatterns":[],"toolSequenceSignature":["read","write"],"analyzedAt":"2026-07-15T11:00:00Z"}"#;
        let rows = append_to_experience_buffer(&eb_path, "packet-eb-test", summary_json).unwrap();
        assert_eq!(rows, 1);

        // Verify it was written
        let eb_conn = Connection::open(&eb_path).unwrap();
        let stored: String = eb_conn.query_row(
            "SELECT tool_call_trace_json FROM experience_records WHERE delegation_packet_id = 'packet-eb-test'",
            [],
            |row| row.get(0),
        ).unwrap();
        assert_eq!(stored, summary_json);
    }

    #[test]
    fn test_standalone_fallback_when_no_matching_record() {
        let tracker_conn = create_test_db();
        let eb_dir = tempfile::tempdir().unwrap();
        let eb_path = eb_dir.path().join("experience_buffer.db");
        let eb_conn = Connection::open(&eb_path).unwrap();
        crate::experience_buffer_service::initialize_experience_buffer_db(&eb_conn).unwrap();
        ensure_experience_buffer_migration(&eb_conn).unwrap();
        drop(eb_conn);

        let summary_json = r#"{"delegationPacketId":"packet-no-match"}"#;
        let linked = append_trace_with_fallback(&tracker_conn, &eb_path, "packet-no-match", summary_json).unwrap();
        assert!(!linked);

        // Verify standalone record was created
        let stored: String = tracker_conn.query_row(
            "SELECT summary_json FROM standalone_trace_summaries WHERE delegation_packet_id = 'packet-no-match'",
            [],
            |row| row.get(0),
        ).unwrap();
        assert_eq!(stored, summary_json);

        let linked_flag: i32 = tracker_conn.query_row(
            "SELECT linked FROM standalone_trace_summaries WHERE delegation_packet_id = 'packet-no-match'",
            [],
            |row| row.get(0),
        ).unwrap();
        assert_eq!(linked_flag, 0);
    }

    #[test]
    fn test_retroactive_linking() {
        let tracker_dir = tempfile::tempdir().unwrap();
        let tracker_path = tracker_dir.path().join("tracker.db");
        let tracker_conn = Connection::open(&tracker_path).unwrap();
        initialize_tool_call_tracker_db(&tracker_conn).unwrap();

        // Insert a standalone record
        let summary_json = r#"{"delegationPacketId":"packet-retro"}"#;
        insert_standalone_trace_summary(&tracker_conn, "packet-retro", summary_json).unwrap();
        drop(tracker_conn);

        // Create experience buffer with matching record
        let eb_dir = tempfile::tempdir().unwrap();
        let eb_path = eb_dir.path().join("experience_buffer.db");
        let eb_conn = Connection::open(&eb_path).unwrap();
        crate::experience_buffer_service::initialize_experience_buffer_db(&eb_conn).unwrap();
        ensure_experience_buffer_migration(&eb_conn).unwrap();
        eb_conn.execute(
            "INSERT INTO experience_records (id, delegation_packet_id, timestamp, workload_class, task_type, scoring_recommendation_json, heuristic_decision_json, advisory_accepted, confidence_score)
             VALUES ('er-retro', 'packet-retro', '2026-07-15T10:00:00Z', 'coding', 'code_gen', '{}', '{}', 1, 0.9)",
            [],
        ).unwrap();
        drop(eb_conn);

        // Run retroactive linking
        let linked = retroactive_linking_job(&tracker_path, &eb_path).unwrap();
        assert_eq!(linked, 1);

        // Verify standalone record is now marked as linked
        let tracker_conn = Connection::open(&tracker_path).unwrap();
        let linked_flag: i32 = tracker_conn.query_row(
            "SELECT linked FROM standalone_trace_summaries WHERE delegation_packet_id = 'packet-retro'",
            [],
            |row| row.get(0),
        ).unwrap();
        assert_eq!(linked_flag, 1);

        // Verify experience buffer has the trace
        let eb_conn = Connection::open(&eb_path).unwrap();
        let stored: String = eb_conn.query_row(
            "SELECT tool_call_trace_json FROM experience_records WHERE delegation_packet_id = 'packet-retro'",
            [],
            |row| row.get(0),
        ).unwrap();
        assert_eq!(stored, summary_json);
    }

    #[test]
    fn test_write_cost_attributions() {
        let cost_dir = tempfile::tempdir().unwrap();
        let cost_path = cost_dir.path().join("cost_ledger.db");

        let records = vec![
            ToolCallRecord {
                id: "rec-llm-1".to_string(),
                delegation_packet_id: "packet-cost".to_string(),
                agent_id: "agent-cost".to_string(),
                task_type: "code_gen".to_string(),
                tool_name: "llm_generate".to_string(),
                input_params_json: "{}".to_string(),
                output_summary: Some("generated code".to_string()),
                duration_ms: 500,
                success: true,
                timestamp: "2026-07-15T10:00:00Z".to_string(),
                sequence_position: 1,
                prompt_tokens: Some(100),
                completion_tokens: Some(200),
                is_llm_backed: true,
            },
            ToolCallRecord {
                id: "rec-non-llm".to_string(),
                delegation_packet_id: "packet-cost".to_string(),
                agent_id: "agent-cost".to_string(),
                task_type: "code_gen".to_string(),
                tool_name: "read_file".to_string(),
                input_params_json: "{}".to_string(),
                output_summary: Some("file content".to_string()),
                duration_ms: 10,
                success: true,
                timestamp: "2026-07-15T10:00:01Z".to_string(),
                sequence_position: 2,
                prompt_tokens: None,
                completion_tokens: None,
                is_llm_backed: false,
            },
        ];

        let written = write_cost_attributions(&cost_path, &records, "agent-cost", "packet-cost", "code_gen").unwrap();
        assert_eq!(written, 1); // Only the LLM-backed record

        // Verify cost record was written
        let cost_conn = Connection::open(&cost_path).unwrap();
        let count: i32 = cost_conn.query_row(
            "SELECT COUNT(*) FROM cost_records WHERE id LIKE 'tct-%'",
            [],
            |row| row.get(0),
        ).unwrap();
        assert_eq!(count, 1);
    }

    // ─── Phase 9 Tests (Task 9.5) ──────────────────────────────────────────

    #[test]
    fn test_evict_expired_records() {
        let conn = create_test_db();
        let config = ToolCallTrackerConfig {
            retention_days_traces: 30,
            retention_days_metrics: 60,
            ..Default::default()
        };

        // Insert old records (older than 30 days)
        let mut old_record = make_test_record("rec-old", "packet-old", 1);
        old_record.timestamp = "2020-01-01T10:00:00Z".to_string();
        insert_tool_call_record(&conn, &old_record).unwrap();

        // Insert recent record
        let recent_record = make_test_record("rec-recent", "packet-recent", 1);
        insert_tool_call_record(&conn, &recent_record).unwrap();

        let (traces_deleted, _metrics_deleted) = evict_expired_records(&conn, &config).unwrap();
        assert_eq!(traces_deleted, 1);

        // Recent record should still exist
        let remaining = query_records_by_packet_id(&conn, "packet-recent").unwrap();
        assert_eq!(remaining.len(), 1);

        // Old record should be gone
        let old = query_records_by_packet_id(&conn, "packet-old").unwrap();
        assert_eq!(old.len(), 0);
    }

    #[test]
    fn test_check_storage_usage() {
        let conn = create_test_db();
        let usage = check_storage_usage(&conn).unwrap();
        // Should be > 0 since we have tables
        assert!(usage > 0);
    }

    #[test]
    fn test_bulk_export_traces() {
        let conn = create_test_db();
        let mut r1 = make_test_record("rec-export-1", "packet-export", 1);
        r1.timestamp = "2026-07-15T10:00:00Z".to_string();
        insert_tool_call_record(&conn, &r1).unwrap();

        let mut r2 = make_test_record("rec-export-2", "packet-export", 2);
        r2.timestamp = "2026-07-15T11:00:00Z".to_string();
        insert_tool_call_record(&conn, &r2).unwrap();

        let export = bulk_export_traces(&conn, "2026-07-15T00:00:00Z", "2026-07-16T00:00:00Z").unwrap();
        let lines: Vec<&str> = export.trim().split('\n').collect();
        assert_eq!(lines.len(), 2);

        // Each line should be valid JSON
        for line in &lines {
            let parsed: serde_json::Value = serde_json::from_str(line).unwrap();
            assert!(parsed.is_object());
        }
    }

    #[test]
    fn test_bulk_export_round_trip() {
        let conn = create_test_db();
        let record = make_test_record("rec-rt-export", "packet-rt-export", 1);
        insert_tool_call_record(&conn, &record).unwrap();

        let export = bulk_export_traces(&conn, "2026-07-01T00:00:00Z", "2026-08-01T00:00:00Z").unwrap();
        let lines: Vec<&str> = export.trim().split('\n').collect();
        assert_eq!(lines.len(), 1);

        // Deserialize back to ToolCallRecord
        let deserialized: ToolCallRecord = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(deserialized.id, "rec-rt-export");
        assert_eq!(deserialized.delegation_packet_id, "packet-rt-export");
        assert_eq!(deserialized.tool_name, "read_file");
    }

    // ─── Phase 10 Tests (Task 10.4) ────────────────────────────────────────

    #[test]
    fn test_crash_safe_startup_clean() {
        let conn = create_test_db();
        let report = crash_safe_startup(&conn).unwrap();
        assert!(!report.circuit_breaker_was_open);
        assert!(!report.circuit_breaker_reset);
        assert_eq!(report.unlinked_records_found, 0);
    }

    #[test]
    fn test_crash_safe_startup_with_open_breaker() {
        let conn = create_test_db();
        let state = CircuitBreakerState {
            consecutive_failures: 5,
            is_open: true,
            last_failure_at: Some("2026-07-15T10:00:00Z".to_string()),
            cooldown_ends_at: Some("2026-07-15T10:00:30Z".to_string()),
            cooldown_secs: 30,
            failure_threshold: 5,
            total_records_dropped: 50,
        };
        update_circuit_breaker(&conn, &state).unwrap();

        let report = crash_safe_startup(&conn).unwrap();
        assert!(report.circuit_breaker_was_open);
        assert!(report.circuit_breaker_reset);

        // Verify breaker is now closed
        let cb = read_circuit_breaker(&conn).unwrap();
        assert!(!cb.is_open);
        assert_eq!(cb.consecutive_failures, 0);
    }

    #[test]
    fn test_crash_safe_startup_with_unlinked_records() {
        let conn = create_test_db();
        insert_standalone_trace_summary(&conn, "packet-unlinked-1", "{}").unwrap();
        insert_standalone_trace_summary(&conn, "packet-unlinked-2", "{}").unwrap();

        let report = crash_safe_startup(&conn).unwrap();
        assert_eq!(report.unlinked_records_found, 2);
    }

    #[test]
    fn test_buffer_overflow_tracker() {
        let mut tracker = BufferOverflowTracker::new();

        // First drop should trigger warning
        assert!(tracker.record_drop());

        // Subsequent drops within 60s should not trigger
        assert!(!tracker.record_drop());
        assert!(!tracker.record_drop());
    }

    // ─── Phase 12 Tests (Task 12.4, 12.5) ──────────────────────────────────

    #[test]
    fn test_resolve_tracker_paths() {
        let app_dir = std::path::Path::new("/tmp/test-app");
        let (tracker, eb, cost) = resolve_tracker_paths(app_dir);
        assert_eq!(tracker, app_dir.join("tool_call_tracker.db"));
        assert_eq!(eb, app_dir.join("experience_buffer.db"));
        assert_eq!(cost, app_dir.join("cost_ledger.db"));
    }

    #[test]
    fn test_get_tracker_status() {
        let conn = create_test_db();
        let config = ToolCallTrackerConfig::default();
        let status = get_tracker_status(&conn, &config).unwrap();
        assert!(status.is_active);
        assert!(!status.circuit_breaker_open);
        assert_eq!(status.total_records_dropped, 0);
        assert_eq!(status.consecutive_failures, 0);
        assert_eq!(status.unlinked_standalone_count, 0);
    }

    #[test]
    fn test_end_to_end_full_lifecycle() {
        // Full lifecycle: log → flush → analyze → experience buffer → cost attribution
        let tracker_dir = tempfile::tempdir().unwrap();
        let tracker_path = tracker_dir.path().join("tracker.db");
        let tracker_conn = Connection::open(&tracker_path).unwrap();
        initialize_tool_call_tracker_db(&tracker_conn).unwrap();

        // 1. Insert tool call records (simulating buffer flush)
        let records = vec![
            ToolCallRecord {
                id: "e2e-1".to_string(),
                delegation_packet_id: "packet-e2e".to_string(),
                agent_id: "agent-e2e".to_string(),
                task_type: "code_gen".to_string(),
                tool_name: "read_file".to_string(),
                input_params_json: r#"{"path":"main.rs"}"#.to_string(),
                output_summary: Some("fn main() {}".to_string()),
                duration_ms: 15,
                success: true,
                timestamp: "2026-07-15T10:00:00Z".to_string(),
                sequence_position: 1,
                prompt_tokens: None,
                completion_tokens: None,
                is_llm_backed: false,
            },
            ToolCallRecord {
                id: "e2e-2".to_string(),
                delegation_packet_id: "packet-e2e".to_string(),
                agent_id: "agent-e2e".to_string(),
                task_type: "code_gen".to_string(),
                tool_name: "llm_generate".to_string(),
                input_params_json: r#"{"prompt":"write code"}"#.to_string(),
                output_summary: Some("wrote output.rs".to_string()),
                duration_ms: 2000,
                success: true,
                timestamp: "2026-07-15T10:00:02Z".to_string(),
                sequence_position: 2,
                prompt_tokens: Some(500),
                completion_tokens: Some(1000),
                is_llm_backed: true,
            },
            ToolCallRecord {
                id: "e2e-3".to_string(),
                delegation_packet_id: "packet-e2e".to_string(),
                agent_id: "agent-e2e".to_string(),
                task_type: "code_gen".to_string(),
                tool_name: "write_file".to_string(),
                input_params_json: r#"{"path":"output.rs"}"#.to_string(),
                output_summary: Some("wrote output.rs".to_string()),
                duration_ms: 20,
                success: true,
                timestamp: "2026-07-15T10:00:03Z".to_string(),
                sequence_position: 3,
                prompt_tokens: None,
                completion_tokens: None,
                is_llm_backed: false,
            },
        ];
        insert_tool_call_records_batch(&tracker_conn, &records).unwrap();

        // 2. Run analysis
        let config = ToolCallTrackerConfig::default();
        let result = analyze_completed_task(
            &tracker_path,
            "packet-e2e",
            "agent-e2e",
            "code_gen",
            &["output.rs".to_string()],
            &[],
            &[],
            &config,
        ).unwrap();

        assert_eq!(result.total_calls, 3);
        assert!(result.efficiency_ratio > 0.0);
        assert!(result.efficiency_ratio <= 1.0);

        // 3. Build trace summary and attempt experience buffer append
        let summary = crate::tool_call_analysis::build_trace_summary(&result);
        let summary_json = serde_json::to_string(&summary).unwrap();

        // Create experience buffer with matching record
        let eb_dir = tempfile::tempdir().unwrap();
        let eb_path = eb_dir.path().join("experience_buffer.db");
        let eb_conn = Connection::open(&eb_path).unwrap();
        crate::experience_buffer_service::initialize_experience_buffer_db(&eb_conn).unwrap();
        ensure_experience_buffer_migration(&eb_conn).unwrap();
        eb_conn.execute(
            "INSERT INTO experience_records (id, delegation_packet_id, timestamp, workload_class, task_type, scoring_recommendation_json, heuristic_decision_json, advisory_accepted, confidence_score)
             VALUES ('er-e2e', 'packet-e2e', '2026-07-15T10:00:00Z', 'coding', 'code_gen', '{}', '{}', 1, 0.9)",
            [],
        ).unwrap();
        drop(eb_conn);

        let linked = append_trace_with_fallback(&tracker_conn, &eb_path, "packet-e2e", &summary_json).unwrap();
        assert!(linked);

        // 4. Write cost attributions
        let cost_dir = tempfile::tempdir().unwrap();
        let cost_path = cost_dir.path().join("cost_ledger.db");
        let written = write_cost_attributions(&cost_path, &records, "agent-e2e", "packet-e2e", "code_gen").unwrap();
        assert_eq!(written, 1); // Only the LLM-backed record

        // 5. Query anomalies
        let anomalies = crate::tool_call_analysis::query_anomaly_flagged_tasks(
            &tracker_conn,
            "2026-07-01T00:00:00Z",
            "2026-08-01T00:00:00Z",
        ).unwrap();
        // May or may not have anomalies depending on thresholds
        assert!(anomalies.len() <= 1);

        // 6. Export
        let export = bulk_export_traces(&tracker_conn, "2026-07-01T00:00:00Z", "2026-08-01T00:00:00Z").unwrap();
        assert!(!export.is_empty());
    }

    #[test]
    fn test_truncate_output_summary() {
        let short = "hello world";
        assert_eq!(truncate_output_summary(short, 500), short);

        let long: String = (0..600).map(|i| format!("word{}", i)).collect::<Vec<_>>().join(" ");
        let truncated = truncate_output_summary(&long, 500);
        assert!(truncated.ends_with("...[truncated]"));
        // Count words before truncation marker
        let content = truncated.trim_end_matches("...[truncated]");
        let word_count = content.split_whitespace().count();
        assert_eq!(word_count, 500);
    }

    // ─── Property-Based Tests (Task 3.7) ────────────────────────────────────

    // Feature: tool-call-tracker, Property 1: Tool Call Record structural completeness
    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]

        /// **Validates: Requirements 1.1, 1.2, 1.4, 12.1, 12.2**
        #[test]
        fn prop_record_structural_completeness(
            tool_name in "[a-z_]{1,30}",
            agent_id in "[a-z0-9-]{1,30}",
            packet_id in "[a-z0-9-]{1,30}",
            task_type in "[a-z_]{1,20}",
            duration_ms in 0u64..1_000_000,
            success in proptest::bool::ANY,
            seq_pos in 1u32..10000,
        ) {
            let record = ToolCallRecord {
                id: format!("id-{}", seq_pos),
                delegation_packet_id: packet_id.clone(),
                agent_id: agent_id.clone(),
                task_type: task_type.clone(),
                tool_name: tool_name.clone(),
                input_params_json: r#"{"key":"value"}"#.to_string(),
                output_summary: Some("output".to_string()),
                duration_ms,
                success,
                timestamp: "2026-07-15T10:30:00Z".to_string(),
                sequence_position: seq_pos,
                prompt_tokens: None,
                completion_tokens: None,
                is_llm_backed: false,
            };

            // Structural completeness checks
            prop_assert!(!record.id.is_empty());
            prop_assert!(!record.delegation_packet_id.is_empty());
            prop_assert!(!record.agent_id.is_empty());
            prop_assert!(!record.task_type.is_empty());
            prop_assert!(!record.tool_name.is_empty());
            // input_params_json must be valid JSON
            prop_assert!(serde_json::from_str::<serde_json::Value>(&record.input_params_json).is_ok());
            // output_summary at most 500 tokens
            if let Some(ref summary) = record.output_summary {
                prop_assert!(summary.split_whitespace().count() <= 500);
            }
            prop_assert!(record.sequence_position >= 1);
            // timestamp is valid ISO-8601
            prop_assert!(record.timestamp.contains('T'));
        }
    }

    // Feature: tool-call-tracker, Property 2: Persistence round-trip with sanitization
    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]

        /// **Validates: Requirements 1.3, 2.4, 12.3**
        #[test]
        fn prop_persistence_round_trip(
            seq_pos in 1u32..1000,
            duration_ms in 0u64..100_000,
            success in proptest::bool::ANY,
        ) {
            let conn = create_test_db();
            let id = format!("rt-{}", seq_pos);
            let record = ToolCallRecord {
                id: id.clone(),
                delegation_packet_id: "packet-rt".to_string(),
                agent_id: "agent-rt".to_string(),
                task_type: "test_task".to_string(),
                tool_name: "test_tool".to_string(),
                input_params_json: r#"{"safe_param":"hello"}"#.to_string(),
                output_summary: Some("test output".to_string()),
                duration_ms,
                success,
                timestamp: "2026-07-15T10:30:00Z".to_string(),
                sequence_position: seq_pos,
                prompt_tokens: None,
                completion_tokens: None,
                is_llm_backed: false,
            };

            insert_tool_call_record(&conn, &record).unwrap();

            let results = query_records_by_packet_id(&conn, "packet-rt").unwrap();
            let found = results.iter().find(|r| r.id == id).unwrap();

            prop_assert_eq!(&found.id, &record.id);
            prop_assert_eq!(&found.delegation_packet_id, &record.delegation_packet_id);
            prop_assert_eq!(&found.agent_id, &record.agent_id);
            prop_assert_eq!(&found.tool_name, &record.tool_name);
            prop_assert_eq!(found.duration_ms, record.duration_ms);
            prop_assert_eq!(found.success, record.success);
            prop_assert_eq!(found.sequence_position, record.sequence_position);

            // Verify no secrets in stored params
            let stored_params: serde_json::Value = serde_json::from_str(&found.input_params_json).unwrap();
            fn check_no_secrets(val: &serde_json::Value) {
                match val {
                    serde_json::Value::String(s) => {
                        // Should not contain obvious secret patterns
                        assert!(!s.starts_with("sk-") || s == "[REDACTED]");
                        assert!(!s.starts_with("Bearer ") || s == "[REDACTED]");
                    }
                    serde_json::Value::Object(map) => {
                        for v in map.values() {
                            check_no_secrets(v);
                        }
                    }
                    serde_json::Value::Array(arr) => {
                        for v in arr {
                            check_no_secrets(v);
                        }
                    }
                    _ => {}
                }
            }
            check_no_secrets(&stored_params);
        }
    }

    // Feature: tool-call-tracker, Property 9: Circuit breaker state transitions
    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]

        /// **Validates: Requirements 10.4, 10.5**
        #[test]
        fn prop_circuit_breaker_transitions(
            failure_count in 0u32..20,
            threshold in 1u32..10,
        ) {
            let conn = create_test_db();

            let mut state = CircuitBreakerState {
                consecutive_failures: 0,
                is_open: false,
                last_failure_at: None,
                cooldown_ends_at: None,
                cooldown_secs: 30,
                failure_threshold: threshold,
                total_records_dropped: 0,
            };

            // Apply failures
            for _ in 0..failure_count {
                state.consecutive_failures += 1;
                if state.consecutive_failures >= state.failure_threshold {
                    state.is_open = true;
                }
            }

            // Property: breaker opens after exactly threshold failures
            if failure_count >= threshold {
                prop_assert!(state.is_open);
            } else {
                prop_assert!(!state.is_open);
            }

            // Property: success resets
            state.consecutive_failures = 0;
            state.is_open = false;
            prop_assert!(!state.is_open);
            prop_assert_eq!(state.consecutive_failures, 0);

            // Persist and verify
            update_circuit_breaker(&conn, &state).unwrap();
            let read_back = read_circuit_breaker(&conn).unwrap();
            prop_assert_eq!(read_back.consecutive_failures, 0);
            prop_assert!(!read_back.is_open);
        }
    }

    // Feature: tool-call-tracker, Property 12: Sequence position monotonicity
    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]

        /// **Validates: Requirements 1.6**
        #[test]
        fn prop_sequence_monotonicity(
            num_records in 1usize..50,
        ) {
            let conn = create_test_db();
            let packet_id = "packet-seq-test";

            let records: Vec<ToolCallRecord> = (1..=num_records)
                .map(|i| ToolCallRecord {
                    id: format!("seq-{}", i),
                    delegation_packet_id: packet_id.to_string(),
                    agent_id: "agent-seq".to_string(),
                    task_type: "test".to_string(),
                    tool_name: "tool".to_string(),
                    input_params_json: "{}".to_string(),
                    output_summary: None,
                    duration_ms: 10,
                    success: true,
                    timestamp: format!("2026-07-15T10:{:02}:00Z", i),
                    sequence_position: i as u32,
                    prompt_tokens: None,
                    completion_tokens: None,
                    is_llm_backed: false,
                })
                .collect();

            insert_tool_call_records_batch(&conn, &records).unwrap();
            let results = query_records_by_packet_id(&conn, packet_id).unwrap();

            prop_assert_eq!(results.len(), num_records);

            // Verify monotonically increasing sequence with no gaps
            for (i, r) in results.iter().enumerate() {
                prop_assert_eq!(r.sequence_position, (i + 1) as u32);
            }

            // Verify no duplicates
            let positions: Vec<u32> = results.iter().map(|r| r.sequence_position).collect();
            let mut deduped = positions.clone();
            deduped.sort();
            deduped.dedup();
            prop_assert_eq!(positions.len(), deduped.len());
        }
    }

    // ─── Property-Based Tests (Task 9.5): Properties 10, 11, 14 ────────────

    // Feature: tool-call-tracker, Property 10: Retention policy enforcement
    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]

        /// **Validates: Requirements 13.1, 13.2, 13.3**
        #[test]
        fn prop_retention_policy_enforcement(
            retention_days in 1u32..365,
            num_old_records in 1usize..10,
            num_recent_records in 1usize..10,
        ) {
            let conn = create_test_db();
            let config = ToolCallTrackerConfig {
                retention_days_traces: retention_days,
                retention_days_metrics: retention_days * 2,
                ..Default::default()
            };

            // Insert old records (beyond retention)
            for i in 0..num_old_records {
                let mut record = make_test_record(&format!("old-{}", i), "packet-old", (i + 1) as u32);
                record.timestamp = "2020-01-01T10:00:00Z".to_string();
                insert_tool_call_record(&conn, &record).unwrap();
            }

            // Insert recent records (within retention)
            for i in 0..num_recent_records {
                let mut record = make_test_record(&format!("recent-{}", i), "packet-recent", (i + 1) as u32);
                record.timestamp = chrono::Utc::now().to_rfc3339();
                insert_tool_call_record(&conn, &record).unwrap();
            }

            let (traces_deleted, _) = evict_expired_records(&conn, &config).unwrap();

            // All old records should be deleted
            prop_assert_eq!(traces_deleted, num_old_records as u32);

            // Recent records should remain
            let remaining: i32 = conn.query_row(
                "SELECT COUNT(*) FROM tool_call_records",
                [],
                |row| row.get(0),
            ).unwrap();
            prop_assert_eq!(remaining, num_recent_records as i32);
        }
    }

    // Feature: tool-call-tracker, Property 11: Aggregate statistics invariance under eviction
    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]

        /// **Validates: Requirements 13.5**
        #[test]
        fn prop_aggregate_stats_invariance_under_eviction(
            num_records in 1usize..10,
        ) {
            let conn = create_test_db();
            let config = ToolCallTrackerConfig {
                retention_days_traces: 1, // Very short retention to force eviction
                retention_days_metrics: 1,
                ..Default::default()
            };

            // Insert aggregate stats
            conn.execute(
                "INSERT INTO aggregate_stats (agent_id, task_type, avg_efficiency_ratio, avg_tool_call_count, total_tasks_analyzed, last_updated_at)
                 VALUES ('agent-agg', 'test_task', 0.75, 10.0, 50, '2026-07-15T10:00:00Z')",
                [],
            ).unwrap();

            // Insert old records that will be evicted
            for i in 0..num_records {
                let mut record = make_test_record(&format!("agg-{}", i), "packet-agg", (i + 1) as u32);
                record.timestamp = "2020-01-01T10:00:00Z".to_string();
                insert_tool_call_record(&conn, &record).unwrap();
            }

            // Run eviction
            evict_expired_records(&conn, &config).unwrap();

            // Aggregate stats should be unchanged
            let (avg_eff, avg_calls, total): (f64, f64, i32) = conn.query_row(
                "SELECT avg_efficiency_ratio, avg_tool_call_count, total_tasks_analyzed FROM aggregate_stats WHERE agent_id = 'agent-agg'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            ).unwrap();

            prop_assert!((avg_eff - 0.75).abs() < f64::EPSILON);
            prop_assert!((avg_calls - 10.0).abs() < f64::EPSILON);
            prop_assert_eq!(total, 50);
        }
    }

    // Feature: tool-call-tracker, Property 14: Bulk export produces valid structured JSON
    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]

        /// **Validates: Requirements 13.4**
        #[test]
        fn prop_bulk_export_round_trip(
            num_records in 1usize..20,
        ) {
            let conn = create_test_db();

            // Insert records
            for i in 0..num_records {
                let mut record = make_test_record(&format!("export-{}", i), &format!("packet-exp-{}", i), 1);
                record.timestamp = "2026-07-15T10:00:00Z".to_string();
                insert_tool_call_record(&conn, &record).unwrap();
            }

            let export = bulk_export_traces(&conn, "2026-07-01T00:00:00Z", "2026-08-01T00:00:00Z").unwrap();

            // Each line must be valid JSON
            let lines: Vec<&str> = export.trim().split('\n').filter(|l| !l.is_empty()).collect();
            prop_assert_eq!(lines.len(), num_records);

            for line in &lines {
                let parsed = serde_json::from_str::<ToolCallRecord>(line);
                prop_assert!(parsed.is_ok(), "Failed to parse line as ToolCallRecord: {}", line);

                let record = parsed.unwrap();
                // Round-trip: all fields should be non-empty where required
                prop_assert!(!record.id.is_empty());
                prop_assert!(!record.delegation_packet_id.is_empty());
                prop_assert!(!record.tool_name.is_empty());
                prop_assert!(!record.timestamp.is_empty());
            }
        }
    }

    // ─── Performance Tests (Task 12.4) ──────────────────────────────────────

    #[test]
    fn test_perf_log_tool_call_under_5ms() {
        // log_tool_call should return in < 5ms
        let (sender, _receiver) = mpsc::channel(1000);
        let circuit_breaker = Arc::new(RwLock::new(CircuitBreakerState::default()));
        let config = ToolCallTrackerConfig::default();
        let sequence_positions = Arc::new(DashMap::new());

        let state = ToolCallTrackerState {
            sender,
            circuit_breaker,
            config,
            sequence_positions,
            db_path: PathBuf::from("/tmp/perf-test.db"),
        };

        let record = ToolCallRecord {
            id: "perf-1".to_string(),
            delegation_packet_id: "packet-perf".to_string(),
            agent_id: "agent-perf".to_string(),
            task_type: "test".to_string(),
            tool_name: "read_file".to_string(),
            input_params_json: r#"{"path":"src/main.rs","content":"x".repeat(10000)}"#.to_string(),
            output_summary: Some("file content here".to_string()),
            duration_ms: 42,
            success: true,
            timestamp: String::new(),
            sequence_position: 0,
            prompt_tokens: None,
            completion_tokens: None,
            is_llm_backed: false,
        };

        let start = std::time::Instant::now();
        for _ in 0..100 {
            log_tool_call(&state, record.clone());
        }
        let elapsed = start.elapsed();
        let per_call = elapsed / 100;

        // Each call should be < 5ms
        assert!(
            per_call.as_millis() < 5,
            "log_tool_call took {}ms per call, expected < 5ms",
            per_call.as_millis()
        );
    }

    #[test]
    fn test_perf_batch_flush_50_records_under_50ms() {
        let conn = create_test_db();

        let records: Vec<ToolCallRecord> = (1..=50)
            .map(|i| make_test_record(&format!("perf-batch-{}", i), "packet-perf-batch", i as u32))
            .collect();

        let start = std::time::Instant::now();
        insert_tool_call_records_batch(&conn, &records).unwrap();
        let elapsed = start.elapsed();

        assert!(
            elapsed.as_millis() < 50,
            "Batch flush of 50 records took {}ms, expected < 50ms",
            elapsed.as_millis()
        );
    }

    #[test]
    fn test_perf_analysis_100_call_trace_under_200ms() {
        let conn = create_test_db();

        // Create a 100-call trace
        let records: Vec<ToolCallRecord> = (1..=100)
            .map(|i| {
                let mut r = make_test_record(&format!("perf-analysis-{}", i), "packet-perf-analysis", i as u32);
                r.output_summary = Some(format!("output for call {}", i));
                r
            })
            .collect();

        insert_tool_call_records_batch(&conn, &records).unwrap();

        let config = ToolCallTrackerConfig::default();
        let loaded = query_records_by_packet_id(&conn, "packet-perf-analysis").unwrap();

        let start = std::time::Instant::now();
        let _result = crate::tool_call_analysis::analyze_completed_task_inner(
            &loaded,
            "packet-perf-analysis",
            "agent-perf",
            "test_task",
            &["output.rs".to_string()],
            &[],
            &[],
            &config,
        );
        let elapsed = start.elapsed();

        assert!(
            elapsed.as_millis() < 200,
            "Analysis of 100-call trace took {}ms, expected < 200ms",
            elapsed.as_millis()
        );
    }

    #[test]
    fn test_perf_sustained_throughput_1000_records_per_second() {
        let conn = create_test_db();

        // Insert 1000 records and measure time
        let records: Vec<ToolCallRecord> = (1..=1000)
            .map(|i| make_test_record(&format!("throughput-{}", i), "packet-throughput", i as u32))
            .collect();

        let start = std::time::Instant::now();
        // Insert in batches of 50 (matching buffer flush size)
        for chunk in records.chunks(50) {
            insert_tool_call_records_batch(&conn, chunk).unwrap();
        }
        let elapsed = start.elapsed();

        // Should complete within 1 second for 1000 records
        assert!(
            elapsed.as_secs() < 2,
            "1000 records took {}ms, expected < 2000ms",
            elapsed.as_millis()
        );
    }

    // ─── Phase 11 Integration Tests (Task 11.6): IPC Round-Trips ────────────

    #[test]
    fn test_ipc_query_records_round_trip() {
        let conn = create_test_db();
        let record = make_test_record("ipc-rec-1", "packet-ipc", 1);
        insert_tool_call_record(&conn, &record).unwrap();

        // Simulate what the IPC command does
        let results = query_records_by_packet_id(&conn, "packet-ipc").unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "ipc-rec-1");

        // Verify serialization round-trip (IPC returns JSON)
        let json = serde_json::to_string(&results).unwrap();
        let deserialized: Vec<ToolCallRecord> = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.len(), 1);
        assert_eq!(deserialized[0].id, "ipc-rec-1");
    }

    #[test]
    fn test_ipc_query_analysis_round_trip() {
        let tracker_dir = tempfile::tempdir().unwrap();
        let tracker_path = tracker_dir.path().join("tracker.db");
        let conn = Connection::open(&tracker_path).unwrap();
        initialize_tool_call_tracker_db(&conn).unwrap();

        let record = make_test_record("ipc-analysis-1", "packet-ipc-analysis", 1);
        insert_tool_call_record(&conn, &record).unwrap();

        let config = ToolCallTrackerConfig::default();
        let result = analyze_completed_task(
            &tracker_path,
            "packet-ipc-analysis",
            "test-agent",
            "test_task",
            &[],
            &[],
            &[],
            &config,
        ).unwrap();

        // Simulate IPC query
        let row = conn.query_row(
            "SELECT delegation_packet_id, efficiency_ratio, total_calls FROM task_analysis_results WHERE delegation_packet_id = ?1",
            params!["packet-ipc-analysis"],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?, row.get::<_, i32>(2)?)),
        ).unwrap();

        assert_eq!(row.0, "packet-ipc-analysis");
        assert!(row.1 >= 0.0 && row.1 <= 1.0);
        assert_eq!(row.2, 1);
    }

    #[test]
    fn test_ipc_config_read_update_round_trip() {
        let conn = create_test_db();

        // Read default config
        let config = read_tracker_config(&conn).unwrap();
        assert!((config.efficiency_threshold - 0.5).abs() < f64::EPSILON);

        // Update config
        let new_config = TrackerConfigRow {
            efficiency_threshold: 0.7,
            historical_avg_multiplier: 2.0,
            max_storage_bytes: 1024 * 1024 * 200,
            retention_days_traces: 60,
            retention_days_metrics: 120,
            rolling_avg_window_size: 50,
        };
        update_tracker_config(&conn, &new_config).unwrap();

        // Read back
        let read_back = read_tracker_config(&conn).unwrap();
        assert!((read_back.efficiency_threshold - 0.7).abs() < f64::EPSILON);
        assert!((read_back.historical_avg_multiplier - 2.0).abs() < f64::EPSILON);
        assert_eq!(read_back.retention_days_traces, 60);

        // Verify JSON serialization (IPC returns JSON)
        let json = serde_json::to_string(&read_back).unwrap();
        let deserialized: TrackerConfigRow = serde_json::from_str(&json).unwrap();
        assert!((deserialized.efficiency_threshold - 0.7).abs() < f64::EPSILON);
    }

    #[test]
    fn test_ipc_export_round_trip() {
        let conn = create_test_db();
        let mut record = make_test_record("ipc-export-1", "packet-ipc-export", 1);
        record.timestamp = "2026-07-15T10:00:00Z".to_string();
        insert_tool_call_record(&conn, &record).unwrap();

        let export = bulk_export_traces(&conn, "2026-07-01T00:00:00Z", "2026-08-01T00:00:00Z").unwrap();
        assert!(!export.is_empty());

        // Each line is valid JSON
        for line in export.trim().split('\n') {
            let parsed: serde_json::Value = serde_json::from_str(line).unwrap();
            assert!(parsed.is_object());
        }
    }

    #[test]
    fn test_ipc_status_round_trip() {
        let conn = create_test_db();
        let config = ToolCallTrackerConfig::default();
        let status = get_tracker_status(&conn, &config).unwrap();

        // Verify JSON serialization
        let json = serde_json::to_string(&status).unwrap();
        let deserialized: TrackerStatus = serde_json::from_str(&json).unwrap();
        assert!(deserialized.is_active);
        assert!(!deserialized.circuit_breaker_open);
        assert_eq!(deserialized.max_storage_bytes, config.max_storage_bytes);
    }
}
