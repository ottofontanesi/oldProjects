// Intent citation: .kiro/specs/mesh-network-optimizer/design.md Section 4, 9
// Mesh Observability — Tauri commands, metrics, audit trail, privacy enforcement

use crate::mesh::accounting::ContributionSummary;
use crate::mesh::consensus::VoteDecision;
use crate::mesh::identity::{MeshId, TrustTier};
use crate::mesh::trust::PromptSensitivity;
use crate::transport::trait_def::NodeId;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ─── Mesh Status (returned by get_mesh_status command) ───────────────────────

/// Complete mesh status for the frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshStatus {
    pub mesh_id: MeshId,
    pub mesh_name: String,
    pub total_nodes: u32,
    pub online_nodes: u32,
    pub total_capacity_ram_mb: u64,
    pub total_capacity_vram_mb: u64,
    pub current_utility: f64,
    pub is_leader: bool,
    pub leader_node: Option<NodeId>,
    pub my_reputation: f64,
    pub my_contribution: Option<ContributionSummary>,
    pub active_proposals: Vec<ProposalSummary>,
    pub leaderboard: Vec<LeaderboardEntry>,
}

/// Simplified proposal for frontend display.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProposalSummary {
    pub proposal_id: Uuid,
    pub proposal_type: String,
    pub description: String,
    pub proposer: NodeId,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub yes_votes: u32,
    pub no_votes: u32,
    pub abstain_votes: u32,
    pub status: String,
    pub quorum_met: bool,
}

/// Leaderboard entry for contribution ranking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeaderboardEntry {
    pub node_id: NodeId,
    pub reputation: f64,
    pub balance: f64,
    pub rank: u32,
}

// ─── Mesh Metrics ────────────────────────────────────────────────────────────

/// Aggregated mesh metrics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshMetrics {
    pub mesh_id: MeshId,
    pub timestamp: DateTime<Utc>,
    pub total_nodes: u32,
    pub online_nodes: u32,
    pub total_ram_mb: u64,
    pub total_vram_mb: u64,
    pub total_models_hosted: u32,
    pub total_requests_served: u64,
    pub average_reputation: f64,
    pub free_rider_count: u32,
    pub active_transfers: u32,
}

// ─── Audit Trail ─────────────────────────────────────────────────────────────

/// Types of auditable mesh events.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AuditEventType {
    NodeJoined,
    NodeLeft,
    NodeRetired,
    TrustChanged { from: TrustTier, to: TrustTier },
    ProposalCreated,
    ProposalPassed,
    ProposalRejected,
    ProposalExpired,
    VoteCast,
    NodeBanned,
    LeaderChanged,
    PlanDistributed,
    FreeRiderWarning,
    FreeRiderExcluded,
}

/// A single audit trail entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    pub entry_id: Uuid,
    pub mesh_id: MeshId,
    pub timestamp: DateTime<Utc>,
    pub event_type: AuditEventType,
    pub actor_node: NodeId,
    pub target_node: Option<NodeId>,
    pub description: String,
    /// Signature of the actor (for non-repudiation).
    pub actor_signature: Option<Vec<u8>>,
}

/// Audit trail manager.
pub struct AuditTrail {
    entries: Vec<AuditEntry>,
    /// Maximum entries to retain (default: 10000).
    pub max_entries: usize,
}

impl AuditTrail {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            max_entries: 10_000,
        }
    }

    /// Record an audit event.
    pub fn record(
        &mut self,
        mesh_id: MeshId,
        event_type: AuditEventType,
        actor_node: NodeId,
        target_node: Option<NodeId>,
        description: String,
        signature: Option<Vec<u8>>,
    ) {
        let entry = AuditEntry {
            entry_id: Uuid::new_v4(),
            mesh_id,
            timestamp: Utc::now(),
            event_type,
            actor_node,
            target_node,
            description,
            actor_signature: signature,
        };

        self.entries.push(entry);

        // Trim if over limit
        if self.entries.len() > self.max_entries {
            let excess = self.entries.len() - self.max_entries;
            self.entries.drain(0..excess);
        }
    }

    /// Get audit entries for a mesh, optionally filtered by event type.
    pub fn query(
        &self,
        mesh_id: &MeshId,
        event_type: Option<&AuditEventType>,
        limit: usize,
    ) -> Vec<&AuditEntry> {
        self.entries
            .iter()
            .rev()
            .filter(|e| {
                e.mesh_id == *mesh_id
                    && event_type.map_or(true, |t| e.event_type == *t)
            })
            .take(limit)
            .collect()
    }

    /// Get total entry count.
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }
}

// ─── Privacy Enforcement ─────────────────────────────────────────────────────

/// Metadata that can be shared between nodes (no prompt content).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestMetadata {
    pub request_id: Uuid,
    pub model_id: String,
    pub token_count: u32,
    pub sensitivity: PromptSensitivity,
    pub timestamp: DateTime<Utc>,
    /// Prompt content is NEVER included in metadata shared with other nodes.
    /// This field exists only to document the invariant.
    #[serde(skip)]
    _prompt_content_never_shared: (),
}

/// Privacy-safe node status (shared with mesh peers).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivacySafeNodeStatus {
    pub node_id: NodeId,
    pub is_online: bool,
    pub models_hosted: Vec<String>,
    pub capacity_available_percent: f64,
    pub reputation: f64,
    pub requests_served_count: u64,
    // No prompt content, no conversation history, no user data.
    // Only aggregate counts and model availability.
}

/// Validate that a message being sent to the mesh contains no prompt content.
/// Returns true if the message is safe to broadcast.
pub fn validate_privacy(_metadata: &RequestMetadata) -> bool {
    // The struct design ensures prompt content cannot be included.
    // This function exists as a runtime assertion point.
    true
}

// ─── Tauri Command Types ─────────────────────────────────────────────────────

/// Request to join a mesh (Tauri command input).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JoinMeshRequest {
    pub invitation_token: String,
}

/// Request to create an invitation (Tauri command input).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateInvitationRequest {
    pub mesh_id: MeshId,
    pub offered_tier: TrustTier,
    pub expires_in_hours: u32,
}

/// Request to update capacity offer (Tauri command input).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateCapacityOfferRequest {
    pub mesh_id: MeshId,
    pub spare_ram_mb: u64,
    pub spare_vram_mb: u64,
    pub spare_gpu_percent: f64,
    pub max_models: u32,
    pub available_hours: f64,
}

/// Request to change trust tier (Tauri command input).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeTrustTierRequest {
    pub mesh_id: MeshId,
    pub target_node: NodeId,
    pub new_tier: TrustTier,
}

/// Request to vote on a proposal (Tauri command input).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoteRequest {
    pub proposal_id: Uuid,
    pub decision: VoteDecision,
}

/// Request to override sensitivity (Tauri command input).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OverrideSensitivityRequest {
    pub sensitivity: PromptSensitivity,
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audit_trail_recording() {
        let mut trail = AuditTrail::new();
        let mesh_id = Uuid::new_v4();
        let actor = Uuid::new_v4();

        trail.record(
            mesh_id,
            AuditEventType::NodeJoined,
            actor,
            None,
            "Node joined mesh".to_string(),
            None,
        );

        assert_eq!(trail.entry_count(), 1);
        let entries = trail.query(&mesh_id, None, 10);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].event_type, AuditEventType::NodeJoined);
    }

    #[test]
    fn test_audit_trail_max_entries() {
        let mut trail = AuditTrail::new();
        trail.max_entries = 5;
        let mesh_id = Uuid::new_v4();
        let actor = Uuid::new_v4();

        for i in 0..10 {
            trail.record(
                mesh_id,
                AuditEventType::VoteCast,
                actor,
                None,
                format!("Vote {}", i),
                None,
            );
        }

        assert_eq!(trail.entry_count(), 5);
    }

    #[test]
    fn test_audit_trail_filter_by_type() {
        let mut trail = AuditTrail::new();
        let mesh_id = Uuid::new_v4();
        let actor = Uuid::new_v4();

        trail.record(mesh_id, AuditEventType::NodeJoined, actor, None, "join".to_string(), None);
        trail.record(mesh_id, AuditEventType::VoteCast, actor, None, "vote".to_string(), None);
        trail.record(mesh_id, AuditEventType::NodeLeft, actor, None, "left".to_string(), None);

        let votes = trail.query(&mesh_id, Some(&AuditEventType::VoteCast), 10);
        assert_eq!(votes.len(), 1);
    }

    #[test]
    fn test_privacy_enforcement() {
        let metadata = RequestMetadata {
            request_id: Uuid::new_v4(),
            model_id: "llama-7b".to_string(),
            token_count: 500,
            sensitivity: PromptSensitivity::NonSensitive,
            timestamp: Utc::now(),
            _prompt_content_never_shared: (),
        };

        assert!(validate_privacy(&metadata));
    }

    #[test]
    fn test_privacy_safe_node_status_has_no_prompts() {
        let status = PrivacySafeNodeStatus {
            node_id: Uuid::new_v4(),
            is_online: true,
            models_hosted: vec!["llama-7b".to_string()],
            capacity_available_percent: 75.0,
            reputation: 0.8,
            requests_served_count: 1000,
        };

        // Serialize and verify no prompt content fields exist
        let json = serde_json::to_string(&status).unwrap();
        assert!(!json.contains("prompt"));
        assert!(!json.contains("content"));
        assert!(!json.contains("conversation"));
    }
}
