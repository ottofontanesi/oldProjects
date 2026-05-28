// Distributed Agent Execution — Orchestrator
// Phase 15: Workflow lifecycle, DAG management, progress reporting
//
// The orchestrator coordinates workflow execution on the local (requesting) node.
// It builds the execution DAG from an agent plan, manages workflow state, reports
// progress to the UI, and supports dynamic step addition during execution.
//
// Satisfies FR-8.1: Orchestrator runs on the requesting node.
// Satisfies FR-8.2: Manages parallel execution, collects results, handles failures, reports progress.
// Satisfies FR-8.3: Orchestrator is lightweight coordination logic.
// Satisfies FR-8.5: Exposes progress to the UI.
// Satisfies FR-2.5: Steps can be dynamically added during execution.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::agents::dag::{
    build_execution_dag, AgentPlan, AgentPlanStep, ExecutionStep, PromptSensitivity, StepId,
    StepStatus, WorkflowId,
};
use crate::agents::protocol::{WorkflowState, WorkflowStatus};
use crate::agents::DistributedAgentConfig;
use crate::network::registry::NodeId;

// ---------------------------------------------------------------------------
// Orchestrator Errors
// ---------------------------------------------------------------------------

/// Errors that can occur during orchestrator operations.
#[derive(Debug, Clone, PartialEq)]
pub enum OrchestratorError {
    /// The referenced workflow does not exist.
    WorkflowNotFound(WorkflowId),
    /// DAG construction from the agent plan failed.
    DagBuildFailed(String),
    /// Adding a step would create a cycle in the DAG.
    CycleDetected,
    /// The workflow has already reached a terminal state (Completed or Failed).
    WorkflowAlreadyComplete,
}

impl std::fmt::Display for OrchestratorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OrchestratorError::WorkflowNotFound(id) => {
                write!(f, "Workflow not found: {}", id)
            }
            OrchestratorError::DagBuildFailed(reason) => {
                write!(f, "DAG build failed: {}", reason)
            }
            OrchestratorError::CycleDetected => {
                write!(f, "Adding step would create a cycle in the DAG")
            }
            OrchestratorError::WorkflowAlreadyComplete => {
                write!(f, "Workflow has already completed or failed")
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Workflow Progress (Task 8.2: Progress reporting to UI)
// ---------------------------------------------------------------------------

/// UI-friendly progress summary for a workflow.
///
/// Provides a snapshot of workflow execution state suitable for rendering
/// in the frontend. Includes step counts, timing estimates, and overall status.
///
/// Satisfies FR-8.5: Orchestrator exposes progress to the UI.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkflowProgress {
    /// Unique identifier for this workflow.
    pub workflow_id: WorkflowId,

    /// Total number of steps in the DAG.
    pub total_steps: u32,

    /// Number of steps that have completed successfully.
    pub completed_steps: u32,

    /// Number of steps currently executing (Dispatched or Running).
    pub running_steps: u32,

    /// Number of steps waiting for dependencies or dispatch (Pending or Ready).
    pub waiting_steps: u32,

    /// Number of steps that have failed.
    pub failed_steps: u32,

    /// Number of steps that have been cancelled.
    pub cancelled_steps: u32,

    /// Estimated remaining time in milliseconds (based on remaining step estimates).
    pub estimated_remaining_ms: u64,

    /// Current high-level workflow status.
    pub status: WorkflowStatus,
}

// ---------------------------------------------------------------------------
// Workflow Events (Task 8.2: Events for UI consumption)
// ---------------------------------------------------------------------------

/// Events emitted by the orchestrator for UI consumption.
///
/// These events allow the frontend to reactively update its display as
/// workflow execution progresses.
///
/// Satisfies FR-8.5: Emit events for UI consumption.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WorkflowEvent {
    /// A workflow has started execution.
    WorkflowStarted {
        workflow_id: WorkflowId,
        total_steps: u32,
    },

    /// A step has begun executing on a node.
    StepStarted {
        workflow_id: WorkflowId,
        step_id: StepId,
        description: String,
        assigned_node: Option<NodeId>,
    },

    /// A step has completed successfully.
    StepCompleted {
        workflow_id: WorkflowId,
        step_id: StepId,
        compute_time_ms: u64,
    },

    /// A step has failed.
    StepFailed {
        workflow_id: WorkflowId,
        step_id: StepId,
        reason: String,
    },

    /// A new step was dynamically added to the workflow.
    StepAdded {
        workflow_id: WorkflowId,
        step_id: StepId,
        description: String,
    },

    /// The entire workflow has completed (all steps done).
    WorkflowCompleted { workflow_id: WorkflowId },

    /// The workflow has failed and cannot continue.
    WorkflowFailed {
        workflow_id: WorkflowId,
        reason: String,
    },
}

// ---------------------------------------------------------------------------
// Workflow Orchestrator (Tasks 8.1, 8.2, 8.3)
// ---------------------------------------------------------------------------

/// The workflow orchestrator: manages workflow lifecycles on the local node.
///
/// The orchestrator always runs on the requesting node (FR-8.1). It is lightweight
/// coordination logic — no GPU needed (FR-8.3). It builds DAGs from agent plans,
/// tracks workflow state, reports progress, and supports dynamic step addition.
///
/// Satisfies FR-8.1: Orchestrator runs on the requesting node.
/// Satisfies FR-8.2: Manages parallel execution, collects results, handles failures.
/// Satisfies FR-8.3: Lightweight coordination logic.
/// Satisfies FR-8.5: Exposes progress to the UI.
/// Satisfies FR-2.5: Dynamic step addition during execution.
#[derive(Debug)]
pub struct WorkflowOrchestrator {
    /// The node where this orchestrator is running (always the requesting node).
    local_node_id: NodeId,

    /// Active workflows managed by this orchestrator.
    workflows: HashMap<WorkflowId, WorkflowState>,

    /// Configuration for distributed agent execution.
    config: DistributedAgentConfig,

    /// Event log for UI consumption. In a real system this would be a channel/broadcast,
    /// but for testability we collect events here.
    events: Vec<WorkflowEvent>,
}

impl WorkflowOrchestrator {
    /// Create a new orchestrator on the given local node.
    ///
    /// The orchestrator always runs on the requesting node (FR-8.1).
    pub fn new(local_node_id: NodeId, config: DistributedAgentConfig) -> Self {
        Self {
            local_node_id,
            workflows: HashMap::new(),
            config,
            events: Vec::new(),
        }
    }

    /// Returns the local node ID where this orchestrator is running.
    pub fn local_node_id(&self) -> NodeId {
        self.local_node_id
    }

    // -----------------------------------------------------------------------
    // Task 8.1: Workflow lifecycle
    // -----------------------------------------------------------------------

    /// Start a new workflow from an agent plan.
    ///
    /// Builds the execution DAG, creates a `WorkflowState`, marks root steps as Ready,
    /// and emits a `WorkflowStarted` event.
    ///
    /// # Errors
    ///
    /// Returns `OrchestratorError::DagBuildFailed` if the plan cannot be converted to a DAG.
    ///
    /// Satisfies FR-8.1: Orchestrator runs on the requesting node.
    /// Satisfies FR-8.2: Decompose plan into DAG, begin execution.
    pub fn start_workflow(&mut self, plan: &AgentPlan) -> Result<WorkflowId, OrchestratorError> {
        // Build the execution DAG from the agent plan
        let mut dag = build_execution_dag(plan)
            .map_err(|e| OrchestratorError::DagBuildFailed(e.to_string()))?;

        // Validate workflow doesn't exceed max steps
        if dag.steps.len() as u32 > self.config.max_workflow_steps {
            return Err(OrchestratorError::DagBuildFailed(format!(
                "Workflow has {} steps, exceeding maximum of {}",
                dag.steps.len(),
                self.config.max_workflow_steps
            )));
        }

        let workflow_id = dag.workflow_id;

        // Mark root steps as Ready (they have no dependencies)
        for &root_id in &dag.root_steps {
            if let Some(step) = dag.steps.get_mut(&root_id) {
                step.status = StepStatus::Ready;
            }
        }

        let total_steps = dag.steps.len() as u32;

        // Create workflow state
        let state = WorkflowState {
            workflow_id,
            agent_id: plan.name.clone(),
            requesting_node: self.local_node_id,
            dag,
            started_at: chrono::Utc::now(),
            status: WorkflowStatus::Running,
            parallel_steps_active: 0,
            total_steps,
            completed_steps: 0,
            checkpoint: None,
        };

        self.workflows.insert(workflow_id, state);

        // Emit workflow started event
        self.events.push(WorkflowEvent::WorkflowStarted {
            workflow_id,
            total_steps,
        });

        Ok(workflow_id)
    }

    /// Cancel a running workflow.
    ///
    /// Marks the workflow as Failed, cancels all non-terminal steps (Pending, Ready,
    /// Dispatched, Running), and emits a `WorkflowFailed` event.
    ///
    /// # Errors
    ///
    /// Returns `OrchestratorError::WorkflowNotFound` if the workflow doesn't exist.
    /// Returns `OrchestratorError::WorkflowAlreadyComplete` if already in a terminal state.
    ///
    /// Satisfies FR-8.2: Handle failures.
    pub fn cancel_workflow(&mut self, workflow_id: WorkflowId) -> Result<(), OrchestratorError> {
        let state = self
            .workflows
            .get_mut(&workflow_id)
            .ok_or(OrchestratorError::WorkflowNotFound(workflow_id))?;

        // Check if workflow is already in a terminal state
        match &state.status {
            WorkflowStatus::Completed | WorkflowStatus::Failed { .. } => {
                return Err(OrchestratorError::WorkflowAlreadyComplete);
            }
            _ => {}
        }

        // Cancel all non-terminal steps
        for step in state.dag.steps.values_mut() {
            match &step.status {
                StepStatus::Pending
                | StepStatus::Ready
                | StepStatus::Dispatched
                | StepStatus::Running => {
                    step.status = StepStatus::Cancelled;
                }
                _ => {} // Already completed, failed, or cancelled — leave as-is
            }
        }

        // Mark workflow as failed due to cancellation
        state.status = WorkflowStatus::Failed {
            failed_step: uuid::Uuid::nil(),
            reason: "Workflow cancelled by user".to_string(),
        };
        state.parallel_steps_active = 0;

        // Emit workflow failed event
        self.events.push(WorkflowEvent::WorkflowFailed {
            workflow_id,
            reason: "Workflow cancelled by user".to_string(),
        });

        Ok(())
    }

    /// Get the current state of a workflow.
    ///
    /// Returns `None` if the workflow doesn't exist.
    ///
    /// Satisfies FR-8.2: Report progress.
    pub fn get_workflow_status(&self, workflow_id: WorkflowId) -> Option<&WorkflowState> {
        self.workflows.get(&workflow_id)
    }

    /// List all active (non-terminal) workflow IDs.
    pub fn list_active_workflows(&self) -> Vec<WorkflowId> {
        self.workflows
            .iter()
            .filter(|(_, state)| {
                matches!(
                    state.status,
                    WorkflowStatus::Running | WorkflowStatus::Paused
                )
            })
            .map(|(&id, _)| id)
            .collect()
    }

    // -----------------------------------------------------------------------
    // Task 8.2: Progress reporting to UI
    // -----------------------------------------------------------------------

    /// Get a UI-friendly progress summary for a workflow.
    ///
    /// Computes step counts by status and estimates remaining time based on
    /// the sum of `estimated_compute_ms` for non-completed steps.
    ///
    /// Returns `None` if the workflow doesn't exist.
    ///
    /// Satisfies FR-8.5: Expose workflow state to UI.
    pub fn get_workflow_progress(&self, workflow_id: WorkflowId) -> Option<WorkflowProgress> {
        let state = self.workflows.get(&workflow_id)?;

        let mut completed_steps: u32 = 0;
        let mut running_steps: u32 = 0;
        let mut waiting_steps: u32 = 0;
        let mut failed_steps: u32 = 0;
        let mut cancelled_steps: u32 = 0;
        let mut estimated_remaining_ms: u64 = 0;

        for step in state.dag.steps.values() {
            match &step.status {
                StepStatus::Completed => {
                    completed_steps += 1;
                }
                StepStatus::Dispatched | StepStatus::Running => {
                    running_steps += 1;
                    // Running steps contribute partial remaining time
                    estimated_remaining_ms += step.estimated_compute_ms / 2;
                }
                StepStatus::Pending | StepStatus::Ready => {
                    waiting_steps += 1;
                    estimated_remaining_ms += step.estimated_compute_ms;
                }
                StepStatus::Failed { .. } => {
                    failed_steps += 1;
                }
                StepStatus::Cancelled => {
                    cancelled_steps += 1;
                }
            }
        }

        Some(WorkflowProgress {
            workflow_id,
            total_steps: state.total_steps,
            completed_steps,
            running_steps,
            waiting_steps,
            failed_steps,
            cancelled_steps,
            estimated_remaining_ms,
            status: state.status.clone(),
        })
    }

    /// Get all events emitted since the orchestrator was created.
    ///
    /// In a production system, events would be sent via a channel/broadcast.
    /// This method is primarily for testing and synchronous UI polling.
    pub fn drain_events(&mut self) -> Vec<WorkflowEvent> {
        std::mem::take(&mut self.events)
    }

    /// Emit a step-started event (called by the executor when a step begins).
    pub fn notify_step_started(
        &mut self,
        workflow_id: WorkflowId,
        step_id: StepId,
    ) -> Result<(), OrchestratorError> {
        let state = self
            .workflows
            .get_mut(&workflow_id)
            .ok_or(OrchestratorError::WorkflowNotFound(workflow_id))?;

        let (description, assigned_node) = state
            .dag
            .steps
            .get(&step_id)
            .map(|s| (s.description.clone(), s.assigned_node))
            .unwrap_or_default();

        state.parallel_steps_active += 1;

        self.events.push(WorkflowEvent::StepStarted {
            workflow_id,
            step_id,
            description,
            assigned_node,
        });

        Ok(())
    }

    /// Emit a step-completed event and update workflow counters.
    pub fn notify_step_completed(
        &mut self,
        workflow_id: WorkflowId,
        step_id: StepId,
        compute_time_ms: u64,
    ) -> Result<(), OrchestratorError> {
        let state = self
            .workflows
            .get_mut(&workflow_id)
            .ok_or(OrchestratorError::WorkflowNotFound(workflow_id))?;

        state.completed_steps += 1;
        if state.parallel_steps_active > 0 {
            state.parallel_steps_active -= 1;
        }

        // Check if all steps are complete
        let all_complete = state
            .dag
            .steps
            .values()
            .all(|s| matches!(s.status, StepStatus::Completed | StepStatus::Cancelled));

        if all_complete {
            state.status = WorkflowStatus::Completed;
            self.events
                .push(WorkflowEvent::WorkflowCompleted { workflow_id });
        }

        self.events.push(WorkflowEvent::StepCompleted {
            workflow_id,
            step_id,
            compute_time_ms,
        });

        Ok(())
    }

    /// Emit a step-failed event and update workflow state.
    pub fn notify_step_failed(
        &mut self,
        workflow_id: WorkflowId,
        step_id: StepId,
        reason: String,
    ) -> Result<(), OrchestratorError> {
        let state = self
            .workflows
            .get_mut(&workflow_id)
            .ok_or(OrchestratorError::WorkflowNotFound(workflow_id))?;

        if state.parallel_steps_active > 0 {
            state.parallel_steps_active -= 1;
        }

        self.events.push(WorkflowEvent::StepFailed {
            workflow_id,
            step_id,
            reason: reason.clone(),
        });

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Task 8.3: Dynamic step addition
    // -----------------------------------------------------------------------

    /// Add a new step to an active workflow's DAG during execution.
    ///
    /// The agent can decide to add new steps based on results from previous steps.
    /// The new step is validated to ensure it doesn't create cycles, then inserted
    /// into the DAG. If all its dependencies are already completed, it is marked Ready.
    ///
    /// # Arguments
    ///
    /// * `workflow_id` - The workflow to add the step to.
    /// * `step` - The new plan step to add.
    /// * `depends_on` - Step IDs that the new step depends on (must exist in the DAG).
    ///
    /// # Errors
    ///
    /// * `WorkflowNotFound` - The workflow doesn't exist.
    /// * `WorkflowAlreadyComplete` - The workflow is in a terminal state.
    /// * `CycleDetected` - Adding this step would create a cycle.
    /// * `DagBuildFailed` - A dependency step ID doesn't exist in the DAG.
    ///
    /// Satisfies FR-2.5: Steps can be dynamically added during execution.
    pub fn add_step(
        &mut self,
        workflow_id: WorkflowId,
        step: AgentPlanStep,
        depends_on: Vec<StepId>,
    ) -> Result<StepId, OrchestratorError> {
        let state = self
            .workflows
            .get_mut(&workflow_id)
            .ok_or(OrchestratorError::WorkflowNotFound(workflow_id))?;

        // Check workflow is still active
        match &state.status {
            WorkflowStatus::Completed | WorkflowStatus::Failed { .. } => {
                return Err(OrchestratorError::WorkflowAlreadyComplete);
            }
            _ => {}
        }

        // Validate all dependency step IDs exist in the DAG
        for &dep_id in &depends_on {
            if !state.dag.steps.contains_key(&dep_id) {
                return Err(OrchestratorError::DagBuildFailed(format!(
                    "Dependency step {} does not exist in the DAG",
                    dep_id
                )));
            }
        }

        // Create the new execution step
        let new_step_id = uuid::Uuid::new_v4();
        let sensitivity = step
            .sensitivity
            .clone()
            .unwrap_or(PromptSensitivity::NonSensitive);
        let estimated_compute_ms = if step.estimated_compute_ms > 0 {
            step.estimated_compute_ms
        } else {
            1000
        };

        let exec_step = ExecutionStep {
            step_id: new_step_id,
            description: step.description.clone(),
            required_model: step.model.clone(),
            required_tools: step.tools.clone(),
            sensitivity,
            estimated_compute_ms,
            input_dependencies: depends_on.clone(),
            status: StepStatus::Pending,
            assigned_node: None,
            result: None,
        };

        // Add edges and step to the DAG temporarily to check for cycles
        let new_edges: Vec<(StepId, StepId)> =
            depends_on.iter().map(|&dep| (dep, new_step_id)).collect();

        // Insert step and edges
        state.dag.steps.insert(new_step_id, exec_step);
        state.dag.edges.extend(new_edges.iter().copied());

        // Validate no cycles were introduced
        if state.dag.topological_sort().is_none() {
            // Rollback: remove the step and edges
            state.dag.steps.remove(&new_step_id);
            for edge in &new_edges {
                state.dag.edges.retain(|e| e != edge);
            }
            return Err(OrchestratorError::CycleDetected);
        }

        // Propagate sensitivity from dependencies
        let any_dep_sensitive = depends_on.iter().any(|dep_id| {
            state
                .dag
                .steps
                .get(dep_id)
                .map(|s| s.sensitivity == PromptSensitivity::Sensitive)
                .unwrap_or(false)
        });

        if any_dep_sensitive {
            if let Some(new_step) = state.dag.steps.get_mut(&new_step_id) {
                new_step.sensitivity = PromptSensitivity::Sensitive;
            }
        }

        // Check if all dependencies are already completed — if so, mark as Ready
        let all_deps_complete = depends_on.iter().all(|dep_id| {
            state
                .dag
                .steps
                .get(dep_id)
                .map(|s| matches!(s.status, StepStatus::Completed))
                .unwrap_or(false)
        });

        // Also handle the case where there are no dependencies
        if all_deps_complete || depends_on.is_empty() {
            if let Some(new_step) = state.dag.steps.get_mut(&new_step_id) {
                new_step.status = StepStatus::Ready;
            }
        }

        // Update total steps count
        state.total_steps += 1;

        // Update root steps if the new step has no dependencies
        if depends_on.is_empty() {
            state.dag.root_steps.push(new_step_id);
        }

        // Emit event
        self.events.push(WorkflowEvent::StepAdded {
            workflow_id,
            step_id: new_step_id,
            description: step.description,
        });

        Ok(new_step_id)
    }
}

// ---------------------------------------------------------------------------
// Unit Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::dag::{AgentPlan, AgentPlanStep, PromptSensitivity, StepStatus};

    fn test_config() -> DistributedAgentConfig {
        DistributedAgentConfig::default()
    }

    fn test_node_id() -> NodeId {
        uuid::Uuid::new_v4()
    }

    fn make_plan_step(description: &str, depends_on: Vec<usize>) -> AgentPlanStep {
        AgentPlanStep {
            description: description.to_string(),
            model: None,
            tools: Vec::new(),
            depends_on,
            sensitivity: None,
            estimated_compute_ms: 1000,
        }
    }

    fn make_simple_plan() -> AgentPlan {
        AgentPlan {
            name: "test-agent".to_string(),
            steps: vec![
                make_plan_step("Step A", vec![]),
                make_plan_step("Step B", vec![]),
                make_plan_step("Step C", vec![0, 1]),
            ],
        }
    }

    // -----------------------------------------------------------------------
    // Task 8.1: Workflow lifecycle tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_start_workflow_creates_correct_state() {
        let node_id = test_node_id();
        let mut orchestrator = WorkflowOrchestrator::new(node_id, test_config());
        let plan = make_simple_plan();

        let workflow_id = orchestrator.start_workflow(&plan).unwrap();

        let state = orchestrator.get_workflow_status(workflow_id).unwrap();
        assert_eq!(state.workflow_id, workflow_id);
        assert_eq!(state.agent_id, "test-agent");
        assert_eq!(state.requesting_node, node_id);
        assert_eq!(state.status, WorkflowStatus::Running);
        assert_eq!(state.total_steps, 3);
        assert_eq!(state.completed_steps, 0);
        assert_eq!(state.parallel_steps_active, 0);
    }

    #[test]
    fn test_start_workflow_marks_root_steps_ready() {
        let node_id = test_node_id();
        let mut orchestrator = WorkflowOrchestrator::new(node_id, test_config());
        let plan = make_simple_plan();

        let workflow_id = orchestrator.start_workflow(&plan).unwrap();

        let state = orchestrator.get_workflow_status(workflow_id).unwrap();

        // Root steps (A and B) should be Ready, C should be Pending
        let ready_count = state
            .dag
            .steps
            .values()
            .filter(|s| s.status == StepStatus::Ready)
            .count();
        let pending_count = state
            .dag
            .steps
            .values()
            .filter(|s| s.status == StepStatus::Pending)
            .count();

        assert_eq!(ready_count, 2); // A and B
        assert_eq!(pending_count, 1); // C
    }

    #[test]
    fn test_start_workflow_emits_event() {
        let node_id = test_node_id();
        let mut orchestrator = WorkflowOrchestrator::new(node_id, test_config());
        let plan = make_simple_plan();

        let workflow_id = orchestrator.start_workflow(&plan).unwrap();
        let events = orchestrator.drain_events();

        assert_eq!(events.len(), 1);
        match &events[0] {
            WorkflowEvent::WorkflowStarted {
                workflow_id: wid,
                total_steps,
            } => {
                assert_eq!(*wid, workflow_id);
                assert_eq!(*total_steps, 3);
            }
            _ => panic!("Expected WorkflowStarted event"),
        }
    }

    #[test]
    fn test_start_workflow_rejects_empty_plan() {
        let node_id = test_node_id();
        let mut orchestrator = WorkflowOrchestrator::new(node_id, test_config());
        let plan = AgentPlan {
            name: "empty".to_string(),
            steps: Vec::new(),
        };

        let err = orchestrator.start_workflow(&plan).unwrap_err();
        assert!(matches!(err, OrchestratorError::DagBuildFailed(_)));
    }

    #[test]
    fn test_cancel_workflow_marks_all_steps_cancelled() {
        let node_id = test_node_id();
        let mut orchestrator = WorkflowOrchestrator::new(node_id, test_config());
        let plan = make_simple_plan();

        let workflow_id = orchestrator.start_workflow(&plan).unwrap();
        orchestrator.cancel_workflow(workflow_id).unwrap();

        let state = orchestrator.get_workflow_status(workflow_id).unwrap();

        // All steps should be cancelled
        for step in state.dag.steps.values() {
            assert_eq!(step.status, StepStatus::Cancelled);
        }

        // Workflow should be Failed
        assert!(matches!(state.status, WorkflowStatus::Failed { .. }));
    }

    #[test]
    fn test_cancel_workflow_emits_failed_event() {
        let node_id = test_node_id();
        let mut orchestrator = WorkflowOrchestrator::new(node_id, test_config());
        let plan = make_simple_plan();

        let workflow_id = orchestrator.start_workflow(&plan).unwrap();
        orchestrator.drain_events(); // Clear start event
        orchestrator.cancel_workflow(workflow_id).unwrap();

        let events = orchestrator.drain_events();
        assert_eq!(events.len(), 1);
        match &events[0] {
            WorkflowEvent::WorkflowFailed { workflow_id: wid, reason } => {
                assert_eq!(*wid, workflow_id);
                assert!(reason.contains("cancelled"));
            }
            _ => panic!("Expected WorkflowFailed event"),
        }
    }

    #[test]
    fn test_cancel_nonexistent_workflow_returns_error() {
        let node_id = test_node_id();
        let mut orchestrator = WorkflowOrchestrator::new(node_id, test_config());
        let fake_id = uuid::Uuid::new_v4();

        let err = orchestrator.cancel_workflow(fake_id).unwrap_err();
        assert_eq!(err, OrchestratorError::WorkflowNotFound(fake_id));
    }

    #[test]
    fn test_cancel_already_complete_workflow_returns_error() {
        let node_id = test_node_id();
        let mut orchestrator = WorkflowOrchestrator::new(node_id, test_config());
        let plan = make_simple_plan();

        let workflow_id = orchestrator.start_workflow(&plan).unwrap();
        orchestrator.cancel_workflow(workflow_id).unwrap();

        // Try to cancel again
        let err = orchestrator.cancel_workflow(workflow_id).unwrap_err();
        assert_eq!(err, OrchestratorError::WorkflowAlreadyComplete);
    }

    #[test]
    fn test_get_workflow_status_returns_none_for_unknown() {
        let node_id = test_node_id();
        let orchestrator = WorkflowOrchestrator::new(node_id, test_config());
        let fake_id = uuid::Uuid::new_v4();

        assert!(orchestrator.get_workflow_status(fake_id).is_none());
    }

    #[test]
    fn test_list_active_workflows() {
        let node_id = test_node_id();
        let mut orchestrator = WorkflowOrchestrator::new(node_id, test_config());
        let plan = make_simple_plan();

        let wf1 = orchestrator.start_workflow(&plan).unwrap();
        let wf2 = orchestrator.start_workflow(&plan).unwrap();

        let active = orchestrator.list_active_workflows();
        assert_eq!(active.len(), 2);
        assert!(active.contains(&wf1));
        assert!(active.contains(&wf2));

        // Cancel one
        orchestrator.cancel_workflow(wf1).unwrap();
        let active = orchestrator.list_active_workflows();
        assert_eq!(active.len(), 1);
        assert!(active.contains(&wf2));
    }

    #[test]
    fn test_orchestrator_node_id_always_equals_local() {
        let node_id = test_node_id();
        let mut orchestrator = WorkflowOrchestrator::new(node_id, test_config());
        let plan = make_simple_plan();

        let workflow_id = orchestrator.start_workflow(&plan).unwrap();
        let state = orchestrator.get_workflow_status(workflow_id).unwrap();

        // Orchestrator locality: requesting_node == local_node_id
        assert_eq!(state.requesting_node, node_id);
        assert_eq!(orchestrator.local_node_id(), node_id);
    }

    // -----------------------------------------------------------------------
    // Task 8.2: Progress reporting tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_progress_shows_correct_counts() {
        let node_id = test_node_id();
        let mut orchestrator = WorkflowOrchestrator::new(node_id, test_config());
        let plan = make_simple_plan();

        let workflow_id = orchestrator.start_workflow(&plan).unwrap();
        let progress = orchestrator.get_workflow_progress(workflow_id).unwrap();

        // 2 root steps are Ready (waiting), 1 is Pending (waiting)
        assert_eq!(progress.total_steps, 3);
        assert_eq!(progress.completed_steps, 0);
        assert_eq!(progress.running_steps, 0);
        assert_eq!(progress.waiting_steps, 3);
        assert_eq!(progress.failed_steps, 0);
        assert_eq!(progress.cancelled_steps, 0);
        assert_eq!(progress.status, WorkflowStatus::Running);
    }

    #[test]
    fn test_progress_after_cancellation() {
        let node_id = test_node_id();
        let mut orchestrator = WorkflowOrchestrator::new(node_id, test_config());
        let plan = make_simple_plan();

        let workflow_id = orchestrator.start_workflow(&plan).unwrap();
        orchestrator.cancel_workflow(workflow_id).unwrap();

        let progress = orchestrator.get_workflow_progress(workflow_id).unwrap();
        assert_eq!(progress.cancelled_steps, 3);
        assert_eq!(progress.running_steps, 0);
        assert_eq!(progress.waiting_steps, 0);
    }

    #[test]
    fn test_progress_estimated_time() {
        let node_id = test_node_id();
        let mut orchestrator = WorkflowOrchestrator::new(node_id, test_config());

        // Create a plan with known compute estimates
        let plan = AgentPlan {
            name: "timed".to_string(),
            steps: vec![
                AgentPlanStep {
                    description: "Fast step".to_string(),
                    model: None,
                    tools: Vec::new(),
                    depends_on: vec![],
                    sensitivity: None,
                    estimated_compute_ms: 2000,
                },
                AgentPlanStep {
                    description: "Slow step".to_string(),
                    model: None,
                    tools: Vec::new(),
                    depends_on: vec![0],
                    sensitivity: None,
                    estimated_compute_ms: 5000,
                },
            ],
        };

        let workflow_id = orchestrator.start_workflow(&plan).unwrap();
        let progress = orchestrator.get_workflow_progress(workflow_id).unwrap();

        // Both steps are waiting (Pending/Ready), so estimated = 2000 + 5000 = 7000
        assert_eq!(progress.estimated_remaining_ms, 7000);
    }

    #[test]
    fn test_progress_returns_none_for_unknown_workflow() {
        let node_id = test_node_id();
        let orchestrator = WorkflowOrchestrator::new(node_id, test_config());
        let fake_id = uuid::Uuid::new_v4();

        assert!(orchestrator.get_workflow_progress(fake_id).is_none());
    }

    #[test]
    fn test_notify_step_started_increments_active() {
        let node_id = test_node_id();
        let mut orchestrator = WorkflowOrchestrator::new(node_id, test_config());
        let plan = make_simple_plan();

        let workflow_id = orchestrator.start_workflow(&plan).unwrap();
        let step_id = *orchestrator
            .get_workflow_status(workflow_id)
            .unwrap()
            .dag
            .root_steps
            .first()
            .unwrap();

        orchestrator
            .notify_step_started(workflow_id, step_id)
            .unwrap();

        let state = orchestrator.get_workflow_status(workflow_id).unwrap();
        assert_eq!(state.parallel_steps_active, 1);
    }

    #[test]
    fn test_notify_step_completed_updates_counters() {
        let node_id = test_node_id();
        let mut orchestrator = WorkflowOrchestrator::new(node_id, test_config());
        let plan = AgentPlan {
            name: "single".to_string(),
            steps: vec![make_plan_step("Only step", vec![])],
        };

        let workflow_id = orchestrator.start_workflow(&plan).unwrap();
        let step_id = *orchestrator
            .get_workflow_status(workflow_id)
            .unwrap()
            .dag
            .root_steps
            .first()
            .unwrap();

        // Mark step as completed in the DAG
        orchestrator
            .workflows
            .get_mut(&workflow_id)
            .unwrap()
            .dag
            .steps
            .get_mut(&step_id)
            .unwrap()
            .status = StepStatus::Completed;

        orchestrator.notify_step_started(workflow_id, step_id).unwrap();
        orchestrator
            .notify_step_completed(workflow_id, step_id, 500)
            .unwrap();

        let state = orchestrator.get_workflow_status(workflow_id).unwrap();
        assert_eq!(state.completed_steps, 1);
        assert_eq!(state.parallel_steps_active, 0);
        // Single step workflow should be marked complete
        assert_eq!(state.status, WorkflowStatus::Completed);
    }

    // -----------------------------------------------------------------------
    // Task 8.3: Dynamic step addition tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_add_step_to_active_workflow() {
        let node_id = test_node_id();
        let mut orchestrator = WorkflowOrchestrator::new(node_id, test_config());
        let plan = AgentPlan {
            name: "dynamic".to_string(),
            steps: vec![make_plan_step("Initial step", vec![])],
        };

        let workflow_id = orchestrator.start_workflow(&plan).unwrap();

        // Get the initial step's ID
        let initial_step_id = *orchestrator
            .get_workflow_status(workflow_id)
            .unwrap()
            .dag
            .steps
            .keys()
            .next()
            .unwrap();

        // Add a new step that depends on the initial step
        let new_step = AgentPlanStep {
            description: "Dynamic step".to_string(),
            model: None,
            tools: vec!["browser".to_string()],
            depends_on: vec![], // Not used — depends_on param is used instead
            sensitivity: None,
            estimated_compute_ms: 2000,
        };

        let new_step_id = orchestrator
            .add_step(workflow_id, new_step, vec![initial_step_id])
            .unwrap();

        let state = orchestrator.get_workflow_status(workflow_id).unwrap();
        assert_eq!(state.total_steps, 2);
        assert!(state.dag.steps.contains_key(&new_step_id));

        // New step should be Pending (dependency not yet completed)
        let new_step = state.dag.steps.get(&new_step_id).unwrap();
        assert_eq!(new_step.status, StepStatus::Pending);
        assert_eq!(new_step.description, "Dynamic step");
        assert_eq!(new_step.required_tools, vec!["browser".to_string()]);
    }

    #[test]
    fn test_add_step_with_completed_deps_is_ready() {
        let node_id = test_node_id();
        let mut orchestrator = WorkflowOrchestrator::new(node_id, test_config());
        let plan = AgentPlan {
            name: "dynamic".to_string(),
            steps: vec![make_plan_step("Initial step", vec![])],
        };

        let workflow_id = orchestrator.start_workflow(&plan).unwrap();

        // Mark the initial step as completed
        let initial_step_id = *orchestrator
            .workflows
            .get(&workflow_id)
            .unwrap()
            .dag
            .steps
            .keys()
            .next()
            .unwrap();

        orchestrator
            .workflows
            .get_mut(&workflow_id)
            .unwrap()
            .dag
            .steps
            .get_mut(&initial_step_id)
            .unwrap()
            .status = StepStatus::Completed;

        // Add a new step depending on the completed step
        let new_step = make_plan_step("Follow-up step", vec![]);
        let new_step_id = orchestrator
            .add_step(workflow_id, new_step, vec![initial_step_id])
            .unwrap();

        let state = orchestrator.get_workflow_status(workflow_id).unwrap();
        let added = state.dag.steps.get(&new_step_id).unwrap();
        assert_eq!(added.status, StepStatus::Ready);
    }

    #[test]
    fn test_add_step_with_no_deps_is_ready() {
        let node_id = test_node_id();
        let mut orchestrator = WorkflowOrchestrator::new(node_id, test_config());
        let plan = AgentPlan {
            name: "dynamic".to_string(),
            steps: vec![make_plan_step("Initial step", vec![])],
        };

        let workflow_id = orchestrator.start_workflow(&plan).unwrap();

        // Add a step with no dependencies
        let new_step = make_plan_step("Independent step", vec![]);
        let new_step_id = orchestrator
            .add_step(workflow_id, new_step, vec![])
            .unwrap();

        let state = orchestrator.get_workflow_status(workflow_id).unwrap();
        let added = state.dag.steps.get(&new_step_id).unwrap();
        assert_eq!(added.status, StepStatus::Ready);
        assert!(state.dag.root_steps.contains(&new_step_id));
    }

    #[test]
    fn test_add_step_rejects_cycle() {
        let node_id = test_node_id();
        let mut orchestrator = WorkflowOrchestrator::new(node_id, test_config());

        // Create a plan: A -> B
        let plan = AgentPlan {
            name: "cycle-test".to_string(),
            steps: vec![
                make_plan_step("Step A", vec![]),
                make_plan_step("Step B", vec![0]),
            ],
        };

        let workflow_id = orchestrator.start_workflow(&plan).unwrap();

        // Get step IDs
        let state = orchestrator.get_workflow_status(workflow_id).unwrap();
        let step_a_id = state.dag.root_steps[0]; // A is the root
        let step_b_id = *state
            .dag
            .steps
            .keys()
            .find(|&&id| id != step_a_id)
            .unwrap();

        // Try to add a step that depends on B but that A depends on (creating A -> B -> new -> A)
        // Actually, let's add a step C that depends on B, then try to make A depend on C
        // We can't modify existing edges, but we can try to add a step that B depends on
        // which also depends on B (creating a cycle: B -> new -> B)

        // Add step C depending on B
        let step_c = make_plan_step("Step C", vec![]);
        let step_c_id = orchestrator
            .add_step(workflow_id, step_c, vec![step_b_id])
            .unwrap();

        // Now try to add step D that depends on C but that A depends on
        // Actually, the simplest cycle: add a step that depends on step_b_id,
        // then add an edge from the new step back to step_a_id
        // We can't do that directly, but we can try to add a step that
        // step_a_id depends on (which would require modifying A's deps).
        // Instead, let's create a scenario where adding a step creates a cycle
        // by having the new step depend on C, and then adding an edge from new -> A
        // which already has A -> B -> C.

        // The simplest way: manually add an edge from C -> A to create a cycle,
        // then try to add a step. But that's modifying internals.
        // Let's just verify that if we try to add a step where the DAG would cycle:

        // Add edge C -> A manually to create a potential cycle scenario
        orchestrator
            .workflows
            .get_mut(&workflow_id)
            .unwrap()
            .dag
            .edges
            .push((step_c_id, step_a_id));

        // Now the DAG has a cycle: A -> B -> C -> A
        // Try to add another step — the cycle already exists, so topological sort will fail
        let step_d = make_plan_step("Step D", vec![]);
        let err = orchestrator
            .add_step(workflow_id, step_d, vec![step_c_id])
            .unwrap_err();
        assert_eq!(err, OrchestratorError::CycleDetected);
    }

    #[test]
    fn test_add_step_to_nonexistent_workflow() {
        let node_id = test_node_id();
        let mut orchestrator = WorkflowOrchestrator::new(node_id, test_config());
        let fake_id = uuid::Uuid::new_v4();

        let step = make_plan_step("New step", vec![]);
        let err = orchestrator.add_step(fake_id, step, vec![]).unwrap_err();
        assert_eq!(err, OrchestratorError::WorkflowNotFound(fake_id));
    }

    #[test]
    fn test_add_step_to_completed_workflow() {
        let node_id = test_node_id();
        let mut orchestrator = WorkflowOrchestrator::new(node_id, test_config());
        let plan = make_simple_plan();

        let workflow_id = orchestrator.start_workflow(&plan).unwrap();
        orchestrator.cancel_workflow(workflow_id).unwrap();

        let step = make_plan_step("Late step", vec![]);
        let err = orchestrator
            .add_step(workflow_id, step, vec![])
            .unwrap_err();
        assert_eq!(err, OrchestratorError::WorkflowAlreadyComplete);
    }

    #[test]
    fn test_add_step_with_invalid_dependency() {
        let node_id = test_node_id();
        let mut orchestrator = WorkflowOrchestrator::new(node_id, test_config());
        let plan = AgentPlan {
            name: "test".to_string(),
            steps: vec![make_plan_step("Step A", vec![])],
        };

        let workflow_id = orchestrator.start_workflow(&plan).unwrap();
        let fake_dep = uuid::Uuid::new_v4();

        let step = make_plan_step("Bad dep step", vec![]);
        let err = orchestrator
            .add_step(workflow_id, step, vec![fake_dep])
            .unwrap_err();
        assert!(matches!(err, OrchestratorError::DagBuildFailed(_)));
    }

    #[test]
    fn test_add_step_emits_event() {
        let node_id = test_node_id();
        let mut orchestrator = WorkflowOrchestrator::new(node_id, test_config());
        let plan = AgentPlan {
            name: "test".to_string(),
            steps: vec![make_plan_step("Step A", vec![])],
        };

        let workflow_id = orchestrator.start_workflow(&plan).unwrap();
        orchestrator.drain_events(); // Clear start event

        let step = make_plan_step("Dynamic step", vec![]);
        let new_step_id = orchestrator
            .add_step(workflow_id, step, vec![])
            .unwrap();

        let events = orchestrator.drain_events();
        assert_eq!(events.len(), 1);
        match &events[0] {
            WorkflowEvent::StepAdded {
                workflow_id: wid,
                step_id,
                description,
            } => {
                assert_eq!(*wid, workflow_id);
                assert_eq!(*step_id, new_step_id);
                assert_eq!(description, "Dynamic step");
            }
            _ => panic!("Expected StepAdded event"),
        }
    }

    #[test]
    fn test_add_step_propagates_sensitivity() {
        let node_id = test_node_id();
        let mut orchestrator = WorkflowOrchestrator::new(node_id, test_config());

        let plan = AgentPlan {
            name: "sensitive".to_string(),
            steps: vec![AgentPlanStep {
                description: "Sensitive step".to_string(),
                model: None,
                tools: Vec::new(),
                depends_on: vec![],
                sensitivity: Some(PromptSensitivity::Sensitive),
                estimated_compute_ms: 1000,
            }],
        };

        let workflow_id = orchestrator.start_workflow(&plan).unwrap();
        let sensitive_step_id = *orchestrator
            .get_workflow_status(workflow_id)
            .unwrap()
            .dag
            .steps
            .keys()
            .next()
            .unwrap();

        // Add a non-sensitive step depending on the sensitive one
        let new_step = AgentPlanStep {
            description: "Should become sensitive".to_string(),
            model: None,
            tools: Vec::new(),
            depends_on: vec![],
            sensitivity: Some(PromptSensitivity::NonSensitive),
            estimated_compute_ms: 1000,
        };

        let new_step_id = orchestrator
            .add_step(workflow_id, new_step, vec![sensitive_step_id])
            .unwrap();

        let state = orchestrator.get_workflow_status(workflow_id).unwrap();
        let added = state.dag.steps.get(&new_step_id).unwrap();
        assert_eq!(added.sensitivity, PromptSensitivity::Sensitive);
    }

    // -----------------------------------------------------------------------
    // Task 8.4: Property Test — Orchestrator Locality
    // -----------------------------------------------------------------------

    use proptest::prelude::*;

    /// Strategy to generate a random NodeId (UUID v4).
    fn arb_node_id() -> impl Strategy<Value = NodeId> {
        (any::<u128>()).prop_map(|bits| uuid::Uuid::from_u128(bits))
    }

    /// Strategy to generate a simple agent plan with 1-5 steps.
    fn arb_simple_plan() -> impl Strategy<Value = AgentPlan> {
        (1usize..=5).prop_flat_map(|num_steps| {
            proptest::collection::vec(
                (0usize..num_steps).prop_flat_map(move |idx| {
                    // Each step can depend on any earlier step
                    let deps_strategy = if idx == 0 {
                        Just(vec![]).boxed()
                    } else {
                        proptest::collection::vec(0usize..idx, 0..=idx.min(2)).boxed()
                    };
                    deps_strategy.prop_map(move |deps| {
                        AgentPlanStep {
                            description: format!("Step {}", idx),
                            model: None,
                            tools: Vec::new(),
                            depends_on: deps,
                            sensitivity: None,
                            estimated_compute_ms: 1000,
                        }
                    })
                }),
                num_steps..=num_steps,
            )
            .prop_map(|steps| AgentPlan {
                name: "prop-test-agent".to_string(),
                steps,
            })
        })
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(200))]

        /// **Validates: Requirements FR-8.1, Correctness Property 10**
        ///
        /// Property 10: Orchestrator locality — the orchestrator node_id always
        /// equals the requesting node_id; it is never reassigned during workflow
        /// execution. We verify this across workflow lifecycle operations:
        /// start, cancel, dynamic step addition, and progress queries.
        #[test]
        fn prop_orchestrator_locality_never_reassigned(
            node_id in arb_node_id(),
            plan in arb_simple_plan(),
        ) {
            let config = test_config();
            let mut orchestrator = WorkflowOrchestrator::new(node_id, config);

            // Invariant: local_node_id() always equals the original node_id
            prop_assert_eq!(
                orchestrator.local_node_id(), node_id,
                "Orchestrator local_node_id should equal the node_id it was created with"
            );

            // Start a workflow
            let workflow_id = orchestrator.start_workflow(&plan).unwrap();

            // After starting workflow: local_node_id unchanged
            prop_assert_eq!(
                orchestrator.local_node_id(), node_id,
                "local_node_id changed after start_workflow"
            );

            // Verify requesting_node in workflow state equals local_node_id
            let state = orchestrator.get_workflow_status(workflow_id).unwrap();
            prop_assert_eq!(
                state.requesting_node, node_id,
                "WorkflowState.requesting_node should equal orchestrator's local_node_id"
            );

            // Start a second workflow — locality still holds
            let plan2 = AgentPlan {
                name: "second-workflow".to_string(),
                steps: vec![AgentPlanStep {
                    description: "Single step".to_string(),
                    model: None,
                    tools: Vec::new(),
                    depends_on: vec![],
                    sensitivity: None,
                    estimated_compute_ms: 500,
                }],
            };
            let wf2 = orchestrator.start_workflow(&plan2).unwrap();

            prop_assert_eq!(
                orchestrator.local_node_id(), node_id,
                "local_node_id changed after starting second workflow"
            );

            let state2 = orchestrator.get_workflow_status(wf2).unwrap();
            prop_assert_eq!(
                state2.requesting_node, node_id,
                "Second workflow requesting_node should equal local_node_id"
            );

            // Add a dynamic step — locality still holds
            let step_id = *orchestrator
                .get_workflow_status(workflow_id)
                .unwrap()
                .dag
                .steps
                .keys()
                .next()
                .unwrap();

            let dynamic_step = AgentPlanStep {
                description: "Dynamic".to_string(),
                model: None,
                tools: Vec::new(),
                depends_on: vec![],
                sensitivity: None,
                estimated_compute_ms: 100,
            };
            let _ = orchestrator.add_step(workflow_id, dynamic_step, vec![step_id]);

            prop_assert_eq!(
                orchestrator.local_node_id(), node_id,
                "local_node_id changed after add_step"
            );

            // Cancel workflow — locality still holds
            orchestrator.cancel_workflow(workflow_id).unwrap();

            prop_assert_eq!(
                orchestrator.local_node_id(), node_id,
                "local_node_id changed after cancel_workflow"
            );

            // Drain events — locality still holds
            orchestrator.drain_events();

            prop_assert_eq!(
                orchestrator.local_node_id(), node_id,
                "local_node_id changed after drain_events"
            );
        }
    }
}
