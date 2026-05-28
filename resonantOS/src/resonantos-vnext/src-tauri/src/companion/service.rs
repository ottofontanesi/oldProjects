//! CompanionService: top-level orchestrator wiring all companion subsystems together.
//!
//! Initialization order: Identity → Transport → Health → Assignment → LayerWorker → Lifecycle
//!
//! Implements:
//! - Subsystem initialization in correct dependency order
//! - Message dispatch loop: receive CoordinatorMessage, route to handler
//! - PhoneMessage sending for all outbound messages

use crate::companion::assignment::{AssignmentManager, PhoneConstraints};
use crate::companion::health::{HealthReporter, HealthReporterConfig, PhoneHealthState};
use crate::companion::layer_worker::LayerWorker;
use crate::companion::lifecycle::{AppLifecycle, PlatformLifecycle};
use crate::companion::transport_bridge::CompanionTransportBridge;
use crate::companion::types::{
    AssignmentResponse, CalibrationResult, CoordinatorMessage, ModelId, NodeId, PhoneMessage,
    PhoneSettings, SessionId,
};

use uuid::Uuid;

// ─── Service State ───────────────────────────────────────────────────────────

/// Current state of the companion service.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceState {
    /// Service has not been initialized yet.
    Uninitialized,
    /// Service is initializing subsystems.
    Initializing,
    /// Service is running and connected to the mesh.
    Running,
    /// Service is paused (app in background).
    Paused,
    /// Service has been stopped.
    Stopped,
    /// Service encountered a fatal error.
    Error,
}

/// Errors that can occur during service operations.
#[derive(Debug, Clone)]
pub enum ServiceError {
    /// Service is not in the expected state for this operation.
    InvalidState { expected: ServiceState, actual: ServiceState },
    /// A subsystem failed to initialize.
    InitializationFailed(String),
    /// Message dispatch failed.
    DispatchFailed(String),
    /// The service is not connected to the mesh.
    NotConnected,
}

impl std::fmt::Display for ServiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidState { expected, actual } => {
                write!(f, "Invalid state: expected {:?}, got {:?}", expected, actual)
            }
            Self::InitializationFailed(msg) => write!(f, "Initialization failed: {}", msg),
            Self::DispatchFailed(msg) => write!(f, "Dispatch failed: {}", msg),
            Self::NotConnected => write!(f, "Not connected to mesh"),
        }
    }
}

impl std::error::Error for ServiceError {}

// ─── Message Dispatch Result ─────────────────────────────────────────────────

/// Result of dispatching a CoordinatorMessage.
#[derive(Debug, Clone)]
pub enum DispatchResult {
    /// Assignment was handled, response ready to send.
    AssignmentHandled(AssignmentResponse),
    /// Model was unloaded successfully.
    ModelUnloaded { model_id: ModelId },
    /// Split session started, calibration result ready.
    SessionStarted {
        session_id: SessionId,
        calibration: CalibrationResult,
    },
    /// Split session ended.
    SessionEnded { session_id: SessionId },
    /// Ping received, pong ready to send.
    Pong,
    /// Dispatch failed with an error message.
    Failed { reason: String },
}

// ─── CompanionService ────────────────────────────────────────────────────────

/// Top-level service that holds and orchestrates all companion subsystems.
///
/// Initialization order:
/// 1. Identity (MeshIdentity) — generates or loads Ed25519 keypair
/// 2. Transport (CompanionTransportBridge) — connects to mesh transport layer
/// 3. Health (HealthReporter) — starts periodic heartbeats
/// 4. Assignment (AssignmentManager) — ready to accept model assignments
/// 5. LayerWorker — ready to participate in split inference
/// 6. Lifecycle (AppLifecycle) — manages background/foreground transitions
pub struct CompanionService {
    /// Current service state.
    state: ServiceState,
    /// The node's identity in the mesh.
    node_id: NodeId,
    /// Transport bridge for path selection and failover.
    transport_bridge: CompanionTransportBridge,
    /// Health reporter for heartbeats and alerts.
    health_reporter: HealthReporter,
    /// Assignment manager for model placement.
    assignment_manager: AssignmentManager,
    /// Layer worker for split inference participation.
    layer_worker: LayerWorker,
    /// App lifecycle manager.
    lifecycle: AppLifecycle,
    /// Outbound message queue (messages to send to Coordinator).
    outbound_messages: Vec<PhoneMessage>,
}

impl CompanionService {
    /// Create a new CompanionService in uninitialized state.
    pub fn new(node_id: NodeId) -> Self {
        Self {
            state: ServiceState::Uninitialized,
            node_id,
            transport_bridge: CompanionTransportBridge::with_defaults(),
            health_reporter: HealthReporter::with_defaults(node_id),
            assignment_manager: AssignmentManager::with_defaults(),
            layer_worker: LayerWorker::new(3072), // 3GB default
            lifecycle: AppLifecycle::new_platform(),
            outbound_messages: Vec::new(),
        }
    }

    /// Create a new CompanionService with custom configuration.
    pub fn with_config(
        node_id: NodeId,
        health_config: HealthReporterConfig,
        constraints: PhoneConstraints,
        available_memory_mb: u64,
        platform: PlatformLifecycle,
    ) -> Self {
        Self {
            state: ServiceState::Uninitialized,
            node_id,
            transport_bridge: CompanionTransportBridge::with_defaults(),
            health_reporter: HealthReporter::new(health_config, node_id),
            assignment_manager: AssignmentManager::new(constraints),
            layer_worker: LayerWorker::new(available_memory_mb),
            lifecycle: AppLifecycle::new(platform),
            outbound_messages: Vec::new(),
        }
    }

    /// Initialize all subsystems in the correct order.
    ///
    /// Order: Identity → Transport → Health → Assignment → LayerWorker → Lifecycle
    pub fn initialize(&mut self) -> Result<(), ServiceError> {
        if self.state != ServiceState::Uninitialized && self.state != ServiceState::Stopped {
            return Err(ServiceError::InvalidState {
                expected: ServiceState::Uninitialized,
                actual: self.state,
            });
        }

        self.state = ServiceState::Initializing;

        // Step 1: Identity is already set (node_id provided at construction)
        // Step 2: Transport bridge is ready (paths will be updated when connected)
        // Step 3: Health reporter is ready (will start sending heartbeats)
        // Step 4: Assignment manager is ready
        // Step 5: Layer worker is ready
        // Step 6: Launch lifecycle
        self.lifecycle
            .on_launch()
            .map_err(|e| ServiceError::InitializationFailed(e.to_string()))?;

        self.state = ServiceState::Running;
        Ok(())
    }

    /// Get the current service state.
    pub fn state(&self) -> ServiceState {
        self.state
    }

    /// Get the node ID.
    pub fn node_id(&self) -> NodeId {
        self.node_id
    }

    /// Get a reference to the transport bridge.
    pub fn transport_bridge(&self) -> &CompanionTransportBridge {
        &self.transport_bridge
    }

    /// Get a mutable reference to the transport bridge.
    pub fn transport_bridge_mut(&mut self) -> &mut CompanionTransportBridge {
        &mut self.transport_bridge
    }

    /// Get a reference to the health reporter.
    pub fn health_reporter(&self) -> &HealthReporter {
        &self.health_reporter
    }

    /// Get a reference to the assignment manager.
    pub fn assignment_manager(&self) -> &AssignmentManager {
        &self.assignment_manager
    }

    /// Get a reference to the layer worker.
    pub fn layer_worker(&self) -> &LayerWorker {
        &self.layer_worker
    }

    /// Get a reference to the lifecycle manager.
    pub fn lifecycle(&self) -> &AppLifecycle {
        &self.lifecycle
    }

    /// Drain the outbound message queue.
    pub fn drain_outbound_messages(&mut self) -> Vec<PhoneMessage> {
        std::mem::take(&mut self.outbound_messages)
    }

    /// Dispatch a CoordinatorMessage to the appropriate handler.
    ///
    /// Routes messages to the correct subsystem and queues outbound responses.
    pub fn dispatch(&mut self, message: CoordinatorMessage) -> DispatchResult {
        if self.state != ServiceState::Running {
            return DispatchResult::Failed {
                reason: format!("Service not running (state: {:?})", self.state),
            };
        }

        match message {
            CoordinatorMessage::AssignModel(assignment) => {
                let response = self.assignment_manager.handle_assignment(&assignment);
                self.outbound_messages
                    .push(PhoneMessage::AssignmentResponse(response.clone()));
                DispatchResult::AssignmentHandled(response)
            }

            CoordinatorMessage::UnloadModel { model_id } => {
                match self.assignment_manager.handle_unload(&model_id) {
                    Ok(()) => {
                        self.outbound_messages.push(PhoneMessage::UnloadConfirm {
                            model_id: model_id.clone(),
                        });
                        DispatchResult::ModelUnloaded { model_id }
                    }
                    Err(e) => DispatchResult::Failed {
                        reason: e.to_string(),
                    },
                }
            }

            CoordinatorMessage::StartSplitSession {
                session_id,
                assignment,
            } => {
                match self.layer_worker.accept_assignment(&assignment) {
                    Ok(()) => {
                        match self.layer_worker.calibrate() {
                            Ok(calibration) => {
                                self.outbound_messages.push(PhoneMessage::SessionReady {
                                    session_id,
                                    calibration: calibration.clone(),
                                });
                                DispatchResult::SessionStarted {
                                    session_id,
                                    calibration,
                                }
                            }
                            Err(e) => {
                                self.outbound_messages.push(PhoneMessage::SessionFailed {
                                    session_id,
                                    reason: e.to_string(),
                                });
                                DispatchResult::Failed {
                                    reason: e.to_string(),
                                }
                            }
                        }
                    }
                    Err(e) => {
                        self.outbound_messages.push(PhoneMessage::SessionFailed {
                            session_id,
                            reason: e.to_string(),
                        });
                        DispatchResult::Failed {
                            reason: e.to_string(),
                        }
                    }
                }
            }

            CoordinatorMessage::EndSplitSession { session_id } => {
                let _ = self.layer_worker.release_session();
                DispatchResult::SessionEnded { session_id }
            }

            CoordinatorMessage::Ping => {
                self.outbound_messages.push(PhoneMessage::Pong);
                DispatchResult::Pong
            }
        }
    }

    /// Generate a heartbeat from the current health state.
    pub fn generate_heartbeat(
        &self,
        health_state: &PhoneHealthState,
        timestamp_ms: u64,
    ) -> PhoneMessage {
        let heartbeat = self.health_reporter.build_heartbeat(health_state, timestamp_ms);
        PhoneMessage::Heartbeat(heartbeat)
    }

    /// Pause the service (app moving to background).
    pub fn pause(&mut self) {
        self.lifecycle.on_background();
        self.state = ServiceState::Paused;
    }

    /// Resume the service (app returning to foreground).
    pub fn resume(&mut self) -> Result<(), ServiceError> {
        self.lifecycle
            .on_launch()
            .map_err(|e| ServiceError::InitializationFailed(e.to_string()))?;
        self.state = ServiceState::Running;
        Ok(())
    }

    /// Stop the service gracefully.
    pub fn stop(&mut self) {
        self.lifecycle.on_user_stop();
        self.outbound_messages.push(PhoneMessage::GracefulLeave);
        self.state = ServiceState::Stopped;
    }

    /// Update phone settings on the assignment manager.
    pub fn update_settings(&mut self, settings: &PhoneSettings) {
        let constraints = PhoneConstraints {
            battery_threshold: settings.battery_threshold,
            allow_cellular: settings.allow_cellular,
            max_model_size_mb: settings.max_model_size_mb,
            ..PhoneConstraints::default()
        };
        self.assignment_manager.update_constraints(constraints);
    }
}

// ─── Unit Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::companion::types::{
        AssignmentPriority, AssignmentType, LayerAssignment, ModelAssignment, SplitProtocol,
    };

    fn test_node_id() -> NodeId {
        Uuid::new_v4()
    }

    fn make_model_assignment() -> ModelAssignment {
        ModelAssignment {
            model_id: "test-model".to_string(),
            assignment_type: AssignmentType::FullModel { params_b: 2.0 },
            download_url: "http://example.com/model.gguf".to_string(),
            weight_size_mb: 1024,
            priority: AssignmentPriority::Normal,
        }
    }

    fn make_layer_assignment(session_id: SessionId) -> LayerAssignment {
        LayerAssignment {
            session_id,
            model_id: "llama-7b".to_string(),
            layer_range: (0, 7),
            layer_count: 8,
            weight_download_url: "http://example.com/layers.gguf".to_string(),
            weight_size_mb: 1024,
            protocol: SplitProtocol::PipelineParallel,
            prev_node: None,
            next_node: Some(Uuid::new_v4()),
            timeout_ms: 100.0,
        }
    }

    // ─── Initialization Tests ────────────────────────────────────────────────

    #[test]
    fn test_service_initial_state() {
        let service = CompanionService::new(test_node_id());
        assert_eq!(service.state(), ServiceState::Uninitialized);
    }

    #[test]
    fn test_service_initialize() {
        let mut service = CompanionService::new(test_node_id());
        let result = service.initialize();
        assert!(result.is_ok());
        assert_eq!(service.state(), ServiceState::Running);
    }

    #[test]
    fn test_service_cannot_initialize_when_running() {
        let mut service = CompanionService::new(test_node_id());
        service.initialize().unwrap();

        let result = service.initialize();
        assert!(result.is_err());
    }

    #[test]
    fn test_service_can_reinitialize_after_stop() {
        let mut service = CompanionService::new(test_node_id());
        service.initialize().unwrap();
        service.stop();

        let result = service.initialize();
        assert!(result.is_ok());
        assert_eq!(service.state(), ServiceState::Running);
    }

    // ─── Dispatch Tests ──────────────────────────────────────────────────────

    #[test]
    fn test_dispatch_assign_model() {
        let mut service = CompanionService::new(test_node_id());
        service.initialize().unwrap();

        let assignment = make_model_assignment();
        let result = service.dispatch(CoordinatorMessage::AssignModel(assignment));

        assert!(matches!(result, DispatchResult::AssignmentHandled(AssignmentResponse::Accepted { .. })));

        let messages = service.drain_outbound_messages();
        assert_eq!(messages.len(), 1);
        assert!(matches!(messages[0], PhoneMessage::AssignmentResponse(AssignmentResponse::Accepted { .. })));
    }

    #[test]
    fn test_dispatch_unload_model() {
        let mut service = CompanionService::new(test_node_id());
        service.initialize().unwrap();

        // First assign a model
        let assignment = make_model_assignment();
        service.dispatch(CoordinatorMessage::AssignModel(assignment));
        service.drain_outbound_messages(); // Clear

        // Then unload it
        let result = service.dispatch(CoordinatorMessage::UnloadModel {
            model_id: "test-model".to_string(),
        });

        assert!(matches!(result, DispatchResult::ModelUnloaded { .. }));

        let messages = service.drain_outbound_messages();
        assert_eq!(messages.len(), 1);
        assert!(matches!(messages[0], PhoneMessage::UnloadConfirm { .. }));
    }

    #[test]
    fn test_dispatch_unload_nonexistent_model() {
        let mut service = CompanionService::new(test_node_id());
        service.initialize().unwrap();

        let result = service.dispatch(CoordinatorMessage::UnloadModel {
            model_id: "nonexistent".to_string(),
        });

        assert!(matches!(result, DispatchResult::Failed { .. }));
    }

    #[test]
    fn test_dispatch_start_split_session() {
        let mut service = CompanionService::new(test_node_id());
        service.initialize().unwrap();

        let session_id = Uuid::new_v4();
        let assignment = make_layer_assignment(session_id);

        let result = service.dispatch(CoordinatorMessage::StartSplitSession {
            session_id,
            assignment,
        });

        assert!(matches!(result, DispatchResult::SessionStarted { .. }));

        let messages = service.drain_outbound_messages();
        assert_eq!(messages.len(), 1);
        assert!(matches!(messages[0], PhoneMessage::SessionReady { .. }));
    }

    #[test]
    fn test_dispatch_end_split_session() {
        let mut service = CompanionService::new(test_node_id());
        service.initialize().unwrap();

        let session_id = Uuid::new_v4();
        let assignment = make_layer_assignment(session_id);

        // Start session first
        service.dispatch(CoordinatorMessage::StartSplitSession {
            session_id,
            assignment,
        });
        service.drain_outbound_messages();

        // End session
        let result = service.dispatch(CoordinatorMessage::EndSplitSession { session_id });
        assert!(matches!(result, DispatchResult::SessionEnded { .. }));
    }

    #[test]
    fn test_dispatch_ping_pong() {
        let mut service = CompanionService::new(test_node_id());
        service.initialize().unwrap();

        let result = service.dispatch(CoordinatorMessage::Ping);
        assert!(matches!(result, DispatchResult::Pong));

        let messages = service.drain_outbound_messages();
        assert_eq!(messages.len(), 1);
        assert!(matches!(messages[0], PhoneMessage::Pong));
    }

    #[test]
    fn test_dispatch_fails_when_not_running() {
        let mut service = CompanionService::new(test_node_id());
        // Don't initialize

        let result = service.dispatch(CoordinatorMessage::Ping);
        assert!(matches!(result, DispatchResult::Failed { .. }));
    }

    // ─── Lifecycle Tests ─────────────────────────────────────────────────────

    #[test]
    fn test_service_pause_and_resume() {
        let mut service = CompanionService::new(test_node_id());
        service.initialize().unwrap();

        service.pause();
        assert_eq!(service.state(), ServiceState::Paused);

        service.resume().unwrap();
        assert_eq!(service.state(), ServiceState::Running);
    }

    #[test]
    fn test_service_stop() {
        let mut service = CompanionService::new(test_node_id());
        service.initialize().unwrap();

        service.stop();
        assert_eq!(service.state(), ServiceState::Stopped);

        let messages = service.drain_outbound_messages();
        assert!(messages.iter().any(|m| matches!(m, PhoneMessage::GracefulLeave)));
    }

    // ─── Heartbeat Tests ─────────────────────────────────────────────────────

    #[test]
    fn test_generate_heartbeat() {
        let node_id = test_node_id();
        let service = CompanionService::new(node_id);

        let health_state = PhoneHealthState::default();
        let msg = service.generate_heartbeat(&health_state, 1000);

        match msg {
            PhoneMessage::Heartbeat(hb) => {
                assert_eq!(hb.node_id, node_id);
                assert_eq!(hb.timestamp_ms, 1000);
            }
            _ => panic!("Expected Heartbeat message"),
        }
    }

    // ─── Settings Tests ──────────────────────────────────────────────────────

    #[test]
    fn test_update_settings() {
        let mut service = CompanionService::new(test_node_id());
        service.initialize().unwrap();

        let settings = PhoneSettings {
            battery_threshold: 30,
            allow_cellular: true,
            max_model_size_mb: 2048,
            ..PhoneSettings::default()
        };

        service.update_settings(&settings);

        let constraints = service.assignment_manager().constraints();
        assert_eq!(constraints.battery_threshold, 30);
        assert!(constraints.allow_cellular);
        assert_eq!(constraints.max_model_size_mb, 2048);
    }

    // ─── Full Workflow Test ──────────────────────────────────────────────────

    #[test]
    fn test_full_service_workflow() {
        let mut service = CompanionService::new(test_node_id());

        // Initialize
        service.initialize().unwrap();
        assert_eq!(service.state(), ServiceState::Running);

        // Receive ping
        service.dispatch(CoordinatorMessage::Ping);
        let msgs = service.drain_outbound_messages();
        assert_eq!(msgs.len(), 1);

        // Receive model assignment
        let assignment = make_model_assignment();
        service.dispatch(CoordinatorMessage::AssignModel(assignment));
        let msgs = service.drain_outbound_messages();
        assert_eq!(msgs.len(), 1);

        // Pause (background)
        service.pause();
        assert_eq!(service.state(), ServiceState::Paused);

        // Resume
        service.resume().unwrap();
        assert_eq!(service.state(), ServiceState::Running);

        // Stop
        service.stop();
        assert_eq!(service.state(), ServiceState::Stopped);
        let msgs = service.drain_outbound_messages();
        assert!(msgs.iter().any(|m| matches!(m, PhoneMessage::GracefulLeave)));
    }
}
