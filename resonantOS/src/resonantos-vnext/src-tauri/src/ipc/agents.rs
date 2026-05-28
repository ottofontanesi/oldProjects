// IPC Agent Commands — workflow start/stop/status/list
//
// 4 commands for managing distributed agent workflows from the frontend.

use super::state::AppState;
use super::types::{
    StartWorkflowRequest, StartWorkflowResponse, StopWorkflowResponse, WorkflowStatusResponse,
    WorkflowSummary,
};
use crate::agents::dag::{AgentPlan, AgentPlanStep};
use crate::agents::protocol::WorkflowStatus;

/// Start a new distributed agent workflow.
///
/// Validates the request, creates a workflow via the orchestrator, and returns
/// the new workflow ID. The workflow executes asynchronously in the background.
pub async fn start_agent_workflow(
    state: &AppState,
    request: StartWorkflowRequest,
) -> Result<StartWorkflowResponse, String> {
    let mut orchestrator_guard = state.agent_orchestrator.write().await;
    let orchestrator = orchestrator_guard
        .as_mut()
        .ok_or_else(|| "Agent orchestrator not initialized. Please wait for startup to complete.".to_string())?;

    // Build an AgentPlan from the request
    let max_steps = request.max_steps.unwrap_or(1);
    let steps: Vec<AgentPlanStep> = (0..max_steps)
        .map(|i| AgentPlanStep {
            description: if i == 0 {
                request.task_description.clone()
            } else {
                format!("Step {} of workflow", i + 1)
            },
            model: request.model_preference.clone(),
            tools: request.required_tools.clone(),
            depends_on: if i == 0 { vec![] } else { vec![0] },
            sensitivity: None,
            estimated_compute_ms: request.timeout_ms.unwrap_or(30_000),
        })
        .collect();

    let plan = AgentPlan {
        name: request.task_description.clone(),
        steps,
    };

    let workflow_id = orchestrator
        .start_workflow(&plan)
        .map_err(|e| format!("Failed to start workflow: {}", e))?;

    let now_ms = chrono::Utc::now().timestamp_millis() as u64;

    Ok(StartWorkflowResponse {
        workflow_id: workflow_id.to_string(),
        status: "pending".to_string(),
        created_at_ms: now_ms,
    })
}

/// Stop a running workflow. Returns completion stats.
pub async fn stop_agent_workflow(
    state: &AppState,
    workflow_id: String,
) -> Result<StopWorkflowResponse, String> {
    let mut orchestrator_guard = state.agent_orchestrator.write().await;
    let orchestrator = orchestrator_guard
        .as_mut()
        .ok_or_else(|| "Agent orchestrator not initialized. Please wait for startup to complete.".to_string())?;

    let wf_id: uuid::Uuid = workflow_id
        .parse()
        .map_err(|_| format!("Invalid workflow_id: '{}'", workflow_id))?;

    // Get progress before cancelling to report stats
    let progress = orchestrator.get_workflow_progress(wf_id);
    let was_running = progress
        .as_ref()
        .map(|p| matches!(p.status, WorkflowStatus::Running))
        .unwrap_or(false);
    let steps_completed_before = progress.as_ref().map(|p| p.completed_steps).unwrap_or(0);

    orchestrator
        .cancel_workflow(wf_id)
        .map_err(|e| format!("Failed to stop workflow '{}': {}", workflow_id, e))?;

    // Get updated progress after cancellation
    let progress_after = orchestrator.get_workflow_progress(wf_id);
    let steps_cancelled = progress_after
        .as_ref()
        .map(|p| p.cancelled_steps)
        .unwrap_or(0);

    Ok(StopWorkflowResponse {
        workflow_id,
        was_running,
        steps_completed: steps_completed_before,
        steps_cancelled,
    })
}

/// Get the current status of a workflow.
pub async fn get_workflow_status(
    state: &AppState,
    workflow_id: String,
) -> Result<WorkflowStatusResponse, String> {
    let orchestrator_guard = state.agent_orchestrator.read().await;
    let orchestrator = orchestrator_guard
        .as_ref()
        .ok_or_else(|| "Agent orchestrator not initialized. Please wait for startup to complete.".to_string())?;

    let wf_id: uuid::Uuid = workflow_id
        .parse()
        .map_err(|_| format!("Invalid workflow_id: '{}'", workflow_id))?;

    let progress = orchestrator
        .get_workflow_progress(wf_id)
        .ok_or_else(|| format!("Workflow '{}' not found", workflow_id))?;

    let (status_str, error_message) = match &progress.status {
        WorkflowStatus::Running => ("running".to_string(), None),
        WorkflowStatus::Paused => ("pending".to_string(), None),
        WorkflowStatus::Completed => ("completed".to_string(), None),
        WorkflowStatus::Failed { reason, .. } => ("failed".to_string(), Some(reason.clone())),
        WorkflowStatus::Checkpointed => ("pending".to_string(), None),
    };

    let elapsed_ms = progress.estimated_remaining_ms; // Approximation

    Ok(WorkflowStatusResponse {
        workflow_id,
        status: status_str,
        current_step: progress.completed_steps + progress.running_steps,
        total_steps: progress.total_steps,
        elapsed_ms,
        steps_completed: progress.completed_steps,
        steps_failed: progress.failed_steps,
        steps_running: progress.running_steps,
        error_message,
    })
}

/// List all active (running or pending) workflows.
pub async fn list_active_workflows(
    state: &AppState,
) -> Result<Vec<WorkflowSummary>, String> {
    let orchestrator_guard = state.agent_orchestrator.read().await;
    let orchestrator = orchestrator_guard
        .as_ref()
        .ok_or_else(|| "Agent orchestrator not initialized. Please wait for startup to complete.".to_string())?;

    let active_ids = orchestrator.list_active_workflows();
    let mut summaries = Vec::new();

    for wf_id in active_ids {
        if let Some(wf_state) = orchestrator.get_workflow_status(wf_id) {
            let progress = orchestrator.get_workflow_progress(wf_id);
            let progress_percent = progress
                .as_ref()
                .map(|p| {
                    if p.total_steps == 0 {
                        0u8
                    } else {
                        ((p.completed_steps as f64 / p.total_steps as f64) * 100.0) as u8
                    }
                })
                .unwrap_or(0);

            let status_str = match &wf_state.status {
                WorkflowStatus::Running => "running",
                WorkflowStatus::Paused => "pending",
                WorkflowStatus::Completed => "completed",
                WorkflowStatus::Failed { .. } => "failed",
                WorkflowStatus::Checkpointed => "pending",
            };

            let started_at_ms = wf_state.started_at.timestamp_millis() as u64;

            summaries.push(WorkflowSummary {
                workflow_id: wf_id.to_string(),
                status: status_str.to_string(),
                task_description: wf_state.agent_id.clone(),
                started_at_ms,
                progress_percent,
            });
        }
    }

    Ok(summaries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::orchestrator::WorkflowOrchestrator;
    use crate::agents::DistributedAgentConfig;

    async fn make_ready_state() -> AppState {
        let state = AppState::new();
        let node_id = uuid::Uuid::new_v4();
        let orchestrator = WorkflowOrchestrator::new(node_id, DistributedAgentConfig::default());
        *state.agent_orchestrator.write().await = Some(orchestrator);
        state
    }

    #[tokio::test]
    async fn test_start_workflow_returns_id() {
        let state = make_ready_state().await;
        let request = StartWorkflowRequest {
            task_description: "Test task".to_string(),
            model_preference: None,
            required_tools: vec![],
            max_steps: Some(2),
            timeout_ms: None,
        };

        let result = start_agent_workflow(&state, request).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.status, "pending");
        assert!(!response.workflow_id.is_empty());
    }

    #[tokio::test]
    async fn test_start_workflow_uninitialized_returns_error() {
        let state = AppState::new();
        let request = StartWorkflowRequest {
            task_description: "Test".to_string(),
            model_preference: None,
            required_tools: vec![],
            max_steps: Some(1),
            timeout_ms: None,
        };

        let result = start_agent_workflow(&state, request).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not initialized"));
    }

    #[tokio::test]
    async fn test_stop_nonexistent_workflow_returns_error() {
        let state = make_ready_state().await;
        let fake_id = uuid::Uuid::new_v4().to_string();

        let result = stop_agent_workflow(&state, fake_id).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not found"));
    }

    #[tokio::test]
    async fn test_get_status_with_valid_id() {
        let state = make_ready_state().await;
        let request = StartWorkflowRequest {
            task_description: "Status test".to_string(),
            model_preference: None,
            required_tools: vec![],
            max_steps: Some(2),
            timeout_ms: None,
        };

        let start_resp = start_agent_workflow(&state, request).await.unwrap();
        let result = get_workflow_status(&state, start_resp.workflow_id.clone()).await;
        assert!(result.is_ok());
        let status = result.unwrap();
        assert_eq!(status.workflow_id, start_resp.workflow_id);
        assert_eq!(status.status, "running");
        assert_eq!(status.total_steps, 2);
    }

    #[tokio::test]
    async fn test_list_active_returns_running_workflows() {
        let state = make_ready_state().await;
        let request = StartWorkflowRequest {
            task_description: "Active workflow".to_string(),
            model_preference: None,
            required_tools: vec![],
            max_steps: Some(2),
            timeout_ms: None,
        };

        start_agent_workflow(&state, request).await.unwrap();
        let result = list_active_workflows(&state).await;
        assert!(result.is_ok());
        let workflows = result.unwrap();
        assert_eq!(workflows.len(), 1);
        assert_eq!(workflows[0].status, "running");
    }

    #[tokio::test]
    async fn test_list_active_uninitialized_returns_error() {
        let state = AppState::new();
        let result = list_active_workflows(&state).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not initialized"));
    }

    #[tokio::test]
    async fn test_stop_workflow_returns_stats() {
        let state = make_ready_state().await;
        let request = StartWorkflowRequest {
            task_description: "Stop test".to_string(),
            model_preference: None,
            required_tools: vec![],
            max_steps: Some(3),
            timeout_ms: None,
        };

        let start_resp = start_agent_workflow(&state, request).await.unwrap();
        let result = stop_agent_workflow(&state, start_resp.workflow_id.clone()).await;
        assert!(result.is_ok());
        let stop_resp = result.unwrap();
        assert_eq!(stop_resp.workflow_id, start_resp.workflow_id);
        assert!(stop_resp.was_running);
        assert!(stop_resp.steps_cancelled > 0);
    }
}
