// Distributed Agent Execution — Step worker
// Phase 15: Execute a single step on a worker node (inference + tool call)
//
// The StepWorker handles incoming ExecuteStep messages from the orchestrator,
// verifies resource availability (model loaded, tools available), executes the
// step locally via the StepExecutor trait, and reports results back.
//
// Satisfies FR-8.4: Worker receives and executes steps dispatched by orchestrator.
// Satisfies FR-3.1: Verifies required model and tools before execution.
// Satisfies FR-1.4: Dynamic tool availability checking.
// Satisfies FR-7.1: Returns retryable failures when resources become unavailable.
// Satisfies FR-8.5: Reports progress during long-running steps.

use std::collections::HashMap;

use crate::agents::dag::{ExecutionStep, StepId, StepResult, WorkflowId};
use crate::agents::protocol::AgentStepMessage;
use crate::agents::tools::ToolCapability;
use crate::network::registry::NodeId;

// ---------------------------------------------------------------------------
// Step Execution Error
// ---------------------------------------------------------------------------

/// Errors that can occur during local step execution.
///
/// Each variant indicates whether the failure is retryable on another node.
#[derive(Debug, Clone, PartialEq)]
pub enum StepExecutionError {
    /// Model inference failed (e.g., OOM, model corrupted). Retryable on another node.
    ModelError(String),

    /// Tool invocation failed (e.g., tool crashed, permission denied). Retryable.
    ToolError(String),

    /// General execution error (e.g., invalid input, logic error). Non-retryable.
    ExecutionError(String),

    /// Step execution timed out. Retryable on another node (may have more resources).
    Timeout,
}

impl StepExecutionError {
    /// Whether this error type is retryable on an alternative node.
    pub fn is_retryable(&self) -> bool {
        match self {
            StepExecutionError::ModelError(_) => true,
            StepExecutionError::ToolError(_) => true,
            StepExecutionError::ExecutionError(_) => false,
            StepExecutionError::Timeout => true,
        }
    }
}

impl std::fmt::Display for StepExecutionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StepExecutionError::ModelError(msg) => write!(f, "Model error: {}", msg),
            StepExecutionError::ToolError(msg) => write!(f, "Tool error: {}", msg),
            StepExecutionError::ExecutionError(msg) => write!(f, "Execution error: {}", msg),
            StepExecutionError::Timeout => write!(f, "Step execution timed out"),
        }
    }
}

// ---------------------------------------------------------------------------
// Step Executor Trait
// ---------------------------------------------------------------------------

/// Trait for executing a step locally on this node.
///
/// The worker delegates actual execution (model inference + tool calls) to an
/// implementor of this trait. This makes the worker testable with mock executors.
pub trait StepExecutor: Send + Sync {
    /// Execute a step with the given input data from completed dependencies.
    ///
    /// Returns the serialized output data on success, or a `StepExecutionError` on failure.
    fn execute(
        &self,
        step: &ExecutionStep,
        input_data: &HashMap<StepId, Vec<u8>>,
    ) -> Result<Vec<u8>, StepExecutionError>;
}

// ---------------------------------------------------------------------------
// Step Worker
// ---------------------------------------------------------------------------

/// The step worker: handles incoming step execution requests on a worker node.
///
/// Maintains awareness of locally loaded models and available tools, verifies
/// resource availability before execution, and reports results (or failures)
/// back to the orchestrator via protocol messages.
///
/// Satisfies FR-8.4: Worker receives ExecuteStep, sends StepStarted/StepCompleted/StepFailed.
/// Satisfies FR-1.4: Dynamic tool availability checking.
/// Satisfies FR-8.5: Progress reporting during long-running steps.
pub struct StepWorker {
    /// This worker's node ID.
    node_id: NodeId,

    /// Currently loaded model IDs on this node.
    loaded_models: Vec<String>,

    /// Current tool inventory on this node.
    available_tools: Vec<ToolCapability>,
}

impl StepWorker {
    /// Create a new StepWorker with the given node identity and resource state.
    pub fn new(
        node_id: NodeId,
        loaded_models: Vec<String>,
        available_tools: Vec<ToolCapability>,
    ) -> Self {
        Self {
            node_id,
            loaded_models,
            available_tools,
        }
    }

    /// Handle an incoming ExecuteStep request.
    ///
    /// Verifies resource availability, executes the step via the provided executor,
    /// and returns the protocol messages to send back to the orchestrator:
    /// - On success: [StepStarted, StepCompleted]
    /// - On pre-execution failure (resource unavailable): [StepFailed] (retryable)
    /// - On execution failure: [StepStarted, StepFailed]
    ///
    /// Satisfies FR-8.4, FR-3.1, FR-7.1.
    pub fn handle_execute_step(
        &self,
        workflow_id: WorkflowId,
        step: &ExecutionStep,
        input_data: &HashMap<StepId, Vec<u8>>,
        executor: &dyn StepExecutor,
    ) -> Vec<AgentStepMessage> {
        let mut messages = Vec::new();

        // Verify required model is still loaded
        if let Err(error) = self.verify_model_available(step) {
            messages.push(AgentStepMessage::StepFailed {
                workflow_id,
                step_id: step.step_id,
                error,
                retryable: true,
            });
            return messages;
        }

        // Verify all required tools are still available
        if let Err(error) = self.verify_tools_available(&step.required_tools) {
            messages.push(AgentStepMessage::StepFailed {
                workflow_id,
                step_id: step.step_id,
                error,
                retryable: true,
            });
            return messages;
        }

        // Notify orchestrator we're starting
        messages.push(AgentStepMessage::StepStarted {
            workflow_id,
            step_id: step.step_id,
            node_id: self.node_id,
        });

        // Execute the step locally
        match executor.execute(step, input_data) {
            Ok(output_data) => {
                let output_size_bytes = output_data.len() as u64;
                let result = StepResult {
                    step_id: step.step_id,
                    output_data,
                    output_size_bytes,
                    execution_node: self.node_id,
                    compute_time_ms: 0, // Real timing would be measured by the executor
                    model_used: step.required_model.clone(),
                    tools_used: step.required_tools.clone(),
                };
                messages.push(AgentStepMessage::StepCompleted {
                    workflow_id,
                    step_id: step.step_id,
                    result,
                });
            }
            Err(e) => {
                messages.push(AgentStepMessage::StepFailed {
                    workflow_id,
                    step_id: step.step_id,
                    error: e.to_string(),
                    retryable: e.is_retryable(),
                });
            }
        }

        messages
    }

    // ─── Tool Availability Checking (Task 7.2) ──────────────────────────────

    /// Verify that the required model (if any) is still loaded on this node.
    ///
    /// Returns `Ok(())` if no model is required or the model is loaded.
    /// Returns `Err(reason)` if the model is no longer available.
    fn verify_model_available(&self, step: &ExecutionStep) -> Result<(), String> {
        if let Some(ref model_id) = step.required_model {
            if !self.loaded_models.iter().any(|m| m == model_id) {
                return Err(format!("Model '{}' no longer loaded", model_id));
            }
        }
        Ok(())
    }

    /// Verify that all required tools are available on this node.
    ///
    /// Queries the local tool registry for each required tool ID.
    /// If any tool became unavailable since routing, returns a retryable failure.
    ///
    /// Satisfies FR-1.4: Dynamic tool availability checking.
    /// Satisfies FR-7.1: Returns retryable failure when tool unavailable.
    pub fn verify_tools_available(&self, tool_ids: &[String]) -> Result<(), String> {
        for tool_id in tool_ids {
            let tool = self
                .available_tools
                .iter()
                .find(|t| t.tool_id == *tool_id);

            match tool {
                None => {
                    return Err(format!("Tool '{}' not found in local registry", tool_id));
                }
                Some(t) if !t.is_available => {
                    return Err(format!(
                        "Tool '{}' is registered but currently unavailable",
                        tool_id
                    ));
                }
                _ => {} // Tool found and available
            }
        }
        Ok(())
    }

    /// Update the list of loaded models on this node.
    ///
    /// Called when models are loaded or unloaded to keep the worker's state current.
    pub fn update_loaded_models(&mut self, models: Vec<String>) {
        self.loaded_models = models;
    }

    /// Update the availability status of a single tool.
    ///
    /// Reports dynamic tool availability changes. If a tool crashes or becomes
    /// unavailable, this should be called to update the worker's local state.
    /// The caller is responsible for propagating this change to the network registry.
    ///
    /// Satisfies FR-1.4: Tool availability is dynamic.
    pub fn update_tool_availability(&mut self, tool_id: &str, available: bool) {
        if let Some(tool) = self.available_tools.iter_mut().find(|t| t.tool_id == tool_id) {
            tool.is_available = available;
        }
    }

    // ─── Progress Reporting (Task 7.3) ──────────────────────────────────────

    /// Create a StepProgress message for reporting intermediate progress.
    ///
    /// Used during long-running steps to inform the orchestrator of progress.
    /// The progress_percent should be in [0.0, 100.0].
    ///
    /// Satisfies FR-8.5: Progress reporting during long-running steps.
    pub fn report_progress(
        &self,
        workflow_id: WorkflowId,
        step_id: StepId,
        progress_percent: f32,
        message: String,
    ) -> AgentStepMessage {
        AgentStepMessage::StepProgress {
            workflow_id,
            step_id,
            progress_percent: progress_percent.clamp(0.0, 100.0),
            message,
        }
    }

    /// Get this worker's node ID.
    pub fn node_id(&self) -> NodeId {
        self.node_id
    }

    /// Get the list of currently loaded models.
    pub fn loaded_models(&self) -> &[String] {
        &self.loaded_models
    }

    /// Get the list of available tools.
    pub fn available_tools(&self) -> &[ToolCapability] {
        &self.available_tools
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::dag::{PromptSensitivity, StepStatus};
    use crate::agents::tools::{ToolCategory, ToolResources};

    /// A mock executor that always succeeds with a fixed output.
    struct MockSuccessExecutor {
        output: Vec<u8>,
    }

    impl StepExecutor for MockSuccessExecutor {
        fn execute(
            &self,
            _step: &ExecutionStep,
            _input_data: &HashMap<StepId, Vec<u8>>,
        ) -> Result<Vec<u8>, StepExecutionError> {
            Ok(self.output.clone())
        }
    }

    /// A mock executor that always fails with a given error.
    struct MockFailExecutor {
        error: StepExecutionError,
    }

    impl StepExecutor for MockFailExecutor {
        fn execute(
            &self,
            _step: &ExecutionStep,
            _input_data: &HashMap<StepId, Vec<u8>>,
        ) -> Result<Vec<u8>, StepExecutionError> {
            Err(self.error.clone())
        }
    }

    fn make_tool(tool_id: &str, available: bool) -> ToolCapability {
        ToolCapability {
            tool_id: tool_id.to_string(),
            tool_name: format!("Tool {}", tool_id),
            category: ToolCategory::Filesystem,
            resource_requirements: ToolResources::default(),
            is_available: available,
            version: "1.0.0".to_string(),
        }
    }

    fn make_step(
        step_id: StepId,
        model: Option<&str>,
        tools: Vec<&str>,
    ) -> ExecutionStep {
        ExecutionStep {
            step_id,
            description: "Test step".to_string(),
            required_model: model.map(|s| s.to_string()),
            required_tools: tools.into_iter().map(|s| s.to_string()).collect(),
            sensitivity: PromptSensitivity::NonSensitive,
            estimated_compute_ms: 1000,
            input_dependencies: Vec::new(),
            status: StepStatus::Dispatched,
            assigned_node: None,
            result: None,
        }
    }

    fn make_worker(models: Vec<&str>, tools: Vec<ToolCapability>) -> StepWorker {
        StepWorker::new(
            uuid::Uuid::new_v4(),
            models.into_iter().map(|s| s.to_string()).collect(),
            tools,
        )
    }

    // ─── Task 7.1: Worker handler tests ─────────────────────────────────────

    #[test]
    fn test_successful_step_execution_returns_started_and_completed() {
        let worker = make_worker(
            vec!["qwen2.5:7b"],
            vec![make_tool("browser", true)],
        );
        let step_id = uuid::Uuid::new_v4();
        let workflow_id = uuid::Uuid::new_v4();
        let step = make_step(step_id, Some("qwen2.5:7b"), vec!["browser"]);
        let executor = MockSuccessExecutor {
            output: vec![42, 43, 44],
        };

        let messages = worker.handle_execute_step(
            workflow_id,
            &step,
            &HashMap::new(),
            &executor,
        );

        assert_eq!(messages.len(), 2);

        // First message: StepStarted
        match &messages[0] {
            AgentStepMessage::StepStarted {
                workflow_id: wid,
                step_id: sid,
                node_id,
            } => {
                assert_eq!(*wid, workflow_id);
                assert_eq!(*sid, step_id);
                assert_eq!(*node_id, worker.node_id());
            }
            _ => panic!("Expected StepStarted, got {:?}", messages[0]),
        }

        // Second message: StepCompleted
        match &messages[1] {
            AgentStepMessage::StepCompleted {
                workflow_id: wid,
                step_id: sid,
                result,
            } => {
                assert_eq!(*wid, workflow_id);
                assert_eq!(*sid, step_id);
                assert_eq!(result.output_data, vec![42, 43, 44]);
                assert_eq!(result.output_size_bytes, 3);
                assert_eq!(result.execution_node, worker.node_id());
            }
            _ => panic!("Expected StepCompleted, got {:?}", messages[1]),
        }
    }

    #[test]
    fn test_missing_model_returns_retryable_failure() {
        let worker = make_worker(
            vec!["qwen2.5:7b"], // Only has 7b loaded
            vec![make_tool("browser", true)],
        );
        let step_id = uuid::Uuid::new_v4();
        let workflow_id = uuid::Uuid::new_v4();
        let step = make_step(step_id, Some("llama3:70b"), vec!["browser"]); // Needs 70b
        let executor = MockSuccessExecutor { output: vec![] };

        let messages = worker.handle_execute_step(
            workflow_id,
            &step,
            &HashMap::new(),
            &executor,
        );

        assert_eq!(messages.len(), 1);
        match &messages[0] {
            AgentStepMessage::StepFailed {
                workflow_id: wid,
                step_id: sid,
                error,
                retryable,
            } => {
                assert_eq!(*wid, workflow_id);
                assert_eq!(*sid, step_id);
                assert!(error.contains("llama3:70b"));
                assert!(error.contains("no longer loaded"));
                assert!(*retryable);
            }
            _ => panic!("Expected StepFailed, got {:?}", messages[0]),
        }
    }

    #[test]
    fn test_missing_tool_returns_retryable_failure() {
        let worker = make_worker(
            vec!["qwen2.5:7b"],
            vec![make_tool("filesystem", true)], // Only has filesystem
        );
        let step_id = uuid::Uuid::new_v4();
        let workflow_id = uuid::Uuid::new_v4();
        let step = make_step(step_id, Some("qwen2.5:7b"), vec!["browser"]); // Needs browser
        let executor = MockSuccessExecutor { output: vec![] };

        let messages = worker.handle_execute_step(
            workflow_id,
            &step,
            &HashMap::new(),
            &executor,
        );

        assert_eq!(messages.len(), 1);
        match &messages[0] {
            AgentStepMessage::StepFailed {
                error, retryable, ..
            } => {
                assert!(error.contains("browser"));
                assert!(error.contains("not found"));
                assert!(*retryable);
            }
            _ => panic!("Expected StepFailed, got {:?}", messages[0]),
        }
    }

    #[test]
    fn test_unavailable_tool_returns_retryable_failure() {
        let worker = make_worker(
            vec!["qwen2.5:7b"],
            vec![make_tool("browser", false)], // Browser registered but unavailable
        );
        let step_id = uuid::Uuid::new_v4();
        let workflow_id = uuid::Uuid::new_v4();
        let step = make_step(step_id, Some("qwen2.5:7b"), vec!["browser"]);
        let executor = MockSuccessExecutor { output: vec![] };

        let messages = worker.handle_execute_step(
            workflow_id,
            &step,
            &HashMap::new(),
            &executor,
        );

        assert_eq!(messages.len(), 1);
        match &messages[0] {
            AgentStepMessage::StepFailed {
                error, retryable, ..
            } => {
                assert!(error.contains("browser"));
                assert!(error.contains("unavailable"));
                assert!(*retryable);
            }
            _ => panic!("Expected StepFailed, got {:?}", messages[0]),
        }
    }

    #[test]
    fn test_execution_error_returns_non_retryable_failure() {
        let worker = make_worker(
            vec!["qwen2.5:7b"],
            vec![make_tool("browser", true)],
        );
        let step_id = uuid::Uuid::new_v4();
        let workflow_id = uuid::Uuid::new_v4();
        let step = make_step(step_id, Some("qwen2.5:7b"), vec!["browser"]);
        let executor = MockFailExecutor {
            error: StepExecutionError::ExecutionError("Invalid input format".to_string()),
        };

        let messages = worker.handle_execute_step(
            workflow_id,
            &step,
            &HashMap::new(),
            &executor,
        );

        // Should have StepStarted + StepFailed (resources were verified OK)
        assert_eq!(messages.len(), 2);
        match &messages[0] {
            AgentStepMessage::StepStarted { .. } => {}
            _ => panic!("Expected StepStarted, got {:?}", messages[0]),
        }
        match &messages[1] {
            AgentStepMessage::StepFailed {
                error, retryable, ..
            } => {
                assert!(error.contains("Invalid input format"));
                assert!(!retryable); // ExecutionError is non-retryable
            }
            _ => panic!("Expected StepFailed, got {:?}", messages[1]),
        }
    }

    #[test]
    fn test_timeout_error_is_retryable() {
        let worker = make_worker(
            vec!["qwen2.5:7b"],
            vec![make_tool("browser", true)],
        );
        let step_id = uuid::Uuid::new_v4();
        let workflow_id = uuid::Uuid::new_v4();
        let step = make_step(step_id, Some("qwen2.5:7b"), vec!["browser"]);
        let executor = MockFailExecutor {
            error: StepExecutionError::Timeout,
        };

        let messages = worker.handle_execute_step(
            workflow_id,
            &step,
            &HashMap::new(),
            &executor,
        );

        assert_eq!(messages.len(), 2);
        match &messages[1] {
            AgentStepMessage::StepFailed {
                error, retryable, ..
            } => {
                assert!(error.contains("timed out"));
                assert!(*retryable);
            }
            _ => panic!("Expected StepFailed, got {:?}", messages[1]),
        }
    }

    #[test]
    fn test_step_with_no_model_requirement_skips_model_check() {
        let worker = make_worker(
            vec![], // No models loaded
            vec![make_tool("filesystem", true)],
        );
        let step_id = uuid::Uuid::new_v4();
        let workflow_id = uuid::Uuid::new_v4();
        let step = make_step(step_id, None, vec!["filesystem"]); // No model needed
        let executor = MockSuccessExecutor {
            output: vec![1, 2, 3],
        };

        let messages = worker.handle_execute_step(
            workflow_id,
            &step,
            &HashMap::new(),
            &executor,
        );

        assert_eq!(messages.len(), 2);
        match &messages[0] {
            AgentStepMessage::StepStarted { .. } => {}
            _ => panic!("Expected StepStarted"),
        }
        match &messages[1] {
            AgentStepMessage::StepCompleted { result, .. } => {
                assert_eq!(result.output_data, vec![1, 2, 3]);
            }
            _ => panic!("Expected StepCompleted"),
        }
    }

    // ─── Task 7.2: Tool availability checking tests ─────────────────────────

    #[test]
    fn test_verify_tools_all_available() {
        let worker = make_worker(
            vec![],
            vec![
                make_tool("browser", true),
                make_tool("filesystem", true),
            ],
        );

        let result = worker.verify_tools_available(&[
            "browser".to_string(),
            "filesystem".to_string(),
        ]);
        assert!(result.is_ok());
    }

    #[test]
    fn test_verify_tools_one_missing() {
        let worker = make_worker(
            vec![],
            vec![make_tool("browser", true)],
        );

        let result = worker.verify_tools_available(&[
            "browser".to_string(),
            "code_exec".to_string(),
        ]);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("code_exec"));
    }

    #[test]
    fn test_verify_tools_one_unavailable() {
        let worker = make_worker(
            vec![],
            vec![
                make_tool("browser", true),
                make_tool("code_exec", false),
            ],
        );

        let result = worker.verify_tools_available(&[
            "browser".to_string(),
            "code_exec".to_string(),
        ]);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("unavailable"));
    }

    #[test]
    fn test_update_tool_availability() {
        let mut worker = make_worker(
            vec![],
            vec![make_tool("browser", true)],
        );

        // Browser becomes unavailable
        worker.update_tool_availability("browser", false);
        let result = worker.verify_tools_available(&["browser".to_string()]);
        assert!(result.is_err());

        // Browser comes back
        worker.update_tool_availability("browser", true);
        let result = worker.verify_tools_available(&["browser".to_string()]);
        assert!(result.is_ok());
    }

    #[test]
    fn test_update_loaded_models() {
        let mut worker = make_worker(
            vec!["qwen2.5:7b"],
            vec![make_tool("browser", true)],
        );
        let step_id = uuid::Uuid::new_v4();
        let workflow_id = uuid::Uuid::new_v4();
        let step = make_step(step_id, Some("llama3:8b"), vec!["browser"]);
        let executor = MockSuccessExecutor { output: vec![] };

        // Initially fails — model not loaded
        let messages = worker.handle_execute_step(
            workflow_id,
            &step,
            &HashMap::new(),
            &executor,
        );
        assert_eq!(messages.len(), 1);
        match &messages[0] {
            AgentStepMessage::StepFailed { .. } => {}
            _ => panic!("Expected StepFailed"),
        }

        // Load the model
        worker.update_loaded_models(vec!["qwen2.5:7b".to_string(), "llama3:8b".to_string()]);

        // Now succeeds
        let messages = worker.handle_execute_step(
            workflow_id,
            &step,
            &HashMap::new(),
            &executor,
        );
        assert_eq!(messages.len(), 2);
        match &messages[0] {
            AgentStepMessage::StepStarted { .. } => {}
            _ => panic!("Expected StepStarted"),
        }
    }

    // ─── Task 7.3: Progress reporting tests ─────────────────────────────────

    #[test]
    fn test_progress_reporting_creates_correct_message() {
        let worker = make_worker(vec![], vec![]);
        let workflow_id = uuid::Uuid::new_v4();
        let step_id = uuid::Uuid::new_v4();

        let msg = worker.report_progress(
            workflow_id,
            step_id,
            45.5,
            "Processing document 3 of 7".to_string(),
        );

        match msg {
            AgentStepMessage::StepProgress {
                workflow_id: wid,
                step_id: sid,
                progress_percent,
                message,
            } => {
                assert_eq!(wid, workflow_id);
                assert_eq!(sid, step_id);
                assert!((progress_percent - 45.5).abs() < f32::EPSILON);
                assert_eq!(message, "Processing document 3 of 7");
            }
            _ => panic!("Expected StepProgress, got {:?}", msg),
        }
    }

    #[test]
    fn test_progress_percent_clamped_to_valid_range() {
        let worker = make_worker(vec![], vec![]);
        let workflow_id = uuid::Uuid::new_v4();
        let step_id = uuid::Uuid::new_v4();

        // Over 100
        let msg = worker.report_progress(workflow_id, step_id, 150.0, "done".to_string());
        match msg {
            AgentStepMessage::StepProgress {
                progress_percent, ..
            } => {
                assert!((progress_percent - 100.0).abs() < f32::EPSILON);
            }
            _ => panic!("Expected StepProgress"),
        }

        // Below 0
        let msg = worker.report_progress(workflow_id, step_id, -10.0, "start".to_string());
        match msg {
            AgentStepMessage::StepProgress {
                progress_percent, ..
            } => {
                assert!((progress_percent - 0.0).abs() < f32::EPSILON);
            }
            _ => panic!("Expected StepProgress"),
        }
    }

    // ─── Error type tests ───────────────────────────────────────────────────

    #[test]
    fn test_step_execution_error_retryable() {
        assert!(StepExecutionError::ModelError("oom".to_string()).is_retryable());
        assert!(StepExecutionError::ToolError("crashed".to_string()).is_retryable());
        assert!(!StepExecutionError::ExecutionError("bad input".to_string()).is_retryable());
        assert!(StepExecutionError::Timeout.is_retryable());
    }

    #[test]
    fn test_step_execution_error_display() {
        let err = StepExecutionError::ModelError("out of memory".to_string());
        assert_eq!(format!("{}", err), "Model error: out of memory");

        let err = StepExecutionError::Timeout;
        assert_eq!(format!("{}", err), "Step execution timed out");
    }
}
