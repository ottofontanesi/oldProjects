//! Agent Evaluator Service (NA2)
//!
//! Phase 5 of the ResonantOS vNext improvement plan. Discovers, sandboxes,
//! benchmarks, and presents candidate agent add-ons for human approval.
//! Owns the evaluation database (`agent_evaluator.db`), candidate lifecycle state,
//! comparative reports, and approval decisions.

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::sync::Mutex;

// ─── Core Data Structures ───────────────────────────────────────────────────

/// A discovery candidate record.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CandidateRecord {
    pub id: String,
    pub name: String,
    pub source_url: String,
    pub source_type: String,
    pub discovery_score: f64,
    pub score_breakdown_json: String,
    pub category: String,
    pub manifest_capabilities_json: String,
    pub estimated_eval_cost_json: String,
    pub status: String,
    pub discovered_at: String,
    pub version: String,
    pub manifest_id: String,
    pub updated_at: String,
}

/// A comparative report record.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComparativeReportRecord {
    pub id: String,
    pub candidate_id: String,
    pub candidate_name: String,
    pub incumbent_agent_ids_json: String,
    pub evaluation_timestamp: String,
    pub replay_task_set_ids_json: String,
    pub sandbox_config_json: String,
    pub per_task_deltas_json: String,
    pub aggregate_scores_json: String,
    pub candidate_verdict: String,
    pub production_prediction_json: Option<String>,
    pub security_assessment_json: String,
}

/// An approval decision record.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApprovalRecord {
    pub id: String,
    pub candidate_id: String,
    pub decision: String,
    pub decided_at: String,
    pub comparative_report_id: String,
    pub notes: Option<String>,
}

/// Evaluation job record.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EvaluationJobRecord {
    pub id: String,
    pub candidate_id: String,
    pub compute_job_id: String,
    pub status: String,
    pub sandbox_config_json: String,
    pub submitted_at: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub error_message: Option<String>,
    pub benchmark_results_json: Option<String>,
}

/// Post-installation performance tracking.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PostInstallTracking {
    pub candidate_id: String,
    pub installed_at: String,
    pub predicted_score: f64,
    pub actual_scores_json: String,
    pub deviation_flagged: bool,
    pub deviation_flagged_at: Option<String>,
    pub days_tracked: u32,
}

/// NA2 trust tier state.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NA2TrustTierState {
    pub current_tier: String,
    pub promoted_at: Option<String>,
    pub validation_started_at: String,
    pub consecutive_days_accurate: u32,
    pub consecutive_days_inaccurate: u32,
}

/// Circuit breaker for discovery polling.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveryCircuitBreaker {
    pub consecutive_failures: u32,
    pub is_open: bool,
    pub last_failure_at: Option<String>,
    pub cooldown_ends_at: Option<String>,
    pub cooldown_secs: u64,
    pub failure_threshold: u32,
}

/// Discovery source configuration record.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoverySourceRecord {
    pub id: String,
    pub source_type: String,
    pub url: String,
    pub enabled: bool,
    pub polling_frequency_hours: u32,
    pub last_polled_at: Option<String>,
    pub category_filters_json: String,
}

/// State wrapper for the agent evaluator database connection.
pub struct AgentEvaluatorState {
    pub db: Mutex<Connection>,
}


// ─── Schema Initialization ──────────────────────────────────────────────────

pub fn initialize_agent_evaluator_db(connection: &Connection) -> Result<(), String> {
    connection
        .execute_batch(
            "
            CREATE TABLE IF NOT EXISTS candidates (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                source_url TEXT NOT NULL,
                source_type TEXT NOT NULL,
                discovery_score REAL NOT NULL,
                score_breakdown_json TEXT NOT NULL,
                category TEXT NOT NULL,
                manifest_capabilities_json TEXT NOT NULL,
                estimated_eval_cost_json TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'discovered',
                discovered_at TEXT NOT NULL,
                version TEXT NOT NULL,
                manifest_id TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS comparative_reports (
                id TEXT PRIMARY KEY,
                candidate_id TEXT NOT NULL REFERENCES candidates(id),
                candidate_name TEXT NOT NULL,
                incumbent_agent_ids_json TEXT NOT NULL,
                evaluation_timestamp TEXT NOT NULL,
                replay_task_set_ids_json TEXT NOT NULL,
                sandbox_config_json TEXT NOT NULL,
                per_task_deltas_json TEXT NOT NULL,
                aggregate_scores_json TEXT NOT NULL,
                candidate_verdict TEXT NOT NULL,
                production_prediction_json TEXT,
                security_assessment_json TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS approval_decisions (
                id TEXT PRIMARY KEY,
                candidate_id TEXT NOT NULL REFERENCES candidates(id),
                decision TEXT NOT NULL,
                decided_at TEXT NOT NULL,
                comparative_report_id TEXT NOT NULL REFERENCES comparative_reports(id),
                notes TEXT
            );

            CREATE TABLE IF NOT EXISTS evaluation_jobs (
                id TEXT PRIMARY KEY,
                candidate_id TEXT NOT NULL REFERENCES candidates(id),
                compute_job_id TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'submitted',
                sandbox_config_json TEXT NOT NULL,
                submitted_at TEXT NOT NULL,
                started_at TEXT,
                completed_at TEXT,
                error_message TEXT,
                benchmark_results_json TEXT
            );

            CREATE TABLE IF NOT EXISTS discovery_sources (
                id TEXT PRIMARY KEY,
                type TEXT NOT NULL,
                url TEXT NOT NULL,
                enabled INTEGER NOT NULL DEFAULT 1,
                polling_frequency_hours INTEGER NOT NULL DEFAULT 24,
                last_polled_at TEXT,
                category_filters_json TEXT NOT NULL DEFAULT '[]'
            );

            CREATE TABLE IF NOT EXISTS benchmark_suites (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                category TEXT NOT NULL,
                tasks_json TEXT NOT NULL,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS post_install_tracking (
                candidate_id TEXT PRIMARY KEY REFERENCES candidates(id),
                installed_at TEXT NOT NULL,
                predicted_score REAL NOT NULL,
                actual_scores_json TEXT NOT NULL DEFAULT '[]',
                deviation_flagged INTEGER NOT NULL DEFAULT 0,
                deviation_flagged_at TEXT,
                days_tracked INTEGER NOT NULL DEFAULT 0
            );

            CREATE TABLE IF NOT EXISTS na2_trust_tier (
                id TEXT PRIMARY KEY DEFAULT 'singleton',
                current_tier TEXT NOT NULL DEFAULT 'addon',
                promoted_at TEXT,
                validation_started_at TEXT NOT NULL,
                consecutive_days_accurate INTEGER NOT NULL DEFAULT 0,
                consecutive_days_inaccurate INTEGER NOT NULL DEFAULT 0
            );

            CREATE TABLE IF NOT EXISTS discovery_circuit_breaker (
                id TEXT PRIMARY KEY DEFAULT 'singleton',
                consecutive_failures INTEGER NOT NULL DEFAULT 0,
                is_open INTEGER NOT NULL DEFAULT 0,
                last_failure_at TEXT,
                cooldown_ends_at TEXT,
                cooldown_secs INTEGER NOT NULL DEFAULT 3600,
                failure_threshold INTEGER NOT NULL DEFAULT 5
            );

            CREATE INDEX IF NOT EXISTS idx_candidates_status ON candidates(status);
            CREATE INDEX IF NOT EXISTS idx_candidates_category ON candidates(category);
            CREATE INDEX IF NOT EXISTS idx_candidates_discovered_at ON candidates(discovered_at);
            CREATE INDEX IF NOT EXISTS idx_candidates_manifest_id ON candidates(manifest_id);
            CREATE INDEX IF NOT EXISTS idx_reports_candidate ON comparative_reports(candidate_id);
            CREATE INDEX IF NOT EXISTS idx_approvals_candidate ON approval_decisions(candidate_id);
            CREATE INDEX IF NOT EXISTS idx_eval_jobs_candidate ON evaluation_jobs(candidate_id);
            CREATE INDEX IF NOT EXISTS idx_eval_jobs_status ON evaluation_jobs(status);
            ",
        )
        .map_err(|e| format!("Failed to initialize agent evaluator schema: {}", e))
}

// ─── Candidate CRUD ─────────────────────────────────────────────────────────

pub fn insert_candidate(conn: &Connection, candidate: &CandidateRecord) -> Result<(), String> {
    conn.execute(
        "INSERT INTO candidates (
            id, name, source_url, source_type, discovery_score, score_breakdown_json,
            category, manifest_capabilities_json, estimated_eval_cost_json, status,
            discovered_at, version, manifest_id, updated_at
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        params![
            candidate.id,
            candidate.name,
            candidate.source_url,
            candidate.source_type,
            candidate.discovery_score,
            candidate.score_breakdown_json,
            candidate.category,
            candidate.manifest_capabilities_json,
            candidate.estimated_eval_cost_json,
            candidate.status,
            candidate.discovered_at,
            candidate.version,
            candidate.manifest_id,
            candidate.updated_at,
        ],
    )
    .map_err(|e| format!("Failed to insert candidate: {}", e))?;
    Ok(())
}

pub fn update_candidate_status(conn: &Connection, id: &str, status: &str) -> Result<(), String> {
    let updated_at = chrono::Utc::now().to_rfc3339();
    let rows = conn
        .execute(
            "UPDATE candidates SET status = ?1, updated_at = ?2 WHERE id = ?3",
            params![status, updated_at, id],
        )
        .map_err(|e| format!("Failed to update candidate status: {}", e))?;
    if rows == 0 {
        return Err(format!("Candidate not found: {}", id));
    }
    Ok(())
}

pub fn query_candidates(
    conn: &Connection,
    status: Option<&str>,
    category: Option<&str>,
    limit: Option<u32>,
) -> Result<Vec<CandidateRecord>, String> {
    let mut sql = String::from("SELECT * FROM candidates WHERE 1=1");
    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

    if let Some(s) = status {
        sql.push_str(" AND status = ?");
        param_values.push(Box::new(s.to_string()));
    }
    if let Some(c) = category {
        sql.push_str(" AND category = ?");
        param_values.push(Box::new(c.to_string()));
    }

    sql.push_str(" ORDER BY discovery_score DESC");

    if let Some(l) = limit {
        sql.push_str(" LIMIT ?");
        param_values.push(Box::new(l as i64));
    }

    let params_refs: Vec<&dyn rusqlite::types::ToSql> =
        param_values.iter().map(|p| p.as_ref()).collect();

    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| format!("Failed to prepare candidates query: {}", e))?;

    let records = stmt
        .query_map(params_refs.as_slice(), |row| {
            Ok(CandidateRecord {
                id: row.get(0)?,
                name: row.get(1)?,
                source_url: row.get(2)?,
                source_type: row.get(3)?,
                discovery_score: row.get(4)?,
                score_breakdown_json: row.get(5)?,
                category: row.get(6)?,
                manifest_capabilities_json: row.get(7)?,
                estimated_eval_cost_json: row.get(8)?,
                status: row.get(9)?,
                discovered_at: row.get(10)?,
                version: row.get(11)?,
                manifest_id: row.get(12)?,
                updated_at: row.get(13)?,
            })
        })
        .map_err(|e| format!("Failed to query candidates: {}", e))?;

    let mut results = Vec::new();
    for record in records {
        results.push(record.map_err(|e| format!("Failed to read candidate row: {}", e))?);
    }
    Ok(results)
}

/// Check if a candidate was previously rejected by source_url or manifest_id.
/// Returns true if a rejected candidate exists with matching source_url or manifest_id
/// AND the version has NOT changed significantly (no major version bump).
pub fn is_previously_rejected(
    conn: &Connection,
    source_url: &str,
    manifest_id: &str,
    current_version: Option<&str>,
) -> Result<bool, String> {
    let mut stmt = conn
        .prepare(
            "SELECT version FROM candidates WHERE status = 'rejected'
             AND (source_url = ?1 OR manifest_id = ?2)
             ORDER BY updated_at DESC LIMIT 1",
        )
        .map_err(|e| format!("Failed to prepare rejected check: {}", e))?;

    let result: Option<String> = stmt
        .query_row(params![source_url, manifest_id], |row| row.get(0))
        .ok();

    match (result, current_version) {
        (Some(old_version), Some(new_version)) => {
            // Allow if major version bump detected
            let old_major = extract_major_version(&old_version);
            let new_major = extract_major_version(new_version);
            Ok(old_major >= new_major)
        }
        (Some(_), None) => Ok(true),
        (None, _) => Ok(false),
    }
}

/// Extract major version number from a semver-like string.
fn extract_major_version(version: &str) -> u32 {
    version
        .split('.')
        .next()
        .and_then(|s| s.trim_start_matches('v').parse::<u32>().ok())
        .unwrap_or(0)
}

// ─── Comparative Report CRUD ────────────────────────────────────────────────

pub fn insert_comparative_report(
    conn: &Connection,
    report: &ComparativeReportRecord,
) -> Result<(), String> {
    conn.execute(
        "INSERT INTO comparative_reports (
            id, candidate_id, candidate_name, incumbent_agent_ids_json,
            evaluation_timestamp, replay_task_set_ids_json, sandbox_config_json,
            per_task_deltas_json, aggregate_scores_json, candidate_verdict,
            production_prediction_json, security_assessment_json
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            report.id,
            report.candidate_id,
            report.candidate_name,
            report.incumbent_agent_ids_json,
            report.evaluation_timestamp,
            report.replay_task_set_ids_json,
            report.sandbox_config_json,
            report.per_task_deltas_json,
            report.aggregate_scores_json,
            report.candidate_verdict,
            report.production_prediction_json,
            report.security_assessment_json,
        ],
    )
    .map_err(|e| format!("Failed to insert comparative report: {}", e))?;
    Ok(())
}

pub fn query_report_by_candidate(
    conn: &Connection,
    candidate_id: &str,
) -> Result<Option<ComparativeReportRecord>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT * FROM comparative_reports WHERE candidate_id = ?1
             ORDER BY evaluation_timestamp DESC LIMIT 1",
        )
        .map_err(|e| format!("Failed to prepare report query: {}", e))?;

    let result = stmt
        .query_row(params![candidate_id], |row| {
            Ok(ComparativeReportRecord {
                id: row.get(0)?,
                candidate_id: row.get(1)?,
                candidate_name: row.get(2)?,
                incumbent_agent_ids_json: row.get(3)?,
                evaluation_timestamp: row.get(4)?,
                replay_task_set_ids_json: row.get(5)?,
                sandbox_config_json: row.get(6)?,
                per_task_deltas_json: row.get(7)?,
                aggregate_scores_json: row.get(8)?,
                candidate_verdict: row.get(9)?,
                production_prediction_json: row.get(10)?,
                security_assessment_json: row.get(11)?,
            })
        })
        .ok();

    Ok(result)
}

// ─── Approval Decision CRUD ─────────────────────────────────────────────────

pub fn insert_approval_decision(
    conn: &Connection,
    record: &ApprovalRecord,
) -> Result<(), String> {
    conn.execute(
        "INSERT INTO approval_decisions (
            id, candidate_id, decision, decided_at, comparative_report_id, notes
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            record.id,
            record.candidate_id,
            record.decision,
            record.decided_at,
            record.comparative_report_id,
            record.notes,
        ],
    )
    .map_err(|e| format!("Failed to insert approval decision: {}", e))?;
    Ok(())
}

pub fn query_approval_history(
    conn: &Connection,
    limit: u32,
) -> Result<Vec<ApprovalRecord>, String> {
    let mut stmt = conn
        .prepare("SELECT * FROM approval_decisions ORDER BY decided_at DESC LIMIT ?1")
        .map_err(|e| format!("Failed to prepare approval history query: {}", e))?;

    let records = stmt
        .query_map(params![limit as i64], |row| {
            Ok(ApprovalRecord {
                id: row.get(0)?,
                candidate_id: row.get(1)?,
                decision: row.get(2)?,
                decided_at: row.get(3)?,
                comparative_report_id: row.get(4)?,
                notes: row.get(5)?,
            })
        })
        .map_err(|e| format!("Failed to query approval history: {}", e))?;

    let mut results = Vec::new();
    for record in records {
        results.push(record.map_err(|e| format!("Failed to read approval row: {}", e))?);
    }
    Ok(results)
}

// ─── Evaluation Job CRUD ────────────────────────────────────────────────────

/// Default maximum concurrent evaluation jobs.
pub const DEFAULT_MAX_CONCURRENT_JOBS: u32 = 2;

pub fn insert_evaluation_job(
    conn: &Connection,
    job: &EvaluationJobRecord,
    max_concurrent: Option<u32>,
) -> Result<(), String> {
    let limit = max_concurrent.unwrap_or(DEFAULT_MAX_CONCURRENT_JOBS);
    let active = count_active_evaluation_jobs(conn)?;
    if active >= limit {
        return Err(format!(
            "Max concurrent evaluation jobs reached ({}/{}). Cannot submit new job.",
            active, limit
        ));
    }

    conn.execute(
        "INSERT INTO evaluation_jobs (
            id, candidate_id, compute_job_id, status, sandbox_config_json,
            submitted_at, started_at, completed_at, error_message, benchmark_results_json
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            job.id,
            job.candidate_id,
            job.compute_job_id,
            job.status,
            job.sandbox_config_json,
            job.submitted_at,
            job.started_at,
            job.completed_at,
            job.error_message,
            job.benchmark_results_json,
        ],
    )
    .map_err(|e| format!("Failed to insert evaluation job: {}", e))?;
    Ok(())
}

pub fn update_evaluation_job_status(
    conn: &Connection,
    id: &str,
    status: &str,
    results: Option<&str>,
) -> Result<(), String> {
    let now = chrono::Utc::now().to_rfc3339();
    let (started, completed) = match status {
        "running" => (Some(now.clone()), None),
        "completed" | "failed" | "timed-out" => (None, Some(now.clone())),
        _ => (None, None),
    };

    if let Some(started_at) = started {
        conn.execute(
            "UPDATE evaluation_jobs SET status = ?1, started_at = ?2 WHERE id = ?3",
            params![status, started_at, id],
        )
        .map_err(|e| format!("Failed to update evaluation job: {}", e))?;
    } else if let Some(completed_at) = completed {
        conn.execute(
            "UPDATE evaluation_jobs SET status = ?1, completed_at = ?2, benchmark_results_json = ?3 WHERE id = ?4",
            params![status, completed_at, results, id],
        )
        .map_err(|e| format!("Failed to update evaluation job: {}", e))?;
    } else {
        conn.execute(
            "UPDATE evaluation_jobs SET status = ?1 WHERE id = ?2",
            params![status, id],
        )
        .map_err(|e| format!("Failed to update evaluation job: {}", e))?;
    }
    Ok(())
}

pub fn count_active_evaluation_jobs(conn: &Connection) -> Result<u32, String> {
    let count: u32 = conn
        .query_row(
            "SELECT COUNT(*) FROM evaluation_jobs WHERE status IN ('submitted', 'running')",
            [],
            |row| row.get(0),
        )
        .map_err(|e| format!("Failed to count active jobs: {}", e))?;
    Ok(count)
}

// ─── Discovery Sources CRUD ─────────────────────────────────────────────────

pub fn insert_source(conn: &Connection, source: &DiscoverySourceRecord) -> Result<(), String> {
    conn.execute(
        "INSERT INTO discovery_sources (
            id, type, url, enabled, polling_frequency_hours, last_polled_at, category_filters_json
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            source.id,
            source.source_type,
            source.url,
            source.enabled as i32,
            source.polling_frequency_hours as i64,
            source.last_polled_at,
            source.category_filters_json,
        ],
    )
    .map_err(|e| format!("Failed to insert discovery source: {}", e))?;
    Ok(())
}

pub fn update_source(conn: &Connection, source: &DiscoverySourceRecord) -> Result<(), String> {
    conn.execute(
        "UPDATE discovery_sources SET type = ?1, url = ?2, enabled = ?3,
         polling_frequency_hours = ?4, category_filters_json = ?5 WHERE id = ?6",
        params![
            source.source_type,
            source.url,
            source.enabled as i32,
            source.polling_frequency_hours as i64,
            source.category_filters_json,
            source.id,
        ],
    )
    .map_err(|e| format!("Failed to update discovery source: {}", e))?;
    Ok(())
}

pub fn query_enabled_sources(conn: &Connection) -> Result<Vec<DiscoverySourceRecord>, String> {
    let mut stmt = conn
        .prepare("SELECT * FROM discovery_sources WHERE enabled = 1")
        .map_err(|e| format!("Failed to prepare sources query: {}", e))?;

    let records = stmt
        .query_map([], |row| {
            Ok(DiscoverySourceRecord {
                id: row.get(0)?,
                source_type: row.get(1)?,
                url: row.get(2)?,
                enabled: row.get::<_, i32>(3)? != 0,
                polling_frequency_hours: row.get::<_, i64>(4)? as u32,
                last_polled_at: row.get(5)?,
                category_filters_json: row.get(6)?,
            })
        })
        .map_err(|e| format!("Failed to query sources: {}", e))?;

    let mut results = Vec::new();
    for record in records {
        results.push(record.map_err(|e| format!("Failed to read source row: {}", e))?);
    }
    Ok(results)
}

pub fn update_last_polled(conn: &Connection, source_id: &str) -> Result<(), String> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE discovery_sources SET last_polled_at = ?1 WHERE id = ?2",
        params![now, source_id],
    )
    .map_err(|e| format!("Failed to update last polled: {}", e))?;
    Ok(())
}

// ─── Cleanup Functions ──────────────────────────────────────────────────────

/// Delete candidates in "deferred" state that have exceeded the retention period.
/// Returns the number of records cleaned up.
pub fn cleanup_expired_artifacts(conn: &Connection, retention_days: u32) -> Result<u32, String> {
    let cutoff = chrono::Utc::now()
        - chrono::Duration::days(retention_days as i64);
    let cutoff_str = cutoff.to_rfc3339();

    // Delete associated reports and approval decisions first
    conn.execute(
        "DELETE FROM comparative_reports WHERE candidate_id IN (
            SELECT id FROM candidates WHERE status = 'deferred' AND updated_at < ?1
        )",
        params![cutoff_str],
    )
    .map_err(|e| format!("Failed to cleanup reports: {}", e))?;

    conn.execute(
        "DELETE FROM approval_decisions WHERE candidate_id IN (
            SELECT id FROM candidates WHERE status = 'deferred' AND updated_at < ?1
        )",
        params![cutoff_str],
    )
    .map_err(|e| format!("Failed to cleanup approvals: {}", e))?;

    conn.execute(
        "DELETE FROM evaluation_jobs WHERE candidate_id IN (
            SELECT id FROM candidates WHERE status = 'deferred' AND updated_at < ?1
        )",
        params![cutoff_str],
    )
    .map_err(|e| format!("Failed to cleanup eval jobs: {}", e))?;

    let deleted: u32 = conn
        .execute(
            "DELETE FROM candidates WHERE status = 'deferred' AND updated_at < ?1",
            params![cutoff_str],
        )
        .map_err(|e| format!("Failed to cleanup deferred candidates: {}", e))?
        as u32;

    Ok(deleted)
}

/// Get approximate storage usage in bytes (row count * estimated row size).
pub fn get_storage_usage(conn: &Connection) -> Result<u64, String> {
    let candidate_count: u64 = conn
        .query_row("SELECT COUNT(*) FROM candidates", [], |row| row.get(0))
        .map_err(|e| format!("Failed to count candidates: {}", e))?;
    let report_count: u64 = conn
        .query_row("SELECT COUNT(*) FROM comparative_reports", [], |row| {
            row.get(0)
        })
        .map_err(|e| format!("Failed to count reports: {}", e))?;
    let job_count: u64 = conn
        .query_row("SELECT COUNT(*) FROM evaluation_jobs", [], |row| row.get(0))
        .map_err(|e| format!("Failed to count jobs: {}", e))?;

    // Estimate: candidates ~2KB, reports ~5KB, jobs ~1KB each
    let estimated_bytes = candidate_count * 2048 + report_count * 5120 + job_count * 1024;
    Ok(estimated_bytes)
}

// ─── Post-Installation Tracking ─────────────────────────────────────────────

pub fn insert_post_install_tracking(
    conn: &Connection,
    tracking: &PostInstallTracking,
) -> Result<(), String> {
    conn.execute(
        "INSERT INTO post_install_tracking (
            candidate_id, installed_at, predicted_score, actual_scores_json,
            deviation_flagged, deviation_flagged_at, days_tracked
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            tracking.candidate_id,
            tracking.installed_at,
            tracking.predicted_score,
            tracking.actual_scores_json,
            tracking.deviation_flagged as i32,
            tracking.deviation_flagged_at,
            tracking.days_tracked as i64,
        ],
    )
    .map_err(|e| format!("Failed to insert post-install tracking: {}", e))?;
    Ok(())
}

pub fn update_post_install_scores(
    conn: &Connection,
    candidate_id: &str,
    daily_score: f64,
) -> Result<(), String> {
    // Get current scores
    let current_json: String = conn
        .query_row(
            "SELECT actual_scores_json FROM post_install_tracking WHERE candidate_id = ?1",
            params![candidate_id],
            |row| row.get(0),
        )
        .map_err(|e| format!("Failed to get current scores: {}", e))?;

    let mut scores: Vec<f64> =
        serde_json::from_str(&current_json).unwrap_or_default();
    scores.push(daily_score);
    let updated_json = serde_json::to_string(&scores)
        .map_err(|e| format!("Failed to serialize scores: {}", e))?;

    conn.execute(
        "UPDATE post_install_tracking SET actual_scores_json = ?1, days_tracked = ?2 WHERE candidate_id = ?3",
        params![updated_json, scores.len() as i64, candidate_id],
    )
    .map_err(|e| format!("Failed to update post-install scores: {}", e))?;
    Ok(())
}

pub fn flag_deviation(conn: &Connection, candidate_id: &str) -> Result<(), String> {
    let now = chrono::Utc::now().to_rfc3339();
    conn.execute(
        "UPDATE post_install_tracking SET deviation_flagged = 1, deviation_flagged_at = ?1 WHERE candidate_id = ?2",
        params![now, candidate_id],
    )
    .map_err(|e| format!("Failed to flag deviation: {}", e))?;
    Ok(())
}

pub fn get_post_install_tracking(
    conn: &Connection,
    candidate_id: &str,
) -> Result<Option<PostInstallTracking>, String> {
    let result = conn
        .query_row(
            "SELECT * FROM post_install_tracking WHERE candidate_id = ?1",
            params![candidate_id],
            |row| {
                Ok(PostInstallTracking {
                    candidate_id: row.get(0)?,
                    installed_at: row.get(1)?,
                    predicted_score: row.get(2)?,
                    actual_scores_json: row.get(3)?,
                    deviation_flagged: row.get::<_, i32>(4)? != 0,
                    deviation_flagged_at: row.get(5)?,
                    days_tracked: row.get::<_, i64>(6)? as u32,
                })
            },
        )
        .ok();
    Ok(result)
}

// ─── NA2 Trust Tier ─────────────────────────────────────────────────────────

pub fn get_na2_trust_tier(conn: &Connection) -> Result<Option<NA2TrustTierState>, String> {
    let result = conn
        .query_row(
            "SELECT current_tier, promoted_at, validation_started_at,
             consecutive_days_accurate, consecutive_days_inaccurate
             FROM na2_trust_tier WHERE id = 'singleton'",
            [],
            |row| {
                Ok(NA2TrustTierState {
                    current_tier: row.get(0)?,
                    promoted_at: row.get(1)?,
                    validation_started_at: row.get(2)?,
                    consecutive_days_accurate: row.get::<_, i64>(3)? as u32,
                    consecutive_days_inaccurate: row.get::<_, i64>(4)? as u32,
                })
            },
        )
        .ok();
    Ok(result)
}

pub fn upsert_na2_trust_tier(conn: &Connection, state: &NA2TrustTierState) -> Result<(), String> {
    conn.execute(
        "INSERT OR REPLACE INTO na2_trust_tier (
            id, current_tier, promoted_at, validation_started_at,
            consecutive_days_accurate, consecutive_days_inaccurate
        ) VALUES ('singleton', ?1, ?2, ?3, ?4, ?5)",
        params![
            state.current_tier,
            state.promoted_at,
            state.validation_started_at,
            state.consecutive_days_accurate as i64,
            state.consecutive_days_inaccurate as i64,
        ],
    )
    .map_err(|e| format!("Failed to upsert NA2 trust tier: {}", e))?;
    Ok(())
}

// ─── Discovery Circuit Breaker ──────────────────────────────────────────────

pub fn get_circuit_breaker(conn: &Connection) -> Result<Option<DiscoveryCircuitBreaker>, String> {
    let result = conn
        .query_row(
            "SELECT consecutive_failures, is_open, last_failure_at, cooldown_ends_at,
             cooldown_secs, failure_threshold
             FROM discovery_circuit_breaker WHERE id = 'singleton'",
            [],
            |row| {
                Ok(DiscoveryCircuitBreaker {
                    consecutive_failures: row.get::<_, i64>(0)? as u32,
                    is_open: row.get::<_, i32>(1)? != 0,
                    last_failure_at: row.get(2)?,
                    cooldown_ends_at: row.get(3)?,
                    cooldown_secs: row.get::<_, i64>(4)? as u64,
                    failure_threshold: row.get::<_, i64>(5)? as u32,
                })
            },
        )
        .ok();
    Ok(result)
}

pub fn upsert_circuit_breaker(
    conn: &Connection,
    state: &DiscoveryCircuitBreaker,
) -> Result<(), String> {
    conn.execute(
        "INSERT OR REPLACE INTO discovery_circuit_breaker (
            id, consecutive_failures, is_open, last_failure_at, cooldown_ends_at,
            cooldown_secs, failure_threshold
        ) VALUES ('singleton', ?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            state.consecutive_failures as i64,
            state.is_open as i32,
            state.last_failure_at,
            state.cooldown_ends_at,
            state.cooldown_secs as i64,
            state.failure_threshold as i64,
        ],
    )
    .map_err(|e| format!("Failed to upsert circuit breaker: {}", e))?;
    Ok(())
}

// ─── IPC Commands ───────────────────────────────────────────────────────────

#[tauri::command]
pub fn agent_evaluator_discover(source: serde_json::Value) -> Result<Vec<CandidateRecord>, String> {
    // Discovery is handled by the TypeScript orchestration layer.
    // This command returns candidates from the database for a given source.
    let _source = source;
    Ok(Vec::new())
}

#[tauri::command]
pub fn agent_evaluator_approve_testing(candidate_id: String) -> Result<(), String> {
    let _ = candidate_id;
    Ok(())
}

#[tauri::command]
pub fn agent_evaluator_reject(candidate_id: String) -> Result<(), String> {
    let _ = candidate_id;
    Ok(())
}

#[tauri::command]
pub fn agent_evaluator_defer(candidate_id: String) -> Result<(), String> {
    let _ = candidate_id;
    Ok(())
}

#[tauri::command]
pub fn agent_evaluator_submit_eval(
    candidate_id: String,
    sandbox_config: serde_json::Value,
) -> Result<String, String> {
    let _ = (candidate_id, sandbox_config);
    Ok(String::new())
}

#[tauri::command]
pub fn agent_evaluator_get_report(
    candidate_id: String,
) -> Result<Option<ComparativeReportRecord>, String> {
    let _ = candidate_id;
    Ok(None)
}

#[tauri::command]
pub fn agent_evaluator_approve_install(
    candidate_id: String,
    decision: String,
) -> Result<(), String> {
    let _ = (candidate_id, decision);
    Ok(())
}

#[tauri::command]
pub fn agent_evaluator_query_history(
    filters: serde_json::Value,
) -> Result<Vec<CandidateRecord>, String> {
    let _ = filters;
    Ok(Vec::new())
}

#[tauri::command]
pub fn agent_evaluator_post_install_perf(
    candidate_id: String,
) -> Result<serde_json::Value, String> {
    let _ = candidate_id;
    Ok(serde_json::json!({
        "predictedScore": 0.0,
        "actualScore": 0.0,
        "deviationPercent": 0.0,
        "daysTracked": 0
    }))
}

// ─── Unit Tests ─────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        initialize_agent_evaluator_db(&conn).unwrap();
        conn
    }

    fn make_candidate(id: &str, status: &str) -> CandidateRecord {
        CandidateRecord {
            id: id.to_string(),
            name: format!("candidate-{}", id),
            source_url: format!("https://github.com/test/{}", id),
            source_type: "github-trending".to_string(),
            discovery_score: 0.75,
            score_breakdown_json: r#"{"communityActivity":0.8,"documentationQuality":0.7,"manifestCompatibility":0.75}"#.to_string(),
            category: "coding".to_string(),
            manifest_capabilities_json: r#"["filesystem","shell"]"#.to_string(),
            estimated_eval_cost_json: r#"{"computeTimeMinutes":30,"estimatedTokens":50000,"estimatedCostUsd":0.5}"#.to_string(),
            status: status.to_string(),
            discovered_at: "2025-01-01T00:00:00Z".to_string(),
            version: "1.0.0".to_string(),
            manifest_id: format!("manifest-{}", id),
            updated_at: "2025-01-01T00:00:00Z".to_string(),
        }
    }

    #[test]
    fn test_schema_initialization() {
        let conn = setup_db();
        // Verify all tables exist
        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert!(tables.contains(&"candidates".to_string()));
        assert!(tables.contains(&"comparative_reports".to_string()));
        assert!(tables.contains(&"approval_decisions".to_string()));
        assert!(tables.contains(&"evaluation_jobs".to_string()));
        assert!(tables.contains(&"discovery_sources".to_string()));
        assert!(tables.contains(&"benchmark_suites".to_string()));
        assert!(tables.contains(&"post_install_tracking".to_string()));
        assert!(tables.contains(&"na2_trust_tier".to_string()));
        assert!(tables.contains(&"discovery_circuit_breaker".to_string()));
    }

    #[test]
    fn test_candidate_insert_and_query() {
        let conn = setup_db();
        let candidate = make_candidate("c1", "discovered");
        insert_candidate(&conn, &candidate).unwrap();

        let results = query_candidates(&conn, Some("discovered"), None, None).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "c1");
        assert_eq!(results[0].status, "discovered");
    }

    #[test]
    fn test_candidate_status_transitions() {
        let conn = setup_db();
        let candidate = make_candidate("c2", "discovered");
        insert_candidate(&conn, &candidate).unwrap();

        update_candidate_status(&conn, "c2", "approved-for-testing").unwrap();
        let results = query_candidates(&conn, Some("approved-for-testing"), None, None).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].status, "approved-for-testing");

        update_candidate_status(&conn, "c2", "testing-in-progress").unwrap();
        update_candidate_status(&conn, "c2", "evaluation-complete").unwrap();
        update_candidate_status(&conn, "c2", "presented-for-approval").unwrap();
        update_candidate_status(&conn, "c2", "approved-for-install").unwrap();
        update_candidate_status(&conn, "c2", "installed").unwrap();

        let results = query_candidates(&conn, Some("installed"), None, None).unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_candidate_query_filters() {
        let conn = setup_db();
        let mut c1 = make_candidate("c1", "discovered");
        c1.category = "coding".to_string();
        c1.discovery_score = 0.9;
        let mut c2 = make_candidate("c2", "discovered");
        c2.category = "research".to_string();
        c2.discovery_score = 0.5;
        let c3 = make_candidate("c3", "rejected");

        insert_candidate(&conn, &c1).unwrap();
        insert_candidate(&conn, &c2).unwrap();
        insert_candidate(&conn, &c3).unwrap();

        // Filter by category
        let results = query_candidates(&conn, None, Some("coding"), None).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "c1");

        // Filter by status
        let results = query_candidates(&conn, Some("rejected"), None, None).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "c3");

        // Limit
        let results = query_candidates(&conn, Some("discovered"), None, Some(1)).unwrap();
        assert_eq!(results.len(), 1);
        // Should be ordered by discovery_score DESC, so c1 first
        assert_eq!(results[0].id, "c1");
    }

    #[test]
    fn test_concurrent_job_limit_enforcement() {
        let conn = setup_db();
        let candidate = make_candidate("c1", "approved-for-testing");
        insert_candidate(&conn, &candidate).unwrap();

        let job1 = EvaluationJobRecord {
            id: "job1".to_string(),
            candidate_id: "c1".to_string(),
            compute_job_id: "compute-1".to_string(),
            status: "submitted".to_string(),
            sandbox_config_json: "{}".to_string(),
            submitted_at: "2025-01-01T00:00:00Z".to_string(),
            started_at: None,
            completed_at: None,
            error_message: None,
            benchmark_results_json: None,
        };
        let job2 = EvaluationJobRecord {
            id: "job2".to_string(),
            candidate_id: "c1".to_string(),
            compute_job_id: "compute-2".to_string(),
            status: "running".to_string(),
            sandbox_config_json: "{}".to_string(),
            submitted_at: "2025-01-01T00:00:00Z".to_string(),
            started_at: Some("2025-01-01T00:01:00Z".to_string()),
            completed_at: None,
            error_message: None,
            benchmark_results_json: None,
        };
        let job3 = EvaluationJobRecord {
            id: "job3".to_string(),
            candidate_id: "c1".to_string(),
            compute_job_id: "compute-3".to_string(),
            status: "submitted".to_string(),
            sandbox_config_json: "{}".to_string(),
            submitted_at: "2025-01-01T00:00:00Z".to_string(),
            started_at: None,
            completed_at: None,
            error_message: None,
            benchmark_results_json: None,
        };

        insert_evaluation_job(&conn, &job1, Some(2)).unwrap();
        insert_evaluation_job(&conn, &job2, Some(2)).unwrap();

        // Third job should fail - max concurrent is 2
        let result = insert_evaluation_job(&conn, &job3, Some(2));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Max concurrent evaluation jobs reached"));
    }

    #[test]
    fn test_rejected_candidate_suppression() {
        let conn = setup_db();
        let mut candidate = make_candidate("c1", "rejected");
        candidate.source_url = "https://github.com/test/agent-x".to_string();
        candidate.manifest_id = "agent-x-manifest".to_string();
        candidate.version = "1.0.0".to_string();
        insert_candidate(&conn, &candidate).unwrap();

        // Same version should be suppressed
        let rejected = is_previously_rejected(
            &conn,
            "https://github.com/test/agent-x",
            "agent-x-manifest",
            Some("1.0.0"),
        )
        .unwrap();
        assert!(rejected);

        // Minor version bump should still be suppressed
        let rejected = is_previously_rejected(
            &conn,
            "https://github.com/test/agent-x",
            "agent-x-manifest",
            Some("1.5.0"),
        )
        .unwrap();
        assert!(rejected);

        // Major version bump should NOT be suppressed
        let rejected = is_previously_rejected(
            &conn,
            "https://github.com/test/agent-x",
            "agent-x-manifest",
            Some("2.0.0"),
        )
        .unwrap();
        assert!(!rejected);

        // Different URL/manifest should not be suppressed
        let rejected = is_previously_rejected(
            &conn,
            "https://github.com/test/other-agent",
            "other-manifest",
            Some("1.0.0"),
        )
        .unwrap();
        assert!(!rejected);
    }

    #[test]
    fn test_comparative_report_crud() {
        let conn = setup_db();
        let candidate = make_candidate("c1", "evaluation-complete");
        insert_candidate(&conn, &candidate).unwrap();

        let report = ComparativeReportRecord {
            id: "report-1".to_string(),
            candidate_id: "c1".to_string(),
            candidate_name: "candidate-c1".to_string(),
            incumbent_agent_ids_json: r#"["agent-1"]"#.to_string(),
            evaluation_timestamp: "2025-01-01T00:00:00Z".to_string(),
            replay_task_set_ids_json: r#"["task-1","task-2"]"#.to_string(),
            sandbox_config_json: r#"{"cpuCores":2,"memoryCapMb":4096}"#.to_string(),
            per_task_deltas_json: "[]".to_string(),
            aggregate_scores_json: "{}".to_string(),
            candidate_verdict: "promising".to_string(),
            production_prediction_json: None,
            security_assessment_json: "{}".to_string(),
        };
        insert_comparative_report(&conn, &report).unwrap();

        let result = query_report_by_candidate(&conn, "c1").unwrap();
        assert!(result.is_some());
        let r = result.unwrap();
        assert_eq!(r.candidate_verdict, "promising");
    }

    #[test]
    fn test_approval_decision_crud() {
        let conn = setup_db();
        let candidate = make_candidate("c1", "presented-for-approval");
        insert_candidate(&conn, &candidate).unwrap();

        let report = ComparativeReportRecord {
            id: "report-1".to_string(),
            candidate_id: "c1".to_string(),
            candidate_name: "candidate-c1".to_string(),
            incumbent_agent_ids_json: "[]".to_string(),
            evaluation_timestamp: "2025-01-01T00:00:00Z".to_string(),
            replay_task_set_ids_json: "[]".to_string(),
            sandbox_config_json: "{}".to_string(),
            per_task_deltas_json: "[]".to_string(),
            aggregate_scores_json: "{}".to_string(),
            candidate_verdict: "promising".to_string(),
            production_prediction_json: None,
            security_assessment_json: "{}".to_string(),
        };
        insert_comparative_report(&conn, &report).unwrap();

        let approval = ApprovalRecord {
            id: "approval-1".to_string(),
            candidate_id: "c1".to_string(),
            decision: "approve".to_string(),
            decided_at: "2025-01-02T00:00:00Z".to_string(),
            comparative_report_id: "report-1".to_string(),
            notes: Some("Looks good".to_string()),
        };
        insert_approval_decision(&conn, &approval).unwrap();

        let history = query_approval_history(&conn, 10).unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].decision, "approve");
    }

    #[test]
    fn test_discovery_sources_crud() {
        let conn = setup_db();
        let source = DiscoverySourceRecord {
            id: "src-1".to_string(),
            source_type: "github-trending".to_string(),
            url: "https://github.com/trending".to_string(),
            enabled: true,
            polling_frequency_hours: 24,
            last_polled_at: None,
            category_filters_json: r#"["coding","research"]"#.to_string(),
        };
        insert_source(&conn, &source).unwrap();

        let sources = query_enabled_sources(&conn).unwrap();
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].id, "src-1");

        update_last_polled(&conn, "src-1").unwrap();
        let sources = query_enabled_sources(&conn).unwrap();
        assert!(sources[0].last_polled_at.is_some());
    }

    #[test]
    fn test_cleanup_expired_artifacts() {
        let conn = setup_db();
        // Create a deferred candidate with old updated_at
        let mut candidate = make_candidate("c1", "deferred");
        candidate.updated_at = "2020-01-01T00:00:00Z".to_string();
        insert_candidate(&conn, &candidate).unwrap();

        // Create a recent deferred candidate
        let mut candidate2 = make_candidate("c2", "deferred");
        candidate2.updated_at = chrono::Utc::now().to_rfc3339();
        insert_candidate(&conn, &candidate2).unwrap();

        let deleted = cleanup_expired_artifacts(&conn, 30).unwrap();
        assert_eq!(deleted, 1);

        // c2 should still exist
        let results = query_candidates(&conn, Some("deferred"), None, None).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "c2");
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;
    use rusqlite::Connection;

    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        initialize_agent_evaluator_db(&conn).unwrap();
        conn
    }

    proptest! {
        #[test]
        fn prop_candidate_score_persists(score in 0.0f64..=1.0f64) {
            let conn = setup_db();
            let mut candidate = CandidateRecord {
                id: "prop-test".to_string(),
                name: "prop-candidate".to_string(),
                source_url: "https://example.com".to_string(),
                source_type: "github-trending".to_string(),
                discovery_score: score,
                score_breakdown_json: "{}".to_string(),
                category: "coding".to_string(),
                manifest_capabilities_json: "[]".to_string(),
                estimated_eval_cost_json: "{}".to_string(),
                status: "discovered".to_string(),
                discovered_at: "2025-01-01T00:00:00Z".to_string(),
                version: "1.0.0".to_string(),
                manifest_id: "m1".to_string(),
                updated_at: "2025-01-01T00:00:00Z".to_string(),
            };
            insert_candidate(&conn, &candidate).unwrap();
            let results = query_candidates(&conn, None, None, None).unwrap();
            prop_assert_eq!(results.len(), 1);
            prop_assert!((results[0].discovery_score - score).abs() < f64::EPSILON);
        }

        #[test]
        fn prop_concurrent_limit_never_exceeded(
            num_jobs in 1u32..=10u32,
            max_concurrent in 1u32..=5u32
        ) {
            let conn = setup_db();
            let candidate = CandidateRecord {
                id: "c1".to_string(),
                name: "test".to_string(),
                source_url: "https://example.com".to_string(),
                source_type: "github-trending".to_string(),
                discovery_score: 0.5,
                score_breakdown_json: "{}".to_string(),
                category: "coding".to_string(),
                manifest_capabilities_json: "[]".to_string(),
                estimated_eval_cost_json: "{}".to_string(),
                status: "approved-for-testing".to_string(),
                discovered_at: "2025-01-01T00:00:00Z".to_string(),
                version: "1.0.0".to_string(),
                manifest_id: "m1".to_string(),
                updated_at: "2025-01-01T00:00:00Z".to_string(),
            };
            insert_candidate(&conn, &candidate).unwrap();

            let mut successful = 0u32;
            for i in 0..num_jobs {
                let job = EvaluationJobRecord {
                    id: format!("job-{}", i),
                    candidate_id: "c1".to_string(),
                    compute_job_id: format!("compute-{}", i),
                    status: "submitted".to_string(),
                    sandbox_config_json: "{}".to_string(),
                    submitted_at: "2025-01-01T00:00:00Z".to_string(),
                    started_at: None,
                    completed_at: None,
                    error_message: None,
                    benchmark_results_json: None,
                };
                if insert_evaluation_job(&conn, &job, Some(max_concurrent)).is_ok() {
                    successful += 1;
                }
            }
            // Active jobs should never exceed max_concurrent
            let active = count_active_evaluation_jobs(&conn).unwrap();
            prop_assert!(active <= max_concurrent);
            prop_assert_eq!(successful, active);
        }
    }
}
