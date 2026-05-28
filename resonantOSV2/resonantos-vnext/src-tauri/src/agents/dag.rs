// Distributed Agent Execution — DAG builder and execution graph types
// Phase 15: Agent plan decomposition into step DAG
//
// Defines the core execution DAG data model: steps, edges, statuses, results.
// Includes DAG validation (cycle detection, edge validity) and sensitivity classification.
//
// Satisfies FR-2.1: Agent execution plan represented as a DAG of steps.
// Satisfies FR-2.2: Each step declares required_model, required_tools, input_dependencies,
//                   estimated_compute_time, sensitivity_level.

use std::collections::{HashMap, HashSet, VecDeque};

use serde::{Deserialize, Serialize};

use crate::network::registry::NodeId;

// ---------------------------------------------------------------------------
// Agent Plan types (input to the DAG builder)
// ---------------------------------------------------------------------------

/// An agent's execution plan: a list of steps with declared dependencies.
///
/// This is the input to `build_execution_dag`. It represents the high-level plan
/// produced by the agent planner before decomposition into a concrete execution DAG.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentPlan {
    /// Human-readable name or description of the overall plan.
    pub name: String,

    /// Ordered list of plan steps. Dependencies reference steps by index in this list.
    pub steps: Vec<AgentPlanStep>,
}

/// A single step in an agent plan.
///
/// Each step declares what it needs (model, tools) and which other steps it depends on
/// (by index into the parent `AgentPlan.steps` list).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentPlanStep {
    /// Human-readable description of what this step does.
    pub description: String,

    /// Model required for this step (None if step only needs tools).
    pub model: Option<ModelId>,

    /// Tool IDs required for this step.
    pub tools: Vec<String>,

    /// Indices of steps in the plan that this step depends on.
    /// These are indices into `AgentPlan.steps`.
    pub depends_on: Vec<usize>,

    /// Optional sensitivity hint. If `Some(Sensitive)`, the step is marked sensitive.
    /// If `None`, defaults to `NonSensitive`.
    pub sensitivity: Option<PromptSensitivity>,

    /// Estimated compute time in milliseconds. If 0 or not provided, a default is used.
    pub estimated_compute_ms: u64,
}

// ---------------------------------------------------------------------------
// DAG builder errors
// ---------------------------------------------------------------------------

/// Errors that can occur during DAG construction from an agent plan.
#[derive(Debug, Clone, PartialEq)]
pub enum DagBuildError {
    /// A step references a dependency index that is out of bounds.
    InvalidDependencyIndex {
        step_index: usize,
        dependency_index: usize,
        plan_length: usize,
    },
    /// A step depends on itself.
    SelfDependency { step_index: usize },
    /// The resulting DAG contains a cycle (dependencies form a loop).
    CycleDetected,
    /// The plan is empty (no steps).
    EmptyPlan,
}

impl std::fmt::Display for DagBuildError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DagBuildError::InvalidDependencyIndex {
                step_index,
                dependency_index,
                plan_length,
            } => {
                write!(
                    f,
                    "Step {} references dependency index {} but plan only has {} steps",
                    step_index, dependency_index, plan_length
                )
            }
            DagBuildError::SelfDependency { step_index } => {
                write!(f, "Step {} depends on itself", step_index)
            }
            DagBuildError::CycleDetected => write!(f, "Agent plan contains a dependency cycle"),
            DagBuildError::EmptyPlan => write!(f, "Agent plan has no steps"),
        }
    }
}

// ---------------------------------------------------------------------------
// DAG builder
// ---------------------------------------------------------------------------

/// Build an execution DAG from an agent plan.
///
/// Converts each `AgentPlanStep` into an `ExecutionStep`, builds directed edges
/// from declared `input_dependencies`, identifies root steps (no incoming edges),
/// and validates the resulting DAG is acyclic via topological sort.
///
/// # Errors
///
/// Returns `DagBuildError` if:
/// - The plan is empty
/// - A step references an out-of-bounds dependency index
/// - A step depends on itself
/// - The dependencies form a cycle
///
/// Satisfies FR-2.1: Agent execution plan represented as a DAG of steps.
/// Satisfies FR-2.3: Steps with no mutual dependencies can execute in parallel.
/// Satisfies FR-2.4: The orchestrator analyzes the DAG to identify maximum parallelism.
pub fn build_execution_dag(plan: &AgentPlan) -> Result<ExecutionDag, DagBuildError> {
    if plan.steps.is_empty() {
        return Err(DagBuildError::EmptyPlan);
    }

    let workflow_id = uuid::Uuid::new_v4();

    // Assign a StepId to each plan step (indexed by plan position).
    let step_ids: Vec<StepId> = (0..plan.steps.len())
        .map(|_| uuid::Uuid::new_v4())
        .collect();

    // Validate dependency indices and build ExecutionSteps.
    let mut steps: HashMap<StepId, ExecutionStep> = HashMap::with_capacity(plan.steps.len());

    for (idx, plan_step) in plan.steps.iter().enumerate() {
        // Validate dependencies
        for &dep_idx in &plan_step.depends_on {
            if dep_idx >= plan.steps.len() {
                return Err(DagBuildError::InvalidDependencyIndex {
                    step_index: idx,
                    dependency_index: dep_idx,
                    plan_length: plan.steps.len(),
                });
            }
            if dep_idx == idx {
                return Err(DagBuildError::SelfDependency { step_index: idx });
            }
        }

        // Map dependency indices to StepIds
        let input_dependencies: Vec<StepId> =
            plan_step.depends_on.iter().map(|&i| step_ids[i]).collect();

        let sensitivity = plan_step
            .sensitivity
            .clone()
            .unwrap_or(PromptSensitivity::NonSensitive);

        let estimated_compute_ms = if plan_step.estimated_compute_ms > 0 {
            plan_step.estimated_compute_ms
        } else {
            1000 // default 1 second estimate
        };

        let exec_step = ExecutionStep {
            step_id: step_ids[idx],
            description: plan_step.description.clone(),
            required_model: plan_step.model.clone(),
            required_tools: plan_step.tools.clone(),
            sensitivity,
            estimated_compute_ms,
            input_dependencies,
            status: StepStatus::Pending,
            assigned_node: None,
            result: None,
        };

        steps.insert(step_ids[idx], exec_step);
    }

    // Build edges from dependencies: (dependency_step_id, dependent_step_id)
    let mut edges: Vec<(StepId, StepId)> = Vec::new();
    for (idx, plan_step) in plan.steps.iter().enumerate() {
        for &dep_idx in &plan_step.depends_on {
            edges.push((step_ids[dep_idx], step_ids[idx]));
        }
    }

    // Identify root steps (no incoming edges)
    let dependents: HashSet<StepId> = edges.iter().map(|&(_, to)| to).collect();
    let root_steps: Vec<StepId> = step_ids
        .iter()
        .filter(|id| !dependents.contains(id))
        .copied()
        .collect();

    let mut dag = ExecutionDag {
        workflow_id,
        steps,
        edges,
        root_steps,
    };

    // Validate: ensure no cycles by running topological sort
    if dag.topological_sort().is_none() {
        return Err(DagBuildError::CycleDetected);
    }

    // Propagate sensitivity forward: if A is sensitive and B depends on A, B becomes sensitive.
    // Satisfies FR-6.1, FR-6.5 (Privacy classification propagation).
    dag.propagate_sensitivity();

    Ok(dag)
}

/// Model identifier (consistent with the rest of the codebase).
pub type ModelId = String;

/// Unique identifier for a step within an execution DAG.
pub type StepId = uuid::Uuid;

/// Unique identifier for a workflow (one DAG execution instance).
pub type WorkflowId = uuid::Uuid;

/// Sensitivity classification for a step's prompt and data.
///
/// Determines trust-tier routing constraints. Sensitive steps execute only on
/// tier-3 (local-owned) nodes. NonSensitive steps can execute on any trusted node.
///
/// Satisfies FR-6.1: Each step has a sensitivity level.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PromptSensitivity {
    /// Step involves private/personal data — restricted to tier-3 nodes.
    Sensitive,
    /// Step involves non-private data — can execute on any trusted node.
    NonSensitive,
}

/// The execution DAG: a directed acyclic graph of steps forming a workflow.
///
/// Steps are connected by edges representing data dependencies. Root steps have
/// no incoming edges and can begin execution immediately.
///
/// Satisfies FR-2.1: Agent execution plan as a DAG.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionDag {
    /// Unique identifier for this workflow instance.
    pub workflow_id: WorkflowId,

    /// All steps in the DAG, keyed by step ID.
    pub steps: HashMap<StepId, ExecutionStep>,

    /// Directed edges: (dependency, dependent). The dependent step cannot start
    /// until the dependency step completes.
    pub edges: Vec<(StepId, StepId)>,

    /// Steps with no incoming edges — can start immediately.
    pub root_steps: Vec<StepId>,
}

/// A single execution step within the DAG.
///
/// Satisfies FR-2.2: Step declares required_model, required_tools, input_dependencies,
/// estimated_compute_time, sensitivity_level.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionStep {
    /// Unique identifier for this step.
    pub step_id: StepId,

    /// Human-readable description of what this step does.
    pub description: String,

    /// Model required for this step (None if step only needs tools).
    pub required_model: Option<ModelId>,

    /// Tool IDs required for this step.
    pub required_tools: Vec<String>,

    /// Privacy/trust sensitivity classification.
    pub sensitivity: PromptSensitivity,

    /// Estimated compute time in milliseconds (used for scheduling).
    pub estimated_compute_ms: u64,

    /// Steps whose output this step depends on.
    pub input_dependencies: Vec<StepId>,

    /// Current execution status of this step.
    pub status: StepStatus,

    /// Node this step has been assigned to (set during routing).
    pub assigned_node: Option<NodeId>,

    /// Result of execution (set when step completes).
    pub result: Option<StepResult>,
}

/// Execution status of a step.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum StepStatus {
    /// Waiting for dependencies to complete.
    Pending,
    /// All dependencies met — ready for dispatch.
    Ready,
    /// Sent to a worker node for execution.
    Dispatched,
    /// Worker confirmed execution has started.
    Running,
    /// Execution completed successfully — result available.
    Completed,
    /// Execution failed.
    Failed {
        /// Human-readable failure reason.
        reason: String,
        /// Number of retry attempts made.
        retries: u32,
    },
    /// Cancelled (speculative execution or workflow abort).
    Cancelled,
}

/// Result produced by a completed step.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepResult {
    /// Step that produced this result.
    pub step_id: StepId,

    /// Serialized output data from the step.
    pub output_data: Vec<u8>,

    /// Size of the output in bytes.
    pub output_size_bytes: u64,

    /// Node that executed this step.
    pub execution_node: NodeId,

    /// Actual compute time in milliseconds.
    pub compute_time_ms: u64,

    /// Model that was used (if any).
    pub model_used: Option<ModelId>,

    /// Tools that were actually invoked during execution.
    pub tools_used: Vec<String>,
}

/// Errors that can occur during DAG validation.
#[derive(Debug, Clone, PartialEq)]
pub enum DagValidationError {
    /// The DAG contains a cycle (not acyclic).
    CycleDetected,
    /// An edge references a step ID that doesn't exist in the steps map.
    InvalidEdge {
        from: StepId,
        to: StepId,
        missing: StepId,
    },
}

impl std::fmt::Display for DagValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DagValidationError::CycleDetected => write!(f, "DAG contains a cycle"),
            DagValidationError::InvalidEdge { from, to, missing } => {
                write!(
                    f,
                    "Edge ({} -> {}) references non-existent step {}",
                    from, to, missing
                )
            }
        }
    }
}

impl ExecutionDag {
    /// Validate the DAG structure:
    /// 1. All edges reference valid step IDs (both source and target exist in `steps`).
    /// 2. No cycles exist (topological sort succeeds).
    ///
    /// Returns `Ok(())` if valid, or the first validation error found.
    pub fn validate(&self) -> Result<(), DagValidationError> {
        // Check all edges reference valid step IDs
        for &(from, to) in &self.edges {
            if !self.steps.contains_key(&from) {
                return Err(DagValidationError::InvalidEdge {
                    from,
                    to,
                    missing: from,
                });
            }
            if !self.steps.contains_key(&to) {
                return Err(DagValidationError::InvalidEdge {
                    from,
                    to,
                    missing: to,
                });
            }
        }

        // Check for cycles using Kahn's algorithm (topological sort)
        self.topological_sort()
            .map(|_| ())
            .ok_or(DagValidationError::CycleDetected)
    }

    /// Perform a topological sort of the DAG using Kahn's algorithm.
    ///
    /// Returns `Some(ordered_step_ids)` if the DAG is acyclic,
    /// or `None` if a cycle is detected.
    pub fn topological_sort(&self) -> Option<Vec<StepId>> {
        let step_ids: Vec<StepId> = self.steps.keys().copied().collect();

        // Build in-degree map
        let mut in_degree: HashMap<StepId, usize> = step_ids.iter().map(|&id| (id, 0)).collect();
        let mut adjacency: HashMap<StepId, Vec<StepId>> =
            step_ids.iter().map(|&id| (id, Vec::new())).collect();

        for &(from, to) in &self.edges {
            if let Some(deg) = in_degree.get_mut(&to) {
                *deg += 1;
            }
            if let Some(adj) = adjacency.get_mut(&from) {
                adj.push(to);
            }
        }

        // Start with nodes that have in-degree 0
        let mut queue: VecDeque<StepId> = in_degree
            .iter()
            .filter(|(_, &deg)| deg == 0)
            .map(|(&id, _)| id)
            .collect();

        let mut sorted = Vec::with_capacity(step_ids.len());

        while let Some(node) = queue.pop_front() {
            sorted.push(node);

            if let Some(neighbors) = adjacency.get(&node) {
                for &neighbor in neighbors {
                    if let Some(deg) = in_degree.get_mut(&neighbor) {
                        *deg -= 1;
                        if *deg == 0 {
                            queue.push_back(neighbor);
                        }
                    }
                }
            }
        }

        // If we processed all nodes, no cycle exists
        if sorted.len() == step_ids.len() {
            Some(sorted)
        } else {
            None
        }
    }

    /// Propagate sensitivity forward through the DAG in topological order.
    ///
    /// If step A is sensitive and step B depends on A (directly or transitively),
    /// B is marked as sensitive. This ensures that any step consuming output from
    /// a sensitive step is also treated as sensitive.
    ///
    /// Satisfies FR-6.1: Each step has a sensitivity level.
    /// Satisfies FR-6.5: Sensitivity propagates forward through the DAG.
    pub fn propagate_sensitivity(&mut self) {
        let sorted = match self.topological_sort() {
            Some(order) => order,
            None => return, // Cycle detected — skip propagation (DAG is invalid)
        };

        for step_id in sorted {
            // Check if this step is sensitive
            let is_sensitive = self
                .steps
                .get(&step_id)
                .map(|s| s.sensitivity == PromptSensitivity::Sensitive)
                .unwrap_or(false);

            if is_sensitive {
                // Find all direct dependents (edges where this step is the source)
                let dependents: Vec<StepId> = self
                    .edges
                    .iter()
                    .filter(|(from, _)| *from == step_id)
                    .map(|(_, to)| *to)
                    .collect();

                // Mark each dependent as sensitive
                for dep_id in dependents {
                    if let Some(dep_step) = self.steps.get_mut(&dep_id) {
                        dep_step.sensitivity = PromptSensitivity::Sensitive;
                    }
                }
            }
        }
    }

    /// Compute root steps: steps with no incoming edges.
    /// Updates `self.root_steps` in place.
    pub fn compute_root_steps(&mut self) {
        let dependents: HashSet<StepId> = self.edges.iter().map(|&(_, to)| to).collect();
        self.root_steps = self
            .steps
            .keys()
            .filter(|id| !dependents.contains(id))
            .copied()
            .collect();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_step(step_id: StepId) -> ExecutionStep {
        ExecutionStep {
            step_id,
            description: format!("Step {}", step_id),
            required_model: None,
            required_tools: Vec::new(),
            sensitivity: PromptSensitivity::NonSensitive,
            estimated_compute_ms: 1000,
            input_dependencies: Vec::new(),
            status: StepStatus::Pending,
            assigned_node: None,
            result: None,
        }
    }

    #[test]
    fn test_empty_dag_is_valid() {
        let dag = ExecutionDag {
            workflow_id: uuid::Uuid::new_v4(),
            steps: HashMap::new(),
            edges: Vec::new(),
            root_steps: Vec::new(),
        };
        assert!(dag.validate().is_ok());
    }

    #[test]
    fn test_single_step_dag_is_valid() {
        let id = uuid::Uuid::new_v4();
        let mut steps = HashMap::new();
        steps.insert(id, make_step(id));

        let dag = ExecutionDag {
            workflow_id: uuid::Uuid::new_v4(),
            steps,
            edges: Vec::new(),
            root_steps: vec![id],
        };
        assert!(dag.validate().is_ok());
    }

    #[test]
    fn test_linear_dag_is_valid() {
        let a = uuid::Uuid::new_v4();
        let b = uuid::Uuid::new_v4();
        let c = uuid::Uuid::new_v4();

        let mut steps = HashMap::new();
        steps.insert(a, make_step(a));
        steps.insert(b, make_step(b));
        steps.insert(c, make_step(c));

        let dag = ExecutionDag {
            workflow_id: uuid::Uuid::new_v4(),
            steps,
            edges: vec![(a, b), (b, c)],
            root_steps: vec![a],
        };
        assert!(dag.validate().is_ok());
    }

    #[test]
    fn test_diamond_dag_is_valid() {
        //   A
        //  / \
        // B   C
        //  \ /
        //   D
        let a = uuid::Uuid::new_v4();
        let b = uuid::Uuid::new_v4();
        let c = uuid::Uuid::new_v4();
        let d = uuid::Uuid::new_v4();

        let mut steps = HashMap::new();
        steps.insert(a, make_step(a));
        steps.insert(b, make_step(b));
        steps.insert(c, make_step(c));
        steps.insert(d, make_step(d));

        let dag = ExecutionDag {
            workflow_id: uuid::Uuid::new_v4(),
            steps,
            edges: vec![(a, b), (a, c), (b, d), (c, d)],
            root_steps: vec![a],
        };
        assert!(dag.validate().is_ok());
    }

    #[test]
    fn test_cycle_detected() {
        let a = uuid::Uuid::new_v4();
        let b = uuid::Uuid::new_v4();
        let c = uuid::Uuid::new_v4();

        let mut steps = HashMap::new();
        steps.insert(a, make_step(a));
        steps.insert(b, make_step(b));
        steps.insert(c, make_step(c));

        // A -> B -> C -> A (cycle)
        let dag = ExecutionDag {
            workflow_id: uuid::Uuid::new_v4(),
            steps,
            edges: vec![(a, b), (b, c), (c, a)],
            root_steps: Vec::new(),
        };
        assert_eq!(dag.validate(), Err(DagValidationError::CycleDetected));
    }

    #[test]
    fn test_self_loop_detected() {
        let a = uuid::Uuid::new_v4();

        let mut steps = HashMap::new();
        steps.insert(a, make_step(a));

        // A -> A (self-loop)
        let dag = ExecutionDag {
            workflow_id: uuid::Uuid::new_v4(),
            steps,
            edges: vec![(a, a)],
            root_steps: Vec::new(),
        };
        assert_eq!(dag.validate(), Err(DagValidationError::CycleDetected));
    }

    #[test]
    fn test_invalid_edge_from_missing() {
        let a = uuid::Uuid::new_v4();
        let b = uuid::Uuid::new_v4();
        let phantom = uuid::Uuid::new_v4();

        let mut steps = HashMap::new();
        steps.insert(a, make_step(a));
        steps.insert(b, make_step(b));

        // Edge from non-existent step
        let dag = ExecutionDag {
            workflow_id: uuid::Uuid::new_v4(),
            steps,
            edges: vec![(phantom, b)],
            root_steps: vec![a],
        };
        let err = dag.validate().unwrap_err();
        assert_eq!(
            err,
            DagValidationError::InvalidEdge {
                from: phantom,
                to: b,
                missing: phantom,
            }
        );
    }

    #[test]
    fn test_invalid_edge_to_missing() {
        let a = uuid::Uuid::new_v4();
        let b = uuid::Uuid::new_v4();
        let phantom = uuid::Uuid::new_v4();

        let mut steps = HashMap::new();
        steps.insert(a, make_step(a));
        steps.insert(b, make_step(b));

        // Edge to non-existent step
        let dag = ExecutionDag {
            workflow_id: uuid::Uuid::new_v4(),
            steps,
            edges: vec![(a, phantom)],
            root_steps: vec![a],
        };
        let err = dag.validate().unwrap_err();
        assert_eq!(
            err,
            DagValidationError::InvalidEdge {
                from: a,
                to: phantom,
                missing: phantom,
            }
        );
    }

    #[test]
    fn test_topological_sort_linear() {
        let a = uuid::Uuid::new_v4();
        let b = uuid::Uuid::new_v4();
        let c = uuid::Uuid::new_v4();

        let mut steps = HashMap::new();
        steps.insert(a, make_step(a));
        steps.insert(b, make_step(b));
        steps.insert(c, make_step(c));

        let dag = ExecutionDag {
            workflow_id: uuid::Uuid::new_v4(),
            steps,
            edges: vec![(a, b), (b, c)],
            root_steps: vec![a],
        };

        let sorted = dag.topological_sort().unwrap();
        assert_eq!(sorted.len(), 3);

        // a must come before b, b must come before c
        let pos_a = sorted.iter().position(|&x| x == a).unwrap();
        let pos_b = sorted.iter().position(|&x| x == b).unwrap();
        let pos_c = sorted.iter().position(|&x| x == c).unwrap();
        assert!(pos_a < pos_b);
        assert!(pos_b < pos_c);
    }

    #[test]
    fn test_topological_sort_cycle_returns_none() {
        let a = uuid::Uuid::new_v4();
        let b = uuid::Uuid::new_v4();

        let mut steps = HashMap::new();
        steps.insert(a, make_step(a));
        steps.insert(b, make_step(b));

        let dag = ExecutionDag {
            workflow_id: uuid::Uuid::new_v4(),
            steps,
            edges: vec![(a, b), (b, a)],
            root_steps: Vec::new(),
        };

        assert!(dag.topological_sort().is_none());
    }

    #[test]
    fn test_compute_root_steps() {
        let a = uuid::Uuid::new_v4();
        let b = uuid::Uuid::new_v4();
        let c = uuid::Uuid::new_v4();

        let mut steps = HashMap::new();
        steps.insert(a, make_step(a));
        steps.insert(b, make_step(b));
        steps.insert(c, make_step(c));

        let mut dag = ExecutionDag {
            workflow_id: uuid::Uuid::new_v4(),
            steps,
            edges: vec![(a, b), (a, c)],
            root_steps: Vec::new(),
        };

        dag.compute_root_steps();
        assert_eq!(dag.root_steps.len(), 1);
        assert!(dag.root_steps.contains(&a));
    }

    #[test]
    fn test_compute_root_steps_multiple_roots() {
        let a = uuid::Uuid::new_v4();
        let b = uuid::Uuid::new_v4();
        let c = uuid::Uuid::new_v4();

        let mut steps = HashMap::new();
        steps.insert(a, make_step(a));
        steps.insert(b, make_step(b));
        steps.insert(c, make_step(c));

        // a and b are both roots, c depends on both
        let mut dag = ExecutionDag {
            workflow_id: uuid::Uuid::new_v4(),
            steps,
            edges: vec![(a, c), (b, c)],
            root_steps: Vec::new(),
        };

        dag.compute_root_steps();
        assert_eq!(dag.root_steps.len(), 2);
        assert!(dag.root_steps.contains(&a));
        assert!(dag.root_steps.contains(&b));
    }

    #[test]
    fn test_step_status_serialization() {
        let statuses = vec![
            StepStatus::Pending,
            StepStatus::Ready,
            StepStatus::Dispatched,
            StepStatus::Running,
            StepStatus::Completed,
            StepStatus::Failed {
                reason: "timeout".to_string(),
                retries: 2,
            },
            StepStatus::Cancelled,
        ];

        for status in &statuses {
            let json = serde_json::to_string(status).unwrap();
            let deserialized: StepStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(&deserialized, status);
        }
    }

    #[test]
    fn test_prompt_sensitivity_serialization() {
        let sensitive = PromptSensitivity::Sensitive;
        let non_sensitive = PromptSensitivity::NonSensitive;

        let json_s = serde_json::to_string(&sensitive).unwrap();
        let json_ns = serde_json::to_string(&non_sensitive).unwrap();

        assert_eq!(
            serde_json::from_str::<PromptSensitivity>(&json_s).unwrap(),
            PromptSensitivity::Sensitive
        );
        assert_eq!(
            serde_json::from_str::<PromptSensitivity>(&json_ns).unwrap(),
            PromptSensitivity::NonSensitive
        );
    }

    #[test]
    fn test_step_result_serialization() {
        let result = StepResult {
            step_id: uuid::Uuid::new_v4(),
            output_data: vec![1, 2, 3, 4],
            output_size_bytes: 4,
            execution_node: uuid::Uuid::new_v4(),
            compute_time_ms: 500,
            model_used: Some("qwen2.5:14b".to_string()),
            tools_used: vec!["browser".to_string(), "filesystem".to_string()],
        };

        let json = serde_json::to_string(&result).unwrap();
        let deserialized: StepResult = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.step_id, result.step_id);
        assert_eq!(deserialized.output_data, vec![1, 2, 3, 4]);
        assert_eq!(deserialized.output_size_bytes, 4);
        assert_eq!(deserialized.compute_time_ms, 500);
        assert_eq!(deserialized.model_used, Some("qwen2.5:14b".to_string()));
        assert_eq!(deserialized.tools_used.len(), 2);
    }

    #[test]
    fn test_execution_step_with_dependencies() {
        let dep1 = uuid::Uuid::new_v4();
        let dep2 = uuid::Uuid::new_v4();
        let step_id = uuid::Uuid::new_v4();

        let step = ExecutionStep {
            step_id,
            description: "Synthesize results".to_string(),
            required_model: Some("llama3:70b".to_string()),
            required_tools: vec!["code_exec".to_string()],
            sensitivity: PromptSensitivity::Sensitive,
            estimated_compute_ms: 5000,
            input_dependencies: vec![dep1, dep2],
            status: StepStatus::Pending,
            assigned_node: None,
            result: None,
        };

        assert_eq!(step.input_dependencies.len(), 2);
        assert_eq!(step.sensitivity, PromptSensitivity::Sensitive);
        assert_eq!(step.required_model, Some("llama3:70b".to_string()));
    }

    // -----------------------------------------------------------------------
    // DAG Builder tests
    // -----------------------------------------------------------------------

    fn make_plan_step(description: &str, depends_on: Vec<usize>) -> AgentPlanStep {
        AgentPlanStep {
            description: description.to_string(),
            model: None,
            tools: Vec::new(),
            depends_on,
            sensitivity: None,
            estimated_compute_ms: 0,
        }
    }

    #[test]
    fn test_build_dag_empty_plan_returns_error() {
        let plan = AgentPlan {
            name: "empty".to_string(),
            steps: Vec::new(),
        };
        let err = build_execution_dag(&plan).unwrap_err();
        assert_eq!(err, DagBuildError::EmptyPlan);
    }

    #[test]
    fn test_build_dag_single_step() {
        let plan = AgentPlan {
            name: "single".to_string(),
            steps: vec![make_plan_step("Do something", vec![])],
        };

        let dag = build_execution_dag(&plan).unwrap();
        assert_eq!(dag.steps.len(), 1);
        assert_eq!(dag.edges.len(), 0);
        assert_eq!(dag.root_steps.len(), 1);

        let step = dag.steps.values().next().unwrap();
        assert_eq!(step.description, "Do something");
        assert_eq!(step.status, StepStatus::Pending);
        assert_eq!(step.sensitivity, PromptSensitivity::NonSensitive);
    }

    #[test]
    fn test_build_dag_linear_chain() {
        // A -> B -> C
        let plan = AgentPlan {
            name: "linear".to_string(),
            steps: vec![
                make_plan_step("Step A", vec![]),
                make_plan_step("Step B", vec![0]),
                make_plan_step("Step C", vec![1]),
            ],
        };

        let dag = build_execution_dag(&plan).unwrap();
        assert_eq!(dag.steps.len(), 3);
        assert_eq!(dag.edges.len(), 2);
        assert_eq!(dag.root_steps.len(), 1);

        // Topological sort should succeed
        let sorted = dag.topological_sort().unwrap();
        assert_eq!(sorted.len(), 3);

        // Root step should be the first in topological order
        assert!(dag.root_steps.contains(&sorted[0]));
    }

    #[test]
    fn test_build_dag_parallel_steps() {
        // A and B are independent roots, C depends on both
        let plan = AgentPlan {
            name: "parallel".to_string(),
            steps: vec![
                make_plan_step("Step A", vec![]),
                make_plan_step("Step B", vec![]),
                make_plan_step("Step C", vec![0, 1]),
            ],
        };

        let dag = build_execution_dag(&plan).unwrap();
        assert_eq!(dag.steps.len(), 3);
        assert_eq!(dag.edges.len(), 2);
        assert_eq!(dag.root_steps.len(), 2);

        // Topological sort: A and B before C
        let sorted = dag.topological_sort().unwrap();
        let step_c = dag
            .steps
            .values()
            .find(|s| s.description == "Step C")
            .unwrap();
        let pos_c = sorted.iter().position(|&id| id == step_c.step_id).unwrap();
        // C must be last (index 2)
        assert_eq!(pos_c, 2);
    }

    #[test]
    fn test_build_dag_diamond_shape() {
        //   A
        //  / \
        // B   C
        //  \ /
        //   D
        let plan = AgentPlan {
            name: "diamond".to_string(),
            steps: vec![
                make_plan_step("A", vec![]),
                make_plan_step("B", vec![0]),
                make_plan_step("C", vec![0]),
                make_plan_step("D", vec![1, 2]),
            ],
        };

        let dag = build_execution_dag(&plan).unwrap();
        assert_eq!(dag.steps.len(), 4);
        assert_eq!(dag.edges.len(), 4); // A->B, A->C, B->D, C->D
        assert_eq!(dag.root_steps.len(), 1);

        // Validate topological ordering
        let sorted = dag.topological_sort().unwrap();
        assert_eq!(sorted.len(), 4);

        // Find steps by description
        let find_step = |desc: &str| -> StepId {
            dag.steps
                .values()
                .find(|s| s.description == desc)
                .unwrap()
                .step_id
        };
        let a = find_step("A");
        let b = find_step("B");
        let c = find_step("C");
        let d = find_step("D");

        let pos = |id: StepId| sorted.iter().position(|&x| x == id).unwrap();
        assert!(pos(a) < pos(b));
        assert!(pos(a) < pos(c));
        assert!(pos(b) < pos(d));
        assert!(pos(c) < pos(d));
    }

    #[test]
    fn test_build_dag_invalid_dependency_index() {
        let plan = AgentPlan {
            name: "bad dep".to_string(),
            steps: vec![
                make_plan_step("A", vec![]),
                make_plan_step("B", vec![5]), // index 5 doesn't exist
            ],
        };

        let err = build_execution_dag(&plan).unwrap_err();
        assert_eq!(
            err,
            DagBuildError::InvalidDependencyIndex {
                step_index: 1,
                dependency_index: 5,
                plan_length: 2,
            }
        );
    }

    #[test]
    fn test_build_dag_self_dependency() {
        let plan = AgentPlan {
            name: "self dep".to_string(),
            steps: vec![make_plan_step("A", vec![0])],
        };

        let err = build_execution_dag(&plan).unwrap_err();
        assert_eq!(err, DagBuildError::SelfDependency { step_index: 0 });
    }

    #[test]
    fn test_build_dag_cycle_detected() {
        // A depends on B, B depends on A (mutual dependency via indices)
        // Since depends_on uses indices, a cycle requires: step 0 depends on step 1, step 1 depends on step 0
        let plan = AgentPlan {
            name: "cycle".to_string(),
            steps: vec![
                AgentPlanStep {
                    description: "A".to_string(),
                    model: None,
                    tools: Vec::new(),
                    depends_on: vec![1],
                    sensitivity: None,
                    estimated_compute_ms: 0,
                },
                AgentPlanStep {
                    description: "B".to_string(),
                    model: None,
                    tools: Vec::new(),
                    depends_on: vec![0],
                    sensitivity: None,
                    estimated_compute_ms: 0,
                },
            ],
        };

        let err = build_execution_dag(&plan).unwrap_err();
        assert_eq!(err, DagBuildError::CycleDetected);
    }

    #[test]
    fn test_build_dag_preserves_model_and_tools() {
        let plan = AgentPlan {
            name: "model+tools".to_string(),
            steps: vec![AgentPlanStep {
                description: "Inference step".to_string(),
                model: Some("qwen2.5:14b".to_string()),
                tools: vec!["browser".to_string(), "filesystem".to_string()],
                depends_on: vec![],
                sensitivity: Some(PromptSensitivity::Sensitive),
                estimated_compute_ms: 5000,
            }],
        };

        let dag = build_execution_dag(&plan).unwrap();
        let step = dag.steps.values().next().unwrap();

        assert_eq!(step.required_model, Some("qwen2.5:14b".to_string()));
        assert_eq!(step.required_tools, vec!["browser", "filesystem"]);
        assert_eq!(step.sensitivity, PromptSensitivity::Sensitive);
        assert_eq!(step.estimated_compute_ms, 5000);
    }

    #[test]
    fn test_build_dag_default_sensitivity_and_compute() {
        let plan = AgentPlan {
            name: "defaults".to_string(),
            steps: vec![AgentPlanStep {
                description: "Simple step".to_string(),
                model: None,
                tools: Vec::new(),
                depends_on: vec![],
                sensitivity: None,
                estimated_compute_ms: 0,
            }],
        };

        let dag = build_execution_dag(&plan).unwrap();
        let step = dag.steps.values().next().unwrap();

        assert_eq!(step.sensitivity, PromptSensitivity::NonSensitive);
        assert_eq!(step.estimated_compute_ms, 1000); // default
    }

    #[test]
    fn test_build_dag_edges_match_dependencies() {
        // A -> B, A -> C
        let plan = AgentPlan {
            name: "edges".to_string(),
            steps: vec![
                make_plan_step("A", vec![]),
                make_plan_step("B", vec![0]),
                make_plan_step("C", vec![0]),
            ],
        };

        let dag = build_execution_dag(&plan).unwrap();

        // Find step IDs by description
        let find_step = |desc: &str| -> StepId {
            dag.steps
                .values()
                .find(|s| s.description == desc)
                .unwrap()
                .step_id
        };
        let a = find_step("A");
        let b = find_step("B");
        let c = find_step("C");

        // Edges should be (A, B) and (A, C)
        assert!(dag.edges.contains(&(a, b)));
        assert!(dag.edges.contains(&(a, c)));
        assert_eq!(dag.edges.len(), 2);
    }

    #[test]
    fn test_build_dag_input_dependencies_are_step_ids() {
        // B depends on A
        let plan = AgentPlan {
            name: "deps as ids".to_string(),
            steps: vec![
                make_plan_step("A", vec![]),
                make_plan_step("B", vec![0]),
            ],
        };

        let dag = build_execution_dag(&plan).unwrap();

        let find_step = |desc: &str| -> &ExecutionStep {
            dag.steps
                .values()
                .find(|s| s.description == desc)
                .unwrap()
        };
        let a = find_step("A");
        let b = find_step("B");

        // B's input_dependencies should contain A's step_id
        assert_eq!(b.input_dependencies.len(), 1);
        assert_eq!(b.input_dependencies[0], a.step_id);
    }

    #[test]
    fn test_build_dag_workflow_id_is_unique() {
        let plan = AgentPlan {
            name: "unique".to_string(),
            steps: vec![make_plan_step("A", vec![])],
        };

        let dag1 = build_execution_dag(&plan).unwrap();
        let dag2 = build_execution_dag(&plan).unwrap();

        assert_ne!(dag1.workflow_id, dag2.workflow_id);
    }

    #[test]
    fn test_build_dag_multiple_dependencies() {
        // D depends on A, B, and C
        let plan = AgentPlan {
            name: "multi-dep".to_string(),
            steps: vec![
                make_plan_step("A", vec![]),
                make_plan_step("B", vec![]),
                make_plan_step("C", vec![]),
                make_plan_step("D", vec![0, 1, 2]),
            ],
        };

        let dag = build_execution_dag(&plan).unwrap();
        assert_eq!(dag.steps.len(), 4);
        assert_eq!(dag.edges.len(), 3);
        assert_eq!(dag.root_steps.len(), 3);

        let step_d = dag
            .steps
            .values()
            .find(|s| s.description == "D")
            .unwrap();
        assert_eq!(step_d.input_dependencies.len(), 3);
    }

    // -----------------------------------------------------------------------
    // Sensitivity propagation tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_propagate_sensitivity_direct_dependent() {
        // A (sensitive) -> B (non-sensitive)
        // After propagation, B should become sensitive.
        let plan = AgentPlan {
            name: "sensitivity direct".to_string(),
            steps: vec![
                AgentPlanStep {
                    description: "A".to_string(),
                    model: None,
                    tools: Vec::new(),
                    depends_on: vec![],
                    sensitivity: Some(PromptSensitivity::Sensitive),
                    estimated_compute_ms: 1000,
                },
                AgentPlanStep {
                    description: "B".to_string(),
                    model: None,
                    tools: Vec::new(),
                    depends_on: vec![0],
                    sensitivity: None, // defaults to NonSensitive
                    estimated_compute_ms: 1000,
                },
            ],
        };

        let dag = build_execution_dag(&plan).unwrap();

        let step_b = dag
            .steps
            .values()
            .find(|s| s.description == "B")
            .unwrap();
        assert_eq!(step_b.sensitivity, PromptSensitivity::Sensitive);
    }

    #[test]
    fn test_propagate_sensitivity_transitive() {
        // A (sensitive) -> B (non-sensitive) -> C (non-sensitive)
        // After propagation, both B and C should become sensitive.
        let plan = AgentPlan {
            name: "sensitivity transitive".to_string(),
            steps: vec![
                AgentPlanStep {
                    description: "A".to_string(),
                    model: None,
                    tools: Vec::new(),
                    depends_on: vec![],
                    sensitivity: Some(PromptSensitivity::Sensitive),
                    estimated_compute_ms: 1000,
                },
                AgentPlanStep {
                    description: "B".to_string(),
                    model: None,
                    tools: Vec::new(),
                    depends_on: vec![0],
                    sensitivity: None,
                    estimated_compute_ms: 1000,
                },
                AgentPlanStep {
                    description: "C".to_string(),
                    model: None,
                    tools: Vec::new(),
                    depends_on: vec![1],
                    sensitivity: None,
                    estimated_compute_ms: 1000,
                },
            ],
        };

        let dag = build_execution_dag(&plan).unwrap();

        let step_b = dag
            .steps
            .values()
            .find(|s| s.description == "B")
            .unwrap();
        let step_c = dag
            .steps
            .values()
            .find(|s| s.description == "C")
            .unwrap();

        assert_eq!(step_b.sensitivity, PromptSensitivity::Sensitive);
        assert_eq!(step_c.sensitivity, PromptSensitivity::Sensitive);
    }

    #[test]
    fn test_propagate_sensitivity_non_sensitive_does_not_affect_dependents() {
        // A (non-sensitive) -> B (non-sensitive)
        // B should remain non-sensitive.
        let plan = AgentPlan {
            name: "no propagation".to_string(),
            steps: vec![
                make_plan_step("A", vec![]),
                make_plan_step("B", vec![0]),
            ],
        };

        let dag = build_execution_dag(&plan).unwrap();

        let step_a = dag
            .steps
            .values()
            .find(|s| s.description == "A")
            .unwrap();
        let step_b = dag
            .steps
            .values()
            .find(|s| s.description == "B")
            .unwrap();

        assert_eq!(step_a.sensitivity, PromptSensitivity::NonSensitive);
        assert_eq!(step_b.sensitivity, PromptSensitivity::NonSensitive);
    }

    #[test]
    fn test_propagate_sensitivity_diamond_shape() {
        // A (sensitive) -> B, A -> C, B -> D, C -> D
        // All descendants of A should become sensitive.
        let plan = AgentPlan {
            name: "sensitivity diamond".to_string(),
            steps: vec![
                AgentPlanStep {
                    description: "A".to_string(),
                    model: None,
                    tools: Vec::new(),
                    depends_on: vec![],
                    sensitivity: Some(PromptSensitivity::Sensitive),
                    estimated_compute_ms: 1000,
                },
                AgentPlanStep {
                    description: "B".to_string(),
                    model: None,
                    tools: Vec::new(),
                    depends_on: vec![0],
                    sensitivity: None,
                    estimated_compute_ms: 1000,
                },
                AgentPlanStep {
                    description: "C".to_string(),
                    model: None,
                    tools: Vec::new(),
                    depends_on: vec![0],
                    sensitivity: None,
                    estimated_compute_ms: 1000,
                },
                AgentPlanStep {
                    description: "D".to_string(),
                    model: None,
                    tools: Vec::new(),
                    depends_on: vec![1, 2],
                    sensitivity: None,
                    estimated_compute_ms: 1000,
                },
            ],
        };

        let dag = build_execution_dag(&plan).unwrap();

        let step_b = dag
            .steps
            .values()
            .find(|s| s.description == "B")
            .unwrap();
        let step_c = dag
            .steps
            .values()
            .find(|s| s.description == "C")
            .unwrap();
        let step_d = dag
            .steps
            .values()
            .find(|s| s.description == "D")
            .unwrap();

        assert_eq!(step_b.sensitivity, PromptSensitivity::Sensitive);
        assert_eq!(step_c.sensitivity, PromptSensitivity::Sensitive);
        assert_eq!(step_d.sensitivity, PromptSensitivity::Sensitive);
    }

    #[test]
    fn test_propagate_sensitivity_partial_graph() {
        // A (non-sensitive) -> C, B (sensitive) -> C
        // C depends on both A and B. Since B is sensitive, C should become sensitive.
        // A should remain non-sensitive.
        let plan = AgentPlan {
            name: "partial sensitivity".to_string(),
            steps: vec![
                AgentPlanStep {
                    description: "A".to_string(),
                    model: None,
                    tools: Vec::new(),
                    depends_on: vec![],
                    sensitivity: None,
                    estimated_compute_ms: 1000,
                },
                AgentPlanStep {
                    description: "B".to_string(),
                    model: None,
                    tools: Vec::new(),
                    depends_on: vec![],
                    sensitivity: Some(PromptSensitivity::Sensitive),
                    estimated_compute_ms: 1000,
                },
                AgentPlanStep {
                    description: "C".to_string(),
                    model: None,
                    tools: Vec::new(),
                    depends_on: vec![0, 1],
                    sensitivity: None,
                    estimated_compute_ms: 1000,
                },
            ],
        };

        let dag = build_execution_dag(&plan).unwrap();

        let step_a = dag
            .steps
            .values()
            .find(|s| s.description == "A")
            .unwrap();
        let step_b = dag
            .steps
            .values()
            .find(|s| s.description == "B")
            .unwrap();
        let step_c = dag
            .steps
            .values()
            .find(|s| s.description == "C")
            .unwrap();

        assert_eq!(step_a.sensitivity, PromptSensitivity::NonSensitive);
        assert_eq!(step_b.sensitivity, PromptSensitivity::Sensitive);
        assert_eq!(step_c.sensitivity, PromptSensitivity::Sensitive);
    }
}


// ---------------------------------------------------------------------------
// Property-based tests for DAG validation
// ---------------------------------------------------------------------------
//
// **Validates: Requirements FR-2.1, Correctness Property 1**
//
// Property 1: DAG execution order — generated DAGs with random edges never have
// cycles after validation; topological sort always succeeds.

#[cfg(test)]
mod proptest_dag_validation {
    use super::*;
    use proptest::prelude::*;
    use std::collections::HashMap;

    /// Strategy to generate a valid DAG with `num_steps` steps and random forward edges.
    ///
    /// We generate edges only from lower-indexed steps to higher-indexed steps,
    /// which guarantees the generated DAG is acyclic by construction. This lets us
    /// test that `validate()` correctly accepts valid DAGs and `topological_sort()`
    /// always succeeds on them.
    fn arb_valid_dag(max_steps: usize) -> impl Strategy<Value = ExecutionDag> {
        // Generate between 1 and max_steps steps
        (1..=max_steps).prop_flat_map(|num_steps| {
            // For each pair (i, j) where i < j, randomly decide if edge (i -> j) exists.
            // This gives us a random DAG that is guaranteed acyclic.
            let num_possible_edges = num_steps * (num_steps.saturating_sub(1)) / 2;
            proptest::collection::vec(proptest::bool::ANY, num_possible_edges).prop_map(
                move |edge_bits| {
                    let step_ids: Vec<StepId> =
                        (0..num_steps).map(|_| uuid::Uuid::new_v4()).collect();

                    let mut steps: HashMap<StepId, ExecutionStep> = HashMap::new();
                    for &id in &step_ids {
                        steps.insert(
                            id,
                            ExecutionStep {
                                step_id: id,
                                description: format!("Step {}", id),
                                required_model: None,
                                required_tools: Vec::new(),
                                sensitivity: PromptSensitivity::NonSensitive,
                                estimated_compute_ms: 1000,
                                input_dependencies: Vec::new(),
                                status: StepStatus::Pending,
                                assigned_node: None,
                                result: None,
                            },
                        );
                    }

                    // Build edges from the random bits: iterate over all (i, j) pairs where i < j
                    let mut edges: Vec<(StepId, StepId)> = Vec::new();
                    let mut bit_idx = 0;
                    for i in 0..num_steps {
                        for j in (i + 1)..num_steps {
                            if edge_bits[bit_idx] {
                                edges.push((step_ids[i], step_ids[j]));
                            }
                            bit_idx += 1;
                        }
                    }

                    // Compute root steps (no incoming edges)
                    let dependents: HashSet<StepId> =
                        edges.iter().map(|&(_, to)| to).collect();
                    let root_steps: Vec<StepId> = step_ids
                        .iter()
                        .filter(|id| !dependents.contains(id))
                        .copied()
                        .collect();

                    ExecutionDag {
                        workflow_id: uuid::Uuid::new_v4(),
                        steps,
                        edges,
                        root_steps,
                    }
                },
            )
        })
    }

    /// Strategy to generate a DAG with arbitrary edges (may contain cycles).
    /// We generate random edges between any pair of steps (including backward edges).
    fn arb_arbitrary_dag(max_steps: usize) -> impl Strategy<Value = ExecutionDag> {
        (1..=max_steps).prop_flat_map(|num_steps| {
            // Generate a random number of edges (0 to num_steps * 2)
            let max_edges = num_steps * 2;
            proptest::collection::vec(
                (0..num_steps, 0..num_steps),
                0..=max_edges,
            )
            .prop_map(move |edge_indices| {
                let step_ids: Vec<StepId> =
                    (0..num_steps).map(|_| uuid::Uuid::new_v4()).collect();

                let mut steps: HashMap<StepId, ExecutionStep> = HashMap::new();
                for &id in &step_ids {
                    steps.insert(
                        id,
                        ExecutionStep {
                            step_id: id,
                            description: format!("Step {}", id),
                            required_model: None,
                            required_tools: Vec::new(),
                            sensitivity: PromptSensitivity::NonSensitive,
                            estimated_compute_ms: 1000,
                            input_dependencies: Vec::new(),
                            status: StepStatus::Pending,
                            assigned_node: None,
                            result: None,
                        },
                    );
                }

                // Build edges from random index pairs
                let edges: Vec<(StepId, StepId)> = edge_indices
                    .iter()
                    .map(|&(from_idx, to_idx)| (step_ids[from_idx], step_ids[to_idx]))
                    .collect();

                let dependents: HashSet<StepId> =
                    edges.iter().map(|&(_, to)| to).collect();
                let root_steps: Vec<StepId> = step_ids
                    .iter()
                    .filter(|id| !dependents.contains(id))
                    .copied()
                    .collect();

                ExecutionDag {
                    workflow_id: uuid::Uuid::new_v4(),
                    steps,
                    edges,
                    root_steps,
                }
            })
        })
    }

    proptest! {
        /// **Validates: Requirements FR-2.1, Correctness Property 1**
        ///
        /// Property: Any DAG generated with only forward edges (i < j) is always
        /// valid — validate() succeeds and topological_sort() returns a valid ordering.
        #[test]
        fn valid_dag_always_passes_validation(dag in arb_valid_dag(10)) {
            // Validation must succeed for acyclic DAGs
            prop_assert!(dag.validate().is_ok(),
                "Valid DAG (forward-only edges) should pass validation");

            // Topological sort must always succeed
            let sorted = dag.topological_sort();
            prop_assert!(sorted.is_some(),
                "Topological sort should succeed on a valid DAG");

            let sorted = sorted.unwrap();

            // Sorted order must contain all steps exactly once
            prop_assert_eq!(sorted.len(), dag.steps.len(),
                "Topological sort must include all steps");

            // Verify ordering: for every edge (a, b), a must appear before b
            for &(from, to) in &dag.edges {
                let pos_from = sorted.iter().position(|&id| id == from);
                let pos_to = sorted.iter().position(|&id| id == to);
                prop_assert!(pos_from.is_some() && pos_to.is_some(),
                    "Both endpoints of an edge must appear in sorted output");
                prop_assert!(pos_from.unwrap() < pos_to.unwrap(),
                    "Dependency must appear before dependent in topological order: {:?} -> {:?}",
                    from, to);
            }
        }

        /// **Validates: Requirements FR-2.1, Correctness Property 1**
        ///
        /// Property: For any DAG with arbitrary edges, if validate() succeeds then
        /// topological_sort() also succeeds (no cycles remain). If validate() fails
        /// with CycleDetected, then topological_sort() returns None.
        #[test]
        fn validation_and_topo_sort_agree_on_cycles(dag in arb_arbitrary_dag(8)) {
            match dag.validate() {
                Ok(()) => {
                    // If validation passes, topological sort must succeed
                    let sorted = dag.topological_sort();
                    prop_assert!(sorted.is_some(),
                        "If validate() passes, topological_sort() must succeed");

                    let sorted = sorted.unwrap();
                    prop_assert_eq!(sorted.len(), dag.steps.len(),
                        "Topological sort must include all steps");

                    // Verify ordering invariant
                    for &(from, to) in &dag.edges {
                        let pos_from = sorted.iter().position(|&id| id == from);
                        let pos_to = sorted.iter().position(|&id| id == to);
                        if let (Some(pf), Some(pt)) = (pos_from, pos_to) {
                            prop_assert!(pf < pt,
                                "Dependency must appear before dependent in topological order");
                        }
                    }
                }
                Err(DagValidationError::CycleDetected) => {
                    // If validation detects a cycle, topological sort must also fail
                    prop_assert!(dag.topological_sort().is_none(),
                        "If validate() detects a cycle, topological_sort() must return None");
                }
                Err(DagValidationError::InvalidEdge { .. }) => {
                    // Invalid edges are a different class of error — not about cycles.
                    // This shouldn't happen with our generator (all edges reference valid IDs).
                    // But if it does, that's fine — it's a valid rejection.
                }
            }
        }

        /// **Validates: Requirements FR-2.1, Correctness Property 1**
        ///
        /// Property: Building a DAG via build_execution_dag from a valid plan always
        /// produces a DAG where topological_sort succeeds (the builder rejects cycles).
        #[test]
        fn build_execution_dag_always_produces_valid_dag(
            num_steps in 1usize..8,
        ) {
            // Generate a random valid plan: each step can only depend on earlier steps
            let mut plan_steps: Vec<AgentPlanStep> = Vec::new();
            // Use a simple deterministic approach: step i can depend on any subset of [0..i)
            // For property testing, we just make each step depend on the previous one (linear chain)
            // to keep it simple while still exercising the builder.
            for i in 0..num_steps {
                let depends_on = if i > 0 { vec![i - 1] } else { vec![] };
                plan_steps.push(AgentPlanStep {
                    description: format!("Step {}", i),
                    model: None,
                    tools: Vec::new(),
                    depends_on,
                    sensitivity: None,
                    estimated_compute_ms: 1000,
                });
            }

            let plan = AgentPlan {
                name: "proptest plan".to_string(),
                steps: plan_steps,
            };

            let dag = build_execution_dag(&plan).unwrap();

            // The built DAG must always be valid
            prop_assert!(dag.validate().is_ok(),
                "DAG built from a valid plan must pass validation");

            // Topological sort must succeed
            let sorted = dag.topological_sort();
            prop_assert!(sorted.is_some(),
                "Topological sort must succeed on a DAG built from a valid plan");

            let sorted = sorted.unwrap();
            prop_assert_eq!(sorted.len(), num_steps,
                "Topological sort must include all steps");
        }

        /// **Validates: Requirements FR-2.1, Correctness Property 1**
        ///
        /// Property: For any valid DAG, every step in the topological sort order
        /// has ALL its dependencies appearing earlier in the sort. This directly
        /// validates that steps only execute after all input dependencies complete.
        #[test]
        fn topo_sort_respects_all_dependencies(dag in arb_valid_dag(10)) {
            let sorted = dag.topological_sort().unwrap();

            // Build a position map for O(1) lookup
            let position: HashMap<StepId, usize> = sorted
                .iter()
                .enumerate()
                .map(|(pos, &id)| (id, pos))
                .collect();

            // For every edge (dep, dependent), dep must have a lower position
            for &(dep, dependent) in &dag.edges {
                let dep_pos = position[&dep];
                let dependent_pos = position[&dependent];
                prop_assert!(dep_pos < dependent_pos,
                    "Dependency step must appear before dependent step in topological order. \
                     dep position: {}, dependent position: {}", dep_pos, dependent_pos);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Property-based tests for sensitivity propagation
// ---------------------------------------------------------------------------
//
// **Validates: Requirements FR-6.1, Correctness Property 9**
//
// Property 9: Privacy classification propagation — for any DAG, if step A is
// sensitive and B transitively depends on A, B is always classified sensitive.

#[cfg(test)]
mod proptest_sensitivity_propagation {
    use super::*;
    use proptest::prelude::*;
    use std::collections::{HashMap, HashSet, VecDeque};

    /// Strategy to generate a valid DAG with random sensitivity assignments.
    ///
    /// Generates forward-only edges (i < j) to guarantee acyclicity, then randomly
    /// marks a subset of steps as Sensitive. Sensitivity propagation is NOT applied
    /// yet — the test will call `propagate_sensitivity()` and verify the result.
    fn arb_dag_with_sensitivity(max_steps: usize) -> impl Strategy<Value = ExecutionDag> {
        (2..=max_steps).prop_flat_map(|num_steps| {
            let num_possible_edges = num_steps * (num_steps.saturating_sub(1)) / 2;
            // Generate edge presence bits and sensitivity bits for each step
            (
                proptest::collection::vec(proptest::bool::ANY, num_possible_edges),
                proptest::collection::vec(proptest::bool::ANY, num_steps),
            )
                .prop_map(move |(edge_bits, sensitivity_bits)| {
                    let step_ids: Vec<StepId> =
                        (0..num_steps).map(|_| uuid::Uuid::new_v4()).collect();

                    let mut steps: HashMap<StepId, ExecutionStep> = HashMap::new();
                    for (idx, &id) in step_ids.iter().enumerate() {
                        let sensitivity = if sensitivity_bits[idx] {
                            PromptSensitivity::Sensitive
                        } else {
                            PromptSensitivity::NonSensitive
                        };
                        steps.insert(
                            id,
                            ExecutionStep {
                                step_id: id,
                                description: format!("Step {}", idx),
                                required_model: None,
                                required_tools: Vec::new(),
                                sensitivity,
                                estimated_compute_ms: 1000,
                                input_dependencies: Vec::new(),
                                status: StepStatus::Pending,
                                assigned_node: None,
                                result: None,
                            },
                        );
                    }

                    // Build forward-only edges (guaranteed acyclic)
                    let mut edges: Vec<(StepId, StepId)> = Vec::new();
                    let mut bit_idx = 0;
                    for i in 0..num_steps {
                        for j in (i + 1)..num_steps {
                            if edge_bits[bit_idx] {
                                edges.push((step_ids[i], step_ids[j]));
                            }
                            bit_idx += 1;
                        }
                    }

                    // Compute root steps
                    let dependents: HashSet<StepId> =
                        edges.iter().map(|&(_, to)| to).collect();
                    let root_steps: Vec<StepId> = step_ids
                        .iter()
                        .filter(|id| !dependents.contains(id))
                        .copied()
                        .collect();

                    ExecutionDag {
                        workflow_id: uuid::Uuid::new_v4(),
                        steps,
                        edges,
                        root_steps,
                    }
                })
        })
    }

    /// Compute the set of all transitive dependents of a given step in the DAG.
    /// Uses BFS over the forward edges.
    fn transitive_dependents(dag: &ExecutionDag, start: StepId) -> HashSet<StepId> {
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();

        // Seed with direct dependents
        for &(from, to) in &dag.edges {
            if from == start {
                if visited.insert(to) {
                    queue.push_back(to);
                }
            }
        }

        // BFS forward
        while let Some(current) = queue.pop_front() {
            for &(from, to) in &dag.edges {
                if from == current && visited.insert(to) {
                    queue.push_back(to);
                }
            }
        }

        visited
    }

    proptest! {
        /// **Validates: Requirements FR-6.1, Correctness Property 9**
        ///
        /// Property: For any valid DAG, after calling propagate_sensitivity(), every
        /// step that transitively depends on a Sensitive step is also marked Sensitive.
        /// Sensitivity always propagates forward — it is never lost along dependency chains.
        #[test]
        fn sensitivity_propagates_to_all_transitive_dependents(
            mut dag in arb_dag_with_sensitivity(10)
        ) {
            // Record which steps were originally sensitive (before propagation)
            let originally_sensitive: HashSet<StepId> = dag
                .steps
                .iter()
                .filter(|(_, step)| step.sensitivity == PromptSensitivity::Sensitive)
                .map(|(&id, _)| id)
                .collect();

            // Apply sensitivity propagation
            dag.propagate_sensitivity();

            // For every originally-sensitive step, ALL transitive dependents must now be Sensitive
            for &sensitive_id in &originally_sensitive {
                let dependents = transitive_dependents(&dag, sensitive_id);
                for &dep_id in &dependents {
                    let dep_step = dag.steps.get(&dep_id).unwrap();
                    prop_assert!(
                        dep_step.sensitivity == PromptSensitivity::Sensitive,
                        "Step {:?} transitively depends on sensitive step {:?} but is not marked Sensitive",
                        dep_id,
                        sensitive_id
                    );
                }
            }
        }

        /// **Validates: Requirements FR-6.1, Correctness Property 9**
        ///
        /// Property: Propagation never downgrades — a step that was already Sensitive
        /// before propagation remains Sensitive after propagation.
        #[test]
        fn sensitivity_propagation_never_downgrades(
            mut dag in arb_dag_with_sensitivity(10)
        ) {
            // Record which steps were originally sensitive
            let originally_sensitive: HashSet<StepId> = dag
                .steps
                .iter()
                .filter(|(_, step)| step.sensitivity == PromptSensitivity::Sensitive)
                .map(|(&id, _)| id)
                .collect();

            // Apply sensitivity propagation
            dag.propagate_sensitivity();

            // Every originally-sensitive step must still be Sensitive
            for &id in &originally_sensitive {
                let step = dag.steps.get(&id).unwrap();
                prop_assert!(
                    step.sensitivity == PromptSensitivity::Sensitive,
                    "Step {:?} was originally Sensitive but was downgraded after propagation",
                    id
                );
            }
        }

        /// **Validates: Requirements FR-6.1, Correctness Property 9**
        ///
        /// Property: If no step in the DAG is sensitive, propagation does not mark
        /// any step as sensitive (no false positives).
        #[test]
        fn no_sensitive_steps_means_no_propagation(num_steps in 2usize..10) {
            // Build a DAG where all steps are NonSensitive
            let step_ids: Vec<StepId> = (0..num_steps).map(|_| uuid::Uuid::new_v4()).collect();
            let mut steps: HashMap<StepId, ExecutionStep> = HashMap::new();
            for (idx, &id) in step_ids.iter().enumerate() {
                steps.insert(
                    id,
                    ExecutionStep {
                        step_id: id,
                        description: format!("Step {}", idx),
                        required_model: None,
                        required_tools: Vec::new(),
                        sensitivity: PromptSensitivity::NonSensitive,
                        estimated_compute_ms: 1000,
                        input_dependencies: Vec::new(),
                        status: StepStatus::Pending,
                        assigned_node: None,
                        result: None,
                    },
                );
            }

            // Linear chain of edges
            let edges: Vec<(StepId, StepId)> = step_ids
                .windows(2)
                .map(|w| (w[0], w[1]))
                .collect();

            let root_steps = vec![step_ids[0]];

            let mut dag = ExecutionDag {
                workflow_id: uuid::Uuid::new_v4(),
                steps,
                edges,
                root_steps,
            };

            dag.propagate_sensitivity();

            // No step should become sensitive
            for step in dag.steps.values() {
                prop_assert!(
                    step.sensitivity == PromptSensitivity::NonSensitive,
                    "Step {:?} became Sensitive despite no sensitive sources in the DAG",
                    step.step_id
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Property-based tests for parallel independence
// ---------------------------------------------------------------------------
//
// **Validates: Requirements FR-2.3, Correctness Property 2**
//
// Property 2: Parallel independence — steps identified as parallelizable never
// share a dependency edge (direct or transitive).

#[cfg(test)]
mod proptest_parallel_independence {
    use super::*;
    use crate::agents::DistributedAgentConfig;
    use crate::agents::executor::ParallelExecutor;
    use proptest::prelude::*;
    use std::collections::{HashMap, HashSet, VecDeque};

    /// Strategy to generate a valid DAG suitable for parallel execution testing.
    ///
    /// Generates forward-only edges (i < j) to guarantee acyclicity. The DAG
    /// will have multiple root steps (potential parallelism) by controlling edge density.
    fn arb_dag_for_parallel(max_steps: usize) -> impl Strategy<Value = ExecutionDag> {
        (3..=max_steps).prop_flat_map(|num_steps| {
            let num_possible_edges = num_steps * (num_steps.saturating_sub(1)) / 2;
            proptest::collection::vec(proptest::bool::ANY, num_possible_edges).prop_map(
                move |edge_bits| {
                    let step_ids: Vec<StepId> =
                        (0..num_steps).map(|_| uuid::Uuid::new_v4()).collect();

                    let mut steps: HashMap<StepId, ExecutionStep> = HashMap::new();
                    for &id in &step_ids {
                        steps.insert(
                            id,
                            ExecutionStep {
                                step_id: id,
                                description: format!("Step {}", id),
                                required_model: None,
                                required_tools: Vec::new(),
                                sensitivity: PromptSensitivity::NonSensitive,
                                estimated_compute_ms: 1000,
                                input_dependencies: Vec::new(),
                                status: StepStatus::Pending,
                                assigned_node: None,
                                result: None,
                            },
                        );
                    }

                    // Build forward-only edges
                    let mut edges: Vec<(StepId, StepId)> = Vec::new();
                    let mut bit_idx = 0;
                    for i in 0..num_steps {
                        for j in (i + 1)..num_steps {
                            if edge_bits[bit_idx] {
                                edges.push((step_ids[i], step_ids[j]));
                            }
                            bit_idx += 1;
                        }
                    }

                    // Compute root steps
                    let dependents: HashSet<StepId> =
                        edges.iter().map(|&(_, to)| to).collect();
                    let root_steps: Vec<StepId> = step_ids
                        .iter()
                        .filter(|id| !dependents.contains(id))
                        .copied()
                        .collect();

                    ExecutionDag {
                        workflow_id: uuid::Uuid::new_v4(),
                        steps,
                        edges,
                        root_steps,
                    }
                },
            )
        })
    }

    /// Compute all transitive ancestors (dependencies) of a step via backward BFS.
    fn transitive_ancestors(dag: &ExecutionDag, step_id: StepId) -> HashSet<StepId> {
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();

        // Seed with direct dependencies (edges where `to == step_id`)
        for &(from, to) in &dag.edges {
            if to == step_id {
                if visited.insert(from) {
                    queue.push_back(from);
                }
            }
        }

        // BFS backward
        while let Some(current) = queue.pop_front() {
            for &(from, to) in &dag.edges {
                if to == current && visited.insert(from) {
                    queue.push_back(from);
                }
            }
        }

        visited
    }

    /// Compute all transitive dependents (successors) of a step via forward BFS.
    fn transitive_dependents(dag: &ExecutionDag, step_id: StepId) -> HashSet<StepId> {
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();

        for &(from, to) in &dag.edges {
            if from == step_id {
                if visited.insert(to) {
                    queue.push_back(to);
                }
            }
        }

        while let Some(current) = queue.pop_front() {
            for &(from, to) in &dag.edges {
                if from == current && visited.insert(to) {
                    queue.push_back(to);
                }
            }
        }

        visited
    }

    /// Check if two steps share any transitive dependency relationship.
    /// Returns true if step_a is a transitive ancestor/descendant of step_b.
    fn shares_transitive_dependency(
        dag: &ExecutionDag,
        step_a: StepId,
        step_b: StepId,
    ) -> bool {
        // Check if B is a transitive dependent of A
        let a_dependents = transitive_dependents(dag, step_a);
        if a_dependents.contains(&step_b) {
            return true;
        }

        // Check if A is a transitive dependent of B
        let b_dependents = transitive_dependents(dag, step_b);
        if b_dependents.contains(&step_a) {
            return true;
        }

        false
    }

    proptest! {
        /// **Validates: Requirements FR-2.3, Correctness Property 2**
        ///
        /// Property: Steps identified as parallelizable (returned by find_ready_steps
        /// at the same time) never share a direct or transitive dependency edge.
        /// Two steps in the same parallel batch are always independent.
        #[test]
        fn parallel_steps_have_no_mutual_dependencies(
            mut dag in arb_dag_for_parallel(10),
            max_parallel in 2u32..10,
        ) {
            let config = DistributedAgentConfig {
                max_parallel_steps: max_parallel,
                ..Default::default()
            };
            let mut executor = ParallelExecutor::new(config);

            // Initialize DAG — marks root steps as Ready
            executor.initialize_dag(&mut dag);

            // Get the set of steps that would execute in parallel
            let ready_steps = executor.find_ready_steps(&dag);

            // For every pair of ready steps, verify no transitive dependency exists
            for i in 0..ready_steps.len() {
                for j in (i + 1)..ready_steps.len() {
                    let step_a = ready_steps[i];
                    let step_b = ready_steps[j];

                    // Check no direct edge between them
                    let has_direct_edge = dag.edges.iter().any(|&(from, to)| {
                        (from == step_a && to == step_b) || (from == step_b && to == step_a)
                    });
                    prop_assert!(
                        !has_direct_edge,
                        "Parallel steps {:?} and {:?} share a direct dependency edge",
                        step_a,
                        step_b
                    );

                    // Check no transitive dependency
                    prop_assert!(
                        !shares_transitive_dependency(&dag, step_a, step_b),
                        "Parallel steps {:?} and {:?} share a transitive dependency",
                        step_a,
                        step_b
                    );
                }
            }
        }

        /// **Validates: Requirements FR-2.3, Correctness Property 2**
        ///
        /// Property: After completing some steps and unlocking dependents, the newly
        /// ready steps (next parallel batch) also have no mutual dependencies among
        /// themselves. This tests that parallel independence holds across multiple
        /// execution rounds, not just the initial batch.
        #[test]
        fn parallel_independence_holds_after_step_completion(
            mut dag in arb_dag_for_parallel(8),
        ) {
            let config = DistributedAgentConfig {
                max_parallel_steps: 10, // High limit so we see all ready steps
                ..Default::default()
            };
            let mut executor = ParallelExecutor::new(config);
            executor.initialize_dag(&mut dag);

            let node_id = uuid::Uuid::new_v4();

            // Simulate completing all initial ready steps, then check next batch
            let initial_ready = executor.find_ready_steps(&dag);

            // Mark all initial ready steps as dispatched, then completed
            for &step_id in &initial_ready {
                executor.mark_dispatched(&mut dag, step_id, node_id);
            }
            for &step_id in &initial_ready {
                let result = StepResult {
                    step_id,
                    output_data: vec![],
                    output_size_bytes: 0,
                    execution_node: node_id,
                    compute_time_ms: 100,
                    model_used: None,
                    tools_used: Vec::new(),
                };
                executor.handle_step_completed(&mut dag, step_id, result);
            }

            // Get the next batch of ready steps
            let next_ready = executor.find_ready_steps(&dag);

            // Verify no pair in the next batch shares a transitive dependency
            for i in 0..next_ready.len() {
                for j in (i + 1)..next_ready.len() {
                    let step_a = next_ready[i];
                    let step_b = next_ready[j];

                    prop_assert!(
                        !shares_transitive_dependency(&dag, step_a, step_b),
                        "After completing first batch, next parallel steps {:?} and {:?} share a transitive dependency",
                        step_a,
                        step_b
                    );
                }
            }
        }
    }
}
