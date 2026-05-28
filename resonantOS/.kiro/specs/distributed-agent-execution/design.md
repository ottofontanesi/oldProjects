# Technical Design: Distributed Agent Execution (Phase 15)

## 1. Architecture Overview

```
┌─────────────────────────────────────────────────────────────────────────┐
│  Requesting Node (Orchestrator runs here)                                │
│                                                                           │
│  ┌───────────────────────────────────────────────────────────────────┐  │
│  │  Agent Orchestrator                                                │  │
│  │  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────────┐  │  │
│  │  │ DAG Builder  │  │ Step Router │  │ Parallel Executor       │  │  │
│  │  │ (decompose   │  │ (find best  │  │ (dispatch + collect     │  │  │
│  │  │  plan→graph) │  │  node/step) │  │  + retry + checkpoint)  │  │  │
│  │  └─────────────┘  └─────────────┘  └─────────────────────────┘  │  │
│  └───────────────────────────────────────────────────────────────────┘  │
│                            │ dispatch steps via Phase 10 transport       │
├────────────────────────────┼────────────────────────────────────────────┤
│                            ▼                                             │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐                 │
│  │ Desktop      │  │ Laptop       │  │ Phone        │                 │
│  │ model: 14B   │  │ model: 7B    │  │ model: 3B    │                 │
│  │ tools: [fs,  │  │ tools: [fs,  │  │ tools: [mic, │                 │
│  │  browser,    │  │  code_exec]  │  │  camera,gps, │                 │
│  │  code_exec]  │  │              │  │  speaker]    │                 │
│  │              │  │              │  │              │                 │
│  │ StepWorker   │  │ StepWorker   │  │ StepWorker   │                 │
│  └──────────────┘  └──────────────┘  └──────────────┘                 │
└─────────────────────────────────────────────────────────────────────────┘
```

The agent orchestrator decomposes a workflow into a DAG of steps. Independent steps
execute in parallel across different nodes. Each step is routed to the best node for
that operation (model + tool availability + trust + latency). Phone nodes participate
as step workers like any other node — receiving steps that need their tools (mic,
camera, GPS) or their loaded model.

Parallelization is the core value: if the network latency is good and tasks are fast,
multiple steps run simultaneously on desktop + laptop + phone, completing the workflow
in a fraction of the sequential time.

### 1.1 Module Decomposition

| Module | Responsibility | Crate Path |
|--------|---------------|------------|
| `orchestrator` | Workflow lifecycle, DAG management, progress reporting | `src-tauri/src/agents/orchestrator.rs` |
| `dag_builder` | Decompose agent plan into step DAG, identify parallelism | `src-tauri/src/agents/dag.rs` |
| `step_router` | Route each step to best node (model + tool + trust) | `src-tauri/src/agents/router.rs` |
| `parallel_executor` | Dispatch parallel steps, collect results, handle failures | `src-tauri/src/agents/executor.rs` |
| `step_worker` | Execute a single step on a worker node (inference + tool call) | `src-tauri/src/agents/worker.rs` |
| `result_cache` | Cache completed step results for retry efficiency | `src-tauri/src/agents/cache.rs` |
| `checkpoint` | Persist workflow progress for resume after crash | `src-tauri/src/agents/checkpoint.rs` |

## 2. Data Models

### 2.1 Tool Registry (extends NodeCapabilities)

```rust
/// Added to NodeCapabilities (Phase 9A extension point)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCapability {
    pub tool_id: String,
    pub tool_name: String,
    pub category: ToolCategory,
    pub resource_requirements: ToolResources,
    pub is_available: bool,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum ToolCategory {
    Filesystem,
    WebSearch,
    Browser,
    CodeExecution,
    GpuCompute,
    Database,
    Custom(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResources {
    pub cpu_cores: Option<u32>,
    pub ram_mb: Option<u64>,
    pub gpu_required: bool,
    pub network_required: bool,
}
```

### 2.2 Execution DAG

```rust
pub type StepId = uuid::Uuid;
pub type WorkflowId = uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionDag {
    pub workflow_id: WorkflowId,
    pub steps: HashMap<StepId, ExecutionStep>,
    pub edges: Vec<(StepId, StepId)>,  // (dependency, dependent)
    pub root_steps: Vec<StepId>,        // Steps with no dependencies (can start immediately)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionStep {
    pub step_id: StepId,
    pub description: String,
    pub required_model: Option<ModelId>,
    pub required_tools: Vec<String>,    // tool_ids
    pub sensitivity: PromptSensitivity,
    pub estimated_compute_ms: u64,
    pub input_dependencies: Vec<StepId>,
    pub status: StepStatus,
    pub assigned_node: Option<NodeId>,
    pub result: Option<StepResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum StepStatus {
    Pending,            // Waiting for dependencies
    Ready,              // Dependencies met, can be dispatched
    Dispatched,         // Sent to worker node
    Running,            // Worker confirmed execution started
    Completed,          // Result available
    Failed { reason: String, retries: u32 },
    Cancelled,          // Cancelled due to speculative execution or workflow abort
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepResult {
    pub step_id: StepId,
    pub output_data: Vec<u8>,           // Serialized step output
    pub output_size_bytes: u64,
    pub execution_node: NodeId,
    pub compute_time_ms: u64,
    pub model_used: Option<ModelId>,
    pub tools_used: Vec<String>,
}
```

### 2.3 Workflow State

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowState {
    pub workflow_id: WorkflowId,
    pub agent_id: String,
    pub requesting_node: NodeId,
    pub dag: ExecutionDag,
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub status: WorkflowStatus,
    pub parallel_steps_active: u32,
    pub total_steps: u32,
    pub completed_steps: u32,
    pub checkpoint: Option<WorkflowCheckpoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum WorkflowStatus {
    Running,
    Completed,
    Failed { failed_step: StepId, reason: String },
    Paused,             // Waiting for user input or resource availability
    Checkpointed,       // Saved for resume after restart
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowCheckpoint {
    pub checkpointed_at: chrono::DateTime<chrono::Utc>,
    pub completed_step_results: HashMap<StepId, StepResult>,
    pub pending_steps: Vec<StepId>,
}
```

### 2.4 Protocol Messages

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AgentStepMessage {
    // Orchestrator → Worker
    ExecuteStep {
        workflow_id: WorkflowId,
        step: ExecutionStep,
        input_data: HashMap<StepId, Vec<u8>>,  // Results from dependency steps
    },
    CancelStep {
        workflow_id: WorkflowId,
        step_id: StepId,
        reason: String,
    },
    
    // Worker → Orchestrator
    StepStarted {
        workflow_id: WorkflowId,
        step_id: StepId,
        node_id: NodeId,
    },
    StepCompleted {
        workflow_id: WorkflowId,
        step_id: StepId,
        result: StepResult,
    },
    StepFailed {
        workflow_id: WorkflowId,
        step_id: StepId,
        error: String,
        retryable: bool,
    },
    StepProgress {
        workflow_id: WorkflowId,
        step_id: StepId,
        progress_percent: f32,
        message: String,
    },
}
```

## 3. Algorithm Design

### 3.1 DAG Construction

```pseudocode
function build_execution_dag(agent_plan):
    dag = ExecutionDag::new()
    
    for step in agent_plan.steps:
        exec_step = ExecutionStep {
            step_id: uuid::new_v4(),
            description: step.description,
            required_model: step.model,
            required_tools: step.tools,
            sensitivity: classify_step_sensitivity(step, agent_plan.context),
            estimated_compute_ms: estimate_step_time(step),
            input_dependencies: step.depends_on,
            status: Pending,
        }
        dag.steps.insert(exec_step.step_id, exec_step)
    
    // Build edges from dependencies
    for step in dag.steps.values():
        for dep in step.input_dependencies:
            dag.edges.push((dep, step.step_id))
    
    // Identify root steps (no incoming edges)
    dag.root_steps = dag.steps.keys()
        .filter(|id| !dag.edges.iter().any(|(_, to)| to == id))
        .collect()
    
    // Propagate sensitivity: if step A is sensitive and B depends on A, B is at least as sensitive
    propagate_sensitivity(dag)
    
    return dag

function propagate_sensitivity(dag):
    // Topological sort, then propagate forward
    for step_id in topological_sort(dag):
        step = dag.steps[step_id]
        for (from, to) in dag.edges.where(|(f, _)| f == step_id):
            dependent = dag.steps[to]
            if step.sensitivity == Sensitive:
                dependent.sensitivity = Sensitive
```

### 3.2 Step Routing

```pseudocode
function route_step(step, network_state, optimizer_plan):
    // Find nodes that satisfy ALL requirements
    candidates = network_state.online_nodes().filter(|node| {
        // Model requirement
        if let Some(model) = step.required_model:
            if !optimizer_plan.has_model_on_node(model, node.id):
                return false
        
        // Tool requirements
        for tool_id in step.required_tools:
            if !node.capabilities.available_tools.iter().any(|t| t.tool_id == tool_id && t.is_available):
                return false
        
        // Trust requirement
        if step.sensitivity == Sensitive AND node.trust_tier < TrustTier::LocalOwned:
            return false
        
        true
    })
    
    if candidates.is_empty():
        // No single node has everything — try decomposition
        return try_decompose_step(step, network_state, optimizer_plan)
    
    // Score candidates (reuse Phase 9A scoring logic)
    scored = candidates.map(|node| {
        let mut score = 0.0
        score += (1.0 - node.utilization.queue_depth as f64 / 10.0) * 0.3  // Prefer less busy
        score += node.stability_score * 0.2                                   // Prefer stable
        score += data_locality_bonus(step, node) * 0.3                       // Prefer nodes with input data
        score += latency_score(requesting_node, node) * 0.2                  // Prefer low latency
        (node, score)
    })
    
    return scored.max_by_score().node_id

function try_decompose_step(step, network_state, optimizer_plan):
    // Step needs model on node A but tool on node B
    // Decompose into: inference on A → transfer result → tool on B
    
    model_node = find_node_with_model(step.required_model, network_state)
    tool_node = find_node_with_tools(step.required_tools, network_state)
    
    if model_node.is_none() OR tool_node.is_none():
        return Error(NoCapableNode { step: step.step_id, missing: ... })
    
    // Create two sub-steps with data transfer between them
    return DecomposedRoute {
        inference_step: (step.model_part, model_node),
        tool_step: (step.tool_part, tool_node),
        transfer_between: (model_node, tool_node),
    }
```

### 3.3 Parallel Execution

```pseudocode
function execute_workflow(dag, network_state):
    // Mark root steps as Ready
    for step_id in dag.root_steps:
        dag.steps[step_id].status = Ready
    
    // Main execution loop
    while !all_steps_terminal(dag):
        // Find all Ready steps
        ready_steps = dag.steps.values().filter(|s| s.status == Ready)
        
        // Dispatch all ready steps in parallel
        for step in ready_steps:
            node = route_step(step, network_state)
            step.assigned_node = Some(node)
            step.status = Dispatched
            
            // Gather input data from completed dependencies
            input_data = step.input_dependencies.map(|dep_id| {
                (dep_id, dag.steps[dep_id].result.unwrap().output_data)
            }).collect()
            
            // Send to worker
            transport.send(node, ExecuteStep {
                workflow_id: dag.workflow_id,
                step: step.clone(),
                input_data,
            }, priority: Normal, request_type: AgentStep)
        
        // Wait for any step to complete or fail
        match wait_for_step_event():
            StepCompleted { step_id, result } => {
                dag.steps[step_id].status = Completed
                dag.steps[step_id].result = Some(result)
                
                // Check if this unlocks new steps
                for (from, to) in dag.edges.where(|(f, _)| f == step_id):
                    dependent = dag.steps[to]
                    if all_dependencies_completed(dependent, dag):
                        dependent.status = Ready
                
                // Checkpoint if workflow is long-running
                if elapsed > 5.minutes():
                    save_checkpoint(dag)
            }
            
            StepFailed { step_id, error, retryable } => {
                if retryable AND dag.steps[step_id].retries < 2:
                    // Retry on alternative node
                    dag.steps[step_id].status = Ready
                    dag.steps[step_id].retries += 1
                    exclude_node(dag.steps[step_id].assigned_node)
                else:
                    dag.steps[step_id].status = Failed { reason: error }
                    // Cancel all steps that depend on this one
                    cancel_dependents(step_id, dag)
                    return WorkflowFailed { failed_step: step_id, reason: error }
            }
```

### 3.4 Worker Node Handler

```pseudocode
// Runs on each worker node — handles incoming step execution requests
function handle_execute_step(request: ExecuteStep):
    step = request.step
    
    // Verify we still have the required resources
    if let Some(model) = step.required_model:
        if !is_model_loaded(model):
            return StepFailed { retryable: true, error: "Model no longer loaded" }
    
    for tool_id in step.required_tools:
        if !is_tool_available(tool_id):
            return StepFailed { retryable: true, error: format!("Tool {} unavailable", tool_id) }
    
    // Notify orchestrator we're starting
    send(request.orchestrator, StepStarted { step_id: step.step_id, node_id: my_node_id })
    
    // Execute the step
    match execute_step_locally(step, request.input_data):
        Ok(output) => {
            result = StepResult {
                step_id: step.step_id,
                output_data: serialize(output),
                output_size_bytes: output.size(),
                execution_node: my_node_id,
                compute_time_ms: elapsed,
                model_used: step.required_model,
                tools_used: step.required_tools,
            }
            send(request.orchestrator, StepCompleted { result })
        }
        Err(e) => {
            send(request.orchestrator, StepFailed {
                step_id: step.step_id,
                error: e.to_string(),
                retryable: is_retryable(e),
            })
        }
```

## 4. Optimizer Extension Points

### 4.1 Co-location Signal

The optimizer receives a new demand signal: which (model, tool) pairs are frequently needed together.

```pseudocode
function compute_colocation_demand(agent_step_log):
    // From historical agent executions, count how often each (model, tool) pair co-occurs
    pairs = agent_step_log.group_by(|entry| (entry.model_used, entry.tools_used))
    
    // Top co-occurring pairs influence model placement
    // If model X is needed with tool Y 80% of the time, prefer placing X on nodes that have Y
    return pairs.sorted_by_frequency().take(20)
```

This feeds into Phase 9A's placement scoring as a bonus:
```
colocation_bonus(model, node) = 
    if node.has_tools(frequently_paired_tools(model)):
        0.15  // Bonus for co-location
    else:
        0.0
```

### 4.2 Transport Extension

Phase 10 transport gets a new request type:
```rust
pub enum RequestType {
    InferenceActivation,    // Existing
    InferenceRequest,       // Existing
    ModelTransfer,          // Existing
    AgentStepDispatch,      // NEW: orchestrator → worker step dispatch
    AgentStepResult,        // NEW: worker → orchestrator step result
    AgentStepData,          // NEW: inter-step data transfer
}
```

## 5. Integration with Existing Phases

| Phase | Extension Point | What Phase 15 Uses |
|-------|----------------|-------------------|
| 9A | `NodeCapabilities.available_tools` field | Tool registry per node |
| 9A | Placement scoring | Co-location bonus for model+tool pairs |
| 9B | Trust tier enforcement | Sensitive step routing |
| 10 | Transport message types | AgentStep dispatch/result/data |
| 10 | Path selection | Route agent steps like inference requests |
| 11 | Split inference | Agent steps that need split models |
| 13 | RL demand signal | Agent step patterns feed into demand estimation |

## 6. Configuration

```rust
pub struct DistributedAgentConfig {
    pub max_parallel_steps: u32,            // Default: 10
    pub max_workflow_steps: u32,            // Default: 50
    pub step_timeout_ms: u64,              // Default: 30000 (30s per step)
    pub max_retries_per_step: u32,         // Default: 2
    pub checkpoint_interval_secs: u64,     // Default: 300 (5 min)
    pub max_intermediate_result_mb: u64,   // Default: 100
    pub colocation_bonus_weight: f64,      // Default: 0.15
    pub speculative_execution_enabled: bool, // Default: false (opt-in)
}
```

## 7. Dependencies

- **Phase 9A (Local Network Optimizer)**: Node registry with tool capabilities, placement scoring
- **Phase 9B (Mesh Network Optimizer)**: Trust tier enforcement for sensitive steps
- **Phase 10 (Unified Mesh Transport)**: Step dispatch and result transfer
- **Phase 11 (Split Inference)**: Steps requiring split models
- **Phase 13 (RL-Optimizer Integration)**: Agent step demand feeds into optimizer
- **Existing Agent Framework (openClaw)**: Agent plan generation, tool definitions

## 8. Phone Nodes (Phase 16 Integration)

Phone nodes participate in distributed agent execution as step workers — no different
from any other node. They receive parallelized tool calls, execute them, return results.

Their existing constraints (battery, thermal, 3GB model limit, connectivity) are
already enforced by the Phase 9A phone constraint system. The step router treats
phones identically to desktops and laptops — it scores all nodes the same way.

No special phone routing logic. No phone-specific placement algorithm.
A phone is just a node with certain tools (mic, camera, GPS) and certain limits (RAM, battery).
The optimizer will be adapted later to account for agents + tools competing for resources.
