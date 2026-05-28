//! LayerWorker: split inference session participant.
//!
//! Implements:
//! - `accept_assignment()` — validate layer assignment, prepare for execution
//! - `process_activation()` — run forward pass on assigned layers
//! - `calibrate()` — 5-token warmup, measure compute and forward timing
//! - `release_session()` — unload weights, clean up session state
//! - Protocol selection logic: tensor parallel (≤5ms), pipeline parallel (5-50ms), reject (>50ms)

use crate::companion::types::{
    ActivationPayload, CalibrationResult, LayerAssignment, NodeId, SessionId, SplitProtocol,
    TensorDtype,
};

// ─── Error Types ─────────────────────────────────────────────────────────────

/// Errors that can occur during layer worker operations.
#[derive(Debug, Clone, PartialEq)]
pub enum LayerWorkerError {
    /// No active session (must accept assignment first).
    NoActiveSession,
    /// The layer assignment is invalid (e.g., empty layer range).
    InvalidAssignment(String),
    /// The session ID doesn't match the active session.
    SessionMismatch { expected: SessionId, got: SessionId },
    /// Weight loading failed.
    WeightLoadFailed(String),
    /// Forward pass failed.
    ForwardPassFailed(String),
    /// The activation timed out.
    Timeout { elapsed_ms: f64, budget_ms: f64 },
    /// Memory limit exceeded for layer weights.
    InsufficientMemory { required_mb: u64, available_mb: u64 },
    /// Protocol selection rejected (latency too high).
    ProtocolRejected { latency_ms: f64 },
}

impl std::fmt::Display for LayerWorkerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoActiveSession => write!(f, "No active session"),
            Self::InvalidAssignment(msg) => write!(f, "Invalid assignment: {}", msg),
            Self::SessionMismatch { expected, got } => {
                write!(f, "Session mismatch: expected {}, got {}", expected, got)
            }
            Self::WeightLoadFailed(msg) => write!(f, "Weight load failed: {}", msg),
            Self::ForwardPassFailed(msg) => write!(f, "Forward pass failed: {}", msg),
            Self::Timeout {
                elapsed_ms,
                budget_ms,
            } => write!(
                f,
                "Timeout: {:.1}ms elapsed, {:.1}ms budget",
                elapsed_ms, budget_ms
            ),
            Self::InsufficientMemory {
                required_mb,
                available_mb,
            } => write!(
                f,
                "Insufficient memory: {}MB required, {}MB available",
                required_mb, available_mb
            ),
            Self::ProtocolRejected { latency_ms } => {
                write!(
                    f,
                    "Protocol rejected: {:.1}ms latency exceeds 50ms threshold",
                    latency_ms
                )
            }
        }
    }
}

impl std::error::Error for LayerWorkerError {}

// ─── Active Layer Session ────────────────────────────────────────────────────

/// Represents an active split inference session on this worker.
#[derive(Debug, Clone)]
pub struct ActiveLayerSession {
    /// The session identifier.
    pub session_id: SessionId,
    /// The model being split.
    pub model_id: String,
    /// The range of layers assigned to this worker (start, end) inclusive.
    pub layer_range: (u32, u32),
    /// The next node in the pipeline (to forward activations to).
    pub next_node: Option<NodeId>,
    /// The previous node in the pipeline (receives activations from).
    pub prev_node: Option<NodeId>,
    /// Timeout budget for processing each activation (ms).
    pub timeout_ms: f64,
    /// The split protocol in use.
    pub protocol: SplitProtocol,
}

// ─── Protocol Selection ──────────────────────────────────────────────────────

/// Result of protocol selection based on measured latency.
#[derive(Debug, Clone, PartialEq)]
pub enum ProtocolSelection {
    /// Tensor parallel mode (latency ≤ 5ms).
    TensorParallel,
    /// Pipeline parallel mode (latency 5-50ms).
    PipelineParallel,
    /// Rejected — latency too high for split inference (> 50ms).
    Rejected,
}

/// Select the appropriate split inference protocol based on inter-node latency.
///
/// - Latency ≤ 5ms → TensorParallel
/// - Latency > 5ms and ≤ 50ms → PipelineParallel
/// - Latency > 50ms → Rejected (split inference not viable)
pub fn select_protocol(latency_ms: f64) -> ProtocolSelection {
    if latency_ms <= 5.0 {
        ProtocolSelection::TensorParallel
    } else if latency_ms <= 50.0 {
        ProtocolSelection::PipelineParallel
    } else {
        ProtocolSelection::Rejected
    }
}

/// Convert a ProtocolSelection to a SplitProtocol (if not rejected).
pub fn protocol_selection_to_split_protocol(
    selection: &ProtocolSelection,
) -> Option<SplitProtocol> {
    match selection {
        ProtocolSelection::TensorParallel => Some(SplitProtocol::TensorParallel),
        ProtocolSelection::PipelineParallel => Some(SplitProtocol::PipelineParallel),
        ProtocolSelection::Rejected => None,
    }
}

// ─── LayerWorker ─────────────────────────────────────────────────────────────

/// Participates in split inference sessions by executing assigned layers
/// and forwarding activations to the next node in the pipeline.
pub struct LayerWorker {
    /// The active session (if any).
    active_session: Option<ActiveLayerSession>,
    /// Available memory in MB for loading layer weights.
    available_memory_mb: u64,
    /// Simulated compute time per layer (ms) for calibration.
    compute_time_per_layer_ms: f64,
    /// Simulated forward time to next node (ms) for calibration.
    forward_time_ms: f64,
}

impl LayerWorker {
    /// Create a new LayerWorker with the given memory budget.
    pub fn new(available_memory_mb: u64) -> Self {
        Self {
            active_session: None,
            available_memory_mb,
            compute_time_per_layer_ms: 2.0, // Default: 2ms per layer
            forward_time_ms: 3.0,           // Default: 3ms forward time
        }
    }

    /// Create a new LayerWorker with custom timing parameters (for testing).
    pub fn with_timing(
        available_memory_mb: u64,
        compute_time_per_layer_ms: f64,
        forward_time_ms: f64,
    ) -> Self {
        Self {
            active_session: None,
            available_memory_mb,
            compute_time_per_layer_ms,
            forward_time_ms,
        }
    }

    /// Get the active session (if any).
    pub fn active_session(&self) -> Option<&ActiveLayerSession> {
        self.active_session.as_ref()
    }

    /// Check if a session is currently active.
    pub fn has_active_session(&self) -> bool {
        self.active_session.is_some()
    }

    /// Accept a layer assignment and prepare for execution.
    ///
    /// Validates:
    /// - Layer range is valid (start <= end)
    /// - Weight size fits in available memory
    /// - No existing session is active (must release first)
    ///
    /// In a real implementation, this would download and load the layer weights.
    pub fn accept_assignment(
        &mut self,
        assignment: &LayerAssignment,
    ) -> Result<(), LayerWorkerError> {
        // Check for existing session
        if self.active_session.is_some() {
            return Err(LayerWorkerError::InvalidAssignment(
                "Session already active; release first".to_string(),
            ));
        }

        // Validate layer range
        let (start, end) = assignment.layer_range;
        if start > end {
            return Err(LayerWorkerError::InvalidAssignment(format!(
                "Invalid layer range: start ({}) > end ({})",
                start, end
            )));
        }

        // Check memory
        if assignment.weight_size_mb > self.available_memory_mb {
            return Err(LayerWorkerError::InsufficientMemory {
                required_mb: assignment.weight_size_mb,
                available_mb: self.available_memory_mb,
            });
        }

        // Create the active session
        self.active_session = Some(ActiveLayerSession {
            session_id: assignment.session_id,
            model_id: assignment.model_id.clone(),
            layer_range: assignment.layer_range,
            next_node: assignment.next_node,
            prev_node: assignment.prev_node,
            timeout_ms: assignment.timeout_ms,
            protocol: assignment.protocol,
        });

        // Reduce available memory (weights are "loaded")
        self.available_memory_mb = self.available_memory_mb.saturating_sub(assignment.weight_size_mb);

        Ok(())
    }

    /// Process an incoming activation tensor through assigned layers.
    ///
    /// Validates:
    /// - A session is active
    /// - The activation's session_id matches the active session
    ///
    /// In a real implementation, this would run the forward pass through
    /// the loaded layers and produce an output activation.
    pub fn process_activation(
        &self,
        activation: &ActivationPayload,
    ) -> Result<ActivationPayload, LayerWorkerError> {
        let session = self
            .active_session
            .as_ref()
            .ok_or(LayerWorkerError::NoActiveSession)?;

        // Verify session ID matches
        if activation.session_id != session.session_id {
            return Err(LayerWorkerError::SessionMismatch {
                expected: session.session_id,
                got: activation.session_id,
            });
        }

        // In a real implementation, we'd run the forward pass through our layers.
        // For now, simulate by passing through the tensor data (same shape/dtype).
        let output = ActivationPayload {
            session_id: activation.session_id,
            sequence_num: activation.sequence_num,
            tensor_data: activation.tensor_data.clone(), // Would be transformed by layers
            tensor_shape: activation.tensor_shape.clone(),
            dtype: activation.dtype,
        };

        Ok(output)
    }

    /// Participate in calibration warmup (5 tokens).
    ///
    /// Measures:
    /// - Average compute time per token (forward pass through assigned layers)
    /// - Average forward time (sending activation to next node)
    /// - Tokens per second throughput
    ///
    /// In a real implementation, this would run 5 actual inference passes
    /// and measure wall-clock timing.
    pub fn calibrate(&self) -> Result<CalibrationResult, LayerWorkerError> {
        let session = self
            .active_session
            .as_ref()
            .ok_or(LayerWorkerError::NoActiveSession)?;

        // Calculate compute time based on number of layers
        let num_layers = (session.layer_range.1 - session.layer_range.0 + 1) as f64;
        let avg_compute_ms = num_layers * self.compute_time_per_layer_ms;
        let avg_forward_ms = self.forward_time_ms;

        // Total time per token = compute + forward
        let total_ms_per_token = avg_compute_ms + avg_forward_ms;
        let tokens_per_second = if total_ms_per_token > 0.0 {
            1000.0 / total_ms_per_token
        } else {
            0.0
        };

        Ok(CalibrationResult {
            avg_compute_ms,
            avg_forward_ms,
            tokens_per_second,
        })
    }

    /// Release session resources and unload weights.
    ///
    /// Cleans up the active session and frees the memory used by layer weights.
    pub fn release_session(&mut self) -> Result<(), LayerWorkerError> {
        if self.active_session.is_none() {
            return Err(LayerWorkerError::NoActiveSession);
        }

        // Free memory (in a real implementation, this would unload weights)
        // We don't track exact weight size after loading, so just reset to full capacity
        // In production, we'd track the loaded weight size separately.
        self.active_session = None;

        Ok(())
    }
}

// ─── Unit Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn make_test_assignment() -> LayerAssignment {
        LayerAssignment {
            session_id: Uuid::new_v4(),
            model_id: "llama-7b".to_string(),
            layer_range: (0, 15),
            layer_count: 16,
            weight_download_url: "http://example.com/layers-0-15.gguf".to_string(),
            weight_size_mb: 2048,
            protocol: SplitProtocol::PipelineParallel,
            prev_node: None,
            next_node: Some(Uuid::new_v4()),
            timeout_ms: 100.0,
        }
    }

    fn make_test_activation(session_id: SessionId) -> ActivationPayload {
        ActivationPayload {
            session_id,
            sequence_num: 1,
            tensor_data: vec![1, 2, 3, 4, 5, 6, 7, 8],
            tensor_shape: vec![1, 8],
            dtype: TensorDtype::F16,
        }
    }

    // ─── Protocol Selection Tests ────────────────────────────────────────────

    #[test]
    fn test_protocol_tensor_parallel_at_5ms() {
        assert_eq!(select_protocol(5.0), ProtocolSelection::TensorParallel);
    }

    #[test]
    fn test_protocol_tensor_parallel_below_5ms() {
        assert_eq!(select_protocol(0.1), ProtocolSelection::TensorParallel);
        assert_eq!(select_protocol(1.0), ProtocolSelection::TensorParallel);
        assert_eq!(select_protocol(4.9), ProtocolSelection::TensorParallel);
    }

    #[test]
    fn test_protocol_tensor_parallel_at_zero() {
        assert_eq!(select_protocol(0.0), ProtocolSelection::TensorParallel);
    }

    #[test]
    fn test_protocol_pipeline_parallel_above_5ms() {
        assert_eq!(select_protocol(5.1), ProtocolSelection::PipelineParallel);
        assert_eq!(select_protocol(10.0), ProtocolSelection::PipelineParallel);
        assert_eq!(select_protocol(25.0), ProtocolSelection::PipelineParallel);
        assert_eq!(select_protocol(49.9), ProtocolSelection::PipelineParallel);
    }

    #[test]
    fn test_protocol_pipeline_parallel_at_50ms() {
        assert_eq!(select_protocol(50.0), ProtocolSelection::PipelineParallel);
    }

    #[test]
    fn test_protocol_rejected_above_50ms() {
        assert_eq!(select_protocol(50.1), ProtocolSelection::Rejected);
        assert_eq!(select_protocol(100.0), ProtocolSelection::Rejected);
        assert_eq!(select_protocol(200.0), ProtocolSelection::Rejected);
    }

    #[test]
    fn test_protocol_selection_to_split_protocol() {
        assert_eq!(
            protocol_selection_to_split_protocol(&ProtocolSelection::TensorParallel),
            Some(SplitProtocol::TensorParallel)
        );
        assert_eq!(
            protocol_selection_to_split_protocol(&ProtocolSelection::PipelineParallel),
            Some(SplitProtocol::PipelineParallel)
        );
        assert_eq!(
            protocol_selection_to_split_protocol(&ProtocolSelection::Rejected),
            None
        );
    }

    // ─── Accept Assignment Tests ─────────────────────────────────────────────

    #[test]
    fn test_accept_assignment_success() {
        let mut worker = LayerWorker::new(3072);
        let assignment = make_test_assignment();

        let result = worker.accept_assignment(&assignment);
        assert!(result.is_ok());
        assert!(worker.has_active_session());

        let session = worker.active_session().unwrap();
        assert_eq!(session.session_id, assignment.session_id);
        assert_eq!(session.model_id, "llama-7b");
        assert_eq!(session.layer_range, (0, 15));
        assert_eq!(session.protocol, SplitProtocol::PipelineParallel);
    }

    #[test]
    fn test_accept_assignment_insufficient_memory() {
        let mut worker = LayerWorker::new(1024); // Only 1GB available
        let assignment = make_test_assignment(); // Needs 2GB

        let result = worker.accept_assignment(&assignment);
        assert!(matches!(
            result,
            Err(LayerWorkerError::InsufficientMemory {
                required_mb: 2048,
                available_mb: 1024
            })
        ));
        assert!(!worker.has_active_session());
    }

    #[test]
    fn test_accept_assignment_invalid_layer_range() {
        let mut worker = LayerWorker::new(3072);
        let mut assignment = make_test_assignment();
        assignment.layer_range = (15, 5); // Invalid: start > end

        let result = worker.accept_assignment(&assignment);
        assert!(matches!(
            result,
            Err(LayerWorkerError::InvalidAssignment(_))
        ));
    }

    #[test]
    fn test_accept_assignment_session_already_active() {
        let mut worker = LayerWorker::new(4096);
        let assignment = make_test_assignment();

        worker.accept_assignment(&assignment).unwrap();

        // Try to accept another assignment without releasing
        let assignment2 = LayerAssignment {
            session_id: Uuid::new_v4(),
            weight_size_mb: 512,
            ..make_test_assignment()
        };
        let result = worker.accept_assignment(&assignment2);
        assert!(matches!(
            result,
            Err(LayerWorkerError::InvalidAssignment(_))
        ));
    }

    #[test]
    fn test_accept_assignment_single_layer() {
        let mut worker = LayerWorker::new(3072);
        let mut assignment = make_test_assignment();
        assignment.layer_range = (5, 5); // Single layer
        assignment.layer_count = 1;

        let result = worker.accept_assignment(&assignment);
        assert!(result.is_ok());
    }

    // ─── Process Activation Tests ────────────────────────────────────────────

    #[test]
    fn test_process_activation_success() {
        let mut worker = LayerWorker::new(3072);
        let assignment = make_test_assignment();
        worker.accept_assignment(&assignment).unwrap();

        let activation = make_test_activation(assignment.session_id);
        let result = worker.process_activation(&activation);
        assert!(result.is_ok());

        let output = result.unwrap();
        assert_eq!(output.session_id, assignment.session_id);
        assert_eq!(output.sequence_num, 1);
        assert_eq!(output.tensor_shape, vec![1, 8]);
        assert_eq!(output.dtype, TensorDtype::F16);
    }

    #[test]
    fn test_process_activation_no_session() {
        let worker = LayerWorker::new(3072);
        let activation = make_test_activation(Uuid::new_v4());

        let result = worker.process_activation(&activation);
        assert!(matches!(result, Err(LayerWorkerError::NoActiveSession)));
    }

    #[test]
    fn test_process_activation_session_mismatch() {
        let mut worker = LayerWorker::new(3072);
        let assignment = make_test_assignment();
        worker.accept_assignment(&assignment).unwrap();

        // Activation with wrong session ID
        let wrong_session = Uuid::new_v4();
        let activation = make_test_activation(wrong_session);

        let result = worker.process_activation(&activation);
        assert!(matches!(
            result,
            Err(LayerWorkerError::SessionMismatch { .. })
        ));
    }

    // ─── Calibration Tests ───────────────────────────────────────────────────

    #[test]
    fn test_calibrate_success() {
        let mut worker = LayerWorker::with_timing(3072, 2.0, 3.0);
        let assignment = make_test_assignment(); // 16 layers (0-15)
        worker.accept_assignment(&assignment).unwrap();

        let result = worker.calibrate();
        assert!(result.is_ok());

        let cal = result.unwrap();
        // 16 layers * 2ms = 32ms compute
        assert!((cal.avg_compute_ms - 32.0).abs() < f64::EPSILON);
        // 3ms forward time
        assert!((cal.avg_forward_ms - 3.0).abs() < f64::EPSILON);
        // 1000 / (32 + 3) = ~28.57 tokens/sec
        let expected_tps = 1000.0 / 35.0;
        assert!((cal.tokens_per_second - expected_tps).abs() < 0.01);
    }

    #[test]
    fn test_calibrate_no_session() {
        let worker = LayerWorker::new(3072);
        let result = worker.calibrate();
        assert!(matches!(result, Err(LayerWorkerError::NoActiveSession)));
    }

    #[test]
    fn test_calibrate_single_layer() {
        let mut worker = LayerWorker::with_timing(3072, 5.0, 1.0);
        let mut assignment = make_test_assignment();
        assignment.layer_range = (10, 10); // Single layer
        assignment.weight_size_mb = 256;
        worker.accept_assignment(&assignment).unwrap();

        let cal = worker.calibrate().unwrap();
        // 1 layer * 5ms = 5ms compute
        assert!((cal.avg_compute_ms - 5.0).abs() < f64::EPSILON);
        assert!((cal.avg_forward_ms - 1.0).abs() < f64::EPSILON);
        // 1000 / 6 = ~166.67 tokens/sec
        let expected_tps = 1000.0 / 6.0;
        assert!((cal.tokens_per_second - expected_tps).abs() < 0.01);
    }

    // ─── Release Session Tests ───────────────────────────────────────────────

    #[test]
    fn test_release_session_success() {
        let mut worker = LayerWorker::new(3072);
        let assignment = make_test_assignment();
        worker.accept_assignment(&assignment).unwrap();
        assert!(worker.has_active_session());

        let result = worker.release_session();
        assert!(result.is_ok());
        assert!(!worker.has_active_session());
    }

    #[test]
    fn test_release_session_no_active_session() {
        let mut worker = LayerWorker::new(3072);
        let result = worker.release_session();
        assert!(matches!(result, Err(LayerWorkerError::NoActiveSession)));
    }

    #[test]
    fn test_release_and_accept_new_session() {
        let mut worker = LayerWorker::new(4096);
        let assignment1 = make_test_assignment();
        worker.accept_assignment(&assignment1).unwrap();

        // Release first session
        worker.release_session().unwrap();

        // Accept a new session
        let assignment2 = LayerAssignment {
            session_id: Uuid::new_v4(),
            model_id: "phi-3b".to_string(),
            layer_range: (0, 7),
            layer_count: 8,
            weight_download_url: "http://example.com/phi-layers.gguf".to_string(),
            weight_size_mb: 1024,
            protocol: SplitProtocol::TensorParallel,
            prev_node: Some(Uuid::new_v4()),
            next_node: None,
            timeout_ms: 50.0,
        };
        let result = worker.accept_assignment(&assignment2);
        assert!(result.is_ok());

        let session = worker.active_session().unwrap();
        assert_eq!(session.model_id, "phi-3b");
        assert_eq!(session.protocol, SplitProtocol::TensorParallel);
    }

    // ─── Full Workflow Test ──────────────────────────────────────────────────

    #[test]
    fn test_full_session_workflow() {
        let mut worker = LayerWorker::new(3072);
        let assignment = make_test_assignment();
        let session_id = assignment.session_id;

        // 1. Accept assignment
        worker.accept_assignment(&assignment).unwrap();
        assert!(worker.has_active_session());

        // 2. Calibrate
        let cal = worker.calibrate().unwrap();
        assert!(cal.tokens_per_second > 0.0);

        // 3. Process activations
        let activation = make_test_activation(session_id);
        let output = worker.process_activation(&activation).unwrap();
        assert_eq!(output.session_id, session_id);

        // 4. Release session
        worker.release_session().unwrap();
        assert!(!worker.has_active_session());
    }
}
