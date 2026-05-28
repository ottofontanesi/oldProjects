// Intent citation: .kiro/specs/mesh-network-optimizer/design.md Section 2.5, 3.9
// Consensus Protocol — proposal creation, voting, quorum checking, execution

use crate::mesh::identity::{MeshId, TrustTier};
use crate::mesh::solver::ModelId;
use crate::transport::trait_def::NodeId;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

// ─── Proposal Types ──────────────────────────────────────────────────────────

/// Types of proposals that can be voted on.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ProposalType {
    AddModel { model_id: ModelId },
    RemoveModel { model_id: ModelId },
    BanNode { node_id: NodeId, reason: String },
    ConfigChange { key: String, old_value: String, new_value: String },
    TrustChange { node_id: NodeId, new_tier: TrustTier },
}

impl ProposalType {
    /// Whether this is an emergency proposal (shorter timeout, lower threshold).
    pub fn is_emergency(&self) -> bool {
        matches!(self, ProposalType::BanNode { .. })
    }
}

/// A vote decision.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum VoteDecision {
    Yes,
    No,
    Abstain,
}

/// Status of a proposal.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ProposalStatus {
    Open,
    Passed,
    Rejected,
    Expired,
}

/// A single vote on a proposal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Vote {
    pub voter: NodeId,
    pub decision: VoteDecision,
    pub timestamp: DateTime<Utc>,
}

/// A proposal for the mesh to vote on.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Proposal {
    pub proposal_id: Uuid,
    pub mesh_id: MeshId,
    pub proposer: NodeId,
    pub proposal_type: ProposalType,
    pub description: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub votes: Vec<Vote>,
    pub status: ProposalStatus,
}

// ─── Consensus Manager ───────────────────────────────────────────────────────

/// Manages proposals and voting for mesh governance.
pub struct ConsensusManager {
    /// Active and historical proposals per mesh.
    proposals: HashMap<MeshId, Vec<Proposal>>,
    /// Normal proposal timeout (default: 24 hours).
    pub normal_timeout_hours: u32,
    /// Emergency proposal timeout (default: 1 hour).
    pub emergency_timeout_hours: u32,
    /// Quorum threshold: fraction of eligible voters that must participate (default: 0.50).
    pub quorum_threshold: f64,
    /// Approval threshold for normal proposals (default: 0.66).
    pub approval_threshold: f64,
    /// Approval threshold for emergency proposals (default: 0.50).
    pub emergency_approval_threshold: f64,
}

impl ConsensusManager {
    pub fn new() -> Self {
        Self {
            proposals: HashMap::new(),
            normal_timeout_hours: 24,
            emergency_timeout_hours: 1,
            quorum_threshold: 0.50,
            approval_threshold: 0.66,
            emergency_approval_threshold: 0.50,
        }
    }

    /// Create a new proposal. Only tier-3 nodes can create proposals.
    pub fn create_proposal(
        &mut self,
        proposer: NodeId,
        proposer_tier: TrustTier,
        mesh_id: MeshId,
        proposal_type: ProposalType,
        description: String,
    ) -> Result<Uuid, ConsensusError> {
        // Only tier-3 nodes can create proposals
        if proposer_tier != TrustTier::LocalOwned {
            return Err(ConsensusError::InsufficientTier);
        }

        let timeout_hours = if proposal_type.is_emergency() {
            self.emergency_timeout_hours
        } else {
            self.normal_timeout_hours
        };

        let proposal = Proposal {
            proposal_id: Uuid::new_v4(),
            mesh_id,
            proposer,
            proposal_type,
            description,
            created_at: Utc::now(),
            expires_at: Utc::now() + Duration::hours(timeout_hours as i64),
            votes: Vec::new(),
            status: ProposalStatus::Open,
        };

        let id = proposal.proposal_id;
        self.proposals
            .entry(mesh_id)
            .or_insert_with(Vec::new)
            .push(proposal);

        Ok(id)
    }

    /// Cast a vote on a proposal. Only tier-3 nodes can vote.
    pub fn cast_vote(
        &mut self,
        proposal_id: Uuid,
        voter: NodeId,
        voter_tier: TrustTier,
        decision: VoteDecision,
        eligible_voter_count: u32,
    ) -> Result<(), ConsensusError> {
        // Only tier-3 nodes can vote
        if voter_tier != TrustTier::LocalOwned {
            return Err(ConsensusError::InsufficientTier);
        }

        // Find the proposal
        let proposal = self.find_proposal_mut(&proposal_id)?;

        // Check proposal is still open
        if proposal.status != ProposalStatus::Open {
            return Err(ConsensusError::ProposalNotOpen);
        }

        // Check expiry
        if Utc::now() > proposal.expires_at {
            proposal.status = ProposalStatus::Expired;
            return Err(ConsensusError::ProposalExpired);
        }

        // Check for duplicate vote
        if proposal.votes.iter().any(|v| v.voter == voter) {
            return Err(ConsensusError::AlreadyVoted);
        }

        // Record vote
        proposal.votes.push(Vote {
            voter,
            decision,
            timestamp: Utc::now(),
        });

        // Check outcome
        self.check_outcome_internal(proposal_id, eligible_voter_count);

        Ok(())
    }

    /// Check the outcome of a proposal given the current votes.
    pub fn check_outcome(
        &mut self,
        proposal_id: &Uuid,
        eligible_voter_count: u32,
    ) -> Option<ProposalStatus> {
        self.check_outcome_internal(*proposal_id, eligible_voter_count)
    }

    fn check_outcome_internal(
        &mut self,
        proposal_id: Uuid,
        eligible_voter_count: u32,
    ) -> Option<ProposalStatus> {
        let quorum_threshold = self.quorum_threshold;
        let emergency_approval_threshold = self.emergency_approval_threshold;
        let approval_threshold = self.approval_threshold;

        let proposal = match self.find_proposal_mut(&proposal_id) {
            Ok(p) => p,
            Err(_) => return None,
        };

        if proposal.status != ProposalStatus::Open {
            return Some(proposal.status.clone());
        }

        // Check expiry
        if Utc::now() > proposal.expires_at {
            proposal.status = ProposalStatus::Expired;
            return Some(ProposalStatus::Expired);
        }

        let votes_cast = proposal.votes.len() as f64;
        let eligible = eligible_voter_count as f64;

        // Quorum check: >50% must participate
        if eligible > 0.0 && votes_cast / eligible <= quorum_threshold {
            return None; // Not enough votes yet
        }

        let yes_votes = proposal
            .votes
            .iter()
            .filter(|v| v.decision == VoteDecision::Yes)
            .count() as f64;
        let no_votes = proposal
            .votes
            .iter()
            .filter(|v| v.decision == VoteDecision::No)
            .count() as f64;

        let threshold = if proposal.proposal_type.is_emergency() {
            emergency_approval_threshold
        } else {
            approval_threshold
        };

        if votes_cast > 0.0 && yes_votes / votes_cast > threshold {
            proposal.status = ProposalStatus::Passed;
            Some(ProposalStatus::Passed)
        } else if votes_cast > 0.0 && no_votes / votes_cast >= (1.0 - threshold) {
            proposal.status = ProposalStatus::Rejected;
            Some(ProposalStatus::Rejected)
        } else {
            None
        }
    }

    /// Get a proposal by ID.
    pub fn get_proposal(&self, proposal_id: &Uuid) -> Option<&Proposal> {
        for proposals in self.proposals.values() {
            if let Some(p) = proposals.iter().find(|p| p.proposal_id == *proposal_id) {
                return Some(p);
            }
        }
        None
    }

    /// Get all active proposals for a mesh.
    pub fn active_proposals(&self, mesh_id: &MeshId) -> Vec<&Proposal> {
        self.proposals
            .get(mesh_id)
            .map(|proposals| {
                proposals
                    .iter()
                    .filter(|p| p.status == ProposalStatus::Open)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Expire all timed-out proposals. Returns IDs of newly expired proposals.
    pub fn expire_timed_out(&mut self) -> Vec<Uuid> {
        let now = Utc::now();
        let mut expired = Vec::new();

        for proposals in self.proposals.values_mut() {
            for proposal in proposals.iter_mut() {
                if proposal.status == ProposalStatus::Open && now > proposal.expires_at {
                    proposal.status = ProposalStatus::Expired;
                    expired.push(proposal.proposal_id);
                }
            }
        }

        expired
    }

    /// Find a mutable reference to a proposal by ID.
    fn find_proposal_mut(&mut self, proposal_id: &Uuid) -> Result<&mut Proposal, ConsensusError> {
        for proposals in self.proposals.values_mut() {
            if let Some(p) = proposals.iter_mut().find(|p| p.proposal_id == *proposal_id) {
                return Ok(p);
            }
        }
        Err(ConsensusError::ProposalNotFound)
    }
}

/// Errors during consensus operations.
#[derive(Debug, Clone, PartialEq)]
pub enum ConsensusError {
    InsufficientTier,
    ProposalNotFound,
    ProposalNotOpen,
    ProposalExpired,
    AlreadyVoted,
}

impl std::fmt::Display for ConsensusError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InsufficientTier => write!(f, "Only tier-3 nodes can create proposals or vote"),
            Self::ProposalNotFound => write!(f, "Proposal not found"),
            Self::ProposalNotOpen => write!(f, "Proposal is no longer open"),
            Self::ProposalExpired => write!(f, "Proposal has expired"),
            Self::AlreadyVoted => write!(f, "Node has already voted on this proposal"),
        }
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn arb_decision() -> impl Strategy<Value = VoteDecision> {
        prop_oneof![
            Just(VoteDecision::Yes),
            Just(VoteDecision::No),
            Just(VoteDecision::Abstain),
        ]
    }

    proptest! {
        /// Property: proposals without quorum never pass.
        #[test]
        fn prop_no_quorum_never_passes(
            num_voters in 1u32..10,
            votes_cast in 0u32..5
        ) {
            let mut cm = ConsensusManager::new();
            let mesh_id = Uuid::new_v4();
            let proposer = Uuid::new_v4();

            let eligible = num_voters.max(4); // At least 4 eligible
            let actual_votes = votes_cast.min(eligible / 2); // Ensure below quorum

            let pid = cm.create_proposal(
                proposer,
                TrustTier::LocalOwned,
                mesh_id,
                ProposalType::AddModel { model_id: "test".to_string() },
                "Test".to_string(),
            ).unwrap();

            // Cast fewer votes than quorum requires
            for i in 0..actual_votes {
                let voter = Uuid::new_v4();
                let _ = cm.cast_vote(pid, voter, TrustTier::LocalOwned, VoteDecision::Yes, eligible);
            }

            let proposal = cm.get_proposal(&pid).unwrap();
            let votes_fraction = proposal.votes.len() as f64 / eligible as f64;

            if votes_fraction <= 0.50 {
                prop_assert!(
                    proposal.status == ProposalStatus::Open,
                    "Without quorum, proposal should remain Open, got {:?}", proposal.status
                );
            }
        }

        /// Property: proposals below threshold never pass.
        #[test]
        fn prop_below_threshold_never_passes(
            num_yes in 0u32..3,
            num_no in 3u32..10
        ) {
            let mut cm = ConsensusManager::new();
            let mesh_id = Uuid::new_v4();
            let proposer = Uuid::new_v4();
            let eligible = num_yes + num_no + 1; // Ensure quorum met

            let pid = cm.create_proposal(
                proposer,
                TrustTier::LocalOwned,
                mesh_id,
                ProposalType::AddModel { model_id: "test".to_string() },
                "Test".to_string(),
            ).unwrap();

            for _ in 0..num_yes {
                let voter = Uuid::new_v4();
                let _ = cm.cast_vote(pid, voter, TrustTier::LocalOwned, VoteDecision::Yes, eligible);
            }
            for _ in 0..num_no {
                let voter = Uuid::new_v4();
                let _ = cm.cast_vote(pid, voter, TrustTier::LocalOwned, VoteDecision::No, eligible);
            }

            let proposal = cm.get_proposal(&pid).unwrap();
            let total_votes = (num_yes + num_no) as f64;
            let yes_fraction = num_yes as f64 / total_votes;

            if yes_fraction <= 0.66 {
                prop_assert!(
                    proposal.status != ProposalStatus::Passed,
                    "Below threshold should not pass: {}/{} = {:.2}", num_yes, total_votes, yes_fraction
                );
            }
        }

        /// Property: expired proposals marked as expired.
        #[test]
        fn prop_expired_proposals_marked(
            _dummy in 0u8..10
        ) {
            let mut cm = ConsensusManager::new();
            let mesh_id = Uuid::new_v4();
            let proposer = Uuid::new_v4();

            let pid = cm.create_proposal(
                proposer,
                TrustTier::LocalOwned,
                mesh_id,
                ProposalType::AddModel { model_id: "test".to_string() },
                "Test".to_string(),
            ).unwrap();

            // Manually expire the proposal
            if let Ok(p) = cm.find_proposal_mut(&pid) {
                p.expires_at = Utc::now() - Duration::hours(1);
            }

            let expired = cm.expire_timed_out();
            prop_assert!(expired.contains(&pid));

            let proposal = cm.get_proposal(&pid).unwrap();
            prop_assert_eq!(proposal.status.clone(), ProposalStatus::Expired);
        }

        /// Property: emergency proposals use correct (lower) threshold.
        #[test]
        fn prop_emergency_lower_threshold(
            num_yes in 3u32..6,
            num_no in 1u32..4
        ) {
            let mut cm = ConsensusManager::new();
            let mesh_id = Uuid::new_v4();
            let proposer = Uuid::new_v4();
            let eligible = num_yes + num_no;

            let pid = cm.create_proposal(
                proposer,
                TrustTier::LocalOwned,
                mesh_id,
                ProposalType::BanNode { node_id: Uuid::new_v4(), reason: "bad".to_string() },
                "Ban bad node".to_string(),
            ).unwrap();

            for _ in 0..num_yes {
                let voter = Uuid::new_v4();
                let _ = cm.cast_vote(pid, voter, TrustTier::LocalOwned, VoteDecision::Yes, eligible);
            }
            for _ in 0..num_no {
                let voter = Uuid::new_v4();
                let _ = cm.cast_vote(pid, voter, TrustTier::LocalOwned, VoteDecision::No, eligible);
            }

            let proposal = cm.get_proposal(&pid).unwrap();
            let total = (num_yes + num_no) as f64;
            let yes_fraction = num_yes as f64 / total;
            let quorum_met = total / eligible as f64 > 0.50;

            if quorum_met && yes_fraction > 0.50 {
                prop_assert_eq!(proposal.status.clone(), ProposalStatus::Passed,
                    "Emergency with {:.2} yes should pass (threshold 0.50)", yes_fraction);
            }
        }
    }

    // ─── Unit Tests ──────────────────────────────────────────────────────────

    #[test]
    fn test_non_tier3_cannot_create_proposal() {
        let mut cm = ConsensusManager::new();
        let result = cm.create_proposal(
            Uuid::new_v4(),
            TrustTier::InvitedFriend,
            Uuid::new_v4(),
            ProposalType::AddModel { model_id: "test".to_string() },
            "Test".to_string(),
        );
        assert_eq!(result, Err(ConsensusError::InsufficientTier));
    }

    #[test]
    fn test_non_tier3_cannot_vote() {
        let mut cm = ConsensusManager::new();
        let mesh_id = Uuid::new_v4();
        let pid = cm.create_proposal(
            Uuid::new_v4(),
            TrustTier::LocalOwned,
            mesh_id,
            ProposalType::AddModel { model_id: "test".to_string() },
            "Test".to_string(),
        ).unwrap();

        let result = cm.cast_vote(pid, Uuid::new_v4(), TrustTier::InvitedFriend, VoteDecision::Yes, 5);
        assert_eq!(result, Err(ConsensusError::InsufficientTier));
    }

    #[test]
    fn test_proposal_passes_with_supermajority() {
        let mut cm = ConsensusManager::new();
        let mesh_id = Uuid::new_v4();
        let eligible = 3;

        let pid = cm.create_proposal(
            Uuid::new_v4(),
            TrustTier::LocalOwned,
            mesh_id,
            ProposalType::RemoveModel { model_id: "old".to_string() },
            "Remove old model".to_string(),
        ).unwrap();

        // 3 yes votes out of 3 eligible (100% > 66%)
        for _ in 0..3 {
            cm.cast_vote(pid, Uuid::new_v4(), TrustTier::LocalOwned, VoteDecision::Yes, eligible).unwrap();
        }

        let proposal = cm.get_proposal(&pid).unwrap();
        assert_eq!(proposal.status, ProposalStatus::Passed);
    }

    #[test]
    fn test_duplicate_vote_rejected() {
        let mut cm = ConsensusManager::new();
        let mesh_id = Uuid::new_v4();
        let voter = Uuid::new_v4();

        let pid = cm.create_proposal(
            Uuid::new_v4(),
            TrustTier::LocalOwned,
            mesh_id,
            ProposalType::AddModel { model_id: "test".to_string() },
            "Test".to_string(),
        ).unwrap();

        cm.cast_vote(pid, voter, TrustTier::LocalOwned, VoteDecision::Yes, 5).unwrap();
        let result = cm.cast_vote(pid, voter, TrustTier::LocalOwned, VoteDecision::No, 5);
        assert_eq!(result, Err(ConsensusError::AlreadyVoted));
    }
}
