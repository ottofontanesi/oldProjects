// Distributed Agent Execution — Step router
// Phase 15: Route each step to best node (model + tool + trust)
//
// Implements candidate filtering for step routing:
// - Filter by model availability (node has the required model loaded)
// - Filter by tool availability (node has ALL required tools available)
// - Filter by trust tier (sensitive steps require TrustTier::LocalOwned)
//
// Satisfies FR-3.1: Each step is routed to the best node that satisfies ALL requirements.
// Satisfies FR-6.2: Sensitive steps execute only on tier-3 (local-owned) nodes.

use std::collections::HashMap;

use crate::agents::dag::{ExecutionStep, PromptSensitivity, StepId};
use crate::mesh::identity::TrustTier;
use crate::network::registry::{NodeId, NodeState};

// ---------------------------------------------------------------------------
// Routing errors
// ---------------------------------------------------------------------------

/// Errors that can occur during step routing.
#[derive(Debug, Clone, PartialEq)]
pub enum RoutingError {
    /// No candidate nodes satisfy all requirements for the step.
    NoCandidateNodes {
        /// The step that could not be routed.
        step_description: String,
        /// What was missing (human-readable).
        reason: String,
    },
}

impl std::fmt::Display for RoutingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RoutingError::NoCandidateNodes {
                step_description,
                reason,
            } => {
                write!(
                    f,
                    "No candidate nodes for step '{}': {}",
                    step_description, reason
                )
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Candidate filtering
// ---------------------------------------------------------------------------

/// Filter candidate nodes for a given execution step.
///
/// Applies three filters in order:
/// 1. **Online**: Only considers nodes that are currently online.
/// 2. **Model availability**: If the step requires a model, the node must have it loaded.
/// 3. **Tool availability**: The node must have ALL required tools available.
/// 4. **Trust tier**: Sensitive steps require `TrustTier::LocalOwned` (tier 3).
///
/// # Arguments
///
/// * `step` - The execution step to route.
/// * `nodes` - All known node states (online and offline).
/// * `trust_tiers` - Trust tier for each node (from TrustManager or equivalent).
///
/// # Returns
///
/// A list of node IDs that satisfy all requirements, or a `RoutingError` if none qualify.
///
/// Satisfies FR-3.1: Route to node with required model + tools + trust.
/// Satisfies FR-6.2: Sensitive steps only on tier-3 nodes.
pub fn filter_candidates(
    step: &ExecutionStep,
    nodes: &[NodeState],
    trust_tiers: &HashMap<NodeId, TrustTier>,
) -> Result<Vec<NodeId>, RoutingError> {
    let candidates: Vec<NodeId> = nodes
        .iter()
        .filter(|node| {
            // Must be online
            if !node.is_online {
                return false;
            }

            let node_id = node.capabilities.node_id;

            // Model requirement: node must have the required model loaded
            if let Some(ref required_model) = step.required_model {
                let has_model = node
                    .loaded_models
                    .iter()
                    .any(|m| &m.model_id == required_model);
                if !has_model {
                    return false;
                }
            }

            // Tool requirements: node must have ALL required tools available
            for tool_id in &step.required_tools {
                let has_tool = node
                    .capabilities
                    .available_tools
                    .iter()
                    .any(|t| &t.tool_id == tool_id && t.is_available);
                if !has_tool {
                    return false;
                }
            }

            // Trust requirement: sensitive steps require TrustTier::LocalOwned
            if step.sensitivity == PromptSensitivity::Sensitive {
                let tier = trust_tiers.get(&node_id).copied();
                match tier {
                    Some(t) if t >= TrustTier::LocalOwned => {}
                    _ => return false,
                }
            }

            true
        })
        .map(|node| node.capabilities.node_id)
        .collect();

    if candidates.is_empty() {
        return Err(RoutingError::NoCandidateNodes {
            step_description: step.description.clone(),
            reason: build_failure_reason(step, nodes, trust_tiers),
        });
    }

    Ok(candidates)
}

/// Build a human-readable reason explaining why no candidates were found.
fn build_failure_reason(
    step: &ExecutionStep,
    nodes: &[NodeState],
    trust_tiers: &HashMap<NodeId, TrustTier>,
) -> String {
    let online_nodes: Vec<&NodeState> = nodes.iter().filter(|n| n.is_online).collect();

    if online_nodes.is_empty() {
        return "No online nodes available".to_string();
    }

    let mut reasons = Vec::new();

    // Check model availability across online nodes
    if let Some(ref required_model) = step.required_model {
        let nodes_with_model = online_nodes
            .iter()
            .filter(|n| n.loaded_models.iter().any(|m| &m.model_id == required_model))
            .count();
        if nodes_with_model == 0 {
            reasons.push(format!(
                "no online node has model '{}' loaded",
                required_model
            ));
        }
    }

    // Check tool availability
    for tool_id in &step.required_tools {
        let nodes_with_tool = online_nodes
            .iter()
            .filter(|n| {
                n.capabilities
                    .available_tools
                    .iter()
                    .any(|t| &t.tool_id == tool_id && t.is_available)
            })
            .count();
        if nodes_with_tool == 0 {
            reasons.push(format!("no online node has tool '{}' available", tool_id));
        }
    }

    // Check trust tier
    if step.sensitivity == PromptSensitivity::Sensitive {
        let tier3_nodes = online_nodes
            .iter()
            .filter(|n| {
                trust_tiers
                    .get(&n.capabilities.node_id)
                    .copied()
                    .map(|t| t >= TrustTier::LocalOwned)
                    .unwrap_or(false)
            })
            .count();
        if tier3_nodes == 0 {
            reasons.push("no online node has trust tier >= LocalOwned for sensitive step".to_string());
        }
    }

    if reasons.is_empty() {
        "no single node satisfies all requirements simultaneously".to_string()
    } else {
        reasons.join("; ")
    }
}

// ---------------------------------------------------------------------------
// Candidate scoring
// ---------------------------------------------------------------------------

/// A scored candidate node with its composite score.
#[derive(Debug, Clone)]
pub struct ScoredCandidate {
    pub node_id: NodeId,
    pub score: f64,
    pub queue_depth_score: f64,
    pub stability_score: f64,
    pub data_locality_score: f64,
    pub latency_score: f64,
}

/// Scoring weights for candidate selection.
/// Reuses the weighted-scoring pattern from Phase 9A `network/solver.rs`.
const WEIGHT_QUEUE_DEPTH: f64 = 0.3;
const WEIGHT_STABILITY: f64 = 0.2;
const WEIGHT_DATA_LOCALITY: f64 = 0.3;
const WEIGHT_LATENCY: f64 = 0.2;

/// Maximum queue depth used for normalization. Nodes at or above this depth score 0.
const MAX_QUEUE_DEPTH: f64 = 10.0;

/// Maximum RTT (ms) used for normalization. Nodes at or above this latency score 0.
const MAX_RTT_MS: f64 = 100.0;

/// Score candidate nodes for a given execution step.
///
/// For each candidate, computes a composite score in [0.0, 1.0] based on:
/// - **Queue depth** (weight 0.3): Prefer less busy nodes. Score = (1 - queue_depth/10).max(0).
/// - **Stability** (weight 0.2): Prefer stable nodes. Score = node.stability_score.
/// - **Data locality** (weight 0.3): Prefer nodes that already hold output data from the step's
///   input dependencies. Score = (deps_on_node / total_deps) if total_deps > 0, else 0.
/// - **Latency** (weight 0.2): Prefer low-latency nodes relative to the requesting node.
///   Score = (1 - rtt_ms/100).max(0).
///
/// # Arguments
///
/// * `candidate_ids` - Node IDs that passed filtering (from `filter_candidates`).
/// * `nodes` - Full node states for utilization, stability, and latency data.
/// * `step` - The execution step being routed (for data locality via `input_dependencies`).
/// * `requesting_node` - The orchestrator's node ID (for latency scoring).
/// * `data_locations` - Maps each completed step's ID to the node that holds its output data.
///
/// # Returns
///
/// Candidates sorted by score descending (best first).
///
/// Satisfies FR-3.3: Multi-factor scoring for optimal node selection.
/// Satisfies FR-3.4: Data locality awareness reduces inter-node transfers.
/// Satisfies NFR-1.1: Low-latency preference for responsive execution.
/// Satisfies NFR-1.4: Queue-depth awareness prevents overloading busy nodes.
pub fn score_candidates(
    candidate_ids: &[NodeId],
    nodes: &[NodeState],
    step: &ExecutionStep,
    requesting_node: NodeId,
    data_locations: &HashMap<StepId, NodeId>,
) -> Vec<ScoredCandidate> {
    // Build a lookup map for quick node access
    let node_map: HashMap<NodeId, &NodeState> = nodes
        .iter()
        .map(|n| (n.capabilities.node_id, n))
        .collect();

    let mut scored: Vec<ScoredCandidate> = candidate_ids
        .iter()
        .filter_map(|&candidate_id| {
            let node = node_map.get(&candidate_id)?;

            // 1. Queue depth score: prefer less busy nodes
            let queue_depth = node.utilization.queue_depth as f64;
            let queue_score = (1.0 - queue_depth / MAX_QUEUE_DEPTH).max(0.0);

            // 2. Stability score: use node's stability_score directly (already [0, 1])
            let stability = node.stability_score.clamp(0.0, 1.0);

            // 3. Data locality score: fraction of step's input dependencies on this node
            let locality = compute_data_locality(step, candidate_id, data_locations);

            // 4. Latency score: lower RTT to requesting node = higher score
            let latency = compute_latency_score(node, requesting_node);

            // Composite weighted score
            let score = queue_score * WEIGHT_QUEUE_DEPTH
                + stability * WEIGHT_STABILITY
                + locality * WEIGHT_DATA_LOCALITY
                + latency * WEIGHT_LATENCY;

            Some(ScoredCandidate {
                node_id: candidate_id,
                score,
                queue_depth_score: queue_score,
                stability_score: stability,
                data_locality_score: locality,
                latency_score: latency,
            })
        })
        .collect();

    // Sort by score descending (best candidate first)
    scored.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));

    scored
}

/// Compute data locality score for a candidate node.
///
/// Returns the fraction of the step's input dependencies whose output data
/// resides on the candidate node. If the step has no dependencies, returns 0.0.
fn compute_data_locality(
    step: &ExecutionStep,
    candidate_id: NodeId,
    data_locations: &HashMap<StepId, NodeId>,
) -> f64 {
    let total_deps = step.input_dependencies.len();
    if total_deps == 0 {
        return 0.0;
    }

    let deps_on_node = step
        .input_dependencies
        .iter()
        .filter(|dep_id| data_locations.get(dep_id) == Some(&candidate_id))
        .count();

    deps_on_node as f64 / total_deps as f64
}

/// Compute latency score for a candidate node relative to the requesting node.
///
/// Uses the candidate's `latency_to_peers` map to find RTT to the requesting node.
/// If no measurement exists (e.g., the candidate IS the requesting node, or no data),
/// returns 1.0 (best possible — assume local or unmeasured = fast).
fn compute_latency_score(node: &NodeState, requesting_node: NodeId) -> f64 {
    // If the candidate is the requesting node itself, latency is effectively 0
    if node.capabilities.node_id == requesting_node {
        return 1.0;
    }

    match node.latency_to_peers.get(&requesting_node) {
        Some(measurement) => (1.0 - measurement.rtt_ms / MAX_RTT_MS).max(0.0),
        // No measurement available — use neutral score (0.5) to avoid penalizing or rewarding
        None => 0.5,
    }
}

// ---------------------------------------------------------------------------
// Step decomposition (FR-3.2)
// ---------------------------------------------------------------------------

/// A decomposed route: inference runs on one node, tool execution on another.
///
/// When no single node has both the required model and tools, the step is split
/// into an inference sub-step (on the model node) and a tool sub-step (on the
/// tool node), with a data transfer edge between them.
///
/// Satisfies FR-3.2: Decompose step when no single node has model + tools.
#[derive(Debug, Clone, PartialEq)]
pub struct DecomposedRoute {
    /// Node that will run the inference (model) portion of the step.
    pub inference_node: NodeId,
    /// Node that will run the tool execution portion of the step.
    pub tool_node: NodeId,
    /// Sub-step ID for the inference portion.
    pub inference_step_id: StepId,
    /// Sub-step ID for the tool execution portion.
    pub tool_step_id: StepId,
}

/// The result of routing a step: either a single node or a decomposed route.
#[derive(Debug, Clone, PartialEq)]
pub enum RoutingDecision {
    /// Step can execute entirely on one node.
    SingleNode(NodeId),
    /// Step must be split: inference on one node, tools on another.
    Decomposed(DecomposedRoute),
}

/// Attempt to decompose a step into inference + tool sub-steps on separate nodes.
///
/// This is called as a fallback when `filter_candidates` finds no single node that
/// has both the required model AND all required tools. Decomposition only applies
/// when the step requires BOTH a model AND tools.
///
/// # Logic
///
/// 1. Find an online node that has the required model (ignoring tool requirements).
/// 2. Find an online node that has ALL required tools (ignoring model requirements).
/// 3. Both nodes must satisfy trust tier constraints for sensitive steps.
/// 4. If both are found, return a `DecomposedRoute` with new sub-step IDs.
///
/// # Arguments
///
/// * `step` - The execution step that could not be routed to a single node.
/// * `nodes` - All known node states.
/// * `trust_tiers` - Trust tier for each node.
///
/// # Returns
///
/// A `DecomposedRoute` if decomposition is possible, or a `RoutingError` if not.
///
/// Satisfies FR-3.2: Split into inference sub-step and tool sub-step.
pub fn try_decompose_step(
    step: &ExecutionStep,
    nodes: &[NodeState],
    trust_tiers: &HashMap<NodeId, TrustTier>,
) -> Result<DecomposedRoute, RoutingError> {
    // Decomposition only applies when the step requires BOTH a model AND tools.
    let required_model = step.required_model.as_ref().ok_or_else(|| {
        RoutingError::NoCandidateNodes {
            step_description: step.description.clone(),
            reason: "step has no model requirement; decomposition not applicable".to_string(),
        }
    })?;

    if step.required_tools.is_empty() {
        return Err(RoutingError::NoCandidateNodes {
            step_description: step.description.clone(),
            reason: "step has no tool requirements; decomposition not applicable".to_string(),
        });
    }

    // Find a node with the required model (ignoring tool requirements)
    let model_node = nodes
        .iter()
        .filter(|node| {
            if !node.is_online {
                return false;
            }
            // Must have the required model
            let has_model = node
                .loaded_models
                .iter()
                .any(|m| &m.model_id == required_model);
            if !has_model {
                return false;
            }
            // Trust constraint for sensitive steps
            if step.sensitivity == PromptSensitivity::Sensitive {
                let tier = trust_tiers.get(&node.capabilities.node_id).copied();
                match tier {
                    Some(t) if t >= TrustTier::LocalOwned => {}
                    _ => return false,
                }
            }
            true
        })
        .map(|node| node.capabilities.node_id)
        .next();

    // Find a node with ALL required tools (ignoring model requirements)
    let tool_node = nodes
        .iter()
        .filter(|node| {
            if !node.is_online {
                return false;
            }
            // Must have ALL required tools
            for tool_id in &step.required_tools {
                let has_tool = node
                    .capabilities
                    .available_tools
                    .iter()
                    .any(|t| &t.tool_id == tool_id && t.is_available);
                if !has_tool {
                    return false;
                }
            }
            // Trust constraint for sensitive steps
            if step.sensitivity == PromptSensitivity::Sensitive {
                let tier = trust_tiers.get(&node.capabilities.node_id).copied();
                match tier {
                    Some(t) if t >= TrustTier::LocalOwned => {}
                    _ => return false,
                }
            }
            true
        })
        .map(|node| node.capabilities.node_id)
        .next();

    match (model_node, tool_node) {
        (Some(m_node), Some(t_node)) => Ok(DecomposedRoute {
            inference_node: m_node,
            tool_node: t_node,
            inference_step_id: uuid::Uuid::new_v4(),
            tool_step_id: uuid::Uuid::new_v4(),
        }),
        (None, _) => Err(RoutingError::NoCandidateNodes {
            step_description: step.description.clone(),
            reason: format!(
                "no online node has model '{}' loaded (even ignoring tool requirements)",
                required_model
            ),
        }),
        (_, None) => Err(RoutingError::NoCandidateNodes {
            step_description: step.description.clone(),
            reason: format!(
                "no online node has all required tools {:?} available (even ignoring model requirements)",
                step.required_tools
            ),
        }),
    }
}

// ---------------------------------------------------------------------------
// Combined routing: filter + score
// ---------------------------------------------------------------------------

/// Route a step to the best candidate node by filtering then scoring.
///
/// Combines `filter_candidates` (task 3.1) with `score_candidates` (task 3.2)
/// to select the single best node for executing the given step.
///
/// When no single node satisfies all requirements (model + tools + trust),
/// falls back to step decomposition (task 3.3): splits the step into an
/// inference sub-step on a model node and a tool sub-step on a tool node.
///
/// # Arguments
///
/// * `step` - The execution step to route.
/// * `nodes` - All known node states.
/// * `trust_tiers` - Trust tier for each node.
/// * `requesting_node` - The orchestrator's node ID.
/// * `data_locations` - Maps completed step IDs to the node holding their output.
///
/// # Returns
///
/// A `RoutingDecision`: either a single node or a decomposed route.
/// Returns `RoutingError` if neither approach succeeds.
///
/// Satisfies FR-3.1, FR-3.2.
pub fn route_step(
    step: &ExecutionStep,
    nodes: &[NodeState],
    trust_tiers: &HashMap<NodeId, TrustTier>,
    requesting_node: NodeId,
    data_locations: &HashMap<StepId, NodeId>,
) -> Result<RoutingDecision, RoutingError> {
    // Try to find a single node that satisfies all requirements
    match filter_candidates(step, nodes, trust_tiers) {
        Ok(candidates) => {
            let scored =
                score_candidates(&candidates, nodes, step, requesting_node, data_locations);

            // scored is sorted best-first; take the top candidate
            scored
                .first()
                .map(|s| RoutingDecision::SingleNode(s.node_id))
                .ok_or_else(|| RoutingError::NoCandidateNodes {
                    step_description: step.description.clone(),
                    reason: "scoring produced no results".to_string(),
                })
        }
        Err(_) => {
            // No single node has everything — try decomposition as fallback
            let decomposed = try_decompose_step(step, nodes, trust_tiers)?;
            Ok(RoutingDecision::Decomposed(decomposed))
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::dag::{PromptSensitivity, StepStatus};
    use crate::agents::tools::{ToolCapability, ToolCategory, ToolResources};
    use crate::network::registry::*;

    // ─── Test Helpers ────────────────────────────────────────────────────────

    fn make_tool(tool_id: &str, available: bool) -> ToolCapability {
        ToolCapability {
            tool_id: tool_id.to_string(),
            tool_name: tool_id.to_string(),
            category: ToolCategory::Custom(tool_id.to_string()),
            resource_requirements: ToolResources {
                cpu_cores: None,
                ram_mb: None,
                gpu_required: false,
                network_required: false,
            },
            is_available: available,
            version: "1.0".to_string(),
        }
    }

    fn make_node_state(node_id: NodeId, online: bool) -> NodeState {
        NodeState {
            capabilities: NodeCapabilities {
                node_id,
                hostname: format!("node-{}", &node_id.to_string()[..8]),
                device_type: DeviceType::Desktop,
                cpu: CpuProfile {
                    cores: 8,
                    architecture: "x86_64".to_string(),
                    clock_mhz: 4000,
                    isa_extensions: vec![],
                },
                ram: RamProfile {
                    total_mb: 32768,
                    available_mb: 24000,
                    ddr_generation: 4,
                },
                gpu: None,
                storage: StorageProfile {
                    storage_type: StorageType::Nvme,
                    available_mb: 500000,
                    read_speed_mbps: 7000,
                },
                network_interfaces: vec![],
                phone_info: None,
                available_tools: vec![],
            },
            utilization: NodeUtilization::default(),
            loaded_models: vec![],
            stability_score: 0.95,
            last_heartbeat_ms: 0,
            is_online: online,
            latency_to_peers: HashMap::new(),
            thermal_state: ThermalState::default(),
        }
    }

    fn make_step(description: &str) -> ExecutionStep {
        ExecutionStep {
            step_id: uuid::Uuid::new_v4(),
            description: description.to_string(),
            required_model: None,
            required_tools: vec![],
            sensitivity: PromptSensitivity::NonSensitive,
            estimated_compute_ms: 1000,
            input_dependencies: vec![],
            status: StepStatus::Ready,
            assigned_node: None,
            result: None,
        }
    }

    // ─── Unit Tests ──────────────────────────────────────────────────────────

    #[test]
    fn test_filter_returns_all_online_nodes_for_unconstrained_step() {
        let id1 = uuid::Uuid::new_v4();
        let id2 = uuid::Uuid::new_v4();
        let id3 = uuid::Uuid::new_v4();

        let nodes = vec![
            make_node_state(id1, true),
            make_node_state(id2, true),
            make_node_state(id3, false), // offline
        ];
        let trust_tiers = HashMap::new();
        let step = make_step("simple step");

        let result = filter_candidates(&step, &nodes, &trust_tiers).unwrap();
        assert_eq!(result.len(), 2);
        assert!(result.contains(&id1));
        assert!(result.contains(&id2));
        assert!(!result.contains(&id3));
    }

    #[test]
    fn test_filter_excludes_offline_nodes() {
        let id1 = uuid::Uuid::new_v4();
        let nodes = vec![make_node_state(id1, false)];
        let trust_tiers = HashMap::new();
        let step = make_step("step");

        let result = filter_candidates(&step, &nodes, &trust_tiers);
        assert!(result.is_err());
        match result.unwrap_err() {
            RoutingError::NoCandidateNodes { reason, .. } => {
                assert!(reason.contains("No online nodes"));
            }
        }
    }

    #[test]
    fn test_filter_by_model_availability() {
        let id1 = uuid::Uuid::new_v4();
        let id2 = uuid::Uuid::new_v4();

        let mut node1 = make_node_state(id1, true);
        node1.loaded_models.push(LoadedModelInfo {
            model_id: "qwen2.5:14b".to_string(),
            ram_used_mb: 8000,
            vram_used_mb: 0,
            active_requests: 0,
            avg_tok_s: 30.0,
        });

        let node2 = make_node_state(id2, true); // no models loaded

        let nodes = vec![node1, node2];
        let trust_tiers = HashMap::new();

        let mut step = make_step("inference step");
        step.required_model = Some("qwen2.5:14b".to_string());

        let result = filter_candidates(&step, &nodes, &trust_tiers).unwrap();
        assert_eq!(result, vec![id1]);
    }

    #[test]
    fn test_filter_by_model_no_match() {
        let id1 = uuid::Uuid::new_v4();
        let mut node1 = make_node_state(id1, true);
        node1.loaded_models.push(LoadedModelInfo {
            model_id: "llama3:7b".to_string(),
            ram_used_mb: 4000,
            vram_used_mb: 0,
            active_requests: 0,
            avg_tok_s: 20.0,
        });

        let nodes = vec![node1];
        let trust_tiers = HashMap::new();

        let mut step = make_step("needs big model");
        step.required_model = Some("llama3:70b".to_string());

        let result = filter_candidates(&step, &nodes, &trust_tiers);
        assert!(result.is_err());
    }

    #[test]
    fn test_filter_by_tool_availability() {
        let id1 = uuid::Uuid::new_v4();
        let id2 = uuid::Uuid::new_v4();

        let mut node1 = make_node_state(id1, true);
        node1.capabilities.available_tools = vec![
            make_tool("browser", true),
            make_tool("filesystem", true),
        ];

        let mut node2 = make_node_state(id2, true);
        node2.capabilities.available_tools = vec![
            make_tool("filesystem", true),
        ];

        let nodes = vec![node1, node2];
        let trust_tiers = HashMap::new();

        let mut step = make_step("browse and read");
        step.required_tools = vec!["browser".to_string(), "filesystem".to_string()];

        let result = filter_candidates(&step, &nodes, &trust_tiers).unwrap();
        assert_eq!(result, vec![id1]);
    }

    #[test]
    fn test_filter_tool_unavailable_flag() {
        let id1 = uuid::Uuid::new_v4();

        let mut node1 = make_node_state(id1, true);
        // Tool exists but is marked unavailable
        node1.capabilities.available_tools = vec![make_tool("browser", false)];

        let nodes = vec![node1];
        let trust_tiers = HashMap::new();

        let mut step = make_step("browse");
        step.required_tools = vec!["browser".to_string()];

        let result = filter_candidates(&step, &nodes, &trust_tiers);
        assert!(result.is_err());
    }

    #[test]
    fn test_filter_by_trust_tier_sensitive_step() {
        let id1 = uuid::Uuid::new_v4();
        let id2 = uuid::Uuid::new_v4();
        let id3 = uuid::Uuid::new_v4();

        let nodes = vec![
            make_node_state(id1, true),
            make_node_state(id2, true),
            make_node_state(id3, true),
        ];

        let mut trust_tiers = HashMap::new();
        trust_tiers.insert(id1, TrustTier::Public);
        trust_tiers.insert(id2, TrustTier::InvitedFriend);
        trust_tiers.insert(id3, TrustTier::LocalOwned);

        let mut step = make_step("private step");
        step.sensitivity = PromptSensitivity::Sensitive;

        let result = filter_candidates(&step, &nodes, &trust_tiers).unwrap();
        assert_eq!(result, vec![id3]);
    }

    #[test]
    fn test_filter_non_sensitive_step_ignores_trust() {
        let id1 = uuid::Uuid::new_v4();
        let id2 = uuid::Uuid::new_v4();

        let nodes = vec![
            make_node_state(id1, true),
            make_node_state(id2, true),
        ];

        let mut trust_tiers = HashMap::new();
        trust_tiers.insert(id1, TrustTier::Public);
        trust_tiers.insert(id2, TrustTier::InvitedFriend);

        let step = make_step("public step");

        let result = filter_candidates(&step, &nodes, &trust_tiers).unwrap();
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_filter_sensitive_step_no_trust_info_excluded() {
        let id1 = uuid::Uuid::new_v4();

        let nodes = vec![make_node_state(id1, true)];
        // No trust tier info for this node
        let trust_tiers = HashMap::new();

        let mut step = make_step("sensitive step");
        step.sensitivity = PromptSensitivity::Sensitive;

        let result = filter_candidates(&step, &nodes, &trust_tiers);
        assert!(result.is_err());
    }

    #[test]
    fn test_filter_combined_model_tool_trust() {
        let id1 = uuid::Uuid::new_v4();
        let id2 = uuid::Uuid::new_v4();
        let id3 = uuid::Uuid::new_v4();

        // Node 1: has model + tools + tier 3
        let mut node1 = make_node_state(id1, true);
        node1.loaded_models.push(LoadedModelInfo {
            model_id: "qwen2.5:14b".to_string(),
            ram_used_mb: 8000,
            vram_used_mb: 0,
            active_requests: 0,
            avg_tok_s: 30.0,
        });
        node1.capabilities.available_tools = vec![make_tool("code_exec", true)];

        // Node 2: has model + tools but only tier 2
        let mut node2 = make_node_state(id2, true);
        node2.loaded_models.push(LoadedModelInfo {
            model_id: "qwen2.5:14b".to_string(),
            ram_used_mb: 8000,
            vram_used_mb: 0,
            active_requests: 0,
            avg_tok_s: 25.0,
        });
        node2.capabilities.available_tools = vec![make_tool("code_exec", true)];

        // Node 3: tier 3 but no model
        let mut node3 = make_node_state(id3, true);
        node3.capabilities.available_tools = vec![make_tool("code_exec", true)];

        let nodes = vec![node1, node2, node3];

        let mut trust_tiers = HashMap::new();
        trust_tiers.insert(id1, TrustTier::LocalOwned);
        trust_tiers.insert(id2, TrustTier::InvitedFriend);
        trust_tiers.insert(id3, TrustTier::LocalOwned);

        let mut step = make_step("sensitive inference with tools");
        step.required_model = Some("qwen2.5:14b".to_string());
        step.required_tools = vec!["code_exec".to_string()];
        step.sensitivity = PromptSensitivity::Sensitive;

        // Only node1 satisfies all three: model + tool + trust
        let result = filter_candidates(&step, &nodes, &trust_tiers).unwrap();
        assert_eq!(result, vec![id1]);
    }

    #[test]
    fn test_filter_empty_nodes_returns_error() {
        let nodes: Vec<NodeState> = vec![];
        let trust_tiers = HashMap::new();
        let step = make_step("any step");

        let result = filter_candidates(&step, &nodes, &trust_tiers);
        assert!(result.is_err());
    }

    #[test]
    fn test_error_message_includes_missing_model() {
        let id1 = uuid::Uuid::new_v4();
        let nodes = vec![make_node_state(id1, true)];
        let trust_tiers = HashMap::new();

        let mut step = make_step("needs model");
        step.required_model = Some("nonexistent:model".to_string());

        let err = filter_candidates(&step, &nodes, &trust_tiers).unwrap_err();
        match err {
            RoutingError::NoCandidateNodes { reason, .. } => {
                assert!(reason.contains("nonexistent:model"));
            }
        }
    }

    #[test]
    fn test_error_message_includes_missing_tool() {
        let id1 = uuid::Uuid::new_v4();
        let nodes = vec![make_node_state(id1, true)];
        let trust_tiers = HashMap::new();

        let mut step = make_step("needs tool");
        step.required_tools = vec!["magic_tool".to_string()];

        let err = filter_candidates(&step, &nodes, &trust_tiers).unwrap_err();
        match err {
            RoutingError::NoCandidateNodes { reason, .. } => {
                assert!(reason.contains("magic_tool"));
            }
        }
    }

    // ─── Scoring Tests ───────────────────────────────────────────────────────

    #[test]
    fn test_score_prefers_less_busy_node() {
        let id1 = uuid::Uuid::new_v4();
        let id2 = uuid::Uuid::new_v4();
        let requesting = uuid::Uuid::new_v4();

        let mut node1 = make_node_state(id1, true);
        node1.utilization.queue_depth = 8; // busy
        node1.stability_score = 0.9;

        let mut node2 = make_node_state(id2, true);
        node2.utilization.queue_depth = 1; // idle
        node2.stability_score = 0.9;

        let nodes = vec![node1, node2];
        let step = make_step("simple step");
        let data_locations = HashMap::new();

        let scored = score_candidates(&[id1, id2], &nodes, &step, requesting, &data_locations);

        assert_eq!(scored.len(), 2);
        // Node2 (queue_depth=1) should score higher than node1 (queue_depth=8)
        assert_eq!(scored[0].node_id, id2);
        assert!(scored[0].queue_depth_score > scored[1].queue_depth_score);
    }

    #[test]
    fn test_score_prefers_stable_node() {
        let id1 = uuid::Uuid::new_v4();
        let id2 = uuid::Uuid::new_v4();
        let requesting = uuid::Uuid::new_v4();

        let mut node1 = make_node_state(id1, true);
        node1.stability_score = 0.3; // unstable
        node1.utilization.queue_depth = 0;

        let mut node2 = make_node_state(id2, true);
        node2.stability_score = 1.0; // very stable
        node2.utilization.queue_depth = 0;

        let nodes = vec![node1, node2];
        let step = make_step("step");
        let data_locations = HashMap::new();

        let scored = score_candidates(&[id1, id2], &nodes, &step, requesting, &data_locations);

        assert_eq!(scored[0].node_id, id2);
        assert!(scored[0].stability_score > scored[1].stability_score);
    }

    #[test]
    fn test_score_prefers_data_locality() {
        let id1 = uuid::Uuid::new_v4();
        let id2 = uuid::Uuid::new_v4();
        let requesting = uuid::Uuid::new_v4();
        let dep1 = uuid::Uuid::new_v4();
        let dep2 = uuid::Uuid::new_v4();

        let mut node1 = make_node_state(id1, true);
        node1.stability_score = 0.9;
        node1.utilization.queue_depth = 0;

        let mut node2 = make_node_state(id2, true);
        node2.stability_score = 0.9;
        node2.utilization.queue_depth = 0;

        let nodes = vec![node1, node2];

        // Step depends on dep1 and dep2; both are on node1
        let mut step = make_step("data-heavy step");
        step.input_dependencies = vec![dep1, dep2];

        let mut data_locations = HashMap::new();
        data_locations.insert(dep1, id1);
        data_locations.insert(dep2, id1);

        let scored = score_candidates(&[id1, id2], &nodes, &step, requesting, &data_locations);

        assert_eq!(scored[0].node_id, id1);
        assert_eq!(scored[0].data_locality_score, 1.0); // all deps on this node
        assert_eq!(scored[1].data_locality_score, 0.0); // no deps on this node
    }

    #[test]
    fn test_score_partial_data_locality() {
        let id1 = uuid::Uuid::new_v4();
        let id2 = uuid::Uuid::new_v4();
        let requesting = uuid::Uuid::new_v4();
        let dep1 = uuid::Uuid::new_v4();
        let dep2 = uuid::Uuid::new_v4();

        let mut node1 = make_node_state(id1, true);
        node1.stability_score = 0.9;
        node1.utilization.queue_depth = 0;

        let mut node2 = make_node_state(id2, true);
        node2.stability_score = 0.9;
        node2.utilization.queue_depth = 0;

        let nodes = vec![node1, node2];

        let mut step = make_step("step with deps");
        step.input_dependencies = vec![dep1, dep2];

        // dep1 on node1, dep2 on node2
        let mut data_locations = HashMap::new();
        data_locations.insert(dep1, id1);
        data_locations.insert(dep2, id2);

        let scored = score_candidates(&[id1, id2], &nodes, &step, requesting, &data_locations);

        // Both should have 0.5 data locality (1 of 2 deps)
        assert_eq!(scored[0].data_locality_score, 0.5);
        assert_eq!(scored[1].data_locality_score, 0.5);
    }

    #[test]
    fn test_score_prefers_low_latency() {
        let id1 = uuid::Uuid::new_v4();
        let id2 = uuid::Uuid::new_v4();
        let requesting = uuid::Uuid::new_v4();

        let mut node1 = make_node_state(id1, true);
        node1.stability_score = 0.9;
        node1.utilization.queue_depth = 0;
        node1.latency_to_peers.insert(
            requesting,
            LatencyMeasurement {
                peer_id: requesting,
                rtt_ms: 80.0, // high latency
                bandwidth_mbps: 100.0,
                measured_at_ms: 0,
            },
        );

        let mut node2 = make_node_state(id2, true);
        node2.stability_score = 0.9;
        node2.utilization.queue_depth = 0;
        node2.latency_to_peers.insert(
            requesting,
            LatencyMeasurement {
                peer_id: requesting,
                rtt_ms: 5.0, // low latency
                bandwidth_mbps: 1000.0,
                measured_at_ms: 0,
            },
        );

        let nodes = vec![node1, node2];
        let step = make_step("latency-sensitive step");
        let data_locations = HashMap::new();

        let scored = score_candidates(&[id1, id2], &nodes, &step, requesting, &data_locations);

        assert_eq!(scored[0].node_id, id2);
        assert!(scored[0].latency_score > scored[1].latency_score);
    }

    #[test]
    fn test_score_requesting_node_gets_max_latency_score() {
        let requesting = uuid::Uuid::new_v4();
        let id2 = uuid::Uuid::new_v4();

        let mut node1 = make_node_state(requesting, true);
        node1.stability_score = 0.9;
        node1.utilization.queue_depth = 0;

        let mut node2 = make_node_state(id2, true);
        node2.stability_score = 0.9;
        node2.utilization.queue_depth = 0;
        node2.latency_to_peers.insert(
            requesting,
            LatencyMeasurement {
                peer_id: requesting,
                rtt_ms: 10.0,
                bandwidth_mbps: 500.0,
                measured_at_ms: 0,
            },
        );

        let nodes = vec![node1, node2];
        let step = make_step("step");
        let data_locations = HashMap::new();

        let scored =
            score_candidates(&[requesting, id2], &nodes, &step, requesting, &data_locations);

        // The requesting node itself should get latency_score = 1.0
        let requesting_scored = scored.iter().find(|s| s.node_id == requesting).unwrap();
        assert_eq!(requesting_scored.latency_score, 1.0);
    }

    #[test]
    fn test_score_no_latency_measurement_gives_neutral() {
        let id1 = uuid::Uuid::new_v4();
        let requesting = uuid::Uuid::new_v4();

        let mut node1 = make_node_state(id1, true);
        node1.stability_score = 0.9;
        node1.utilization.queue_depth = 0;
        // No latency_to_peers entry for requesting node

        let nodes = vec![node1];
        let step = make_step("step");
        let data_locations = HashMap::new();

        let scored = score_candidates(&[id1], &nodes, &step, requesting, &data_locations);

        assert_eq!(scored[0].latency_score, 0.5); // neutral when no measurement
    }

    #[test]
    fn test_score_queue_depth_at_max_gives_zero() {
        let id1 = uuid::Uuid::new_v4();
        let requesting = uuid::Uuid::new_v4();

        let mut node1 = make_node_state(id1, true);
        node1.utilization.queue_depth = 10; // at max
        node1.stability_score = 0.9;

        let nodes = vec![node1];
        let step = make_step("step");
        let data_locations = HashMap::new();

        let scored = score_candidates(&[id1], &nodes, &step, requesting, &data_locations);

        assert_eq!(scored[0].queue_depth_score, 0.0);
    }

    #[test]
    fn test_score_queue_depth_above_max_clamped_to_zero() {
        let id1 = uuid::Uuid::new_v4();
        let requesting = uuid::Uuid::new_v4();

        let mut node1 = make_node_state(id1, true);
        node1.utilization.queue_depth = 15; // above max
        node1.stability_score = 0.9;

        let nodes = vec![node1];
        let step = make_step("step");
        let data_locations = HashMap::new();

        let scored = score_candidates(&[id1], &nodes, &step, requesting, &data_locations);

        assert_eq!(scored[0].queue_depth_score, 0.0);
    }

    #[test]
    fn test_score_composite_weights_sum_correctly() {
        let id1 = uuid::Uuid::new_v4();
        let requesting = id1; // same node = latency 1.0
        let dep1 = uuid::Uuid::new_v4();

        let mut node1 = make_node_state(id1, true);
        node1.utilization.queue_depth = 0; // queue score = 1.0
        node1.stability_score = 1.0; // stability = 1.0

        let nodes = vec![node1];

        let mut step = make_step("perfect step");
        step.input_dependencies = vec![dep1];

        // dep1 is on this node
        let mut data_locations = HashMap::new();
        data_locations.insert(dep1, id1);

        let scored = score_candidates(&[id1], &nodes, &step, requesting, &data_locations);

        // All components at max: 1.0*0.3 + 1.0*0.2 + 1.0*0.3 + 1.0*0.2 = 1.0
        let expected = 1.0 * 0.3 + 1.0 * 0.2 + 1.0 * 0.3 + 1.0 * 0.2;
        assert!((scored[0].score - expected).abs() < 1e-10);
    }

    #[test]
    fn test_score_no_dependencies_locality_is_zero() {
        let id1 = uuid::Uuid::new_v4();
        let requesting = uuid::Uuid::new_v4();

        let mut node1 = make_node_state(id1, true);
        node1.stability_score = 0.9;
        node1.utilization.queue_depth = 0;

        let nodes = vec![node1];
        let step = make_step("no deps"); // input_dependencies is empty
        let data_locations = HashMap::new();

        let scored = score_candidates(&[id1], &nodes, &step, requesting, &data_locations);

        assert_eq!(scored[0].data_locality_score, 0.0);
    }

    // ─── route_step integration tests ────────────────────────────────────────

    #[test]
    fn test_route_step_selects_best_candidate() {
        let id1 = uuid::Uuid::new_v4();
        let id2 = uuid::Uuid::new_v4();
        let requesting = uuid::Uuid::new_v4();

        // Node1: busy, unstable
        let mut node1 = make_node_state(id1, true);
        node1.utilization.queue_depth = 8;
        node1.stability_score = 0.3;

        // Node2: idle, stable
        let mut node2 = make_node_state(id2, true);
        node2.utilization.queue_depth = 0;
        node2.stability_score = 1.0;

        let nodes = vec![node1, node2];
        let trust_tiers = HashMap::new();
        let data_locations = HashMap::new();
        let step = make_step("route me");

        let result = route_step(&step, &nodes, &trust_tiers, requesting, &data_locations).unwrap();
        assert_eq!(result, RoutingDecision::SingleNode(id2));
    }

    #[test]
    fn test_route_step_returns_error_when_no_candidates() {
        let nodes: Vec<NodeState> = vec![];
        let trust_tiers = HashMap::new();
        let data_locations = HashMap::new();
        let requesting = uuid::Uuid::new_v4();
        let step = make_step("impossible step");

        let result = route_step(&step, &nodes, &trust_tiers, requesting, &data_locations);
        assert!(result.is_err());
    }

    #[test]
    fn test_route_step_with_data_locality_preference() {
        let id1 = uuid::Uuid::new_v4();
        let id2 = uuid::Uuid::new_v4();
        let requesting = uuid::Uuid::new_v4();
        let dep1 = uuid::Uuid::new_v4();
        let dep2 = uuid::Uuid::new_v4();
        let dep3 = uuid::Uuid::new_v4();

        // Both nodes are similar in queue/stability/latency
        let mut node1 = make_node_state(id1, true);
        node1.utilization.queue_depth = 1;
        node1.stability_score = 0.9;

        let mut node2 = make_node_state(id2, true);
        node2.utilization.queue_depth = 1;
        node2.stability_score = 0.9;

        let nodes = vec![node1, node2];
        let trust_tiers = HashMap::new();

        // Step depends on 3 deps; all on node1
        let mut step = make_step("data-local step");
        step.input_dependencies = vec![dep1, dep2, dep3];

        let mut data_locations = HashMap::new();
        data_locations.insert(dep1, id1);
        data_locations.insert(dep2, id1);
        data_locations.insert(dep3, id1);

        let result =
            route_step(&step, &nodes, &trust_tiers, requesting, &data_locations).unwrap();
        // Node1 should win due to data locality (weight 0.3)
        assert_eq!(result, RoutingDecision::SingleNode(id1));
    }

    // ─── Step Decomposition Tests ────────────────────────────────────────────

    #[test]
    fn test_decompose_step_splits_model_and_tools() {
        let model_node_id = uuid::Uuid::new_v4();
        let tool_node_id = uuid::Uuid::new_v4();

        // Node 1: has model but no tools
        let mut model_node = make_node_state(model_node_id, true);
        model_node.loaded_models.push(LoadedModelInfo {
            model_id: "qwen2.5:14b".to_string(),
            ram_used_mb: 8000,
            vram_used_mb: 0,
            active_requests: 0,
            avg_tok_s: 30.0,
        });

        // Node 2: has tools but no model
        let mut tool_node = make_node_state(tool_node_id, true);
        tool_node.capabilities.available_tools = vec![
            make_tool("browser", true),
            make_tool("filesystem", true),
        ];

        let nodes = vec![model_node, tool_node];
        let trust_tiers = HashMap::new();

        let mut step = make_step("needs model and tools");
        step.required_model = Some("qwen2.5:14b".to_string());
        step.required_tools = vec!["browser".to_string(), "filesystem".to_string()];

        let result = try_decompose_step(&step, &nodes, &trust_tiers).unwrap();
        assert_eq!(result.inference_node, model_node_id);
        assert_eq!(result.tool_node, tool_node_id);
        // Sub-step IDs should be distinct
        assert_ne!(result.inference_step_id, result.tool_step_id);
    }

    #[test]
    fn test_decompose_step_fails_when_no_model_node() {
        let tool_node_id = uuid::Uuid::new_v4();

        // Only a tool node, no model node
        let mut tool_node = make_node_state(tool_node_id, true);
        tool_node.capabilities.available_tools = vec![make_tool("browser", true)];

        let nodes = vec![tool_node];
        let trust_tiers = HashMap::new();

        let mut step = make_step("needs model and tools");
        step.required_model = Some("qwen2.5:14b".to_string());
        step.required_tools = vec!["browser".to_string()];

        let result = try_decompose_step(&step, &nodes, &trust_tiers);
        assert!(result.is_err());
        match result.unwrap_err() {
            RoutingError::NoCandidateNodes { reason, .. } => {
                assert!(reason.contains("no online node has model"));
            }
        }
    }

    #[test]
    fn test_decompose_step_fails_when_no_tool_node() {
        let model_node_id = uuid::Uuid::new_v4();

        // Only a model node, no tool node
        let mut model_node = make_node_state(model_node_id, true);
        model_node.loaded_models.push(LoadedModelInfo {
            model_id: "qwen2.5:14b".to_string(),
            ram_used_mb: 8000,
            vram_used_mb: 0,
            active_requests: 0,
            avg_tok_s: 30.0,
        });

        let nodes = vec![model_node];
        let trust_tiers = HashMap::new();

        let mut step = make_step("needs model and tools");
        step.required_model = Some("qwen2.5:14b".to_string());
        step.required_tools = vec!["browser".to_string()];

        let result = try_decompose_step(&step, &nodes, &trust_tiers);
        assert!(result.is_err());
        match result.unwrap_err() {
            RoutingError::NoCandidateNodes { reason, .. } => {
                assert!(reason.contains("no online node has all required tools"));
            }
        }
    }

    #[test]
    fn test_decompose_step_not_applicable_without_model() {
        let id1 = uuid::Uuid::new_v4();
        let mut node1 = make_node_state(id1, true);
        node1.capabilities.available_tools = vec![make_tool("browser", true)];

        let nodes = vec![node1];
        let trust_tiers = HashMap::new();

        // Step only needs tools, no model
        let mut step = make_step("tools only");
        step.required_tools = vec!["browser".to_string()];

        let result = try_decompose_step(&step, &nodes, &trust_tiers);
        assert!(result.is_err());
        match result.unwrap_err() {
            RoutingError::NoCandidateNodes { reason, .. } => {
                assert!(reason.contains("no model requirement"));
            }
        }
    }

    #[test]
    fn test_decompose_step_not_applicable_without_tools() {
        let id1 = uuid::Uuid::new_v4();
        let mut node1 = make_node_state(id1, true);
        node1.loaded_models.push(LoadedModelInfo {
            model_id: "qwen2.5:14b".to_string(),
            ram_used_mb: 8000,
            vram_used_mb: 0,
            active_requests: 0,
            avg_tok_s: 30.0,
        });

        let nodes = vec![node1];
        let trust_tiers = HashMap::new();

        // Step only needs model, no tools
        let mut step = make_step("model only");
        step.required_model = Some("qwen2.5:14b".to_string());

        let result = try_decompose_step(&step, &nodes, &trust_tiers);
        assert!(result.is_err());
        match result.unwrap_err() {
            RoutingError::NoCandidateNodes { reason, .. } => {
                assert!(reason.contains("no tool requirements"));
            }
        }
    }

    #[test]
    fn test_decompose_step_respects_trust_for_sensitive_step() {
        let model_node_id = uuid::Uuid::new_v4();
        let tool_node_id = uuid::Uuid::new_v4();
        let untrusted_tool_node_id = uuid::Uuid::new_v4();

        // Model node: has model, tier 3
        let mut model_node = make_node_state(model_node_id, true);
        model_node.loaded_models.push(LoadedModelInfo {
            model_id: "qwen2.5:14b".to_string(),
            ram_used_mb: 8000,
            vram_used_mb: 0,
            active_requests: 0,
            avg_tok_s: 30.0,
        });

        // Tool node (trusted): has tools, tier 3
        let mut tool_node = make_node_state(tool_node_id, true);
        tool_node.capabilities.available_tools = vec![make_tool("browser", true)];

        // Tool node (untrusted): has tools, tier 1
        let mut untrusted_tool_node = make_node_state(untrusted_tool_node_id, true);
        untrusted_tool_node.capabilities.available_tools = vec![make_tool("browser", true)];

        let nodes = vec![model_node, untrusted_tool_node, tool_node];

        let mut trust_tiers = HashMap::new();
        trust_tiers.insert(model_node_id, TrustTier::LocalOwned);
        trust_tiers.insert(untrusted_tool_node_id, TrustTier::Public);
        trust_tiers.insert(tool_node_id, TrustTier::LocalOwned);

        let mut step = make_step("sensitive decomposed step");
        step.required_model = Some("qwen2.5:14b".to_string());
        step.required_tools = vec!["browser".to_string()];
        step.sensitivity = PromptSensitivity::Sensitive;

        let result = try_decompose_step(&step, &nodes, &trust_tiers).unwrap();
        assert_eq!(result.inference_node, model_node_id);
        // Should pick the trusted tool node, not the untrusted one
        assert_eq!(result.tool_node, tool_node_id);
    }

    #[test]
    fn test_decompose_step_fails_sensitive_no_trusted_model_node() {
        let model_node_id = uuid::Uuid::new_v4();
        let tool_node_id = uuid::Uuid::new_v4();

        // Model node: has model but only tier 1
        let mut model_node = make_node_state(model_node_id, true);
        model_node.loaded_models.push(LoadedModelInfo {
            model_id: "qwen2.5:14b".to_string(),
            ram_used_mb: 8000,
            vram_used_mb: 0,
            active_requests: 0,
            avg_tok_s: 30.0,
        });

        // Tool node: has tools, tier 3
        let mut tool_node = make_node_state(tool_node_id, true);
        tool_node.capabilities.available_tools = vec![make_tool("browser", true)];

        let nodes = vec![model_node, tool_node];

        let mut trust_tiers = HashMap::new();
        trust_tiers.insert(model_node_id, TrustTier::Public);
        trust_tiers.insert(tool_node_id, TrustTier::LocalOwned);

        let mut step = make_step("sensitive step");
        step.required_model = Some("qwen2.5:14b".to_string());
        step.required_tools = vec!["browser".to_string()];
        step.sensitivity = PromptSensitivity::Sensitive;

        let result = try_decompose_step(&step, &nodes, &trust_tiers);
        assert!(result.is_err());
        match result.unwrap_err() {
            RoutingError::NoCandidateNodes { reason, .. } => {
                assert!(reason.contains("no online node has model"));
            }
        }
    }

    #[test]
    fn test_route_step_falls_back_to_decomposition() {
        let model_node_id = uuid::Uuid::new_v4();
        let tool_node_id = uuid::Uuid::new_v4();
        let requesting = uuid::Uuid::new_v4();

        // Node 1: has model but no tools
        let mut model_node = make_node_state(model_node_id, true);
        model_node.loaded_models.push(LoadedModelInfo {
            model_id: "qwen2.5:14b".to_string(),
            ram_used_mb: 8000,
            vram_used_mb: 0,
            active_requests: 0,
            avg_tok_s: 30.0,
        });

        // Node 2: has tools but no model
        let mut tool_node = make_node_state(tool_node_id, true);
        tool_node.capabilities.available_tools = vec![make_tool("browser", true)];

        let nodes = vec![model_node, tool_node];
        let trust_tiers = HashMap::new();
        let data_locations = HashMap::new();

        let mut step = make_step("needs both");
        step.required_model = Some("qwen2.5:14b".to_string());
        step.required_tools = vec!["browser".to_string()];

        let result =
            route_step(&step, &nodes, &trust_tiers, requesting, &data_locations).unwrap();

        match result {
            RoutingDecision::Decomposed(decomposed) => {
                assert_eq!(decomposed.inference_node, model_node_id);
                assert_eq!(decomposed.tool_node, tool_node_id);
            }
            RoutingDecision::SingleNode(_) => {
                panic!("Expected decomposed route, got single node");
            }
        }
    }

    #[test]
    fn test_route_step_prefers_single_node_over_decomposition() {
        let combined_node_id = uuid::Uuid::new_v4();
        let model_only_id = uuid::Uuid::new_v4();
        let requesting = uuid::Uuid::new_v4();

        // Node 1: has BOTH model and tools
        let mut combined_node = make_node_state(combined_node_id, true);
        combined_node.loaded_models.push(LoadedModelInfo {
            model_id: "qwen2.5:14b".to_string(),
            ram_used_mb: 8000,
            vram_used_mb: 0,
            active_requests: 0,
            avg_tok_s: 30.0,
        });
        combined_node.capabilities.available_tools = vec![make_tool("browser", true)];

        // Node 2: has model only
        let mut model_only = make_node_state(model_only_id, true);
        model_only.loaded_models.push(LoadedModelInfo {
            model_id: "qwen2.5:14b".to_string(),
            ram_used_mb: 8000,
            vram_used_mb: 0,
            active_requests: 0,
            avg_tok_s: 30.0,
        });

        let nodes = vec![combined_node, model_only];
        let trust_tiers = HashMap::new();
        let data_locations = HashMap::new();

        let mut step = make_step("needs both");
        step.required_model = Some("qwen2.5:14b".to_string());
        step.required_tools = vec!["browser".to_string()];

        let result =
            route_step(&step, &nodes, &trust_tiers, requesting, &data_locations).unwrap();

        // Should pick single node, not decompose
        assert_eq!(result, RoutingDecision::SingleNode(combined_node_id));
    }

    #[test]
    fn test_route_step_decomposition_error_when_nothing_available() {
        let requesting = uuid::Uuid::new_v4();

        // No nodes at all
        let nodes: Vec<NodeState> = vec![];
        let trust_tiers = HashMap::new();
        let data_locations = HashMap::new();

        let mut step = make_step("needs both");
        step.required_model = Some("qwen2.5:14b".to_string());
        step.required_tools = vec!["browser".to_string()];

        let result = route_step(&step, &nodes, &trust_tiers, requesting, &data_locations);
        assert!(result.is_err());
    }
}

// ---------------------------------------------------------------------------
// Property-Based Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod proptest_trust_enforcement {
    use super::*;
    use crate::agents::dag::{ExecutionStep, PromptSensitivity, StepStatus};
    use crate::agents::tools::{ToolCapability, ToolCategory, ToolResources};
    use crate::mesh::identity::TrustTier;
    use crate::network::registry::*;
    use proptest::prelude::*;
    use std::collections::HashMap;

    // ─── Generators ─────────────────────────────────────────────────────────

    fn arb_tool_id() -> impl Strategy<Value = String> {
        prop_oneof![
            Just("browser".to_string()),
            Just("filesystem".to_string()),
            Just("code_exec".to_string()),
            Just("web_search".to_string()),
            Just("database".to_string()),
            Just("gpu_compute".to_string()),
        ]
    }

    fn arb_trust_tier() -> impl Strategy<Value = TrustTier> {
        prop_oneof![
            Just(TrustTier::Public),
            Just(TrustTier::InvitedFriend),
            Just(TrustTier::LocalOwned),
        ]
    }

    fn make_tool(tool_id: &str, available: bool) -> ToolCapability {
        ToolCapability {
            tool_id: tool_id.to_string(),
            tool_name: tool_id.to_string(),
            category: ToolCategory::Custom(tool_id.to_string()),
            resource_requirements: ToolResources {
                cpu_cores: None,
                ram_mb: None,
                gpu_required: false,
                network_required: false,
            },
            is_available: available,
            version: "1.0".to_string(),
        }
    }

    fn make_node_state(node_id: NodeId, online: bool) -> NodeState {
        NodeState {
            capabilities: NodeCapabilities {
                node_id,
                hostname: format!("node-{}", &node_id.to_string()[..8]),
                device_type: DeviceType::Desktop,
                cpu: CpuProfile {
                    cores: 8,
                    architecture: "x86_64".to_string(),
                    clock_mhz: 4000,
                    isa_extensions: vec![],
                },
                ram: RamProfile {
                    total_mb: 32768,
                    available_mb: 24000,
                    ddr_generation: 4,
                },
                gpu: None,
                storage: StorageProfile {
                    storage_type: StorageType::Nvme,
                    available_mb: 500000,
                    read_speed_mbps: 7000,
                },
                network_interfaces: vec![],
                phone_info: None,
                available_tools: vec![],
            },
            utilization: NodeUtilization::default(),
            loaded_models: vec![],
            stability_score: 0.95,
            last_heartbeat_ms: 0,
            is_online: online,
            latency_to_peers: HashMap::new(),
            thermal_state: ThermalState::default(),
        }
    }

    /// Strategy to generate a network of 1-6 online nodes with random trust tiers
    /// and random tool sets. At least one node is always online.
    fn arb_network_with_trust(
        max_nodes: usize,
    ) -> impl Strategy<Value = (Vec<NodeState>, HashMap<NodeId, TrustTier>)> {
        (1..=max_nodes)
            .prop_flat_map(|num_nodes| {
                // For each node: generate a trust tier and a subset of tools
                proptest::collection::vec(
                    (
                        arb_trust_tier(),
                        proptest::collection::vec(arb_tool_id(), 0..=4),
                    ),
                    num_nodes,
                )
            })
            .prop_map(|node_configs| {
                let mut nodes = Vec::new();
                let mut trust_tiers = HashMap::new();

                for (tier, tools) in node_configs {
                    let node_id = uuid::Uuid::new_v4();
                    let mut node = make_node_state(node_id, true);

                    // Assign tools (deduplicated)
                    let unique_tools: Vec<String> = tools
                        .into_iter()
                        .collect::<std::collections::HashSet<_>>()
                        .into_iter()
                        .collect();
                    node.capabilities.available_tools = unique_tools
                        .iter()
                        .map(|t| make_tool(t, true))
                        .collect();

                    nodes.push(node);
                    trust_tiers.insert(node_id, tier);
                }

                (nodes, trust_tiers)
            })
    }

    /// Strategy to generate a sensitive step with random required tools.
    fn arb_sensitive_step() -> impl Strategy<Value = ExecutionStep> {
        proptest::collection::vec(arb_tool_id(), 0..=3).prop_map(|tools| {
            let unique_tools: Vec<String> = tools
                .into_iter()
                .collect::<std::collections::HashSet<_>>()
                .into_iter()
                .collect();

            ExecutionStep {
                step_id: uuid::Uuid::new_v4(),
                description: "sensitive step".to_string(),
                required_model: None,
                required_tools: unique_tools,
                sensitivity: PromptSensitivity::Sensitive,
                estimated_compute_ms: 1000,
                input_dependencies: vec![],
                status: StepStatus::Ready,
                assigned_node: None,
                result: None,
            }
        })
    }

    // ─── Property Tests ─────────────────────────────────────────────────────

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(200))]

        /// **Validates: Requirements FR-6.2, FR-6.4, Correctness Property 3**
        ///
        /// Property 3: Trust enforcement — for any sensitive step, the router
        /// never selects a node with trust tier < 3 (LocalOwned). If no tier-3
        /// node has the required tools, routing returns an error (never downgrades).
        #[test]
        fn prop_trust_enforcement_sensitive_steps_never_on_low_trust(
            (nodes, trust_tiers) in arb_network_with_trust(6),
            step in arb_sensitive_step(),
        ) {
            let requesting = uuid::Uuid::new_v4();
            let data_locations = HashMap::new();

            let result = route_step(&step, &nodes, &trust_tiers, requesting, &data_locations);

            match result {
                Ok(RoutingDecision::SingleNode(selected_node)) => {
                    // The selected node MUST have trust tier >= LocalOwned (tier 3)
                    let tier = trust_tiers.get(&selected_node);
                    prop_assert!(
                        tier.is_some(),
                        "Selected node {:?} has no trust tier entry", selected_node
                    );
                    prop_assert!(
                        *tier.unwrap() >= TrustTier::LocalOwned,
                        "Sensitive step routed to node {:?} with trust tier {:?} (< LocalOwned). \
                         Trust enforcement violated!",
                        selected_node,
                        tier.unwrap()
                    );
                }
                Ok(RoutingDecision::Decomposed(decomposed)) => {
                    // Both inference and tool nodes must be tier 3 for sensitive steps
                    let inf_tier = trust_tiers.get(&decomposed.inference_node);
                    let tool_tier = trust_tiers.get(&decomposed.tool_node);

                    prop_assert!(
                        inf_tier.map(|t| *t >= TrustTier::LocalOwned).unwrap_or(false),
                        "Sensitive step decomposed: inference node {:?} has trust {:?} (< LocalOwned)",
                        decomposed.inference_node,
                        inf_tier
                    );
                    prop_assert!(
                        tool_tier.map(|t| *t >= TrustTier::LocalOwned).unwrap_or(false),
                        "Sensitive step decomposed: tool node {:?} has trust {:?} (< LocalOwned)",
                        decomposed.tool_node,
                        tool_tier
                    );
                }
                Err(_) => {
                    // Routing failed — this is acceptable. Verify that no tier-3 node
                    // could have satisfied all requirements (the router correctly refused
                    // to downgrade rather than routing to a lower-trust node).
                    let tier3_nodes: Vec<&NodeState> = nodes
                        .iter()
                        .filter(|n| {
                            n.is_online
                                && trust_tiers
                                    .get(&n.capabilities.node_id)
                                    .map(|t| *t >= TrustTier::LocalOwned)
                                    .unwrap_or(false)
                        })
                        .collect();

                    // If there IS a tier-3 node with all required tools, routing should
                    // have succeeded. Verify none exists.
                    let tier3_with_all_tools = tier3_nodes.iter().any(|n| {
                        step.required_tools.iter().all(|tool_id| {
                            n.capabilities
                                .available_tools
                                .iter()
                                .any(|t| &t.tool_id == tool_id && t.is_available)
                        })
                    });

                    // If step has no model requirement and a tier-3 node has all tools,
                    // routing should have succeeded
                    if step.required_model.is_none() && tier3_with_all_tools {
                        prop_assert!(
                            false,
                            "Routing failed but a tier-3 node with all required tools exists. \
                             The router should have selected it."
                        );
                    }
                    // Otherwise, the error is correct: no suitable tier-3 node exists
                }
            }
        }
    }
}

#[cfg(test)]
mod proptest_tool_requirement_satisfaction {
    use super::*;
    use crate::agents::dag::{ExecutionStep, PromptSensitivity, StepStatus};
    use crate::agents::tools::{ToolCapability, ToolCategory, ToolResources};
    use crate::mesh::identity::TrustTier;
    use crate::network::registry::*;
    use proptest::prelude::*;
    use std::collections::HashMap;

    // ─── Generators ─────────────────────────────────────────────────────────

    /// Pool of tool IDs to draw from.
    fn arb_tool_id() -> impl Strategy<Value = String> {
        prop_oneof![
            Just("browser".to_string()),
            Just("filesystem".to_string()),
            Just("code_exec".to_string()),
            Just("web_search".to_string()),
            Just("database".to_string()),
            Just("gpu_compute".to_string()),
            Just("mic".to_string()),
            Just("camera".to_string()),
        ]
    }

    fn make_tool(tool_id: &str, available: bool) -> ToolCapability {
        ToolCapability {
            tool_id: tool_id.to_string(),
            tool_name: tool_id.to_string(),
            category: ToolCategory::Custom(tool_id.to_string()),
            resource_requirements: ToolResources {
                cpu_cores: None,
                ram_mb: None,
                gpu_required: false,
                network_required: false,
            },
            is_available: available,
            version: "1.0".to_string(),
        }
    }

    fn make_node_state(node_id: NodeId, online: bool) -> NodeState {
        NodeState {
            capabilities: NodeCapabilities {
                node_id,
                hostname: format!("node-{}", &node_id.to_string()[..8]),
                device_type: DeviceType::Desktop,
                cpu: CpuProfile {
                    cores: 8,
                    architecture: "x86_64".to_string(),
                    clock_mhz: 4000,
                    isa_extensions: vec![],
                },
                ram: RamProfile {
                    total_mb: 32768,
                    available_mb: 24000,
                    ddr_generation: 4,
                },
                gpu: None,
                storage: StorageProfile {
                    storage_type: StorageType::Nvme,
                    available_mb: 500000,
                    read_speed_mbps: 7000,
                },
                network_interfaces: vec![],
                phone_info: None,
                available_tools: vec![],
            },
            utilization: NodeUtilization::default(),
            loaded_models: vec![],
            stability_score: 0.95,
            last_heartbeat_ms: 0,
            is_online: online,
            latency_to_peers: HashMap::new(),
            thermal_state: ThermalState::default(),
        }
    }

    /// Strategy to generate a network of 1-6 online nodes where each node has
    /// a random subset of tools (some available, some not).
    fn arb_network_with_tools(
        max_nodes: usize,
    ) -> impl Strategy<Value = (Vec<NodeState>, HashMap<NodeId, TrustTier>)> {
        (1..=max_nodes)
            .prop_flat_map(|num_nodes| {
                proptest::collection::vec(
                    // Each node gets a random set of tools with random availability
                    proptest::collection::vec(
                        (arb_tool_id(), proptest::bool::ANY),
                        0..=5,
                    ),
                    num_nodes,
                )
            })
            .prop_map(|node_tool_configs| {
                let mut nodes = Vec::new();
                let mut trust_tiers = HashMap::new();

                for tools_with_availability in node_tool_configs {
                    let node_id = uuid::Uuid::new_v4();
                    let mut node = make_node_state(node_id, true);

                    // Deduplicate tools by tool_id, keeping last availability flag
                    let mut tool_map: HashMap<String, bool> = HashMap::new();
                    for (tool_id, available) in tools_with_availability {
                        tool_map.insert(tool_id, available);
                    }

                    node.capabilities.available_tools = tool_map
                        .iter()
                        .map(|(id, &avail)| make_tool(id, avail))
                        .collect();

                    nodes.push(node);
                    // All nodes get tier 3 so trust doesn't interfere with tool testing
                    trust_tiers.insert(node_id, TrustTier::LocalOwned);
                }

                (nodes, trust_tiers)
            })
    }

    /// Strategy to generate a step with 1-4 required tools (non-empty).
    fn arb_step_with_tools() -> impl Strategy<Value = ExecutionStep> {
        proptest::collection::vec(arb_tool_id(), 1..=4).prop_map(|tools| {
            let unique_tools: Vec<String> = tools
                .into_iter()
                .collect::<std::collections::HashSet<_>>()
                .into_iter()
                .collect();

            ExecutionStep {
                step_id: uuid::Uuid::new_v4(),
                description: "tool-requiring step".to_string(),
                required_model: None,
                required_tools: unique_tools,
                sensitivity: PromptSensitivity::NonSensitive,
                estimated_compute_ms: 1000,
                input_dependencies: vec![],
                status: StepStatus::Ready,
                assigned_node: None,
                result: None,
            }
        })
    }

    // ─── Property Tests ─────────────────────────────────────────────────────

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(200))]

        /// **Validates: Requirements FR-3.1, Correctness Property 4**
        ///
        /// Property 4: Tool requirement satisfaction — for any step with
        /// required_tools, the selected node always has ALL required tools
        /// available. The router never routes to a node missing a tool.
        #[test]
        fn prop_tool_requirement_satisfaction_selected_node_has_all_tools(
            (nodes, trust_tiers) in arb_network_with_tools(6),
            step in arb_step_with_tools(),
        ) {
            let requesting = uuid::Uuid::new_v4();
            let data_locations = HashMap::new();

            let result = route_step(&step, &nodes, &trust_tiers, requesting, &data_locations);

            match result {
                Ok(RoutingDecision::SingleNode(selected_node)) => {
                    // Find the selected node's state
                    let node_state = nodes
                        .iter()
                        .find(|n| n.capabilities.node_id == selected_node)
                        .expect("Selected node must exist in node list");

                    // Verify ALL required tools are available on the selected node
                    for required_tool in &step.required_tools {
                        let has_tool = node_state
                            .capabilities
                            .available_tools
                            .iter()
                            .any(|t| &t.tool_id == required_tool && t.is_available);

                        prop_assert!(
                            has_tool,
                            "Selected node {:?} is missing required tool '{}'. \
                             Node tools: {:?}. Step required: {:?}",
                            selected_node,
                            required_tool,
                            node_state.capabilities.available_tools
                                .iter()
                                .map(|t| (&t.tool_id, t.is_available))
                                .collect::<Vec<_>>(),
                            step.required_tools
                        );
                    }
                }
                Ok(RoutingDecision::Decomposed(decomposed)) => {
                    // For decomposed routes, the tool node must have ALL required tools
                    let tool_node_state = nodes
                        .iter()
                        .find(|n| n.capabilities.node_id == decomposed.tool_node)
                        .expect("Tool node must exist in node list");

                    for required_tool in &step.required_tools {
                        let has_tool = tool_node_state
                            .capabilities
                            .available_tools
                            .iter()
                            .any(|t| &t.tool_id == required_tool && t.is_available);

                        prop_assert!(
                            has_tool,
                            "Decomposed route: tool node {:?} is missing required tool '{}'. \
                             Node tools: {:?}. Step required: {:?}",
                            decomposed.tool_node,
                            required_tool,
                            tool_node_state.capabilities.available_tools
                                .iter()
                                .map(|t| (&t.tool_id, t.is_available))
                                .collect::<Vec<_>>(),
                            step.required_tools
                        );
                    }
                }
                Err(_) => {
                    // Routing failed — verify that indeed no node has all required tools
                    // available. The router correctly refused rather than routing to an
                    // incomplete node.
                    let any_node_has_all_tools = nodes.iter().any(|n| {
                        n.is_online
                            && step.required_tools.iter().all(|tool_id| {
                                n.capabilities
                                    .available_tools
                                    .iter()
                                    .any(|t| &t.tool_id == tool_id && t.is_available)
                            })
                    });

                    // If a node exists with all tools and proper trust, routing should
                    // have succeeded. (All nodes are tier-3 in this test, so trust is
                    // not the blocker for non-sensitive steps.)
                    if any_node_has_all_tools && step.sensitivity == PromptSensitivity::NonSensitive {
                        prop_assert!(
                            false,
                            "Routing failed but a node with all required tools {:?} exists. \
                             The router should have selected it.",
                            step.required_tools
                        );
                    }
                }
            }
        }
    }
}
