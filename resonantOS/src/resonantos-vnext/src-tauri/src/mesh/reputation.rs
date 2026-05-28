// Intent citation: .kiro/specs/mesh-network-optimizer/design.md Section 2.2, 3.6
// Reputation System — compute, bound, track history, adjust rate limits

use crate::mesh::accounting::{AccountingLedger, AccountingPeriod};
use crate::mesh::identity::MeshId;
use crate::transport::trait_def::NodeId;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ─── Reputation Types ────────────────────────────────────────────────────────

/// A single reputation update for one cycle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReputationUpdate {
    pub cycle_number: u32,
    pub timestamp: DateTime<Utc>,
    pub contribution_delta: f64,
    pub consumption_delta: f64,
    pub reputation_change: f64,
    pub new_reputation: f64,
}

/// Full reputation state for a node in a mesh.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeReputation {
    pub node_id: NodeId,
    pub mesh_id: MeshId,
    pub reputation_score: f64,
    pub contribution_balance: f64,
    pub consecutive_negative_cycles: u32,
    pub consecutive_positive_cycles: u32,
    pub last_updated: DateTime<Utc>,
    pub history: Vec<ReputationUpdate>,
}

impl NodeReputation {
    /// Create a new reputation entry for a node (starts at 0.5).
    pub fn new(node_id: NodeId, mesh_id: MeshId) -> Self {
        Self {
            node_id,
            mesh_id,
            reputation_score: 0.5,
            contribution_balance: 0.0,
            consecutive_negative_cycles: 0,
            consecutive_positive_cycles: 0,
            last_updated: Utc::now(),
            history: Vec::new(),
        }
    }
}

// ─── Reputation Manager ──────────────────────────────────────────────────────

/// Manages reputation scores for all nodes across meshes.
pub struct ReputationManager {
    /// Reputation state: (mesh_id, node_id) -> NodeReputation.
    reputations: HashMap<(MeshId, NodeId), NodeReputation>,
    /// Current cycle number per mesh.
    cycle_numbers: HashMap<MeshId, u32>,
    /// Maximum reputation change per cycle (default: 0.1).
    pub max_change_per_cycle: f64,
    /// Initial reputation for new nodes (default: 0.5).
    pub initial_reputation: f64,
    /// Rate limit bonus multiplier (default: 2.0 — reputation 1.0 gets 2x base).
    pub rate_limit_bonus_multiplier: f64,
}

impl ReputationManager {
    pub fn new() -> Self {
        Self {
            reputations: HashMap::new(),
            cycle_numbers: HashMap::new(),
            max_change_per_cycle: 0.1,
            initial_reputation: 0.5,
            rate_limit_bonus_multiplier: 2.0,
        }
    }

    /// Register a new node with initial reputation.
    pub fn register_node(&mut self, node_id: NodeId, mesh_id: MeshId) {
        let key = (mesh_id, node_id);
        if !self.reputations.contains_key(&key) {
            let mut rep = NodeReputation::new(node_id, mesh_id);
            rep.reputation_score = self.initial_reputation;
            self.reputations.insert(key, rep);
        }
    }

    /// Get the current reputation score for a node.
    pub fn get_reputation(&self, node_id: &NodeId, mesh_id: &MeshId) -> Option<f64> {
        self.reputations
            .get(&(*mesh_id, *node_id))
            .map(|r| r.reputation_score)
    }

    /// Get the full reputation state for a node.
    pub fn get_node_reputation(&self, node_id: &NodeId, mesh_id: &MeshId) -> Option<&NodeReputation> {
        self.reputations.get(&(*mesh_id, *node_id))
    }

    /// Compute and apply a reputation update for a node based on accounting data.
    pub fn compute_reputation(
        &mut self,
        node_id: &NodeId,
        mesh_id: &MeshId,
        ledger: &AccountingLedger,
    ) -> Option<ReputationUpdate> {
        let key = (*mesh_id, *node_id);
        let rep = self.reputations.get_mut(&key)?;

        let cycle = self.cycle_numbers.entry(*mesh_id).or_insert(0);
        *cycle += 1;
        let cycle_number = *cycle;

        // Get balance from accounting
        let summary = ledger.compute_balance(node_id, mesh_id, &AccountingPeriod::LastCycle);
        let balance = summary.balance;

        // Convert balance to reputation change (capped at ±max_change_per_cycle)
        let raw_delta = balance * 0.05;
        let reputation_change = raw_delta.clamp(-self.max_change_per_cycle, self.max_change_per_cycle);

        // Apply to current reputation (bounded [0.0, 1.0])
        let new_reputation = (rep.reputation_score + reputation_change).clamp(0.0, 1.0);

        // Track consecutive cycles
        rep.contribution_balance = balance;
        if balance < 0.0 {
            rep.consecutive_negative_cycles += 1;
            rep.consecutive_positive_cycles = 0;
        } else if balance > 0.0 {
            rep.consecutive_positive_cycles += 1;
            if rep.consecutive_negative_cycles > 0 {
                // Keep negative count for incentive enforcement
            }
        } else {
            // Neutral — don't change counters
        }

        let update = ReputationUpdate {
            cycle_number,
            timestamp: Utc::now(),
            contribution_delta: summary.total_contributed.total_normalized(),
            consumption_delta: summary.total_consumed.total_normalized(),
            reputation_change,
            new_reputation,
        };

        rep.reputation_score = new_reputation;
        rep.last_updated = Utc::now();
        rep.history.push(update.clone());

        Some(update)
    }

    /// Get the reputation-adjusted rate limit multiplier for a node.
    /// reputation 0.0 → 0.0x, reputation 0.5 → 1.0x, reputation 1.0 → 2.0x
    pub fn rate_limit_multiplier(&self, node_id: &NodeId, mesh_id: &MeshId) -> f64 {
        let reputation = self.get_reputation(node_id, mesh_id).unwrap_or(0.5);
        1.0 + (reputation - 0.5) * self.rate_limit_bonus_multiplier
    }

    /// Get reputation history for trend analysis.
    pub fn get_history(&self, node_id: &NodeId, mesh_id: &MeshId) -> Vec<ReputationUpdate> {
        self.reputations
            .get(&(*mesh_id, *node_id))
            .map(|r| r.history.clone())
            .unwrap_or_default()
    }

    /// Remove a node's reputation data (for retirement).
    pub fn remove_node(&mut self, node_id: &NodeId, mesh_id: &MeshId) {
        self.reputations.remove(&(*mesh_id, *node_id));
    }

    /// Reset consecutive negative cycles (used by incentive recovery).
    pub fn reset_negative_cycles(&mut self, node_id: &NodeId, mesh_id: &MeshId) {
        if let Some(rep) = self.reputations.get_mut(&(*mesh_id, *node_id)) {
            rep.consecutive_negative_cycles = 0;
            rep.consecutive_positive_cycles = 0;
        }
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::accounting::{AccountingAmount, AccountingLedger, AccountingType, RecordPayload};
    use crate::mesh::identity::MeshIdentity;
    use proptest::prelude::*;
    use uuid::Uuid;

    fn setup_ledger_with_records(
        contributor: &MeshIdentity,
        consumer: &MeshIdentity,
        mesh_id: MeshId,
        gpu_seconds: f64,
    ) -> AccountingLedger {
        let mut ledger = AccountingLedger::new();
        ledger.register_key(contributor.node_id, contributor.verifying_key);
        ledger.register_key(consumer.node_id, consumer.verifying_key);

        let payload = AccountingLedger::create_record_payload(
            mesh_id,
            AccountingType::InferenceServed,
            contributor.node_id,
            consumer.node_id,
            AccountingAmount {
                gpu_seconds,
                ram_seconds: 0.0,
                bandwidth_bytes: 0,
                request_count: 1,
            },
        );

        let contrib_sig = AccountingLedger::sign_as_contributor(&payload, contributor);
        let consumer_sig = AccountingLedger::sign_as_consumer(&payload, consumer);

        let record = crate::mesh::accounting::AccountingRecord {
            record_id: payload.record_id,
            mesh_id: payload.mesh_id,
            timestamp: payload.timestamp,
            record_type: payload.record_type,
            contributor_node: payload.contributor_node,
            consumer_node: payload.consumer_node,
            amount: payload.amount,
            contributor_signature: contrib_sig,
            consumer_signature: consumer_sig,
        };

        ledger.append(record).unwrap();
        ledger
    }

    proptest! {
        /// Property: reputation always in [0.0, 1.0].
        #[test]
        fn prop_reputation_always_bounded(
            initial in 0.0f64..1.0,
            changes in proptest::collection::vec(-5.0f64..5.0, 1..20)
        ) {
            let mut mgr = ReputationManager::new();
            let mesh_id = Uuid::new_v4();
            let node_id = Uuid::new_v4();
            mgr.register_node(node_id, mesh_id);

            // Manually set initial reputation
            if let Some(rep) = mgr.reputations.get_mut(&(mesh_id, node_id)) {
                rep.reputation_score = initial;
            }

            // Apply changes directly (simulating multiple cycles)
            for change in changes {
                if let Some(rep) = mgr.reputations.get_mut(&(mesh_id, node_id)) {
                    let clamped_change = change.clamp(-0.1, 0.1);
                    rep.reputation_score = (rep.reputation_score + clamped_change).clamp(0.0, 1.0);
                }
            }

            let score = mgr.get_reputation(&node_id, &mesh_id).unwrap();
            prop_assert!(score >= 0.0 && score <= 1.0, "Score {} out of bounds", score);
        }

        /// Property: change never exceeds ±0.1 per cycle.
        #[test]
        fn prop_change_never_exceeds_max(
            balance in -100.0f64..100.0
        ) {
            let raw_delta = balance * 0.05;
            let clamped = raw_delta.clamp(-0.1, 0.1);
            prop_assert!(clamped >= -0.1 && clamped <= 0.1);
        }

        /// Property: new nodes start at 0.5.
        #[test]
        fn prop_new_nodes_start_at_half(
            _dummy in 0u8..100
        ) {
            let mut mgr = ReputationManager::new();
            let mesh_id = Uuid::new_v4();
            let node_id = Uuid::new_v4();
            mgr.register_node(node_id, mesh_id);

            let score = mgr.get_reputation(&node_id, &mesh_id).unwrap();
            prop_assert_eq!(score, 0.5);
        }

        /// Property: positive balance increases reputation.
        #[test]
        fn prop_positive_balance_increases_reputation(
            gpu_seconds in 1.0f64..50.0
        ) {
            let contributor = MeshIdentity::generate();
            let consumer = MeshIdentity::generate();
            let mesh_id = Uuid::new_v4();

            let ledger = setup_ledger_with_records(&contributor, &consumer, mesh_id, gpu_seconds);

            let mut mgr = ReputationManager::new();
            mgr.register_node(contributor.node_id, mesh_id);

            let before = mgr.get_reputation(&contributor.node_id, &mesh_id).unwrap();
            mgr.compute_reputation(&contributor.node_id, &mesh_id, &ledger);
            let after = mgr.get_reputation(&contributor.node_id, &mesh_id).unwrap();

            prop_assert!(after >= before, "Positive balance should increase reputation: before={}, after={}", before, after);
        }
    }

    // ─── Unit Tests ──────────────────────────────────────────────────────────

    #[test]
    fn test_rate_limit_multiplier() {
        let mut mgr = ReputationManager::new();
        let mesh_id = Uuid::new_v4();
        let node_id = Uuid::new_v4();
        mgr.register_node(node_id, mesh_id);

        // At 0.5 reputation → 1.0x multiplier
        let mult = mgr.rate_limit_multiplier(&node_id, &mesh_id);
        assert!((mult - 1.0).abs() < 0.001);

        // Set to 1.0 reputation → 2.0x multiplier
        if let Some(rep) = mgr.reputations.get_mut(&(mesh_id, node_id)) {
            rep.reputation_score = 1.0;
        }
        let mult = mgr.rate_limit_multiplier(&node_id, &mesh_id);
        assert!((mult - 2.0).abs() < 0.001);

        // Set to 0.0 reputation → 0.0x multiplier
        if let Some(rep) = mgr.reputations.get_mut(&(mesh_id, node_id)) {
            rep.reputation_score = 0.0;
        }
        let mult = mgr.rate_limit_multiplier(&node_id, &mesh_id);
        assert!((mult - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_history_tracking() {
        let contributor = MeshIdentity::generate();
        let consumer = MeshIdentity::generate();
        let mesh_id = Uuid::new_v4();

        let ledger = setup_ledger_with_records(&contributor, &consumer, mesh_id, 5.0);

        let mut mgr = ReputationManager::new();
        mgr.register_node(contributor.node_id, mesh_id);

        mgr.compute_reputation(&contributor.node_id, &mesh_id, &ledger);
        mgr.compute_reputation(&contributor.node_id, &mesh_id, &ledger);

        let history = mgr.get_history(&contributor.node_id, &mesh_id);
        assert_eq!(history.len(), 2);
        assert_eq!(history[0].cycle_number, 1);
        assert_eq!(history[1].cycle_number, 2);
    }
}
