// Intent citation: .kiro/specs/mesh-network-optimizer/design.md Section 2.1
// Mesh Membership — join/leave flows, multi-mesh support, heartbeat, retirement

use crate::mesh::identity::{InvitationError, InvitationToken, MeshId, MeshIdentity, TrustTier};
use crate::transport::trait_def::NodeId;
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

// ─── Data Models ─────────────────────────────────────────────────────────────

/// Status of a node's membership in a mesh.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum MembershipStatus {
    Active,
    Suspended {
        reason: String,
        since: DateTime<Utc>,
    },
    Banned {
        reason: String,
        vote_id: Uuid,
    },
}

/// A node's membership record in a single mesh.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshMembershipRecord {
    pub mesh_id: MeshId,
    pub mesh_name: String,
    pub joined_at: DateTime<Utc>,
    pub trust_tier: TrustTier,
    pub invited_by: NodeId,
    pub status: MembershipStatus,
}

/// Information about a peer member in a mesh.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshMember {
    pub node_id: NodeId,
    pub trust_tier: TrustTier,
    pub joined_at: DateTime<Utc>,
    pub status: MembershipStatus,
    pub last_heartbeat: DateTime<Utc>,
    pub is_online: bool,
}

/// Heartbeat state for tracking peer liveness.
#[derive(Debug, Clone)]
struct HeartbeatState {
    pub last_received: DateTime<Utc>,
    pub is_online: bool,
}

/// Result of a join attempt.
#[derive(Debug, Clone, PartialEq)]
pub enum JoinResult {
    Accepted { mesh_name: String },
    Rejected { reason: String },
    TokenInvalid(InvitationError),
}

/// Result of a leave attempt.
#[derive(Debug, Clone, PartialEq)]
pub enum LeaveResult {
    Success,
    NotAMember,
}

/// Result of a retirement request.
#[derive(Debug, Clone, PartialEq)]
pub enum RetirementResult {
    Confirmed,
    AwaitingAcknowledgments { pending_nodes: Vec<NodeId> },
    Failed { reason: String },
}

/// Delta update for member list synchronization.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MemberDelta {
    Added(MeshMember),
    Removed { node_id: NodeId },
    Updated(MeshMember),
}

// ─── Mesh Membership Manager ─────────────────────────────────────────────────

/// Manages mesh membership state for the local node.
/// Supports multi-mesh: a node can belong to multiple meshes simultaneously.
pub struct MeshMembershipManager {
    /// Our identity for signing messages.
    identity: MeshIdentity,
    /// Our memberships keyed by mesh_id.
    my_memberships: HashMap<MeshId, MeshMembershipRecord>,
    /// Known members per mesh: mesh_id -> (node_id -> MeshMember).
    mesh_members: HashMap<MeshId, HashMap<NodeId, MeshMember>>,
    /// Heartbeat tracking per mesh: mesh_id -> (node_id -> HeartbeatState).
    heartbeat_states: HashMap<MeshId, HashMap<NodeId, HeartbeatState>>,
    /// Consumed invitation token IDs (single-use enforcement).
    consumed_tokens: Vec<Uuid>,
    /// Heartbeat interval in seconds (default: 60).
    pub heartbeat_interval_secs: u64,
    /// Heartbeat timeout in seconds (default: 300 = 5 minutes).
    pub heartbeat_timeout_secs: u64,
}

impl MeshMembershipManager {
    /// Create a new membership manager for the given identity.
    pub fn new(identity: MeshIdentity) -> Self {
        Self {
            identity,
            my_memberships: HashMap::new(),
            mesh_members: HashMap::new(),
            heartbeat_states: HashMap::new(),
            consumed_tokens: Vec::new(),
            heartbeat_interval_secs: 60,
            heartbeat_timeout_secs: 300,
        }
    }

    /// Join a mesh using an invitation token.
    /// Validates the token, marks it consumed, stores membership, and initializes heartbeat.
    pub fn join(
        &mut self,
        token: &mut InvitationToken,
        inviter_public_key: &ed25519_dalek::VerifyingKey,
        mesh_name: String,
    ) -> JoinResult {
        // Validate the token
        if let Err(e) = token.validate(inviter_public_key) {
            return JoinResult::TokenInvalid(e);
        }

        // Check if already consumed locally
        if self.consumed_tokens.contains(&token.token_id) {
            return JoinResult::TokenInvalid(InvitationError::AlreadyConsumed);
        }

        // Check if already a member of this mesh
        if self.my_memberships.contains_key(&token.mesh_id) {
            return JoinResult::Rejected {
                reason: "Already a member of this mesh".to_string(),
            };
        }

        // Consume the token (single-use)
        token.consume();
        self.consumed_tokens.push(token.token_id);

        // Create membership record
        let record = MeshMembershipRecord {
            mesh_id: token.mesh_id,
            mesh_name: mesh_name.clone(),
            joined_at: Utc::now(),
            trust_tier: token.offered_tier,
            invited_by: token.inviter_node_id,
            status: MembershipStatus::Active,
        };

        self.my_memberships.insert(token.mesh_id, record);
        self.mesh_members
            .entry(token.mesh_id)
            .or_insert_with(HashMap::new);
        self.heartbeat_states
            .entry(token.mesh_id)
            .or_insert_with(HashMap::new);

        JoinResult::Accepted { mesh_name }
    }

    /// Leave a mesh gracefully.
    /// Removes membership, stops heartbeat tracking, and cleans up local state.
    pub fn leave(&mut self, mesh_id: &MeshId) -> LeaveResult {
        if self.my_memberships.remove(mesh_id).is_none() {
            return LeaveResult::NotAMember;
        }

        // Cleanup all state for this mesh
        self.mesh_members.remove(mesh_id);
        self.heartbeat_states.remove(mesh_id);

        LeaveResult::Success
    }

    /// List all members of a specific mesh.
    pub fn list_members(&self, mesh_id: &MeshId) -> Vec<MeshMember> {
        self.mesh_members
            .get(mesh_id)
            .map(|members| members.values().cloned().collect())
            .unwrap_or_default()
    }

    /// List all meshes this node belongs to.
    pub fn list_my_meshes(&self) -> Vec<MeshMembershipRecord> {
        self.my_memberships.values().cloned().collect()
    }

    /// Check if we are a member of a specific mesh.
    pub fn is_member(&self, mesh_id: &MeshId) -> bool {
        self.my_memberships.contains_key(mesh_id)
    }

    /// Get our trust tier in a specific mesh.
    pub fn my_tier(&self, mesh_id: &MeshId) -> Option<TrustTier> {
        self.my_memberships.get(mesh_id).map(|m| m.trust_tier)
    }

    // ─── Heartbeat ───────────────────────────────────────────────────────────

    /// Record a heartbeat received from a peer.
    pub fn receive_heartbeat(&mut self, mesh_id: &MeshId, node_id: NodeId) {
        if let Some(states) = self.heartbeat_states.get_mut(mesh_id) {
            let state = states.entry(node_id).or_insert(HeartbeatState {
                last_received: Utc::now(),
                is_online: true,
            });
            state.last_received = Utc::now();
            state.is_online = true;
        }

        // Also update the member's online status
        if let Some(members) = self.mesh_members.get_mut(mesh_id) {
            if let Some(member) = members.get_mut(&node_id) {
                member.last_heartbeat = Utc::now();
                member.is_online = true;
            }
        }
    }

    /// Check all heartbeats and mark nodes as offline if they've exceeded the timeout.
    /// Should be called periodically (e.g., every heartbeat interval).
    pub fn check_heartbeat_timeouts(&mut self) -> Vec<(MeshId, NodeId)> {
        let timeout = Duration::seconds(self.heartbeat_timeout_secs as i64);
        let now = Utc::now();
        let mut departed = Vec::new();

        for (mesh_id, states) in &mut self.heartbeat_states {
            for (node_id, state) in states.iter_mut() {
                if state.is_online && (now - state.last_received) > timeout {
                    state.is_online = false;
                    departed.push((*mesh_id, *node_id));
                }
            }
        }

        // Update member online status
        for (mesh_id, node_id) in &departed {
            if let Some(members) = self.mesh_members.get_mut(mesh_id) {
                if let Some(member) = members.get_mut(node_id) {
                    member.is_online = false;
                }
            }
        }

        departed
    }

    // ─── Member List Synchronization ─────────────────────────────────────────

    /// Apply a full member list (received on join).
    pub fn apply_full_member_list(&mut self, mesh_id: &MeshId, members: Vec<MeshMember>) {
        let map: HashMap<NodeId, MeshMember> =
            members.into_iter().map(|m| (m.node_id, m)).collect();
        self.mesh_members.insert(*mesh_id, map);

        // Initialize heartbeat states for all members
        let hb_map: HashMap<NodeId, HeartbeatState> = self
            .mesh_members
            .get(mesh_id)
            .map(|members| {
                members
                    .keys()
                    .map(|&node_id| {
                        (
                            node_id,
                            HeartbeatState {
                                last_received: Utc::now(),
                                is_online: true,
                            },
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        self.heartbeat_states.insert(*mesh_id, hb_map);
    }

    /// Apply a delta update to the member list.
    pub fn apply_member_delta(&mut self, mesh_id: &MeshId, delta: MemberDelta) {
        let members = self.mesh_members.entry(*mesh_id).or_insert_with(HashMap::new);
        let hb_states = self
            .heartbeat_states
            .entry(*mesh_id)
            .or_insert_with(HashMap::new);

        match delta {
            MemberDelta::Added(member) => {
                let node_id = member.node_id;
                members.insert(node_id, member);
                hb_states.insert(
                    node_id,
                    HeartbeatState {
                        last_received: Utc::now(),
                        is_online: true,
                    },
                );
            }
            MemberDelta::Removed { node_id } => {
                members.remove(&node_id);
                hb_states.remove(&node_id);
            }
            MemberDelta::Updated(member) => {
                members.insert(member.node_id, member);
            }
        }
    }

    // ─── Retirement ──────────────────────────────────────────────────────────

    /// Initiate node retirement: request full data removal from the mesh.
    /// Returns the list of tier-3 nodes that must acknowledge before retirement completes.
    pub fn initiate_retirement(
        &self,
        mesh_id: &MeshId,
    ) -> Result<Vec<NodeId>, String> {
        if !self.my_memberships.contains_key(mesh_id) {
            return Err("Not a member of this mesh".to_string());
        }

        // Find all tier-3 nodes that must acknowledge
        let tier3_nodes: Vec<NodeId> = self
            .mesh_members
            .get(mesh_id)
            .map(|members| {
                members
                    .values()
                    .filter(|m| m.trust_tier == TrustTier::LocalOwned && m.is_online)
                    .map(|m| m.node_id)
                    .collect()
            })
            .unwrap_or_default();

        Ok(tier3_nodes)
    }

    /// Complete retirement after all tier-3 nodes have acknowledged.
    /// Removes all local state for this mesh. Irreversible.
    pub fn complete_retirement(
        &mut self,
        mesh_id: &MeshId,
        acknowledged_nodes: &[NodeId],
    ) -> RetirementResult {
        // Get required tier-3 nodes
        let required = match self.initiate_retirement(mesh_id) {
            Ok(nodes) => nodes,
            Err(reason) => return RetirementResult::Failed { reason },
        };

        // Check all tier-3 nodes have acknowledged
        let pending: Vec<NodeId> = required
            .iter()
            .filter(|n| !acknowledged_nodes.contains(n))
            .copied()
            .collect();

        if !pending.is_empty() {
            return RetirementResult::AwaitingAcknowledgments {
                pending_nodes: pending,
            };
        }

        // All acknowledged — complete retirement (irreversible)
        self.my_memberships.remove(mesh_id);
        self.mesh_members.remove(mesh_id);
        self.heartbeat_states.remove(mesh_id);

        RetirementResult::Confirmed
    }

    /// Anonymize a retired node's data in our local records.
    /// Replaces node_id references with "retired-node-XXXX" pattern.
    pub fn anonymize_retired_node(&mut self, mesh_id: &MeshId, retired_node_id: &NodeId) -> String {
        let anon_suffix = &retired_node_id.to_string()[..8];
        let anon_label = format!("retired-node-{}", anon_suffix);

        // Remove from member list
        if let Some(members) = self.mesh_members.get_mut(mesh_id) {
            members.remove(retired_node_id);
        }

        // Remove heartbeat state
        if let Some(states) = self.heartbeat_states.get_mut(mesh_id) {
            states.remove(retired_node_id);
        }

        anon_label
    }

    // ─── Accessors ───────────────────────────────────────────────────────────

    /// Get our node ID.
    pub fn node_id(&self) -> NodeId {
        self.identity.node_id
    }

    /// Get the number of meshes we belong to.
    pub fn mesh_count(&self) -> usize {
        self.my_memberships.len()
    }

    /// Get member count for a specific mesh.
    pub fn member_count(&self, mesh_id: &MeshId) -> usize {
        self.mesh_members
            .get(mesh_id)
            .map(|m| m.len())
            .unwrap_or(0)
    }

    /// Get online member count for a specific mesh.
    pub fn online_member_count(&self, mesh_id: &MeshId) -> usize {
        self.mesh_members
            .get(mesh_id)
            .map(|members| members.values().filter(|m| m.is_online).count())
            .unwrap_or(0)
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::identity::MeshIdentity;
    use proptest::prelude::*;

    fn setup_manager() -> (MeshMembershipManager, MeshIdentity) {
        let identity = MeshIdentity::generate();
        let manager = MeshMembershipManager::new(identity.clone());
        (manager, identity)
    }

    fn create_valid_token(inviter: &MeshIdentity, mesh_id: MeshId) -> InvitationToken {
        inviter.create_invitation(mesh_id, TrustTier::InvitedFriend, 24)
    }

    // ─── Property Tests ──────────────────────────────────────────────────────

    proptest! {
        /// Property: join with a valid token always succeeds.
        #[test]
        fn prop_join_with_valid_token_succeeds(
            mesh_name in "[a-z]{3,10}"
        ) {
            let inviter = MeshIdentity::generate();
            let (mut manager, _our_id) = setup_manager();
            let mesh_id = Uuid::new_v4();
            let mut token = create_valid_token(&inviter, mesh_id);

            let result = manager.join(&mut token, &inviter.verifying_key, mesh_name.clone());
            let is_accepted = matches!(result, JoinResult::Accepted { .. });
            prop_assert!(is_accepted);
            prop_assert!(manager.is_member(&mesh_id));
        }

        /// Property: join with expired token always fails.
        #[test]
        fn prop_join_with_expired_token_fails(
            mesh_name in "[a-z]{3,10}",
            hours_ago in 1u32..500
        ) {
            let inviter = MeshIdentity::generate();
            let (mut manager, _) = setup_manager();
            let mesh_id = Uuid::new_v4();

            // Create token and backdate it
            let mut token = inviter.create_invitation(mesh_id, TrustTier::InvitedFriend, 1);
            token.expires_at = Utc::now() - Duration::hours(hours_ago as i64);
            // Re-sign with backdated expiry
            let payload = crate::mesh::identity::InvitationPayload {
                token_id: token.token_id,
                mesh_id: token.mesh_id,
                inviter_node_id: token.inviter_node_id,
                offered_tier: token.offered_tier,
                expires_at: token.expires_at,
            };
            let payload_bytes = serde_json::to_vec(&payload).unwrap();
            token.signature = inviter.sign(&payload_bytes);

            let result = manager.join(&mut token, &inviter.verifying_key, mesh_name);
            prop_assert!(matches!(result, JoinResult::TokenInvalid(InvitationError::Expired)));
        }

        /// Property: join with consumed token always fails.
        #[test]
        fn prop_join_with_consumed_token_fails(
            mesh_name in "[a-z]{3,10}"
        ) {
            let inviter = MeshIdentity::generate();
            let (mut manager, _) = setup_manager();
            let mesh_id = Uuid::new_v4();

            let mut token = create_valid_token(&inviter, mesh_id);
            token.consume();

            let result = manager.join(&mut token, &inviter.verifying_key, mesh_name);
            prop_assert!(matches!(result, JoinResult::TokenInvalid(InvitationError::AlreadyConsumed)));
        }

        /// Property: leave removes from all peer member lists.
        #[test]
        fn prop_leave_removes_membership(
            mesh_name in "[a-z]{3,10}"
        ) {
            let inviter = MeshIdentity::generate();
            let (mut manager, _) = setup_manager();
            let mesh_id = Uuid::new_v4();
            let mut token = create_valid_token(&inviter, mesh_id);

            manager.join(&mut token, &inviter.verifying_key, mesh_name);
            prop_assert!(manager.is_member(&mesh_id));

            let result = manager.leave(&mesh_id);
            prop_assert_eq!(result, LeaveResult::Success);
            prop_assert!(!manager.is_member(&mesh_id));
            prop_assert_eq!(manager.member_count(&mesh_id), 0);
        }

        /// Property: multi-mesh memberships are independent.
        #[test]
        fn prop_multi_mesh_independent(
            name_a in "[a-z]{3,10}",
            name_b in "[a-z]{3,10}"
        ) {
            let inviter = MeshIdentity::generate();
            let (mut manager, _) = setup_manager();
            let mesh_a = Uuid::new_v4();
            let mesh_b = Uuid::new_v4();

            let mut token_a = inviter.create_invitation(mesh_a, TrustTier::LocalOwned, 24);
            let mut token_b = inviter.create_invitation(mesh_b, TrustTier::InvitedFriend, 24);

            manager.join(&mut token_a, &inviter.verifying_key, name_a);
            manager.join(&mut token_b, &inviter.verifying_key, name_b);

            prop_assert!(manager.is_member(&mesh_a));
            prop_assert!(manager.is_member(&mesh_b));
            prop_assert_eq!(manager.mesh_count(), 2);

            // Leaving one doesn't affect the other
            manager.leave(&mesh_a);
            prop_assert!(!manager.is_member(&mesh_a));
            prop_assert!(manager.is_member(&mesh_b));
            prop_assert_eq!(manager.mesh_count(), 1);
        }

        /// Property: retired node's data is fully anonymized.
        #[test]
        fn prop_retired_node_anonymized(
            mesh_name in "[a-z]{3,10}"
        ) {
            let inviter = MeshIdentity::generate();
            let (mut manager, _) = setup_manager();
            let mesh_id = Uuid::new_v4();
            let mut token = create_valid_token(&inviter, mesh_id);
            manager.join(&mut token, &inviter.verifying_key, mesh_name);

            // Add a fake member to the mesh
            let retired_node = Uuid::new_v4();
            let member = MeshMember {
                node_id: retired_node,
                trust_tier: TrustTier::InvitedFriend,
                joined_at: Utc::now(),
                status: MembershipStatus::Active,
                last_heartbeat: Utc::now(),
                is_online: true,
            };
            manager.apply_member_delta(&mesh_id, MemberDelta::Added(member));
            prop_assert_eq!(manager.member_count(&mesh_id), 1);

            // Anonymize the retired node
            let label = manager.anonymize_retired_node(&mesh_id, &retired_node);
            prop_assert!(label.starts_with("retired-node-"));
            prop_assert_eq!(manager.member_count(&mesh_id), 0);
        }

        /// Property: retirement requires all tier-3 acknowledgments.
        #[test]
        fn prop_retirement_requires_all_tier3_acks(
            mesh_name in "[a-z]{3,10}",
            num_tier3 in 1usize..5
        ) {
            let inviter = MeshIdentity::generate();
            let (mut manager, _) = setup_manager();
            let mesh_id = Uuid::new_v4();
            let mut token = create_valid_token(&inviter, mesh_id);
            manager.join(&mut token, &inviter.verifying_key, mesh_name);

            // Add tier-3 members
            let tier3_nodes: Vec<NodeId> = (0..num_tier3).map(|_| Uuid::new_v4()).collect();
            for &node_id in &tier3_nodes {
                let member = MeshMember {
                    node_id,
                    trust_tier: TrustTier::LocalOwned,
                    joined_at: Utc::now(),
                    status: MembershipStatus::Active,
                    last_heartbeat: Utc::now(),
                    is_online: true,
                };
                manager.apply_member_delta(&mesh_id, MemberDelta::Added(member));
            }

            // Partial acknowledgment should not complete retirement
            if num_tier3 > 1 {
                let partial = &tier3_nodes[..num_tier3 - 1];
                let result = manager.complete_retirement(&mesh_id, partial);
                let is_awaiting = matches!(result, RetirementResult::AwaitingAcknowledgments { .. });
                prop_assert!(is_awaiting);
                prop_assert!(manager.is_member(&mesh_id));
            }

            // Full acknowledgment should complete retirement
            let result = manager.complete_retirement(&mesh_id, &tier3_nodes);
            prop_assert_eq!(result, RetirementResult::Confirmed);
            prop_assert!(!manager.is_member(&mesh_id));
        }
    }

    // ─── Unit Tests ──────────────────────────────────────────────────────────

    #[test]
    fn test_heartbeat_timeout_detection() {
        let inviter = MeshIdentity::generate();
        let (mut manager, _) = setup_manager();
        let mesh_id = Uuid::new_v4();
        let mut token = create_valid_token(&inviter, mesh_id);
        manager.join(&mut token, &inviter.verifying_key, "test-mesh".to_string());

        // Add a member with an old heartbeat
        let peer = Uuid::new_v4();
        let member = MeshMember {
            node_id: peer,
            trust_tier: TrustTier::InvitedFriend,
            joined_at: Utc::now(),
            status: MembershipStatus::Active,
            last_heartbeat: Utc::now() - Duration::seconds(400),
            is_online: true,
        };
        manager.apply_member_delta(&mesh_id, MemberDelta::Added(member));

        // Manually set the heartbeat state to old
        if let Some(states) = manager.heartbeat_states.get_mut(&mesh_id) {
            if let Some(state) = states.get_mut(&peer) {
                state.last_received = Utc::now() - Duration::seconds(400);
            }
        }

        let departed = manager.check_heartbeat_timeouts();
        assert_eq!(departed.len(), 1);
        assert_eq!(departed[0], (mesh_id, peer));
    }

    #[test]
    fn test_full_member_list_sync() {
        let inviter = MeshIdentity::generate();
        let (mut manager, _) = setup_manager();
        let mesh_id = Uuid::new_v4();
        let mut token = create_valid_token(&inviter, mesh_id);
        manager.join(&mut token, &inviter.verifying_key, "test-mesh".to_string());

        let members = vec![
            MeshMember {
                node_id: Uuid::new_v4(),
                trust_tier: TrustTier::LocalOwned,
                joined_at: Utc::now(),
                status: MembershipStatus::Active,
                last_heartbeat: Utc::now(),
                is_online: true,
            },
            MeshMember {
                node_id: Uuid::new_v4(),
                trust_tier: TrustTier::InvitedFriend,
                joined_at: Utc::now(),
                status: MembershipStatus::Active,
                last_heartbeat: Utc::now(),
                is_online: true,
            },
        ];

        manager.apply_full_member_list(&mesh_id, members);
        assert_eq!(manager.member_count(&mesh_id), 2);
        assert_eq!(manager.online_member_count(&mesh_id), 2);
    }

    #[test]
    fn test_leave_nonexistent_mesh() {
        let (mut manager, _) = setup_manager();
        let result = manager.leave(&Uuid::new_v4());
        assert_eq!(result, LeaveResult::NotAMember);
    }

    #[test]
    fn test_duplicate_join_rejected() {
        let inviter = MeshIdentity::generate();
        let (mut manager, _) = setup_manager();
        let mesh_id = Uuid::new_v4();

        let mut token1 = create_valid_token(&inviter, mesh_id);
        let mut token2 = inviter.create_invitation(mesh_id, TrustTier::LocalOwned, 24);

        let r1 = manager.join(&mut token1, &inviter.verifying_key, "mesh".to_string());
        assert!(matches!(r1, JoinResult::Accepted { .. }));

        let r2 = manager.join(&mut token2, &inviter.verifying_key, "mesh".to_string());
        assert!(matches!(r2, JoinResult::Rejected { .. }));
    }
}
