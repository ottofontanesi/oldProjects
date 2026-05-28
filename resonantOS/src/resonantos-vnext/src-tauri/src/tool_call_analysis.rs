//! Tool Call Analysis
//!
//! Pure functions for efficiency ratio computation, sequence pattern detection,
//! anomaly flagging. Runs as a tokio::spawn background task after task completion.

use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::tool_call_tracker_service::{ToolCallRecord, ToolCallTrackerConfig};

// ─── Phase 4: Efficiency Classification Types ───────────────────────────────

/// Classification of a single tool call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CallClassification {
    Useful,
    Redundant,
}

/// The complete analysis result for a task.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisResult {
    pub delegation_packet_id: String,
    pub agent_id: String,
    pub task_type: String,
    pub efficiency_ratio: f64,
    pub total_calls: u32,
    pub useful_calls: u32,
    pub redundant_calls: u32,
    pub detected_patterns: Vec<SequencePattern>,
    pub anomaly_flags: Vec<AnomalyFlag>,
    pub tool_sequence_signature: Vec<String>,
    pub analyzed_at: String,
}

/// The trace summary appended to an ExperienceRecord.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallTraceSummary {
    pub delegation_packet_id: String,
    pub efficiency_ratio: f64,
    pub total_calls: u32,
    pub useful_calls: u32,
    pub redundant_calls: u32,
    pub detected_patterns: Vec<SequencePattern>,
    pub tool_sequence_signature: Vec<String>,
    pub analyzed_at: String,
}

// ─── Phase 4: Efficiency Classification ─────────────────────────────────────

/// Determine the index of the tool call that produced the final expected artifact.
/// Returns None if no artifact production is detected.
pub fn find_final_artifact_index(
    records: &[ToolCallRecord],
    expected_artifacts: &[String],
) -> Option<usize> {
    if expected_artifacts.is_empty() {
        return None;
    }

    let mut last_artifact_idx: Option<usize> = None;

    for (i, record) in records.iter().enumerate() {
        if let Some(ref output) = record.output_summary {
            let output_lower = output.to_lowercase();
            for artifact in expected_artifacts {
                if output_lower.contains(&artifact.to_lowercase()) {
                    last_artifact_idx = Some(i);
                }
            }
        }
        // Also check if the tool name suggests artifact creation
        let tool_lower = record.tool_name.to_lowercase();
        if tool_lower.contains("write") || tool_lower.contains("create") || tool_lower.contains("save") {
            if record.success {
                // Check if input params reference an expected artifact
                let params_lower = record.input_params_json.to_lowercase();
                for artifact in expected_artifacts {
                    if params_lower.contains(&artifact.to_lowercase()) {
                        last_artifact_idx = Some(i);
                    }
                }
            }
        }
    }

    last_artifact_idx
}

/// Classify a tool call record within the context of the full trace.
pub fn classify_tool_call(
    record: &ToolCallRecord,
    index: usize,
    prior_records: &[ToolCallRecord],
    expected_artifacts: &[String],
    final_artifact_index: Option<usize>,
) -> CallClassification {
    // Check post-answer: if this call is after the final artifact was produced
    if let Some(final_idx) = final_artifact_index {
        if index > final_idx {
            return CallClassification::Redundant;
        }
    }

    // Check duplicate: same tool + same params as a prior call with same output
    for prior in prior_records {
        if prior.tool_name == record.tool_name
            && prior.input_params_json == record.input_params_json
            && prior.output_summary == record.output_summary
            && prior.success == record.success
        {
            return CallClassification::Redundant;
        }
    }

    // Check state-change indicators (write/create/modify/delete in tool name or output)
    let tool_lower = record.tool_name.to_lowercase();
    if tool_lower.contains("write")
        || tool_lower.contains("create")
        || tool_lower.contains("modify")
        || tool_lower.contains("delete")
        || tool_lower.contains("update")
        || tool_lower.contains("save")
    {
        if record.success {
            return CallClassification::Useful;
        }
    }

    // Check artifact contribution
    if let Some(ref output) = record.output_summary {
        let output_lower = output.to_lowercase();
        for artifact in expected_artifacts {
            if output_lower.contains(&artifact.to_lowercase()) {
                return CallClassification::Useful;
            }
        }
    }

    // Check if input params reference expected artifacts
    let params_lower = record.input_params_json.to_lowercase();
    for artifact in expected_artifacts {
        if params_lower.contains(&artifact.to_lowercase()) {
            return CallClassification::Useful;
        }
    }

    // If the tool returned new information not in prior outputs, it's useful
    if record.success {
        if let Some(ref output) = record.output_summary {
            if !output.is_empty() {
                let output_already_seen = prior_records.iter().any(|prior| {
                    prior.output_summary.as_deref() == Some(output.as_str())
                });
                if !output_already_seen {
                    return CallClassification::Useful;
                }
            }
        }
    }

    // Default: if successful with no output or output already seen
    if record.success && record.output_summary.is_none() {
        // Successful call with no output — could be a side-effect operation
        return CallClassification::Useful;
    }

    CallClassification::Redundant
}

/// Compute efficiency ratio for a complete trace.
/// Returns 1.0 for empty traces (no tools needed = no waste).
pub fn compute_efficiency_ratio(
    records: &[ToolCallRecord],
    expected_artifacts: &[String],
) -> f64 {
    if records.is_empty() {
        return 1.0;
    }

    let final_artifact_idx = find_final_artifact_index(records, expected_artifacts);
    let useful_count = records
        .iter()
        .enumerate()
        .filter(|(i, r)| {
            classify_tool_call(r, *i, &records[..*i], expected_artifacts, final_artifact_idx)
                == CallClassification::Useful
        })
        .count();

    useful_count as f64 / records.len() as f64
}

// ─── Phase 5: Sequence Pattern Detection ────────────────────────────────────

/// Types of anti-patterns detected in tool call sequences.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PatternType {
    RepeatedIdenticalCalls,
    AlwaysFailingCalls,
    PostAnswerCalls,
    UnnecessaryPermissionChecks,
}

/// A detected sequence pattern with evidence.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SequencePattern {
    pub pattern_type: PatternType,
    pub offending_indices: Vec<usize>,
    pub description: String,
}

/// Detect all anti-patterns in a completed tool call trace.
pub fn detect_patterns(
    records: &[ToolCallRecord],
    expected_artifacts: &[String],
    allowed_tools: &[String],
    capability_grants: &[String],
) -> Vec<SequencePattern> {
    let mut patterns = Vec::new();
    patterns.extend(detect_repeated_identical(records));
    patterns.extend(detect_always_failing(records));
    patterns.extend(detect_post_answer(records, expected_artifacts));
    patterns.extend(detect_unnecessary_permission_checks(
        records,
        allowed_tools,
        capability_grants,
    ));

    // Deduplicate overlapping indices across patterns of the same type
    deduplicate_patterns(&mut patterns);
    patterns
}

/// Detect consecutive identical calls (same tool + same params, 2+ times).
pub fn detect_repeated_identical(records: &[ToolCallRecord]) -> Vec<SequencePattern> {
    let mut patterns = Vec::new();

    if records.len() < 2 {
        return patterns;
    }

    let mut i = 0;
    while i < records.len() {
        let mut group = vec![i];
        let mut j = i + 1;

        while j < records.len()
            && records[j].tool_name == records[i].tool_name
            && records[j].input_params_json == records[i].input_params_json
        {
            group.push(j);
            j += 1;
        }

        if group.len() >= 2 {
            patterns.push(SequencePattern {
                pattern_type: PatternType::RepeatedIdenticalCalls,
                offending_indices: group.clone(),
                description: format!(
                    "Tool '{}' called {} consecutive times with identical parameters",
                    records[i].tool_name,
                    group.len()
                ),
            });
        }

        i = j;
    }

    patterns
}

/// Detect tools invoked 3+ times in the trace that fail every time.
pub fn detect_always_failing(records: &[ToolCallRecord]) -> Vec<SequencePattern> {
    use std::collections::HashMap;

    let mut tool_invocations: HashMap<&str, Vec<usize>> = HashMap::new();

    for (i, record) in records.iter().enumerate() {
        tool_invocations
            .entry(&record.tool_name)
            .or_default()
            .push(i);
    }

    let mut patterns = Vec::new();

    for (tool_name, indices) in &tool_invocations {
        if indices.len() >= 3 {
            let all_failed = indices
                .iter()
                .all(|&idx| !records[idx].success);

            if all_failed {
                patterns.push(SequencePattern {
                    pattern_type: PatternType::AlwaysFailingCalls,
                    offending_indices: indices.clone(),
                    description: format!(
                        "Tool '{}' invoked {} times, all invocations failed",
                        tool_name,
                        indices.len()
                    ),
                });
            }
        }
    }

    patterns
}

/// Detect tool calls after the final artifact was produced.
pub fn detect_post_answer(
    records: &[ToolCallRecord],
    expected_artifacts: &[String],
) -> Vec<SequencePattern> {
    let final_idx = find_final_artifact_index(records, expected_artifacts);

    match final_idx {
        Some(idx) if idx < records.len() - 1 => {
            let offending: Vec<usize> = ((idx + 1)..records.len()).collect();
            if offending.is_empty() {
                return Vec::new();
            }
            vec![SequencePattern {
                pattern_type: PatternType::PostAnswerCalls,
                offending_indices: offending.clone(),
                description: format!(
                    "{} tool call(s) made after the final artifact was produced at index {}",
                    offending.len(),
                    idx
                ),
            }]
        }
        _ => Vec::new(),
    }
}

/// Detect permission/capability queries for things already granted.
pub fn detect_unnecessary_permission_checks(
    records: &[ToolCallRecord],
    allowed_tools: &[String],
    capability_grants: &[String],
) -> Vec<SequencePattern> {
    // Known permission-check tool name patterns
    let permission_check_patterns = [
        "check_permission",
        "query_capability",
        "get_permissions",
        "list_capabilities",
        "verify_access",
        "check_access",
        "has_permission",
        "can_access",
    ];

    let mut offending_indices = Vec::new();

    for (i, record) in records.iter().enumerate() {
        let tool_lower = record.tool_name.to_lowercase();

        let is_permission_check = permission_check_patterns
            .iter()
            .any(|pattern| tool_lower.contains(pattern));

        if is_permission_check {
            // Check if the queried permission/capability is already granted
            let params_lower = record.input_params_json.to_lowercase();

            let already_granted = allowed_tools.iter().any(|tool| {
                params_lower.contains(&tool.to_lowercase())
            }) || capability_grants.iter().any(|cap| {
                params_lower.contains(&cap.to_lowercase())
            });

            if already_granted {
                offending_indices.push(i);
            }
        }
    }

    if offending_indices.is_empty() {
        Vec::new()
    } else {
        vec![SequencePattern {
            pattern_type: PatternType::UnnecessaryPermissionChecks,
            offending_indices: offending_indices.clone(),
            description: format!(
                "{} unnecessary permission check(s) for already-granted capabilities",
                offending_indices.len()
            ),
        }]
    }
}

/// Deduplicate patterns with overlapping indices of the same type.
fn deduplicate_patterns(patterns: &mut Vec<SequencePattern>) {
    // Simple dedup: remove patterns of the same type whose indices are a subset of another
    let len = patterns.len();
    let mut to_remove = vec![false; len];

    for i in 0..len {
        if to_remove[i] {
            continue;
        }
        for j in (i + 1)..len {
            if to_remove[j] {
                continue;
            }
            if patterns[i].pattern_type == patterns[j].pattern_type {
                let i_set: std::collections::HashSet<usize> =
                    patterns[i].offending_indices.iter().copied().collect();
                let j_set: std::collections::HashSet<usize> =
                    patterns[j].offending_indices.iter().copied().collect();

                if j_set.is_subset(&i_set) {
                    to_remove[j] = true;
                } else if i_set.is_subset(&j_set) {
                    to_remove[i] = true;
                    break;
                }
            }
        }
    }

    let mut idx = 0;
    patterns.retain(|_| {
        let keep = !to_remove[idx];
        idx += 1;
        keep
    });
}

// ─── Phase 6: Anomaly Detection ─────────────────────────────────────────────

/// Reason for an anomaly flag.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AnomalyReason {
    LowEfficiency,
    ExcessiveCalls,
    Both,
}

/// An anomaly flag applied to a task.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnomalyFlag {
    pub reason: AnomalyReason,
    pub efficiency_ratio: f64,
    pub efficiency_threshold: f64,
    pub total_calls: u32,
    pub historical_avg_calls: f64,
    pub historical_avg_multiplier: f64,
    pub flagged_at: String,
}

/// Check if a task should be flagged as anomalous.
pub fn check_anomaly(
    efficiency_ratio: f64,
    total_calls: u32,
    historical_avg_calls: f64,
    config: &ToolCallTrackerConfig,
) -> Option<AnomalyFlag> {
    let low_efficiency = efficiency_ratio < config.efficiency_threshold;
    let excessive_calls =
        (total_calls as f64) > historical_avg_calls * config.historical_avg_multiplier;

    let reason = match (low_efficiency, excessive_calls) {
        (true, true) => AnomalyReason::Both,
        (true, false) => AnomalyReason::LowEfficiency,
        (false, true) => AnomalyReason::ExcessiveCalls,
        (false, false) => return None,
    };

    Some(AnomalyFlag {
        reason,
        efficiency_ratio,
        efficiency_threshold: config.efficiency_threshold,
        total_calls,
        historical_avg_calls,
        historical_avg_multiplier: config.historical_avg_multiplier,
        flagged_at: Utc::now().to_rfc3339(),
    })
}

/// Update the rolling average for a task type (pure computation).
/// Returns (new_avg_calls, new_avg_efficiency, new_sample_count).
pub fn update_task_type_average(
    current_avg_calls: f64,
    current_avg_efficiency: f64,
    current_sample_count: u32,
    new_total_calls: u32,
    new_efficiency: f64,
    window_size: u32,
) -> (f64, f64, u32) {
    let effective_window = current_sample_count.min(window_size) as f64;
    let new_avg_calls =
        (current_avg_calls * effective_window + new_total_calls as f64) / (effective_window + 1.0);
    let new_avg_efficiency =
        (current_avg_efficiency * effective_window + new_efficiency) / (effective_window + 1.0);
    let new_sample_count = current_sample_count + 1;

    (new_avg_calls, new_avg_efficiency, new_sample_count)
}

/// Update aggregate stats (pure computation).
/// Returns (new_avg_efficiency, new_avg_calls, new_total_tasks).
pub fn update_aggregate_stats(
    current_avg_efficiency: f64,
    current_avg_calls: f64,
    current_total_tasks: u32,
    new_efficiency: f64,
    new_total_calls: u32,
) -> (f64, f64, u32) {
    let n = current_total_tasks as f64;
    let new_avg_eff = (current_avg_efficiency * n + new_efficiency) / (n + 1.0);
    let new_avg_calls_val = (current_avg_calls * n + new_total_calls as f64) / (n + 1.0);
    let new_total = current_total_tasks + 1;

    (new_avg_eff, new_avg_calls_val, new_total)
}

/// Query anomaly-flagged tasks within a time window.
/// This is a DB query helper — returns task analysis results with non-null anomaly flags.
pub fn query_anomaly_flagged_tasks(
    conn: &rusqlite::Connection,
    from: &str,
    to: &str,
) -> Result<Vec<TaskAnalysisResult>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT delegation_packet_id, agent_id, task_type, efficiency_ratio,
             total_calls, useful_calls, redundant_calls, detected_patterns_json,
             anomaly_flags_json, tool_sequence_signature_json, analyzed_at
             FROM task_analysis_results
             WHERE anomaly_flags_json IS NOT NULL
             AND analyzed_at >= ?1 AND analyzed_at <= ?2
             ORDER BY analyzed_at DESC",
        )
        .map_err(|e| format!("Failed to prepare anomaly query: {}", e))?;

    let rows = stmt
        .query_map(rusqlite::params![from, to], |row| {
            Ok(TaskAnalysisResult {
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
            })
        })
        .map_err(|e| format!("Failed to query anomaly tasks: {}", e))?;

    let mut results = Vec::new();
    for row in rows {
        results.push(row.map_err(|e| format!("Failed to read anomaly row: {}", e))?);
    }
    Ok(results)
}

/// Task analysis result row (for query results).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskAnalysisResult {
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
}

// ─── Phase 7: Analysis Orchestrator (inner logic) ───────────────────────────

/// Inner analysis logic (pure function, no DB access).
/// Called by the service layer's analyze_completed_task.
pub fn analyze_completed_task_inner(
    records: &[ToolCallRecord],
    delegation_packet_id: &str,
    agent_id: &str,
    task_type: &str,
    expected_artifacts: &[String],
    allowed_tools: &[String],
    capability_grants: &[String],
    config: &ToolCallTrackerConfig,
) -> AnalysisResult {
    // 1. Compute efficiency ratio
    let efficiency_ratio = compute_efficiency_ratio(records, expected_artifacts);

    // 2. Classify each record
    let final_artifact_idx = find_final_artifact_index(records, expected_artifacts);
    let mut useful_calls = 0u32;
    let mut redundant_calls = 0u32;

    for (i, record) in records.iter().enumerate() {
        match classify_tool_call(record, i, &records[..i], expected_artifacts, final_artifact_idx) {
            CallClassification::Useful => useful_calls += 1,
            CallClassification::Redundant => redundant_calls += 1,
        }
    }

    // 3. Detect patterns
    let detected_patterns = detect_patterns(records, expected_artifacts, allowed_tools, capability_grants);

    // 4. Check anomaly (use 0.0 as historical avg if unknown — will be updated by caller)
    let anomaly_flags = check_anomaly(efficiency_ratio, records.len() as u32, 0.0, config)
        .map(|f| vec![f])
        .unwrap_or_default();

    // 5. Build tool sequence signature
    let tool_sequence_signature: Vec<String> = records.iter().map(|r| r.tool_name.clone()).collect();

    let analyzed_at = Utc::now().to_rfc3339();

    AnalysisResult {
        delegation_packet_id: delegation_packet_id.to_string(),
        agent_id: agent_id.to_string(),
        task_type: task_type.to_string(),
        efficiency_ratio,
        total_calls: records.len() as u32,
        useful_calls,
        redundant_calls,
        detected_patterns,
        anomaly_flags,
        tool_sequence_signature,
        analyzed_at,
    }
}

/// Build a ToolCallTraceSummary from an AnalysisResult.
pub fn build_trace_summary(result: &AnalysisResult) -> ToolCallTraceSummary {
    ToolCallTraceSummary {
        delegation_packet_id: result.delegation_packet_id.clone(),
        efficiency_ratio: result.efficiency_ratio,
        total_calls: result.total_calls,
        useful_calls: result.useful_calls,
        redundant_calls: result.redundant_calls,
        detected_patterns: result.detected_patterns.clone(),
        tool_sequence_signature: result.tool_sequence_signature.clone(),
        analyzed_at: result.analyzed_at.clone(),
    }
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn make_record(tool_name: &str, params: &str, output: Option<&str>, success: bool, seq: u32) -> ToolCallRecord {
        ToolCallRecord {
            id: format!("rec-{}", seq),
            delegation_packet_id: "packet-test".to_string(),
            agent_id: "agent-test".to_string(),
            task_type: "test_task".to_string(),
            tool_name: tool_name.to_string(),
            input_params_json: params.to_string(),
            output_summary: output.map(|s| s.to_string()),
            duration_ms: 10,
            success,
            timestamp: format!("2026-07-15T10:{:02}:00Z", seq),
            sequence_position: seq,
            prompt_tokens: None,
            completion_tokens: None,
            is_llm_backed: false,
        }
    }

    fn default_config() -> ToolCallTrackerConfig {
        ToolCallTrackerConfig::default()
    }

    // ─── Unit Tests: Efficiency ─────────────────────────────────────────────

    #[test]
    fn test_empty_trace_efficiency_is_one() {
        let ratio = compute_efficiency_ratio(&[], &[]);
        assert!((ratio - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_single_useful_call() {
        let records = vec![make_record("write_file", r#"{"path":"out.txt"}"#, Some("file written"), true, 1)];
        let ratio = compute_efficiency_ratio(&records, &["out.txt".to_string()]);
        assert!((ratio - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_all_redundant_calls() {
        // Same tool, same params, same output — duplicates
        let records = vec![
            make_record("read_file", r#"{"path":"a.txt"}"#, Some("content"), true, 1),
            make_record("read_file", r#"{"path":"a.txt"}"#, Some("content"), true, 2),
            make_record("read_file", r#"{"path":"a.txt"}"#, Some("content"), true, 3),
        ];
        let ratio = compute_efficiency_ratio(&records, &[]);
        // First call is useful (new info), rest are redundant
        assert!((ratio - 1.0 / 3.0).abs() < 0.01);
    }

    #[test]
    fn test_find_final_artifact_index() {
        let records = vec![
            make_record("read_file", "{}", Some("reading"), true, 1),
            make_record("write_file", r#"{"path":"result.json"}"#, Some("wrote result.json"), true, 2),
            make_record("read_file", "{}", Some("extra read"), true, 3),
        ];
        let idx = find_final_artifact_index(&records, &["result.json".to_string()]);
        assert_eq!(idx, Some(1));
    }

    // ─── Unit Tests: Pattern Detection ──────────────────────────────────────

    #[test]
    fn test_detect_repeated_identical() {
        let records = vec![
            make_record("read_file", r#"{"path":"a.txt"}"#, Some("content"), true, 1),
            make_record("read_file", r#"{"path":"a.txt"}"#, Some("content"), true, 2),
            make_record("read_file", r#"{"path":"a.txt"}"#, Some("content"), true, 3),
            make_record("write_file", r#"{"path":"b.txt"}"#, Some("done"), true, 4),
        ];
        let patterns = detect_repeated_identical(&records);
        assert_eq!(patterns.len(), 1);
        assert_eq!(patterns[0].pattern_type, PatternType::RepeatedIdenticalCalls);
        assert_eq!(patterns[0].offending_indices, vec![0, 1, 2]);
    }

    #[test]
    fn test_detect_always_failing() {
        let records = vec![
            make_record("api_call", r#"{"url":"http://x"}"#, None, false, 1),
            make_record("api_call", r#"{"url":"http://y"}"#, None, false, 2),
            make_record("api_call", r#"{"url":"http://z"}"#, None, false, 3),
            make_record("read_file", "{}", Some("ok"), true, 4),
        ];
        let patterns = detect_always_failing(&records);
        assert_eq!(patterns.len(), 1);
        assert_eq!(patterns[0].pattern_type, PatternType::AlwaysFailingCalls);
        assert_eq!(patterns[0].offending_indices, vec![0, 1, 2]);
    }

    #[test]
    fn test_detect_post_answer() {
        let records = vec![
            make_record("read_file", "{}", Some("data"), true, 1),
            make_record("write_file", r#"{"path":"output.txt"}"#, Some("wrote output.txt"), true, 2),
            make_record("read_file", "{}", Some("extra"), true, 3),
            make_record("list_dir", "{}", Some("files"), true, 4),
        ];
        let patterns = detect_post_answer(&records, &["output.txt".to_string()]);
        assert_eq!(patterns.len(), 1);
        assert_eq!(patterns[0].pattern_type, PatternType::PostAnswerCalls);
        assert_eq!(patterns[0].offending_indices, vec![2, 3]);
    }

    #[test]
    fn test_detect_unnecessary_permission_checks() {
        let records = vec![
            make_record("check_permission", r#"{"tool":"write_file"}"#, Some("granted"), true, 1),
            make_record("write_file", r#"{"path":"out.txt"}"#, Some("done"), true, 2),
        ];
        let allowed = vec!["write_file".to_string()];
        let caps = vec![];
        let patterns = detect_unnecessary_permission_checks(&records, &allowed, &caps);
        assert_eq!(patterns.len(), 1);
        assert_eq!(patterns[0].pattern_type, PatternType::UnnecessaryPermissionChecks);
    }

    // ─── Unit Tests: Anomaly Detection ──────────────────────────────────────

    #[test]
    fn test_anomaly_low_efficiency() {
        let config = default_config();
        let flag = check_anomaly(0.3, 10, 10.0, &config);
        assert!(flag.is_some());
        assert_eq!(flag.unwrap().reason, AnomalyReason::LowEfficiency);
    }

    #[test]
    fn test_anomaly_excessive_calls() {
        let config = default_config();
        let flag = check_anomaly(0.8, 100, 10.0, &config); // 100 > 10 * 3.0
        assert!(flag.is_some());
        assert_eq!(flag.unwrap().reason, AnomalyReason::ExcessiveCalls);
    }

    #[test]
    fn test_anomaly_both() {
        let config = default_config();
        let flag = check_anomaly(0.2, 100, 10.0, &config);
        assert!(flag.is_some());
        assert_eq!(flag.unwrap().reason, AnomalyReason::Both);
    }

    #[test]
    fn test_anomaly_none() {
        let config = default_config();
        let flag = check_anomaly(0.8, 10, 10.0, &config);
        assert!(flag.is_none());
    }

    #[test]
    fn test_anomaly_at_threshold_boundary() {
        let config = default_config(); // threshold = 0.5, multiplier = 3.0
        // Exactly at threshold — not below
        let flag = check_anomaly(0.5, 30, 10.0, &config); // 30 == 10*3.0, not >
        assert!(flag.is_none());
    }

    // ─── Property-Based Tests (Task 4.5): Properties 5 and 6 ────────────────

    // Feature: tool-call-tracker, Property 5: Classification mutual exclusivity and exhaustiveness
    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]

        /// **Validates: Requirements 3.2, 3.3, 3.4**
        #[test]
        fn prop_classification_exhaustive(
            num_records in 1usize..20,
            tool_idx in prop::collection::vec(0usize..5, 1..20),
        ) {
            let tool_names = ["read_file", "write_file", "grep_search", "list_dir", "api_call"];
            let num = num_records.min(tool_idx.len());

            let records: Vec<ToolCallRecord> = (0..num)
                .map(|i| {
                    let tn = tool_names[tool_idx[i] % tool_names.len()];
                    make_record(tn, &format!(r#"{{"idx":{}}}"#, i), Some(&format!("output-{}", i)), true, (i + 1) as u32)
                })
                .collect();

            let expected_artifacts: Vec<String> = vec![];
            let final_idx = find_final_artifact_index(&records, &expected_artifacts);

            let mut useful = 0u32;
            let mut redundant = 0u32;

            for (i, record) in records.iter().enumerate() {
                match classify_tool_call(record, i, &records[..i], &expected_artifacts, final_idx) {
                    CallClassification::Useful => useful += 1,
                    CallClassification::Redundant => redundant += 1,
                }
            }

            // Exhaustiveness: useful + redundant == total
            prop_assert_eq!(useful + redundant, records.len() as u32);
        }
    }

    // Feature: tool-call-tracker, Property 6: Efficiency ratio bounds and formula
    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]

        /// **Validates: Requirements 3.5, 3.6**
        #[test]
        fn prop_efficiency_ratio_bounds(
            num_records in 0usize..30,
            success_pattern in prop::collection::vec(proptest::bool::ANY, 0..30),
        ) {
            let num = num_records.min(success_pattern.len());

            let records: Vec<ToolCallRecord> = (0..num)
                .map(|i| {
                    make_record(
                        "tool",
                        &format!(r#"{{"i":{}}}"#, i),
                        Some(&format!("out-{}", i)),
                        success_pattern[i],
                        (i + 1) as u32,
                    )
                })
                .collect();

            let ratio = compute_efficiency_ratio(&records, &[]);

            // Bounds: [0.0, 1.0]
            prop_assert!(ratio >= 0.0);
            prop_assert!(ratio <= 1.0);

            // Empty trace → 1.0
            if records.is_empty() {
                prop_assert!((ratio - 1.0).abs() < f64::EPSILON);
            }

            // Formula: ratio = useful / total for non-empty
            if !records.is_empty() {
                let final_idx = find_final_artifact_index(&records, &[]);
                let useful_count = records.iter().enumerate()
                    .filter(|(i, r)| {
                        classify_tool_call(r, *i, &records[..*i], &[], final_idx)
                            == CallClassification::Useful
                    })
                    .count();
                let expected_ratio = useful_count as f64 / records.len() as f64;
                prop_assert!((ratio - expected_ratio).abs() < f64::EPSILON);
            }
        }
    }

    // ─── Property-Based Tests (Task 5.6): Property 7 ────────────────────────

    // Feature: tool-call-tracker, Property 7: Pattern detection correctness
    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]

        /// **Validates: Requirements 4.1, 4.2, 4.3, 4.4, 4.5**
        #[test]
        fn prop_pattern_detection_repeated_identical(
            repeat_count in 2usize..8,
        ) {
            // Create a trace with repeated identical calls
            let mut records: Vec<ToolCallRecord> = (0..repeat_count)
                .map(|i| make_record("read_file", r#"{"path":"same.txt"}"#, Some("same output"), true, (i + 1) as u32))
                .collect();
            // Add a different call at the end
            records.push(make_record("write_file", r#"{"path":"out.txt"}"#, Some("done"), true, (repeat_count + 1) as u32));

            let patterns = detect_repeated_identical(&records);

            // Must detect the repeated pattern
            prop_assert!(!patterns.is_empty());
            let p = &patterns[0];
            prop_assert_eq!(p.pattern_type.clone(), PatternType::RepeatedIdenticalCalls);
            prop_assert!(p.offending_indices.len() >= 2);
            prop_assert!(!p.description.is_empty());

            // All offending indices must be valid
            for &idx in &p.offending_indices {
                prop_assert!(idx < records.len());
            }
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]

        /// Property 7 continued: always-failing detection
        /// **Validates: Requirements 4.1, 4.2, 4.3, 4.4, 4.5**
        #[test]
        fn prop_pattern_detection_always_failing(
            fail_count in 3usize..10,
        ) {
            let records: Vec<ToolCallRecord> = (0..fail_count)
                .map(|i| make_record("broken_tool", &format!(r#"{{"attempt":{}}}"#, i), None, false, (i + 1) as u32))
                .collect();

            let patterns = detect_always_failing(&records);

            prop_assert!(!patterns.is_empty());
            let p = &patterns[0];
            prop_assert_eq!(p.pattern_type.clone(), PatternType::AlwaysFailingCalls);
            prop_assert_eq!(p.offending_indices.len(), fail_count);
            prop_assert!(!p.description.is_empty());

            for &idx in &p.offending_indices {
                prop_assert!(idx < records.len());
            }
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]

        /// Property 7 continued: post-answer detection
        /// **Validates: Requirements 4.1, 4.2, 4.3, 4.4, 4.5**
        #[test]
        fn prop_pattern_detection_post_answer(
            post_count in 1usize..5,
        ) {
            let mut records = vec![
                make_record("write_file", r#"{"path":"artifact.txt"}"#, Some("wrote artifact.txt"), true, 1),
            ];
            for i in 0..post_count {
                records.push(make_record("read_file", &format!(r#"{{"extra":{}}}"#, i), Some("extra"), true, (i + 2) as u32));
            }

            let patterns = detect_post_answer(&records, &["artifact.txt".to_string()]);

            prop_assert!(!patterns.is_empty());
            let p = &patterns[0];
            prop_assert_eq!(p.pattern_type.clone(), PatternType::PostAnswerCalls);
            prop_assert_eq!(p.offending_indices.len(), post_count);
            prop_assert!(!p.description.is_empty());

            for &idx in &p.offending_indices {
                prop_assert!(idx < records.len());
                prop_assert!(idx > 0); // all after the artifact
            }
        }
    }

    // ─── Property-Based Tests (Task 6.5): Property 8 ────────────────────────

    // Feature: tool-call-tracker, Property 8: Anomaly detection correctness
    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]

        /// **Validates: Requirements 5.1, 5.2, 5.5**
        #[test]
        fn prop_anomaly_detection_correctness(
            efficiency_ratio in 0.0f64..=1.0,
            total_calls in 0u32..200,
            historical_avg in 1.0f64..50.0,
            threshold in 0.1f64..0.9,
            multiplier in 1.5f64..5.0,
        ) {
            let config = ToolCallTrackerConfig {
                efficiency_threshold: threshold,
                historical_avg_multiplier: multiplier,
                ..ToolCallTrackerConfig::default()
            };

            let result = check_anomaly(efficiency_ratio, total_calls, historical_avg, &config);

            let low_eff = efficiency_ratio < threshold;
            let excessive = (total_calls as f64) > historical_avg * multiplier;

            match (low_eff, excessive) {
                (false, false) => {
                    prop_assert!(result.is_none());
                }
                (true, true) => {
                    prop_assert!(result.is_some());
                    prop_assert_eq!(result.unwrap().reason, AnomalyReason::Both);
                }
                (true, false) => {
                    prop_assert!(result.is_some());
                    prop_assert_eq!(result.unwrap().reason, AnomalyReason::LowEfficiency);
                }
                (false, true) => {
                    prop_assert!(result.is_some());
                    prop_assert_eq!(result.unwrap().reason, AnomalyReason::ExcessiveCalls);
                }
            }
        }
    }

    // ─── Property-Based Tests (Task 7.4): Property 13 ───────────────────────

    // Feature: tool-call-tracker, Property 13: Trace summary structural completeness
    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]

        /// **Validates: Requirements 6.2, 6.3, 6.5**
        #[test]
        fn prop_trace_summary_structural_completeness(
            num_records in 0usize..20,
        ) {
            let records: Vec<ToolCallRecord> = (0..num_records)
                .map(|i| make_record(
                    &format!("tool_{}", i % 5),
                    &format!(r#"{{"i":{}}}"#, i),
                    Some(&format!("output-{}", i)),
                    true,
                    (i + 1) as u32,
                ))
                .collect();

            let config = default_config();
            let result = analyze_completed_task_inner(
                &records,
                "packet-prop13",
                "agent-prop13",
                "test_task",
                &[],
                &[],
                &[],
                &config,
            );

            let summary = build_trace_summary(&result);

            // Non-empty delegation_packet_id
            prop_assert!(!summary.delegation_packet_id.is_empty());

            // Efficiency ratio in [0.0, 1.0]
            prop_assert!(summary.efficiency_ratio >= 0.0);
            prop_assert!(summary.efficiency_ratio <= 1.0);

            // total_calls >= 0
            prop_assert_eq!(summary.total_calls, num_records as u32);

            // useful + redundant == total
            prop_assert_eq!(summary.useful_calls + summary.redundant_calls, summary.total_calls);

            // tool_sequence_signature length == total_calls
            prop_assert_eq!(summary.tool_sequence_signature.len(), num_records);

            // Each element in signature is the tool name in order
            for (i, name) in summary.tool_sequence_signature.iter().enumerate() {
                prop_assert_eq!(name, &format!("tool_{}", i % 5));
            }

            // analyzed_at is non-empty and contains 'T' (ISO-8601)
            prop_assert!(!summary.analyzed_at.is_empty());
            prop_assert!(summary.analyzed_at.contains('T'));

            // detected_patterns: each has valid structure
            for pattern in &summary.detected_patterns {
                prop_assert!(!pattern.offending_indices.is_empty());
                prop_assert!(!pattern.description.is_empty());
                for &idx in &pattern.offending_indices {
                    prop_assert!(idx < num_records);
                }
            }
        }
    }

    // ─── Integration Test (Task 7.5) ────────────────────────────────────────

    #[test]
    fn test_end_to_end_logging_through_analysis() {
        use crate::tool_call_tracker_service::{
            initialize_tool_call_tracker_db, insert_tool_call_records_batch,
            query_records_by_packet_id,
        };
        use rusqlite::Connection;

        let conn = Connection::open_in_memory().unwrap();
        initialize_tool_call_tracker_db(&conn).unwrap();

        // Simulate logging a sequence of tool calls
        let records = vec![
            make_record("read_file", r#"{"path":"src/main.rs"}"#, Some("fn main() {}"), true, 1),
            make_record("grep_search", r#"{"query":"TODO"}"#, Some("found 3 TODOs"), true, 2),
            make_record("write_file", r#"{"path":"output.rs"}"#, Some("wrote output.rs"), true, 3),
            make_record("read_file", r#"{"path":"src/main.rs"}"#, Some("fn main() {}"), true, 4), // duplicate
            make_record("list_dir", r#"{"path":"."}"#, Some("files listed"), true, 5), // post-answer
        ];

        insert_tool_call_records_batch(&conn, &records).unwrap();

        // Verify records persisted
        let loaded = query_records_by_packet_id(&conn, "packet-test").unwrap();
        assert_eq!(loaded.len(), 5);

        // Run analysis
        let config = default_config();
        let result = analyze_completed_task_inner(
            &loaded,
            "packet-test",
            "agent-test",
            "test_task",
            &["output.rs".to_string()],
            &[],
            &[],
            &config,
        );

        // Verify analysis result
        assert_eq!(result.total_calls, 5);
        assert_eq!(result.useful_calls + result.redundant_calls, 5);
        assert!(result.efficiency_ratio >= 0.0 && result.efficiency_ratio <= 1.0);
        assert_eq!(result.tool_sequence_signature.len(), 5);
        assert_eq!(result.tool_sequence_signature[0], "read_file");
        assert_eq!(result.tool_sequence_signature[2], "write_file");

        // Should detect post-answer pattern (calls after write_file produced artifact)
        let post_answer_patterns: Vec<_> = result
            .detected_patterns
            .iter()
            .filter(|p| p.pattern_type == PatternType::PostAnswerCalls)
            .collect();
        assert!(!post_answer_patterns.is_empty());

        // Build trace summary
        let summary = build_trace_summary(&result);
        assert_eq!(summary.delegation_packet_id, "packet-test");
        assert_eq!(summary.total_calls, 5);
        assert!(!summary.analyzed_at.is_empty());
    }
}
