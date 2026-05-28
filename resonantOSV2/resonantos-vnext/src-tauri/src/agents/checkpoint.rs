// Distributed Agent Execution — Checkpoint
// Phase 15: Persist workflow progress for resume after crash
//
// Serializes WorkflowCheckpoint (completed results + pending steps) to disk.
// Triggers checkpoint after `checkpoint_interval_secs` of elapsed execution.
// On app restart, detects incomplete workflows and offers resume.
//
// Satisfies FR-7.5: Long-running workflows checkpoint their progress —
//                   can resume after app restart.
// Satisfies NFR-3.3: Orchestrator failure during long workflow is recoverable
//                    (checkpoint/resume).

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::agents::dag::{ExecutionDag, StepId, StepResult, StepStatus, WorkflowId};
use crate::agents::protocol::WorkflowCheckpoint;

// ---------------------------------------------------------------------------
// Checkpoint errors
// ---------------------------------------------------------------------------

/// Errors that can occur during checkpoint operations.
#[derive(Debug, Clone, PartialEq)]
pub enum CheckpointError {
    /// An I/O error occurred (reading/writing checkpoint files).
    IoError(String),
    /// Serialization or deserialization failed.
    SerializationError(String),
    /// No checkpoint found for the given workflow.
    NotFound(WorkflowId),
}

impl std::fmt::Display for CheckpointError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CheckpointError::IoError(msg) => write!(f, "Checkpoint I/O error: {}", msg),
            CheckpointError::SerializationError(msg) => {
                write!(f, "Checkpoint serialization error: {}", msg)
            }
            CheckpointError::NotFound(id) => {
                write!(f, "No checkpoint found for workflow {}", id)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Checkpoint manager
// ---------------------------------------------------------------------------

/// Manages workflow checkpoint persistence for crash recovery.
///
/// Checkpoints are saved as JSON files in a designated directory. Each workflow
/// gets a single checkpoint file named `{workflow_id}.json`. The manager tracks
/// when each workflow was last checkpointed and only writes a new checkpoint
/// after `checkpoint_interval` has elapsed.
pub struct CheckpointManager {
    /// Directory where checkpoint files are stored.
    checkpoint_dir: PathBuf,

    /// Minimum interval between checkpoints for a given workflow.
    checkpoint_interval: Duration,

    /// Tracks when each workflow was last checkpointed.
    last_checkpoint: HashMap<WorkflowId, Instant>,
}

impl CheckpointManager {
    /// Create a new checkpoint manager.
    ///
    /// # Arguments
    ///
    /// * `checkpoint_dir` — Directory where checkpoint JSON files will be stored.
    /// * `interval_secs` — Minimum seconds between checkpoints for a workflow
    ///   (corresponds to `DistributedAgentConfig::checkpoint_interval_secs`).
    pub fn new(checkpoint_dir: PathBuf, interval_secs: u64) -> Self {
        Self {
            checkpoint_dir,
            checkpoint_interval: Duration::from_secs(interval_secs),
            last_checkpoint: HashMap::new(),
        }
    }

    /// Returns `true` if enough time has elapsed since the last checkpoint for
    /// this workflow, meaning a new checkpoint should be saved.
    ///
    /// If the workflow has never been checkpointed, returns `true`.
    pub fn should_checkpoint(&self, workflow_id: WorkflowId) -> bool {
        match self.last_checkpoint.get(&workflow_id) {
            Some(last) => last.elapsed() >= self.checkpoint_interval,
            None => true,
        }
    }

    /// Save a checkpoint for the given workflow by extracting state from the DAG.
    ///
    /// Creates the checkpoint directory if it doesn't exist. Writes the checkpoint
    /// as a JSON file at `{checkpoint_dir}/{workflow_id}.json`.
    ///
    /// Updates the internal `last_checkpoint` timestamp on success.
    pub fn save_checkpoint(
        &mut self,
        workflow_id: WorkflowId,
        dag: &ExecutionDag,
    ) -> Result<(), CheckpointError> {
        let checkpoint = self.build_checkpoint(dag);

        // Ensure checkpoint directory exists.
        std::fs::create_dir_all(&self.checkpoint_dir)
            .map_err(|e| CheckpointError::IoError(e.to_string()))?;

        let file_path = self.checkpoint_file_path(workflow_id);

        let json = serde_json::to_string_pretty(&checkpoint)
            .map_err(|e| CheckpointError::SerializationError(e.to_string()))?;

        std::fs::write(&file_path, json)
            .map_err(|e| CheckpointError::IoError(e.to_string()))?;

        // Record the checkpoint time.
        self.last_checkpoint.insert(workflow_id, Instant::now());

        Ok(())
    }

    /// Load a checkpoint from disk for the given workflow.
    ///
    /// Returns `CheckpointError::NotFound` if no checkpoint file exists.
    pub fn load_checkpoint(
        &self,
        workflow_id: WorkflowId,
    ) -> Result<WorkflowCheckpoint, CheckpointError> {
        let file_path = self.checkpoint_file_path(workflow_id);

        if !file_path.exists() {
            return Err(CheckpointError::NotFound(workflow_id));
        }

        let json = std::fs::read_to_string(&file_path)
            .map_err(|e| CheckpointError::IoError(e.to_string()))?;

        let checkpoint: WorkflowCheckpoint = serde_json::from_str(&json)
            .map_err(|e| CheckpointError::SerializationError(e.to_string()))?;

        Ok(checkpoint)
    }

    /// List all workflow IDs that have incomplete (checkpointed) workflows on disk.
    ///
    /// Scans the checkpoint directory for `.json` files and extracts the workflow ID
    /// from each filename. Used on app restart to detect workflows that can be resumed.
    pub fn list_incomplete_workflows(&self) -> Result<Vec<WorkflowId>, CheckpointError> {
        if !self.checkpoint_dir.exists() {
            return Ok(Vec::new());
        }

        let entries = std::fs::read_dir(&self.checkpoint_dir)
            .map_err(|e| CheckpointError::IoError(e.to_string()))?;

        let mut workflow_ids = Vec::new();

        for entry in entries {
            let entry = entry.map_err(|e| CheckpointError::IoError(e.to_string()))?;
            let path = entry.path();

            if path.extension().and_then(|ext| ext.to_str()) == Some("json") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    if let Ok(id) = uuid::Uuid::parse_str(stem) {
                        workflow_ids.push(id);
                    }
                }
            }
        }

        Ok(workflow_ids)
    }

    /// Delete the checkpoint file for a workflow (e.g., after workflow completes).
    ///
    /// Also removes the workflow from the internal `last_checkpoint` tracking.
    /// Returns `Ok(())` even if the file doesn't exist (idempotent).
    pub fn delete_checkpoint(&mut self, workflow_id: WorkflowId) -> Result<(), CheckpointError> {
        let file_path = self.checkpoint_file_path(workflow_id);

        if file_path.exists() {
            std::fs::remove_file(&file_path)
                .map_err(|e| CheckpointError::IoError(e.to_string()))?;
        }

        self.last_checkpoint.remove(&workflow_id);

        Ok(())
    }

    /// Build a `WorkflowCheckpoint` from the current state of an execution DAG.
    ///
    /// Extracts:
    /// - `completed_step_results`: results from all steps with `StepStatus::Completed`
    /// - `pending_steps`: IDs of all steps that are not yet completed
    ///   (Pending, Ready, Dispatched, Running)
    pub fn build_checkpoint(&self, dag: &ExecutionDag) -> WorkflowCheckpoint {
        let mut completed_step_results: HashMap<StepId, StepResult> = HashMap::new();
        let mut pending_steps: Vec<StepId> = Vec::new();

        for (step_id, step) in &dag.steps {
            match &step.status {
                StepStatus::Completed => {
                    if let Some(result) = &step.result {
                        completed_step_results.insert(*step_id, result.clone());
                    }
                }
                StepStatus::Pending
                | StepStatus::Ready
                | StepStatus::Dispatched
                | StepStatus::Running => {
                    pending_steps.push(*step_id);
                }
                // Failed and Cancelled steps are not included in pending —
                // they will need to be re-evaluated on resume.
                StepStatus::Failed { .. } | StepStatus::Cancelled => {
                    pending_steps.push(*step_id);
                }
            }
        }

        WorkflowCheckpoint {
            checkpointed_at: chrono::Utc::now(),
            completed_step_results,
            pending_steps,
        }
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    /// Compute the file path for a workflow's checkpoint file.
    fn checkpoint_file_path(&self, workflow_id: WorkflowId) -> PathBuf {
        self.checkpoint_dir.join(format!("{}.json", workflow_id))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::dag::{ExecutionDag, ExecutionStep, PromptSensitivity, StepStatus};
    use std::collections::HashMap;

    /// Helper: create a minimal ExecutionStep with the given status and optional result.
    fn make_step(step_id: StepId, status: StepStatus, result: Option<StepResult>) -> ExecutionStep {
        ExecutionStep {
            step_id,
            description: format!("Step {}", step_id),
            required_model: None,
            required_tools: Vec::new(),
            sensitivity: PromptSensitivity::NonSensitive,
            estimated_compute_ms: 1000,
            input_dependencies: Vec::new(),
            status,
            assigned_node: None,
            result,
        }
    }

    /// Helper: create a StepResult for a given step.
    fn make_result(step_id: StepId) -> StepResult {
        StepResult {
            step_id,
            output_data: vec![1, 2, 3, 4],
            output_size_bytes: 4,
            execution_node: uuid::Uuid::new_v4(),
            compute_time_ms: 200,
            model_used: None,
            tools_used: vec!["browser".to_string()],
        }
    }

    /// Helper: create a test DAG with mixed step statuses.
    ///
    /// Structure: A (Completed) -> B (Running) -> C (Pending)
    ///            A -> D (Completed)
    fn make_test_dag() -> ExecutionDag {
        let wf = uuid::Uuid::new_v4();
        let a = uuid::Uuid::new_v4();
        let b = uuid::Uuid::new_v4();
        let c = uuid::Uuid::new_v4();
        let d = uuid::Uuid::new_v4();

        let result_a = make_result(a);
        let result_d = make_result(d);

        let mut steps = HashMap::new();
        steps.insert(
            a,
            make_step(a, StepStatus::Completed, Some(result_a)),
        );
        steps.insert(b, make_step(b, StepStatus::Running, None));
        steps.insert(c, make_step(c, StepStatus::Pending, None));
        steps.insert(
            d,
            make_step(d, StepStatus::Completed, Some(result_d)),
        );

        ExecutionDag {
            workflow_id: wf,
            steps,
            edges: vec![(a, b), (b, c), (a, d)],
            root_steps: vec![a],
        }
    }

    // -----------------------------------------------------------------------
    // build_checkpoint tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_build_checkpoint_extracts_completed_results() {
        let dag = make_test_dag();
        let manager = CheckpointManager::new(PathBuf::from("/tmp/test"), 300);

        let checkpoint = manager.build_checkpoint(&dag);

        // A and D are completed — their results should be in completed_step_results.
        assert_eq!(checkpoint.completed_step_results.len(), 2);

        // Find the step IDs for A and D (completed steps).
        let completed_ids: Vec<StepId> = dag
            .steps
            .iter()
            .filter(|(_, s)| s.status == StepStatus::Completed)
            .map(|(id, _)| *id)
            .collect();

        for id in &completed_ids {
            assert!(
                checkpoint.completed_step_results.contains_key(id),
                "Completed step {} should be in checkpoint results",
                id
            );
        }
    }

    #[test]
    fn test_build_checkpoint_extracts_pending_steps() {
        let dag = make_test_dag();
        let manager = CheckpointManager::new(PathBuf::from("/tmp/test"), 300);

        let checkpoint = manager.build_checkpoint(&dag);

        // B (Running) and C (Pending) should be in pending_steps.
        assert_eq!(checkpoint.pending_steps.len(), 2);

        let non_completed_ids: Vec<StepId> = dag
            .steps
            .iter()
            .filter(|(_, s)| s.status != StepStatus::Completed)
            .map(|(id, _)| *id)
            .collect();

        for id in &non_completed_ids {
            assert!(
                checkpoint.pending_steps.contains(id),
                "Non-completed step {} should be in pending_steps",
                id
            );
        }
    }

    #[test]
    fn test_build_checkpoint_all_completed() {
        let wf = uuid::Uuid::new_v4();
        let a = uuid::Uuid::new_v4();
        let b = uuid::Uuid::new_v4();

        let mut steps = HashMap::new();
        steps.insert(
            a,
            make_step(a, StepStatus::Completed, Some(make_result(a))),
        );
        steps.insert(
            b,
            make_step(b, StepStatus::Completed, Some(make_result(b))),
        );

        let dag = ExecutionDag {
            workflow_id: wf,
            steps,
            edges: vec![(a, b)],
            root_steps: vec![a],
        };

        let manager = CheckpointManager::new(PathBuf::from("/tmp/test"), 300);
        let checkpoint = manager.build_checkpoint(&dag);

        assert_eq!(checkpoint.completed_step_results.len(), 2);
        assert!(checkpoint.pending_steps.is_empty());
    }

    #[test]
    fn test_build_checkpoint_all_pending() {
        let wf = uuid::Uuid::new_v4();
        let a = uuid::Uuid::new_v4();
        let b = uuid::Uuid::new_v4();

        let mut steps = HashMap::new();
        steps.insert(a, make_step(a, StepStatus::Pending, None));
        steps.insert(b, make_step(b, StepStatus::Ready, None));

        let dag = ExecutionDag {
            workflow_id: wf,
            steps,
            edges: vec![(a, b)],
            root_steps: vec![a],
        };

        let manager = CheckpointManager::new(PathBuf::from("/tmp/test"), 300);
        let checkpoint = manager.build_checkpoint(&dag);

        assert!(checkpoint.completed_step_results.is_empty());
        assert_eq!(checkpoint.pending_steps.len(), 2);
    }

    // -----------------------------------------------------------------------
    // should_checkpoint timing tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_should_checkpoint_first_time_returns_true() {
        let manager = CheckpointManager::new(PathBuf::from("/tmp/test"), 300);
        let wf = uuid::Uuid::new_v4();

        // Never checkpointed before — should return true.
        assert!(manager.should_checkpoint(wf));
    }

    #[test]
    fn test_should_checkpoint_after_save_returns_false() {
        let dir = tempfile::tempdir().unwrap();
        let mut manager = CheckpointManager::new(dir.path().to_path_buf(), 300);
        let dag = make_test_dag();
        let wf = dag.workflow_id;

        // Save a checkpoint — this records the timestamp.
        manager.save_checkpoint(wf, &dag).unwrap();

        // Immediately after, should_checkpoint returns false (interval not elapsed).
        assert!(!manager.should_checkpoint(wf));
    }

    #[test]
    fn test_should_checkpoint_with_zero_interval_always_true() {
        let dir = tempfile::tempdir().unwrap();
        let mut manager = CheckpointManager::new(dir.path().to_path_buf(), 0);
        let dag = make_test_dag();
        let wf = dag.workflow_id;

        manager.save_checkpoint(wf, &dag).unwrap();

        // With 0-second interval, should always be ready to checkpoint.
        assert!(manager.should_checkpoint(wf));
    }

    // -----------------------------------------------------------------------
    // Serialization/deserialization roundtrip
    // -----------------------------------------------------------------------

    #[test]
    fn test_save_and_load_checkpoint_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let mut manager = CheckpointManager::new(dir.path().to_path_buf(), 300);
        let dag = make_test_dag();
        let wf = dag.workflow_id;

        // Save checkpoint.
        manager.save_checkpoint(wf, &dag).unwrap();

        // Load it back.
        let loaded = manager.load_checkpoint(wf).unwrap();

        // Verify completed results match.
        let original = manager.build_checkpoint(&dag);
        assert_eq!(
            loaded.completed_step_results.len(),
            original.completed_step_results.len()
        );
        assert_eq!(loaded.pending_steps.len(), original.pending_steps.len());

        // Verify each completed result is present.
        for (step_id, result) in &original.completed_step_results {
            let loaded_result = loaded.completed_step_results.get(step_id).unwrap();
            assert_eq!(loaded_result.step_id, result.step_id);
            assert_eq!(loaded_result.output_data, result.output_data);
            assert_eq!(loaded_result.output_size_bytes, result.output_size_bytes);
            assert_eq!(loaded_result.compute_time_ms, result.compute_time_ms);
        }

        // Verify pending steps match (order may differ).
        let mut original_pending = original.pending_steps.clone();
        let mut loaded_pending = loaded.pending_steps.clone();
        original_pending.sort();
        loaded_pending.sort();
        assert_eq!(original_pending, loaded_pending);
    }

    #[test]
    fn test_load_checkpoint_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let manager = CheckpointManager::new(dir.path().to_path_buf(), 300);
        let wf = uuid::Uuid::new_v4();

        let result = manager.load_checkpoint(wf);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), CheckpointError::NotFound(wf));
    }

    #[test]
    fn test_load_checkpoint_invalid_json() {
        let dir = tempfile::tempdir().unwrap();
        let manager = CheckpointManager::new(dir.path().to_path_buf(), 300);
        let wf = uuid::Uuid::new_v4();

        // Write invalid JSON to the checkpoint file.
        let file_path = dir.path().join(format!("{}.json", wf));
        std::fs::write(&file_path, "not valid json {{{").unwrap();

        let result = manager.load_checkpoint(wf);
        assert!(result.is_err());
        match result.unwrap_err() {
            CheckpointError::SerializationError(_) => {} // expected
            other => panic!("Expected SerializationError, got {:?}", other),
        }
    }

    // -----------------------------------------------------------------------
    // list_incomplete_workflows
    // -----------------------------------------------------------------------

    #[test]
    fn test_list_incomplete_workflows_empty_dir() {
        let dir = tempfile::tempdir().unwrap();
        let manager = CheckpointManager::new(dir.path().to_path_buf(), 300);

        let workflows = manager.list_incomplete_workflows().unwrap();
        assert!(workflows.is_empty());
    }

    #[test]
    fn test_list_incomplete_workflows_nonexistent_dir() {
        let manager = CheckpointManager::new(PathBuf::from("/nonexistent/path/xyz"), 300);

        let workflows = manager.list_incomplete_workflows().unwrap();
        assert!(workflows.is_empty());
    }

    #[test]
    fn test_list_incomplete_workflows_finds_checkpoints() {
        let dir = tempfile::tempdir().unwrap();
        let mut manager = CheckpointManager::new(dir.path().to_path_buf(), 300);

        let dag1 = make_test_dag();
        let dag2 = {
            let mut d = make_test_dag();
            d.workflow_id = uuid::Uuid::new_v4();
            d
        };

        manager.save_checkpoint(dag1.workflow_id, &dag1).unwrap();
        manager.save_checkpoint(dag2.workflow_id, &dag2).unwrap();

        let workflows = manager.list_incomplete_workflows().unwrap();
        assert_eq!(workflows.len(), 2);
        assert!(workflows.contains(&dag1.workflow_id));
        assert!(workflows.contains(&dag2.workflow_id));
    }

    #[test]
    fn test_list_incomplete_workflows_ignores_non_json_files() {
        let dir = tempfile::tempdir().unwrap();
        let manager = CheckpointManager::new(dir.path().to_path_buf(), 300);

        // Create a non-JSON file in the directory.
        std::fs::write(dir.path().join("readme.txt"), "not a checkpoint").unwrap();
        // Create a JSON file with a non-UUID name.
        std::fs::write(dir.path().join("not-a-uuid.json"), "{}").unwrap();

        let workflows = manager.list_incomplete_workflows().unwrap();
        assert!(workflows.is_empty());
    }

    // -----------------------------------------------------------------------
    // delete_checkpoint
    // -----------------------------------------------------------------------

    #[test]
    fn test_delete_checkpoint_removes_file() {
        let dir = tempfile::tempdir().unwrap();
        let mut manager = CheckpointManager::new(dir.path().to_path_buf(), 300);
        let dag = make_test_dag();
        let wf = dag.workflow_id;

        manager.save_checkpoint(wf, &dag).unwrap();
        assert!(manager.load_checkpoint(wf).is_ok());

        manager.delete_checkpoint(wf).unwrap();

        // File should be gone.
        assert_eq!(
            manager.load_checkpoint(wf).unwrap_err(),
            CheckpointError::NotFound(wf)
        );
    }

    #[test]
    fn test_delete_checkpoint_nonexistent_is_ok() {
        let dir = tempfile::tempdir().unwrap();
        let mut manager = CheckpointManager::new(dir.path().to_path_buf(), 300);
        let wf = uuid::Uuid::new_v4();

        // Should not error even if file doesn't exist.
        assert!(manager.delete_checkpoint(wf).is_ok());
    }

    #[test]
    fn test_delete_checkpoint_clears_last_checkpoint_tracking() {
        let dir = tempfile::tempdir().unwrap();
        let mut manager = CheckpointManager::new(dir.path().to_path_buf(), 300);
        let dag = make_test_dag();
        let wf = dag.workflow_id;

        manager.save_checkpoint(wf, &dag).unwrap();
        // After save, should_checkpoint returns false (recently checkpointed).
        assert!(!manager.should_checkpoint(wf));

        manager.delete_checkpoint(wf).unwrap();
        // After delete, should_checkpoint returns true (no record of last checkpoint).
        assert!(manager.should_checkpoint(wf));
    }
}
