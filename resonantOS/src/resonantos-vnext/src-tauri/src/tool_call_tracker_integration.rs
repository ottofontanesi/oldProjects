//! Tool Call Tracker Integration
//!
//! Phase 8-12: Experience Buffer integration, Cost Ledger integration,
//! retention/eviction, graceful degradation, IPC commands, and performance hooks.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use chrono::Utc;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tokio::time::{self, Duration};

use crate::cost_ledger_service::{estimate_cost_usd, initialize_cost_ledger_db, record_cost_entry, CostRecord};
use crate::tool_call_analysis::{
    build_trace_summary, AnalysisResult, AnomalyFlag, SequencePattern, TaskAnalysisResult,
    ToolCallTraceSummary,
};
use crate::tool_call_tracker_service::{
    initialize_tool_call_tracker_db, query_records_by_packet_id, query_records_by_time_range,
    read_circuit_breaker, read_tracker_config, reset_circuit_breaker, update_circuit_breaker,
    update_tracker_config, CircuitBreakerState, ToolCallRecord, ToolCallTrackerConfig,
    ToolCallTrackerState, TrackerConfigRow,
};

// ─── Phase 8: Experience Buffer Schema Migration ────────────────────────────

/// Idempotent schema migration: adds tool_call_trace_json column to experience_records
/// if it doesn't already exist.
pub fn migrate_experience_buffer_schema(conn: &Connection) -> Result<(), String> {
    // Check if column already exists by querying table_info
    let has_column: bool = conn
        .prepare("PRAGMA table_info(experience_records)")
        .map_err(|e| format!("Failed to prepare PRAGMA: {}", e))?
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|e| format!("Failed to query table_info: {}", e))?
        .any(|col| col.map(|c| c == "tool_call_trace_json").unwrap_or(false));

    if !has_column {
        conn.execute(
            "ALTER TABLE experience_records ADD COLUMN tool_call_trace_json TEXT",
            [],
        )
        .map_err(|e| format!("Failed to add tool_call_trace_json column: {}", e))?;
    }

    Ok(())
}
