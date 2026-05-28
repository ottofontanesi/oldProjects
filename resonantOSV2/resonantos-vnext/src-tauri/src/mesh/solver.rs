// Intent citation: .kiro/specs/mesh-network-optimizer/design.md Section 3.1-3.4
// Mesh Solver — extends Phase 9A GreedySolver with trust, reputation, fairness constraints

use crate::mesh::identity::{MeshId, TrustTier};
use crate::mesh::incentive::FreeRiderStatus;
use crate::transport::trait_def::NodeId;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use uuid::Uuid;

// ─── Mesh Solver Types ───────────────────────────────────────────────────────

/// A model identifier (same as Phase 9A).
pub type ModelId = String;

/// Capacity a node offers to the mesh.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapacityOffer {
    pub node_id: NodeId,
    pub spare_ram_mb: u64,
    pub spare_vram_mb: u64,
    pub spare_gpu_percent: f64,
    pub max_models_willing_to_host: u32,
    pub available_hours_per_day: f64,
    /// Phase 15 extension point: tools this node offers to the mesh.
    pub available_tools: Vec<String>,
}

/// A node's state as seen by the mesh solver.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshNodeState {
    pub node_id: NodeId,
    pub owner_id: NodeId,
    pub trust_tier: TrustTier,
    pub reputation: f64,
    pub capacity_offer: CapacityOffer,
    pub free_rider_status: FreeRiderStatus,
    pub is_online: bool,
    pub uptime_seconds: u64,
}

/// A model candidate for mesh placement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshModelCandidate {
    pub model_id: ModelId,
    pub ram_required_mb: u64,
    pub vram_required_mb: u64,
    pub parameter_count_b: f64,
    pub quality_score: f64,
    pub serves_sensitive_workload: bool,
    pub min_trust_tier: TrustTier,
}

/// Inputs to the mesh solver.
pub struct MeshSolverInputs {
    pub mesh_id: MeshId,
    pub nodes: Vec<MeshNodeState>,
    pub candidates: Vec<MeshModelCandidate>,
    pub current_placements: Vec<MeshModelPlacement>,
    pub timeout: Duration,
}

impl MeshSolverInputs {
    pub fn new(mesh_id: MeshId) -> Self {
        Self {
            mesh_id,
            nodes: Vec::new(),
            candidates: Vec::new(),
            current_placements: Vec::new(),
            timeout: Duration::from_secs(5),
        }
    }
}

/// A single model placement in the mesh plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshModelPlacement {
    pub model_id: ModelId,
    pub instance_id: Uuid,
    pub assigned_node: NodeId,
    pub owner_node: NodeId,
    pub trust_requirement: TrustTier,
    pub estimated_tok_s: f32,
    pub ram_allocated_mb: u64,
    pub vram_allocated_mb: u64,
}

/// The complete mesh placement plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshPlacementPlan {
    pub plan_id: Uuid,
    pub mesh_id: MeshId,
    pub created_at: DateTime<Utc>,
    pub leader_node: NodeId,
    pub solver_duration_ms: u64,
    pub cycle_number: u32,
    pub placements: Vec<MeshModelPlacement>,
    pub acknowledgments: HashMap<NodeId, PlanAcknowledgment>,
}

/// A node's response to a proposed plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanAcknowledgment {
    pub node_id: NodeId,
    pub accepted: bool,
    pub rejection_reason: Option<String>,
    pub timestamp: DateTime<Utc>,
}

// ─── Mesh Solver ─────────────────────────────────────────────────────────────

/// Mesh solver that wraps Phase 9A's greedy approach with additional constraints.
pub struct MeshSolver {
    /// Maximum share of models any single owner can host (default: 0.60).
    pub max_owner_hosting_share: f64,
    /// Solver timeout (default: 5 seconds).
    pub timeout: Duration,
    /// Current cycle number.
    cycle_number: u32,
}

impl MeshSolver {
    pub fn new() -> Self {
        Self {
            max_owner_hosting_share: 0.60,
            timeout: Duration::from_secs(5),
            cycle_number: 0,
        }
    }

    /// Run the mesh solver to produce a placement plan.
    pub fn solve(
        &mut self,
        inputs: &MeshSolverInputs,
        leader_node: NodeId,
    ) -> MeshPlacementPlan {
        let start = Instant::now();
        self.cycle_number += 1;

        // Phase A: Filter candidates by trust tier availability
        let feasible = self.filter_by_trust(&inputs.candidates, &inputs.nodes);

        // Phase B: Assign models to nodes (greedy, reputation-weighted)
        let mut placements = Vec::new();
        let mut node_allocations: HashMap<NodeId, (u64, u64)> = HashMap::new(); // (ram, vram)
        let mut owner_model_count: HashMap<NodeId, u32> = HashMap::new();

        // Sort candidates by quality descending (best models first)
        let mut sorted_candidates = feasible;
        sorted_candidates.sort_by(|a, b| {
            b.quality_score
                .partial_cmp(&a.quality_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        for candidate in &sorted_candidates {
            // Check timeout
            if start.elapsed() >= inputs.timeout {
                break;
            }

            // Find best node for this model
            if let Some(placement) =
                self.find_best_placement(candidate, &inputs.nodes, &node_allocations, &owner_model_count, placements.len())
            {
                // Update allocations
                let alloc = node_allocations
                    .entry(placement.assigned_node)
                    .or_insert((0, 0));
                alloc.0 += placement.ram_allocated_mb;
                alloc.1 += placement.vram_allocated_mb;

                *owner_model_count.entry(placement.owner_node).or_insert(0) += 1;
                placements.push(placement);
            }
        }

        // Phase C: Validate mesh constraints
        self.validate_constraints(&mut placements, &inputs.nodes, &owner_model_count);

        let duration = start.elapsed();

        MeshPlacementPlan {
            plan_id: Uuid::new_v4(),
            mesh_id: inputs.mesh_id,
            created_at: Utc::now(),
            leader_node,
            solver_duration_ms: duration.as_millis() as u64,
            cycle_number: self.cycle_number,
            placements,
            acknowledgments: HashMap::new(),
        }
    }

    /// Filter candidates by trust tier: only keep models that have nodes at the required tier.
    fn filter_by_trust(
        &self,
        candidates: &[MeshModelCandidate],
        nodes: &[MeshNodeState],
    ) -> Vec<MeshModelCandidate> {
        candidates
            .iter()
            .filter(|c| {
                nodes.iter().any(|n| {
                    n.trust_tier >= c.min_trust_tier
                        && n.is_online
                        && !matches!(n.free_rider_status, FreeRiderStatus::Excluded { .. })
                })
            })
            .cloned()
            .collect()
    }

    /// Find the best node to place a model on, considering all constraints.
    fn find_best_placement(
        &self,
        candidate: &MeshModelCandidate,
        nodes: &[MeshNodeState],
        allocations: &HashMap<NodeId, (u64, u64)>,
        owner_counts: &HashMap<NodeId, u32>,
        total_placements: usize,
    ) -> Option<MeshModelPlacement> {
        let mut best_score = f64::NEG_INFINITY;
        let mut best_node: Option<&MeshNodeState> = None;

        for node in nodes {
            // Skip offline nodes
            if !node.is_online {
                continue;
            }

            // Skip excluded nodes
            if matches!(node.free_rider_status, FreeRiderStatus::Excluded { .. }) {
                continue;
            }

            // Trust tier check
            if node.trust_tier < candidate.min_trust_tier {
                continue;
            }

            // Capacity offer check
            let (used_ram, used_vram) = allocations.get(&node.node_id).copied().unwrap_or((0, 0));
            let available_ram = node.capacity_offer.spare_ram_mb.saturating_sub(used_ram);
            let available_vram = node.capacity_offer.spare_vram_mb.saturating_sub(used_vram);

            if candidate.ram_required_mb > available_ram {
                continue;
            }
            if candidate.vram_required_mb > available_vram {
                continue;
            }

            // Max models per node check
            let models_on_node = allocations.get(&node.node_id).map(|_| 1u32).unwrap_or(0);
            if models_on_node >= node.capacity_offer.max_models_willing_to_host {
                continue;
            }

            // Cross-owner fairness check
            let owner_count = owner_counts.get(&node.owner_id).copied().unwrap_or(0);
            if total_placements > 0 {
                let owner_share = (owner_count + 1) as f64 / (total_placements + 1) as f64;
                if owner_share > self.max_owner_hosting_share {
                    continue;
                }
            }

            // Score this placement
            let score = self.score_placement(node, candidate);
            if score > best_score {
                best_score = score;
                best_node = Some(node);
            }
        }

        best_node.map(|node| MeshModelPlacement {
            model_id: candidate.model_id.clone(),
            instance_id: Uuid::new_v4(),
            assigned_node: node.node_id,
            owner_node: node.owner_id,
            trust_requirement: candidate.min_trust_tier,
            estimated_tok_s: 0.0, // Would be computed from hardware profile
            ram_allocated_mb: candidate.ram_required_mb,
            vram_allocated_mb: candidate.vram_required_mb,
        })
    }

    /// Score a placement: base quality + reputation bonus.
    fn score_placement(&self, node: &MeshNodeState, candidate: &MeshModelCandidate) -> f64 {
        let base_score = candidate.quality_score;

        // Reputation bonus: prefer high-reputation nodes
        let reputation_bonus = node.reputation * 0.3;

        // Capacity fit bonus: prefer nodes with more spare capacity
        let ram_fit = 1.0
            - (candidate.ram_required_mb as f64 / node.capacity_offer.spare_ram_mb.max(1) as f64);
        let capacity_bonus = ram_fit.clamp(0.0, 1.0) * 0.1;

        base_score + reputation_bonus + capacity_bonus
    }

    /// Validate mesh-specific constraints and remove violating placements.
    fn validate_constraints(
        &self,
        placements: &mut Vec<MeshModelPlacement>,
        nodes: &[MeshNodeState],
        _owner_counts: &HashMap<NodeId, u32>,
    ) {
        // Constraint 1: Trust routing invariant
        placements.retain(|p| {
            if let Some(node) = nodes.iter().find(|n| n.node_id == p.assigned_node) {
                node.trust_tier >= p.trust_requirement
            } else {
                false
            }
        });

        // Constraint 2: Cross-owner fairness (already enforced during placement)
        // Double-check: no owner > 60%
        if !placements.is_empty() {
            let mut owner_counts: HashMap<NodeId, usize> = HashMap::new();
            for p in placements.iter() {
                *owner_counts.entry(p.owner_node).or_insert(0) += 1;
            }
            let total = placements.len();
            let max_allowed = (total as f64 * self.max_owner_hosting_share).ceil() as usize;

            // Remove excess from over-represented owners
            for (owner, count) in &owner_counts {
                if *count > max_allowed {
                    let excess = count - max_allowed;
                    let mut removed = 0;
                    placements.retain(|p| {
                        if p.owner_node == *owner && removed < excess {
                            removed += 1;
                            false
                        } else {
                            true
                        }
                    });
                }
            }
        }
    }

    /// Process plan acknowledgments. Returns nodes that rejected.
    pub fn process_acknowledgments(
        plan: &mut MeshPlacementPlan,
        acks: Vec<PlanAcknowledgment>,
    ) -> Vec<NodeId> {
        let mut rejectors = Vec::new();

        for ack in acks {
            if !ack.accepted {
                rejectors.push(ack.node_id);
            }
            plan.acknowledgments.insert(ack.node_id, ack);
        }

        // Remove placements on rejecting nodes
        if !rejectors.is_empty() {
            plan.placements
                .retain(|p| !rejectors.contains(&p.assigned_node));
        }

        rejectors
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn make_node(
        trust_tier: TrustTier,
        reputation: f64,
        spare_ram_mb: u64,
        owner_id: NodeId,
    ) -> MeshNodeState {
        let node_id = Uuid::new_v4();
        MeshNodeState {
            node_id,
            owner_id,
            trust_tier,
            reputation,
            capacity_offer: CapacityOffer {
                node_id,
                spare_ram_mb,
                spare_vram_mb: 8000,
                spare_gpu_percent: 50.0,
                max_models_willing_to_host: 5,
                available_hours_per_day: 24.0,
                available_tools: vec![],
            },
            free_rider_status: FreeRiderStatus::Good,
            is_online: true,
            uptime_seconds: 86400,
        }
    }

    fn make_candidate(model_id: &str, ram_mb: u64, sensitive: bool) -> MeshModelCandidate {
        MeshModelCandidate {
            model_id: model_id.to_string(),
            ram_required_mb: ram_mb,
            vram_required_mb: 0,
            parameter_count_b: 7.0,
            quality_score: 0.8,
            serves_sensitive_workload: sensitive,
            min_trust_tier: if sensitive {
                TrustTier::LocalOwned
            } else {
                TrustTier::InvitedFriend
            },
        }
    }

    proptest! {
        /// Property: trust routing invariant — sensitive never placed on tier < 3.
        #[test]
        fn prop_trust_routing_invariant(
            num_tier2 in 1usize..5,
            num_tier3 in 1usize..5
        ) {
            let mut solver = MeshSolver::new();
            let mesh_id = Uuid::new_v4();
            let owner = Uuid::new_v4();

            let mut nodes = Vec::new();
            for _ in 0..num_tier2 {
                nodes.push(make_node(TrustTier::InvitedFriend, 0.8, 16000, owner));
            }
            for _ in 0..num_tier3 {
                nodes.push(make_node(TrustTier::LocalOwned, 0.8, 16000, owner));
            }

            let candidates = vec![
                make_candidate("sensitive-model", 4000, true),
            ];

            let inputs = MeshSolverInputs {
                mesh_id,
                nodes,
                candidates,
                current_placements: vec![],
                timeout: Duration::from_secs(5),
            };

            let plan = solver.solve(&inputs, Uuid::new_v4());

            for placement in &plan.placements {
                if placement.trust_requirement == TrustTier::LocalOwned {
                    let node = inputs.nodes.iter().find(|n| n.node_id == placement.assigned_node).unwrap();
                    prop_assert!(
                        node.trust_tier >= TrustTier::LocalOwned,
                        "Sensitive model placed on tier {:?} node", node.trust_tier
                    );
                }
            }
        }

        /// Property: fairness constraint enforced — no owner > 60%.
        #[test]
        fn prop_fairness_constraint(
            num_models in 3usize..10
        ) {
            let mut solver = MeshSolver::new();
            let mesh_id = Uuid::new_v4();
            let owner_a = Uuid::new_v4();
            let owner_b = Uuid::new_v4();

            // Owner A has many nodes, Owner B has few
            let mut nodes = Vec::new();
            for _ in 0..5 {
                nodes.push(make_node(TrustTier::LocalOwned, 0.9, 32000, owner_a));
            }
            nodes.push(make_node(TrustTier::LocalOwned, 0.9, 32000, owner_b));

            let candidates: Vec<MeshModelCandidate> = (0..num_models)
                .map(|i| make_candidate(&format!("model-{}", i), 2000, false))
                .collect();

            let inputs = MeshSolverInputs {
                mesh_id,
                nodes,
                candidates,
                current_placements: vec![],
                timeout: Duration::from_secs(5),
            };

            let plan = solver.solve(&inputs, Uuid::new_v4());

            if plan.placements.len() > 1 {
                let mut owner_counts: HashMap<NodeId, usize> = HashMap::new();
                for p in &plan.placements {
                    *owner_counts.entry(p.owner_node).or_insert(0) += 1;
                }
                let total = plan.placements.len();
                for (_owner, count) in &owner_counts {
                    let share = *count as f64 / total as f64;
                    prop_assert!(
                        share <= 0.61, // Small tolerance for rounding
                        "Owner has {:.2}% share ({}/{})", share * 100.0, count, total
                    );
                }
            }
        }

        /// Property: capacity offers never exceeded.
        #[test]
        fn prop_capacity_never_exceeded(
            spare_ram in 4000u64..32000,
            num_models in 1usize..8
        ) {
            let mut solver = MeshSolver::new();
            let mesh_id = Uuid::new_v4();
            let owner = Uuid::new_v4();

            let nodes = vec![make_node(TrustTier::LocalOwned, 0.8, spare_ram, owner)];

            let candidates: Vec<MeshModelCandidate> = (0..num_models)
                .map(|i| make_candidate(&format!("model-{}", i), 2000, false))
                .collect();

            let inputs = MeshSolverInputs {
                mesh_id,
                nodes: nodes.clone(),
                candidates,
                current_placements: vec![],
                timeout: Duration::from_secs(5),
            };

            let plan = solver.solve(&inputs, Uuid::new_v4());

            // Check total RAM allocated to each node doesn't exceed offer
            let mut node_ram: HashMap<NodeId, u64> = HashMap::new();
            for p in &plan.placements {
                *node_ram.entry(p.assigned_node).or_insert(0) += p.ram_allocated_mb;
            }

            for (node_id, allocated) in &node_ram {
                let node = nodes.iter().find(|n| n.node_id == *node_id).unwrap();
                prop_assert!(
                    *allocated <= node.capacity_offer.spare_ram_mb,
                    "Allocated {} MB > offered {} MB",
                    allocated, node.capacity_offer.spare_ram_mb
                );
            }
        }

        /// Property: solver completes within timeout.
        #[test]
        fn prop_solver_within_timeout(
            num_nodes in 1usize..100,
            num_models in 1usize..50
        ) {
            let mut solver = MeshSolver::new();
            let mesh_id = Uuid::new_v4();
            let owner = Uuid::new_v4();

            let nodes: Vec<MeshNodeState> = (0..num_nodes)
                .map(|_| make_node(TrustTier::LocalOwned, 0.7, 16000, owner))
                .collect();

            let candidates: Vec<MeshModelCandidate> = (0..num_models)
                .map(|i| make_candidate(&format!("m-{}", i), 1000, false))
                .collect();

            let inputs = MeshSolverInputs {
                mesh_id,
                nodes,
                candidates,
                current_placements: vec![],
                timeout: Duration::from_secs(5),
            };

            let start = Instant::now();
            let _plan = solver.solve(&inputs, Uuid::new_v4());
            let elapsed = start.elapsed();

            prop_assert!(elapsed < Duration::from_secs(5), "Solver took {:?}", elapsed);
        }
    }

    // ─── Unit Tests ──────────────────────────────────────────────────────────

    #[test]
    fn test_excluded_nodes_not_used() {
        let mut solver = MeshSolver::new();
        let mesh_id = Uuid::new_v4();
        let owner = Uuid::new_v4();

        let mut node = make_node(TrustTier::LocalOwned, 0.5, 16000, owner);
        node.free_rider_status = FreeRiderStatus::Excluded { since_cycle: 1 };

        let inputs = MeshSolverInputs {
            mesh_id,
            nodes: vec![node],
            candidates: vec![make_candidate("test-model", 4000, false)],
            current_placements: vec![],
            timeout: Duration::from_secs(5),
        };

        let plan = solver.solve(&inputs, Uuid::new_v4());
        assert!(plan.placements.is_empty());
    }

    #[test]
    fn test_plan_acknowledgment_rejection() {
        let mut solver = MeshSolver::new();
        let mesh_id = Uuid::new_v4();
        let owner = Uuid::new_v4();

        let nodes = vec![
            make_node(TrustTier::LocalOwned, 0.9, 16000, owner),
            make_node(TrustTier::LocalOwned, 0.8, 16000, owner),
        ];

        let inputs = MeshSolverInputs {
            mesh_id,
            nodes: nodes.clone(),
            candidates: vec![
                make_candidate("model-a", 4000, false),
                make_candidate("model-b", 4000, false),
            ],
            current_placements: vec![],
            timeout: Duration::from_secs(5),
        };

        let mut plan = solver.solve(&inputs, Uuid::new_v4());
        let initial_count = plan.placements.len();

        // First node rejects
        let rejector = plan.placements[0].assigned_node;
        let acks = vec![PlanAcknowledgment {
            node_id: rejector,
            accepted: false,
            rejection_reason: Some("Capacity changed".to_string()),
            timestamp: Utc::now(),
        }];

        let rejectors = MeshSolver::process_acknowledgments(&mut plan, acks);
        assert_eq!(rejectors.len(), 1);
        assert!(plan.placements.len() < initial_count);
    }
}
