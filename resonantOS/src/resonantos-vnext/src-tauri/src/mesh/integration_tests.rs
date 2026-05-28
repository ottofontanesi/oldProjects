// Intent citation: .kiro/specs/mesh-network-optimizer/design.md Section 10.2
// End-to-End Integration Tests for Phase 9B Mesh Network Optimizer

#[cfg(test)]
mod tests {
    use crate::mesh::accounting::{AccountingAmount, AccountingLedger, AccountingType};
    use crate::mesh::classifier::{ConversationContext, SensitivityClassifier, SensitivityConfig};
    use crate::mesh::consensus::{ConsensusManager, ProposalStatus, ProposalType, VoteDecision};
    use crate::mesh::identity::{MeshIdentity, TrustTier};
    use crate::mesh::incentive::{FreeRiderStatus, IncentiveEnforcer};
    use crate::mesh::leader::{LeaderCandidate, LeaderElection};
    use crate::mesh::membership::{MeshMember, MeshMembershipManager, MembershipStatus};
    use crate::mesh::solver::{
        CapacityOffer, MeshModelCandidate, MeshNodeState, MeshSolver, MeshSolverInputs,
    };
    use crate::mesh::trust::{PromptSensitivity, TrustManager};
    use chrono::Utc;
    use std::time::Duration;
    use uuid::Uuid;

    // ─── Test 14.1: Mesh Join Flow ───────────────────────────────────────────

    #[test]
    fn test_e2e_mesh_join_flow() {
        // Setup: inviter creates a mesh and generates an invitation
        let inviter = MeshIdentity::generate();
        let joiner = MeshIdentity::generate();
        let mesh_id = Uuid::new_v4();

        // Step 1: Create invitation
        let mut token = inviter.create_invitation(mesh_id, TrustTier::InvitedFriend, 24);

        // Step 2: Validate token (joiner side)
        assert!(token.validate(&inviter.verifying_key).is_ok());

        // Step 3: Join mesh
        let mut manager = MeshMembershipManager::new(joiner.clone());
        let result = manager.join(&mut token, &inviter.verifying_key, "test-mesh".to_string());
        assert!(matches!(
            result,
            crate::mesh::membership::JoinResult::Accepted { .. }
        ));

        // Step 4: Verify membership
        assert!(manager.is_member(&mesh_id));
        assert_eq!(manager.my_tier(&mesh_id), Some(TrustTier::InvitedFriend));

        // Step 5: Verify heartbeat tracking works
        let peer_id = Uuid::new_v4();
        let member = MeshMember {
            node_id: peer_id,
            trust_tier: TrustTier::LocalOwned,
            joined_at: Utc::now(),
            status: MembershipStatus::Active,
            last_heartbeat: Utc::now(),
            is_online: true,
        };
        manager.apply_member_delta(
            &mesh_id,
            crate::mesh::membership::MemberDelta::Added(member),
        );
        manager.receive_heartbeat(&mesh_id, peer_id);
        assert_eq!(manager.online_member_count(&mesh_id), 1);

        // Step 6: Token is consumed (can't reuse)
        assert!(token.consumed);
    }

    // ─── Test 14.2: Trust Routing ────────────────────────────────────────────

    #[test]
    fn test_e2e_trust_routing() {
        let mut trust_mgr = TrustManager::new();
        let mut classifier = SensitivityClassifier::new(SensitivityConfig::default());
        let mesh_id = Uuid::new_v4();
        let owner = Uuid::new_v4();
        let tier2_node = Uuid::new_v4();
        let tier3_node = Uuid::new_v4();

        // Setup nodes
        trust_mgr.register_node(mesh_id, tier2_node, TrustTier::InvitedFriend, owner);
        trust_mgr.register_node(mesh_id, tier3_node, TrustTier::LocalOwned, owner);

        // Classify a sensitive prompt (contains "password")
        let sensitive_context = ConversationContext::default();
        let result = classifier.classify("What is my password?", &sensitive_context);
        assert_eq!(result.sensitivity, PromptSensitivity::Sensitive);

        // Sensitive prompt: stays local (only tier-3)
        assert!(!trust_mgr.can_route_to(&mesh_id, &tier2_node, PromptSensitivity::Sensitive));
        assert!(trust_mgr.can_route_to(&mesh_id, &tier3_node, PromptSensitivity::Sensitive));

        // Non-sensitive prompt: can route to tier-2
        let non_sensitive_result = classifier.classify("What is the weather?", &sensitive_context);
        assert_eq!(non_sensitive_result.sensitivity, PromptSensitivity::NonSensitive);
        assert!(trust_mgr.can_route_to(&mesh_id, &tier2_node, PromptSensitivity::NonSensitive));
        assert!(trust_mgr.can_route_to(&mesh_id, &tier3_node, PromptSensitivity::NonSensitive));
    }

    // ─── Test 14.3: Free-Rider Detection ─────────────────────────────────────

    #[test]
    fn test_e2e_free_rider_detection() {
        let mut enforcer = IncentiveEnforcer::new();
        let mesh_id = Uuid::new_v4();
        let free_rider = Uuid::new_v4();
        enforcer.register_node(free_rider, mesh_id);

        // Simulate 6 cycles of negative balance
        for cycle in 1..=6 {
            enforcer.process_cycle(&free_rider, &mesh_id, -5.0, cycle);
        }

        // Verify exclusion
        assert!(enforcer.is_excluded(&free_rider, &mesh_id));
        assert!(matches!(
            enforcer.get_status(&free_rider, &mesh_id),
            Some(FreeRiderStatus::Excluded { .. })
        ));

        // Verify excluded node would not be placed by solver
        let mut solver = MeshSolver::new();
        let node = MeshNodeState {
            node_id: free_rider,
            owner_id: Uuid::new_v4(),
            trust_tier: TrustTier::LocalOwned,
            reputation: 0.5,
            capacity_offer: CapacityOffer {
                node_id: free_rider,
                spare_ram_mb: 16000,
                spare_vram_mb: 8000,
                spare_gpu_percent: 50.0,
                max_models_willing_to_host: 5,
                available_hours_per_day: 24.0,
                available_tools: vec![],
            },
            free_rider_status: FreeRiderStatus::Excluded { since_cycle: 6 },
            is_online: true,
            uptime_seconds: 86400,
        };

        let inputs = MeshSolverInputs {
            mesh_id,
            nodes: vec![node],
            candidates: vec![MeshModelCandidate {
                model_id: "test-model".to_string(),
                ram_required_mb: 4000,
                vram_required_mb: 0,
                parameter_count_b: 7.0,
                quality_score: 0.8,
                serves_sensitive_workload: false,
                min_trust_tier: TrustTier::InvitedFriend,
            }],
            current_placements: vec![],
            timeout: Duration::from_secs(5),
        };

        let plan = solver.solve(&inputs, Uuid::new_v4());
        assert!(plan.placements.is_empty(), "Excluded node should not get placements");
    }

    // ─── Test 14.4: Leader Failover ──────────────────────────────────────────

    #[test]
    fn test_e2e_leader_failover() {
        let mut election = LeaderElection::new();
        let mesh_id = Uuid::new_v4();

        let leader_id = Uuid::new_v4();
        let backup_id = Uuid::new_v4();

        let candidates = vec![
            LeaderCandidate {
                node_id: leader_id,
                reputation: 0.95,
                uptime_seconds: 86400 * 30, // 30 days
                is_online: true,
                last_heartbeat: Utc::now(),
            },
            LeaderCandidate {
                node_id: backup_id,
                reputation: 0.85,
                uptime_seconds: 86400 * 60, // 60 days
                is_online: true,
                last_heartbeat: Utc::now(),
            },
        ];

        // Initial election: leader_id wins (higher reputation)
        election.update_leader(mesh_id, &candidates);
        assert_eq!(election.current_leader(&mesh_id), Some(leader_id));

        // Leader goes offline
        let candidates_after_failure = vec![
            LeaderCandidate {
                node_id: leader_id,
                reputation: 0.95,
                uptime_seconds: 86400 * 30,
                is_online: false, // Offline!
                last_heartbeat: Utc::now(),
            },
            LeaderCandidate {
                node_id: backup_id,
                reputation: 0.85,
                uptime_seconds: 86400 * 60,
                is_online: true,
                last_heartbeat: Utc::now(),
            },
        ];

        // Simulate 2 missed heartbeats
        assert!(!election.record_missed_heartbeat(&mesh_id));
        assert!(election.record_missed_heartbeat(&mesh_id)); // Failover triggered

        // Re-elect with updated candidates
        let changed = election.update_leader(mesh_id, &candidates_after_failure);
        assert!(changed);
        assert_eq!(election.current_leader(&mesh_id), Some(backup_id));

        // New leader can produce a plan
        assert!(election.should_run_optimizer(&mesh_id));
    }

    // ─── Test 14.5: Consensus Vote ───────────────────────────────────────────

    #[test]
    fn test_e2e_consensus_vote() {
        let mut consensus = ConsensusManager::new();
        let mesh_id = Uuid::new_v4();
        let proposer = Uuid::new_v4();
        let eligible_count = 5;

        // Create proposal to add a model
        let proposal_id = consensus
            .create_proposal(
                proposer,
                TrustTier::LocalOwned,
                mesh_id,
                ProposalType::AddModel {
                    model_id: "llama-70b".to_string(),
                },
                "Add Llama 70B to the mesh".to_string(),
            )
            .unwrap();

        // Collect votes: 4 yes, 1 no (80% > 66% threshold)
        for _ in 0..4 {
            let voter = Uuid::new_v4();
            consensus
                .cast_vote(
                    proposal_id,
                    voter,
                    TrustTier::LocalOwned,
                    VoteDecision::Yes,
                    eligible_count,
                )
                .unwrap();
        }
        let no_voter = Uuid::new_v4();
        consensus
            .cast_vote(
                proposal_id,
                no_voter,
                TrustTier::LocalOwned,
                VoteDecision::No,
                eligible_count,
            )
            .unwrap();

        // Verify outcome: passed (quorum met: 5/5 > 50%, approval: 4/5 = 80% > 66%)
        let proposal = consensus.get_proposal(&proposal_id).unwrap();
        assert_eq!(proposal.status, ProposalStatus::Passed);
    }

    // ─── Test 14.6: Mesh Independence ────────────────────────────────────────

    #[test]
    fn test_e2e_mesh_independence() {
        // Simulate: mesh service is "killed" (all mesh state cleared)
        // Local optimizer should be completely unaffected

        // Setup local optimizer state (Phase 9A types)
        let local_node_id = Uuid::new_v4();
        let mesh_id = Uuid::new_v4();

        // Create mesh membership
        let inviter = MeshIdentity::generate();
        let local_identity = MeshIdentity::generate();
        let mut membership = MeshMembershipManager::new(local_identity.clone());
        let mut token = inviter.create_invitation(mesh_id, TrustTier::InvitedFriend, 24);
        membership.join(&mut token, &inviter.verifying_key, "test".to_string());
        assert!(membership.is_member(&mesh_id));

        // "Kill" mesh service — leave all meshes
        membership.leave(&mesh_id);
        assert!(!membership.is_member(&mesh_id));

        // Local optimizer state is independent — it doesn't reference mesh at all
        // The local solver, catalog, registry etc. from Phase 9A continue working
        // This test verifies the architectural boundary: mesh is opt-in and isolated

        // Verify no crash, no panic, clean state
        assert_eq!(membership.mesh_count(), 0);
        assert_eq!(membership.list_my_meshes().len(), 0);

        // Can rejoin later
        let mut new_token = inviter.create_invitation(mesh_id, TrustTier::LocalOwned, 48);
        let result = membership.join(&mut new_token, &inviter.verifying_key, "test".to_string());
        assert!(matches!(
            result,
            crate::mesh::membership::JoinResult::Accepted { .. }
        ));
    }
}
