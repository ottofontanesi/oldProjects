# Design Document: End-to-End Integration Tests

## Overview

Cross-module integration tests that exercise full system flows: pairing → assignment → split inference, agent workflow execution, transport failover, optimizer cycles, and crash recovery. All tests use mock adapters and in-memory state — no real network, no real hardware, no real files. The test harness provides a `TestWorld` that wires all modules together with controllable mocks.

### Design Principles

1. **No external dependencies**: Tests run with `cargo test` — no network, no files, no databases.
2. **Deterministic**: No real timers or random delays. Time is simulated via a controllable clock.
3. **Fast**: All tests complete in <30 seconds total. Individual tests <5 seconds.
4. **Reusable harness**: `TestWorld` makes writing new integration tests trivial.
5. **Independent**: Each test creates its own `TestWorld` — no shared mutable state.

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                        TestWorld                                  │
│                                                                  │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────────────┐  │
│  │ MockTransport│  │ MockNodes    │  │ InMemoryPersistence  │  │
│  │              │  │              │  │                      │  │
│  │ • latency    │  │ • desktop    │  │ • node state         │  │
│  │ • failures   │  │ • laptop     │  │ • checkpoints        │  │
│  │ • captures   │  │ • phone      │  │ • resume state       │  │
│  └──────┬───────┘  └──────┬───────┘  └──────────┬───────────┘  │
│         │                  │                     │              │
│  ┌──────┴──────────────────┴─────────────────────┴───────────┐  │
│  │                    Wired Modules                            │  │
│  │                                                            │  │
│  │  NodeRegistry ←→ TransportManager ←→ Solver                │  │
│  │       ↕                  ↕                ↕                │  │
│  │  CompanionService   AgentOrchestrator   Executor           │  │
│  │       ↕                  ↕                                 │  │
│  │  LayerWorker        StepWorker                             │  │
│  └────────────────────────────────────────────────────────────┘  │
│                                                                  │
│  ┌──────────────────────────────────────────────────────────┐    │
│  │                    ScenarioRunner                          │    │
│  │  • advance_time(duration)                                 │    │
│  │  • inject_failure(node, type)                             │    │
│  │  • assert_state(predicate)                                │    │
│  │  • wait_for(condition, timeout)                           │    │
│  └──────────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────────┘
```

## TestWorld API

```rust
pub struct TestWorld {
    pub registry: Arc<NodeRegistry>,
    pub transport: Arc<MockTransportManager>,
    pub solver_config: SolverConfig,
    pub agent_orchestrator: WorkflowOrchestrator,
    pub companion_service: Option<CompanionService>,
    pub persistence: Arc<InMemoryPersistence>,
    pub clock: SimulatedClock,
    pub event_log: Arc<Mutex<Vec<TestEvent>>>,
}

impl TestWorld {
    /// Create a fresh test world with no nodes.
    pub fn new() -> Self;

    /// Add a mock node with the given capabilities.
    pub fn add_node(&mut self, config: MockNodeConfig) -> NodeId;

    /// Add a phone companion node (paired).
    pub fn add_phone(&mut self, config: MockPhoneConfig) -> NodeId;

    /// Run a full optimizer cycle and return the plan.
    pub fn run_optimizer(&mut self) -> PlacementPlan;

    /// Submit an agent workflow and return the workflow ID.
    pub fn submit_workflow(&mut self, plan: AgentPlan) -> WorkflowId;

    /// Advance simulated time by the given duration.
    pub fn advance_time(&mut self, duration: Duration);

    /// Inject a transport failure for a specific node.
    pub fn inject_transport_failure(&mut self, node_id: NodeId);

    /// Recover a previously failed transport.
    pub fn recover_transport(&mut self, node_id: NodeId);

    /// Take a node offline (simulates crash).
    pub fn crash_node(&mut self, node_id: NodeId);

    /// Bring a node back online.
    pub fn restore_node(&mut self, node_id: NodeId);

    /// Get all captured messages sent via transport.
    pub fn captured_messages(&self) -> Vec<CapturedMessage>;

    /// Assert a condition, with a descriptive failure message.
    pub fn assert(&self, condition: bool, msg: &str);
}
```

### MockNodeConfig

```rust
pub struct MockNodeConfig {
    pub hostname: String,
    pub device_type: DeviceType,
    pub ram_mb: u64,
    pub vram_mb: u64,
    pub cpu_cores: u32,
    pub clock_mhz: u32,
    pub tools: Vec<String>,
    pub models_loaded: Vec<String>,
    pub stability: f64,
    pub latency_to_peers: HashMap<NodeId, f64>,
}

pub struct MockPhoneConfig {
    pub hostname: String,
    pub ram_mb: u64,
    pub battery_percent: u8,
    pub npu_type: String,
    pub tools: Vec<String>,
    pub connection_type: ConnectionType,
}
```

### MockTransportManager

```rust
pub struct MockTransportManager {
    pub latency_ms: f64,
    pub failure_rate: f64,
    pub messages: Arc<Mutex<Vec<CapturedMessage>>>,
    pub failed_nodes: Arc<Mutex<HashSet<NodeId>>>,
    pub delivery_callback: Option<Box<dyn Fn(&TransportMessage) + Send + Sync>>,
}

impl MeshTransport for MockTransportManager {
    fn send(&self, target: &NodeId, message: &TransportMessage) -> Result<(), TransportError> {
        if self.failed_nodes.lock().contains(target) {
            return Err(TransportError::Unreachable { target: *target });
        }
        // Simulate latency (in real tests, this is instant since time is simulated)
        self.messages.lock().push(CapturedMessage { target: *target, message: message.clone() });
        Ok(())
    }
    // ... other trait methods
}
```

### InMemoryPersistence

```rust
pub struct InMemoryPersistence {
    pub node_states: Mutex<HashMap<NodeId, PhoneNodeState>>,
    pub checkpoints: Mutex<HashMap<WorkflowId, WorkflowCheckpoint>>,
    pub resume_states: Mutex<HashMap<DownloadId, ResumeState>>,
}

impl InMemoryPersistence {
    pub fn new() -> Self;
    pub fn save_checkpoint(&self, id: WorkflowId, cp: WorkflowCheckpoint);
    pub fn load_checkpoint(&self, id: WorkflowId) -> Option<WorkflowCheckpoint>;
    pub fn save_node_state(&self, id: NodeId, state: PhoneNodeState);
    pub fn load_node_state(&self, id: NodeId) -> Option<PhoneNodeState>;
}
```

## Test Scenarios

### Scenario 1: Pairing → Assignment → Split Inference

```rust
#[test]
fn test_pairing_to_inference_flow() {
    let mut world = TestWorld::new();

    // Setup: desktop with 32GB RAM, GPU
    let desktop = world.add_node(MockNodeConfig {
        hostname: "desktop".into(),
        device_type: DeviceType::Desktop,
        ram_mb: 32_000,
        vram_mb: 24_000,
        ..Default::default()
    });

    // Step 1: Phone pairs with desktop
    let phone = world.add_phone(MockPhoneConfig {
        hostname: "iphone".into(),
        ram_mb: 6_000,
        battery_percent: 85,
        npu_type: "Apple Neural Engine".into(),
        ..Default::default()
    });

    // Step 2: Verify phone in registry
    assert!(world.registry.get_node(&phone).is_some());

    // Step 3: Run optimizer → phone gets layer assignment
    let plan = world.run_optimizer();
    assert!(!plan.agent_placements.is_empty() || !plan.placements.is_empty());

    // Step 4: Simulate split inference using phone's layers
    // ... (verify activation forwarding and result collection)
}
```

### Scenario 2: Agent Workflow Execution

```rust
#[test]
fn test_agent_workflow_end_to_end() {
    let mut world = TestWorld::new();

    // Setup: 2 nodes with different tools
    let node_a = world.add_node(MockNodeConfig {
        tools: vec!["browser".into(), "filesystem".into()],
        ..desktop_config()
    });
    let node_b = world.add_node(MockNodeConfig {
        tools: vec!["code_exec".into(), "filesystem".into()],
        ..laptop_config()
    });

    // Submit 3-step workflow: search (browser) || code (code_exec) → synthesize (filesystem)
    let plan = AgentPlan { steps: vec![...] };
    let workflow_id = world.submit_workflow(plan);

    // Execute all steps
    world.advance_time(Duration::from_secs(5));

    // Verify: all steps completed, parallel steps ran on different nodes
    let status = world.agent_orchestrator.get_workflow_status(workflow_id);
    assert_eq!(status.status, WorkflowStatus::Completed);
}
```

### Scenario 3: Transport Failover

```rust
#[test]
fn test_transport_failover_flow() {
    let mut world = TestWorld::new();
    let node_a = world.add_node(desktop_config());
    let node_b = world.add_node(laptop_config());

    // Send message via primary transport — succeeds
    world.send_message(node_a, node_b, test_message());
    assert_eq!(world.captured_messages().len(), 1);

    // Inject failure on primary path
    world.inject_transport_failure(node_b);

    // Send message — should fail on primary, succeed on secondary
    // (failover within 100ms)
    world.send_message(node_a, node_b, test_message());
    // Verify failover occurred
    // ...

    // Recover primary
    world.recover_transport(node_b);
    // Verify traffic returns to primary
}
```

### Scenario 4: Full Optimizer Cycle

```rust
#[test]
fn test_optimizer_cycle_multi_node() {
    let mut world = TestWorld::new();

    // Setup: heterogeneous network
    let desktop = world.add_node(MockNodeConfig {
        hostname: "desktop".into(),
        device_type: DeviceType::Desktop,
        ram_mb: 64_000,
        vram_mb: 24_000,
        cpu_cores: 16,
        clock_mhz: 4000,
        tools: vec!["browser".into(), "code_exec".into(), "filesystem".into()],
        ..Default::default()
    });

    let laptop = world.add_node(MockNodeConfig {
        hostname: "laptop".into(),
        device_type: DeviceType::Laptop,
        ram_mb: 16_000,
        vram_mb: 0,
        cpu_cores: 8,
        clock_mhz: 3200,
        tools: vec!["filesystem".into()],
        ..Default::default()
    });

    let phone = world.add_phone(MockPhoneConfig {
        hostname: "iphone".into(),
        ram_mb: 6_000,
        battery_percent: 75,
        npu_type: "Apple Neural Engine".into(),
        tools: vec!["mic".into(), "camera".into()],
        connection_type: ConnectionType::WiFi,
    });

    // Configure demand: coding 60%, chat 30%, image 10%
    world.set_demand(vec![
        ("coding", 0.6),
        ("chat", 0.3),
        ("image", 0.1),
    ]);

    // Run optimizer cycle
    let plan = world.run_optimizer();

    // Verify: models fit within node RAM/VRAM
    for placement in &plan.placements {
        for &node_id in &placement.assigned_nodes {
            let node = world.get_node(node_id);
            let total_ram_on_node: u64 = plan.placements.iter()
                .filter(|p| p.assigned_nodes.contains(&node_id))
                .map(|p| /* model RAM */)
                .sum();
            assert!(total_ram_on_node <= node.ram_mb);
        }
    }

    // Verify: phone constraints respected (max 3B params, battery > 20%)
    for placement in &plan.placements {
        if placement.assigned_nodes.contains(&phone) {
            // Phone should only get small models
            assert!(/* model params <= 3B */);
        }
    }

    // Verify: Pareto improvement (each node benefits vs running alone)
    // ... (check utility per node)

    // Verify: observability events emitted
    let events = world.event_log.lock();
    assert!(events.iter().any(|e| matches!(e, TestEvent::PlanCreated { .. })));

    // Verify: plan executor diffs correctly
    let diff = world.compute_plan_diff(&plan);
    assert!(diff.models_to_load.len() > 0 || diff.models_to_unload.len() == 0);
}
```

### Scenario 5: Crash Recovery

```rust
#[test]
fn test_workflow_crash_recovery() {
    let mut world = TestWorld::new();
    let node = world.add_node(desktop_config());

    // Start workflow, let it checkpoint after step 2
    let plan = AgentPlan { steps: vec![step1, step2, step3, step4] };
    let workflow_id = world.submit_workflow(plan);

    // Execute steps 1 and 2
    world.advance_time(Duration::from_secs(2));
    // Trigger checkpoint
    world.agent_orchestrator.checkpoint(workflow_id);

    // Simulate crash: drop orchestrator
    drop(world.agent_orchestrator);

    // Create new orchestrator, load checkpoint
    let mut new_orchestrator = WorkflowOrchestrator::new(node, config);
    let checkpoint = world.persistence.load_checkpoint(workflow_id).unwrap();
    new_orchestrator.resume_from_checkpoint(workflow_id, checkpoint);

    // Verify: resumes from step 3, not step 1
    let status = new_orchestrator.get_workflow_status(workflow_id);
    assert_eq!(status.completed_steps, 2);
    assert_eq!(status.status, WorkflowStatus::Running);
}
```

## Correctness Properties

### Property 1: Flow Completeness
Every integration test SHALL exercise at least 2 module boundaries (cross-module interaction).

### Property 2: Determinism
Tests SHALL produce the same result on every run (no real timers, no real randomness in test paths).

### Property 3: Independence
Each test SHALL create its own TestWorld — no shared state between tests.

### Property 4: Performance Bound
All integration tests combined SHALL complete within 30 seconds.

## Testing Strategy

These ARE the tests — they test the integration between modules. They use:
- `#[test]` attribute (standard Rust tests)
- `TestWorld` harness for setup
- Direct assertions on state after operations
- No proptest (these are scenario-based, not property-based)

## File Structure

```
src/resonantos-vnext/src-tauri/src/
├── integration_tests/
│   ├── mod.rs              # Module declaration, TestWorld, helpers
│   ├── harness.rs          # TestWorld implementation
│   ├── mock_transport.rs   # MockTransportManager
│   ├── mock_node.rs        # MockNodeConfig, MockPhoneConfig
│   ├── persistence.rs      # InMemoryPersistence
│   ├── test_pairing.rs     # Pairing → inference flow
│   ├── test_agent.rs       # Agent workflow flow
│   ├── test_transport.rs   # Transport failover flow
│   ├── test_optimizer.rs   # Optimizer cycle flow
│   ├── test_recovery.rs    # Crash recovery flow
│   ├── test_concurrent.rs  # Concurrency tests
│   └── test_errors.rs      # Error propagation tests
```
