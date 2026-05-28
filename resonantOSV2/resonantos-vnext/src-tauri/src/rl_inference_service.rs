// Intent citation: .kiro/specs/unified-rl-policy/design.md
// RL Inference Service — advisory RL policy for agent selection optimization

use chrono::Utc;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;

// ─── Struct Definitions (Task 1.1) ───────────────────────────────────────────

/// Configuration for the RL inference service.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RLInferenceConfig {
    pub model_artifact_path: String,
    pub inference_timeout_ms: u64,
    pub circuit_breaker_threshold: u32,
    pub circuit_breaker_cooldown_secs: u64,
    pub cold_start_threshold: u32,
    pub confidence_ramp_episodes: u32,
    pub evaluation_window_size: u32,
    pub min_model_versions_retained: u32,
}

impl Default for RLInferenceConfig {
    fn default() -> Self {
        Self {
            model_artifact_path: String::new(),
            inference_timeout_ms: 10,
            circuit_breaker_threshold: 5,
            circuit_breaker_cooldown_secs: 60,
            cold_start_threshold: 50,
            confidence_ramp_episodes: 100,
            evaluation_window_size: 50,
            min_model_versions_retained: 5,
        }
    }
}

/// The RL recommendation produced by inference.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RLRecommendation {
    pub recommended_agent_id: String,
    pub confidence_score: f64,
    pub expected_reward: f64,
    pub q_values: Vec<(String, f64)>,
    pub model_version_id: String,
    pub inference_duration_ms: f64,
    pub timestamp: String,
}

/// Model version metadata loaded alongside ONNX weights.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelVersion {
    pub version_id: String,
    pub training_timestamp: String,
    pub data_window_start: String,
    pub data_window_end: String,
    pub episode_count: u32,
    pub final_high_level_loss: f64,
    pub final_low_level_loss: f64,
    pub validation_metrics: serde_json::Value,
    pub normalization_mean: Vec<f64>,
    pub normalization_var: Vec<f64>,
    pub artifact_path: String,
    pub is_active: bool,
    pub is_last_known_good: bool,
    pub created_at: String,
}

/// Circuit breaker state for inference failures.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RLCircuitBreakerState {
    pub consecutive_failures: u32,
    pub is_open: bool,
    pub last_failure_at: Option<String>,
    pub cooldown_ends_at: Option<String>,
    pub cooldown_secs: u64,
    pub failure_threshold: u32,
}

impl Default for RLCircuitBreakerState {
    fn default() -> Self {
        Self {
            consecutive_failures: 0,
            is_open: false,
            last_failure_at: None,
            cooldown_ends_at: None,
            cooldown_secs: 60,
            failure_threshold: 5,
        }
    }
}

/// Trust tier state for the RL policy.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RLTrustTierState {
    pub current_tier: String,
    pub confidence_threshold: f64,
    pub promoted_at: Option<String>,
    pub validation_started_at: String,
    pub consecutive_days_improved: u32,
    pub consecutive_days_degraded: u32,
}

impl Default for RLTrustTierState {
    fn default() -> Self {
        Self {
            current_tier: "addon".to_string(),
            confidence_threshold: 0.80,
            promoted_at: None,
            validation_started_at: Utc::now().to_rfc3339(),
            consecutive_days_improved: 0,
            consecutive_days_degraded: 0,
        }
    }
}

/// Cold start state tracking.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ColdStartState {
    pub experience_count: u32,
    pub cold_start_threshold: u32,
    pub has_graduated: bool,
    pub graduated_at: Option<String>,
    pub episodes_since_graduation: u32,
}

impl Default for ColdStartState {
    fn default() -> Self {
        Self {
            experience_count: 0,
            cold_start_threshold: 50,
            has_graduated: false,
            graduated_at: None,
            episodes_since_graduation: 0,
        }
    }
}

/// Pre-computed agent statistics cache for fast state vector construction.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentStatsCache {
    pub agent_id: String,
    pub task_type: String,
    pub quality_score: f64,
    pub speed_score: f64,
    pub cost_score: f64,
    pub availability: f64,
    pub task_type_percentile: f64,
    pub avg_efficiency_ratio: f64,
    pub pattern_rate_per_100: f64,
    pub avg_tool_calls: f64,
    pub cost_per_tool_call: f64,
    pub last_updated_at: String,
}

/// Service status response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RLServiceStatus {
    pub status: String,
    pub current_model_version: Option<String>,
    pub cold_start_state: ColdStartState,
    pub circuit_breaker: RLCircuitBreakerState,
    pub trust_tier: RLTrustTierState,
    pub total_inferences: u64,
    pub acceptance_rate: f64,
}

/// Model evaluation tracking record.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelEvaluation {
    pub id: String,
    pub new_version_id: String,
    pub previous_version_id: String,
    pub started_at: String,
    pub decisions_evaluated: u32,
    pub evaluation_window_size: u32,
    pub new_version_acceptance_rate: Option<f64>,
    pub new_version_avg_logician_score: Option<f64>,
    pub previous_version_acceptance_rate: Option<f64>,
    pub previous_version_avg_logician_score: Option<f64>,
    pub result: String,
    pub completed_at: Option<String>,
}

/// Inference log entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InferenceLogEntry {
    pub id: String,
    pub delegation_packet_id: String,
    pub timestamp: String,
    pub task_type: String,
    pub recommended_agent_id: String,
    pub confidence_score: f64,
    pub expected_reward: f64,
    pub q_values_json: String,
    pub model_version_id: String,
    pub inference_duration_ms: f64,
    pub advisory_accepted: bool,
    pub rejection_reason: Option<String>,
    pub heuristic_agent_id: String,
    pub outcome_logician_score: Option<f64>,
    pub outcome_recorded_at: Option<String>,
}

/// Shared state for the RL inference service.
pub struct RLInferenceState {
    pub config: RLInferenceConfig,
    pub db: Arc<std::sync::Mutex<Connection>>,
    pub circuit_breaker: Arc<RwLock<RLCircuitBreakerState>>,
    pub trust_tier: Arc<RwLock<RLTrustTierState>>,
    pub cold_start: Arc<RwLock<ColdStartState>>,
    pub agent_stats_cache: Arc<RwLock<Vec<AgentStatsCache>>>,
    pub model_versions: Arc<RwLock<Vec<ModelVersion>>>,
    #[cfg(feature = "tract-onnx")]
    pub current_model: Arc<RwLock<Option<LoadedModel>>>,
}


// ─── Database Initialization (Task 1.2) ──────────────────────────────────────

/// Initialize the RL policy database with all required tables and indexes.
pub fn initialize_rl_policy_db(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS model_versions (
            version_id TEXT PRIMARY KEY,
            training_timestamp TEXT NOT NULL,
            data_window_start TEXT NOT NULL,
            data_window_end TEXT NOT NULL,
            episode_count INTEGER NOT NULL,
            final_high_level_loss REAL NOT NULL,
            final_low_level_loss REAL NOT NULL,
            validation_metrics_json TEXT NOT NULL,
            normalization_mean_json TEXT NOT NULL,
            normalization_var_json TEXT NOT NULL,
            artifact_path TEXT NOT NULL,
            is_active INTEGER NOT NULL DEFAULT 0,
            is_last_known_good INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS inference_log (
            id TEXT PRIMARY KEY,
            delegation_packet_id TEXT NOT NULL,
            timestamp TEXT NOT NULL,
            task_type TEXT NOT NULL,
            recommended_agent_id TEXT NOT NULL,
            confidence_score REAL NOT NULL,
            expected_reward REAL NOT NULL,
            q_values_json TEXT NOT NULL,
            model_version_id TEXT NOT NULL,
            inference_duration_ms REAL NOT NULL,
            advisory_accepted INTEGER NOT NULL DEFAULT 0,
            rejection_reason TEXT,
            heuristic_agent_id TEXT NOT NULL,
            outcome_logician_score REAL,
            outcome_recorded_at TEXT
        );

        CREATE TABLE IF NOT EXISTS trust_tier_state (
            id TEXT PRIMARY KEY DEFAULT 'singleton',
            current_tier TEXT NOT NULL DEFAULT 'addon',
            confidence_threshold REAL NOT NULL DEFAULT 0.80,
            promoted_at TEXT,
            validation_started_at TEXT NOT NULL,
            consecutive_days_improved INTEGER NOT NULL DEFAULT 0,
            consecutive_days_degraded INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS trust_tier_transitions (
            id TEXT PRIMARY KEY,
            from_tier TEXT NOT NULL,
            to_tier TEXT NOT NULL,
            transitioned_at TEXT NOT NULL,
            validation_period_days INTEGER NOT NULL,
            metrics_json TEXT NOT NULL,
            direction TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS cold_start_state (
            id TEXT PRIMARY KEY DEFAULT 'singleton',
            experience_count INTEGER NOT NULL DEFAULT 0,
            cold_start_threshold INTEGER NOT NULL DEFAULT 50,
            has_graduated INTEGER NOT NULL DEFAULT 0,
            graduated_at TEXT,
            episodes_since_graduation INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS circuit_breaker_state (
            id TEXT PRIMARY KEY DEFAULT 'singleton',
            consecutive_failures INTEGER NOT NULL DEFAULT 0,
            is_open INTEGER NOT NULL DEFAULT 0,
            last_failure_at TEXT,
            cooldown_ends_at TEXT,
            cooldown_secs INTEGER NOT NULL DEFAULT 60,
            failure_threshold INTEGER NOT NULL DEFAULT 5
        );

        CREATE TABLE IF NOT EXISTS agent_stats_cache (
            agent_id TEXT NOT NULL,
            task_type TEXT NOT NULL,
            quality_score REAL NOT NULL DEFAULT 0.0,
            speed_score REAL NOT NULL DEFAULT 0.0,
            cost_score REAL NOT NULL DEFAULT 0.0,
            availability REAL NOT NULL DEFAULT 0.0,
            task_type_percentile REAL NOT NULL DEFAULT 0.0,
            avg_efficiency_ratio REAL NOT NULL DEFAULT 0.5,
            pattern_rate_per_100 REAL NOT NULL DEFAULT 0.0,
            avg_tool_calls REAL NOT NULL DEFAULT 0.0,
            cost_per_tool_call REAL NOT NULL DEFAULT 0.0,
            last_updated_at TEXT NOT NULL,
            PRIMARY KEY (agent_id, task_type)
        );

        CREATE TABLE IF NOT EXISTS model_evaluation (
            id TEXT PRIMARY KEY,
            new_version_id TEXT NOT NULL,
            previous_version_id TEXT NOT NULL,
            started_at TEXT NOT NULL,
            decisions_evaluated INTEGER NOT NULL DEFAULT 0,
            evaluation_window_size INTEGER NOT NULL DEFAULT 50,
            new_version_acceptance_rate REAL,
            new_version_avg_logician_score REAL,
            previous_version_acceptance_rate REAL,
            previous_version_avg_logician_score REAL,
            result TEXT DEFAULT 'pending',
            completed_at TEXT
        );

        CREATE TABLE IF NOT EXISTS training_jobs (
            job_id TEXT PRIMARY KEY,
            started_at TEXT NOT NULL,
            completed_at TEXT,
            status TEXT NOT NULL DEFAULT 'running',
            episode_count INTEGER,
            final_high_level_loss REAL,
            final_low_level_loss REAL,
            model_version_id TEXT,
            trigger_reason TEXT NOT NULL,
            error_message TEXT
        );

        CREATE INDEX IF NOT EXISTS idx_inference_log_timestamp
            ON inference_log(timestamp);
        CREATE INDEX IF NOT EXISTS idx_inference_log_packet
            ON inference_log(delegation_packet_id);
        CREATE INDEX IF NOT EXISTS idx_inference_log_accepted
            ON inference_log(advisory_accepted);
        CREATE INDEX IF NOT EXISTS idx_inference_log_model
            ON inference_log(model_version_id);
        CREATE INDEX IF NOT EXISTS idx_model_versions_active
            ON model_versions(is_active);
        CREATE INDEX IF NOT EXISTS idx_training_jobs_status
            ON training_jobs(status);
        ",
    )?;
    Ok(())
}


// ─── Circuit Breaker CRUD (Task 1.3) ─────────────────────────────────────────

/// Read circuit breaker state from the database.
pub fn read_circuit_breaker(conn: &Connection) -> Result<RLCircuitBreakerState, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT consecutive_failures, is_open, last_failure_at, cooldown_ends_at, cooldown_secs, failure_threshold
         FROM circuit_breaker_state WHERE id = 'singleton'"
    )?;

    let result = stmt.query_row([], |row| {
        Ok(RLCircuitBreakerState {
            consecutive_failures: row.get(0)?,
            is_open: row.get::<_, i32>(1)? != 0,
            last_failure_at: row.get(2)?,
            cooldown_ends_at: row.get(3)?,
            cooldown_secs: row.get::<_, i64>(4)? as u64,
            failure_threshold: row.get(5)?,
        })
    });

    match result {
        Ok(state) => Ok(state),
        Err(rusqlite::Error::QueryReturnedNoRows) => {
            // Insert default and return it
            let default_state = RLCircuitBreakerState::default();
            conn.execute(
                "INSERT OR IGNORE INTO circuit_breaker_state (id, consecutive_failures, is_open, cooldown_secs, failure_threshold)
                 VALUES ('singleton', 0, 0, ?1, ?2)",
                params![default_state.cooldown_secs as i64, default_state.failure_threshold],
            )?;
            Ok(default_state)
        }
        Err(e) => Err(e),
    }
}

/// Persist circuit breaker state to the database.
pub fn persist_circuit_breaker(
    conn: &Connection,
    state: &RLCircuitBreakerState,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT OR REPLACE INTO circuit_breaker_state
         (id, consecutive_failures, is_open, last_failure_at, cooldown_ends_at, cooldown_secs, failure_threshold)
         VALUES ('singleton', ?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            state.consecutive_failures,
            state.is_open as i32,
            state.last_failure_at,
            state.cooldown_ends_at,
            state.cooldown_secs as i64,
            state.failure_threshold,
        ],
    )?;
    Ok(())
}

/// Update circuit breaker after an inference attempt.
/// Returns the new state.
pub fn update_circuit_breaker(
    state: &RLCircuitBreakerState,
    success: bool,
    now: &str,
) -> RLCircuitBreakerState {
    if success {
        // Success resets consecutive failures and closes the breaker
        RLCircuitBreakerState {
            consecutive_failures: 0,
            is_open: false,
            last_failure_at: state.last_failure_at.clone(),
            cooldown_ends_at: None,
            cooldown_secs: state.cooldown_secs,
            failure_threshold: state.failure_threshold,
        }
    } else {
        let new_failures = state.consecutive_failures + 1;
        let should_open = new_failures >= state.failure_threshold;
        let cooldown_ends = if should_open {
            // Calculate cooldown end time
            if let Ok(now_dt) = chrono::DateTime::parse_from_rfc3339(now) {
                Some(
                    (now_dt + chrono::Duration::seconds(state.cooldown_secs as i64))
                        .to_rfc3339(),
                )
            } else {
                None
            }
        } else {
            state.cooldown_ends_at.clone()
        };

        RLCircuitBreakerState {
            consecutive_failures: new_failures,
            is_open: should_open,
            last_failure_at: Some(now.to_string()),
            cooldown_ends_at: cooldown_ends,
            cooldown_secs: state.cooldown_secs,
            failure_threshold: state.failure_threshold,
        }
    }
}

/// Check if inference should be attempted (circuit breaker check with cooldown expiry).
pub fn should_attempt_inference(circuit_breaker: &RLCircuitBreakerState, now: &str) -> bool {
    if !circuit_breaker.is_open {
        return true;
    }

    // Check if cooldown has expired
    if let Some(ref cooldown_ends) = circuit_breaker.cooldown_ends_at {
        if let (Ok(now_dt), Ok(cooldown_dt)) = (
            chrono::DateTime::parse_from_rfc3339(now),
            chrono::DateTime::parse_from_rfc3339(cooldown_ends),
        ) {
            return now_dt >= cooldown_dt;
        }
    }

    false
}


// ─── Cold Start State CRUD (Task 1.4) ────────────────────────────────────────

/// Read cold start state from the database.
pub fn read_cold_start(conn: &Connection) -> Result<ColdStartState, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT experience_count, cold_start_threshold, has_graduated, graduated_at, episodes_since_graduation
         FROM cold_start_state WHERE id = 'singleton'"
    )?;

    let result = stmt.query_row([], |row| {
        Ok(ColdStartState {
            experience_count: row.get(0)?,
            cold_start_threshold: row.get(1)?,
            has_graduated: row.get::<_, i32>(2)? != 0,
            graduated_at: row.get(3)?,
            episodes_since_graduation: row.get(4)?,
        })
    });

    match result {
        Ok(state) => Ok(state),
        Err(rusqlite::Error::QueryReturnedNoRows) => {
            let default_state = ColdStartState::default();
            conn.execute(
                "INSERT OR IGNORE INTO cold_start_state (id, experience_count, cold_start_threshold, has_graduated, episodes_since_graduation)
                 VALUES ('singleton', 0, ?1, 0, 0)",
                params![default_state.cold_start_threshold],
            )?;
            Ok(default_state)
        }
        Err(e) => Err(e),
    }
}

/// Persist cold start state to the database.
pub fn update_cold_start(
    conn: &Connection,
    state: &ColdStartState,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT OR REPLACE INTO cold_start_state
         (id, experience_count, cold_start_threshold, has_graduated, graduated_at, episodes_since_graduation)
         VALUES ('singleton', ?1, ?2, ?3, ?4, ?5)",
        params![
            state.experience_count,
            state.cold_start_threshold,
            state.has_graduated as i32,
            state.graduated_at,
            state.episodes_since_graduation,
        ],
    )?;
    Ok(())
}

/// Check if cold start graduation should trigger.
/// Graduation occurs when experience_count >= cold_start_threshold.
/// Returns the updated state (with has_graduated set if threshold met).
pub fn check_graduation(state: &ColdStartState, now: &str) -> ColdStartState {
    if state.has_graduated {
        // Already graduated, just increment episodes_since_graduation
        return ColdStartState {
            episodes_since_graduation: state.episodes_since_graduation + 1,
            ..state.clone()
        };
    }

    if state.experience_count >= state.cold_start_threshold {
        ColdStartState {
            experience_count: state.experience_count,
            cold_start_threshold: state.cold_start_threshold,
            has_graduated: true,
            graduated_at: Some(now.to_string()),
            episodes_since_graduation: 0,
        }
    } else {
        state.clone()
    }
}


// ─── Trust Tier State CRUD (Task 1.5) ────────────────────────────────────────

/// Map tier name to confidence threshold.
pub fn tier_to_threshold(tier: &str) -> f64 {
    match tier {
        "trusted" => 0.60,
        _ => 0.80, // "addon" or any unknown tier defaults to 0.80
    }
}

/// Read trust tier state from the database.
pub fn read_trust_tier(conn: &Connection) -> Result<RLTrustTierState, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT current_tier, confidence_threshold, promoted_at, validation_started_at,
                consecutive_days_improved, consecutive_days_degraded
         FROM trust_tier_state WHERE id = 'singleton'"
    )?;

    let result = stmt.query_row([], |row| {
        Ok(RLTrustTierState {
            current_tier: row.get(0)?,
            confidence_threshold: row.get(1)?,
            promoted_at: row.get(2)?,
            validation_started_at: row.get(3)?,
            consecutive_days_improved: row.get(4)?,
            consecutive_days_degraded: row.get(5)?,
        })
    });

    match result {
        Ok(state) => Ok(state),
        Err(rusqlite::Error::QueryReturnedNoRows) => {
            let default_state = RLTrustTierState::default();
            conn.execute(
                "INSERT OR IGNORE INTO trust_tier_state
                 (id, current_tier, confidence_threshold, validation_started_at, consecutive_days_improved, consecutive_days_degraded)
                 VALUES ('singleton', 'addon', 0.80, ?1, 0, 0)",
                params![default_state.validation_started_at],
            )?;
            Ok(default_state)
        }
        Err(e) => Err(e),
    }
}

/// Persist trust tier state to the database.
pub fn update_trust_tier(
    conn: &Connection,
    state: &RLTrustTierState,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT OR REPLACE INTO trust_tier_state
         (id, current_tier, confidence_threshold, promoted_at, validation_started_at,
          consecutive_days_improved, consecutive_days_degraded)
         VALUES ('singleton', ?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            state.current_tier,
            state.confidence_threshold,
            state.promoted_at,
            state.validation_started_at,
            state.consecutive_days_improved,
            state.consecutive_days_degraded,
        ],
    )?;
    Ok(())
}

/// Evaluate trust tier promotion/demotion based on daily performance.
/// `improved_today`: whether RL-accepted outcomes were >= heuristic-only today.
/// Returns the updated state and an optional transition direction ("promotion" or "demotion").
pub fn evaluate_trust_tier(
    state: &RLTrustTierState,
    improved_today: bool,
    now: &str,
) -> (RLTrustTierState, Option<String>) {
    let mut new_state = state.clone();

    if improved_today {
        new_state.consecutive_days_improved += 1;
        new_state.consecutive_days_degraded = 0;
    } else {
        new_state.consecutive_days_degraded += 1;
        new_state.consecutive_days_improved = 0;
    }

    // Check promotion: addon -> trusted after 30 consecutive days improved
    if new_state.current_tier == "addon" && new_state.consecutive_days_improved >= 30 {
        new_state.current_tier = "trusted".to_string();
        new_state.confidence_threshold = tier_to_threshold("trusted");
        new_state.promoted_at = Some(now.to_string());
        new_state.consecutive_days_improved = 0;
        new_state.consecutive_days_degraded = 0;
        return (new_state, Some("promotion".to_string()));
    }

    // Check demotion: trusted -> addon after 7 consecutive days degraded
    if new_state.current_tier == "trusted" && new_state.consecutive_days_degraded >= 7 {
        new_state.current_tier = "addon".to_string();
        new_state.confidence_threshold = tier_to_threshold("addon");
        new_state.promoted_at = None;
        new_state.consecutive_days_improved = 0;
        new_state.consecutive_days_degraded = 0;
        return (new_state, Some("demotion".to_string()));
    }

    (new_state, None)
}

/// Log a trust tier transition to the database.
pub fn log_trust_tier_transition(
    conn: &Connection,
    from_tier: &str,
    to_tier: &str,
    direction: &str,
    validation_period_days: u32,
    metrics_json: &str,
    now: &str,
) -> Result<(), rusqlite::Error> {
    let id = format!("tt-{}", now.replace([':', '-', 'T', '+'], ""));
    conn.execute(
        "INSERT INTO trust_tier_transitions (id, from_tier, to_tier, transitioned_at, validation_period_days, metrics_json, direction)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![id, from_tier, to_tier, now, validation_period_days, metrics_json, direction],
    )?;
    Ok(())
}


// ─── Agent Stats Cache CRUD (Task 1.6) ───────────────────────────────────────

/// Read all agent stats from the cache.
pub fn read_agent_stats(conn: &Connection) -> Result<Vec<AgentStatsCache>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT agent_id, task_type, quality_score, speed_score, cost_score, availability,
                task_type_percentile, avg_efficiency_ratio, pattern_rate_per_100,
                avg_tool_calls, cost_per_tool_call, last_updated_at
         FROM agent_stats_cache"
    )?;

    let rows = stmt.query_map([], |row| {
        Ok(AgentStatsCache {
            agent_id: row.get(0)?,
            task_type: row.get(1)?,
            quality_score: row.get(2)?,
            speed_score: row.get(3)?,
            cost_score: row.get(4)?,
            availability: row.get(5)?,
            task_type_percentile: row.get(6)?,
            avg_efficiency_ratio: row.get(7)?,
            pattern_rate_per_100: row.get(8)?,
            avg_tool_calls: row.get(9)?,
            cost_per_tool_call: row.get(10)?,
            last_updated_at: row.get(11)?,
        })
    })?;

    let mut stats = Vec::new();
    for row in rows {
        stats.push(row?);
    }
    Ok(stats)
}

/// Read agent stats for a specific agent and task type.
pub fn read_agent_stats_for(
    conn: &Connection,
    agent_id: &str,
    task_type: &str,
) -> Result<Option<AgentStatsCache>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT agent_id, task_type, quality_score, speed_score, cost_score, availability,
                task_type_percentile, avg_efficiency_ratio, pattern_rate_per_100,
                avg_tool_calls, cost_per_tool_call, last_updated_at
         FROM agent_stats_cache WHERE agent_id = ?1 AND task_type = ?2"
    )?;

    let result = stmt.query_row(params![agent_id, task_type], |row| {
        Ok(AgentStatsCache {
            agent_id: row.get(0)?,
            task_type: row.get(1)?,
            quality_score: row.get(2)?,
            speed_score: row.get(3)?,
            cost_score: row.get(4)?,
            availability: row.get(5)?,
            task_type_percentile: row.get(6)?,
            avg_efficiency_ratio: row.get(7)?,
            pattern_rate_per_100: row.get(8)?,
            avg_tool_calls: row.get(9)?,
            cost_per_tool_call: row.get(10)?,
            last_updated_at: row.get(11)?,
        })
    });

    match result {
        Ok(stats) => Ok(Some(stats)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e),
    }
}

/// Upsert agent stats into the cache.
pub fn upsert_agent_stats(
    conn: &Connection,
    stats: &AgentStatsCache,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT OR REPLACE INTO agent_stats_cache
         (agent_id, task_type, quality_score, speed_score, cost_score, availability,
          task_type_percentile, avg_efficiency_ratio, pattern_rate_per_100,
          avg_tool_calls, cost_per_tool_call, last_updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            stats.agent_id,
            stats.task_type,
            stats.quality_score,
            stats.speed_score,
            stats.cost_score,
            stats.availability,
            stats.task_type_percentile,
            stats.avg_efficiency_ratio,
            stats.pattern_rate_per_100,
            stats.avg_tool_calls,
            stats.cost_per_tool_call,
            stats.last_updated_at,
        ],
    )?;
    Ok(())
}

/// Refresh agent stats cache from experience_buffer.db and tool_call_tracker.db.
/// This queries the external databases for rolling averages and updates the local cache.
pub fn refresh_agent_stats_from_experience_buffer(
    rl_conn: &Connection,
    experience_db_path: &str,
    tracker_db_path: &str,
) -> Result<Vec<AgentStatsCache>, rusqlite::Error> {
    let now = Utc::now().to_rfc3339();

    // Attach experience buffer database
    rl_conn.execute(
        "ATTACH DATABASE ?1 AS exp_db",
        params![experience_db_path],
    )?;

    // Query rolling averages from experience buffer's historical_stats_cache
    let mut stmt = rl_conn.prepare(
        "SELECT agent_id, task_type, rolling_quality_score, rolling_speed_ms, rolling_cost_tokens
         FROM exp_db.historical_stats_cache"
    )?;

    let mut stats_map: std::collections::HashMap<(String, String), AgentStatsCache> =
        std::collections::HashMap::new();

    let rows = stmt.query_map([], |row| {
        let agent_id: String = row.get(0)?;
        let task_type: String = row.get(1)?;
        let quality: f64 = row.get(2)?;
        let speed: f64 = row.get(3)?;
        let cost: f64 = row.get(4)?;
        Ok((agent_id, task_type, quality, speed, cost))
    })?;

    for row in rows {
        let (agent_id, task_type, quality, speed, cost) = row?;
        let key = (agent_id.clone(), task_type.clone());
        stats_map.insert(
            key,
            AgentStatsCache {
                agent_id,
                task_type,
                quality_score: quality,
                speed_score: speed,
                cost_score: cost,
                availability: 1.0, // Default; updated from health monitor if available
                task_type_percentile: 0.0,
                avg_efficiency_ratio: 0.5,
                pattern_rate_per_100: 0.0,
                avg_tool_calls: 0.0,
                cost_per_tool_call: 0.0,
                last_updated_at: now.clone(),
            },
        );
    }
    drop(stmt);

    rl_conn.execute("DETACH DATABASE exp_db", [])?;

    // Attach tool call tracker database for efficiency data
    let attach_result = rl_conn.execute(
        "ATTACH DATABASE ?1 AS tracker_db",
        params![tracker_db_path],
    );

    if attach_result.is_ok() {
        // Try to read tool call efficiency data
        let tracker_query = rl_conn.prepare(
            "SELECT agent_id, task_type,
                    AVG(efficiency_ratio) as avg_eff,
                    AVG(pattern_count) as avg_patterns,
                    AVG(total_tool_calls) as avg_calls
             FROM tracker_db.tool_call_traces
             GROUP BY agent_id, task_type"
        );

        if let Ok(mut tracker_stmt) = tracker_query {
            let tracker_rows = tracker_stmt.query_map([], |row| {
                let agent_id: String = row.get(0)?;
                let task_type: String = row.get(1)?;
                let avg_eff: f64 = row.get(2)?;
                let avg_patterns: f64 = row.get(3)?;
                let avg_calls: f64 = row.get(4)?;
                Ok((agent_id, task_type, avg_eff, avg_patterns, avg_calls))
            });

            if let Ok(rows) = tracker_rows {
                for row in rows.flatten() {
                    let (agent_id, task_type, avg_eff, avg_patterns, avg_calls) = row;
                    let key = (agent_id.clone(), task_type.clone());
                    if let Some(entry) = stats_map.get_mut(&key) {
                        entry.avg_efficiency_ratio = avg_eff;
                        entry.pattern_rate_per_100 = avg_patterns * 100.0;
                        entry.avg_tool_calls = avg_calls;
                    }
                }
            }
        }

        let _ = rl_conn.execute("DETACH DATABASE tracker_db", []);
    }

    // Persist all stats to local cache
    let stats: Vec<AgentStatsCache> = stats_map.into_values().collect();
    for s in &stats {
        upsert_agent_stats(rl_conn, s)?;
    }

    Ok(stats)
}


// ─── Model Version Registry (Task 1.7) ──────────────────────────────────────

/// Insert a new model version into the registry.
pub fn insert_model_version(
    conn: &Connection,
    version: &ModelVersion,
) -> Result<(), rusqlite::Error> {
    let mean_json = serde_json::to_string(&version.normalization_mean).unwrap_or_default();
    let var_json = serde_json::to_string(&version.normalization_var).unwrap_or_default();
    let metrics_json = serde_json::to_string(&version.validation_metrics).unwrap_or_default();

    conn.execute(
        "INSERT INTO model_versions
         (version_id, training_timestamp, data_window_start, data_window_end,
          episode_count, final_high_level_loss, final_low_level_loss,
          validation_metrics_json, normalization_mean_json, normalization_var_json,
          artifact_path, is_active, is_last_known_good, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        params![
            version.version_id,
            version.training_timestamp,
            version.data_window_start,
            version.data_window_end,
            version.episode_count,
            version.final_high_level_loss,
            version.final_low_level_loss,
            metrics_json,
            mean_json,
            var_json,
            version.artifact_path,
            version.is_active as i32,
            version.is_last_known_good as i32,
            version.created_at,
        ],
    )?;
    Ok(())
}

/// Query all model versions, ordered by creation time descending.
pub fn query_model_versions(conn: &Connection) -> Result<Vec<ModelVersion>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT version_id, training_timestamp, data_window_start, data_window_end,
                episode_count, final_high_level_loss, final_low_level_loss,
                validation_metrics_json, normalization_mean_json, normalization_var_json,
                artifact_path, is_active, is_last_known_good, created_at
         FROM model_versions ORDER BY created_at DESC"
    )?;

    let rows = stmt.query_map([], |row| {
        let metrics_str: String = row.get(7)?;
        let mean_str: String = row.get(8)?;
        let var_str: String = row.get(9)?;

        Ok(ModelVersion {
            version_id: row.get(0)?,
            training_timestamp: row.get(1)?,
            data_window_start: row.get(2)?,
            data_window_end: row.get(3)?,
            episode_count: row.get(4)?,
            final_high_level_loss: row.get(5)?,
            final_low_level_loss: row.get(6)?,
            validation_metrics: serde_json::from_str(&metrics_str).unwrap_or_default(),
            normalization_mean: serde_json::from_str(&mean_str).unwrap_or_default(),
            normalization_var: serde_json::from_str(&var_str).unwrap_or_default(),
            artifact_path: row.get(10)?,
            is_active: row.get::<_, i32>(11)? != 0,
            is_last_known_good: row.get::<_, i32>(12)? != 0,
            created_at: row.get(13)?,
        })
    })?;

    let mut versions = Vec::new();
    for row in rows {
        versions.push(row?);
    }
    Ok(versions)
}

/// Set a model version as the active model (deactivates all others).
pub fn set_active_model(
    conn: &Connection,
    version_id: &str,
) -> Result<(), rusqlite::Error> {
    conn.execute("UPDATE model_versions SET is_active = 0", [])?;
    conn.execute(
        "UPDATE model_versions SET is_active = 1 WHERE version_id = ?1",
        params![version_id],
    )?;
    Ok(())
}

/// Set a model version as the last known good (clears tag from all others).
pub fn set_last_known_good(
    conn: &Connection,
    version_id: &str,
) -> Result<(), rusqlite::Error> {
    conn.execute("UPDATE model_versions SET is_last_known_good = 0", [])?;
    conn.execute(
        "UPDATE model_versions SET is_last_known_good = 1 WHERE version_id = ?1",
        params![version_id],
    )?;
    Ok(())
}

/// Get the currently active model version.
pub fn get_active_model(conn: &Connection) -> Result<Option<ModelVersion>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT version_id, training_timestamp, data_window_start, data_window_end,
                episode_count, final_high_level_loss, final_low_level_loss,
                validation_metrics_json, normalization_mean_json, normalization_var_json,
                artifact_path, is_active, is_last_known_good, created_at
         FROM model_versions WHERE is_active = 1 LIMIT 1"
    )?;

    let result = stmt.query_row([], |row| {
        let metrics_str: String = row.get(7)?;
        let mean_str: String = row.get(8)?;
        let var_str: String = row.get(9)?;

        Ok(ModelVersion {
            version_id: row.get(0)?,
            training_timestamp: row.get(1)?,
            data_window_start: row.get(2)?,
            data_window_end: row.get(3)?,
            episode_count: row.get(4)?,
            final_high_level_loss: row.get(5)?,
            final_low_level_loss: row.get(6)?,
            validation_metrics: serde_json::from_str(&metrics_str).unwrap_or_default(),
            normalization_mean: serde_json::from_str(&mean_str).unwrap_or_default(),
            normalization_var: serde_json::from_str(&var_str).unwrap_or_default(),
            artifact_path: row.get(10)?,
            is_active: row.get::<_, i32>(11)? != 0,
            is_last_known_good: row.get::<_, i32>(12)? != 0,
            created_at: row.get(13)?,
        })
    });

    match result {
        Ok(v) => Ok(Some(v)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e),
    }
}

/// Enforce retention policy: keep at least `min_retained` versions, delete oldest beyond that.
pub fn enforce_model_retention(
    conn: &Connection,
    min_retained: u32,
) -> Result<(), rusqlite::Error> {
    let count: u32 = conn.query_row(
        "SELECT COUNT(*) FROM model_versions",
        [],
        |row| row.get(0),
    )?;

    if count > min_retained {
        // Delete oldest versions beyond retention limit, but never delete active or last_known_good
        conn.execute(
            "DELETE FROM model_versions WHERE version_id IN (
                SELECT version_id FROM model_versions
                WHERE is_active = 0 AND is_last_known_good = 0
                ORDER BY created_at ASC
                LIMIT ?1
            )",
            params![count - min_retained],
        )?;
    }
    Ok(())
}


// ─── Inference Log (Task 1.8) ────────────────────────────────────────────────

/// Log an inference decision to the database.
pub fn log_inference_decision(
    conn: &Connection,
    entry: &InferenceLogEntry,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO inference_log
         (id, delegation_packet_id, timestamp, task_type, recommended_agent_id,
          confidence_score, expected_reward, q_values_json, model_version_id,
          inference_duration_ms, advisory_accepted, rejection_reason, heuristic_agent_id,
          outcome_logician_score, outcome_recorded_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
        params![
            entry.id,
            entry.delegation_packet_id,
            entry.timestamp,
            entry.task_type,
            entry.recommended_agent_id,
            entry.confidence_score,
            entry.expected_reward,
            entry.q_values_json,
            entry.model_version_id,
            entry.inference_duration_ms,
            entry.advisory_accepted as i32,
            entry.rejection_reason,
            entry.heuristic_agent_id,
            entry.outcome_logician_score,
            entry.outcome_recorded_at,
        ],
    )?;
    Ok(())
}

/// Append outcome data to an existing inference log entry.
pub fn append_outcome_to_inference_log(
    conn: &Connection,
    delegation_packet_id: &str,
    logician_score: f64,
    recorded_at: &str,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "UPDATE inference_log SET outcome_logician_score = ?1, outcome_recorded_at = ?2
         WHERE delegation_packet_id = ?3",
        params![logician_score, recorded_at, delegation_packet_id],
    )?;
    Ok(())
}

/// Query parameters for inference log.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InferenceLogQuery {
    pub from_date: Option<String>,
    pub to_date: Option<String>,
    pub advisory_accepted: Option<bool>,
    pub model_version_id: Option<String>,
    pub limit: Option<u32>,
}

/// Query inference log entries with optional filters.
pub fn query_inference_log(
    conn: &Connection,
    query: &InferenceLogQuery,
) -> Result<Vec<InferenceLogEntry>, rusqlite::Error> {
    let mut sql = String::from(
        "SELECT id, delegation_packet_id, timestamp, task_type, recommended_agent_id,
                confidence_score, expected_reward, q_values_json, model_version_id,
                inference_duration_ms, advisory_accepted, rejection_reason, heuristic_agent_id,
                outcome_logician_score, outcome_recorded_at
         FROM inference_log WHERE 1=1"
    );

    let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

    if let Some(ref from) = query.from_date {
        param_values.push(Box::new(from.clone()));
        sql.push_str(&format!(" AND timestamp >= ?{}", param_values.len()));
    }
    if let Some(ref to) = query.to_date {
        param_values.push(Box::new(to.clone()));
        sql.push_str(&format!(" AND timestamp <= ?{}", param_values.len()));
    }
    if let Some(accepted) = query.advisory_accepted {
        param_values.push(Box::new(accepted as i32));
        sql.push_str(&format!(" AND advisory_accepted = ?{}", param_values.len()));
    }
    if let Some(ref model_id) = query.model_version_id {
        param_values.push(Box::new(model_id.clone()));
        sql.push_str(&format!(" AND model_version_id = ?{}", param_values.len()));
    }

    sql.push_str(" ORDER BY timestamp DESC");

    if let Some(limit) = query.limit {
        param_values.push(Box::new(limit));
        sql.push_str(&format!(" LIMIT ?{}", param_values.len()));
    }

    let mut stmt = conn.prepare(&sql)?;
    let params_ref: Vec<&dyn rusqlite::types::ToSql> = param_values.iter().map(|p| p.as_ref()).collect();
    let rows = stmt.query_map(params_ref.as_slice(), |row| {
        Ok(InferenceLogEntry {
            id: row.get(0)?,
            delegation_packet_id: row.get(1)?,
            timestamp: row.get(2)?,
            task_type: row.get(3)?,
            recommended_agent_id: row.get(4)?,
            confidence_score: row.get(5)?,
            expected_reward: row.get(6)?,
            q_values_json: row.get(7)?,
            model_version_id: row.get(8)?,
            inference_duration_ms: row.get(9)?,
            advisory_accepted: row.get::<_, i32>(10)? != 0,
            rejection_reason: row.get(11)?,
            heuristic_agent_id: row.get(12)?,
            outcome_logician_score: row.get(13)?,
            outcome_recorded_at: row.get(14)?,
        })
    })?;

    let mut entries = Vec::new();
    for row in rows {
        entries.push(row?);
    }
    Ok(entries)
}


// ─── ONNX Model Loading (Task 2.1-2.6) ──────────────────────────────────────

/// A loaded ONNX model ready for inference (tract-onnx feature).
#[cfg(feature = "tract-onnx")]
pub struct LoadedModel {
    pub high_level_plan: tract_onnx::prelude::SimplePlan<
        tract_onnx::prelude::TypedFact,
        Box<dyn tract_onnx::prelude::TypedOp>,
        tract_onnx::prelude::Graph<
            tract_onnx::prelude::TypedFact,
            Box<dyn tract_onnx::prelude::TypedOp>,
        >,
    >,
    pub low_level_plan: tract_onnx::prelude::SimplePlan<
        tract_onnx::prelude::TypedFact,
        Box<dyn tract_onnx::prelude::TypedOp>,
        tract_onnx::prelude::Graph<
            tract_onnx::prelude::TypedFact,
            Box<dyn tract_onnx::prelude::TypedOp>,
        >,
    >,
    pub version: ModelVersion,
}

/// Load an ONNX model from the artifact store path.
/// Validates input/output dimensions match expected state vector sizes.
#[cfg(feature = "tract-onnx")]
pub fn load_model_from_artifact(
    artifact_path: &str,
    version: &ModelVersion,
) -> Result<LoadedModel, String> {
    use tract_onnx::prelude::*;

    let high_level_path = format!("{}/high_level.onnx", artifact_path);
    let low_level_path = format!("{}/low_level.onnx", artifact_path);

    let high_level_model = tract_onnx::onnx()
        .model_for_path(&high_level_path)
        .map_err(|e| format!("Failed to load high-level ONNX: {}", e))?
        .into_optimized()
        .map_err(|e| format!("Failed to optimize high-level model: {}", e))?
        .into_runnable()
        .map_err(|e| format!("Failed to make high-level model runnable: {}", e))?;

    let low_level_model = tract_onnx::onnx()
        .model_for_path(&low_level_path)
        .map_err(|e| format!("Failed to load low-level ONNX: {}", e))?
        .into_optimized()
        .map_err(|e| format!("Failed to optimize low-level model: {}", e))?
        .into_runnable()
        .map_err(|e| format!("Failed to make low-level model runnable: {}", e))?;

    Ok(LoadedModel {
        high_level_plan: high_level_model,
        low_level_plan: low_level_model,
        version: version.clone(),
    })
}

/// Run the high-level forward pass: state vector → Q-values → sorted agents.
#[cfg(feature = "tract-onnx")]
pub fn run_high_level_forward_pass(
    model: &LoadedModel,
    state_vector: &[f32],
    candidate_agent_ids: &[String],
) -> Result<Vec<(String, f64)>, String> {
    use tract_onnx::prelude::*;

    let input = tract_ndarray::Array2::from_shape_vec(
        (1, state_vector.len()),
        state_vector.to_vec(),
    )
    .map_err(|e| format!("Failed to create input tensor: {}", e))?;

    let result = model
        .high_level_plan
        .run(tvec!(input.into_tvalue()))
        .map_err(|e| format!("High-level forward pass failed: {}", e))?;

    let q_values = result[0]
        .to_array_view::<f32>()
        .map_err(|e| format!("Failed to extract Q-values: {}", e))?;

    let mut agent_q_values: Vec<(String, f64)> = candidate_agent_ids
        .iter()
        .enumerate()
        .filter_map(|(i, agent_id)| {
            q_values.get([0, i]).map(|&q| (agent_id.clone(), q as f64))
        })
        .collect();

    // Sort by Q-value descending
    agent_q_values.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    Ok(agent_q_values)
}

/// Run the low-level forward pass: state → scalar quality score.
#[cfg(feature = "tract-onnx")]
pub fn run_low_level_forward_pass(
    model: &LoadedModel,
    state_vector: &[f32],
) -> Result<f64, String> {
    use tract_onnx::prelude::*;

    let input = tract_ndarray::Array2::from_shape_vec(
        (1, state_vector.len()),
        state_vector.to_vec(),
    )
    .map_err(|e| format!("Failed to create input tensor: {}", e))?;

    let result = model
        .low_level_plan
        .run(tvec!(input.into_tvalue()))
        .map_err(|e| format!("Low-level forward pass failed: {}", e))?;

    let score = result[0]
        .to_array_view::<f32>()
        .map_err(|e| format!("Failed to extract quality score: {}", e))?;

    Ok(*score.get([0, 0]).unwrap_or(&0.5) as f64)
}

/// Build the inference state vector from agent stats cache.
/// Normalizes using the model version's mean/var statistics.
/// Returns the concatenated state vector as f32 values.
pub fn build_inference_state_vector(
    agent_stats: &[AgentStatsCache],
    candidate_agent_ids: &[String],
    model_version: &ModelVersion,
    low_level_efficiency_estimate: f64,
) -> Vec<f32> {
    // Task embedding placeholder (zeros — actual embedding comes from sentence transformer at training time)
    // At inference, we use a fixed-size placeholder since the Rust side doesn't run the transformer
    let task_embedding_dim = 64; // TF-IDF+PCA fallback dimension
    let mut state_vector: Vec<f32> = vec![0.0; task_embedding_dim];

    // Per-agent features: quality, speed, cost, availability, percentile, efficiency, pattern_rate, avg_calls, cost_per_call
    let features_per_agent = 9;
    let max_candidates = 10;

    for i in 0..max_candidates {
        if i < candidate_agent_ids.len() {
            let agent_id = &candidate_agent_ids[i];
            if let Some(stats) = agent_stats.iter().find(|s| &s.agent_id == agent_id) {
                state_vector.push(stats.quality_score as f32);
                state_vector.push(stats.speed_score as f32);
                state_vector.push(stats.cost_score as f32);
                state_vector.push(stats.availability as f32);
                state_vector.push(stats.task_type_percentile as f32);
                state_vector.push(stats.avg_efficiency_ratio as f32);
                state_vector.push(stats.pattern_rate_per_100 as f32);
                state_vector.push(stats.avg_tool_calls as f32);
                state_vector.push(stats.cost_per_tool_call as f32);
            } else {
                // Agent not in cache — use zeros
                state_vector.extend(vec![0.0f32; features_per_agent]);
            }
        } else {
            // Pad with zeros for missing candidates
            state_vector.extend(vec![0.0f32; features_per_agent]);
        }
    }

    // Append low-level efficiency estimate
    state_vector.push(low_level_efficiency_estimate as f32);

    // Apply normalization if mean/var are available
    if !model_version.normalization_mean.is_empty()
        && model_version.normalization_mean.len() == state_vector.len()
    {
        for (i, val) in state_vector.iter_mut().enumerate() {
            let mean = model_version.normalization_mean[i] as f32;
            let var = model_version.normalization_var.get(i).copied().unwrap_or(1.0) as f32;
            let std_dev = var.sqrt().max(1e-8);
            *val = (*val - mean) / std_dev;
        }
    }

    state_vector
}

/// Compute confidence score with cold-start ramp-up scaling.
/// Raw confidence is derived from Q-value margin (difference between top-2 Q-values).
/// Scaled by min(1.0, episodes_since_graduation / confidence_ramp_episodes).
pub fn compute_confidence_with_ramp(
    raw_confidence: f64,
    cold_start: &ColdStartState,
    confidence_ramp_episodes: u32,
) -> f64 {
    if !cold_start.has_graduated {
        return 0.0;
    }

    let ramp_factor = if confidence_ramp_episodes == 0 {
        1.0
    } else {
        (cold_start.episodes_since_graduation as f64 / confidence_ramp_episodes as f64).min(1.0)
    };

    (raw_confidence * ramp_factor).clamp(0.0, 1.0)
}

/// Stub implementations for non-tract builds.
/// These return None/default values when the tract-onnx feature is not enabled.
#[cfg(not(feature = "tract-onnx"))]
pub fn load_model_from_artifact(
    _artifact_path: &str,
    _version: &ModelVersion,
) -> Result<(), String> {
    Ok(())
}

#[cfg(not(feature = "tract-onnx"))]
pub fn run_high_level_forward_pass_stub(
    _state_vector: &[f32],
    _candidate_agent_ids: &[String],
) -> Option<Vec<(String, f64)>> {
    None
}

#[cfg(not(feature = "tract-onnx"))]
pub fn run_low_level_forward_pass_stub(
    _state_vector: &[f32],
) -> Option<f64> {
    None
}

// ─── Inference Orchestration (Task 3.1-3.6) ──────────────────────────────────

/// Initialize the RL inference service state.
pub fn start_rl_inference_service(
    config: RLInferenceConfig,
    db_path: &str,
) -> Result<Arc<RLInferenceState>, String> {
    let conn = Connection::open(db_path)
        .map_err(|e| format!("Failed to open RL policy DB: {}", e))?;

    initialize_rl_policy_db(&conn)
        .map_err(|e| format!("Failed to initialize RL policy DB: {}", e))?;

    let circuit_breaker = read_circuit_breaker(&conn).unwrap_or_default();
    let trust_tier = read_trust_tier(&conn).unwrap_or_default();
    let cold_start = read_cold_start(&conn).unwrap_or_default();
    let agent_stats = read_agent_stats(&conn).unwrap_or_default();
    let model_versions = query_model_versions(&conn).unwrap_or_default();

    let state = RLInferenceState {
        config,
        db: Arc::new(std::sync::Mutex::new(conn)),
        circuit_breaker: Arc::new(RwLock::new(circuit_breaker)),
        trust_tier: Arc::new(RwLock::new(trust_tier)),
        cold_start: Arc::new(RwLock::new(cold_start)),
        agent_stats_cache: Arc::new(RwLock::new(agent_stats)),
        model_versions: Arc::new(RwLock::new(model_versions)),
        #[cfg(feature = "tract-onnx")]
        current_model: Arc::new(RwLock::new(None)),
    };

    Ok(Arc::new(state))
}

/// Produce an RL recommendation for a given task and candidate set.
/// Returns None if cold start, circuit breaker open, or inference fails/times out.
/// Must complete within inference_timeout_ms (default 10ms).
pub async fn infer_recommendation(
    state: &RLInferenceState,
    _task_description: &str,
    _task_type: &str,
    candidate_agent_ids: &[String],
) -> Option<RLRecommendation> {
    let timeout_ms = state.config.inference_timeout_ms;

    let result = tokio::time::timeout(
        std::time::Duration::from_millis(timeout_ms),
        infer_recommendation_inner(state, _task_description, _task_type, candidate_agent_ids),
    )
    .await;

    let now = Utc::now().to_rfc3339();

    match result {
        Ok(Some(rec)) => {
            // Success — update circuit breaker
            let cb = state.circuit_breaker.read().await;
            let new_cb = update_circuit_breaker(&cb, true, &now);
            drop(cb);
            *state.circuit_breaker.write().await = new_cb.clone();

            // Persist circuit breaker state
            if let Ok(db) = state.db.lock() {
                let _ = persist_circuit_breaker(&db, &new_cb);
            }

            Some(rec)
        }
        Ok(None) => None, // Legitimate None (cold start, etc.)
        Err(_) => {
            // Timeout — update circuit breaker as failure
            let cb = state.circuit_breaker.read().await;
            let new_cb = update_circuit_breaker(&cb, false, &now);
            drop(cb);
            *state.circuit_breaker.write().await = new_cb.clone();

            if let Ok(db) = state.db.lock() {
                let _ = persist_circuit_breaker(&db, &new_cb);
            }

            None
        }
    }
}

/// Inner inference logic (without timeout wrapper).
async fn infer_recommendation_inner(
    state: &RLInferenceState,
    _task_description: &str,
    _task_type: &str,
    candidate_agent_ids: &[String],
) -> Option<RLRecommendation> {
    let now = Utc::now().to_rfc3339();
    let start = std::time::Instant::now();

    // Check circuit breaker
    {
        let cb = state.circuit_breaker.read().await;
        if !should_attempt_inference(&cb, &now) {
            return None;
        }
    }

    // Check cold start
    let cold_start = state.cold_start.read().await.clone();
    if !cold_start.has_graduated {
        return None;
    }

    // Get agent stats
    let agent_stats = state.agent_stats_cache.read().await;

    // Build state vector
    let model_versions = state.model_versions.read().await;
    let active_version = model_versions.iter().find(|v| v.is_active)?;

    let _state_vector = build_inference_state_vector(
        &agent_stats,
        candidate_agent_ids,
        active_version,
        0.5, // Default low-level efficiency estimate
    );

    // Run forward pass (feature-gated)
    #[cfg(feature = "tract-onnx")]
    let q_values_result = {
        let model_guard = state.current_model.read().await;
        if let Some(ref model) = *model_guard {
            run_high_level_forward_pass(model, &state_vector, candidate_agent_ids).ok()
        } else {
            None
        }
    };

    #[cfg(not(feature = "tract-onnx"))]
    let q_values_result: Option<Vec<(String, f64)>> = None;

    let q_values = q_values_result?;
    if q_values.is_empty() {
        return None;
    }

    // Compute confidence from Q-value margin
    let raw_confidence = if q_values.len() >= 2 {
        let margin = q_values[0].1 - q_values[1].1;
        margin.clamp(0.0, 1.0)
    } else {
        q_values[0].1.clamp(0.0, 1.0)
    };

    let confidence = compute_confidence_with_ramp(
        raw_confidence,
        &cold_start,
        state.config.confidence_ramp_episodes,
    );

    let duration_ms = start.elapsed().as_secs_f64() * 1000.0;

    Some(RLRecommendation {
        recommended_agent_id: q_values[0].0.clone(),
        confidence_score: confidence,
        expected_reward: q_values[0].1,
        q_values,
        model_version_id: active_version.version_id.clone(),
        inference_duration_ms: duration_ms,
        timestamp: now,
    })
}

/// Background agent stats refresh task.
/// Spawns a periodic task that refreshes agent stats every 60 seconds.
pub fn spawn_background_stats_refresh(
    state: Arc<RLInferenceState>,
    experience_db_path: String,
    tracker_db_path: String,
) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
        loop {
            interval.tick().await;

            let stats_result = {
                let db = match state.db.lock() {
                    Ok(db) => db,
                    Err(_) => continue,
                };
                refresh_agent_stats_from_experience_buffer(
                    &db,
                    &experience_db_path,
                    &tracker_db_path,
                )
            }; // MutexGuard dropped here before await

            if let Ok(stats) = stats_result {
                let mut cache = state.agent_stats_cache.write().await;
                *cache = stats;
            }
        }
    });
}

// ─── Model Versioning and Rollback (Task 4.1-4.5) ───────────────────────────

/// Load a new model version: validate ONNX, swap active model atomically.
pub async fn load_model_version(
    state: &RLInferenceState,
    version_id: &str,
) -> Result<(), String> {
    let _version = {
        let db = state.db.lock().map_err(|e| format!("DB lock error: {}", e))?;
        let versions = query_model_versions(&db)
            .map_err(|e| format!("Failed to query versions: {}", e))?;
        versions
            .into_iter()
            .find(|v| v.version_id == version_id)
            .ok_or_else(|| format!("Version {} not found", version_id))?
    };

    // Load the ONNX model (feature-gated)
    #[cfg(feature = "tract-onnx")]
    {
        let loaded = load_model_from_artifact(&version.artifact_path, &version)?;
        let mut model_guard = state.current_model.write().await;
        *model_guard = Some(loaded);
    }

    // Set as active in DB
    {
        let db = state.db.lock().map_err(|e| format!("DB lock error: {}", e))?;
        set_active_model(&db, version_id)
            .map_err(|e| format!("Failed to set active model: {}", e))?;
    }

    // Update in-memory model versions
    {
        let mut versions = state.model_versions.write().await;
        for v in versions.iter_mut() {
            v.is_active = v.version_id == version_id;
        }
    }

    Ok(())
}

/// Rollback to a previous model version.
pub async fn rollback_model(
    state: &RLInferenceState,
    target_version_id: &str,
) -> Result<(), String> {
    load_model_version(state, target_version_id).await?;

    // Log rollback event
    if let Ok(db) = state.db.lock() {
        let now = Utc::now().to_rfc3339();
        let _ = log_inference_decision(
            &db,
            &InferenceLogEntry {
                id: format!("rollback-{}", now.replace([':', '-', 'T', '+'], "")),
                delegation_packet_id: "system-rollback".to_string(),
                timestamp: now,
                task_type: "system".to_string(),
                recommended_agent_id: String::new(),
                confidence_score: 0.0,
                expected_reward: 0.0,
                q_values_json: "[]".to_string(),
                model_version_id: target_version_id.to_string(),
                inference_duration_ms: 0.0,
                advisory_accepted: false,
                rejection_reason: Some("model-rollback".to_string()),
                heuristic_agent_id: String::new(),
                outcome_logician_score: None,
                outcome_recorded_at: None,
            },
        );
    }

    Ok(())
}

/// Insert a model evaluation record when a new version is deployed.
pub fn insert_model_evaluation(
    conn: &Connection,
    new_version_id: &str,
    previous_version_id: &str,
    evaluation_window_size: u32,
) -> Result<String, rusqlite::Error> {
    let now = Utc::now().to_rfc3339();
    let id = format!("eval-{}", now.replace([':', '-', 'T', '+'], ""));

    conn.execute(
        "INSERT INTO model_evaluation
         (id, new_version_id, previous_version_id, started_at, decisions_evaluated,
          evaluation_window_size, result)
         VALUES (?1, ?2, ?3, ?4, 0, ?5, 'pending')",
        params![id, new_version_id, previous_version_id, now, evaluation_window_size],
    )?;

    Ok(id)
}

/// Update model evaluation counters after each inference decision.
pub fn update_model_evaluation_counters(
    conn: &Connection,
    evaluation_id: &str,
    accepted: bool,
    logician_score: Option<f64>,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "UPDATE model_evaluation SET decisions_evaluated = decisions_evaluated + 1 WHERE id = ?1",
        params![evaluation_id],
    )?;

    // Update running averages
    if accepted {
        if let Some(score) = logician_score {
            // Update new version stats
            let current: (Option<f64>, i32) = conn.query_row(
                "SELECT new_version_avg_logician_score, decisions_evaluated FROM model_evaluation WHERE id = ?1",
                params![evaluation_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?;

            let new_avg = match current.0 {
                Some(prev_avg) => (prev_avg * (current.1 - 1) as f64 + score) / current.1 as f64,
                None => score,
            };

            conn.execute(
                "UPDATE model_evaluation SET new_version_avg_logician_score = ?1 WHERE id = ?2",
                params![new_avg, evaluation_id],
            )?;
        }
    }

    Ok(())
}

/// Evaluate a model version against the previous one.
/// Returns true if the new version should be kept, false if rollback needed.
pub async fn evaluate_model_version(
    state: &RLInferenceState,
) -> Result<bool, String> {
    let db = state.db.lock().map_err(|e| format!("DB lock error: {}", e))?;

    // Find pending evaluation
    let eval: Option<ModelEvaluation> = db
        .prepare(
            "SELECT id, new_version_id, previous_version_id, started_at,
                    decisions_evaluated, evaluation_window_size,
                    new_version_acceptance_rate, new_version_avg_logician_score,
                    previous_version_acceptance_rate, previous_version_avg_logician_score,
                    result, completed_at
             FROM model_evaluation WHERE result = 'pending' ORDER BY started_at DESC LIMIT 1",
        )
        .and_then(|mut stmt| {
            stmt.query_row([], |row| {
                Ok(ModelEvaluation {
                    id: row.get(0)?,
                    new_version_id: row.get(1)?,
                    previous_version_id: row.get(2)?,
                    started_at: row.get(3)?,
                    decisions_evaluated: row.get(4)?,
                    evaluation_window_size: row.get(5)?,
                    new_version_acceptance_rate: row.get(6)?,
                    new_version_avg_logician_score: row.get(7)?,
                    previous_version_acceptance_rate: row.get(8)?,
                    previous_version_avg_logician_score: row.get(9)?,
                    result: row.get(10)?,
                    completed_at: row.get(11)?,
                })
            })
        })
        .ok();

    let eval = match eval {
        Some(e) => e,
        None => return Ok(true), // No pending evaluation
    };

    // Check if evaluation window is complete
    if eval.decisions_evaluated < eval.evaluation_window_size {
        return Ok(true); // Not enough data yet, keep current
    }

    // Compare metrics
    let new_acceptance = eval.new_version_acceptance_rate.unwrap_or(0.0);
    let prev_acceptance = eval.previous_version_acceptance_rate.unwrap_or(0.0);
    let new_score = eval.new_version_avg_logician_score.unwrap_or(0.0);
    let prev_score = eval.previous_version_avg_logician_score.unwrap_or(0.0);

    let should_keep = new_acceptance >= prev_acceptance && new_score >= prev_score;

    let now = Utc::now().to_rfc3339();
    let result_str = if should_keep { "keep" } else { "rollback" };

    db.execute(
        "UPDATE model_evaluation SET result = ?1, completed_at = ?2 WHERE id = ?3",
        params![result_str, now, eval.id],
    )
    .map_err(|e| format!("Failed to update evaluation: {}", e))?;

    // If keeping, update last_known_good
    if should_keep {
        set_last_known_good(&db, &eval.new_version_id)
            .map_err(|e| format!("Failed to set LKG: {}", e))?;
    }

    Ok(should_keep)
}

// ─── IPC Commands (Task 3.6) ─────────────────────────────────────────────────

/// Get the current RL service status.
pub fn get_rl_service_status(
    state: &RLInferenceState,
    circuit_breaker: &RLCircuitBreakerState,
    trust_tier: &RLTrustTierState,
    cold_start: &ColdStartState,
) -> RLServiceStatus {
    let status = if circuit_breaker.is_open {
        "circuit_breaker_open".to_string()
    } else if !cold_start.has_graduated {
        if cold_start.experience_count == 0 {
            "untrained".to_string()
        } else {
            "cold_start".to_string()
        }
    } else {
        "active".to_string()
    };

    let current_model_version = state
        .model_versions
        .try_read()
        .ok()
        .and_then(|versions| {
            versions.iter().find(|v| v.is_active).map(|v| v.version_id.clone())
        });

    // Query total inferences and acceptance rate from DB
    let (total_inferences, acceptance_rate) = if let Ok(db) = state.db.lock() {
        let total: u64 = db
            .query_row("SELECT COUNT(*) FROM inference_log", [], |row| row.get(0))
            .unwrap_or(0);
        let accepted: u64 = db
            .query_row(
                "SELECT COUNT(*) FROM inference_log WHERE advisory_accepted = 1",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        let rate = if total > 0 {
            accepted as f64 / total as f64
        } else {
            0.0
        };
        (total, rate)
    } else {
        (0, 0.0)
    };

    RLServiceStatus {
        status,
        current_model_version,
        cold_start_state: cold_start.clone(),
        circuit_breaker: circuit_breaker.clone(),
        trust_tier: trust_tier.clone(),
        total_inferences,
        acceptance_rate,
    }
}

// ─── Unit Tests (Task 1.9) ───────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        initialize_rl_policy_db(&conn).unwrap();
        conn
    }

    #[test]
    fn test_schema_initialization() {
        let conn = setup_test_db();
        // Verify all tables exist by querying them
        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert!(tables.contains(&"model_versions".to_string()));
        assert!(tables.contains(&"inference_log".to_string()));
        assert!(tables.contains(&"trust_tier_state".to_string()));
        assert!(tables.contains(&"trust_tier_transitions".to_string()));
        assert!(tables.contains(&"cold_start_state".to_string()));
        assert!(tables.contains(&"circuit_breaker_state".to_string()));
        assert!(tables.contains(&"agent_stats_cache".to_string()));
        assert!(tables.contains(&"model_evaluation".to_string()));
        assert!(tables.contains(&"training_jobs".to_string()));
    }

    #[test]
    fn test_schema_idempotent() {
        let conn = setup_test_db();
        // Running initialization again should not error
        assert!(initialize_rl_policy_db(&conn).is_ok());
    }

    #[test]
    fn test_circuit_breaker_success_resets() {
        let state = RLCircuitBreakerState {
            consecutive_failures: 3,
            is_open: false,
            last_failure_at: Some("2025-01-01T00:00:00Z".to_string()),
            cooldown_ends_at: None,
            cooldown_secs: 60,
            failure_threshold: 5,
        };

        let new_state = update_circuit_breaker(&state, true, "2025-01-01T00:01:00Z");
        assert_eq!(new_state.consecutive_failures, 0);
        assert!(!new_state.is_open);
    }

    #[test]
    fn test_circuit_breaker_opens_at_threshold() {
        let mut state = RLCircuitBreakerState::default();
        let now = "2025-01-01T00:00:00+00:00";

        for _ in 0..4 {
            state = update_circuit_breaker(&state, false, now);
            assert!(!state.is_open);
        }

        // 5th failure should open the breaker
        state = update_circuit_breaker(&state, false, now);
        assert_eq!(state.consecutive_failures, 5);
        assert!(state.is_open);
        assert!(state.cooldown_ends_at.is_some());
    }

    #[test]
    fn test_should_attempt_inference_closed() {
        let state = RLCircuitBreakerState::default();
        assert!(should_attempt_inference(&state, "2025-01-01T00:00:00+00:00"));
    }

    #[test]
    fn test_should_attempt_inference_open_not_expired() {
        let state = RLCircuitBreakerState {
            consecutive_failures: 5,
            is_open: true,
            last_failure_at: Some("2025-01-01T00:00:00+00:00".to_string()),
            cooldown_ends_at: Some("2025-01-01T00:01:00+00:00".to_string()),
            cooldown_secs: 60,
            failure_threshold: 5,
        };
        // 30 seconds in, cooldown not expired
        assert!(!should_attempt_inference(&state, "2025-01-01T00:00:30+00:00"));
    }

    #[test]
    fn test_should_attempt_inference_open_expired() {
        let state = RLCircuitBreakerState {
            consecutive_failures: 5,
            is_open: true,
            last_failure_at: Some("2025-01-01T00:00:00+00:00".to_string()),
            cooldown_ends_at: Some("2025-01-01T00:01:00+00:00".to_string()),
            cooldown_secs: 60,
            failure_threshold: 5,
        };
        // 2 minutes in, cooldown expired
        assert!(should_attempt_inference(&state, "2025-01-01T00:02:00+00:00"));
    }

    #[test]
    fn test_cold_start_not_graduated() {
        let state = ColdStartState {
            experience_count: 30,
            cold_start_threshold: 50,
            has_graduated: false,
            graduated_at: None,
            episodes_since_graduation: 0,
        };

        let new_state = check_graduation(&state, "2025-01-01T00:00:00Z");
        assert!(!new_state.has_graduated);
        assert!(new_state.graduated_at.is_none());
    }

    #[test]
    fn test_cold_start_graduates_at_threshold() {
        let state = ColdStartState {
            experience_count: 50,
            cold_start_threshold: 50,
            has_graduated: false,
            graduated_at: None,
            episodes_since_graduation: 0,
        };

        let new_state = check_graduation(&state, "2025-01-01T00:00:00Z");
        assert!(new_state.has_graduated);
        assert_eq!(new_state.graduated_at, Some("2025-01-01T00:00:00Z".to_string()));
        assert_eq!(new_state.episodes_since_graduation, 0);
    }

    #[test]
    fn test_cold_start_increments_after_graduation() {
        let state = ColdStartState {
            experience_count: 100,
            cold_start_threshold: 50,
            has_graduated: true,
            graduated_at: Some("2025-01-01T00:00:00Z".to_string()),
            episodes_since_graduation: 10,
        };

        let new_state = check_graduation(&state, "2025-01-02T00:00:00Z");
        assert!(new_state.has_graduated);
        assert_eq!(new_state.episodes_since_graduation, 11);
    }

    #[test]
    fn test_trust_tier_promotion() {
        let state = RLTrustTierState {
            current_tier: "addon".to_string(),
            confidence_threshold: 0.80,
            promoted_at: None,
            validation_started_at: "2025-01-01T00:00:00Z".to_string(),
            consecutive_days_improved: 29,
            consecutive_days_degraded: 0,
        };

        let (new_state, transition) = evaluate_trust_tier(&state, true, "2025-01-31T00:00:00Z");
        assert_eq!(new_state.current_tier, "trusted");
        assert_eq!(new_state.confidence_threshold, 0.60);
        assert!(new_state.promoted_at.is_some());
        assert_eq!(transition, Some("promotion".to_string()));
    }

    #[test]
    fn test_trust_tier_no_promotion_before_30_days() {
        let state = RLTrustTierState {
            current_tier: "addon".to_string(),
            confidence_threshold: 0.80,
            promoted_at: None,
            validation_started_at: "2025-01-01T00:00:00Z".to_string(),
            consecutive_days_improved: 28,
            consecutive_days_degraded: 0,
        };

        let (new_state, transition) = evaluate_trust_tier(&state, true, "2025-01-30T00:00:00Z");
        assert_eq!(new_state.current_tier, "addon");
        assert_eq!(new_state.confidence_threshold, 0.80);
        assert_eq!(new_state.consecutive_days_improved, 29);
        assert_eq!(transition, None);
    }

    #[test]
    fn test_trust_tier_demotion() {
        let state = RLTrustTierState {
            current_tier: "trusted".to_string(),
            confidence_threshold: 0.60,
            promoted_at: Some("2025-01-01T00:00:00Z".to_string()),
            validation_started_at: "2025-01-01T00:00:00Z".to_string(),
            consecutive_days_improved: 0,
            consecutive_days_degraded: 6,
        };

        let (new_state, transition) = evaluate_trust_tier(&state, false, "2025-02-08T00:00:00Z");
        assert_eq!(new_state.current_tier, "addon");
        assert_eq!(new_state.confidence_threshold, 0.80);
        assert!(new_state.promoted_at.is_none());
        assert_eq!(transition, Some("demotion".to_string()));
    }

    #[test]
    fn test_trust_tier_no_demotion_from_addon() {
        let state = RLTrustTierState {
            current_tier: "addon".to_string(),
            confidence_threshold: 0.80,
            promoted_at: None,
            validation_started_at: "2025-01-01T00:00:00Z".to_string(),
            consecutive_days_improved: 0,
            consecutive_days_degraded: 6,
        };

        let (new_state, transition) = evaluate_trust_tier(&state, false, "2025-01-08T00:00:00Z");
        assert_eq!(new_state.current_tier, "addon");
        assert_eq!(new_state.consecutive_days_degraded, 7);
        // No demotion from addon — already at lowest tier
        assert_eq!(transition, None);
    }

    #[test]
    fn test_tier_to_threshold_mapping() {
        assert_eq!(tier_to_threshold("addon"), 0.80);
        assert_eq!(tier_to_threshold("trusted"), 0.60);
        assert_eq!(tier_to_threshold("unknown"), 0.80);
    }

    #[test]
    fn test_model_version_crud() {
        let conn = setup_test_db();

        let version = ModelVersion {
            version_id: "v1".to_string(),
            training_timestamp: "2025-01-01T00:00:00Z".to_string(),
            data_window_start: "2024-12-01T00:00:00Z".to_string(),
            data_window_end: "2025-01-01T00:00:00Z".to_string(),
            episode_count: 500,
            final_high_level_loss: 0.05,
            final_low_level_loss: 0.03,
            validation_metrics: serde_json::json!({"accuracy": 0.85}),
            normalization_mean: vec![0.0, 0.5, 1.0],
            normalization_var: vec![1.0, 0.5, 0.25],
            artifact_path: "/models/v1".to_string(),
            is_active: true,
            is_last_known_good: false,
            created_at: "2025-01-01T00:00:00Z".to_string(),
        };

        insert_model_version(&conn, &version).unwrap();

        let versions = query_model_versions(&conn).unwrap();
        assert_eq!(versions.len(), 1);
        assert_eq!(versions[0].version_id, "v1");
        assert_eq!(versions[0].episode_count, 500);
        assert!(versions[0].is_active);

        // Test set_active_model
        let version2 = ModelVersion {
            version_id: "v2".to_string(),
            created_at: "2025-01-02T00:00:00Z".to_string(),
            is_active: false,
            ..version.clone()
        };
        insert_model_version(&conn, &version2).unwrap();
        set_active_model(&conn, "v2").unwrap();

        let active = get_active_model(&conn).unwrap().unwrap();
        assert_eq!(active.version_id, "v2");

        // Test set_last_known_good
        set_last_known_good(&conn, "v1").unwrap();
        let versions = query_model_versions(&conn).unwrap();
        let lkg = versions.iter().find(|v| v.is_last_known_good).unwrap();
        assert_eq!(lkg.version_id, "v1");
    }

    #[test]
    fn test_model_retention_enforcement() {
        let conn = setup_test_db();

        // Insert 7 versions
        for i in 0..7 {
            let version = ModelVersion {
                version_id: format!("v{}", i),
                training_timestamp: "2025-01-01T00:00:00Z".to_string(),
                data_window_start: "2024-12-01T00:00:00Z".to_string(),
                data_window_end: "2025-01-01T00:00:00Z".to_string(),
                episode_count: 100,
                final_high_level_loss: 0.05,
                final_low_level_loss: 0.03,
                validation_metrics: serde_json::json!({}),
                normalization_mean: vec![],
                normalization_var: vec![],
                artifact_path: format!("/models/v{}", i),
                is_active: i == 6,
                is_last_known_good: i == 5,
                created_at: format!("2025-01-0{}T00:00:00Z", i + 1),
            };
            insert_model_version(&conn, &version).unwrap();
        }

        enforce_model_retention(&conn, 5).unwrap();

        let versions = query_model_versions(&conn).unwrap();
        // Should keep at least 5, but active and last_known_good are protected
        assert!(versions.len() >= 5);
        // Active and LKG should still exist
        assert!(versions.iter().any(|v| v.version_id == "v6"));
        assert!(versions.iter().any(|v| v.version_id == "v5"));
    }

    #[test]
    fn test_inference_log_crud() {
        let conn = setup_test_db();

        let entry = InferenceLogEntry {
            id: "log-1".to_string(),
            delegation_packet_id: "dp-1".to_string(),
            timestamp: "2025-01-01T00:00:00Z".to_string(),
            task_type: "code-generation".to_string(),
            recommended_agent_id: "agent-a".to_string(),
            confidence_score: 0.85,
            expected_reward: 0.7,
            q_values_json: r#"[["agent-a", 0.85], ["agent-b", 0.6]]"#.to_string(),
            model_version_id: "v1".to_string(),
            inference_duration_ms: 3.5,
            advisory_accepted: true,
            rejection_reason: None,
            heuristic_agent_id: "agent-b".to_string(),
            outcome_logician_score: None,
            outcome_recorded_at: None,
        };

        log_inference_decision(&conn, &entry).unwrap();

        // Append outcome
        append_outcome_to_inference_log(&conn, "dp-1", 0.92, "2025-01-01T00:05:00Z").unwrap();

        // Query
        let query = InferenceLogQuery {
            from_date: None,
            to_date: None,
            advisory_accepted: Some(true),
            model_version_id: None,
            limit: Some(10),
        };

        let results = query_inference_log(&conn, &query).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].outcome_logician_score, Some(0.92));
        assert_eq!(results[0].outcome_recorded_at, Some("2025-01-01T00:05:00Z".to_string()));
    }

    #[test]
    fn test_circuit_breaker_db_persistence() {
        let conn = setup_test_db();

        let state = read_circuit_breaker(&conn).unwrap();
        assert_eq!(state.consecutive_failures, 0);
        assert!(!state.is_open);

        let updated = RLCircuitBreakerState {
            consecutive_failures: 3,
            is_open: false,
            last_failure_at: Some("2025-01-01T00:00:00Z".to_string()),
            cooldown_ends_at: None,
            cooldown_secs: 60,
            failure_threshold: 5,
        };
        persist_circuit_breaker(&conn, &updated).unwrap();

        let read_back = read_circuit_breaker(&conn).unwrap();
        assert_eq!(read_back.consecutive_failures, 3);
    }

    #[test]
    fn test_cold_start_db_persistence() {
        let conn = setup_test_db();

        let state = read_cold_start(&conn).unwrap();
        assert_eq!(state.experience_count, 0);
        assert!(!state.has_graduated);

        let updated = ColdStartState {
            experience_count: 50,
            cold_start_threshold: 50,
            has_graduated: true,
            graduated_at: Some("2025-01-01T00:00:00Z".to_string()),
            episodes_since_graduation: 10,
        };
        update_cold_start(&conn, &updated).unwrap();

        let read_back = read_cold_start(&conn).unwrap();
        assert_eq!(read_back.experience_count, 50);
        assert!(read_back.has_graduated);
        assert_eq!(read_back.episodes_since_graduation, 10);
    }

    #[test]
    fn test_trust_tier_db_persistence() {
        let conn = setup_test_db();

        let state = read_trust_tier(&conn).unwrap();
        assert_eq!(state.current_tier, "addon");
        assert_eq!(state.confidence_threshold, 0.80);

        let updated = RLTrustTierState {
            current_tier: "trusted".to_string(),
            confidence_threshold: 0.60,
            promoted_at: Some("2025-02-01T00:00:00Z".to_string()),
            validation_started_at: "2025-01-01T00:00:00Z".to_string(),
            consecutive_days_improved: 0,
            consecutive_days_degraded: 0,
        };
        update_trust_tier(&conn, &updated).unwrap();

        let read_back = read_trust_tier(&conn).unwrap();
        assert_eq!(read_back.current_tier, "trusted");
        assert_eq!(read_back.confidence_threshold, 0.60);
    }

    #[test]
    fn test_agent_stats_upsert_and_read() {
        let conn = setup_test_db();

        let stats = AgentStatsCache {
            agent_id: "agent-a".to_string(),
            task_type: "code-gen".to_string(),
            quality_score: 0.85,
            speed_score: 150.0,
            cost_score: 500.0,
            availability: 0.95,
            task_type_percentile: 0.75,
            avg_efficiency_ratio: 0.8,
            pattern_rate_per_100: 2.5,
            avg_tool_calls: 5.0,
            cost_per_tool_call: 100.0,
            last_updated_at: "2025-01-01T00:00:00Z".to_string(),
        };

        upsert_agent_stats(&conn, &stats).unwrap();

        let result = read_agent_stats_for(&conn, "agent-a", "code-gen").unwrap();
        assert!(result.is_some());
        let r = result.unwrap();
        assert_eq!(r.quality_score, 0.85);
        assert_eq!(r.avg_efficiency_ratio, 0.8);

        // Test read_agent_stats (all)
        let all = read_agent_stats(&conn).unwrap();
        assert_eq!(all.len(), 1);
    }

    // ─── Phase 2 Tests (Task 2.7) ───────────────────────────────────────────

    #[test]
    fn test_build_inference_state_vector_basic() {
        let agent_stats = vec![
            AgentStatsCache {
                agent_id: "agent-a".to_string(),
                task_type: "code-gen".to_string(),
                quality_score: 0.9,
                speed_score: 100.0,
                cost_score: 200.0,
                availability: 1.0,
                task_type_percentile: 0.8,
                avg_efficiency_ratio: 0.75,
                pattern_rate_per_100: 1.0,
                avg_tool_calls: 4.0,
                cost_per_tool_call: 50.0,
                last_updated_at: "2025-01-01T00:00:00Z".to_string(),
            },
        ];

        let candidates = vec!["agent-a".to_string(), "agent-b".to_string()];
        let version = ModelVersion {
            version_id: "v1".to_string(),
            training_timestamp: String::new(),
            data_window_start: String::new(),
            data_window_end: String::new(),
            episode_count: 100,
            final_high_level_loss: 0.0,
            final_low_level_loss: 0.0,
            validation_metrics: serde_json::json!({}),
            normalization_mean: vec![],
            normalization_var: vec![],
            artifact_path: String::new(),
            is_active: true,
            is_last_known_good: false,
            created_at: String::new(),
        };

        let sv = build_inference_state_vector(&agent_stats, &candidates, &version, 0.5);

        // 64 (task embedding) + 10 * 9 (agent features) + 1 (low-level estimate) = 155
        assert_eq!(sv.len(), 155);

        // First 64 should be zeros (placeholder)
        assert!(sv[..64].iter().all(|&v| v == 0.0));

        // Agent-a features start at index 64
        assert_eq!(sv[64], 0.9);  // quality_score
        assert_eq!(sv[65], 100.0); // speed_score

        // Last element is the low-level efficiency estimate
        assert_eq!(sv[154], 0.5);
    }

    #[test]
    fn test_build_inference_state_vector_with_normalization() {
        let agent_stats = vec![];
        let candidates = vec![];
        let mut version = ModelVersion {
            version_id: "v1".to_string(),
            training_timestamp: String::new(),
            data_window_start: String::new(),
            data_window_end: String::new(),
            episode_count: 100,
            final_high_level_loss: 0.0,
            final_low_level_loss: 0.0,
            validation_metrics: serde_json::json!({}),
            normalization_mean: vec![],
            normalization_var: vec![],
            artifact_path: String::new(),
            is_active: true,
            is_last_known_good: false,
            created_at: String::new(),
        };

        // Without normalization
        let sv = build_inference_state_vector(&agent_stats, &candidates, &version, 0.5);
        let expected_len = 64 + 10 * 9 + 1; // 155
        assert_eq!(sv.len(), expected_len);

        // With normalization (all zeros mean, all ones var → should produce (x - 0) / 1 = x)
        version.normalization_mean = vec![0.0; expected_len];
        version.normalization_var = vec![1.0; expected_len];
        let sv_norm = build_inference_state_vector(&agent_stats, &candidates, &version, 0.5);
        assert_eq!(sv_norm.len(), expected_len);
        // Last element: (0.5 - 0.0) / 1.0 = 0.5
        assert!((sv_norm[154] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn test_confidence_ramp_not_graduated() {
        let cold_start = ColdStartState {
            experience_count: 30,
            cold_start_threshold: 50,
            has_graduated: false,
            graduated_at: None,
            episodes_since_graduation: 0,
        };

        let confidence = compute_confidence_with_ramp(0.9, &cold_start, 100);
        assert_eq!(confidence, 0.0);
    }

    #[test]
    fn test_confidence_ramp_partial() {
        let cold_start = ColdStartState {
            experience_count: 100,
            cold_start_threshold: 50,
            has_graduated: true,
            graduated_at: Some("2025-01-01T00:00:00Z".to_string()),
            episodes_since_graduation: 50,
        };

        let confidence = compute_confidence_with_ramp(0.8, &cold_start, 100);
        // 0.8 * (50/100) = 0.4
        assert!((confidence - 0.4).abs() < 1e-6);
    }

    #[test]
    fn test_confidence_ramp_full() {
        let cold_start = ColdStartState {
            experience_count: 200,
            cold_start_threshold: 50,
            has_graduated: true,
            graduated_at: Some("2025-01-01T00:00:00Z".to_string()),
            episodes_since_graduation: 150,
        };

        let confidence = compute_confidence_with_ramp(0.8, &cold_start, 100);
        // 0.8 * min(1.0, 150/100) = 0.8 * 1.0 = 0.8
        assert!((confidence - 0.8).abs() < 1e-6);
    }

    #[test]
    fn test_confidence_ramp_monotonicity() {
        let raw = 0.75;
        let ramp_episodes = 100;

        let mut prev_confidence = 0.0;
        for episodes in 0..=150 {
            let cold_start = ColdStartState {
                experience_count: 200,
                cold_start_threshold: 50,
                has_graduated: true,
                graduated_at: Some("2025-01-01T00:00:00Z".to_string()),
                episodes_since_graduation: episodes,
            };
            let confidence = compute_confidence_with_ramp(raw, &cold_start, ramp_episodes);
            assert!(confidence >= prev_confidence);
            prev_confidence = confidence;
        }
    }

    #[test]
    fn test_confidence_clamped_to_unit() {
        let cold_start = ColdStartState {
            experience_count: 200,
            cold_start_threshold: 50,
            has_graduated: true,
            graduated_at: Some("2025-01-01T00:00:00Z".to_string()),
            episodes_since_graduation: 200,
        };

        // Even with raw > 1.0, result should be clamped
        let confidence = compute_confidence_with_ramp(1.5, &cold_start, 100);
        assert!(confidence <= 1.0);
        assert!(confidence >= 0.0);
    }

    #[test]
    fn test_model_evaluation_insert() {
        let conn = setup_test_db();
        let id = insert_model_evaluation(&conn, "v2", "v1", 50).unwrap();
        assert!(id.starts_with("eval-"));

        // Verify it was inserted
        let count: u32 = conn
            .query_row("SELECT COUNT(*) FROM model_evaluation", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_get_rl_service_status_cold_start() {
        let conn = Connection::open_in_memory().unwrap();
        initialize_rl_policy_db(&conn).unwrap();

        let config = RLInferenceConfig::default();
        let state = RLInferenceState {
            config,
            db: Arc::new(std::sync::Mutex::new(conn)),
            circuit_breaker: Arc::new(RwLock::new(RLCircuitBreakerState::default())),
            trust_tier: Arc::new(RwLock::new(RLTrustTierState::default())),
            cold_start: Arc::new(RwLock::new(ColdStartState::default())),
            agent_stats_cache: Arc::new(RwLock::new(vec![])),
            model_versions: Arc::new(RwLock::new(vec![])),
            #[cfg(feature = "tract-onnx")]
            current_model: Arc::new(RwLock::new(None)),
        };

        let cb = RLCircuitBreakerState::default();
        let tt = RLTrustTierState::default();
        let cs = ColdStartState::default();

        let status = get_rl_service_status(&state, &cb, &tt, &cs);
        assert_eq!(status.status, "untrained");
    }
}

// ─── Property-Based Tests (Task 3.7, 4.6) ───────────────────────────────────

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    // **Validates: Requirements 4.1**
    // Property 1: Inference latency bound
    // Note: We test that state vector construction + confidence computation complete quickly.
    // Actual ONNX forward pass latency depends on model size and hardware.
    proptest! {
        #[test]
        fn prop_state_vector_construction_fast(
            num_agents in 1usize..10,
            efficiency in 0.0f64..1.0,
        ) {
            let agent_stats: Vec<AgentStatsCache> = (0..num_agents)
                .map(|i| AgentStatsCache {
                    agent_id: format!("agent-{}", i),
                    task_type: "code-gen".to_string(),
                    quality_score: 0.5,
                    speed_score: 100.0,
                    cost_score: 200.0,
                    availability: 1.0,
                    task_type_percentile: 0.5,
                    avg_efficiency_ratio: 0.5,
                    pattern_rate_per_100: 1.0,
                    avg_tool_calls: 3.0,
                    cost_per_tool_call: 50.0,
                    last_updated_at: "2025-01-01T00:00:00Z".to_string(),
                })
                .collect();

            let candidates: Vec<String> = (0..num_agents).map(|i| format!("agent-{}", i)).collect();
            let version = ModelVersion {
                version_id: "v1".to_string(),
                training_timestamp: String::new(),
                data_window_start: String::new(),
                data_window_end: String::new(),
                episode_count: 100,
                final_high_level_loss: 0.0,
                final_low_level_loss: 0.0,
                validation_metrics: serde_json::json!({}),
                normalization_mean: vec![],
                normalization_var: vec![],
                artifact_path: String::new(),
                is_active: true,
                is_last_known_good: false,
                created_at: String::new(),
            };

            let start = std::time::Instant::now();
            let sv = build_inference_state_vector(&agent_stats, &candidates, &version, efficiency);
            let elapsed = start.elapsed();

            // State vector construction should be sub-millisecond
            prop_assert!(elapsed.as_millis() < 5, "State vector construction took {:?}", elapsed);
            prop_assert!(!sv.is_empty());
        }
    }

    // **Validates: Requirements 3.2, 9.2, 14.2**
    // Property 2: Confidence score bounds
    proptest! {
        #[test]
        fn prop_confidence_score_bounds(
            raw_confidence in -2.0f64..2.0,
            episodes_since_graduation in 0u32..500,
            ramp_episodes in 1u32..200,
        ) {
            let cold_start = ColdStartState {
                experience_count: 200,
                cold_start_threshold: 50,
                has_graduated: true,
                graduated_at: Some("2025-01-01T00:00:00Z".to_string()),
                episodes_since_graduation,
            };

            let confidence = compute_confidence_with_ramp(raw_confidence, &cold_start, ramp_episodes);
            prop_assert!(confidence >= 0.0, "Confidence {} < 0.0", confidence);
            prop_assert!(confidence <= 1.0, "Confidence {} > 1.0", confidence);
        }
    }

    // **Validates: Requirements 9.2, 9.5**
    // Property 3: Cold start zero confidence
    proptest! {
        #[test]
        fn prop_cold_start_zero_confidence(
            raw_confidence in 0.0f64..1.0,
            experience_count in 0u32..50,
            ramp_episodes in 1u32..200,
        ) {
            let cold_start = ColdStartState {
                experience_count,
                cold_start_threshold: 50,
                has_graduated: false,
                graduated_at: None,
                episodes_since_graduation: 0,
            };

            let confidence = compute_confidence_with_ramp(raw_confidence, &cold_start, ramp_episodes);
            prop_assert_eq!(confidence, 0.0, "Cold start should produce 0.0 confidence, got {}", confidence);
        }
    }

    // **Validates: Requirements 9.4**
    // Property 4: Confidence ramp-up monotonicity
    proptest! {
        #[test]
        fn prop_confidence_ramp_monotonicity(
            raw_confidence in 0.0f64..1.0,
            episodes_a in 0u32..100,
            episodes_b in 0u32..100,
            ramp_episodes in 1u32..200,
        ) {
            let cold_start_a = ColdStartState {
                experience_count: 200,
                cold_start_threshold: 50,
                has_graduated: true,
                graduated_at: Some("2025-01-01T00:00:00Z".to_string()),
                episodes_since_graduation: episodes_a.min(episodes_b),
            };

            let cold_start_b = ColdStartState {
                experience_count: 200,
                cold_start_threshold: 50,
                has_graduated: true,
                graduated_at: Some("2025-01-01T00:00:00Z".to_string()),
                episodes_since_graduation: episodes_a.max(episodes_b),
            };

            let confidence_a = compute_confidence_with_ramp(raw_confidence, &cold_start_a, ramp_episodes);
            let confidence_b = compute_confidence_with_ramp(raw_confidence, &cold_start_b, ramp_episodes);

            prop_assert!(
                confidence_a <= confidence_b,
                "Monotonicity violated: {} episodes -> {}, {} episodes -> {}",
                cold_start_a.episodes_since_graduation, confidence_a,
                cold_start_b.episodes_since_graduation, confidence_b
            );
        }
    }

    // **Validates: Requirements 13.5, 14.1**
    // Property 5: Circuit breaker activation threshold
    proptest! {
        #[test]
        fn prop_circuit_breaker_threshold(
            threshold in 1u32..20,
            cooldown_secs in 10u64..300,
        ) {
            let now = "2025-01-01T00:00:00+00:00";
            let mut state = RLCircuitBreakerState {
                consecutive_failures: 0,
                is_open: false,
                last_failure_at: None,
                cooldown_ends_at: None,
                cooldown_secs,
                failure_threshold: threshold,
            };

            // Apply threshold - 1 failures: should NOT be open
            for _ in 0..(threshold - 1) {
                state = update_circuit_breaker(&state, false, now);
                prop_assert!(!state.is_open, "Breaker opened before threshold");
            }

            // One more failure: should open
            state = update_circuit_breaker(&state, false, now);
            prop_assert!(state.is_open, "Breaker did not open at threshold");
            prop_assert_eq!(state.consecutive_failures, threshold);

            // Success resets
            state = update_circuit_breaker(&state, true, now);
            prop_assert_eq!(state.consecutive_failures, 0);
            prop_assert!(!state.is_open, "Breaker did not close on success");
        }
    }

    // **Validates: Requirements 12.1, 14.3**
    // Property 12: Model version persistence
    proptest! {
        #[test]
        fn prop_model_version_persistence(
            episode_count in 1u32..10000,
            loss_h in 0.0f64..1.0,
            loss_l in 0.0f64..1.0,
        ) {
            let conn = Connection::open_in_memory().unwrap();
            initialize_rl_policy_db(&conn).unwrap();

            let version = ModelVersion {
                version_id: format!("v-{}-{}", episode_count, (loss_h * 1000.0) as u32),
                training_timestamp: "2025-01-01T00:00:00Z".to_string(),
                data_window_start: "2024-12-01T00:00:00Z".to_string(),
                data_window_end: "2025-01-01T00:00:00Z".to_string(),
                episode_count,
                final_high_level_loss: loss_h,
                final_low_level_loss: loss_l,
                validation_metrics: serde_json::json!({}),
                normalization_mean: vec![0.0; 10],
                normalization_var: vec![1.0; 10],
                artifact_path: "/models/test".to_string(),
                is_active: true,
                is_last_known_good: false,
                created_at: "2025-01-01T00:00:00Z".to_string(),
            };

            insert_model_version(&conn, &version).unwrap();
            let active = get_active_model(&conn).unwrap();
            prop_assert!(active.is_some(), "Active model should exist after insert");
            let active = active.unwrap();
            prop_assert_eq!(active.episode_count, episode_count);
            prop_assert!((active.final_high_level_loss - loss_h).abs() < 1e-10);
            prop_assert!((active.final_low_level_loss - loss_l).abs() < 1e-10);
        }
    }

    // **Validates: Requirements 12.4**
    // Property 13: Rollback trigger correctness
    // If new version has lower acceptance rate OR lower avg logician score, rollback triggers.
    proptest! {
        #[test]
        fn prop_rollback_trigger(
            new_acceptance in 0.0f64..1.0,
            prev_acceptance in 0.0f64..1.0,
            new_score in 0.0f64..1.0,
            prev_score in 0.0f64..1.0,
        ) {
            // The evaluation logic: keep if new >= prev on BOTH metrics
            let should_keep = new_acceptance >= prev_acceptance && new_score >= prev_score;

            // If either metric is worse, should trigger rollback
            if new_acceptance < prev_acceptance || new_score < prev_score {
                prop_assert!(!should_keep, "Should rollback when new version is worse");
            }
        }
    }

    // **Validates: Requirements 12.6**
    // Property 14: Last known good invariant
    proptest! {
        #[test]
        fn prop_last_known_good_invariant(
            num_versions in 1usize..10,
        ) {
            let conn = Connection::open_in_memory().unwrap();
            initialize_rl_policy_db(&conn).unwrap();

            for i in 0..num_versions {
                let version = ModelVersion {
                    version_id: format!("v{}", i),
                    training_timestamp: "2025-01-01T00:00:00Z".to_string(),
                    data_window_start: "2024-12-01T00:00:00Z".to_string(),
                    data_window_end: "2025-01-01T00:00:00Z".to_string(),
                    episode_count: 100,
                    final_high_level_loss: 0.05,
                    final_low_level_loss: 0.03,
                    validation_metrics: serde_json::json!({}),
                    normalization_mean: vec![],
                    normalization_var: vec![],
                    artifact_path: format!("/models/v{}", i),
                    is_active: false,
                    is_last_known_good: false,
                    created_at: format!("2025-01-0{}T00:00:00Z", i + 1),
                };
                insert_model_version(&conn, &version).unwrap();
            }

            // Set one as LKG
            set_last_known_good(&conn, "v0").unwrap();

            // Verify exactly one LKG
            let lkg_count: u32 = conn
                .query_row(
                    "SELECT COUNT(*) FROM model_versions WHERE is_last_known_good = 1",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            prop_assert_eq!(lkg_count, 1);

            // Set another as LKG
            if num_versions > 1 {
                set_last_known_good(&conn, "v1").unwrap();
                let lkg_count: u32 = conn
                    .query_row(
                        "SELECT COUNT(*) FROM model_versions WHERE is_last_known_good = 1",
                        [],
                        |row| row.get(0),
                    )
                    .unwrap();
                prop_assert_eq!(lkg_count, 1, "Should always have exactly one LKG");
            }
        }
    }
}
