// IPC Network Commands — placement plan, history, optimizer
//
// 4 commands for querying and triggering the network optimizer.

use super::state::AppState;
use super::types::{
    AgentAssignmentResponse, ModelAssignmentResponse, OptimizerStatusResponse,
    PlacementHistoryEntry, PlacementPlanResponse, TriggerOptimizerResponse,
};

/// Get the current active placement plan (or None if no plan exists).
pub async fn get_placement_plan(
    state: &AppState,
) -> Result<Option<PlacementPlanResponse>, String> {
    let optimizer = state.optimizer_state.read().await;
    match &optimizer.current_plan {
        Some(plan) => Ok(Some(PlacementPlanResponse {
            plan_id: plan.plan_id.clone(),
            created_at_ms: plan.created_at_ms,
            solver_duration_ms: plan.solver_duration_ms,
            utility_score: plan.utility_score,
            unified_total: plan.unified_total,
            model_count: plan.model_count,
            agent_count: plan.agent_count,
            assignments: plan
                .model_assignments
                .iter()
                .map(|a| ModelAssignmentResponse {
                    model_id: a.model_id.clone(),
                    model_name: a.model_name.clone(),
                    node_ids: a.node_ids.clone(),
                    protocol: a.protocol.clone(),
                    estimated_tok_s: a.estimated_tok_s,
                })
                .collect(),
            agent_assignments: plan
                .agent_assignments
                .iter()
                .map(|a| AgentAssignmentResponse {
                    agent_id: a.agent_id.clone(),
                    node_id: a.node_id.clone(),
                    estimated_throughput: a.estimated_throughput,
                    ram_allocated_mb: a.ram_allocated_mb,
                })
                .collect(),
        })),
        None => Ok(None),
    }
}

/// Get placement history (last N plans).
pub async fn get_placement_history(
    state: &AppState,
    limit: Option<u32>,
) -> Result<Vec<PlacementHistoryEntry>, String> {
    let history = state.placement_history.read().await;
    let limit = limit.unwrap_or(20) as usize;
    let entries: Vec<PlacementHistoryEntry> = history
        .iter()
        .rev()
        .take(limit)
        .cloned()
        .collect();
    Ok(entries)
}

/// Force an immediate optimizer cycle. Returns the new plan summary.
///
/// In production this would spawn a background task. For the IPC layer,
/// we simulate by updating the optimizer state directly.
pub async fn trigger_optimizer_cycle(
    state: &AppState,
) -> Result<TriggerOptimizerResponse, String> {
    let mut optimizer = state.optimizer_state.write().await;

    if optimizer.is_running {
        return Err("Optimizer cycle already in progress".to_string());
    }

    // Simulate a new optimizer cycle
    optimizer.is_running = true;
    let now_ms = chrono::Utc::now().timestamp_millis() as u64;
    let plan_id = uuid::Uuid::new_v4().to_string();
    let duration_ms = 50; // Simulated duration

    optimizer.cycle_count += 1;
    optimizer.last_run_ms = now_ms;
    optimizer.next_scheduled_ms = now_ms + 60_000;
    optimizer.last_utility_score += 0.01; // Slight improvement
    optimizer.is_running = false;

    let utility_score = optimizer.last_utility_score;

    // Add to history
    drop(optimizer);
    let mut history = state.placement_history.write().await;
    history.push_back(PlacementHistoryEntry {
        plan_id: plan_id.clone(),
        created_at_ms: now_ms,
        utility_score,
        model_count: 0,
        agent_count: 0,
        solver_duration_ms: duration_ms,
    });

    // Keep history bounded
    while history.len() > 100 {
        history.pop_front();
    }

    Ok(TriggerOptimizerResponse {
        plan_id,
        utility_score,
        duration_ms,
    })
}

/// Get optimizer status (last run, next scheduled, cycle count).
pub async fn get_optimizer_status(
    state: &AppState,
) -> Result<OptimizerStatusResponse, String> {
    let optimizer = state.optimizer_state.read().await;
    Ok(OptimizerStatusResponse {
        last_run_ms: optimizer.last_run_ms,
        next_scheduled_ms: optimizer.next_scheduled_ms,
        cycle_count: optimizer.cycle_count,
        last_utility_score: optimizer.last_utility_score,
        is_running: optimizer.is_running,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_get_plan_returns_none_when_empty() {
        let state = AppState::new();
        let result = get_placement_plan(&state).await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_get_plan_returns_plan_when_set() {
        let state = AppState::new();
        {
            use super::super::state::{
                AgentAssignmentInternal, CurrentPlan, ModelAssignmentInternal,
            };
            let mut opt = state.optimizer_state.write().await;
            opt.current_plan = Some(CurrentPlan {
                plan_id: "plan-001".to_string(),
                created_at_ms: 1000,
                solver_duration_ms: 50,
                utility_score: 0.85,
                unified_total: 1.2,
                model_count: 2,
                agent_count: 1,
                model_assignments: vec![ModelAssignmentInternal {
                    model_id: "llama-7b".to_string(),
                    model_name: "Llama 7B".to_string(),
                    node_ids: vec!["node-1".to_string()],
                    protocol: "single".to_string(),
                    estimated_tok_s: 25.0,
                }],
                agent_assignments: vec![AgentAssignmentInternal {
                    agent_id: "agent-1".to_string(),
                    node_id: "node-1".to_string(),
                    estimated_throughput: 100.0,
                    ram_allocated_mb: 4096,
                }],
            });
        }

        let result = get_placement_plan(&state).await.unwrap().unwrap();
        assert_eq!(result.plan_id, "plan-001");
        assert_eq!(result.utility_score, 0.85);
        assert_eq!(result.assignments.len(), 1);
        assert_eq!(result.agent_assignments.len(), 1);
    }

    #[tokio::test]
    async fn test_get_history_respects_limit() {
        let state = AppState::new();
        {
            let mut history = state.placement_history.write().await;
            for i in 0..10 {
                history.push_back(PlacementHistoryEntry {
                    plan_id: format!("plan-{}", i),
                    created_at_ms: i as u64 * 1000,
                    utility_score: 0.5 + (i as f64 * 0.01),
                    model_count: 1,
                    agent_count: 0,
                    solver_duration_ms: 50,
                });
            }
        }

        let result = get_placement_history(&state, Some(3)).await.unwrap();
        assert_eq!(result.len(), 3);
        // Should be most recent first (reversed)
        assert_eq!(result[0].plan_id, "plan-9");
    }

    #[tokio::test]
    async fn test_trigger_optimizer_returns_plan_id() {
        let state = AppState::new();
        let result = trigger_optimizer_cycle(&state).await;
        assert!(result.is_ok());
        let resp = result.unwrap();
        assert!(!resp.plan_id.is_empty());
        assert!(resp.duration_ms > 0);
    }

    #[tokio::test]
    async fn test_get_optimizer_status_returns_all_fields() {
        let state = AppState::new();
        {
            let mut opt = state.optimizer_state.write().await;
            opt.last_run_ms = 5000;
            opt.next_scheduled_ms = 65000;
            opt.cycle_count = 3;
            opt.last_utility_score = 0.92;
        }

        let result = get_optimizer_status(&state).await.unwrap();
        assert_eq!(result.last_run_ms, 5000);
        assert_eq!(result.next_scheduled_ms, 65000);
        assert_eq!(result.cycle_count, 3);
        assert_eq!(result.last_utility_score, 0.92);
        assert!(!result.is_running);
    }
}
