# Implementation Plan: Unified Resource Scheduler

## Overview

Extend the existing Phase 9A/9B solver at `src/resonantos-vnext/src-tauri/src/network/solver.rs` to handle agents alongside models as a unified scheduling problem. New logic lives in two new files (`solver_agents.rs` and `solver_contention.rs`) with minimal modifications to the existing `solver.rs`. The extension is strictly additive — when `agent_catalog` is empty, the solver produces byte-for-byte identical output.

All Rust code lives in `src/resonantos-vnext/src-tauri/src/network/`.

## Tasks

- [x] 1. Define agent data models and extend existing structures
  - [x] 1.1 Create `solver_agents.rs` with agent data structures
    - Create `src/resonantos-vnext/src-tauri/src/network/solver_agents.rs`
    - Define `AgentId` type alias, `AgentEntry` struct (agent_id, agent_name, version, required_model, tool_declarations, runtime_requirements, download_sources, checksum_sha256)
    - Define `AgentRequirements` struct (ram_mb, cpu_cores, disk_mb)
    - Define `SelectedAgent` struct (agent_id, instance_count, utility_score, required_model)
    - Define `AgentSelectionResult` struct (selected, total_ram_allocated_mb, total_cpu_cores_allocated)
    - Define `AgentPlacement` struct (agent_id, instance_id, assigned_node, required_model_instance_id, estimated_throughput, resource_allocation)
    - Define `AgentWorkloadDemand` struct with Default impl (agent_shares HashMap, total_agent_requests, time_window_hours)
    - Define `CoSelectionAction` enum (ModelAdded, AgentRejected)
    - Derive `Debug, Clone, Serialize, Deserialize` on all structs
    - _Requirements: 1.1, 1.3, 1.4, 2.1, 12.1, 12.2_

  - [x] 1.2 Create `solver_contention.rs` with contention data structures
    - Create `src/resonantos-vnext/src-tauri/src/network/solver_contention.rs`
    - Define `ContentionResult` struct (total_cost, per_node HashMap)
    - Define `NodeContentionDetail` struct (cpu_penalty, memory_penalty, queue_penalty, speed_penalty, latency_penalty, total)
    - Define `ContentionWeights` struct with Default impl (cpu=1.0, memory=1.5, queue=0.8, speed=1.2, latency=1.0)
    - Define `ResourceType` enum (Model, Agent)
    - Define `DownloadPriority` enum (Critical, High, Normal, Low) with Ord derives
    - Define `PendingDownload` struct (resource_type, resource_id, target_node, source, size_mb, priority, depends_on)
    - Define `SolverDiagnostic` struct (resource_type, resource_id, reason)
    - _Requirements: 7.6, 4.1, 4.2, 12.3, 12.4_

  - [x] 1.3 Extend `SolverInputs` in `solver.rs` with optional agent fields
    - Add `agent_catalog: Vec<AgentEntry>` field (default empty Vec)
    - Add `agent_demand: AgentWorkloadDemand` field (default)
    - Ensure existing tests compile without changes (fields default to empty)
    - _Requirements: 10.3, 10.4_

  - [x] 1.4 Extend `SolverConfig` in `solver.rs` with agent-related thresholds
    - Add `max_instances_per_agent: u32` (default: 8)
    - Add `cpu_headroom_percent: f64` (default: 0.80)
    - Add `ram_headroom_percent: f64` (default: 0.10)
    - Add `contention_weights: ContentionWeights` (default)
    - Add `speed_ratio_threshold: f64` (default: 3.0)
    - Add `max_queue_depth_threshold: u32` (default: 5)
    - Add `co_location_affinity_bonus: f64` (default: 0.4)
    - Add `time_budget_small_ms: u64` (default: 500)
    - Add `time_budget_large_ms: u64` (default: 2000)
    - Update `Default` impl to include new fields
    - _Requirements: 2.4, 3.5, 6.2, 7.6, 8.4, 14.1, 14.2_

  - [x] 1.5 Extend `PlacementPlan` and `UtilityScores` in `solver.rs`
    - Add `agent_placements: Vec<AgentPlacement>` to `PlacementPlan` (default empty)
    - Add `pending_downloads: Vec<PendingDownload>` to `PlacementPlan` (default empty)
    - Add `diagnostics: Vec<SolverDiagnostic>` to `PlacementPlan` (default empty)
    - Add `agent_utility: f64` to `UtilityScores` (default 0.0)
    - Add `contention_cost: f64` to `UtilityScores` (default 0.0)
    - Add `unified_total: f64` to `UtilityScores` (default = total)
    - Ensure existing tests pass unchanged (new fields default to zero/empty)
    - _Requirements: 12.1, 12.2, 12.4, 5.1_

  - [x] 1.6 Register new modules in `mod.rs` or `solver.rs`
    - Add `pub mod solver_agents;` and `pub mod solver_contention;` declarations
    - Add necessary `use` imports between modules
    - Verify the project compiles with `cargo check`
    - _Requirements: 10.3_

- [x] 2. Implement agent selection (Phase A extension)
  - [x] 2.1 Implement `select_agents()` in `solver_agents.rs`
    - Filter agents whose `required_model` is in the model selection or catalog
    - Score each agent: `utility = demand_share × throughput_estimate`
    - Sort by utility descending
    - Greedy knapsack: add agents while combined resource footprint fits
    - Return `AgentSelectionResult` with selected agents
    - When `agent_catalog` is empty, return empty result immediately
    - _Requirements: 1.1, 1.2, 1.5, 2.1, 5.3, 10.2_

  - [x] 2.2 Implement `compute_agent_desired_instances()` in `solver_agents.rs`
    - Compute desired instance count per agent based on `agent_demand.agent_shares`
    - Scale instances with demand (higher share → more instances)
    - Cap at `config.max_instances_per_agent` (default: 8)
    - Minimum of 1 instance for any selected agent
    - Consider combined footprint (agent RAM + required model RAM) for capacity
    - _Requirements: 2.1, 2.2, 2.3, 2.4, 2.5_

  - [x] 2.3 Implement `enforce_co_selection()` in `solver_agents.rs`
    - For each selected agent, check if `required_model` is in model selection
    - If model missing but capacity allows: add model to selection, emit `CoSelectionAction::ModelAdded`
    - If model missing and no capacity: reject agent, emit `CoSelectionAction::AgentRejected`
    - When multiple agents share same model, count model RAM only once
    - _Requirements: 1.5, 11.1, 11.2, 11.3, 11.4_

  - [x]* 2.4 Write property tests for agent selection
    - **Property 2: Co-Selection Invariant** — every agent in output has its required_model in model placements
    - **Property 3: Instance Count Monotonicity** — higher demand never produces fewer instances
    - **Property 4: Instance Count Bounded** — instance count always in [1, max_instances_per_agent]
    - **Property 17: Shared Model Single-Counting** — shared model RAM counted once
    - **Validates: Requirements 1.5, 2.2, 2.3, 2.4, 11.1, 11.4**

- [x] 3. Implement agent placement (Phase B extension)
  - [x] 3.1 Implement `assign_agents()` in `solver_agents.rs`
    - Sort agent instances by RAM descending (largest first)
    - For each instance, filter candidate nodes:
      - `agent.tool_declarations ⊆ node.available_tools`
      - `remaining_ram >= agent.runtime_requirements.ram_mb`
      - `remaining_cpu_cores >= agent.runtime_requirements.cpu_cores`
      - Node passes battery/thermal constraints
    - Score candidates with co-location affinity bonus (+0.4 if required model on same node)
    - Place on best-scoring node, update remaining capacity
    - Skip instance if no node fits (capacity exhausted)
    - _Requirements: 3.1, 3.2, 3.4, 3.5, 3.6, 9.1, 9.3, 9.4_

  - [x] 3.2 Implement tool availability validation
    - Check `agent.tool_declarations ⊆ node.available_tools` for each candidate node
    - If no node has all required tools, reject agent and emit `SolverDiagnostic`
    - Use existing `NodeCapabilities.available_tools` structure
    - _Requirements: 13.1, 13.2, 13.4_

  - [x] 3.3 Implement model proximity constraint checking
    - Verify required model is on same node OR on a node with latency < `pipeline_parallel_max_latency_ms`
    - Prefer co-located nodes (same node as required model)
    - Apply latency bonus (+0.2) for low-latency peer nodes
    - _Requirements: 3.3, 3.6_

  - [x] 3.4 Implement priority-based placement ordering
    - Process resources in priority order: active inference (1) > agent steps (2) > background (3) > speculative (4)
    - Never evict higher-priority placements for lower-priority ones
    - Reserve `ram_headroom_percent` (10%) on every node for OS
    - _Requirements: 8.1, 8.2, 8.3, 8.4, 8.5_

  - [x] 3.5 Implement download plan generation
    - For agents placed on nodes without the runtime: emit `PendingDownload` with `ResourceType::Agent`
    - For agents whose required model is also missing: emit model download with higher priority
    - Set `depends_on` so model downloads complete before agent downloads
    - Include agent downloads in bandwidth budgeting alongside model downloads
    - _Requirements: 4.1, 4.2, 4.3, 4.4, 4.5, 12.3, 12.5_

  - [x]* 3.6 Write property tests for agent placement
    - **Property 5: Placement Capacity Invariant** — RAM and CPU never exceed node limits
    - **Property 6: Tool Subset Constraint** — agent tools ⊆ node available_tools
    - **Property 7: Model Proximity Constraint** — required model on same or low-latency node
    - **Property 8: Co-Location Preference** — agents placed on model's node when feasible
    - **Property 9: Download Plan Correctness** — downloads emitted for missing resources with correct dependencies
    - **Property 14: Priority Invariant** — higher-priority placements never evicted for lower
    - **Property 15: Node Eligibility Constraints** — battery and thermal constraints respected
    - **Validates: Requirements 3.1–3.6, 4.1, 4.4, 8.1–8.5, 9.3, 9.4, 13.1**

- [x] 4. Checkpoint - Verify Phase A and Phase B
  - Ensure all tests pass, ask the user if questions arise.

- [x] 5. Implement contention computation and unified objective
  - [x] 5.1 Implement `compute_contention()` in `solver_contention.rs`
    - For each node with co-located models and agents, compute:
      - `cpu_penalty = max(0, (agent_cpu_usage - 0.5 * total_cores) / total_cores)`
      - `memory_penalty = max(0, (total_ram_used - 0.8 * node_ram) / (0.1 * node_ram))`
      - `queue_penalty = max(0, (queue_depth - 5) / 10)`
      - `speed_penalty = if node_speed < 0.33 * max_speed { 1.0 } else { 0.0 }`
      - `latency_penalty = max(0, (latency - step_compute_time) / step_compute_time)`
    - Apply `ContentionWeights` to compute weighted total per node
    - Sum across all nodes for `C_total`
    - When no agents are placed, return `ContentionResult { total_cost: 0.0, per_node: {} }`
    - _Requirements: 7.1, 7.2, 7.3, 7.4, 7.5, 7.6_

  - [x] 5.2 Implement `compute_parallelism_factor()` in `solver_agents.rs`
    - Formula: `independent_steps / total_steps × (1 - avg_network_latency / step_compute_time) × min_node_speed / max_node_speed`
    - Clamp result to [0.0, 1.0]
    - When speed ratio exceeds `speed_ratio_threshold` (default 3.0), return 0.0 (no parallelization)
    - _Requirements: 5.5, 6.1, 6.2_

  - [x] 5.3 Implement `compute_agent_utility()` in `solver_agents.rs`
    - Formula: `U_agent = Σ agent_throughput_j × parallelism_factor_j`
    - Compute `agent_throughput_j` from historical demand data (steps/minute estimate)
    - When no agents placed, return 0.0
    - _Requirements: 5.3, 5.4, 5.5_

  - [x] 5.4 Implement `compute_unified_objective()` in `solver_contention.rs`
    - Formula: `U_total = U_model + U_agent - C_contention`
    - Store in `UtilityScores.unified_total`
    - When no agents: `unified_total = total` (backwards compatible)
    - _Requirements: 5.1, 5.2, 5.7_

  - [x] 5.5 Implement speed-matching logic for load distribution
    - Assign proportional load to nodes based on compute speed
    - Prefer fastest node for compute-heavy steps
    - Prefer least-loaded node for lightweight tool calls
    - Use node benchmark scores (tokens/second, operations/second) as inputs
    - _Requirements: 6.3, 6.4, 6.5_

  - [x]* 5.6 Write property tests for contention and objective
    - **Property 10: Unified Objective Formula** — `unified_total == total + agent_utility - contention_cost` (within epsilon)
    - **Property 11: Parallelism Factor Bounded** — result always in [0.0, 1.0]
    - **Property 12: Speed Ratio Rejection** — parallelism = 0 when speed ratio exceeds threshold
    - **Property 13: Contention Penalties Non-Negative** — all penalties >= 0.0, total = weighted sum
    - **Validates: Requirements 5.1, 5.5, 6.1, 6.2, 7.1–7.6**

- [x] 6. Integrate into existing `solve()` function
  - [x] 6.1 Extend `solve()` to call agent selection after model selection
    - After `select_models(inputs)`, call `select_agents(inputs, &model_selection)`
    - Call `enforce_co_selection(&mut model_selection, &mut agent_selection, inputs)`
    - Guard with early return when `agent_catalog` is empty (no-op path)
    - _Requirements: 1.1, 1.2, 10.1, 10.2_

  - [x] 6.2 Extend `solve()` to call agent placement after model placement
    - After `assign_models(...)`, call `assign_agents(...)` with remaining capacity
    - Call `compute_contention(...)` with both placement sets
    - Respect time budget: if elapsed > budget, return model-only plan
    - _Requirements: 3.1, 14.1, 14.2, 14.3_

  - [x] 6.3 Extend `solve()` to compute unified objective and assemble final plan
    - Call `compute_agent_utility(...)` and `compute_unified_objective(...)`
    - Populate `PlacementPlan.agent_placements`, `pending_downloads`, `diagnostics`
    - Set `UtilityScores.agent_utility`, `contention_cost`, `unified_total`
    - _Requirements: 5.1, 12.1, 12.4_

  - [x] 6.4 Implement anytime algorithm behavior
    - Compute time budget based on network size (500ms for ≤10 nodes, 2000ms for ≤50)
    - Check elapsed time before agent placement phase
    - If budget exceeded, return best-so-far plan (models placed, agents empty)
    - Report `solver_duration_ms` in output
    - _Requirements: 14.1, 14.2, 14.3, 14.4_

  - [x]* 6.5 Write property tests for integration and backwards compatibility
    - **Property 1: Backwards Compatibility** — empty agent_catalog produces identical output to pre-extension solver
    - **Property 16: Cascading Rejection** — rejected model cascades to reject dependent agents with diagnostic
    - **Property 18: Tool Unavailability Rejection** — agent with globally-unavailable tool is rejected with diagnostic
    - **Property 19: Anytime Validity** — solver always returns valid plan, all present placements satisfy constraints
    - **Validates: Requirements 1.2, 10.1, 10.5, 11.2, 13.2, 14.3**

- [x] 7. Checkpoint - Full integration verification
  - Ensure all tests pass, ask the user if questions arise.

- [x] 8. Device-agnostic constraints and edge cases
  - [x] 8.1 Verify no device-type branching in scheduling logic
    - Ensure no `if device_type == X` conditional logic in solver_agents.rs or solver_contention.rs
    - All device differences expressed as per-node constraints (battery, thermal, RAM, CPU, tools, bandwidth)
    - Use existing `NodeCapabilities` and `NodeState` structures only
    - _Requirements: 9.1, 9.2, 9.5_

  - [x] 8.2 Implement battery and thermal constraint enforcement
    - During node eligibility check: enforce `battery_percent >= battery_threshold OR is_charging`
    - Exclude nodes with thermal state Critical from new placements
    - Apply to both model and agent placement candidate filtering
    - _Requirements: 9.3, 9.4_

  - [x] 8.3 Implement re-solve trigger on tool status change
    - When a tool becomes unavailable on a node, mark affected agent placements as invalid
    - Emit diagnostic indicating which agents need relocation
    - Provide a `should_re_solve()` helper that callers can use to detect stale plans
    - _Requirements: 13.3_

  - [x]* 8.4 Write unit tests for device-agnostic constraints
    - Test battery constraint: node with low battery excluded from placement
    - Test thermal constraint: Critical node excluded
    - Test tool removal triggers re-solve indication
    - Test that no device-type enum is used in scheduling decisions
    - _Requirements: 9.1–9.5, 13.3_

- [x] 9. Performance validation and final integration tests
  - [x] 9.1 Implement performance benchmarks
    - Create benchmark test: 10 nodes, 50 models, 20 agents → solve < 500ms
    - Create benchmark test: 50 nodes, 200 models, 100 agents → solve < 2000ms
    - Use `std::time::Instant` to measure and assert timing constraints
    - Profile and optimize hot paths if benchmarks fail
    - _Requirements: 14.1, 14.2_

  - [x]* 9.2 Write integration tests for end-to-end scenarios
    - Test single node + single agent + single model placement
    - Test multi-node with agent requiring tools only on specific nodes
    - Test agent with max instances across heterogeneous network
    - Test backwards compatibility with golden-file input/output pair
    - Test serialization round-trip for all new structs via serde
    - _Requirements: 10.1, 10.5, 12.1, 12.2_

- [x] 10. Final checkpoint - Complete verification
  - Ensure all tests pass, ask the user if questions arise.

## Notes

- Tasks marked with `*` are optional and can be skipped for faster MVP
- The implementation language is Rust, matching the existing solver
- Property-based tests use the `proptest` crate
- New code is isolated in `solver_agents.rs` and `solver_contention.rs` to minimize merge conflicts with existing `solver.rs`
- Backwards compatibility is the highest priority — existing callers must see no behavioral change when `agent_catalog` is empty
- Performance targets: 500ms for 10 nodes, 2000ms for 50 nodes
