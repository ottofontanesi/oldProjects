// Intent citation: .kiro/specs/mesh-network-optimizer/design.md Section 1.3, 6.1
// Leader Election and Mesh Lifecycle — deterministic election, failover, optimizer loop

use crate::mesh::identity::MeshId;
use crate::mesh::solver::{CapacityOffer, ModelId};
use crate::transport::trait_def::NodeId;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ─── Leader Election Types ───────────────────────────────────────────────────

/// Information about a tier-3 node for leader election.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeaderCandidate {
    pub node_id: NodeId,
    pub reputation: f64,
    pub uptime_seconds: u64,
    pub is_online: bool,
    pub last_heartbeat: DateTime<Utc>,
}

/// The current leader state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeaderState {
    pub current_leader: Option<NodeId>,
    pub elected_at: Option<DateTime<Utc>>,
    pub missed_heartbeats: u32,
    pub last_plan_cycle: Option<DateTime<Utc>>,
}

/// Demand request from local optimizer to mesh optimizer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshDemandRequest {
    pub models_wanted: Vec<ModelId>,
    pub task_types_needed: Vec<String>,
    pub min_quality_threshold: f64,
}

/// Response from local optimizer to a mesh proposal.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ProposalResponse {
    Accepted,
    Rejected { reason: String },
}

// ─── Leader Election ─────────────────────────────────────────────────────────

/// Deterministic leader election: highest reputation + longest uptime among tier-3 nodes.
/// All nodes compute the same result — no consensus needed.
pub struct LeaderElection {
    /// Leader state per mesh.
    states: HashMap<MeshId, LeaderState>,
    /// Heartbeat interval for leader (default: 5 minutes = 300 seconds).
    pub leader_heartbeat_timeout_secs: u64,
    /// Optimization interval (default: 15 minutes = 900 seconds).
    pub optimization_interval_secs: u64,
}

impl LeaderElection {
    pub fn new() -> Self {
        Self {
            states: HashMap::new(),
            leader_heartbeat_timeout_secs: 300,
            optimization_interval_secs: 900,
        }
    }

    /// Compute the leader deterministically from the candidate list.
    /// Score = reputation * 0.7 + (uptime_days / 365) * 0.3
    /// All nodes running this function with the same inputs get the same result.
    pub fn elect_leader(&self, candidates: &[LeaderCandidate]) -> Option<NodeId> {
        candidates
            .iter()
            .filter(|c| c.is_online)
            .max_by(|a, b| {
                let score_a = self.leader_score(a);
                let score_b = self.leader_score(b);
                score_a
                    .partial_cmp(&score_b)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|c| c.node_id)
    }

    /// Compute the leader election score for a candidate.
    fn leader_score(&self, candidate: &LeaderCandidate) -> f64 {
        let uptime_days = candidate.uptime_seconds as f64 / 86400.0;
        candidate.reputation * 0.7 + (uptime_days / 365.0) * 0.3
    }

    /// Update leader state for a mesh. Returns true if leader changed.
    pub fn update_leader(
        &mut self,
        mesh_id: MeshId,
        candidates: &[LeaderCandidate],
    ) -> bool {
        let new_leader = self.elect_leader(candidates);
        let state = self.states.entry(mesh_id).or_insert(LeaderState {
            current_leader: None,
            elected_at: None,
            missed_heartbeats: 0,
            last_plan_cycle: None,
        });

        if state.current_leader != new_leader {
            state.current_leader = new_leader;
            state.elected_at = Some(Utc::now());
            state.missed_heartbeats = 0;
            true
        } else {
            false
        }
    }

    /// Record a leader heartbeat. Resets the missed counter.
    pub fn record_leader_heartbeat(&mut self, mesh_id: &MeshId) {
        if let Some(state) = self.states.get_mut(mesh_id) {
            state.missed_heartbeats = 0;
        }
    }

    /// Record a missed leader heartbeat. Returns true if failover should trigger.
    /// Failover triggers after 2 missed heartbeats (10 minutes with 5-min interval).
    pub fn record_missed_heartbeat(&mut self, mesh_id: &MeshId) -> bool {
        if let Some(state) = self.states.get_mut(mesh_id) {
            state.missed_heartbeats += 1;
            state.missed_heartbeats >= 2
        } else {
            false
        }
    }

    /// Get the current leader for a mesh.
    pub fn current_leader(&self, mesh_id: &MeshId) -> Option<NodeId> {
        self.states.get(mesh_id).and_then(|s| s.current_leader)
    }

    /// Check if it's time to run the optimizer loop (every 15 minutes).
    pub fn should_run_optimizer(&self, mesh_id: &MeshId) -> bool {
        if let Some(state) = self.states.get(mesh_id) {
            match state.last_plan_cycle {
                None => true, // Never run before
                Some(last) => {
                    let elapsed = (Utc::now() - last).num_seconds() as u64;
                    elapsed >= self.optimization_interval_secs
                }
            }
        } else {
            false
        }
    }

    /// Record that an optimization cycle was completed.
    pub fn record_optimization_cycle(&mut self, mesh_id: &MeshId) {
        if let Some(state) = self.states.get_mut(mesh_id) {
            state.last_plan_cycle = Some(Utc::now());
        }
    }

    /// Check if we are the leader for a mesh.
    pub fn am_i_leader(&self, mesh_id: &MeshId, my_node_id: &NodeId) -> bool {
        self.current_leader(mesh_id) == Some(*my_node_id)
    }
}

// ─── Local-Mesh Interface ────────────────────────────────────────────────────

/// Interface between local optimizer (9A) and mesh optimizer (9B).
/// Local optimizer exports CapacityOffer and DemandRequest.
/// Mesh optimizer consumes them and proposes placements.
pub struct LocalMeshInterface {
    /// Current capacity offers per mesh.
    capacity_offers: HashMap<MeshId, CapacityOffer>,
    /// Current demand requests per mesh.
    demand_requests: HashMap<MeshId, MeshDemandRequest>,
    /// Whether the local optimizer has rejected the last mesh proposal.
    last_rejection: HashMap<MeshId, Option<String>>,
}

impl LocalMeshInterface {
    pub fn new() -> Self {
        Self {
            capacity_offers: HashMap::new(),
            demand_requests: HashMap::new(),
            last_rejection: HashMap::new(),
        }
    }

    /// Local optimizer reports its capacity offer to the mesh.
    pub fn report_capacity_offer(&mut self, mesh_id: MeshId, offer: CapacityOffer) {
        self.capacity_offers.insert(mesh_id, offer);
    }

    /// Local optimizer reports its demand request.
    pub fn report_demand_request(&mut self, mesh_id: MeshId, demand: MeshDemandRequest) {
        self.demand_requests.insert(mesh_id, demand);
    }

    /// Get the current capacity offer for a mesh.
    pub fn get_capacity_offer(&self, mesh_id: &MeshId) -> Option<&CapacityOffer> {
        self.capacity_offers.get(mesh_id)
    }

    /// Get the current demand request for a mesh.
    pub fn get_demand_request(&self, mesh_id: &MeshId) -> Option<&MeshDemandRequest> {
        self.demand_requests.get(mesh_id)
    }

    /// Receive a mesh proposal and decide whether to accept or reject.
    /// Local optimizer rejection is final — mesh optimizer must respect it.
    pub fn receive_proposal(
        &mut self,
        mesh_id: &MeshId,
        proposed_ram_mb: u64,
        proposed_vram_mb: u64,
    ) -> ProposalResponse {
        if let Some(offer) = self.capacity_offers.get(mesh_id) {
            if proposed_ram_mb > offer.spare_ram_mb {
                let reason = format!(
                    "RAM exceeds offer: proposed {} MB > offered {} MB",
                    proposed_ram_mb, offer.spare_ram_mb
                );
                self.last_rejection.insert(*mesh_id, Some(reason.clone()));
                return ProposalResponse::Rejected { reason };
            }
            if proposed_vram_mb > offer.spare_vram_mb {
                let reason = format!(
                    "VRAM exceeds offer: proposed {} MB > offered {} MB",
                    proposed_vram_mb, offer.spare_vram_mb
                );
                self.last_rejection.insert(*mesh_id, Some(reason.clone()));
                return ProposalResponse::Rejected { reason };
            }
        }

        self.last_rejection.insert(*mesh_id, None);
        ProposalResponse::Accepted
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use uuid::Uuid;

    fn make_candidate(reputation: f64, uptime_secs: u64) -> LeaderCandidate {
        LeaderCandidate {
            node_id: Uuid::new_v4(),
            reputation,
            uptime_seconds: uptime_secs,
            is_online: true,
            last_heartbeat: Utc::now(),
        }
    }

    proptest! {
        /// Property: leader election is deterministic (same inputs = same leader).
        #[test]
        fn prop_leader_election_deterministic(
            rep_a in 0.0f64..1.0,
            rep_b in 0.0f64..1.0,
            uptime_a in 0u64..31_536_000,
            uptime_b in 0u64..31_536_000
        ) {
            let election = LeaderElection::new();

            let mut candidate_a = make_candidate(rep_a, uptime_a);
            let mut candidate_b = make_candidate(rep_b, uptime_b);

            // Fix node IDs so they're the same across calls
            let id_a = Uuid::new_v4();
            let id_b = Uuid::new_v4();
            candidate_a.node_id = id_a;
            candidate_b.node_id = id_b;

            let candidates = vec![candidate_a.clone(), candidate_b.clone()];

            let leader1 = election.elect_leader(&candidates);
            let leader2 = election.elect_leader(&candidates);

            prop_assert_eq!(leader1, leader2, "Same inputs must produce same leader");
        }

        /// Property: leader failover completes within 10 minutes (2 missed heartbeats).
        #[test]
        fn prop_failover_after_two_missed(
            _dummy in 0u8..10
        ) {
            let mut election = LeaderElection::new();
            let mesh_id = Uuid::new_v4();

            let candidates = vec![
                make_candidate(0.9, 86400),
                make_candidate(0.8, 172800),
            ];
            election.update_leader(mesh_id, &candidates);

            // First missed heartbeat — no failover
            let should_failover = election.record_missed_heartbeat(&mesh_id);
            prop_assert!(!should_failover);

            // Second missed heartbeat — failover triggered
            let should_failover = election.record_missed_heartbeat(&mesh_id);
            prop_assert!(should_failover);
        }

        /// Property: local optimizer rejection is final.
        #[test]
        fn prop_local_rejection_is_final(
            offered_ram in 1000u64..16000,
            proposed_ram in 1000u64..32000
        ) {
            let mut interface = LocalMeshInterface::new();
            let mesh_id = Uuid::new_v4();
            let node_id = Uuid::new_v4();

            interface.report_capacity_offer(mesh_id, CapacityOffer {
                node_id,
                spare_ram_mb: offered_ram,
                spare_vram_mb: 8000,
                spare_gpu_percent: 50.0,
                max_models_willing_to_host: 5,
                available_hours_per_day: 24.0,
                available_tools: vec![],
            });

            let response = interface.receive_proposal(&mesh_id, proposed_ram, 0);

            if proposed_ram > offered_ram {
                let is_rejected = matches!(response, ProposalResponse::Rejected { .. });
                prop_assert!(is_rejected);
            } else {
                prop_assert_eq!(response, ProposalResponse::Accepted);
            }
        }
    }

    // ─── Unit Tests ──────────────────────────────────────────────────────────

    #[test]
    fn test_highest_reputation_wins() {
        let election = LeaderElection::new();
        let mut high_rep = make_candidate(0.95, 86400);
        let mut low_rep = make_candidate(0.3, 86400);
        let id_high = Uuid::new_v4();
        let id_low = Uuid::new_v4();
        high_rep.node_id = id_high;
        low_rep.node_id = id_low;

        let leader = election.elect_leader(&[low_rep, high_rep]);
        assert_eq!(leader, Some(id_high));
    }

    #[test]
    fn test_offline_nodes_excluded() {
        let election = LeaderElection::new();
        let mut offline = make_candidate(1.0, 999999);
        offline.is_online = false;
        let online = make_candidate(0.5, 86400);

        let leader = election.elect_leader(&[offline, online.clone()]);
        assert_eq!(leader, Some(online.node_id));
    }

    #[test]
    fn test_no_candidates_returns_none() {
        let election = LeaderElection::new();
        let leader = election.elect_leader(&[]);
        assert_eq!(leader, None);
    }

    #[test]
    fn test_optimization_interval() {
        let mut election = LeaderElection::new();
        election.optimization_interval_secs = 900;
        let mesh_id = Uuid::new_v4();

        let candidates = vec![make_candidate(0.9, 86400)];
        election.update_leader(mesh_id, &candidates);

        // Should run (never run before)
        assert!(election.should_run_optimizer(&mesh_id));

        // Record cycle
        election.record_optimization_cycle(&mesh_id);

        // Should not run (just ran)
        assert!(!election.should_run_optimizer(&mesh_id));
    }
}
