// Distributed Agent Execution — Phase 15
//
// Enables agentic workloads (multi-step AI workflows with tool calls) to execute
// across multiple network nodes. Agent plans are decomposed into a DAG of steps,
// independent steps run in parallel on different nodes, each routed to the node
// with the required model AND tools.

pub mod cache;
pub mod checkpoint;
pub mod colocation;
pub mod dag;
pub mod executor;
pub mod integration;
pub mod orchestrator;
pub mod protocol;
pub mod router;
pub mod tools;
pub mod worker;

use serde::{Deserialize, Serialize};

/// Configuration for distributed agent execution across the mesh network.
///
/// Controls parallelism limits, timeouts, retry behavior, checkpointing frequency,
/// intermediate result size bounds, and optimizer co-location weighting.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DistributedAgentConfig {
    /// Maximum number of steps executing in parallel across all nodes.
    /// Default: 10
    pub max_parallel_steps: u32,

    /// Maximum total steps allowed in a single workflow DAG.
    /// Default: 50
    pub max_workflow_steps: u32,

    /// Timeout in milliseconds for a single step execution.
    /// Default: 30000 (30 seconds)
    pub step_timeout_ms: u64,

    /// Maximum retry attempts per step before declaring failure.
    /// Default: 2
    pub max_retries_per_step: u32,

    /// Interval in seconds between workflow checkpoint saves.
    /// Default: 300 (5 minutes)
    pub checkpoint_interval_secs: u64,

    /// Maximum size in megabytes for intermediate step results.
    /// Default: 100
    pub max_intermediate_result_mb: u64,

    /// Bonus weight applied to nodes that co-locate frequently-paired model+tool combinations.
    /// Default: 0.15
    pub colocation_bonus_weight: f64,

    /// Whether speculative execution is enabled (opt-in).
    /// When enabled, the executor may run alternative approaches in parallel and use
    /// whichever completes first or produces better quality.
    /// Default: false
    pub speculative_execution_enabled: bool,
}

impl Default for DistributedAgentConfig {
    fn default() -> Self {
        Self {
            max_parallel_steps: 10,
            max_workflow_steps: 50,
            step_timeout_ms: 30_000,
            max_retries_per_step: 2,
            checkpoint_interval_secs: 300,
            max_intermediate_result_mb: 100,
            colocation_bonus_weight: 0.15,
            speculative_execution_enabled: false,
        }
    }
}
