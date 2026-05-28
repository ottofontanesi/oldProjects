// Intent citation: .kiro/specs/unified-resource-scheduler/design.md
// Agent data structures and selection logic for the unified resource scheduler.
// These types extend the existing solver to handle agent selection and placement.
//
// DEVICE-AGNOSTIC DESIGN VERIFICATION (Task 8.1):
// This module contains NO device-type branching (no `if device_type == X` conditionals).
// All scheduling decisions are based exclusively on per-node constraints:
//   - RAM capacity (NodeCapabilities.ram.total_mb)
//   - CPU cores (NodeCapabilities.cpu.cores)
//   - Tool availability (NodeCapabilities.available_tools)
//   - Battery state (PhoneInfo.battery_percent, PhoneInfo.is_charging)
//   - Thermal state (approximated via CPU utilization)
//   - Network bandwidth/latency (latency_to_peers)
// The same agent can be placed on Desktop, Laptop, Server, or Phone nodes
// as long as the node satisfies the agent's resource and tool constraints.

use super::catalog::{DownloadSource, ModelId};
use super::registry::{NodeId, NodeState};
use super::solver::{ModelPlacement, SelectionResult, SelectedModel, SolverConfig, SolverInputs};
use super::solver_contention::{DownloadPriority, PendingDownload, ResourceType, SolverDiagnostic};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// â”€â”€â”€ Agent Types â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

/// Unique agent identifier (e.g., "openClaw-v2.1").
pub type AgentId = String;

/// Agent catalog entry â€” analogous to ModelEntry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentEntry {
    pub agent_id: AgentId,
    pub agent_name: String,
    pub version: String,
    pub required_model: ModelId,
    pub tool_declarations: Vec<String>,
    pub runtime_requirements: AgentRequirements,
    pub download_sources: Vec<DownloadSource>,
    pub checksum_sha256: String,
}

/// Resource requirements for an agent runtime.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRequirements {
    pub ram_mb: u64,
    pub cpu_cores: u32,
    pub disk_mb: u64,
}

/// A selected agent with instance count (Phase A output).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelectedAgent {
    pub agent_id: AgentId,
    pub instance_count: u32,
    pub utility_score: f64,
    pub required_model: ModelId,
}

/// Result of Phase A agent selection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSelectionResult {
    pub selected: Vec<SelectedAgent>,
    pub total_ram_allocated_mb: u64,
    pub total_cpu_cores_allocated: u32,
}

/// A single agent placement decision (Phase B output).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentPlacement {
    pub agent_id: AgentId,
    pub instance_id: uuid::Uuid,
    pub assigned_node: NodeId,
    pub required_model_instance_id: uuid::Uuid,
    pub estimated_throughput: f64,
    pub resource_allocation: AgentRequirements,
}

/// Agent workload demand â€” analogous to WorkloadDemand for models.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentWorkloadDemand {
    pub agent_shares: HashMap<AgentId, f64>,
    pub total_agent_requests: u64,
    pub time_window_hours: u32,
}

/// Co-selection action log entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CoSelectionAction {
    ModelAdded { model_id: ModelId, reason: AgentId },
    AgentRejected { agent_id: AgentId, reason: String },
}

// â”€â”€â”€ Agent Selection Functions â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

/// Phase A extension: Select which agents to run.
/// Runs AFTER select_models. Uses model selection to validate co-selection.
///
/// Algorithm:
/// 1. If agent_catalog is empty, return empty result immediately.
/// 2. Filter agents whose required_model is in the model selection or catalog.
/// 3. Score each agent: utility = demand_share * throughput_estimate.
/// 4. Sort by utility descending.
/// 5. Greedy knapsack: add agents while combined resource footprint fits.
pub fn select_agents(
    inputs: &SolverInputs,
    model_selection: &SelectionResult,
) -> AgentSelectionResult {
    // Early return when agent_catalog is empty (backwards compatibility)
    if inputs.agent_catalog.is_empty() {
        return AgentSelectionResult {
            selected: vec![],
            total_ram_allocated_mb: 0,
            total_cpu_cores_allocated: 0,
        };
    }

    // Compute total network capacity for the knapsack budget
    let total_network_ram: u64 = inputs
        .node_states
        .iter()
        .filter(|n| n.is_online)
        .map(|n| n.capabilities.ram.total_mb)
        .sum();

    let total_network_cpu: u32 = inputs
        .node_states
        .iter()
        .filter(|n| n.is_online)
        .map(|n| n.capabilities.cpu.cores)
        .sum();

    // Budget: remaining RAM after model selection, with 10% headroom reserved for OS
    let ram_budget = total_network_ram
        .saturating_sub(model_selection.total_ram_allocated_mb)
        .saturating_sub((total_network_ram as f64 * 0.10) as u64);

    let cpu_budget = (total_network_cpu as f64 * 0.80) as u32;

    // Collect model IDs that are selected or in catalog (for filtering)
    let selected_model_ids: Vec<&ModelId> = model_selection
        .selected
        .iter()
        .map(|s| &s.model_id)
        .collect();

    let catalog_model_ids: Vec<&ModelId> = inputs
        .model_catalog
        .iter()
        .map(|m| &m.model_id)
        .collect();

    // Filter agents whose required_model is in model selection or catalog
    let eligible_agents: Vec<&AgentEntry> = inputs
        .agent_catalog
        .iter()
        .filter(|agent| {
            selected_model_ids.contains(&&agent.required_model)
                || catalog_model_ids.contains(&&agent.required_model)
        })
        .collect();

    // Score each agent: utility = demand_share * throughput_estimate
    let mut scored: Vec<(&AgentEntry, f64)> = eligible_agents
        .iter()
        .map(|agent| {
            let demand_share = inputs
                .agent_demand
                .agent_shares
                .get(&agent.agent_id)
                .copied()
                .unwrap_or(0.0);

            // Throughput estimate: based on model performance (steps/minute)
            // Use the required model's avg_tok_s as a proxy for throughput
            let throughput_estimate = inputs
                .model_catalog
                .iter()
                .find(|m| m.model_id == agent.required_model)
                .map(|m| m.performance.avg_tok_s() as f64)
                .unwrap_or(1.0);

            let utility = demand_share * throughput_estimate;
            (*agent, utility)
        })
        .collect();

    // Sort by utility descending
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    // Greedy knapsack: add agents while combined resource footprint fits
    let mut selected: Vec<SelectedAgent> = Vec::new();
    let mut remaining_ram = ram_budget;
    let mut remaining_cpu = cpu_budget;
    let mut total_ram_allocated: u64 = 0;
    let mut total_cpu_allocated: u32 = 0;

    // Track which models have already been counted for RAM (shared model single-counting)
    let mut counted_model_ram: HashMap<&ModelId, u64> = HashMap::new();

    for (agent, utility) in &scored {
        let agent_ram = agent.runtime_requirements.ram_mb;
        let agent_cpu = agent.runtime_requirements.cpu_cores;

        // Combined footprint: agent RAM + required model RAM (if not already counted)
        let model_ram_cost = if counted_model_ram.contains_key(&agent.required_model) {
            0 // Model RAM already counted for another agent
        } else {
            inputs
                .model_catalog
                .iter()
                .find(|m| m.model_id == agent.required_model)
                .map(|m| m.requirements.min_ram_mb)
                .unwrap_or(0)
        };

        let total_ram_needed = agent_ram + model_ram_cost;

        // Check if agent fits within remaining budget
        if total_ram_needed > remaining_ram || agent_cpu > remaining_cpu {
            continue; // Skip this agent, try next
        }

        // Compute desired instance count
        let config = SolverConfig::default();
        let instance_count = compute_agent_desired_instances(
            &agent.agent_id,
            &inputs.agent_demand,
            &config,
        );

        // Add agent to selection
        selected.push(SelectedAgent {
            agent_id: agent.agent_id.clone(),
            instance_count,
            utility_score: *utility,
            required_model: agent.required_model.clone(),
        });

        // Update remaining capacity
        remaining_ram = remaining_ram.saturating_sub(total_ram_needed);
        remaining_cpu = remaining_cpu.saturating_sub(agent_cpu);
        total_ram_allocated += total_ram_needed;
        total_cpu_allocated += agent_cpu;

        // Mark model RAM as counted
        if model_ram_cost > 0 {
            counted_model_ram.insert(&agent.required_model, model_ram_cost);
        }
    }

    AgentSelectionResult {
        selected,
        total_ram_allocated_mb: total_ram_allocated,
        total_cpu_cores_allocated: total_cpu_allocated,
    }
}

/// Compute desired instance count per agent based on demand shares.
///
/// - Scale instances with demand (higher share -> more instances)
/// - Cap at config.max_instances_per_agent (default: 8)
/// - Minimum of 1 instance for any selected agent
pub fn compute_agent_desired_instances(
    agent_id: &AgentId,
    demand: &AgentWorkloadDemand,
    config: &SolverConfig,
) -> u32 {
    let share = demand
        .agent_shares
        .get(agent_id)
        .copied()
        .unwrap_or(0.0);

    if share == 0.0 || demand.total_agent_requests == 0 {
        return 1; // Minimum 1 instance for any selected agent
    }

    // Compute requests per minute for this agent
    let time_window_minutes = if demand.time_window_hours == 0 {
        1.0 // Avoid division by zero
    } else {
        demand.time_window_hours as f64 * 60.0
    };

    let requests_per_minute = demand.total_agent_requests as f64 * share / time_window_minutes;

    // Assume each instance can handle ~10 requests/minute (reasonable default)
    let capacity_per_instance = 10.0;

    let desired = (requests_per_minute / capacity_per_instance).ceil() as u32;

    // Clamp to [1, max_instances_per_agent]
    desired.clamp(1, config.max_instances_per_agent)
}

/// Co-selection enforcement: ensure every selected agent's required_model
/// is in the model selection. Adds missing models if capacity allows.
///
/// - For each selected agent, check if required_model is in model selection
/// - If model missing but capacity allows: add model to selection, emit ModelAdded
/// - If model missing and no capacity: reject agent, emit AgentRejected
/// - When multiple agents share same model, count model RAM only once
pub fn enforce_co_selection(
    model_selection: &mut SelectionResult,
    agent_selection: &mut AgentSelectionResult,
    inputs: &SolverInputs,
) -> Vec<CoSelectionAction> {
    let mut actions: Vec<CoSelectionAction> = Vec::new();

    // Compute total network RAM for capacity checking
    let total_network_ram: u64 = inputs
        .node_states
        .iter()
        .filter(|n| n.is_online)
        .map(|n| n.capabilities.ram.total_mb)
        .sum();

    // Max RAM budget (90% of total network RAM)
    let max_ram_budget = (total_network_ram as f64 * 0.90) as u64;

    // Track models we've already added in this pass (shared model single-counting)
    let mut models_added_this_pass: HashMap<ModelId, bool> = HashMap::new();

    // Collect agents to reject (can't modify while iterating)
    let mut agents_to_reject: Vec<AgentId> = Vec::new();

    for agent in &agent_selection.selected {
        // Check if required_model is already in model selection
        let model_present = model_selection
            .selected
            .iter()
            .any(|s| s.model_id == agent.required_model);

        if model_present {
            continue; // Model already selected, nothing to do
        }

        // Check if we already added this model for another agent in this pass
        if models_added_this_pass.contains_key(&agent.required_model) {
            continue; // Already handled
        }

        // Model is missing â€” try to add it
        let model_entry = inputs
            .model_catalog
            .iter()
            .find(|m| m.model_id == agent.required_model);

        match model_entry {
            Some(model) => {
                let model_ram = model.requirements.min_ram_mb;
                let current_total_ram = model_selection.total_ram_allocated_mb;

                // Check if adding this model would exceed capacity
                if current_total_ram + model_ram <= max_ram_budget {
                    // Add model to selection
                    model_selection.selected.push(SelectedModel {
                        model_id: model.model_id.clone(),
                        instance_count: 1,
                        utility_score: 0.0, // Co-selected, not utility-driven
                        is_exploration: false,
                    });
                    model_selection.total_ram_allocated_mb += model_ram;

                    models_added_this_pass.insert(agent.required_model.clone(), true);

                    actions.push(CoSelectionAction::ModelAdded {
                        model_id: agent.required_model.clone(),
                        reason: agent.agent_id.clone(),
                    });
                } else {
                    // No capacity â€” reject agent
                    agents_to_reject.push(agent.agent_id.clone());
                    actions.push(CoSelectionAction::AgentRejected {
                        agent_id: agent.agent_id.clone(),
                        reason: format!(
                            "required model '{}' cannot be added: insufficient RAM capacity",
                            agent.required_model
                        ),
                    });
                }
            }
            None => {
                // Model not in catalog â€” reject agent
                agents_to_reject.push(agent.agent_id.clone());
                actions.push(CoSelectionAction::AgentRejected {
                    agent_id: agent.agent_id.clone(),
                    reason: format!(
                        "required model '{}' not found in catalog",
                        agent.required_model
                    ),
                });
            }
        }
    }

    // Remove rejected agents from selection
    if !agents_to_reject.is_empty() {
        agent_selection
            .selected
            .retain(|a| !agents_to_reject.contains(&a.agent_id));

        // Recalculate totals
        agent_selection.total_ram_allocated_mb = agent_selection
            .selected
            .iter()
            .map(|a| {
                inputs
                    .agent_catalog
                    .iter()
                    .find(|e| e.agent_id == a.agent_id)
                    .map(|e| e.runtime_requirements.ram_mb)
                    .unwrap_or(0)
            })
            .sum();

        agent_selection.total_cpu_cores_allocated = agent_selection
            .selected
            .iter()
            .map(|a| {
                inputs
                    .agent_catalog
                    .iter()
                    .find(|e| e.agent_id == a.agent_id)
                    .map(|e| e.runtime_requirements.cpu_cores)
                    .unwrap_or(0)
            })
            .sum();
    }

    actions
}

// â”€â”€â”€ Phase B: Agent Placement â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

/// Priority levels for placement ordering.
/// Lower numeric value = higher priority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PlacementPriority {
    ActiveInference = 1,
    AgentSteps = 2,
    Background = 3,
    Speculative = 4,
}

/// Phase B extension: Assign agent instances to nodes.
/// Runs AFTER assign_models so model placements are known.
///
/// Algorithm:
/// 1. Sort agents by RAM descending (largest first for better bin-packing)
/// 2. For each agent instance:
///    a. Filter candidate nodes by tool availability, capacity, battery/thermal
///    b. Score candidates with co-location affinity and latency bonuses
///    c. Place on best-scoring node, update remaining capacity
/// 3. Generate download plans for missing runtimes/models
/// 4. Emit diagnostics for rejected agents
pub fn assign_agents(
    agent_selection: &AgentSelectionResult,
    nodes: &[NodeState],
    model_placements: &[ModelPlacement],
    catalog: &[AgentEntry],
    config: &SolverConfig,
) -> (Vec<AgentPlacement>, Vec<PendingDownload>, Vec<SolverDiagnostic>) {
    let mut placements: Vec<AgentPlacement> = Vec::new();
    let mut downloads: Vec<PendingDownload> = Vec::new();
    let mut diagnostics: Vec<SolverDiagnostic> = Vec::new();

    // Early return if nothing to place
    if agent_selection.selected.is_empty() {
        return (placements, downloads, diagnostics);
    }

    // Track remaining capacity per node (respecting ram_headroom_percent for OS)
    let mut remaining_ram: HashMap<NodeId, u64> = nodes
        .iter()
        .filter(|n| n.is_online)
        .map(|n| {
            let headroom_reserved =
                (n.capabilities.ram.total_mb as f64 * config.ram_headroom_percent) as u64;
            let usable = n.capabilities.ram.total_mb.saturating_sub(headroom_reserved);
            let used = n.utilization.ram_used_mb;
            (n.capabilities.node_id, usable.saturating_sub(used))
        })
        .collect();

    let mut remaining_cpu: HashMap<NodeId, u32> = nodes
        .iter()
        .filter(|n| n.is_online)
        .map(|n| {
            let usable = (n.capabilities.cpu.cores as f64 * config.cpu_headroom_percent) as u32;
            (n.capabilities.node_id, usable)
        })
        .collect();

    // Note: Model RAM is already accounted for in node utilization tracking.
    // The remaining_ram map starts from (total - headroom - used), which includes model usage.

    // Build sorted list of agent instances (by RAM descending for bin-packing)
    // Process in priority order: agents are priority 2 (after active inference models)
    let mut instances: Vec<(AgentId, u32, &AgentEntry)> = Vec::new();
    for selected in &agent_selection.selected {
        if let Some(entry) = catalog.iter().find(|e| e.agent_id == selected.agent_id) {
            for instance_idx in 0..selected.instance_count {
                instances.push((selected.agent_id.clone(), instance_idx, entry));
            }
        }
    }

    // Sort by RAM descending (largest first for better bin-packing)
    instances.sort_by(|a, b| {
        b.2.runtime_requirements
            .ram_mb
            .cmp(&a.2.runtime_requirements.ram_mb)
    });

    // Track which agents have been fully rejected (no node has required tools)
    let mut globally_rejected: HashMap<AgentId, bool> = HashMap::new();

    for (agent_id, _instance_idx, entry) in &instances {
        // Skip if already globally rejected
        if globally_rejected.contains_key(agent_id) {
            continue;
        }

        // Task 3.2: Tool availability validation
        let tool_eligible_nodes: Vec<&NodeState> = nodes
            .iter()
            .filter(|n| n.is_online)
            .filter(|n| has_all_required_tools(entry, n))
            .collect();

        if tool_eligible_nodes.is_empty() && !entry.tool_declarations.is_empty() {
            // No node has all required tools â€” reject agent globally
            globally_rejected.insert(agent_id.clone(), true);
            diagnostics.push(SolverDiagnostic {
                resource_type: ResourceType::Agent,
                resource_id: agent_id.clone(),
                reason: format!(
                    "required tools {:?} not available on any online node",
                    entry.tool_declarations
                ),
            });
            continue;
        }

        // Task 3.3: Model proximity constraint checking
        // Find model placements for this agent's required model
        let model_nodes: Vec<NodeId> = model_placements
            .iter()
            .filter(|mp| mp.model_id == entry.required_model)
            .flat_map(|mp| mp.assigned_nodes.clone())
            .collect();

        // Filter candidate nodes by all constraints
        let candidates: Vec<(&NodeState, f64)> = nodes
            .iter()
            .filter(|n| n.is_online)
            .filter(|n| has_all_required_tools(entry, n))
            .filter(|n| {
                // RAM capacity check
                let avail_ram = remaining_ram.get(&n.capabilities.node_id).copied().unwrap_or(0);
                avail_ram >= entry.runtime_requirements.ram_mb
            })
            .filter(|n| {
                // CPU capacity check
                let avail_cpu = remaining_cpu.get(&n.capabilities.node_id).copied().unwrap_or(0);
                avail_cpu >= entry.runtime_requirements.cpu_cores
            })
            .filter(|n| {
                // Battery/thermal constraints
                passes_battery_thermal_constraints(n)
            })
            .filter(|n| {
                // Model proximity: model must be on same node or low-latency peer
                passes_model_proximity(n, &model_nodes, nodes, config)
            })
            .map(|n| {
                let score = score_agent_candidate(n, entry, &model_nodes, nodes, config);
                (n, score)
            })
            .collect();

        if candidates.is_empty() {
            // No node fits â€” skip this instance (capacity exhausted)
            continue;
        }

        // Place on best-scoring node
        let best = candidates
            .iter()
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            .unwrap();

        let assigned_node = best.0.capabilities.node_id;

        // Find the model placement instance_id for the required model on this node (or closest)
        let model_instance_id = find_best_model_instance(
            &entry.required_model,
            assigned_node,
            model_placements,
            nodes,
            config,
        );

        placements.push(AgentPlacement {
            agent_id: agent_id.clone(),
            instance_id: uuid::Uuid::new_v4(),
            assigned_node,
            required_model_instance_id: model_instance_id,
            estimated_throughput: estimate_agent_throughput(entry, best.0),
            resource_allocation: entry.runtime_requirements.clone(),
        });

        // Update remaining capacity
        if let Some(ram) = remaining_ram.get_mut(&assigned_node) {
            *ram = ram.saturating_sub(entry.runtime_requirements.ram_mb);
        }
        if let Some(cpu) = remaining_cpu.get_mut(&assigned_node) {
            *cpu = cpu.saturating_sub(entry.runtime_requirements.cpu_cores);
        }
    }

    // Task 3.5: Generate download plans
    downloads = generate_download_plans(&placements, nodes, catalog, model_placements);

    (placements, downloads, diagnostics)
}

// â”€â”€â”€ Helper Functions for Agent Placement â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

/// Task 3.2: Check if a node has all tools required by an agent.
/// Returns true if `agent.tool_declarations âŠ† node.available_tools`.
fn has_all_required_tools(
    agent: &AgentEntry,
    node: &NodeState,
) -> bool {
    if agent.tool_declarations.is_empty() {
        return true;
    }

    let available_tool_ids: Vec<&str> = node
        .capabilities
        .available_tools
        .iter()
        .filter(|t| t.is_available)
        .map(|t| t.tool_id.as_str())
        .collect();

    agent
        .tool_declarations
        .iter()
        .all(|required| available_tool_ids.contains(&required.as_str()))
}

/// Task 3.3: Check if a node satisfies model proximity constraints.
/// The required model must be on the same node OR on a node with latency
/// below `pipeline_parallel_max_latency_ms`.
fn passes_model_proximity(
    candidate: &NodeState,
    model_nodes: &[NodeId],
    _all_nodes: &[NodeState],
    config: &SolverConfig,
) -> bool {
    // If no model placements exist yet (model might be downloaded later), allow placement
    if model_nodes.is_empty() {
        return true;
    }

    let candidate_id = candidate.capabilities.node_id;

    // Co-located: model is on the same node
    if model_nodes.contains(&candidate_id) {
        return true;
    }

    // Low-latency peer: check if any model node has latency below threshold
    for model_node_id in model_nodes {
        if let Some(measurement) = candidate.latency_to_peers.get(model_node_id) {
            if measurement.rtt_ms < config.pipeline_parallel_max_latency_ms {
                return true;
            }
        }
    }

    false
}

/// Check battery and thermal constraints for a node.
/// - Battery: must be >= 20% OR charging (only applies to nodes with battery info)
/// - Thermal: must not be in Critical state
///
/// Task 8.2: This function enforces:
///   `battery_percent >= battery_threshold OR is_charging`
///   Excludes nodes with ThermalState::Critical from new placements.
/// Applied to both model and agent placement candidate filtering.
pub fn passes_battery_thermal_constraints(node: &NodeState) -> bool {
    use crate::network::registry::ThermalState;

    // Thermal constraint: exclude nodes in Critical thermal state
    if node.thermal_state == ThermalState::Critical {
        return false;
    }

    // Battery constraint: >= 20% or charging (only for nodes with battery info)
    if let Some(phone_info) = &node.capabilities.phone_info {
        let battery_threshold = 20u8;
        if phone_info.battery_percent < battery_threshold && !phone_info.is_charging {
            return false;
        }
    }

    // Additional thermal heuristic: if CPU is extremely high (>95%) on a battery device,
    // treat as thermal throttling risk
    if node.capabilities.phone_info.is_some() && node.utilization.cpu_percent > 95.0 {
        return false;
    }

    true
}

// ─── Re-Solve Trigger on Tool Status Change (Task 8.3) ──────────────────────

/// Result of checking whether a placement plan has become stale due to tool changes.
#[derive(Debug, Clone)]
pub struct ReSolveCheck {
    /// Whether the plan should be re-solved.
    pub needs_re_solve: bool,
    /// Agent placements that are now invalid due to tool unavailability.
    pub invalid_placements: Vec<AgentPlacement>,
    /// Diagnostics explaining why each placement is invalid.
    pub diagnostics: Vec<SolverDiagnostic>,
}

/// Check whether a placement plan has become stale due to tool status changes.
///
/// When a tool becomes unavailable on a node, any agent placed on that node
/// which requires that tool is marked as invalid. The caller can use
/// `should_re_solve()` to quickly check if re-planning is needed.
///
/// Task 8.3: Implements re-solve trigger on tool status change.
pub fn check_tool_availability(
    agent_placements: &[AgentPlacement],
    catalog: &[AgentEntry],
    nodes: &[NodeState],
) -> ReSolveCheck {
    let mut invalid_placements: Vec<AgentPlacement> = Vec::new();
    let mut diagnostics: Vec<SolverDiagnostic> = Vec::new();

    for placement in agent_placements {
        // Find the agent's tool declarations from the catalog
        let agent_entry = match catalog.iter().find(|e| e.agent_id == placement.agent_id) {
            Some(e) => e,
            None => continue,
        };

        // Skip agents with no tool requirements
        if agent_entry.tool_declarations.is_empty() {
            continue;
        }

        // Find the node this agent is placed on
        let node = match nodes.iter().find(|n| n.capabilities.node_id == placement.assigned_node) {
            Some(n) => n,
            None => {
                // Node no longer exists — placement is invalid
                invalid_placements.push(placement.clone());
                diagnostics.push(SolverDiagnostic {
                    resource_type: ResourceType::Agent,
                    resource_id: placement.agent_id.clone(),
                    reason: format!(
                        "assigned node {:?} is no longer available",
                        placement.assigned_node
                    ),
                });
                continue;
            }
        };

        // Check if all required tools are still available on this node
        let available_tool_ids: Vec<&str> = node
            .capabilities
            .available_tools
            .iter()
            .filter(|t| t.is_available)
            .map(|t| t.tool_id.as_str())
            .collect();

        let missing_tools: Vec<&String> = agent_entry
            .tool_declarations
            .iter()
            .filter(|required| !available_tool_ids.contains(&required.as_str()))
            .collect();

        if !missing_tools.is_empty() {
            invalid_placements.push(placement.clone());
            diagnostics.push(SolverDiagnostic {
                resource_type: ResourceType::Agent,
                resource_id: placement.agent_id.clone(),
                reason: format!(
                    "required tools {:?} no longer available on node {:?} — agent needs relocation",
                    missing_tools, placement.assigned_node
                ),
            });
        }
    }

    let needs_re_solve = !invalid_placements.is_empty();

    ReSolveCheck {
        needs_re_solve,
        invalid_placements,
        diagnostics,
    }
}

/// Quick helper to determine if a placement plan should be re-solved.
///
/// Returns `true` if any agent placement has become invalid due to:
/// - A required tool becoming unavailable on the assigned node
/// - The assigned node going offline
///
/// Callers can use this as a lightweight check before triggering a full re-solve.
pub fn should_re_solve(
    agent_placements: &[AgentPlacement],
    catalog: &[AgentEntry],
    nodes: &[NodeState],
) -> bool {
    check_tool_availability(agent_placements, catalog, nodes).needs_re_solve
}

/// Score a candidate node for agent placement.
/// Scoring factors:
/// - Co-location bonus: +0.4 if required model is on this node
/// - Latency bonus: +0.2 if required model is on a low-latency peer
/// - Headroom bonus: +0.2 for spare capacity
/// - Queue penalty: -0.2 for high queue depth
/// - Stability bonus: +0.2 for high stability score
fn score_agent_candidate(
    node: &NodeState,
    _agent: &AgentEntry,
    model_nodes: &[NodeId],
    _all_nodes: &[NodeState],
    config: &SolverConfig,
) -> f64 {
    let mut score = 0.0;
    let node_id = node.capabilities.node_id;

    // Co-location affinity bonus (+0.4 if required model on same node)
    if model_nodes.contains(&node_id) {
        score += config.co_location_affinity_bonus; // default: 0.4
    } else {
        // Latency bonus (+0.2) for low-latency peer nodes
        let has_low_latency_model = model_nodes.iter().any(|model_node_id| {
            node.latency_to_peers
                .get(model_node_id)
                .map(|m| m.rtt_ms < config.pipeline_parallel_max_latency_ms)
                .unwrap_or(false)
        });
        if has_low_latency_model {
            score += 0.2;
        }
    }

    // Headroom bonus: prefer nodes with more spare RAM capacity
    let ram_usage = node.utilization.ram_used_mb as f64 / node.capabilities.ram.total_mb as f64;
    let headroom = 1.0 - ram_usage;
    score += headroom * 0.2;

    // Queue penalty: prefer less busy nodes
    let queue_penalty = (node.utilization.queue_depth as f64 / 10.0).min(1.0);
    score += (1.0 - queue_penalty) * 0.2;

    // Stability bonus
    score += node.stability_score * 0.2;

    score
}

/// Find the best model instance_id for an agent's required model.
/// Prefers model instances on the same node, then closest by latency.
fn find_best_model_instance(
    required_model: &ModelId,
    agent_node: NodeId,
    model_placements: &[ModelPlacement],
    nodes: &[NodeState],
    _config: &SolverConfig,
) -> uuid::Uuid {
    let relevant_placements: Vec<&ModelPlacement> = model_placements
        .iter()
        .filter(|mp| mp.model_id == *required_model)
        .collect();

    // Prefer co-located model instance
    if let Some(colocated) = relevant_placements
        .iter()
        .find(|mp| mp.assigned_nodes.contains(&agent_node))
    {
        return colocated.instance_id;
    }

    // Find closest by latency
    let agent_node_state = nodes.iter().find(|n| n.capabilities.node_id == agent_node);

    if let Some(agent_state) = agent_node_state {
        let mut best: Option<(&ModelPlacement, f64)> = None;
        for mp in &relevant_placements {
            for model_node_id in &mp.assigned_nodes {
                let latency = agent_state
                    .latency_to_peers
                    .get(model_node_id)
                    .map(|m| m.rtt_ms)
                    .unwrap_or(f64::MAX);
                if best.is_none() || latency < best.unwrap().1 {
                    best = Some((mp, latency));
                }
            }
        }
        if let Some((mp, _)) = best {
            return mp.instance_id;
        }
    }

    // Fallback: use first available placement or generate a new UUID
    relevant_placements
        .first()
        .map(|mp| mp.instance_id)
        .unwrap_or_else(uuid::Uuid::new_v4)
}

/// Estimate agent throughput (steps/minute) based on node capabilities.
fn estimate_agent_throughput(agent: &AgentEntry, node: &NodeState) -> f64 {
    // Base throughput estimate: CPU clock * cores as a proxy
    let cpu_factor = node.capabilities.cpu.clock_mhz as f64 * node.capabilities.cpu.cores as f64;
    // Normalize to a reasonable steps/minute range (10-100)
    let base_throughput = (cpu_factor / 32000.0).clamp(10.0, 100.0);

    // Scale by agent's resource requirements (lighter agents are faster)
    let ram_factor = 1.0 / (1.0 + agent.runtime_requirements.ram_mb as f64 / 1024.0);
    base_throughput * (0.5 + ram_factor * 0.5)
}

// ─── Parallelism Factor (Task 5.2) ──────────────────────────────────────────

/// Compute the parallelism factor for agent step distribution.
///
/// Formula: independent_steps / total_steps × (1 - avg_network_latency / step_compute_time)
///          × min_node_speed / max_node_speed
///
/// Result is clamped to [0.0, 1.0].
/// When speed ratio exceeds `speed_ratio_threshold` (default 3.0), returns 0.0.
pub fn compute_parallelism_factor(
    independent_steps: u32,
    total_steps: u32,
    avg_network_latency_ms: f64,
    step_compute_time_ms: f64,
    min_node_speed: f64,
    max_node_speed: f64,
    config: &SolverConfig,
) -> f64 {
    // Guard against division by zero
    if total_steps == 0 || step_compute_time_ms <= 0.0 || max_node_speed <= 0.0 {
        return 0.0;
    }

    // Speed ratio check: if max/min exceeds threshold, no parallelization
    if min_node_speed <= 0.0 {
        return 0.0;
    }

    let speed_ratio = max_node_speed / min_node_speed;
    if speed_ratio > config.speed_ratio_threshold {
        return 0.0;
    }

    // Compute each factor
    let step_ratio = independent_steps as f64 / total_steps as f64;
    let latency_factor = 1.0 - (avg_network_latency_ms / step_compute_time_ms);
    let speed_factor = min_node_speed / max_node_speed;

    // Combine and clamp to [0.0, 1.0]
    let result = step_ratio * latency_factor * speed_factor;
    result.clamp(0.0, 1.0)
}

// ─── Agent Utility (Task 5.3) ────────────────────────────────────────────────

/// Compute agent utility for the objective function.
///
/// Formula: U_agent = Σ agent_throughput_j × parallelism_factor_j
///
/// Computes agent_throughput_j from historical demand data (steps/minute estimate).
/// When no agents are placed, returns 0.0.
pub fn compute_agent_utility(
    agent_placements: &[AgentPlacement],
    nodes: &[NodeState],
    config: &SolverConfig,
) -> f64 {
    if agent_placements.is_empty() {
        return 0.0;
    }

    // Compute min and max node speeds across all nodes with agents
    let agent_node_ids: Vec<NodeId> = agent_placements
        .iter()
        .map(|ap| ap.assigned_node)
        .collect();

    let agent_nodes: Vec<&NodeState> = nodes
        .iter()
        .filter(|n| agent_node_ids.contains(&n.capabilities.node_id))
        .collect();

    if agent_nodes.is_empty() {
        return 0.0;
    }

    let node_speeds: Vec<f64> = agent_nodes
        .iter()
        .map(|n| n.capabilities.cpu.clock_mhz as f64 * n.capabilities.cpu.cores as f64)
        .collect();

    let min_node_speed = node_speeds.iter().cloned().fold(f64::MAX, f64::min);
    let max_node_speed = node_speeds.iter().cloned().fold(0.0_f64, f64::max);

    // Compute average network latency across agent nodes
    let avg_network_latency_ms: f64 = {
        let latencies: Vec<f64> = agent_nodes
            .iter()
            .flat_map(|n| n.latency_to_peers.values().map(|m| m.rtt_ms))
            .collect();
        if latencies.is_empty() {
            0.0
        } else {
            latencies.iter().sum::<f64>() / latencies.len() as f64
        }
    };

    // Default step compute time (100ms) and assume 50% of steps are independent
    let step_compute_time_ms = 100.0;
    let independent_ratio = 0.5; // Default: half of steps can run in parallel
    let total_steps = 10u32; // Normalized step count
    let independent_steps = (total_steps as f64 * independent_ratio) as u32;

    let parallelism_factor = compute_parallelism_factor(
        independent_steps,
        total_steps,
        avg_network_latency_ms,
        step_compute_time_ms,
        min_node_speed,
        max_node_speed,
        config,
    );

    // Sum: agent_throughput_j × parallelism_factor_j
    let total_utility: f64 = agent_placements
        .iter()
        .map(|ap| {
            // Normalize throughput to [0, 1] range for utility scoring
            let normalized_throughput = (ap.estimated_throughput / 100.0).clamp(0.0, 1.0);
            normalized_throughput * parallelism_factor
        })
        .sum();

    total_utility
}

// ─── Speed-Matching Load Distribution (Task 5.5) ─────────────────────────────

/// Load distribution recommendation for a step on a specific node.
#[derive(Debug, Clone)]
pub struct LoadAssignment {
    pub node_id: NodeId,
    pub load_fraction: f64,
    pub is_preferred_for_compute: bool,
    pub is_preferred_for_tools: bool,
}

/// Compute speed-matching load distribution across nodes.
///
/// Assigns proportional load to nodes based on compute speed:
/// - Prefer fastest node for compute-heavy steps
/// - Prefer least-loaded node for lightweight tool calls
///
/// Uses node benchmark scores (clock_mhz * cores as proxy for tokens/second).
pub fn compute_load_distribution(
    agent_placements: &[AgentPlacement],
    nodes: &[NodeState],
    _config: &SolverConfig,
) -> Vec<LoadAssignment> {
    if agent_placements.is_empty() || nodes.is_empty() {
        return vec![];
    }

    // Get unique nodes that have agents placed on them
    let agent_node_ids: Vec<NodeId> = agent_placements
        .iter()
        .map(|ap| ap.assigned_node)
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    let agent_nodes: Vec<&NodeState> = nodes
        .iter()
        .filter(|n| agent_node_ids.contains(&n.capabilities.node_id))
        .collect();

    if agent_nodes.is_empty() {
        return vec![];
    }

    // Compute speed scores for each node
    let speed_scores: Vec<(NodeId, f64)> = agent_nodes
        .iter()
        .map(|n| {
            let speed = n.capabilities.cpu.clock_mhz as f64 * n.capabilities.cpu.cores as f64;
            (n.capabilities.node_id, speed)
        })
        .collect();

    let total_speed: f64 = speed_scores.iter().map(|(_, s)| s).sum();
    let max_speed = speed_scores
        .iter()
        .map(|(_, s)| *s)
        .fold(0.0_f64, f64::max);

    // Compute load utilization for each node (lower = less loaded)
    let load_scores: Vec<(NodeId, f64)> = agent_nodes
        .iter()
        .map(|n| {
            let ram_usage =
                n.utilization.ram_used_mb as f64 / n.capabilities.ram.total_mb.max(1) as f64;
            let cpu_usage = n.utilization.cpu_percent as f64 / 100.0;
            let load = (ram_usage + cpu_usage) / 2.0;
            (n.capabilities.node_id, load)
        })
        .collect();

    let min_load = load_scores
        .iter()
        .map(|(_, l)| *l)
        .fold(f64::MAX, f64::min);

    // Build load assignments
    speed_scores
        .iter()
        .map(|(node_id, speed)| {
            // Proportional load based on speed
            let load_fraction = if total_speed > 0.0 {
                speed / total_speed
            } else {
                1.0 / agent_nodes.len() as f64
            };

            // Fastest node preferred for compute-heavy steps
            let is_preferred_for_compute = (*speed - max_speed).abs() < 1e-10;

            // Least-loaded node preferred for lightweight tool calls
            let node_load = load_scores
                .iter()
                .find(|(id, _)| id == node_id)
                .map(|(_, l)| *l)
                .unwrap_or(1.0);
            let is_preferred_for_tools = (node_load - min_load).abs() < 1e-10;

            LoadAssignment {
                node_id: *node_id,
                load_fraction,
                is_preferred_for_compute,
                is_preferred_for_tools,
            }
        })
        .collect()
}

/// Task 3.5: Generate download plans for agents placed on nodes missing runtimes/models.
fn generate_download_plans(
    placements: &[AgentPlacement],
    nodes: &[NodeState],
    catalog: &[AgentEntry],
    model_placements: &[ModelPlacement],
) -> Vec<PendingDownload> {
    let mut downloads: Vec<PendingDownload> = Vec::new();

    for placement in placements {
        let agent_entry = match catalog.iter().find(|e| e.agent_id == placement.agent_id) {
            Some(e) => e,
            None => continue,
        };

        let node = match nodes
            .iter()
            .find(|n| n.capabilities.node_id == placement.assigned_node)
        {
            Some(n) => n,
            None => continue,
        };

        // Check if agent runtime is already on this node
        // (We use loaded_models as a proxy â€” if the agent isn't listed, it needs download)
        let agent_present_on_node = node
            .loaded_models
            .iter()
            .any(|lm| lm.model_id == placement.agent_id);

        // Check if required model is present on the assigned node
        let model_present_on_node = node
            .loaded_models
            .iter()
            .any(|lm| lm.model_id == agent_entry.required_model);

        // Also check if model is placed on this node via model_placements
        let model_placed_on_node = model_placements.iter().any(|mp| {
            mp.model_id == agent_entry.required_model
                && mp.assigned_nodes.contains(&placement.assigned_node)
        });

        let model_needs_download = !model_present_on_node && !model_placed_on_node;

        // Emit model download with higher priority (if needed)
        if model_needs_download {
            let model_download_id = format!("model:{}", agent_entry.required_model);
            // Only add if not already in downloads list
            if !downloads.iter().any(|d| {
                d.resource_id == model_download_id && d.target_node == placement.assigned_node
            }) {
                let source = agent_entry
                    .download_sources
                    .first()
                    .cloned()
                    .unwrap_or(DownloadSource {
                        source_type: super::catalog::SourceType::HuggingFaceHub,
                        url: format!(
                            "https://huggingface.co/models/{}",
                            agent_entry.required_model
                        ),
                        priority: 1,
                    });

                downloads.push(PendingDownload {
                    resource_type: ResourceType::Model,
                    resource_id: model_download_id,
                    target_node: placement.assigned_node,
                    source,
                    size_mb: 0, // Size unknown without model catalog access here
                    priority: DownloadPriority::High,
                    depends_on: vec![],
                });
            }
        }

        // Emit agent runtime download (if needed)
        if !agent_present_on_node {
            let agent_download_id = format!("agent:{}", placement.agent_id);
            let mut depends_on = vec![];

            // Agent download depends on model download completing first
            if model_needs_download {
                depends_on.push(format!("model:{}", agent_entry.required_model));
            }

            let source = agent_entry
                .download_sources
                .first()
                .cloned()
                .unwrap_or(DownloadSource {
                    source_type: super::catalog::SourceType::HuggingFaceHub,
                    url: format!("https://registry.example.com/agents/{}", placement.agent_id),
                    priority: 2,
                });

            downloads.push(PendingDownload {
                resource_type: ResourceType::Agent,
                resource_id: agent_download_id,
                target_node: placement.assigned_node,
                source,
                size_mb: agent_entry.runtime_requirements.disk_mb,
                priority: DownloadPriority::Normal,
                depends_on,
            });
        }
    }

    downloads
}


// â”€â”€â”€ Unit Tests â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::catalog::*;
    use crate::network::demand::WorkloadDemand;
    use crate::network::registry::*;
    use crate::network::solver::{ModelPlacement, ParallelismProtocol, SolverPreferences};

    fn make_model(id: &str, ram_mb: u64) -> ModelEntry {
        ModelEntry {
            model_id: id.to_string(),
            family: "test".to_string(),
            parameter_count_b: 7.0,
            quantization: Quantization::Q4_K_M,
            requirements: ModelRequirements {
                min_ram_mb: ram_mb,
                min_vram_mb: 0,
                disk_size_mb: ram_mb,
                min_compute_capability: None,
            },
            performance: ModelPerformance {
                estimates: vec![PerformanceEstimate {
                    hardware_class: HardwareClass::CpuOnly,
                    estimated_tok_s: 20.0,
                    estimated_prefill_tok_s: 50.0,
                }],
            },
            task_affinity: HashMap::from([(TaskType::Chat, 0.7)]),
            supported_backends: vec![InferenceBackend::Ollama],
            download_sources: vec![],
            checksum_sha256: "test".to_string(),
        }
    }

    fn make_agent(id: &str, required_model: &str, ram_mb: u64, cpu_cores: u32) -> AgentEntry {
        AgentEntry {
            agent_id: id.to_string(),
            agent_name: id.to_string(),
            version: "1.0".to_string(),
            required_model: required_model.to_string(),
            tool_declarations: vec![],
            runtime_requirements: AgentRequirements {
                ram_mb,
                cpu_cores,
                disk_mb: 100,
            },
            download_sources: vec![],
            checksum_sha256: "test".to_string(),
        }
    }

    fn make_node(ram_mb: u64, cores: u32) -> NodeState {
        let node_id = uuid::Uuid::new_v4();
        NodeState {
            capabilities: NodeCapabilities {
                node_id,
                hostname: "test".to_string(),
                device_type: DeviceType::Desktop,
                cpu: CpuProfile {
                    cores,
                    architecture: "x86_64".to_string(),
                    clock_mhz: 4000,
                    isa_extensions: vec![],
                },
                ram: RamProfile {
                    total_mb: ram_mb,
                    available_mb: ram_mb,
                    ddr_generation: 4,
                },
                gpu: None,
                storage: StorageProfile {
                    storage_type: StorageType::Nvme,
                    available_mb: 500000,
                    read_speed_mbps: 5000,
                },
                network_interfaces: vec![],
                phone_info: None,
                available_tools: vec![],
            },
            utilization: NodeUtilization::default(),
            loaded_models: vec![],
            stability_score: 0.95,
            last_heartbeat_ms: 0,
            is_online: true,
            latency_to_peers: HashMap::new(),
            thermal_state: ThermalState::default(),
        }
    }

    fn make_inputs(
        nodes: Vec<NodeState>,
        models: Vec<ModelEntry>,
        agents: Vec<AgentEntry>,
        agent_shares: HashMap<AgentId, f64>,
    ) -> SolverInputs {
        SolverInputs {
            node_states: nodes,
            model_catalog: models,
            workload_demand: WorkloadDemand {
                computed_at_ms: 0,
                time_window_hours: 24,
                model_shares: HashMap::new(),
                task_shares: HashMap::new(),
                total_requests: 100,
                forecast: crate::network::demand::DemandForecast {
                    next_period_model_shares: HashMap::new(),
                    next_period_task_shares: HashMap::new(),
                    confidence: 0.8,
                    prefetch_signals: vec![],
                },
            },
            preferences: SolverPreferences::new(),
            max_network_params_b: 14.0,
            agent_catalog: agents,
            agent_demand: AgentWorkloadDemand {
                agent_shares,
                total_agent_requests: 1000,
                time_window_hours: 24,
            },
        }
    }

    // â”€â”€â”€ select_agents tests â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    #[test]
    fn test_select_agents_empty_catalog_returns_empty() {
        let inputs = make_inputs(
            vec![make_node(32_000, 8)],
            vec![make_model("model-a", 4000)],
            vec![], // empty agent catalog
            HashMap::new(),
        );

        let model_selection = SelectionResult {
            selected: vec![SelectedModel {
                model_id: "model-a".to_string(),
                instance_count: 1,
                utility_score: 0.5,
                is_exploration: false,
            }],
            total_ram_allocated_mb: 4000,
            total_vram_allocated_mb: 0,
            exploration_model: None,
        };

        let result = select_agents(&inputs, &model_selection);
        assert!(result.selected.is_empty());
        assert_eq!(result.total_ram_allocated_mb, 0);
        assert_eq!(result.total_cpu_cores_allocated, 0);
    }

    #[test]
    fn test_select_agents_basic_selection() {
        let inputs = make_inputs(
            vec![make_node(32_000, 16)],
            vec![make_model("model-a", 4000)],
            vec![make_agent("agent-1", "model-a", 512, 2)],
            HashMap::from([("agent-1".to_string(), 0.8)]),
        );

        let model_selection = SelectionResult {
            selected: vec![SelectedModel {
                model_id: "model-a".to_string(),
                instance_count: 1,
                utility_score: 0.5,
                is_exploration: false,
            }],
            total_ram_allocated_mb: 4000,
            total_vram_allocated_mb: 0,
            exploration_model: None,
        };

        let result = select_agents(&inputs, &model_selection);
        assert_eq!(result.selected.len(), 1);
        assert_eq!(result.selected[0].agent_id, "agent-1");
        assert_eq!(result.selected[0].required_model, "model-a");
        assert!(result.selected[0].utility_score > 0.0);
    }

    #[test]
    fn test_select_agents_filters_by_model_availability() {
        let inputs = make_inputs(
            vec![make_node(32_000, 16)],
            vec![make_model("model-a", 4000)],
            vec![
                make_agent("agent-1", "model-a", 512, 2),
                make_agent("agent-2", "model-nonexistent", 512, 2),
            ],
            HashMap::from([
                ("agent-1".to_string(), 0.5),
                ("agent-2".to_string(), 0.5),
            ]),
        );

        let model_selection = SelectionResult {
            selected: vec![SelectedModel {
                model_id: "model-a".to_string(),
                instance_count: 1,
                utility_score: 0.5,
                is_exploration: false,
            }],
            total_ram_allocated_mb: 4000,
            total_vram_allocated_mb: 0,
            exploration_model: None,
        };

        let result = select_agents(&inputs, &model_selection);
        // Only agent-1 should be selected (agent-2's model not in catalog)
        assert_eq!(result.selected.len(), 1);
        assert_eq!(result.selected[0].agent_id, "agent-1");
    }

    #[test]
    fn test_select_agents_respects_capacity() {
        // Node with very limited RAM
        let inputs = make_inputs(
            vec![make_node(5_000, 4)],
            vec![make_model("model-a", 4000)],
            vec![
                make_agent("agent-big", "model-a", 2000, 2),
                make_agent("agent-small", "model-a", 200, 1),
            ],
            HashMap::from([
                ("agent-big".to_string(), 0.3),
                ("agent-small".to_string(), 0.7),
            ]),
        );

        let model_selection = SelectionResult {
            selected: vec![SelectedModel {
                model_id: "model-a".to_string(),
                instance_count: 1,
                utility_score: 0.5,
                is_exploration: false,
            }],
            total_ram_allocated_mb: 4000,
            total_vram_allocated_mb: 0,
            exploration_model: None,
        };

        let result = select_agents(&inputs, &model_selection);
        // With 5000 total, 4000 for model, 500 headroom (10%), only 500 left
        // agent-big needs 2000 (won't fit), agent-small needs 200 (fits)
        assert!(result.selected.iter().all(|a| a.agent_id != "agent-big"));
    }

    #[test]
    fn test_select_agents_sorted_by_utility() {
        let inputs = make_inputs(
            vec![make_node(64_000, 32)],
            vec![make_model("model-a", 4000)],
            vec![
                make_agent("agent-low", "model-a", 512, 2),
                make_agent("agent-high", "model-a", 512, 2),
            ],
            HashMap::from([
                ("agent-low".to_string(), 0.1),
                ("agent-high".to_string(), 0.9),
            ]),
        );

        let model_selection = SelectionResult {
            selected: vec![SelectedModel {
                model_id: "model-a".to_string(),
                instance_count: 1,
                utility_score: 0.5,
                is_exploration: false,
            }],
            total_ram_allocated_mb: 4000,
            total_vram_allocated_mb: 0,
            exploration_model: None,
        };

        let result = select_agents(&inputs, &model_selection);
        assert_eq!(result.selected.len(), 2);
        // Higher utility agent should be first
        assert_eq!(result.selected[0].agent_id, "agent-high");
        assert_eq!(result.selected[1].agent_id, "agent-low");
    }

    #[test]
    fn test_select_agents_shared_model_counted_once() {
        let inputs = make_inputs(
            vec![make_node(32_000, 16)],
            vec![make_model("shared-model", 4000)],
            vec![
                make_agent("agent-1", "shared-model", 512, 2),
                make_agent("agent-2", "shared-model", 512, 2),
            ],
            HashMap::from([
                ("agent-1".to_string(), 0.5),
                ("agent-2".to_string(), 0.5),
            ]),
        );

        // Model NOT in selection (will be counted as model_ram_cost for first agent only)
        let model_selection = SelectionResult {
            selected: vec![],
            total_ram_allocated_mb: 0,
            total_vram_allocated_mb: 0,
            exploration_model: None,
        };

        let result = select_agents(&inputs, &model_selection);
        // Both agents should be selected
        assert_eq!(result.selected.len(), 2);
        // Model RAM (4000) should be counted only once, not twice
        // Total = 4000 (model, once) + 512 (agent-1) + 512 (agent-2) = 5024
        assert_eq!(result.total_ram_allocated_mb, 4000 + 512 + 512);
    }

    // â”€â”€â”€ compute_agent_desired_instances tests â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    #[test]
    fn test_desired_instances_minimum_one() {
        let config = SolverConfig::default();
        let demand = AgentWorkloadDemand {
            agent_shares: HashMap::new(), // No share for this agent
            total_agent_requests: 100,
            time_window_hours: 24,
        };

        let result = compute_agent_desired_instances(&"agent-1".to_string(), &demand, &config);
        assert_eq!(result, 1);
    }

    #[test]
    fn test_desired_instances_scales_with_demand() {
        let config = SolverConfig::default();

        let low_demand = AgentWorkloadDemand {
            agent_shares: HashMap::from([("agent-1".to_string(), 0.1)]),
            total_agent_requests: 100,
            time_window_hours: 24,
        };

        let high_demand = AgentWorkloadDemand {
            agent_shares: HashMap::from([("agent-1".to_string(), 1.0)]),
            total_agent_requests: 10000,
            time_window_hours: 1,
        };

        let low_instances =
            compute_agent_desired_instances(&"agent-1".to_string(), &low_demand, &config);
        let high_instances =
            compute_agent_desired_instances(&"agent-1".to_string(), &high_demand, &config);

        assert!(high_instances >= low_instances);
    }

    #[test]
    fn test_desired_instances_capped_at_max() {
        let config = SolverConfig::default(); // max_instances_per_agent = 8

        let extreme_demand = AgentWorkloadDemand {
            agent_shares: HashMap::from([("agent-1".to_string(), 1.0)]),
            total_agent_requests: 1_000_000,
            time_window_hours: 1,
        };

        let result =
            compute_agent_desired_instances(&"agent-1".to_string(), &extreme_demand, &config);
        assert_eq!(result, 8); // Capped at max
    }

    #[test]
    fn test_desired_instances_always_at_least_one() {
        let config = SolverConfig::default();

        let zero_demand = AgentWorkloadDemand {
            agent_shares: HashMap::from([("agent-1".to_string(), 0.0)]),
            total_agent_requests: 0,
            time_window_hours: 24,
        };

        let result =
            compute_agent_desired_instances(&"agent-1".to_string(), &zero_demand, &config);
        assert_eq!(result, 1);
    }

    #[test]
    fn test_desired_instances_monotonicity() {
        let config = SolverConfig::default();

        let shares = [0.01, 0.1, 0.3, 0.5, 0.8, 1.0];
        let mut prev_instances = 0u32;

        for share in shares {
            let demand = AgentWorkloadDemand {
                agent_shares: HashMap::from([("agent-1".to_string(), share)]),
                total_agent_requests: 5000,
                time_window_hours: 24,
            };
            let instances =
                compute_agent_desired_instances(&"agent-1".to_string(), &demand, &config);
            assert!(instances >= prev_instances, "Monotonicity violated at share={}", share);
            prev_instances = instances;
        }
    }

    // â”€â”€â”€ enforce_co_selection tests â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    #[test]
    fn test_co_selection_model_already_present() {
        let inputs = make_inputs(
            vec![make_node(32_000, 16)],
            vec![make_model("model-a", 4000)],
            vec![make_agent("agent-1", "model-a", 512, 2)],
            HashMap::from([("agent-1".to_string(), 0.5)]),
        );

        let mut model_selection = SelectionResult {
            selected: vec![SelectedModel {
                model_id: "model-a".to_string(),
                instance_count: 1,
                utility_score: 0.5,
                is_exploration: false,
            }],
            total_ram_allocated_mb: 4000,
            total_vram_allocated_mb: 0,
            exploration_model: None,
        };

        let mut agent_selection = AgentSelectionResult {
            selected: vec![SelectedAgent {
                agent_id: "agent-1".to_string(),
                instance_count: 1,
                utility_score: 0.5,
                required_model: "model-a".to_string(),
            }],
            total_ram_allocated_mb: 512,
            total_cpu_cores_allocated: 2,
        };

        let actions = enforce_co_selection(&mut model_selection, &mut agent_selection, &inputs);
        // No actions needed â€” model already present
        assert!(actions.is_empty());
        assert_eq!(agent_selection.selected.len(), 1);
    }

    #[test]
    fn test_co_selection_adds_missing_model() {
        let inputs = make_inputs(
            vec![make_node(32_000, 16)],
            vec![make_model("model-a", 4000)],
            vec![make_agent("agent-1", "model-a", 512, 2)],
            HashMap::from([("agent-1".to_string(), 0.5)]),
        );

        let mut model_selection = SelectionResult {
            selected: vec![], // Model NOT selected
            total_ram_allocated_mb: 0,
            total_vram_allocated_mb: 0,
            exploration_model: None,
        };

        let mut agent_selection = AgentSelectionResult {
            selected: vec![SelectedAgent {
                agent_id: "agent-1".to_string(),
                instance_count: 1,
                utility_score: 0.5,
                required_model: "model-a".to_string(),
            }],
            total_ram_allocated_mb: 512,
            total_cpu_cores_allocated: 2,
        };

        let actions = enforce_co_selection(&mut model_selection, &mut agent_selection, &inputs);

        // Should add model-a
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            CoSelectionAction::ModelAdded { model_id, reason } => {
                assert_eq!(model_id, "model-a");
                assert_eq!(reason, "agent-1");
            }
            _ => panic!("Expected ModelAdded action"),
        }
        // Model should now be in selection
        assert!(model_selection.selected.iter().any(|s| s.model_id == "model-a"));
        assert_eq!(model_selection.total_ram_allocated_mb, 4000);
    }

    #[test]
    fn test_co_selection_rejects_agent_when_no_capacity() {
        let inputs = make_inputs(
            vec![make_node(5_000, 4)], // Very limited capacity
            vec![make_model("model-big", 5000)], // Model takes all capacity
            vec![make_agent("agent-1", "model-big", 512, 2)],
            HashMap::from([("agent-1".to_string(), 0.5)]),
        );

        let mut model_selection = SelectionResult {
            selected: vec![], // Model not selected
            // Already at capacity (90% of 5000 = 4500, model needs 5000)
            total_ram_allocated_mb: 4500,
            total_vram_allocated_mb: 0,
            exploration_model: None,
        };

        let mut agent_selection = AgentSelectionResult {
            selected: vec![SelectedAgent {
                agent_id: "agent-1".to_string(),
                instance_count: 1,
                utility_score: 0.5,
                required_model: "model-big".to_string(),
            }],
            total_ram_allocated_mb: 512,
            total_cpu_cores_allocated: 2,
        };

        let actions = enforce_co_selection(&mut model_selection, &mut agent_selection, &inputs);

        // Should reject agent
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            CoSelectionAction::AgentRejected { agent_id, reason } => {
                assert_eq!(agent_id, "agent-1");
                assert!(reason.contains("insufficient RAM capacity"));
            }
            _ => panic!("Expected AgentRejected action"),
        }
        // Agent should be removed from selection
        assert!(agent_selection.selected.is_empty());
    }

    #[test]
    fn test_co_selection_rejects_agent_model_not_in_catalog() {
        let inputs = make_inputs(
            vec![make_node(32_000, 16)],
            vec![], // Empty catalog â€” model not available
            vec![make_agent("agent-1", "nonexistent-model", 512, 2)],
            HashMap::from([("agent-1".to_string(), 0.5)]),
        );

        let mut model_selection = SelectionResult {
            selected: vec![],
            total_ram_allocated_mb: 0,
            total_vram_allocated_mb: 0,
            exploration_model: None,
        };

        let mut agent_selection = AgentSelectionResult {
            selected: vec![SelectedAgent {
                agent_id: "agent-1".to_string(),
                instance_count: 1,
                utility_score: 0.5,
                required_model: "nonexistent-model".to_string(),
            }],
            total_ram_allocated_mb: 512,
            total_cpu_cores_allocated: 2,
        };

        let actions = enforce_co_selection(&mut model_selection, &mut agent_selection, &inputs);

        assert_eq!(actions.len(), 1);
        match &actions[0] {
            CoSelectionAction::AgentRejected { agent_id, reason } => {
                assert_eq!(agent_id, "agent-1");
                assert!(reason.contains("not found in catalog"));
            }
            _ => panic!("Expected AgentRejected action"),
        }
        assert!(agent_selection.selected.is_empty());
    }

    #[test]
    fn test_co_selection_shared_model_added_once() {
        let inputs = make_inputs(
            vec![make_node(32_000, 16)],
            vec![make_model("shared-model", 4000)],
            vec![
                make_agent("agent-1", "shared-model", 512, 2),
                make_agent("agent-2", "shared-model", 512, 2),
            ],
            HashMap::from([
                ("agent-1".to_string(), 0.5),
                ("agent-2".to_string(), 0.5),
            ]),
        );

        let mut model_selection = SelectionResult {
            selected: vec![], // Model not selected
            total_ram_allocated_mb: 0,
            total_vram_allocated_mb: 0,
            exploration_model: None,
        };

        let mut agent_selection = AgentSelectionResult {
            selected: vec![
                SelectedAgent {
                    agent_id: "agent-1".to_string(),
                    instance_count: 1,
                    utility_score: 0.5,
                    required_model: "shared-model".to_string(),
                },
                SelectedAgent {
                    agent_id: "agent-2".to_string(),
                    instance_count: 1,
                    utility_score: 0.4,
                    required_model: "shared-model".to_string(),
                },
            ],
            total_ram_allocated_mb: 1024,
            total_cpu_cores_allocated: 4,
        };

        let actions = enforce_co_selection(&mut model_selection, &mut agent_selection, &inputs);

        // Model should be added only once
        assert_eq!(actions.len(), 1);
        match &actions[0] {
            CoSelectionAction::ModelAdded { model_id, .. } => {
                assert_eq!(model_id, "shared-model");
            }
            _ => panic!("Expected ModelAdded action"),
        }
        // Both agents should remain selected
        assert_eq!(agent_selection.selected.len(), 2);
        // Model RAM counted once
        assert_eq!(model_selection.total_ram_allocated_mb, 4000);
        // Only one model entry added
        assert_eq!(
            model_selection
                .selected
                .iter()
                .filter(|s| s.model_id == "shared-model")
                .count(),
            1
        );
    }

    // ─── assign_agents tests ─────────────────────────────────────────────────

    fn make_node_with_tools(ram_mb: u64, cores: u32, tools: Vec<&str>) -> NodeState {
        let node_id = uuid::Uuid::new_v4();
        let available_tools = tools
            .into_iter()
            .map(|t| crate::agents::tools::ToolCapability {
                tool_id: t.to_string(),
                tool_name: t.to_string(),
                category: crate::agents::tools::ToolCategory::CodeExecution,
                resource_requirements: crate::agents::tools::ToolResources::default(),
                is_available: true,
                version: "1.0.0".to_string(),
            })
            .collect();

        NodeState {
            capabilities: NodeCapabilities {
                node_id,
                hostname: "test".to_string(),
                device_type: DeviceType::Desktop,
                cpu: CpuProfile {
                    cores,
                    architecture: "x86_64".to_string(),
                    clock_mhz: 4000,
                    isa_extensions: vec![],
                },
                ram: RamProfile {
                    total_mb: ram_mb,
                    available_mb: ram_mb,
                    ddr_generation: 4,
                },
                gpu: None,
                storage: StorageProfile {
                    storage_type: StorageType::Nvme,
                    available_mb: 500000,
                    read_speed_mbps: 5000,
                },
                network_interfaces: vec![],
                phone_info: None,
                available_tools,
            },
            utilization: NodeUtilization::default(),
            loaded_models: vec![],
            stability_score: 0.95,
            last_heartbeat_ms: 0,
            is_online: true,
            latency_to_peers: HashMap::new(),
            thermal_state: ThermalState::default(),
        }
    }

    fn make_agent_with_tools(
        id: &str,
        required_model: &str,
        ram_mb: u64,
        cpu_cores: u32,
        tools: Vec<&str>,
    ) -> AgentEntry {
        AgentEntry {
            agent_id: id.to_string(),
            agent_name: id.to_string(),
            version: "1.0".to_string(),
            required_model: required_model.to_string(),
            tool_declarations: tools.into_iter().map(|t| t.to_string()).collect(),
            runtime_requirements: AgentRequirements {
                ram_mb,
                cpu_cores,
                disk_mb: 100,
            },
            download_sources: vec![],
            checksum_sha256: "test".to_string(),
        }
    }

    fn make_model_placement(model_id: &str, node_id: NodeId) -> ModelPlacement {
        ModelPlacement {
            model_id: model_id.to_string(),
            instance_id: uuid::Uuid::new_v4(),
            assigned_nodes: vec![node_id],
            protocol: ParallelismProtocol::SingleNode,
            estimated_tok_s: 20.0,
        }
    }

    #[test]
    fn test_assign_agents_empty_selection() {
        let config = SolverConfig::default();
        let selection = AgentSelectionResult {
            selected: vec![],
            total_ram_allocated_mb: 0,
            total_cpu_cores_allocated: 0,
        };

        let (placements, downloads, diagnostics) =
            assign_agents(&selection, &[], &[], &[], &config);

        assert!(placements.is_empty());
        assert!(downloads.is_empty());
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn test_assign_agents_basic_placement() {
        let config = SolverConfig::default();
        let node = make_node_with_tools(32_000, 16, vec!["browser", "filesystem"]);
        let node_id = node.capabilities.node_id;

        let agent = make_agent_with_tools("agent-1", "model-a", 512, 2, vec!["browser"]);
        let model_placement = make_model_placement("model-a", node_id);

        let selection = AgentSelectionResult {
            selected: vec![SelectedAgent {
                agent_id: "agent-1".to_string(),
                instance_count: 1,
                utility_score: 0.5,
                required_model: "model-a".to_string(),
            }],
            total_ram_allocated_mb: 512,
            total_cpu_cores_allocated: 2,
        };

        let (placements, _downloads, diagnostics) = assign_agents(
            &selection,
            &[node],
            &[model_placement],
            &[agent],
            &config,
        );

        assert_eq!(placements.len(), 1);
        assert_eq!(placements[0].agent_id, "agent-1");
        assert_eq!(placements[0].assigned_node, node_id);
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn test_assign_agents_tool_validation_rejects() {
        let config = SolverConfig::default();
        // Node only has "filesystem" tool
        let node = make_node_with_tools(32_000, 16, vec!["filesystem"]);
        let node_id = node.capabilities.node_id;

        // Agent requires "browser" which is not available
        let agent = make_agent_with_tools("agent-1", "model-a", 512, 2, vec!["browser"]);
        let model_placement = make_model_placement("model-a", node_id);

        let selection = AgentSelectionResult {
            selected: vec![SelectedAgent {
                agent_id: "agent-1".to_string(),
                instance_count: 1,
                utility_score: 0.5,
                required_model: "model-a".to_string(),
            }],
            total_ram_allocated_mb: 512,
            total_cpu_cores_allocated: 2,
        };

        let (placements, _downloads, diagnostics) = assign_agents(
            &selection,
            &[node],
            &[model_placement],
            &[agent],
            &config,
        );

        // Agent should be rejected — no node has required tools
        assert!(placements.is_empty());
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].resource_id, "agent-1");
        assert!(diagnostics[0].reason.contains("browser"));
    }

    #[test]
    fn test_assign_agents_ram_capacity_respected() {
        let config = SolverConfig::default();
        // Node with limited RAM (after 10% headroom: 900 usable)
        let node = make_node_with_tools(1000, 8, vec![]);
        let node_id = node.capabilities.node_id;

        // Agent needs 1000 MB RAM — won't fit after headroom
        let agent = make_agent_with_tools("agent-big", "model-a", 1000, 2, vec![]);
        let model_placement = make_model_placement("model-a", node_id);

        let selection = AgentSelectionResult {
            selected: vec![SelectedAgent {
                agent_id: "agent-big".to_string(),
                instance_count: 1,
                utility_score: 0.5,
                required_model: "model-a".to_string(),
            }],
            total_ram_allocated_mb: 1000,
            total_cpu_cores_allocated: 2,
        };

        let (placements, _downloads, _diagnostics) = assign_agents(
            &selection,
            &[node],
            &[model_placement],
            &[agent],
            &config,
        );

        // Agent should not be placed — insufficient RAM
        assert!(placements.is_empty());
    }

    #[test]
    fn test_assign_agents_co_location_preferred() {
        let config = SolverConfig::default();

        // Two nodes: one with model, one without
        let node_with_model = make_node_with_tools(32_000, 16, vec![]);
        let node_without_model = make_node_with_tools(32_000, 16, vec![]);
        let model_node_id = node_with_model.capabilities.node_id;

        let agent = make_agent_with_tools("agent-1", "model-a", 512, 2, vec![]);
        let model_placement = make_model_placement("model-a", model_node_id);

        // Add latency info so the second node passes proximity check
        let mut node_without = node_without_model.clone();
        node_without.latency_to_peers.insert(
            model_node_id,
            crate::network::registry::LatencyMeasurement {
                peer_id: model_node_id,
                rtt_ms: 5.0, // Low latency
                bandwidth_mbps: 1000.0,
                measured_at_ms: 0,
            },
        );

        let selection = AgentSelectionResult {
            selected: vec![SelectedAgent {
                agent_id: "agent-1".to_string(),
                instance_count: 1,
                utility_score: 0.5,
                required_model: "model-a".to_string(),
            }],
            total_ram_allocated_mb: 512,
            total_cpu_cores_allocated: 2,
        };

        let (placements, _downloads, _diagnostics) = assign_agents(
            &selection,
            &[node_with_model, node_without],
            &[model_placement],
            &[agent],
            &config,
        );

        // Agent should be placed on the node with the model (co-location bonus)
        assert_eq!(placements.len(), 1);
        assert_eq!(placements[0].assigned_node, model_node_id);
    }

    #[test]
    fn test_assign_agents_model_proximity_enforced() {
        let config = SolverConfig::default();

        // Two nodes: model on node A, agent candidate on node B with HIGH latency
        let node_a = make_node_with_tools(32_000, 16, vec![]);
        let mut node_b = make_node_with_tools(32_000, 16, vec![]);
        let node_a_id = node_a.capabilities.node_id;

        // Node B has very high latency to node A (exceeds pipeline_parallel_max_latency_ms=50)
        node_b.latency_to_peers.insert(
            node_a_id,
            crate::network::registry::LatencyMeasurement {
                peer_id: node_a_id,
                rtt_ms: 200.0, // Way above 50ms threshold
                bandwidth_mbps: 100.0,
                measured_at_ms: 0,
            },
        );

        let agent = make_agent_with_tools("agent-1", "model-a", 512, 2, vec![]);
        let model_placement = make_model_placement("model-a", node_a_id);

        let selection = AgentSelectionResult {
            selected: vec![SelectedAgent {
                agent_id: "agent-1".to_string(),
                instance_count: 1,
                utility_score: 0.5,
                required_model: "model-a".to_string(),
            }],
            total_ram_allocated_mb: 512,
            total_cpu_cores_allocated: 2,
        };

        let (placements, _downloads, _diagnostics) = assign_agents(
            &selection,
            &[node_a, node_b],
            &[model_placement],
            &[agent],
            &config,
        );

        // Agent should be placed on node_a (co-located with model)
        // node_b fails proximity check due to high latency
        assert_eq!(placements.len(), 1);
        assert_eq!(placements[0].assigned_node, node_a_id);
    }

    #[test]
    fn test_assign_agents_battery_constraint() {
        let config = SolverConfig::default();

        // Phone node with low battery
        let mut node = make_node_with_tools(8_000, 4, vec![]);
        let node_id = node.capabilities.node_id;
        node.capabilities.phone_info = Some(PhoneInfo {
            os: PhoneOs::Android,
            npu: None,
            battery_percent: 10, // Below 20% threshold
            is_charging: false,
            connection_type: ConnectionType::Wifi,
        });

        let agent = make_agent_with_tools("agent-1", "model-a", 512, 2, vec![]);
        let model_placement = make_model_placement("model-a", node_id);

        let selection = AgentSelectionResult {
            selected: vec![SelectedAgent {
                agent_id: "agent-1".to_string(),
                instance_count: 1,
                utility_score: 0.5,
                required_model: "model-a".to_string(),
            }],
            total_ram_allocated_mb: 512,
            total_cpu_cores_allocated: 2,
        };

        let (placements, _downloads, _diagnostics) = assign_agents(
            &selection,
            &[node],
            &[model_placement],
            &[agent],
            &config,
        );

        // Agent should not be placed — battery too low
        assert!(placements.is_empty());
    }

    #[test]
    fn test_assign_agents_download_plan_generated() {
        let config = SolverConfig::default();
        let node = make_node_with_tools(32_000, 16, vec![]);
        let node_id = node.capabilities.node_id;

        let agent = make_agent_with_tools("agent-1", "model-a", 512, 2, vec![]);
        let model_placement = make_model_placement("model-a", node_id);

        let selection = AgentSelectionResult {
            selected: vec![SelectedAgent {
                agent_id: "agent-1".to_string(),
                instance_count: 1,
                utility_score: 0.5,
                required_model: "model-a".to_string(),
            }],
            total_ram_allocated_mb: 512,
            total_cpu_cores_allocated: 2,
        };

        let (placements, downloads, _diagnostics) = assign_agents(
            &selection,
            &[node],
            &[model_placement],
            &[agent],
            &config,
        );

        // Agent placed successfully
        assert_eq!(placements.len(), 1);
        // Agent runtime not on node → download emitted
        assert!(!downloads.is_empty());
        let agent_download = downloads
            .iter()
            .find(|d| d.resource_type == ResourceType::Agent);
        assert!(agent_download.is_some());
        assert_eq!(
            agent_download.unwrap().resource_id,
            "agent:agent-1"
        );
    }

    #[test]
    fn test_assign_agents_model_download_with_dependency() {
        let config = SolverConfig::default();
        let node = make_node_with_tools(32_000, 16, vec![]);
        let _node_id = node.capabilities.node_id;

        let agent = make_agent_with_tools("agent-1", "model-a", 512, 2, vec![]);

        // Model placed on a DIFFERENT node (not on agent's node)
        let other_node = make_node_with_tools(32_000, 16, vec![]);
        let other_node_id = other_node.capabilities.node_id;
        let model_placement = make_model_placement("model-a", other_node_id);

        // Add latency so agent node can pass proximity check
        let mut node_with_latency = node.clone();
        node_with_latency.latency_to_peers.insert(
            other_node_id,
            crate::network::registry::LatencyMeasurement {
                peer_id: other_node_id,
                rtt_ms: 5.0,
                bandwidth_mbps: 1000.0,
                measured_at_ms: 0,
            },
        );

        let selection = AgentSelectionResult {
            selected: vec![SelectedAgent {
                agent_id: "agent-1".to_string(),
                instance_count: 1,
                utility_score: 0.5,
                required_model: "model-a".to_string(),
            }],
            total_ram_allocated_mb: 512,
            total_cpu_cores_allocated: 2,
        };

        let (placements, downloads, _diagnostics) = assign_agents(
            &selection,
            &[node_with_latency, other_node],
            &[model_placement],
            &[agent],
            &config,
        );

        assert_eq!(placements.len(), 1);

        // Should have model download (higher priority) and agent download
        let model_download = downloads
            .iter()
            .find(|d| d.resource_type == ResourceType::Model);
        let agent_download = downloads
            .iter()
            .find(|d| d.resource_type == ResourceType::Agent);

        assert!(model_download.is_some());
        assert!(agent_download.is_some());

        // Model download has higher priority
        assert_eq!(model_download.unwrap().priority, DownloadPriority::High);
        assert_eq!(agent_download.unwrap().priority, DownloadPriority::Normal);

        // Agent download depends on model download
        assert!(agent_download
            .unwrap()
            .depends_on
            .contains(&"model:model-a".to_string()));
    }

    #[test]
    fn test_assign_agents_multiple_instances() {
        let config = SolverConfig::default();
        let node = make_node_with_tools(32_000, 16, vec![]);
        let node_id = node.capabilities.node_id;

        let agent = make_agent_with_tools("agent-1", "model-a", 512, 2, vec![]);
        let model_placement = make_model_placement("model-a", node_id);

        let selection = AgentSelectionResult {
            selected: vec![SelectedAgent {
                agent_id: "agent-1".to_string(),
                instance_count: 3, // 3 instances
                utility_score: 0.5,
                required_model: "model-a".to_string(),
            }],
            total_ram_allocated_mb: 1536,
            total_cpu_cores_allocated: 6,
        };

        let (placements, _downloads, _diagnostics) = assign_agents(
            &selection,
            &[node],
            &[model_placement],
            &[agent],
            &config,
        );

        // All 3 instances should be placed
        assert_eq!(placements.len(), 3);
        assert!(placements.iter().all(|p| p.agent_id == "agent-1"));
        assert!(placements.iter().all(|p| p.assigned_node == node_id));
    }

    #[test]
    fn test_assign_agents_cpu_capacity_respected() {
        let config = SolverConfig::default();
        // Node with 4 cores, cpu_headroom_percent=0.80 → 3 usable cores
        let node = make_node_with_tools(32_000, 4, vec![]);
        let node_id = node.capabilities.node_id;

        // Agent needs 4 CPU cores — won't fit (only 3 usable)
        let agent = make_agent_with_tools("agent-big", "model-a", 512, 4, vec![]);
        let model_placement = make_model_placement("model-a", node_id);

        let selection = AgentSelectionResult {
            selected: vec![SelectedAgent {
                agent_id: "agent-big".to_string(),
                instance_count: 1,
                utility_score: 0.5,
                required_model: "model-a".to_string(),
            }],
            total_ram_allocated_mb: 512,
            total_cpu_cores_allocated: 4,
        };

        let (placements, _downloads, _diagnostics) = assign_agents(
            &selection,
            &[node],
            &[model_placement],
            &[agent],
            &config,
        );

        // Agent should not be placed — insufficient CPU
        assert!(placements.is_empty());
    }

    #[test]
    fn test_assign_agents_no_tools_agent_placed_anywhere() {
        let config = SolverConfig::default();
        let node = make_node_with_tools(32_000, 16, vec![]);
        let node_id = node.capabilities.node_id;

        // Agent with no tool requirements
        let agent = make_agent_with_tools("agent-1", "model-a", 512, 2, vec![]);
        let model_placement = make_model_placement("model-a", node_id);

        let selection = AgentSelectionResult {
            selected: vec![SelectedAgent {
                agent_id: "agent-1".to_string(),
                instance_count: 1,
                utility_score: 0.5,
                required_model: "model-a".to_string(),
            }],
            total_ram_allocated_mb: 512,
            total_cpu_cores_allocated: 2,
        };

        let (placements, _downloads, diagnostics) = assign_agents(
            &selection,
            &[node],
            &[model_placement],
            &[agent],
            &config,
        );

        // Agent with no tool requirements should be placed
        assert_eq!(placements.len(), 1);
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn test_assign_agents_latency_bonus_scoring() {
        let config = SolverConfig::default();

        // Node A: has model
        let node_a = make_node_with_tools(32_000, 16, vec![]);
        let node_a_id = node_a.capabilities.node_id;

        // Node B: low latency to node A
        let mut node_b = make_node_with_tools(32_000, 16, vec![]);
        node_b.latency_to_peers.insert(
            node_a_id,
            crate::network::registry::LatencyMeasurement {
                peer_id: node_a_id,
                rtt_ms: 10.0, // Low latency (< 50ms threshold)
                bandwidth_mbps: 1000.0,
                measured_at_ms: 0,
            },
        );

        // Fill node A so agent can't fit there
        let mut node_a_full = node_a.clone();
        node_a_full.utilization.ram_used_mb = 31_000; // Almost full

        let agent = make_agent_with_tools("agent-1", "model-a", 512, 2, vec![]);
        let model_placement = make_model_placement("model-a", node_a_id);

        let selection = AgentSelectionResult {
            selected: vec![SelectedAgent {
                agent_id: "agent-1".to_string(),
                instance_count: 1,
                utility_score: 0.5,
                required_model: "model-a".to_string(),
            }],
            total_ram_allocated_mb: 512,
            total_cpu_cores_allocated: 2,
        };

        let (placements, _downloads, _diagnostics) = assign_agents(
            &selection,
            &[node_a_full, node_b.clone()],
            &[model_placement],
            &[agent],
            &config,
        );

        // Agent should be placed on node B (low-latency peer, since node A is full)
        assert_eq!(placements.len(), 1);
        assert_eq!(placements[0].assigned_node, node_b.capabilities.node_id);
    }

    // ─── compute_parallelism_factor tests (Task 5.2) ─────────────────────────

    #[test]
    fn test_parallelism_factor_basic() {
        let config = SolverConfig::default();

        // 5 independent steps out of 10, low latency, equal speed nodes
        let result = compute_parallelism_factor(
            5,   // independent_steps
            10,  // total_steps
            5.0, // avg_network_latency_ms
            100.0, // step_compute_time_ms
            64000.0, // min_node_speed
            64000.0, // max_node_speed (same = ratio 1.0)
            &config,
        );

        // step_ratio = 5/10 = 0.5
        // latency_factor = 1 - 5/100 = 0.95
        // speed_factor = 64000/64000 = 1.0
        // result = 0.5 * 0.95 * 1.0 = 0.475
        assert!((result - 0.475).abs() < 1e-10);
    }

    #[test]
    fn test_parallelism_factor_clamped_to_zero_one() {
        let config = SolverConfig::default();

        // All steps independent, zero latency, equal speed
        let result = compute_parallelism_factor(10, 10, 0.0, 100.0, 64000.0, 64000.0, &config);
        assert!(result >= 0.0 && result <= 1.0);
        assert!((result - 1.0).abs() < 1e-10);

        // High latency exceeding step_compute_time → negative latency_factor → clamped to 0
        let result = compute_parallelism_factor(5, 10, 200.0, 100.0, 64000.0, 64000.0, &config);
        assert_eq!(result, 0.0);
    }

    #[test]
    fn test_parallelism_factor_speed_ratio_rejection() {
        let config = SolverConfig::default(); // speed_ratio_threshold = 3.0

        // Speed ratio = 128000/32000 = 4.0 > 3.0 → return 0.0
        let result = compute_parallelism_factor(5, 10, 5.0, 100.0, 32000.0, 128000.0, &config);
        assert_eq!(result, 0.0);
    }

    #[test]
    fn test_parallelism_factor_speed_ratio_at_threshold() {
        let config = SolverConfig::default(); // speed_ratio_threshold = 3.0

        // Speed ratio = 96000/32000 = 3.0 (exactly at threshold, not exceeding)
        let result = compute_parallelism_factor(5, 10, 5.0, 100.0, 32000.0, 96000.0, &config);
        // 3.0 is not > 3.0, so parallelism is allowed
        assert!(result > 0.0);
    }

    #[test]
    fn test_parallelism_factor_zero_total_steps() {
        let config = SolverConfig::default();
        let result = compute_parallelism_factor(0, 0, 5.0, 100.0, 64000.0, 64000.0, &config);
        assert_eq!(result, 0.0);
    }

    #[test]
    fn test_parallelism_factor_zero_compute_time() {
        let config = SolverConfig::default();
        let result = compute_parallelism_factor(5, 10, 5.0, 0.0, 64000.0, 64000.0, &config);
        assert_eq!(result, 0.0);
    }

    #[test]
    fn test_parallelism_factor_zero_min_speed() {
        let config = SolverConfig::default();
        let result = compute_parallelism_factor(5, 10, 5.0, 100.0, 0.0, 64000.0, &config);
        assert_eq!(result, 0.0);
    }

    // ─── compute_agent_utility tests (Task 5.3) ──────────────────────────────

    #[test]
    fn test_agent_utility_no_agents_returns_zero() {
        let config = SolverConfig::default();
        let node = make_node(32_000, 16);

        let result = compute_agent_utility(&[], &[node], &config);
        assert_eq!(result, 0.0);
    }

    #[test]
    fn test_agent_utility_single_agent() {
        let config = SolverConfig::default();
        let node = make_node(32_000, 16);
        let node_id = node.capabilities.node_id;

        let placement = AgentPlacement {
            agent_id: "agent-1".to_string(),
            instance_id: uuid::Uuid::new_v4(),
            assigned_node: node_id,
            required_model_instance_id: uuid::Uuid::new_v4(),
            estimated_throughput: 50.0, // 50 steps/minute
            resource_allocation: AgentRequirements {
                ram_mb: 512,
                cpu_cores: 2,
                disk_mb: 100,
            },
        };

        let result = compute_agent_utility(&[placement], &[node], &config);
        // Should be positive (throughput * parallelism_factor)
        assert!(result > 0.0);
    }

    #[test]
    fn test_agent_utility_multiple_agents_additive() {
        let config = SolverConfig::default();
        let node = make_node(32_000, 16);
        let node_id = node.capabilities.node_id;

        let placement1 = AgentPlacement {
            agent_id: "agent-1".to_string(),
            instance_id: uuid::Uuid::new_v4(),
            assigned_node: node_id,
            required_model_instance_id: uuid::Uuid::new_v4(),
            estimated_throughput: 50.0,
            resource_allocation: AgentRequirements {
                ram_mb: 512,
                cpu_cores: 2,
                disk_mb: 100,
            },
        };

        let single_utility = compute_agent_utility(&[placement1.clone()], &[node.clone()], &config);

        let placement2 = AgentPlacement {
            agent_id: "agent-2".to_string(),
            instance_id: uuid::Uuid::new_v4(),
            assigned_node: node_id,
            required_model_instance_id: uuid::Uuid::new_v4(),
            estimated_throughput: 50.0,
            resource_allocation: AgentRequirements {
                ram_mb: 512,
                cpu_cores: 2,
                disk_mb: 100,
            },
        };

        let double_utility = compute_agent_utility(&[placement1, placement2], &[node], &config);
        // Two agents with same throughput should give ~2x utility
        assert!(double_utility > single_utility);
    }

    // ─── compute_load_distribution tests (Task 5.5) ──────────────────────────

    #[test]
    fn test_load_distribution_empty() {
        let config = SolverConfig::default();
        let result = compute_load_distribution(&[], &[], &config);
        assert!(result.is_empty());
    }

    #[test]
    fn test_load_distribution_single_node() {
        let config = SolverConfig::default();
        let node = make_node(32_000, 16);
        let node_id = node.capabilities.node_id;

        let placement = AgentPlacement {
            agent_id: "agent-1".to_string(),
            instance_id: uuid::Uuid::new_v4(),
            assigned_node: node_id,
            required_model_instance_id: uuid::Uuid::new_v4(),
            estimated_throughput: 50.0,
            resource_allocation: AgentRequirements {
                ram_mb: 512,
                cpu_cores: 2,
                disk_mb: 100,
            },
        };

        let result = compute_load_distribution(&[placement], &[node], &config);
        assert_eq!(result.len(), 1);
        assert!((result[0].load_fraction - 1.0).abs() < 1e-10);
        assert!(result[0].is_preferred_for_compute);
        assert!(result[0].is_preferred_for_tools);
    }

    #[test]
    fn test_load_distribution_proportional_to_speed() {
        let config = SolverConfig::default();

        // Fast node: 4000 MHz * 16 cores = 64000
        let fast_node = make_node(32_000, 16);
        let fast_node_id = fast_node.capabilities.node_id;

        // Slow node: 2000 MHz * 4 cores = 8000
        let mut slow_node = make_node(32_000, 4);
        slow_node.capabilities.cpu.clock_mhz = 2000;
        let slow_node_id = slow_node.capabilities.node_id;

        let placement1 = AgentPlacement {
            agent_id: "agent-1".to_string(),
            instance_id: uuid::Uuid::new_v4(),
            assigned_node: fast_node_id,
            required_model_instance_id: uuid::Uuid::new_v4(),
            estimated_throughput: 50.0,
            resource_allocation: AgentRequirements {
                ram_mb: 512,
                cpu_cores: 2,
                disk_mb: 100,
            },
        };

        let placement2 = AgentPlacement {
            agent_id: "agent-2".to_string(),
            instance_id: uuid::Uuid::new_v4(),
            assigned_node: slow_node_id,
            required_model_instance_id: uuid::Uuid::new_v4(),
            estimated_throughput: 30.0,
            resource_allocation: AgentRequirements {
                ram_mb: 512,
                cpu_cores: 2,
                disk_mb: 100,
            },
        };

        let result = compute_load_distribution(
            &[placement1, placement2],
            &[fast_node, slow_node],
            &config,
        );

        assert_eq!(result.len(), 2);

        // Fast node should get more load
        let fast_assignment = result.iter().find(|a| a.node_id == fast_node_id).unwrap();
        let slow_assignment = result.iter().find(|a| a.node_id == slow_node_id).unwrap();

        assert!(fast_assignment.load_fraction > slow_assignment.load_fraction);
        // Fast node: 64000/(64000+8000) = 64000/72000 ≈ 0.889
        assert!((fast_assignment.load_fraction - 64000.0 / 72000.0).abs() < 1e-10);
        assert!(fast_assignment.is_preferred_for_compute);
        assert!(!slow_assignment.is_preferred_for_compute);
    }

    #[test]
    fn test_load_distribution_least_loaded_for_tools() {
        let config = SolverConfig::default();

        // Node A: heavily loaded
        let mut node_a = make_node(32_000, 16);
        node_a.utilization.ram_used_mb = 28_000;
        node_a.utilization.cpu_percent = 80.0;
        let node_a_id = node_a.capabilities.node_id;

        // Node B: lightly loaded
        let mut node_b = make_node(32_000, 16);
        node_b.utilization.ram_used_mb = 4_000;
        node_b.utilization.cpu_percent = 10.0;
        let node_b_id = node_b.capabilities.node_id;

        let placement1 = AgentPlacement {
            agent_id: "agent-1".to_string(),
            instance_id: uuid::Uuid::new_v4(),
            assigned_node: node_a_id,
            required_model_instance_id: uuid::Uuid::new_v4(),
            estimated_throughput: 50.0,
            resource_allocation: AgentRequirements {
                ram_mb: 512,
                cpu_cores: 2,
                disk_mb: 100,
            },
        };

        let placement2 = AgentPlacement {
            agent_id: "agent-2".to_string(),
            instance_id: uuid::Uuid::new_v4(),
            assigned_node: node_b_id,
            required_model_instance_id: uuid::Uuid::new_v4(),
            estimated_throughput: 50.0,
            resource_allocation: AgentRequirements {
                ram_mb: 512,
                cpu_cores: 2,
                disk_mb: 100,
            },
        };

        let result = compute_load_distribution(
            &[placement1, placement2],
            &[node_a, node_b],
            &config,
        );

        let node_b_assignment = result.iter().find(|a| a.node_id == node_b_id).unwrap();
        let node_a_assignment = result.iter().find(|a| a.node_id == node_a_id).unwrap();

        // Node B (less loaded) should be preferred for tools
        assert!(node_b_assignment.is_preferred_for_tools);
        assert!(!node_a_assignment.is_preferred_for_tools);
    }

    // ─── Task 8.1: Device-Agnostic Constraint Tests ──────────────────────────

    #[test]
    fn test_same_agent_placed_on_desktop_laptop_phone_nodes() {
        // Verifies that the same agent can be placed on Desktop, Laptop, and Phone nodes
        // as long as constraints are met — no device-type branching in scheduling logic.
        let config = SolverConfig::default();

        // Create three nodes with different device types but same capabilities
        let mut desktop_node = make_node_with_tools(32_000, 16, vec!["browser"]);
        desktop_node.capabilities.device_type = DeviceType::Desktop;
        let desktop_id = desktop_node.capabilities.node_id;

        let mut laptop_node = make_node_with_tools(32_000, 16, vec!["browser"]);
        laptop_node.capabilities.device_type = DeviceType::Laptop;
        let laptop_id = laptop_node.capabilities.node_id;

        let mut phone_node = make_node_with_tools(32_000, 16, vec!["browser"]);
        phone_node.capabilities.device_type = DeviceType::Phone;
        phone_node.capabilities.phone_info = Some(PhoneInfo {
            os: PhoneOs::Android,
            npu: None,
            battery_percent: 80, // Good battery
            is_charging: false,
            connection_type: ConnectionType::Wifi,
        });
        let phone_id = phone_node.capabilities.node_id;

        let agent = make_agent_with_tools("agent-1", "model-a", 512, 2, vec!["browser"]);

        // Test placement on each node individually
        for (node, expected_id) in [
            (desktop_node, desktop_id),
            (laptop_node, laptop_id),
            (phone_node, phone_id),
        ] {
            let model_placement = make_model_placement("model-a", expected_id);

            let selection = AgentSelectionResult {
                selected: vec![SelectedAgent {
                    agent_id: "agent-1".to_string(),
                    instance_count: 1,
                    utility_score: 0.5,
                    required_model: "model-a".to_string(),
                }],
                total_ram_allocated_mb: 512,
                total_cpu_cores_allocated: 2,
            };

            let (placements, _downloads, diagnostics) = assign_agents(
                &selection,
                &[node],
                &[model_placement],
                &[agent.clone()],
                &config,
            );

            assert_eq!(
                placements.len(), 1,
                "Agent should be placed on node with device_type {:?}",
                expected_id
            );
            assert_eq!(placements[0].assigned_node, expected_id);
            assert!(diagnostics.is_empty());
        }
    }

    #[test]
    fn test_no_device_type_branching_in_scheduling() {
        // Verify that device_type is never used as a scheduling criterion.
        // Two nodes with identical capabilities but different device_types
        // should produce identical placement decisions.
        let config = SolverConfig::default();

        let mut node_desktop = make_node_with_tools(32_000, 16, vec![]);
        node_desktop.capabilities.device_type = DeviceType::Desktop;
        let desktop_id = node_desktop.capabilities.node_id;

        let mut node_server = make_node_with_tools(32_000, 16, vec![]);
        node_server.capabilities.device_type = DeviceType::Server;
        let _server_id = node_server.capabilities.node_id;

        let agent = make_agent_with_tools("agent-1", "model-a", 512, 2, vec![]);

        // Place model on desktop node
        let model_placement = make_model_placement("model-a", desktop_id);

        // Add latency so server passes proximity check
        node_server.latency_to_peers.insert(
            desktop_id,
            crate::network::registry::LatencyMeasurement {
                peer_id: desktop_id,
                rtt_ms: 5.0,
                bandwidth_mbps: 1000.0,
                measured_at_ms: 0,
            },
        );

        let selection = AgentSelectionResult {
            selected: vec![SelectedAgent {
                agent_id: "agent-1".to_string(),
                instance_count: 1,
                utility_score: 0.5,
                required_model: "model-a".to_string(),
            }],
            total_ram_allocated_mb: 512,
            total_cpu_cores_allocated: 2,
        };

        let (placements, _, _) = assign_agents(
            &selection,
            &[node_desktop, node_server],
            &[model_placement],
            &[agent],
            &config,
        );

        // Agent should be placed (on desktop due to co-location bonus)
        assert_eq!(placements.len(), 1);
        // The key point: placement decision was based on co-location, not device_type
        assert_eq!(placements[0].assigned_node, desktop_id);
    }

    // ─── Task 8.2: Battery and Thermal Constraint Tests ──────────────────────

    #[test]
    fn test_battery_constraint_low_battery_excluded() {
        // Node with low battery (< 20%) and not charging should be excluded
        let mut node = make_node(32_000, 16);
        node.capabilities.phone_info = Some(PhoneInfo {
            os: PhoneOs::Android,
            npu: None,
            battery_percent: 15,
            is_charging: false,
            connection_type: ConnectionType::Wifi,
        });

        assert!(!passes_battery_thermal_constraints(&node));
    }

    #[test]
    fn test_battery_constraint_low_battery_charging_allowed() {
        // Node with low battery but charging should be allowed
        let mut node = make_node(32_000, 16);
        node.capabilities.phone_info = Some(PhoneInfo {
            os: PhoneOs::Android,
            npu: None,
            battery_percent: 10,
            is_charging: true, // Charging overrides low battery
            connection_type: ConnectionType::Wifi,
        });

        assert!(passes_battery_thermal_constraints(&node));
    }

    #[test]
    fn test_battery_constraint_sufficient_battery_allowed() {
        // Node with >= 20% battery should be allowed
        let mut node = make_node(32_000, 16);
        node.capabilities.phone_info = Some(PhoneInfo {
            os: PhoneOs::Android,
            npu: None,
            battery_percent: 25,
            is_charging: false,
            connection_type: ConnectionType::Wifi,
        });

        assert!(passes_battery_thermal_constraints(&node));
    }

    #[test]
    fn test_battery_constraint_desktop_no_battery_always_passes() {
        // Desktop nodes without phone_info always pass battery check
        let node = make_node(32_000, 16);
        assert!(node.capabilities.phone_info.is_none());
        assert!(passes_battery_thermal_constraints(&node));
    }

    #[test]
    fn test_thermal_constraint_critical_excluded() {
        // Node with Critical thermal state should be excluded
        let mut node = make_node(32_000, 16);
        node.thermal_state = ThermalState::Critical;

        assert!(!passes_battery_thermal_constraints(&node));
    }

    #[test]
    fn test_thermal_constraint_normal_allowed() {
        let mut node = make_node(32_000, 16);
        node.thermal_state = ThermalState::Normal;

        assert!(passes_battery_thermal_constraints(&node));
    }

    #[test]
    fn test_thermal_constraint_warm_allowed() {
        let mut node = make_node(32_000, 16);
        node.thermal_state = ThermalState::Warm;

        assert!(passes_battery_thermal_constraints(&node));
    }

    #[test]
    fn test_thermal_heuristic_high_cpu_phone_excluded() {
        // Phone with CPU > 95% treated as thermal risk
        let mut node = make_node(32_000, 16);
        node.capabilities.phone_info = Some(PhoneInfo {
            os: PhoneOs::Android,
            npu: None,
            battery_percent: 80,
            is_charging: false,
            connection_type: ConnectionType::Wifi,
        });
        node.utilization.cpu_percent = 96.0;

        assert!(!passes_battery_thermal_constraints(&node));
    }

    #[test]
    fn test_battery_thermal_combined_constraints_in_placement() {
        // Verify that battery/thermal constraints are applied during agent placement
        let config = SolverConfig::default();

        // Node with Critical thermal state
        let mut critical_node = make_node_with_tools(32_000, 16, vec![]);
        critical_node.thermal_state = ThermalState::Critical;
        let critical_id = critical_node.capabilities.node_id;

        let agent = make_agent_with_tools("agent-1", "model-a", 512, 2, vec![]);
        let model_placement = make_model_placement("model-a", critical_id);

        let selection = AgentSelectionResult {
            selected: vec![SelectedAgent {
                agent_id: "agent-1".to_string(),
                instance_count: 1,
                utility_score: 0.5,
                required_model: "model-a".to_string(),
            }],
            total_ram_allocated_mb: 512,
            total_cpu_cores_allocated: 2,
        };

        let (placements, _, _) = assign_agents(
            &selection,
            &[critical_node],
            &[model_placement],
            &[agent],
            &config,
        );

        // Agent should NOT be placed on a Critical thermal node
        assert!(placements.is_empty());
    }

    // ─── Task 8.3: Re-Solve Trigger on Tool Status Change Tests ──────────────

    #[test]
    fn test_should_re_solve_no_agents_returns_false() {
        let nodes = vec![make_node(32_000, 16)];
        let catalog: Vec<AgentEntry> = vec![];
        let placements: Vec<AgentPlacement> = vec![];

        assert!(!should_re_solve(&placements, &catalog, &nodes));
    }

    #[test]
    fn test_should_re_solve_tools_still_available_returns_false() {
        let node = make_node_with_tools(32_000, 16, vec!["browser", "filesystem"]);
        let node_id = node.capabilities.node_id;

        let agent = make_agent_with_tools("agent-1", "model-a", 512, 2, vec!["browser"]);
        let placement = AgentPlacement {
            agent_id: "agent-1".to_string(),
            instance_id: uuid::Uuid::new_v4(),
            assigned_node: node_id,
            required_model_instance_id: uuid::Uuid::new_v4(),
            estimated_throughput: 50.0,
            resource_allocation: AgentRequirements {
                ram_mb: 512,
                cpu_cores: 2,
                disk_mb: 100,
            },
        };

        assert!(!should_re_solve(&[placement], &[agent], &[node]));
    }

    #[test]
    fn test_should_re_solve_tool_became_unavailable_returns_true() {
        // Node originally had "browser" tool, but it's now marked unavailable
        let mut node = make_node_with_tools(32_000, 16, vec!["browser", "filesystem"]);
        let node_id = node.capabilities.node_id;

        // Mark "browser" as unavailable
        for tool in &mut node.capabilities.available_tools {
            if tool.tool_id == "browser" {
                tool.is_available = false;
            }
        }

        let agent = make_agent_with_tools("agent-1", "model-a", 512, 2, vec!["browser"]);
        let placement = AgentPlacement {
            agent_id: "agent-1".to_string(),
            instance_id: uuid::Uuid::new_v4(),
            assigned_node: node_id,
            required_model_instance_id: uuid::Uuid::new_v4(),
            estimated_throughput: 50.0,
            resource_allocation: AgentRequirements {
                ram_mb: 512,
                cpu_cores: 2,
                disk_mb: 100,
            },
        };

        assert!(should_re_solve(&[placement], &[agent], &[node]));
    }

    #[test]
    fn test_check_tool_availability_returns_diagnostics() {
        // Node has "filesystem" but not "browser" anymore
        let node = make_node_with_tools(32_000, 16, vec!["filesystem"]);
        let node_id = node.capabilities.node_id;

        let agent = make_agent_with_tools("agent-1", "model-a", 512, 2, vec!["browser"]);
        let placement = AgentPlacement {
            agent_id: "agent-1".to_string(),
            instance_id: uuid::Uuid::new_v4(),
            assigned_node: node_id,
            required_model_instance_id: uuid::Uuid::new_v4(),
            estimated_throughput: 50.0,
            resource_allocation: AgentRequirements {
                ram_mb: 512,
                cpu_cores: 2,
                disk_mb: 100,
            },
        };

        let result = check_tool_availability(&[placement], &[agent], &[node]);

        assert!(result.needs_re_solve);
        assert_eq!(result.invalid_placements.len(), 1);
        assert_eq!(result.invalid_placements[0].agent_id, "agent-1");
        assert_eq!(result.diagnostics.len(), 1);
        assert!(result.diagnostics[0].reason.contains("browser"));
        assert!(result.diagnostics[0].reason.contains("no longer available"));
    }

    #[test]
    fn test_check_tool_availability_node_gone_offline() {
        // Agent placed on a node that no longer exists in the node list
        let agent = make_agent_with_tools("agent-1", "model-a", 512, 2, vec!["browser"]);
        let placement = AgentPlacement {
            agent_id: "agent-1".to_string(),
            instance_id: uuid::Uuid::new_v4(),
            assigned_node: uuid::Uuid::new_v4(), // Non-existent node
            required_model_instance_id: uuid::Uuid::new_v4(),
            estimated_throughput: 50.0,
            resource_allocation: AgentRequirements {
                ram_mb: 512,
                cpu_cores: 2,
                disk_mb: 100,
            },
        };

        let result = check_tool_availability(&[placement], &[agent], &[]);

        assert!(result.needs_re_solve);
        assert_eq!(result.invalid_placements.len(), 1);
        assert!(result.diagnostics[0].reason.contains("no longer available"));
    }

    #[test]
    fn test_check_tool_availability_agent_no_tools_always_valid() {
        // Agent with no tool requirements is always valid
        let node = make_node_with_tools(32_000, 16, vec![]);
        let node_id = node.capabilities.node_id;

        let agent = make_agent_with_tools("agent-1", "model-a", 512, 2, vec![]);
        let placement = AgentPlacement {
            agent_id: "agent-1".to_string(),
            instance_id: uuid::Uuid::new_v4(),
            assigned_node: node_id,
            required_model_instance_id: uuid::Uuid::new_v4(),
            estimated_throughput: 50.0,
            resource_allocation: AgentRequirements {
                ram_mb: 512,
                cpu_cores: 2,
                disk_mb: 100,
            },
        };

        let result = check_tool_availability(&[placement], &[agent], &[node]);
        assert!(!result.needs_re_solve);
        assert!(result.invalid_placements.is_empty());
    }

    // ─── Task 9.1: Performance Benchmarks ────────────────────────────────────

    #[test]
    fn test_benchmark_small_network_10_nodes_50_models_20_agents() {
        // Performance benchmark: 10 nodes, 50 models, 20 agents → solve < 500ms
        use crate::network::solver::solve;
        use std::time::Instant;

        let nodes: Vec<NodeState> = (0..10)
            .map(|i| {
                let mut node = make_node(32_000 + i * 1000, 8 + (i as u32 % 4));
                node.capabilities.hostname = format!("node-{}", i);
                node
            })
            .collect();

        let models: Vec<ModelEntry> = (0..50)
            .map(|i| make_model(&format!("model-{}", i), 512 + (i as u64 * 100)))
            .collect();

        let agents: Vec<AgentEntry> = (0..20)
            .map(|i| {
                let model_idx = i % 50;
                make_agent(
                    &format!("agent-{}", i),
                    &format!("model-{}", model_idx),
                    256 + (i as u64 * 50),
                    1 + (i as u32 % 3),
                )
            })
            .collect();

        let agent_shares: HashMap<AgentId, f64> = agents
            .iter()
            .enumerate()
            .map(|(i, a)| (a.agent_id.clone(), 0.05 * (1.0 + i as f64 * 0.01)))
            .collect();

        let inputs = make_inputs(nodes, models, agents, agent_shares);
        let config = SolverConfig::default();

        let start = Instant::now();
        let _plan = solve(&inputs, &config, 1000);
        let elapsed = start.elapsed();

        assert!(
            elapsed.as_millis() < 500,
            "Small network solve took {}ms, expected < 500ms",
            elapsed.as_millis()
        );
    }

    #[test]
    fn test_benchmark_large_network_50_nodes_200_models_100_agents() {
        // Performance benchmark: 50 nodes, 200 models, 100 agents → solve < 2000ms
        use crate::network::solver::solve;
        use std::time::Instant;

        let nodes: Vec<NodeState> = (0..50)
            .map(|i| {
                let mut node = make_node(16_000 + i * 500, 4 + (i as u32 % 8));
                node.capabilities.hostname = format!("node-{}", i);
                node
            })
            .collect();

        let models: Vec<ModelEntry> = (0..200)
            .map(|i| make_model(&format!("model-{}", i), 256 + (i as u64 * 50)))
            .collect();

        let agents: Vec<AgentEntry> = (0..100)
            .map(|i| {
                let model_idx = i % 200;
                make_agent(
                    &format!("agent-{}", i),
                    &format!("model-{}", model_idx),
                    128 + (i as u64 * 30),
                    1 + (i as u32 % 2),
                )
            })
            .collect();

        let agent_shares: HashMap<AgentId, f64> = agents
            .iter()
            .enumerate()
            .map(|(i, a)| (a.agent_id.clone(), 0.01 * (1.0 + i as f64 * 0.005)))
            .collect();

        let inputs = make_inputs(nodes, models, agents, agent_shares);
        let config = SolverConfig::default();

        let start = Instant::now();
        let _plan = solve(&inputs, &config, 1000);
        let elapsed = start.elapsed();

        assert!(
            elapsed.as_millis() < 2000,
            "Large network solve took {}ms, expected < 2000ms",
            elapsed.as_millis()
        );
    }
}// Feature: unified-resource-scheduler, Property Tests (Tasks 2.4, 3.6, 5.6, 6.5)
// and Integration Tests (Tasks 8.4, 9.2)

#[cfg(test)]
mod proptest_tests {
    use super::*;
    use crate::network::catalog::*;
    use crate::network::demand::WorkloadDemand;
    use crate::network::registry::*;
    use crate::network::solver::{
        ModelPlacement, ParallelismProtocol, SelectionResult, SelectedModel, SolverConfig,
        SolverInputs, SolverPreferences, UtilityScores,
    };
    use crate::network::solver_contention::{
        compute_contention, compute_unified_objective, ResourceType,
    };
    use proptest::prelude::*;
    use std::collections::HashMap;

    // ─── Helpers ─────────────────────────────────────────────────────────────

    fn make_model(id: &str, ram_mb: u64) -> ModelEntry {
        ModelEntry {
            model_id: id.to_string(),
            family: "test".to_string(),
            parameter_count_b: 7.0,
            quantization: Quantization::Q4_K_M,
            requirements: ModelRequirements {
                min_ram_mb: ram_mb,
                min_vram_mb: 0,
                disk_size_mb: ram_mb,
                min_compute_capability: None,
            },
            performance: ModelPerformance {
                estimates: vec![PerformanceEstimate {
                    hardware_class: HardwareClass::CpuOnly,
                    estimated_tok_s: 20.0,
                    estimated_prefill_tok_s: 50.0,
                }],
            },
            task_affinity: HashMap::from([(TaskType::Chat, 0.7)]),
            supported_backends: vec![InferenceBackend::Ollama],
            download_sources: vec![],
            checksum_sha256: "test".to_string(),
        }
    }

    fn make_agent(id: &str, required_model: &str, ram_mb: u64, cpu_cores: u32) -> AgentEntry {
        AgentEntry {
            agent_id: id.to_string(),
            agent_name: id.to_string(),
            version: "1.0".to_string(),
            required_model: required_model.to_string(),
            tool_declarations: vec![],
            runtime_requirements: AgentRequirements { ram_mb, cpu_cores, disk_mb: 100 },
            download_sources: vec![],
            checksum_sha256: "test".to_string(),
        }
    }

    fn make_agent_with_tools_pt(
        id: &str, required_model: &str, ram_mb: u64, cpu_cores: u32, tools: Vec<&str>,
    ) -> AgentEntry {
        AgentEntry {
            agent_id: id.to_string(),
            agent_name: id.to_string(),
            version: "1.0".to_string(),
            required_model: required_model.to_string(),
            tool_declarations: tools.into_iter().map(|t| t.to_string()).collect(),
            runtime_requirements: AgentRequirements { ram_mb, cpu_cores, disk_mb: 100 },
            download_sources: vec![],
            checksum_sha256: "test".to_string(),
        }
    }

    fn make_node_pt(ram_mb: u64, cores: u32) -> NodeState {
        let node_id = uuid::Uuid::new_v4();
        NodeState {
            capabilities: NodeCapabilities {
                node_id,
                hostname: "test".to_string(),
                device_type: DeviceType::Desktop,
                cpu: CpuProfile { cores, architecture: "x86_64".to_string(), clock_mhz: 4000, isa_extensions: vec![] },
                ram: RamProfile { total_mb: ram_mb, available_mb: ram_mb, ddr_generation: 4 },
                gpu: None,
                storage: StorageProfile { storage_type: StorageType::Nvme, available_mb: 500000, read_speed_mbps: 5000 },
                network_interfaces: vec![],
                phone_info: None,
                available_tools: vec![],
            },
            utilization: NodeUtilization::default(),
            loaded_models: vec![],
            stability_score: 0.95,
            last_heartbeat_ms: 0,
            is_online: true,
            latency_to_peers: HashMap::new(),
            thermal_state: ThermalState::default(),
        }
    }

    fn make_node_with_tools_pt(ram_mb: u64, cores: u32, tools: Vec<&str>) -> NodeState {
        let mut node = make_node_pt(ram_mb, cores);
        node.capabilities.available_tools = tools
            .into_iter()
            .map(|t| crate::agents::tools::ToolCapability {
                tool_id: t.to_string(),
                tool_name: t.to_string(),
                category: crate::agents::tools::ToolCategory::CodeExecution,
                resource_requirements: crate::agents::tools::ToolResources::default(),
                is_available: true,
                version: "1.0.0".to_string(),
            })
            .collect();
        node
    }

    fn make_model_placement_pt(model_id: &str, node_id: NodeId) -> ModelPlacement {
        ModelPlacement {
            model_id: model_id.to_string(),
            instance_id: uuid::Uuid::new_v4(),
            assigned_nodes: vec![node_id],
            protocol: ParallelismProtocol::SingleNode,
            estimated_tok_s: 20.0,
        }
    }

    fn make_inputs_pt(
        nodes: Vec<NodeState>, models: Vec<ModelEntry>, agents: Vec<AgentEntry>,
        agent_shares: HashMap<AgentId, f64>,
    ) -> SolverInputs {
        SolverInputs {
            node_states: nodes,
            model_catalog: models,
            workload_demand: WorkloadDemand {
                computed_at_ms: 0, time_window_hours: 24,
                model_shares: HashMap::new(), task_shares: HashMap::new(),
                total_requests: 100,
                forecast: crate::network::demand::DemandForecast {
                    next_period_model_shares: HashMap::new(),
                    next_period_task_shares: HashMap::new(),
                    confidence: 0.8, prefetch_signals: vec![],
                },
            },
            preferences: SolverPreferences::new(),
            max_network_params_b: 14.0,
            agent_catalog: agents,
            agent_demand: AgentWorkloadDemand {
                agent_shares, total_agent_requests: 1000, time_window_hours: 24,
            },
        }
    }

    // ─── Task 2.4: Property Tests for Agent Selection ────────────────────────

    // Feature: unified-resource-scheduler, Property 2: Co-Selection Invariant
    // **Validates: Requirements 1.5, 11.1**
    proptest! {
        #[test]
        fn prop_co_selection_invariant(
            num_agents in 1u32..5,
            demand_share in 0.1f64..1.0,
        ) {
            let node = make_node_pt(64_000, 32);
            let model = make_model("model-a", 4000);
            let agents: Vec<AgentEntry> = (0..num_agents)
                .map(|i| make_agent(&format!("agent-{}", i), "model-a", 256, 1))
                .collect();
            let shares: HashMap<AgentId, f64> = agents.iter()
                .map(|a| (a.agent_id.clone(), demand_share / num_agents as f64))
                .collect();
            let inputs = make_inputs_pt(vec![node], vec![model], agents, shares);

            let model_selection = SelectionResult {
                selected: vec![SelectedModel {
                    model_id: "model-a".to_string(), instance_count: 1,
                    utility_score: 0.5, is_exploration: false,
                }],
                total_ram_allocated_mb: 4000, total_vram_allocated_mb: 0,
                exploration_model: None,
            };

            let result = select_agents(&inputs, &model_selection);

            // Every selected agent's required_model must be in model selection or catalog
            for agent in &result.selected {
                let model_in_selection = model_selection.selected.iter()
                    .any(|s| s.model_id == agent.required_model);
                let model_in_catalog = inputs.model_catalog.iter()
                    .any(|m| m.model_id == agent.required_model);
                prop_assert!(model_in_selection || model_in_catalog,
                    "Agent {} has required_model {} not in selection or catalog",
                    agent.agent_id, agent.required_model);
            }
        }
    }

    // Feature: unified-resource-scheduler, Property 3: Instance Count Monotonicity
    // **Validates: Requirements 2.2, 2.3**
    proptest! {
        #[test]
        fn prop_instance_count_monotonicity(
            share_low in 0.01f64..0.5,
            share_high in 0.5f64..1.0,
            total_requests in 100u64..50000,
        ) {
            let config = SolverConfig::default();

            let demand_low = AgentWorkloadDemand {
                agent_shares: HashMap::from([("agent-1".to_string(), share_low)]),
                total_agent_requests: total_requests,
                time_window_hours: 24,
            };
            let demand_high = AgentWorkloadDemand {
                agent_shares: HashMap::from([("agent-1".to_string(), share_high)]),
                total_agent_requests: total_requests,
                time_window_hours: 24,
            };

            let instances_low = compute_agent_desired_instances(
                &"agent-1".to_string(), &demand_low, &config);
            let instances_high = compute_agent_desired_instances(
                &"agent-1".to_string(), &demand_high, &config);

            prop_assert!(instances_high >= instances_low,
                "Monotonicity violated: share_low={} gave {} instances, share_high={} gave {}",
                share_low, instances_low, share_high, instances_high);
        }
    }

    // Feature: unified-resource-scheduler, Property 4: Instance Count Bounded
    // **Validates: Requirements 2.4**
    proptest! {
        #[test]
        fn prop_instance_count_bounded(
            share in 0.0f64..1.0,
            total_requests in 0u64..1_000_000,
            time_window in 1u32..168,
        ) {
            let config = SolverConfig::default();
            let demand = AgentWorkloadDemand {
                agent_shares: HashMap::from([("agent-1".to_string(), share)]),
                total_agent_requests: total_requests,
                time_window_hours: time_window,
            };

            let instances = compute_agent_desired_instances(
                &"agent-1".to_string(), &demand, &config);

            prop_assert!(instances >= 1, "Instance count {} < 1", instances);
            prop_assert!(instances <= config.max_instances_per_agent,
                "Instance count {} > max {}", instances, config.max_instances_per_agent);
        }
    }

    // Feature: unified-resource-scheduler, Property 17: Shared Model Single-Counting
    // **Validates: Requirements 11.4**
    proptest! {
        #[test]
        fn prop_shared_model_single_counting(
            num_agents in 2u32..6,
            model_ram in 1000u64..8000,
            agent_ram in 128u64..512,
        ) {
            let node = make_node_pt(64_000, 32);
            let model = make_model("shared-model", model_ram);
            let agents: Vec<AgentEntry> = (0..num_agents)
                .map(|i| make_agent(&format!("agent-{}", i), "shared-model", agent_ram, 1))
                .collect();
            let shares: HashMap<AgentId, f64> = agents.iter()
                .map(|a| (a.agent_id.clone(), 0.5))
                .collect();
            let inputs = make_inputs_pt(vec![node], vec![model], agents.clone(), shares);

            // Model NOT in selection so it gets counted as model_ram_cost
            let model_selection = SelectionResult {
                selected: vec![], total_ram_allocated_mb: 0,
                total_vram_allocated_mb: 0, exploration_model: None,
            };

            let result = select_agents(&inputs, &model_selection);

            if result.selected.len() > 1 {
                // Model RAM should be counted only once, not per-agent
                let expected_max = model_ram + (result.selected.len() as u64 * agent_ram);
                prop_assert!(result.total_ram_allocated_mb <= expected_max,
                    "RAM {} exceeds expected max {} (model counted multiple times?)",
                    result.total_ram_allocated_mb, expected_max);

                // Model RAM should NOT be counted N times
                let over_counted = model_ram * result.selected.len() as u64
                    + (result.selected.len() as u64 * agent_ram);
                if result.selected.len() > 1 {
                    prop_assert!(result.total_ram_allocated_mb < over_counted,
                        "RAM {} equals over-counted {} — model counted multiple times",
                        result.total_ram_allocated_mb, over_counted);
                }
            }
        }
    }

    // ─── Task 3.6: Property Tests for Agent Placement ────────────────────────

    // Feature: unified-resource-scheduler, Property 5: Placement Capacity Invariant
    // **Validates: Requirements 3.1, 3.4, 3.5, 8.4, 8.5**
    proptest! {
        #[test]
        fn prop_placement_capacity_invariant(
            num_agents in 1u32..4,
            agent_ram in 256u64..2048,
            agent_cpu in 1u32..4,
            node_ram in 8000u64..64000,
            node_cores in 8u32..32,
        ) {
            let config = SolverConfig::default();
            let node = make_node_pt(node_ram, node_cores);
            let node_id = node.capabilities.node_id;

            let agents: Vec<AgentEntry> = (0..num_agents)
                .map(|i| make_agent(&format!("agent-{}", i), "model-a", agent_ram, agent_cpu))
                .collect();
            let model_placement = make_model_placement_pt("model-a", node_id);

            let selection = AgentSelectionResult {
                selected: agents.iter().map(|a| SelectedAgent {
                    agent_id: a.agent_id.clone(), instance_count: 1,
                    utility_score: 0.5, required_model: "model-a".to_string(),
                }).collect(),
                total_ram_allocated_mb: num_agents as u64 * agent_ram,
                total_cpu_cores_allocated: num_agents * agent_cpu,
            };

            let (placements, _, _) = assign_agents(
                &selection, &[node.clone()], &[model_placement], &agents, &config);

            // Verify RAM capacity not exceeded
            let total_agent_ram: u64 = placements.iter()
                .filter(|p| p.assigned_node == node_id)
                .map(|p| p.resource_allocation.ram_mb)
                .sum();
            let usable_ram = node_ram - (node_ram as f64 * config.ram_headroom_percent) as u64;
            prop_assert!(total_agent_ram <= usable_ram,
                "Agent RAM {} exceeds usable {} on node", total_agent_ram, usable_ram);

            // Verify CPU capacity not exceeded
            let total_agent_cpu: u32 = placements.iter()
                .filter(|p| p.assigned_node == node_id)
                .map(|p| p.resource_allocation.cpu_cores)
                .sum();
            let usable_cpu = (node_cores as f64 * config.cpu_headroom_percent) as u32;
            prop_assert!(total_agent_cpu <= usable_cpu,
                "Agent CPU {} exceeds usable {} on node", total_agent_cpu, usable_cpu);
        }
    }

    // Feature: unified-resource-scheduler, Property 6: Tool Subset Constraint
    // **Validates: Requirements 3.2, 13.1**
    proptest! {
        #[test]
        fn prop_tool_subset_constraint(
            has_browser in proptest::bool::ANY,
            has_filesystem in proptest::bool::ANY,
        ) {
            let config = SolverConfig::default();
            let mut node_tools = vec![];
            if has_browser { node_tools.push("browser"); }
            if has_filesystem { node_tools.push("filesystem"); }

            let node = make_node_with_tools_pt(32_000, 16, node_tools.clone());
            let node_id = node.capabilities.node_id;

            // Agent requires both tools
            let agent = make_agent_with_tools_pt("agent-1", "model-a", 512, 2, vec!["browser", "filesystem"]);
            let model_placement = make_model_placement_pt("model-a", node_id);

            let selection = AgentSelectionResult {
                selected: vec![SelectedAgent {
                    agent_id: "agent-1".to_string(), instance_count: 1,
                    utility_score: 0.5, required_model: "model-a".to_string(),
                }],
                total_ram_allocated_mb: 512, total_cpu_cores_allocated: 2,
            };

            let (placements, _, _) = assign_agents(
                &selection, &[node.clone()], &[model_placement], &[agent.clone()], &config);

            // If placed, all required tools must be available on the node
            for _p in &placements {
                let placed_node = &node;
                let available: Vec<&str> = placed_node.capabilities.available_tools.iter()
                    .filter(|t| t.is_available)
                    .map(|t| t.tool_id.as_str())
                    .collect();
                for tool in &agent.tool_declarations {
                    prop_assert!(available.contains(&tool.as_str()),
                        "Agent placed on node missing tool {}", tool);
                }
            }
        }
    }

    // Feature: unified-resource-scheduler, Property 7: Model Proximity Constraint
    // **Validates: Requirements 3.3**
    proptest! {
        #[test]
        fn prop_model_proximity_constraint(
            latency_ms in 1.0f64..200.0,
        ) {
            let config = SolverConfig::default();
            let node_a = make_node_pt(32_000, 16);
            let node_a_id = node_a.capabilities.node_id;

            let mut node_b = make_node_pt(32_000, 16);
            let node_b_id = node_b.capabilities.node_id;
            node_b.latency_to_peers.insert(node_a_id, LatencyMeasurement {
                peer_id: node_a_id, rtt_ms: latency_ms, bandwidth_mbps: 1000.0, measured_at_ms: 0,
            });

            let agent = make_agent("agent-1", "model-a", 512, 2);
            let model_placement = make_model_placement_pt("model-a", node_a_id);

            let selection = AgentSelectionResult {
                selected: vec![SelectedAgent {
                    agent_id: "agent-1".to_string(), instance_count: 1,
                    utility_score: 0.5, required_model: "model-a".to_string(),
                }],
                total_ram_allocated_mb: 512, total_cpu_cores_allocated: 2,
            };

            let (placements, _, _) = assign_agents(
                &selection, &[node_a, node_b], &[model_placement], &[agent], &config);

            // If agent is placed on node_b, latency must be below threshold
            for p in &placements {
                if p.assigned_node == node_b_id {
                    prop_assert!(latency_ms < config.pipeline_parallel_max_latency_ms,
                        "Agent placed on node_b with latency {}ms >= threshold {}ms",
                        latency_ms, config.pipeline_parallel_max_latency_ms);
                }
            }
        }
    }

    // Feature: unified-resource-scheduler, Property 8: Co-Location Preference
    // **Validates: Requirements 3.6**
    #[test]
    fn prop_co_location_preference() {
        let config = SolverConfig::default();
        let node_with_model = make_node_pt(32_000, 16);
        let model_node_id = node_with_model.capabilities.node_id;

        let mut node_without_model = make_node_pt(32_000, 16);
        node_without_model.latency_to_peers.insert(model_node_id, LatencyMeasurement {
            peer_id: model_node_id, rtt_ms: 5.0, bandwidth_mbps: 1000.0, measured_at_ms: 0,
        });

        let agent = make_agent("agent-1", "model-a", 512, 2);
        let model_placement = make_model_placement_pt("model-a", model_node_id);

        let selection = AgentSelectionResult {
            selected: vec![SelectedAgent {
                agent_id: "agent-1".to_string(), instance_count: 1,
                utility_score: 0.5, required_model: "model-a".to_string(),
            }],
            total_ram_allocated_mb: 512, total_cpu_cores_allocated: 2,
        };

        let (placements, _, _) = assign_agents(
            &selection, &[node_with_model, node_without_model], &[model_placement], &[agent], &config);

        // Agent should prefer the node with the model (co-location)
        assert_eq!(placements.len(), 1);
        assert_eq!(placements[0].assigned_node, model_node_id);
    }

    // Feature: unified-resource-scheduler, Property 9: Download Plan Correctness
    // **Validates: Requirements 4.1, 4.4, 12.3, 12.5**
    #[test]
    fn prop_download_plan_correctness() {
        let config = SolverConfig::default();
        let node = make_node_pt(32_000, 16);
        let node_id = node.capabilities.node_id;

        let agent = make_agent("agent-1", "model-a", 512, 2);
        let model_placement = make_model_placement_pt("model-a", node_id);

        let selection = AgentSelectionResult {
            selected: vec![SelectedAgent {
                agent_id: "agent-1".to_string(), instance_count: 1,
                utility_score: 0.5, required_model: "model-a".to_string(),
            }],
            total_ram_allocated_mb: 512, total_cpu_cores_allocated: 2,
        };

        let (placements, downloads, _) = assign_agents(
            &selection, &[node], &[model_placement], &[agent], &config);

        // Agent placed => download emitted for agent runtime (not on node)
        if !placements.is_empty() {
            let agent_dl = downloads.iter().find(|d| d.resource_type == ResourceType::Agent);
            assert!(agent_dl.is_some(), "Agent placed but no download emitted");
        }
    }

    // Feature: unified-resource-scheduler, Property 14: Priority Invariant
    // **Validates: Requirements 8.1, 8.2, 8.3**
    #[test]
    fn prop_priority_invariant() {
        // Models are placed first (priority 1), agents second (priority 2).
        // Under tight capacity, models should always be placed before agents.
        let config = SolverConfig::default();
        // Node with limited capacity
        let node = make_node_pt(8_000, 8);
        let node_id = node.capabilities.node_id;

        let agent = make_agent("agent-1", "model-a", 4000, 4);
        let model_placement = make_model_placement_pt("model-a", node_id);

        let selection = AgentSelectionResult {
            selected: vec![SelectedAgent {
                agent_id: "agent-1".to_string(), instance_count: 2,
                utility_score: 0.5, required_model: "model-a".to_string(),
            }],
            total_ram_allocated_mb: 8000, total_cpu_cores_allocated: 8,
        };

        let (placements, _, _) = assign_agents(
            &selection, &[node], &[model_placement], &[agent], &config);

        // Model placement is already done (passed in). Agent placement respects remaining capacity.
        // The solver never evicts model placements for agents.
        // With 8000 RAM, 10% headroom = 7200 usable, agent needs 4000 each.
        // At most 1 agent instance fits (7200/4000 = 1.8 -> 1 with CPU check too)
        assert!(placements.len() <= 1, "Should not exceed capacity");
    }

    // Feature: unified-resource-scheduler, Property 15: Node Eligibility Constraints
    // **Validates: Requirements 9.3, 9.4**
    proptest! {
        #[test]
        fn prop_node_eligibility_constraints(
            battery_percent in 0u8..100,
            is_charging in proptest::bool::ANY,
            thermal_idx in 0u32..3,
        ) {
            let config = SolverConfig::default();
            let mut node = make_node_pt(32_000, 16);
            let node_id = node.capabilities.node_id;

            node.capabilities.phone_info = Some(PhoneInfo {
                os: PhoneOs::Android, npu: None,
                battery_percent, is_charging,
                connection_type: ConnectionType::Wifi,
            });
            node.thermal_state = match thermal_idx {
                0 => ThermalState::Normal,
                1 => ThermalState::Warm,
                _ => ThermalState::Critical,
            };

            let agent = make_agent("agent-1", "model-a", 512, 2);
            let model_placement = make_model_placement_pt("model-a", node_id);

            let selection = AgentSelectionResult {
                selected: vec![SelectedAgent {
                    agent_id: "agent-1".to_string(), instance_count: 1,
                    utility_score: 0.5, required_model: "model-a".to_string(),
                }],
                total_ram_allocated_mb: 512, total_cpu_cores_allocated: 2,
            };

            let (placements, _, _) = assign_agents(
                &selection, &[node.clone()], &[model_placement], &[agent], &config);

            for _p in &placements {
                // If placed, node must pass battery/thermal constraints
                prop_assert!(passes_battery_thermal_constraints(&node),
                    "Agent placed on node failing battery/thermal: battery={}%, charging={}, thermal={:?}",
                    battery_percent, is_charging, node.thermal_state);
            }
        }
    }

    // ─── Task 5.6: Property Tests for Contention and Objective ───────────────

    // Feature: unified-resource-scheduler, Property 10: Unified Objective Formula
    // **Validates: Requirements 5.1**
    proptest! {
        #[test]
        fn prop_unified_objective_formula(
            model_total in 0.0f64..2.0,
            agent_utility in 0.0f64..1.0,
            contention_cost in 0.0f64..1.0,
        ) {
            let model_utility = UtilityScores {
                quality: 0.5, speed: 0.5, mass: 0.5,
                total: model_total,
                agent_utility: 0.0, contention_cost: 0.0, unified_total: model_total,
            };

            let result = compute_unified_objective(&model_utility, agent_utility, contention_cost);
            let expected = model_total + agent_utility - contention_cost;

            prop_assert!((result - expected).abs() < 1e-10,
                "unified_total {} != expected {} (total={} + agent={} - contention={})",
                result, expected, model_total, agent_utility, contention_cost);
        }
    }

    // Feature: unified-resource-scheduler, Property 11: Parallelism Factor Bounded
    // **Validates: Requirements 5.5, 6.1**
    proptest! {
        #[test]
        fn prop_parallelism_factor_bounded(
            independent_steps in 0u32..100,
            total_steps in 1u32..100,
            avg_latency in 0.0f64..500.0,
            step_compute_time in 1.0f64..1000.0,
            min_speed in 1.0f64..100000.0,
            max_speed in 1.0f64..100000.0,
        ) {
            let config = SolverConfig::default();
            let actual_min = min_speed.min(max_speed);
            let actual_max = min_speed.max(max_speed);

            let result = compute_parallelism_factor(
                independent_steps, total_steps, avg_latency,
                step_compute_time, actual_min, actual_max, &config);

            prop_assert!(result >= 0.0, "Parallelism factor {} < 0.0", result);
            prop_assert!(result <= 1.0, "Parallelism factor {} > 1.0", result);
        }
    }

    // Feature: unified-resource-scheduler, Property 12: Speed Ratio Rejection
    // **Validates: Requirements 6.2**
    proptest! {
        #[test]
        fn prop_speed_ratio_rejection(
            min_speed in 1.0f64..1000.0,
            ratio in 3.01f64..20.0,
            independent_steps in 1u32..50,
            total_steps in 1u32..50,
        ) {
            let config = SolverConfig::default(); // threshold = 3.0
            let max_speed = min_speed * ratio; // ratio > 3.0

            let result = compute_parallelism_factor(
                independent_steps, total_steps.max(independent_steps),
                5.0, 100.0, min_speed, max_speed, &config);

            prop_assert_eq!(result, 0.0,
                "Expected parallelism=0 when speed ratio {} > threshold {}, got {}",
                ratio, config.speed_ratio_threshold, result);
        }
    }

    // Feature: unified-resource-scheduler, Property 13: Contention Penalties Non-Negative
    // **Validates: Requirements 7.1, 7.2, 7.3, 7.4, 7.5, 7.6**
    proptest! {
        #[test]
        fn prop_contention_penalties_non_negative(
            node_ram in 4000u64..64000,
            node_cores in 4u32..32,
            agent_cpu in 1u32..8,
            ram_used in 0u64..32000,
            queue_depth in 0u32..30,
        ) {
            let config = SolverConfig::default();
            let mut node = make_node_pt(node_ram, node_cores);
            node.utilization.ram_used_mb = ram_used.min(node_ram - 1);
            node.utilization.queue_depth = queue_depth;
            let node_id = node.capabilities.node_id;

            let model_placement = make_model_placement_pt("model-a", node_id);
            let agent_placement = AgentPlacement {
                agent_id: "agent-1".to_string(),
                instance_id: uuid::Uuid::new_v4(),
                assigned_node: node_id,
                required_model_instance_id: uuid::Uuid::new_v4(),
                estimated_throughput: 30.0,
                resource_allocation: AgentRequirements {
                    ram_mb: 512, cpu_cores: agent_cpu.min(node_cores), disk_mb: 100,
                },
            };

            let result = compute_contention(
                &[model_placement], &[agent_placement], &[node], &config);

            // All penalties must be non-negative
            for (_, detail) in &result.per_node {
                prop_assert!(detail.cpu_penalty >= 0.0, "cpu_penalty {} < 0", detail.cpu_penalty);
                prop_assert!(detail.memory_penalty >= 0.0, "memory_penalty {} < 0", detail.memory_penalty);
                prop_assert!(detail.queue_penalty >= 0.0, "queue_penalty {} < 0", detail.queue_penalty);
                prop_assert!(detail.speed_penalty >= 0.0, "speed_penalty {} < 0", detail.speed_penalty);
                prop_assert!(detail.latency_penalty >= 0.0, "latency_penalty {} < 0", detail.latency_penalty);
                prop_assert!(detail.total >= 0.0, "total {} < 0", detail.total);

                // Total must equal weighted sum
                let expected_total = config.contention_weights.cpu * detail.cpu_penalty
                    + config.contention_weights.memory * detail.memory_penalty
                    + config.contention_weights.queue * detail.queue_penalty
                    + config.contention_weights.speed * detail.speed_penalty
                    + config.contention_weights.latency * detail.latency_penalty;
                prop_assert!((detail.total - expected_total).abs() < 1e-10,
                    "total {} != weighted sum {}", detail.total, expected_total);
            }
            prop_assert!(result.total_cost >= 0.0, "total_cost {} < 0", result.total_cost);
        }
    }

    // ─── Task 6.5: Property Tests for Integration and Backwards Compatibility ─

    // Feature: unified-resource-scheduler, Property 1: Backwards Compatibility
    // **Validates: Requirements 1.2, 5.2, 5.7, 10.1, 10.2, 10.5**
    #[test]
    fn prop_backwards_compatibility_empty_agent_catalog() {
        use crate::network::solver::solve;

        let node = make_node_pt(32_000, 16);
        let model = make_model("model-a", 4000);
        // Empty agent catalog and default agent demand
        let inputs = make_inputs_pt(vec![node], vec![model], vec![], HashMap::new());
        let config = SolverConfig::default();

        let plan = solve(&inputs, &config, 1000);

        // Agent placements must be empty
        assert!(plan.agent_placements.is_empty());
        assert!(plan.pending_downloads.is_empty());
        assert!(plan.diagnostics.is_empty());
        // unified_total == total when no agents
        assert!((plan.utility_scores.unified_total - plan.utility_scores.total).abs() < 1e-10);
        assert_eq!(plan.utility_scores.agent_utility, 0.0);
        assert_eq!(plan.utility_scores.contention_cost, 0.0);
    }

    // Feature: unified-resource-scheduler, Property 16: Cascading Rejection
    // **Validates: Requirements 11.2**
    #[test]
    fn prop_cascading_rejection() {
        // Agent depends on a model not in catalog => agent rejected with diagnostic
        let node = make_node_pt(32_000, 16);
        let agent = make_agent("agent-1", "nonexistent-model", 512, 2);
        let inputs = make_inputs_pt(
            vec![node], vec![], vec![agent],
            HashMap::from([("agent-1".to_string(), 0.5)]),
        );

        let model_selection = SelectionResult {
            selected: vec![], total_ram_allocated_mb: 0,
            total_vram_allocated_mb: 0, exploration_model: None,
        };

        let mut agent_selection = select_agents(&inputs, &model_selection);
        // Agent won't be selected because model not in catalog
        // But if it were somehow selected, enforce_co_selection would reject it
        if !agent_selection.selected.is_empty() {
            let mut ms = model_selection.clone();
            let actions = enforce_co_selection(&mut ms, &mut agent_selection, &inputs);
            // Agent should be rejected
            let rejected = actions.iter().any(|a| matches!(a, CoSelectionAction::AgentRejected { .. }));
            assert!(rejected, "Agent with missing model should be rejected");
            assert!(agent_selection.selected.is_empty());
        }
    }

    // Feature: unified-resource-scheduler, Property 18: Tool Unavailability Rejection
    // **Validates: Requirements 13.2**
    #[test]
    fn prop_tool_unavailability_rejection() {
        let config = SolverConfig::default();
        // Node has "filesystem" but NOT "browser"
        let node = make_node_with_tools_pt(32_000, 16, vec!["filesystem"]);
        let node_id = node.capabilities.node_id;

        // Agent requires "browser" which is globally unavailable
        let agent = make_agent_with_tools_pt("agent-1", "model-a", 512, 2, vec!["browser"]);
        let model_placement = make_model_placement_pt("model-a", node_id);

        let selection = AgentSelectionResult {
            selected: vec![SelectedAgent {
                agent_id: "agent-1".to_string(), instance_count: 1,
                utility_score: 0.5, required_model: "model-a".to_string(),
            }],
            total_ram_allocated_mb: 512, total_cpu_cores_allocated: 2,
        };

        let (placements, _, diagnostics) = assign_agents(
            &selection, &[node], &[model_placement], &[agent], &config);

        // Agent should be rejected
        assert!(placements.is_empty(), "Agent with unavailable tool should not be placed");
        assert!(!diagnostics.is_empty(), "Diagnostic should be emitted");
        assert!(diagnostics[0].reason.contains("browser"),
            "Diagnostic should mention missing tool");
    }

    // Feature: unified-resource-scheduler, Property 19: Anytime Validity
    // **Validates: Requirements 14.3**
    proptest! {
        #[test]
        fn prop_anytime_validity(
            num_nodes in 1u32..5,
            num_agents in 0u32..5,
        ) {
            use crate::network::solver::solve;

            let nodes: Vec<NodeState> = (0..num_nodes)
                .map(|i| make_node_pt(16_000 + i as u64 * 2000, 8))
                .collect();
            let model = make_model("model-a", 2000);
            let agents: Vec<AgentEntry> = (0..num_agents)
                .map(|i| make_agent(&format!("agent-{}", i), "model-a", 256, 1))
                .collect();
            let shares: HashMap<AgentId, f64> = agents.iter()
                .map(|a| (a.agent_id.clone(), 0.5))
                .collect();

            let inputs = make_inputs_pt(nodes.clone(), vec![model], agents, shares);
            let config = SolverConfig::default();

            let plan = solve(&inputs, &config, 1000);

            // Plan must always be valid (even if incomplete)
            // All agent placements must reference valid nodes
            for ap in &plan.agent_placements {
                let node_exists = nodes.iter()
                    .any(|n| n.capabilities.node_id == ap.assigned_node);
                prop_assert!(node_exists,
                    "Agent {} placed on non-existent node", ap.agent_id);
            }

            // All model placements must reference valid nodes
            for mp in &plan.placements {
                for node_id in &mp.assigned_nodes {
                    let node_exists = nodes.iter()
                        .any(|n| n.capabilities.node_id == *node_id);
                    prop_assert!(node_exists,
                        "Model {} placed on non-existent node", mp.model_id);
                }
            }
        }
    }
}

// ─── Tasks 8.4 and 9.2: Integration Tests ───────────────────────────────────

#[cfg(test)]
mod integration_tests {
    use super::*;
    use crate::network::catalog::*;
    use crate::network::demand::WorkloadDemand;
    use crate::network::registry::*;
    use crate::network::solver::{
        ModelPlacement, ParallelismProtocol, SolverConfig, SolverInputs, SolverPreferences, solve,
    };
    use crate::network::solver_contention::ResourceType;
    use std::collections::HashMap;

    fn make_model(id: &str, ram_mb: u64) -> ModelEntry {
        ModelEntry {
            model_id: id.to_string(),
            family: "test".to_string(),
            parameter_count_b: 7.0,
            quantization: Quantization::Q4_K_M,
            requirements: ModelRequirements {
                min_ram_mb: ram_mb, min_vram_mb: 0, disk_size_mb: ram_mb,
                min_compute_capability: None,
            },
            performance: ModelPerformance {
                estimates: vec![PerformanceEstimate {
                    hardware_class: HardwareClass::CpuOnly,
                    estimated_tok_s: 20.0, estimated_prefill_tok_s: 50.0,
                }],
            },
            task_affinity: HashMap::from([(TaskType::Chat, 0.7)]),
            supported_backends: vec![InferenceBackend::Ollama],
            download_sources: vec![],
            checksum_sha256: "test".to_string(),
        }
    }

    fn make_agent(id: &str, required_model: &str, ram_mb: u64, cpu_cores: u32) -> AgentEntry {
        AgentEntry {
            agent_id: id.to_string(),
            agent_name: id.to_string(),
            version: "1.0".to_string(),
            required_model: required_model.to_string(),
            tool_declarations: vec![],
            runtime_requirements: AgentRequirements { ram_mb, cpu_cores, disk_mb: 100 },
            download_sources: vec![],
            checksum_sha256: "test".to_string(),
        }
    }

    fn make_agent_with_tools(
        id: &str, required_model: &str, ram_mb: u64, cpu_cores: u32, tools: Vec<&str>,
    ) -> AgentEntry {
        AgentEntry {
            agent_id: id.to_string(),
            agent_name: id.to_string(),
            version: "1.0".to_string(),
            required_model: required_model.to_string(),
            tool_declarations: tools.into_iter().map(|t| t.to_string()).collect(),
            runtime_requirements: AgentRequirements { ram_mb, cpu_cores, disk_mb: 100 },
            download_sources: vec![],
            checksum_sha256: "test".to_string(),
        }
    }

    fn make_node(ram_mb: u64, cores: u32) -> NodeState {
        let node_id = uuid::Uuid::new_v4();
        NodeState {
            capabilities: NodeCapabilities {
                node_id,
                hostname: "test".to_string(),
                device_type: DeviceType::Desktop,
                cpu: CpuProfile { cores, architecture: "x86_64".to_string(), clock_mhz: 4000, isa_extensions: vec![] },
                ram: RamProfile { total_mb: ram_mb, available_mb: ram_mb, ddr_generation: 4 },
                gpu: None,
                storage: StorageProfile { storage_type: StorageType::Nvme, available_mb: 500000, read_speed_mbps: 5000 },
                network_interfaces: vec![],
                phone_info: None,
                available_tools: vec![],
            },
            utilization: NodeUtilization::default(),
            loaded_models: vec![],
            stability_score: 0.95,
            last_heartbeat_ms: 0,
            is_online: true,
            latency_to_peers: HashMap::new(),
            thermal_state: ThermalState::default(),
        }
    }

    fn make_node_with_tools(ram_mb: u64, cores: u32, tools: Vec<&str>) -> NodeState {
        let mut node = make_node(ram_mb, cores);
        node.capabilities.available_tools = tools
            .into_iter()
            .map(|t| crate::agents::tools::ToolCapability {
                tool_id: t.to_string(),
                tool_name: t.to_string(),
                category: crate::agents::tools::ToolCategory::CodeExecution,
                resource_requirements: crate::agents::tools::ToolResources::default(),
                is_available: true,
                version: "1.0.0".to_string(),
            })
            .collect();
        node
    }

    fn make_inputs(
        nodes: Vec<NodeState>, models: Vec<ModelEntry>, agents: Vec<AgentEntry>,
        agent_shares: HashMap<AgentId, f64>,
    ) -> SolverInputs {
        SolverInputs {
            node_states: nodes,
            model_catalog: models,
            workload_demand: WorkloadDemand {
                computed_at_ms: 0, time_window_hours: 24,
                model_shares: HashMap::new(), task_shares: HashMap::new(),
                total_requests: 100,
                forecast: crate::network::demand::DemandForecast {
                    next_period_model_shares: HashMap::new(),
                    next_period_task_shares: HashMap::new(),
                    confidence: 0.8, prefetch_signals: vec![],
                },
            },
            preferences: SolverPreferences::new(),
            max_network_params_b: 14.0,
            agent_catalog: agents,
            agent_demand: AgentWorkloadDemand {
                agent_shares, total_agent_requests: 1000, time_window_hours: 24,
            },
        }
    }

    // ─── Task 8.4: Unit Tests for Device-Agnostic Constraints ────────────────

    #[test]
    fn test_battery_constraint_low_battery_excluded() {
        let mut node = make_node(32_000, 16);
        node.capabilities.phone_info = Some(PhoneInfo {
            os: PhoneOs::Android, npu: None,
            battery_percent: 10, is_charging: false,
            connection_type: ConnectionType::Wifi,
        });
        assert!(!passes_battery_thermal_constraints(&node));
    }

    #[test]
    fn test_thermal_constraint_critical_excluded() {
        let mut node = make_node(32_000, 16);
        node.thermal_state = ThermalState::Critical;
        assert!(!passes_battery_thermal_constraints(&node));
    }

    #[test]
    fn test_tool_removal_triggers_re_solve() {
        // Place agent on node with tool, then mark tool unavailable
        let mut node = make_node_with_tools(32_000, 16, vec!["browser"]);
        let node_id = node.capabilities.node_id;

        let agent = make_agent_with_tools("agent-1", "model-a", 512, 2, vec!["browser"]);
        let placement = AgentPlacement {
            agent_id: "agent-1".to_string(),
            instance_id: uuid::Uuid::new_v4(),
            assigned_node: node_id,
            required_model_instance_id: uuid::Uuid::new_v4(),
            estimated_throughput: 50.0,
            resource_allocation: AgentRequirements { ram_mb: 512, cpu_cores: 2, disk_mb: 100 },
        };

        // Initially valid
        assert!(!should_re_solve(&[placement.clone()], &[agent.clone()], &[node.clone()]));

        // Mark tool unavailable
        for tool in &mut node.capabilities.available_tools {
            if tool.tool_id == "browser" {
                tool.is_available = false;
            }
        }

        // Now should trigger re-solve
        assert!(should_re_solve(&[placement], &[agent], &[node]));
    }

    #[test]
    fn test_no_device_type_enum_in_scheduling_decisions() {
        // Verify that identical nodes with different device types produce same placement
        let config = SolverConfig::default();

        let mut node_desktop = make_node(32_000, 16);
        node_desktop.capabilities.device_type = DeviceType::Desktop;
        let desktop_id = node_desktop.capabilities.node_id;

        let mut node_phone = make_node(32_000, 16);
        node_phone.capabilities.device_type = DeviceType::Phone;
        node_phone.capabilities.phone_info = Some(PhoneInfo {
            os: PhoneOs::Android, npu: None,
            battery_percent: 80, is_charging: false,
            connection_type: ConnectionType::Wifi,
        });

        let agent = make_agent("agent-1", "model-a", 512, 2);

        // Test on desktop
        let mp_desktop = ModelPlacement {
            model_id: "model-a".to_string(),
            instance_id: uuid::Uuid::new_v4(),
            assigned_nodes: vec![desktop_id],
            protocol: ParallelismProtocol::SingleNode,
            estimated_tok_s: 20.0,
        };
        let selection = AgentSelectionResult {
            selected: vec![SelectedAgent {
                agent_id: "agent-1".to_string(), instance_count: 1,
                utility_score: 0.5, required_model: "model-a".to_string(),
            }],
            total_ram_allocated_mb: 512, total_cpu_cores_allocated: 2,
        };
        let (placements_desktop, _, _) = assign_agents(
            &selection, &[node_desktop], &[mp_desktop], &[agent.clone()], &config);

        // Test on phone (with good battery)
        let phone_id = node_phone.capabilities.node_id;
        let mp_phone = ModelPlacement {
            model_id: "model-a".to_string(),
            instance_id: uuid::Uuid::new_v4(),
            assigned_nodes: vec![phone_id],
            protocol: ParallelismProtocol::SingleNode,
            estimated_tok_s: 20.0,
        };
        let (placements_phone, _, _) = assign_agents(
            &selection, &[node_phone], &[mp_phone], &[agent], &config);

        // Both should place the agent (device type doesn't matter, only constraints)
        assert_eq!(placements_desktop.len(), 1, "Desktop should place agent");
        assert_eq!(placements_phone.len(), 1, "Phone with good battery should place agent");
    }

    // ─── Task 9.2: Integration Tests for End-to-End Scenarios ────────────────

    #[test]
    fn test_single_node_single_agent_single_model() {
        let node = make_node(32_000, 16);
        let model = make_model("model-a", 4000);
        let agent = make_agent("agent-1", "model-a", 512, 2);
        let inputs = make_inputs(
            vec![node], vec![model], vec![agent],
            HashMap::from([("agent-1".to_string(), 0.8)]),
        );
        let config = SolverConfig::default();

        let plan = solve(&inputs, &config, 1000);

        // Model should be placed
        assert!(!plan.placements.is_empty(), "Model should be placed");
        // Agent should be placed
        assert!(!plan.agent_placements.is_empty(), "Agent should be placed");
        assert_eq!(plan.agent_placements[0].agent_id, "agent-1");
        // Unified total should include agent utility
        assert!(plan.utility_scores.unified_total > 0.0);
    }

    #[test]
    fn test_multi_node_agent_requiring_tools_on_specific_nodes() {
        // Node A has "browser", Node B has "filesystem" only
        let node_a = make_node_with_tools(32_000, 16, vec!["browser", "filesystem"]);
        let mut node_b = make_node_with_tools(32_000, 16, vec!["filesystem"]);
        let node_a_id = node_a.capabilities.node_id;
        let _node_b_id = node_b.capabilities.node_id;

        // Add latency between nodes
        node_b.latency_to_peers.insert(node_a_id, LatencyMeasurement {
            peer_id: node_a_id, rtt_ms: 5.0, bandwidth_mbps: 1000.0, measured_at_ms: 0,
        });

        let model = make_model("model-a", 4000);
        // Agent requires "browser" — only available on node A
        let agent = make_agent_with_tools("agent-1", "model-a", 512, 2, vec!["browser"]);
        let inputs = make_inputs(
            vec![node_a, node_b], vec![model], vec![agent],
            HashMap::from([("agent-1".to_string(), 0.8)]),
        );
        let config = SolverConfig::default();

        let plan = solve(&inputs, &config, 1000);

        // Agent must be placed on node A (only node with "browser")
        if !plan.agent_placements.is_empty() {
            assert_eq!(plan.agent_placements[0].assigned_node, node_a_id,
                "Agent should be on node with required tool");
        }
    }

    #[test]
    fn test_agent_max_instances_heterogeneous_network() {
        let nodes: Vec<NodeState> = (0..3)
            .map(|i| make_node(32_000 + i * 8000, 8 + i as u32 * 4))
            .collect();
        let model = make_model("model-a", 2000);
        let agent = make_agent("agent-1", "model-a", 256, 1);
        let inputs = make_inputs(
            nodes, vec![model], vec![agent],
            HashMap::from([("agent-1".to_string(), 1.0)]),
        );
        // Set high demand to trigger multiple instances
        let mut inputs = inputs;
        inputs.agent_demand.total_agent_requests = 100_000;
        inputs.agent_demand.time_window_hours = 1;

        let config = SolverConfig::default();
        let plan = solve(&inputs, &config, 1000);

        // Instance count should be capped at max_instances_per_agent (8)
        let agent_count = plan.agent_placements.iter()
            .filter(|p| p.agent_id == "agent-1")
            .count();
        assert!(agent_count <= config.max_instances_per_agent as usize,
            "Agent instances {} exceeds max {}", agent_count, config.max_instances_per_agent);
    }

    #[test]
    fn test_backwards_compatibility_empty_agent_catalog() {
        let node = make_node(32_000, 16);
        let model = make_model("model-a", 4000);
        let inputs = make_inputs(vec![node], vec![model], vec![], HashMap::new());
        let config = SolverConfig::default();

        let plan = solve(&inputs, &config, 1000);

        // With empty agent catalog, agent-related fields should be empty/zero
        assert!(plan.agent_placements.is_empty());
        assert!(plan.pending_downloads.is_empty());
        assert!(plan.diagnostics.is_empty());
        assert_eq!(plan.utility_scores.agent_utility, 0.0);
        assert_eq!(plan.utility_scores.contention_cost, 0.0);
        assert!((plan.utility_scores.unified_total - plan.utility_scores.total).abs() < 1e-10);
    }

    #[test]
    fn test_serialization_round_trip_all_new_structs() {
        // Test that all new structs serialize and deserialize correctly via serde
        use crate::network::solver_contention::*;

        // AgentEntry
        let agent = AgentEntry {
            agent_id: "test-agent".to_string(),
            agent_name: "Test Agent".to_string(),
            version: "1.0.0".to_string(),
            required_model: "model-x".to_string(),
            tool_declarations: vec!["browser".to_string(), "filesystem".to_string()],
            runtime_requirements: AgentRequirements { ram_mb: 512, cpu_cores: 2, disk_mb: 100 },
            download_sources: vec![],
            checksum_sha256: "abc123".to_string(),
        };
        let json = serde_json::to_string(&agent).unwrap();
        let deserialized: AgentEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.agent_id, "test-agent");
        assert_eq!(deserialized.tool_declarations.len(), 2);

        // AgentWorkloadDemand
        let demand = AgentWorkloadDemand {
            agent_shares: HashMap::from([("agent-1".to_string(), 0.7)]),
            total_agent_requests: 5000,
            time_window_hours: 24,
        };
        let json = serde_json::to_string(&demand).unwrap();
        let deserialized: AgentWorkloadDemand = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.total_agent_requests, 5000);

        // SelectedAgent
        let selected = SelectedAgent {
            agent_id: "agent-1".to_string(),
            instance_count: 3,
            utility_score: 0.85,
            required_model: "model-a".to_string(),
        };
        let json = serde_json::to_string(&selected).unwrap();
        let deserialized: SelectedAgent = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.instance_count, 3);

        // AgentPlacement
        let placement = AgentPlacement {
            agent_id: "agent-1".to_string(),
            instance_id: uuid::Uuid::new_v4(),
            assigned_node: uuid::Uuid::new_v4(),
            required_model_instance_id: uuid::Uuid::new_v4(),
            estimated_throughput: 42.5,
            resource_allocation: AgentRequirements { ram_mb: 1024, cpu_cores: 4, disk_mb: 200 },
        };
        let json = serde_json::to_string(&placement).unwrap();
        let deserialized: AgentPlacement = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.agent_id, "agent-1");
        assert!((deserialized.estimated_throughput - 42.5).abs() < 1e-10);

        // ContentionResult
        let contention = ContentionResult {
            total_cost: 1.5,
            per_node: HashMap::from([(uuid::Uuid::new_v4(), NodeContentionDetail {
                cpu_penalty: 0.2, memory_penalty: 0.3, queue_penalty: 0.1,
                speed_penalty: 0.0, latency_penalty: 0.4, total: 1.5,
            })]),
        };
        let json = serde_json::to_string(&contention).unwrap();
        let deserialized: ContentionResult = serde_json::from_str(&json).unwrap();
        assert!((deserialized.total_cost - 1.5).abs() < 1e-10);

        // PendingDownload
        let download = PendingDownload {
            resource_type: ResourceType::Agent,
            resource_id: "agent:test-agent".to_string(),
            target_node: uuid::Uuid::new_v4(),
            source: crate::network::catalog::DownloadSource {
                source_type: crate::network::catalog::SourceType::HuggingFaceHub,
                url: "https://example.com".to_string(),
                priority: 1,
            },
            size_mb: 500,
            priority: DownloadPriority::Normal,
            depends_on: vec!["model:model-x".to_string()],
        };
        let json = serde_json::to_string(&download).unwrap();
        let deserialized: PendingDownload = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.resource_id, "agent:test-agent");
        assert_eq!(deserialized.depends_on.len(), 1);

        // SolverDiagnostic
        let diagnostic = SolverDiagnostic {
            resource_type: ResourceType::Agent,
            resource_id: "agent-1".to_string(),
            reason: "tool not available".to_string(),
        };
        let json = serde_json::to_string(&diagnostic).unwrap();
        let deserialized: SolverDiagnostic = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.reason, "tool not available");

        // CoSelectionAction
        let action = CoSelectionAction::ModelAdded {
            model_id: "model-a".to_string(),
            reason: "agent-1".to_string(),
        };
        let json = serde_json::to_string(&action).unwrap();
        let deserialized: CoSelectionAction = serde_json::from_str(&json).unwrap();
        match deserialized {
            CoSelectionAction::ModelAdded { model_id, .. } => assert_eq!(model_id, "model-a"),
            _ => panic!("Wrong variant"),
        }
    }
}
