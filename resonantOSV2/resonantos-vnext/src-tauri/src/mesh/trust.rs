// Intent citation: .kiro/specs/mesh-network-optimizer/design.md Section 2.2
// Trust Manager — trust tier assignment, promotion/demotion, routing enforcement

use crate::mesh::identity::{MeshId, TrustTier};
use crate::transport::trait_def::NodeId;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ─── Prompt Sensitivity (forward declaration for routing) ────────────────────

/// Sensitivity level of a prompt — determines routing constraints.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum PromptSensitivity {
    /// Local-only, never leaves tier-3 nodes.
    Sensitive,
    /// Can be routed to tier-2+ nodes.
    NonSensitive,
}

// ─── Trust State ─────────────────────────────────────────────────────────────

/// Trust information for a single node in a mesh.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeTrustInfo {
    pub node_id: NodeId,
    pub mesh_id: MeshId,
    pub tier: TrustTier,
    pub promoted_at: Option<DateTime<Utc>>,
    pub demoted_at: Option<DateTime<Utc>>,
    pub joined_at: DateTime<Utc>,
    pub invited_by: NodeId,
}

/// Errors that can occur during trust operations.
#[derive(Debug, Clone, PartialEq)]
pub enum TrustError {
    NodeNotFound,
    InsufficientPermission,
    PromotionRequirementsNotMet { missing: Vec<String> },
    AlreadyAtTier(TrustTier),
    CannotDemoteBelowPublic,
}

impl std::fmt::Display for TrustError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NodeNotFound => write!(f, "Node not found in mesh"),
            Self::InsufficientPermission => write!(f, "Only tier-3 owners can change trust tiers"),
            Self::PromotionRequirementsNotMet { missing } => {
                write!(f, "Promotion requirements not met: {}", missing.join(", "))
            }
            Self::AlreadyAtTier(tier) => write!(f, "Node is already at tier {:?}", tier),
            Self::CannotDemoteBelowPublic => write!(f, "Cannot demote below Public tier"),
        }
    }
}

// ─── Trust Manager ───────────────────────────────────────────────────────────

/// Manages trust tiers for all nodes across all meshes.
pub struct TrustManager {
    /// Trust info per mesh: mesh_id -> (node_id -> NodeTrustInfo).
    trust_state: HashMap<MeshId, HashMap<NodeId, NodeTrustInfo>>,
    /// Minimum reputation required for promotion (default: 0.7).
    pub promotion_min_reputation: f64,
    /// Minimum days of participation required for promotion (default: 7).
    pub promotion_min_days: u32,
}

impl TrustManager {
    pub fn new() -> Self {
        Self {
            trust_state: HashMap::new(),
            promotion_min_reputation: 0.7,
            promotion_min_days: 7,
        }
    }

    /// Get the trust tier for a node in a mesh.
    pub fn get_tier(&self, mesh_id: &MeshId, node_id: &NodeId) -> Option<TrustTier> {
        self.trust_state
            .get(mesh_id)
            .and_then(|nodes| nodes.get(node_id))
            .map(|info| info.tier)
    }

    /// Register a node's trust info (called when a node joins).
    pub fn register_node(
        &mut self,
        mesh_id: MeshId,
        node_id: NodeId,
        tier: TrustTier,
        invited_by: NodeId,
    ) {
        let info = NodeTrustInfo {
            node_id,
            mesh_id,
            tier,
            promoted_at: None,
            demoted_at: None,
            joined_at: Utc::now(),
            invited_by,
        };
        self.trust_state
            .entry(mesh_id)
            .or_insert_with(HashMap::new)
            .insert(node_id, info);
    }

    /// Remove a node's trust info (called when a node leaves or is retired).
    pub fn unregister_node(&mut self, mesh_id: &MeshId, node_id: &NodeId) {
        if let Some(nodes) = self.trust_state.get_mut(mesh_id) {
            nodes.remove(node_id);
        }
    }

    /// Change a node's trust tier. Only tier-3 owners can do this.
    /// For promotion, additional requirements must be met.
    pub fn change_tier(
        &mut self,
        mesh_id: &MeshId,
        requester_id: &NodeId,
        target_node_id: &NodeId,
        new_tier: TrustTier,
        target_reputation: f64,
    ) -> Result<TrustTier, TrustError> {
        // Verify requester is tier-3
        let requester_tier = self
            .get_tier(mesh_id, requester_id)
            .ok_or(TrustError::NodeNotFound)?;
        if requester_tier != TrustTier::LocalOwned {
            return Err(TrustError::InsufficientPermission);
        }

        // Get target node info
        let nodes = self
            .trust_state
            .get_mut(mesh_id)
            .ok_or(TrustError::NodeNotFound)?;
        let info = nodes
            .get_mut(target_node_id)
            .ok_or(TrustError::NodeNotFound)?;

        let current_tier = info.tier;
        if current_tier == new_tier {
            return Err(TrustError::AlreadyAtTier(new_tier));
        }

        // Promotion check (going up)
        if new_tier > current_tier {
            let mut missing = Vec::new();

            // Requirement 1: tier-3 owner action (already verified above)

            // Requirement 2: reputation >= threshold
            if target_reputation < self.promotion_min_reputation {
                missing.push(format!(
                    "reputation {:.2} < required {:.2}",
                    target_reputation, self.promotion_min_reputation
                ));
            }

            // Requirement 3: participation >= min days
            let days_participated = (Utc::now() - info.joined_at).num_days();
            if days_participated < self.promotion_min_days as i64 {
                missing.push(format!(
                    "participation {} days < required {} days",
                    days_participated, self.promotion_min_days
                ));
            }

            if !missing.is_empty() {
                return Err(TrustError::PromotionRequirementsNotMet { missing });
            }

            info.tier = new_tier;
            info.promoted_at = Some(Utc::now());
        } else {
            // Demotion (going down) — always allowed for tier-3 owners
            if new_tier < TrustTier::Public {
                return Err(TrustError::CannotDemoteBelowPublic);
            }
            info.tier = new_tier;
            info.demoted_at = Some(Utc::now());
        }

        Ok(new_tier)
    }

    /// Demote a node due to reputation drop below threshold.
    /// This is an automatic action, not requiring a tier-3 owner request.
    pub fn auto_demote_for_reputation(
        &mut self,
        mesh_id: &MeshId,
        node_id: &NodeId,
        reputation: f64,
        threshold: f64,
    ) -> Option<TrustTier> {
        if reputation >= threshold {
            return None;
        }

        let nodes = self.trust_state.get_mut(mesh_id)?;
        let info = nodes.get_mut(node_id)?;

        // Only demote if currently above Public
        if info.tier > TrustTier::Public {
            let new_tier = match info.tier {
                TrustTier::LocalOwned => TrustTier::InvitedFriend,
                TrustTier::InvitedFriend => TrustTier::Public,
                TrustTier::Public => return None,
            };
            info.tier = new_tier;
            info.demoted_at = Some(Utc::now());
            Some(new_tier)
        } else {
            None
        }
    }

    /// Core routing decision: can a request with given sensitivity be routed to this node?
    ///
    /// Rules:
    /// - Tier 3 (LocalOwned): any prompt (sensitive or non-sensitive)
    /// - Tier 2 (InvitedFriend): non-sensitive only
    /// - Tier 1 (Public): routing only, no prompts at all
    pub fn can_route_to(
        &self,
        mesh_id: &MeshId,
        node_id: &NodeId,
        sensitivity: PromptSensitivity,
    ) -> bool {
        let tier = match self.get_tier(mesh_id, node_id) {
            Some(t) => t,
            None => return false,
        };

        match sensitivity {
            PromptSensitivity::Sensitive => tier == TrustTier::LocalOwned,
            PromptSensitivity::NonSensitive => tier >= TrustTier::InvitedFriend,
        }
    }

    /// Check if a node can see prompt content (not just metadata).
    ///
    /// - Tier 3: can see all prompts
    /// - Tier 2: can see non-sensitive prompts
    /// - Tier 1: cannot see any prompts (metadata only)
    pub fn can_see_prompts(
        &self,
        mesh_id: &MeshId,
        node_id: &NodeId,
        sensitivity: PromptSensitivity,
    ) -> bool {
        // Same logic as can_route_to for prompt visibility
        self.can_route_to(mesh_id, node_id, sensitivity)
    }

    /// Get all nodes in a mesh at or above a given tier.
    pub fn nodes_at_tier_or_above(
        &self,
        mesh_id: &MeshId,
        min_tier: TrustTier,
    ) -> Vec<NodeId> {
        self.trust_state
            .get(mesh_id)
            .map(|nodes| {
                nodes
                    .values()
                    .filter(|info| info.tier >= min_tier)
                    .map(|info| info.node_id)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Count tier-3 nodes in a mesh (needed for consensus quorum).
    pub fn count_tier3(&self, mesh_id: &MeshId) -> usize {
        self.trust_state
            .get(mesh_id)
            .map(|nodes| {
                nodes
                    .values()
                    .filter(|info| info.tier == TrustTier::LocalOwned)
                    .count()
            })
            .unwrap_or(0)
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;
    use proptest::prelude::*;
    use uuid::Uuid;

    fn arb_sensitivity() -> impl Strategy<Value = PromptSensitivity> {
        prop_oneof![
            Just(PromptSensitivity::Sensitive),
            Just(PromptSensitivity::NonSensitive),
        ]
    }

    fn arb_tier() -> impl Strategy<Value = TrustTier> {
        prop_oneof![
            Just(TrustTier::Public),
            Just(TrustTier::InvitedFriend),
            Just(TrustTier::LocalOwned),
        ]
    }

    proptest! {
        /// Property: sensitive prompts NEVER routable to tier < 3.
        #[test]
        fn prop_sensitive_never_routable_below_tier3(
            tier in arb_tier()
        ) {
            let mut tm = TrustManager::new();
            let mesh_id = Uuid::new_v4();
            let node_id = Uuid::new_v4();
            let owner = Uuid::new_v4();

            tm.register_node(mesh_id, node_id, tier, owner);

            let can_route = tm.can_route_to(&mesh_id, &node_id, PromptSensitivity::Sensitive);

            if tier == TrustTier::LocalOwned {
                prop_assert!(can_route);
            } else {
                prop_assert!(!can_route, "Sensitive prompt routed to tier {:?}", tier);
            }
        }

        /// Property: promotion requires all 3 conditions (tier-3 owner, reputation >= 0.7, participation >= 7 days).
        #[test]
        fn prop_promotion_requires_all_conditions(
            reputation in 0.0f64..1.0,
            days_since_join in 0i64..30
        ) {
            let mut tm = TrustManager::new();
            let mesh_id = Uuid::new_v4();
            let owner_id = Uuid::new_v4();
            let target_id = Uuid::new_v4();

            // Register owner as tier-3
            tm.register_node(mesh_id, owner_id, TrustTier::LocalOwned, owner_id);

            // Register target as tier-1 with a specific join date
            tm.register_node(mesh_id, target_id, TrustTier::Public, owner_id);
            // Backdate the join
            if let Some(nodes) = tm.trust_state.get_mut(&mesh_id) {
                if let Some(info) = nodes.get_mut(&target_id) {
                    info.joined_at = Utc::now() - Duration::days(days_since_join);
                }
            }

            let result = tm.change_tier(
                &mesh_id,
                &owner_id,
                &target_id,
                TrustTier::InvitedFriend,
                reputation,
            );

            let rep_ok = reputation >= 0.7;
            let days_ok = days_since_join >= 7;

            if rep_ok && days_ok {
                prop_assert!(result.is_ok(), "Should succeed: rep={}, days={}", reputation, days_since_join);
            } else {
                prop_assert!(
                    matches!(result, Err(TrustError::PromotionRequirementsNotMet { .. })),
                    "Should fail: rep={}, days={}, got {:?}", reputation, days_since_join, result
                );
            }
        }

        /// Property: demotion always succeeds for tier-3 owners.
        #[test]
        fn prop_demotion_always_succeeds_for_owners(
            target_tier in arb_tier()
        ) {
            let mut tm = TrustManager::new();
            let mesh_id = Uuid::new_v4();
            let owner_id = Uuid::new_v4();
            let target_id = Uuid::new_v4();

            tm.register_node(mesh_id, owner_id, TrustTier::LocalOwned, owner_id);
            tm.register_node(mesh_id, target_id, TrustTier::LocalOwned, owner_id);

            // Demote to a lower tier
            let new_tier = match target_tier {
                TrustTier::LocalOwned => TrustTier::InvitedFriend,
                TrustTier::InvitedFriend => TrustTier::Public,
                TrustTier::Public => {
                    // Can't demote below public — skip this case
                    return Ok(());
                }
            };

            // Set target to the starting tier first
            if let Some(nodes) = tm.trust_state.get_mut(&mesh_id) {
                if let Some(info) = nodes.get_mut(&target_id) {
                    info.tier = target_tier;
                }
            }

            let result = tm.change_tier(&mesh_id, &owner_id, &target_id, new_tier, 0.0);
            prop_assert!(result.is_ok(), "Demotion should always succeed for tier-3 owners");
        }
    }

    // ─── Unit Tests ──────────────────────────────────────────────────────────

    #[test]
    fn test_register_and_get_tier() {
        let mut tm = TrustManager::new();
        let mesh_id = Uuid::new_v4();
        let node_id = Uuid::new_v4();
        let owner = Uuid::new_v4();

        tm.register_node(mesh_id, node_id, TrustTier::InvitedFriend, owner);
        assert_eq!(tm.get_tier(&mesh_id, &node_id), Some(TrustTier::InvitedFriend));
    }

    #[test]
    fn test_can_route_to_tier_enforcement() {
        let mut tm = TrustManager::new();
        let mesh_id = Uuid::new_v4();
        let owner = Uuid::new_v4();
        let tier1 = Uuid::new_v4();
        let tier2 = Uuid::new_v4();
        let tier3 = Uuid::new_v4();

        tm.register_node(mesh_id, tier1, TrustTier::Public, owner);
        tm.register_node(mesh_id, tier2, TrustTier::InvitedFriend, owner);
        tm.register_node(mesh_id, tier3, TrustTier::LocalOwned, owner);

        // Sensitive: only tier-3
        assert!(!tm.can_route_to(&mesh_id, &tier1, PromptSensitivity::Sensitive));
        assert!(!tm.can_route_to(&mesh_id, &tier2, PromptSensitivity::Sensitive));
        assert!(tm.can_route_to(&mesh_id, &tier3, PromptSensitivity::Sensitive));

        // NonSensitive: tier-2 and tier-3
        assert!(!tm.can_route_to(&mesh_id, &tier1, PromptSensitivity::NonSensitive));
        assert!(tm.can_route_to(&mesh_id, &tier2, PromptSensitivity::NonSensitive));
        assert!(tm.can_route_to(&mesh_id, &tier3, PromptSensitivity::NonSensitive));
    }

    #[test]
    fn test_non_owner_cannot_change_tier() {
        let mut tm = TrustManager::new();
        let mesh_id = Uuid::new_v4();
        let owner = Uuid::new_v4();
        let non_owner = Uuid::new_v4();
        let target = Uuid::new_v4();

        tm.register_node(mesh_id, owner, TrustTier::LocalOwned, owner);
        tm.register_node(mesh_id, non_owner, TrustTier::InvitedFriend, owner);
        tm.register_node(mesh_id, target, TrustTier::Public, owner);

        let result = tm.change_tier(&mesh_id, &non_owner, &target, TrustTier::InvitedFriend, 1.0);
        assert_eq!(result, Err(TrustError::InsufficientPermission));
    }

    #[test]
    fn test_auto_demote_for_low_reputation() {
        let mut tm = TrustManager::new();
        let mesh_id = Uuid::new_v4();
        let owner = Uuid::new_v4();
        let node = Uuid::new_v4();

        tm.register_node(mesh_id, node, TrustTier::InvitedFriend, owner);

        // Reputation above threshold — no demotion
        let result = tm.auto_demote_for_reputation(&mesh_id, &node, 0.5, 0.3);
        assert_eq!(result, None);

        // Reputation below threshold — demote
        let result = tm.auto_demote_for_reputation(&mesh_id, &node, 0.2, 0.3);
        assert_eq!(result, Some(TrustTier::Public));
        assert_eq!(tm.get_tier(&mesh_id, &node), Some(TrustTier::Public));
    }

    #[test]
    fn test_nodes_at_tier_or_above() {
        let mut tm = TrustManager::new();
        let mesh_id = Uuid::new_v4();
        let owner = Uuid::new_v4();
        let n1 = Uuid::new_v4();
        let n2 = Uuid::new_v4();
        let n3 = Uuid::new_v4();

        tm.register_node(mesh_id, n1, TrustTier::Public, owner);
        tm.register_node(mesh_id, n2, TrustTier::InvitedFriend, owner);
        tm.register_node(mesh_id, n3, TrustTier::LocalOwned, owner);

        let tier2_plus = tm.nodes_at_tier_or_above(&mesh_id, TrustTier::InvitedFriend);
        assert_eq!(tier2_plus.len(), 2);
        assert!(tier2_plus.contains(&n2));
        assert!(tier2_plus.contains(&n3));
    }
}
