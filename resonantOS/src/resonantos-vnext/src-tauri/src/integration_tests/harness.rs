// TestWorld harness — wires all modules together with controllable mocks.

use super::mock_node::*;
use super::mock_transport::MockTransportManager;
use super::persistence::InMemoryPersistence;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use uuid::Uuid;

/// Events captured during test execution.
#[derive(Debug, Clone)]
pub enum TestEvent {
    NodeAdded { node_id: NodeId },
    PhonePaired { node_id: NodeId },
    PlanCreated { plan_id: String, placements: usize },
    WorkflowStarted { workflow_id: String },
    WorkflowCompleted { workflow_id: String },
    MessageSent { source: NodeId, target: NodeId },
    TransportFailure { node_id: NodeId },
    TransportRecovered { node_id: NodeId },
    NodeCrashed { node_id: NodeId },
    NodeRestored { node_id: NodeId },
}

/// A simplified placement for test assertions.
#[derive(Debug, Clone)]
pub struct TestPlacement {
    pub model_id: String,
    pub assigned_nodes: Vec<NodeId>,
    pub ram_required_mb: u64,
}

/// A simplified placement plan for test assertions.
#[derive(Debug, Clone)]
pub struct TestPlan {
    pub plan_id: String,
    pub placements: Vec<TestPlacement>,
    pub utility_score: f64,
}

/// Workflow status in the test world.
#[derive(Debug, Clone, PartialEq)]
pub enum WorkflowStatus {
    Pending,
    Running,
    Completed,
    Failed { reason: String },
}

/// Workflow state tracked by the test world.
#[derive(Debug, Clone)]
pub struct WorkflowState {
    pub workflow_id: String,
    pub status: WorkflowStatus,
    pub completed_steps: Vec<String>,
    pub total_steps: usize,
}

/// The test world — wires all modules together with controllable mocks.
pub struct TestWorld {
    pub nodes: HashMap<NodeId, RegisteredNode>,
    pub phones: HashMap<NodeId, RegisteredPhone>,
    pub transport: Arc<MockTransportManager>,
    pub persistence: Arc<InMemoryPersistence>,
    pub event_log: Arc<Mutex<Vec<TestEvent>>>,
    pub workflows: HashMap<String, WorkflowState>,
    pub current_plan: Option<TestPlan>,
    pub simulated_time_ms: u64,
    pub demand_weights: HashMap<String, f64>,
}

impl TestWorld {
    /// Create a fresh test world with no nodes.
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            phones: HashMap::new(),
            transport: Arc::new(MockTransportManager::new()),
            persistence: Arc::new(InMemoryPersistence::new()),
            event_log: Arc::new(Mutex::new(Vec::new())),
            workflows: HashMap::new(),
            current_plan: None,
            simulated_time_ms: 0,
            demand_weights: HashMap::new(),
        }
    }

    /// Add a mock node with the given capabilities.
    pub fn add_node(&mut self, config: MockNodeConfig) -> NodeId {
        let id = Uuid::new_v4();
        let node = RegisteredNode {
            id,
            config,
            online: true,
            ram_used_mb: 0,
            vram_used_mb: 0,
        };
        self.nodes.insert(id, node);
        self.emit(TestEvent::NodeAdded { node_id: id });
        id
    }

    /// Add a phone companion node (paired).
    pub fn add_phone(&mut self, config: MockPhoneConfig) -> NodeId {
        let id = Uuid::new_v4();
        let phone = RegisteredPhone {
            id,
            config,
            online: true,
            paired: true,
        };
        self.phones.insert(id, phone);
        self.emit(TestEvent::PhonePaired { node_id: id });
        id
    }

    /// Run a full optimizer cycle and return the plan.
    pub fn run_optimizer(&mut self) -> TestPlan {
        // Simplified optimizer: assign models based on available RAM/VRAM
        let mut placements = Vec::new();
        let nodes: Vec<&RegisteredNode> = self.nodes.values().filter(|n| n.online).collect();

        if !nodes.is_empty() {
            // Create a basic placement for each node with available resources
            for (i, node) in nodes.iter().enumerate() {
                if node.config.vram_mb > 0 {
                    placements.push(TestPlacement {
                        model_id: format!("model-gpu-{}", i),
                        assigned_nodes: vec![node.id],
                        ram_required_mb: 4000,
                    });
                } else if node.config.ram_mb >= 8000 {
                    placements.push(TestPlacement {
                        model_id: format!("model-cpu-{}", i),
                        assigned_nodes: vec![node.id],
                        ram_required_mb: 4000,
                    });
                }
            }

            // Assign phone nodes small models
            for phone in self.phones.values().filter(|p| p.online && p.paired) {
                if phone.config.battery_percent > 20 {
                    placements.push(TestPlacement {
                        model_id: format!("model-phone-{}", phone.config.hostname),
                        assigned_nodes: vec![phone.id],
                        ram_required_mb: 2000,
                    });
                }
            }
        }

        let plan = TestPlan {
            plan_id: Uuid::new_v4().to_string(),
            placements,
            utility_score: 0.85,
        };

        self.emit(TestEvent::PlanCreated {
            plan_id: plan.plan_id.clone(),
            placements: plan.placements.len(),
        });

        self.current_plan = Some(plan.clone());
        plan
    }

    /// Submit an agent workflow and return the workflow ID.
    pub fn submit_workflow(&mut self, steps: Vec<String>) -> String {
        let workflow_id = Uuid::new_v4().to_string();
        let state = WorkflowState {
            workflow_id: workflow_id.clone(),
            status: WorkflowStatus::Running,
            completed_steps: Vec::new(),
            total_steps: steps.len(),
        };
        self.workflows.insert(workflow_id.clone(), state);
        self.emit(TestEvent::WorkflowStarted {
            workflow_id: workflow_id.clone(),
        });
        workflow_id
    }

    /// Advance simulated time and execute pending workflow steps.
    pub fn advance_time(&mut self, duration: Duration) {
        self.simulated_time_ms += duration.as_millis() as u64;

        // Auto-complete one workflow step per second of simulated time
        let steps_to_complete = (duration.as_secs() as usize).max(1);

        for state in self.workflows.values_mut() {
            if state.status == WorkflowStatus::Running {
                for _i in 0..steps_to_complete {
                    if state.completed_steps.len() < state.total_steps {
                        state
                            .completed_steps
                            .push(format!("step-{}", state.completed_steps.len() + 1));
                    }
                }
                if state.completed_steps.len() >= state.total_steps {
                    state.status = WorkflowStatus::Completed;
                }
            }
        }
    }

    /// Inject a transport failure for a specific node.
    pub fn inject_transport_failure(&mut self, node_id: NodeId) {
        self.transport.inject_failure(node_id);
        self.emit(TestEvent::TransportFailure { node_id });
    }

    /// Recover a previously failed transport.
    pub fn recover_transport(&mut self, node_id: NodeId) {
        self.transport.recover(node_id);
        self.emit(TestEvent::TransportRecovered { node_id });
    }

    /// Take a node offline (simulates crash).
    pub fn crash_node(&mut self, node_id: NodeId) {
        if let Some(node) = self.nodes.get_mut(&node_id) {
            node.online = false;
        }
        if let Some(phone) = self.phones.get_mut(&node_id) {
            phone.online = false;
        }
        self.transport.inject_failure(node_id);
        self.emit(TestEvent::NodeCrashed { node_id });
    }

    /// Bring a node back online.
    pub fn restore_node(&mut self, node_id: NodeId) {
        if let Some(node) = self.nodes.get_mut(&node_id) {
            node.online = true;
        }
        if let Some(phone) = self.phones.get_mut(&node_id) {
            phone.online = true;
        }
        self.transport.recover(node_id);
        self.emit(TestEvent::NodeRestored { node_id });
    }

    /// Send a message between nodes via mock transport.
    pub fn send_message(
        &self,
        source: NodeId,
        target: NodeId,
        payload: Vec<u8>,
    ) -> Result<(), super::mock_transport::MockTransportError> {
        self.transport.send(source, target, payload, "default")
    }

    /// Get all captured messages sent via transport.
    pub fn captured_messages(&self) -> Vec<super::mock_transport::CapturedMessage> {
        self.transport.captured_messages()
    }

    /// Get workflow status.
    pub fn get_workflow_status(&self, workflow_id: &str) -> Option<&WorkflowState> {
        self.workflows.get(workflow_id)
    }

    /// Checkpoint a workflow (save to persistence).
    pub fn checkpoint_workflow(&self, workflow_id: &str) {
        if let Some(state) = self.workflows.get(workflow_id) {
            let cp = super::persistence::WorkflowCheckpoint {
                workflow_id: workflow_id.to_string(),
                completed_steps: state.completed_steps.clone(),
                step_results: HashMap::new(),
                created_at_ms: self.simulated_time_ms,
            };
            self.persistence.save_checkpoint(cp);
        }
    }

    /// Resume a workflow from checkpoint.
    pub fn resume_workflow(&mut self, workflow_id: &str, total_steps: usize) -> bool {
        if let Some(cp) = self.persistence.load_checkpoint(workflow_id) {
            let state = WorkflowState {
                workflow_id: workflow_id.to_string(),
                status: WorkflowStatus::Running,
                completed_steps: cp.completed_steps,
                total_steps,
            };
            self.workflows.insert(workflow_id.to_string(), state);
            true
        } else {
            false
        }
    }

    /// Set demand weights for optimizer.
    pub fn set_demand(&mut self, demands: Vec<(&str, f64)>) {
        self.demand_weights = demands
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect();
    }

    /// Get node by ID.
    pub fn get_node(&self, node_id: &NodeId) -> Option<&RegisteredNode> {
        self.nodes.get(node_id)
    }

    /// Get phone by ID.
    pub fn get_phone(&self, node_id: &NodeId) -> Option<&RegisteredPhone> {
        self.phones.get(node_id)
    }

    /// Get all events.
    pub fn events(&self) -> Vec<TestEvent> {
        self.event_log.lock().unwrap().clone()
    }

    fn emit(&self, event: TestEvent) {
        self.event_log.lock().unwrap().push(event);
    }
}
