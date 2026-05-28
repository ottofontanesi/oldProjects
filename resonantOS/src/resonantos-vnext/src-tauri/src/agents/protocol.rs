// Distributed Agent Execution — Protocol messages
// Phase 15: Workflow state and inter-node protocol messages
//
// Defines the workflow lifecycle state machine and the protocol messages
// exchanged between orchestrator and worker nodes during step execution.
//
// Satisfies FR-8.4: Orchestrator communicates with worker nodes via Phase 10 transport.
// Satisfies FR-5.1: Data transfer between steps on different nodes via transport.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::agents::dag::{ExecutionDag, ExecutionStep, StepId, StepResult, WorkflowId};
use crate::network::registry::NodeId;

/// The overall state of a running workflow.
///
/// Tracks the DAG, progress counters, and optional checkpoint for resume.
/// The orchestrator maintains one `WorkflowState` per active workflow.
///
/// Satisfies FR-8.2: Orchestrator manages parallel execution, collects results,
/// handles failures, reports progress.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowState {
    /// Unique identifier for this workflow instance.
    pub workflow_id: WorkflowId,

    /// Identifier of the agent that initiated this workflow.
    pub agent_id: String,

    /// Node where the orchestrator is running (always the requesting node).
    pub requesting_node: NodeId,

    /// The execution DAG for this workflow.
    pub dag: ExecutionDag,

    /// When the workflow started executing.
    pub started_at: chrono::DateTime<chrono::Utc>,

    /// Current high-level status of the workflow.
    pub status: WorkflowStatus,

    /// Number of steps currently executing in parallel.
    pub parallel_steps_active: u32,

    /// Total number of steps in the DAG.
    pub total_steps: u32,

    /// Number of steps that have completed successfully.
    pub completed_steps: u32,

    /// Optional checkpoint for resume after crash (FR-7.5).
    pub checkpoint: Option<WorkflowCheckpoint>,
}

/// High-level status of a workflow.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum WorkflowStatus {
    /// Workflow is actively executing steps.
    Running,
    /// All steps completed successfully.
    Completed,
    /// A step failed and the workflow cannot continue.
    Failed {
        /// The step that caused the failure.
        failed_step: StepId,
        /// Human-readable reason for the failure.
        reason: String,
    },
    /// Workflow is paused (waiting for user input or resource availability).
    Paused,
    /// Workflow progress has been saved for resume after restart (FR-7.5).
    Checkpointed,
}

/// A snapshot of workflow progress for crash recovery.
///
/// Satisfies FR-7.5: Long-running workflows checkpoint their progress —
/// can resume after app restart.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowCheckpoint {
    /// When this checkpoint was created.
    pub checkpointed_at: chrono::DateTime<chrono::Utc>,

    /// Results from steps that completed before the checkpoint.
    /// These do not need to be re-executed on resume.
    pub completed_step_results: HashMap<StepId, StepResult>,

    /// Steps that were pending (not yet completed) at checkpoint time.
    /// These need to be re-dispatched on resume.
    pub pending_steps: Vec<StepId>,
}

/// Protocol messages exchanged between orchestrator and worker nodes.
///
/// The orchestrator sends `ExecuteStep` and `CancelStep` to workers.
/// Workers send `StepStarted`, `StepCompleted`, `StepFailed`, and `StepProgress`
/// back to the orchestrator.
///
/// These messages are serialized and sent via Phase 10 transport using
/// `RequestType::AgentStepDispatch` (orchestrator → worker) and
/// `RequestType::AgentStepResult` (worker → orchestrator).
///
/// Satisfies FR-8.4: Orchestrator communicates with worker nodes via transport.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AgentStepMessage {
    // ─── Orchestrator → Worker ───────────────────────────────────────────

    /// Dispatch a step to a worker node for execution.
    ExecuteStep {
        /// Workflow this step belongs to.
        workflow_id: WorkflowId,
        /// The step to execute (includes model/tool requirements).
        step: ExecutionStep,
        /// Input data from completed dependency steps, keyed by step ID.
        input_data: HashMap<StepId, Vec<u8>>,
    },

    /// Cancel a running step (due to workflow abort or speculative execution).
    CancelStep {
        /// Workflow this step belongs to.
        workflow_id: WorkflowId,
        /// Step to cancel.
        step_id: StepId,
        /// Reason for cancellation.
        reason: String,
    },

    // ─── Worker → Orchestrator ───────────────────────────────────────────

    /// Worker confirms it has started executing the step.
    StepStarted {
        /// Workflow this step belongs to.
        workflow_id: WorkflowId,
        /// Step that started.
        step_id: StepId,
        /// Node executing the step.
        node_id: NodeId,
    },

    /// Worker reports successful step completion with result.
    StepCompleted {
        /// Workflow this step belongs to.
        workflow_id: WorkflowId,
        /// Step that completed.
        step_id: StepId,
        /// The execution result (output data, timing, etc.).
        result: StepResult,
    },

    /// Worker reports step failure.
    StepFailed {
        /// Workflow this step belongs to.
        workflow_id: WorkflowId,
        /// Step that failed.
        step_id: StepId,
        /// Human-readable error description.
        error: String,
        /// Whether this failure is retryable on another node.
        retryable: bool,
    },

    /// Worker reports intermediate progress on a long-running step.
    StepProgress {
        /// Workflow this step belongs to.
        workflow_id: WorkflowId,
        /// Step reporting progress.
        step_id: StepId,
        /// Completion percentage [0.0, 100.0].
        progress_percent: f32,
        /// Human-readable progress message.
        message: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_workflow_status_serialization() {
        let statuses = vec![
            WorkflowStatus::Running,
            WorkflowStatus::Completed,
            WorkflowStatus::Failed {
                failed_step: uuid::Uuid::new_v4(),
                reason: "timeout".to_string(),
            },
            WorkflowStatus::Paused,
            WorkflowStatus::Checkpointed,
        ];

        for status in &statuses {
            let json = serde_json::to_string(status).unwrap();
            let deserialized: WorkflowStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(&deserialized, status);
        }
    }

    #[test]
    fn test_workflow_state_serialization() {
        use crate::agents::dag::ExecutionDag;

        let state = WorkflowState {
            workflow_id: uuid::Uuid::new_v4(),
            agent_id: "research-agent".to_string(),
            requesting_node: uuid::Uuid::new_v4(),
            dag: ExecutionDag {
                workflow_id: uuid::Uuid::new_v4(),
                steps: HashMap::new(),
                edges: Vec::new(),
                root_steps: Vec::new(),
            },
            started_at: chrono::Utc::now(),
            status: WorkflowStatus::Running,
            parallel_steps_active: 2,
            total_steps: 5,
            completed_steps: 1,
            checkpoint: None,
        };

        let json = serde_json::to_string(&state).unwrap();
        let deserialized: WorkflowState = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.workflow_id, state.workflow_id);
        assert_eq!(deserialized.agent_id, "research-agent");
        assert_eq!(deserialized.parallel_steps_active, 2);
        assert_eq!(deserialized.total_steps, 5);
        assert_eq!(deserialized.completed_steps, 1);
        assert_eq!(deserialized.status, WorkflowStatus::Running);
    }

    #[test]
    fn test_workflow_checkpoint_serialization() {
        use crate::agents::dag::StepResult;

        let step_id = uuid::Uuid::new_v4();
        let mut completed = HashMap::new();
        completed.insert(
            step_id,
            StepResult {
                step_id,
                output_data: vec![42, 43, 44],
                output_size_bytes: 3,
                execution_node: uuid::Uuid::new_v4(),
                compute_time_ms: 1200,
                model_used: Some("qwen2.5:14b".to_string()),
                tools_used: vec!["browser".to_string()],
            },
        );

        let checkpoint = WorkflowCheckpoint {
            checkpointed_at: chrono::Utc::now(),
            completed_step_results: completed,
            pending_steps: vec![uuid::Uuid::new_v4(), uuid::Uuid::new_v4()],
        };

        let json = serde_json::to_string(&checkpoint).unwrap();
        let deserialized: WorkflowCheckpoint = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.completed_step_results.len(), 1);
        assert_eq!(deserialized.pending_steps.len(), 2);
        assert!(deserialized.completed_step_results.contains_key(&step_id));
    }

    #[test]
    fn test_agent_step_message_execute_step() {
        use crate::agents::dag::{ExecutionStep, PromptSensitivity, StepStatus};

        let workflow_id = uuid::Uuid::new_v4();
        let step_id = uuid::Uuid::new_v4();
        let dep_id = uuid::Uuid::new_v4();

        let mut input_data = HashMap::new();
        input_data.insert(dep_id, vec![1, 2, 3]);

        let msg = AgentStepMessage::ExecuteStep {
            workflow_id,
            step: ExecutionStep {
                step_id,
                description: "Search the web".to_string(),
                required_model: Some("qwen2.5:7b".to_string()),
                required_tools: vec!["browser".to_string()],
                sensitivity: PromptSensitivity::NonSensitive,
                estimated_compute_ms: 5000,
                input_dependencies: vec![dep_id],
                status: StepStatus::Dispatched,
                assigned_node: None,
                result: None,
            },
            input_data,
        };

        let json = serde_json::to_string(&msg).unwrap();
        let deserialized: AgentStepMessage = serde_json::from_str(&json).unwrap();

        match deserialized {
            AgentStepMessage::ExecuteStep {
                workflow_id: wid,
                step,
                input_data: data,
            } => {
                assert_eq!(wid, workflow_id);
                assert_eq!(step.step_id, step_id);
                assert_eq!(step.required_tools, vec!["browser".to_string()]);
                assert_eq!(data.len(), 1);
                assert_eq!(data[&dep_id], vec![1, 2, 3]);
            }
            _ => panic!("Expected ExecuteStep variant"),
        }
    }

    #[test]
    fn test_agent_step_message_cancel_step() {
        let msg = AgentStepMessage::CancelStep {
            workflow_id: uuid::Uuid::new_v4(),
            step_id: uuid::Uuid::new_v4(),
            reason: "Workflow aborted by user".to_string(),
        };

        let json = serde_json::to_string(&msg).unwrap();
        let deserialized: AgentStepMessage = serde_json::from_str(&json).unwrap();

        match deserialized {
            AgentStepMessage::CancelStep { reason, .. } => {
                assert_eq!(reason, "Workflow aborted by user");
            }
            _ => panic!("Expected CancelStep variant"),
        }
    }

    #[test]
    fn test_agent_step_message_step_started() {
        let workflow_id = uuid::Uuid::new_v4();
        let step_id = uuid::Uuid::new_v4();
        let node_id = uuid::Uuid::new_v4();

        let msg = AgentStepMessage::StepStarted {
            workflow_id,
            step_id,
            node_id,
        };

        let json = serde_json::to_string(&msg).unwrap();
        let deserialized: AgentStepMessage = serde_json::from_str(&json).unwrap();

        match deserialized {
            AgentStepMessage::StepStarted {
                workflow_id: wid,
                step_id: sid,
                node_id: nid,
            } => {
                assert_eq!(wid, workflow_id);
                assert_eq!(sid, step_id);
                assert_eq!(nid, node_id);
            }
            _ => panic!("Expected StepStarted variant"),
        }
    }

    #[test]
    fn test_agent_step_message_step_completed() {
        use crate::agents::dag::StepResult;

        let workflow_id = uuid::Uuid::new_v4();
        let step_id = uuid::Uuid::new_v4();

        let msg = AgentStepMessage::StepCompleted {
            workflow_id,
            step_id,
            result: StepResult {
                step_id,
                output_data: vec![10, 20, 30],
                output_size_bytes: 3,
                execution_node: uuid::Uuid::new_v4(),
                compute_time_ms: 800,
                model_used: None,
                tools_used: vec!["filesystem".to_string()],
            },
        };

        let json = serde_json::to_string(&msg).unwrap();
        let deserialized: AgentStepMessage = serde_json::from_str(&json).unwrap();

        match deserialized {
            AgentStepMessage::StepCompleted { result, .. } => {
                assert_eq!(result.step_id, step_id);
                assert_eq!(result.output_data, vec![10, 20, 30]);
                assert_eq!(result.compute_time_ms, 800);
            }
            _ => panic!("Expected StepCompleted variant"),
        }
    }

    #[test]
    fn test_agent_step_message_step_failed() {
        let msg = AgentStepMessage::StepFailed {
            workflow_id: uuid::Uuid::new_v4(),
            step_id: uuid::Uuid::new_v4(),
            error: "Model no longer loaded".to_string(),
            retryable: true,
        };

        let json = serde_json::to_string(&msg).unwrap();
        let deserialized: AgentStepMessage = serde_json::from_str(&json).unwrap();

        match deserialized {
            AgentStepMessage::StepFailed {
                error, retryable, ..
            } => {
                assert_eq!(error, "Model no longer loaded");
                assert!(retryable);
            }
            _ => panic!("Expected StepFailed variant"),
        }
    }

    #[test]
    fn test_agent_step_message_step_progress() {
        let msg = AgentStepMessage::StepProgress {
            workflow_id: uuid::Uuid::new_v4(),
            step_id: uuid::Uuid::new_v4(),
            progress_percent: 45.5,
            message: "Processing document 3 of 7".to_string(),
        };

        let json = serde_json::to_string(&msg).unwrap();
        let deserialized: AgentStepMessage = serde_json::from_str(&json).unwrap();

        match deserialized {
            AgentStepMessage::StepProgress {
                progress_percent,
                message,
                ..
            } => {
                assert!((progress_percent - 45.5).abs() < f32::EPSILON);
                assert_eq!(message, "Processing document 3 of 7");
            }
            _ => panic!("Expected StepProgress variant"),
        }
    }
}
