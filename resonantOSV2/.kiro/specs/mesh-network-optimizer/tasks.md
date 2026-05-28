# Tasks: Mesh Network Optimizer (Phase 9B)

## Task Instructions
- Test: Vitest 3.2 + fast-check (TS), proptest (Rust)
- No Rust toolchain reliably available — write correct code without compiling
- Depends on Phase 9A (reuses solver algorithm, node registry, model catalog)
- Depends on Phase 10 (Unified Mesh Transport) for inter-node communication

## Tasks

- [x] 1. Mesh Identity and Cryptography
  - [x] 1.1 Implement `src-tauri/src/mesh/identity.rs`: generate Ed25519 keypair on first run using `ed25519-dalek` crate, persist to encrypted local storage
  - [x] 1.2 Implement `MeshIdentity` struct with `sign(data)`, `verify(data, signature, public_key)` methods
  - [x] 1.3 Implement invitation token generation: `create_invitation(mesh_id, offered_tier, expires_in_hours) -> InvitationToken` with inviter signature
  - [x] 1.4 Implement invitation token validation: check expiry, verify signature, check single-use (consumed flag)
  - [x] 1.5 Implement invitation token encoding/decoding: base64url-safe string suitable for URLs and QR codes
  - [x] 1.6 Write property tests: sign/verify roundtrip always succeeds with correct key; verify always fails with wrong key; expired tokens always rejected; consumed tokens always rejected

- [x] 2. Mesh Membership
  - [x] 2.1 Implement `src-tauri/src/mesh/membership.rs`: `MeshMembership` manager with `join()`, `leave()`, `list_members()`, `list_my_meshes()`
  - [x] 2.2 Implement join flow: validate token → send JoinRequest → wait for JoinAccepted → store membership → start heartbeat
  - [x] 2.3 Implement leave flow: send LeaveNotification → stop heartbeat → remove membership → cleanup local state
  - [x] 2.4 Implement multi-mesh support: node can belong to multiple meshes simultaneously, each with independent state
  - [x] 2.5 Implement mesh heartbeat: send MeshHeartbeat every 60 seconds, detect departure after 5 minutes silence
  - [x] 2.6 Implement member list synchronization: on join, receive full member list; on member change, receive delta update
  - [x] 2.7 Implement node retirement (permanent disappearance): node requests full data removal — anonymize all accounting records (replace with "retired-node-XXXX"), delete reputation history, purge membership record. Requires acknowledgment from all tier-3 nodes. Irreversible.
  - [x] 2.8 Implement retirement propagation: broadcast retirement request to all tier-3 nodes, wait for all to confirm data removal, only then confirm retirement to the departing node
  - [x] 2.9 Write property tests: join with valid token always succeeds; join with expired/consumed token always fails; leave removes from all peer member lists; multi-mesh memberships are independent; retired node's data is fully anonymized in all replicas; retirement requires all tier-3 acknowledgments

- [x] 3. Trust Manager
  - [x] 3.1 Implement `src-tauri/src/mesh/trust.rs`: `TrustManager` with `get_tier()`, `change_tier()`, `can_route_to()`, `can_see_prompts()`
  - [x] 3.2 Implement trust tier enforcement: tier 3 = any prompt, tier 2 = non-sensitive only, tier 1 = routing only (no prompts)
  - [x] 3.3 Implement trust promotion: requires tier-3 owner action + reputation >= 0.7 + participation >= 7 days
  - [x] 3.4 Implement trust demotion: triggered by reputation drop below threshold, manual action by inviter, or consensus vote
  - [x] 3.5 Implement `can_route_to(node, sensitivity) -> bool`: core routing decision based on trust tier and prompt sensitivity
  - [x] 3.6 Write property tests: sensitive prompts NEVER routable to tier < 3; promotion requires all 3 conditions; demotion always succeeds for tier-3 owners

- [x] 4. Prompt Sensitivity Classifier
  - [x] 4.1 Implement `src-tauri/src/mesh/classifier.rs`: `SensitivityClassifier` with `classify(prompt, context, config) -> ClassificationResult`
  - [x] 4.2 Implement user-explicit classification: check conversation-level privacy flag
  - [x] 4.3 Implement keyword matching: case-insensitive search against configurable sensitive keyword list
  - [x] 4.4 Implement context-based classification: check persona, private repo patterns
  - [x] 4.5 Implement default policy: configurable (default: NonSensitive), applied when no other signals match
  - [x] 4.6 Implement user override: per-message override that bypasses all other classification
  - [x] 4.7 Write property tests: user-explicit always takes priority; keyword match always returns Sensitive; unclassified defaults to configured policy; every prompt gets classified (no None results)

- [x] 5. Network Accounting
  - [x] 5.1 Implement `src-tauri/src/mesh/accounting.rs`: `AccountingLedger` with append-only storage, dual-signature records
  - [x] 5.2 Implement contribution recording: after each inference served for another node, create `AccountingRecord` signed by both parties
  - [x] 5.3 Implement consumption recording: after each inference received from another node, co-sign the contributor's record
  - [x] 5.4 Implement balance computation: `compute_balance(node_id, mesh_id, period) -> ContributionSummary` with normalized scores
  - [x] 5.5 Implement ledger replication: broadcast new records to all tier-3 nodes for auditability
  - [x] 5.6 Implement accounting query API: per-node summary, per-period breakdown, mesh-wide leaderboard
  - [x] 5.7 Write property tests: dual-signed records cannot be forged (verify both signatures); balance computation is deterministic given same records; ledger is append-only (no deletions)

- [x] 6. Reputation System
  - [x] 6.1 Implement reputation computation: `compute_reputation(node_id, mesh_id) -> ReputationUpdate` from accounting balance
  - [x] 6.2 Implement reputation bounds: always in [0.0, 1.0], initial value 0.5, max change per cycle ±0.1
  - [x] 6.3 Implement reputation history: store per-cycle updates for trend analysis
  - [x] 6.4 Implement reputation-based rate limit adjustment: higher reputation = higher rate limit (up to 2x base)
  - [x] 6.5 Write property tests: reputation always in [0.0, 1.0]; change never exceeds ±0.1 per cycle; new nodes start at 0.5; positive balance increases reputation

- [x] 7. Incentive Enforcement
  - [x] 7.1 Implement `src-tauri/src/mesh/incentive.rs`: `IncentiveEnforcer` with free-rider detection and escalation ladder
  - [x] 7.2 Implement free-rider detection: flag nodes with negative balance for 3+ consecutive cycles
  - [x] 7.3 Implement escalation: cycles 1-2 = grace, cycle 3 = warning notification, cycles 4-5 = deprioritized routing, cycle 6+ = excluded from shared models
  - [x] 7.4 Implement recovery: 2 consecutive positive cycles restores full participation
  - [x] 7.5 Implement consumer-only exemption: designated nodes exempt from free-rider detection but get lowest priority
  - [x] 7.6 Write property tests: escalation follows exact ladder timing; recovery requires exactly 2 positive cycles; exempt nodes never flagged; excluded nodes don't receive shared model allocations

- [x] 8. Mesh Solver (extends Phase 9A)
  - [x] 8.1 Implement `src-tauri/src/mesh/solver.rs`: `MeshSolver` that wraps Phase 9A `GreedySolver` with additional constraints
  - [x] 8.2 Implement trust-aware model selection: filter candidates by minimum trust tier needed for their workload
  - [x] 8.3 Implement reputation-weighted placement scoring: high-reputation nodes get placement bonus
  - [x] 8.4 Implement cross-owner fairness constraint: no single owner's nodes host >60% of shared models
  - [x] 8.5 Implement capacity offer honoring: never allocate more than a node's local optimizer has offered
  - [x] 8.6 Implement mesh solver timeout: 5 seconds (vs 2s for local)
  - [x] 8.7 Implement plan acknowledgment: distribute plan to all nodes, collect accept/reject responses, re-solve on rejection
  - [x] 8.8 Write property tests: trust routing invariant (sensitive never to tier < 3); fairness constraint enforced; capacity offers never exceeded; solver completes within 5s for 100 nodes

- [x] 9. Consensus Protocol
  - [x] 9.1 Implement `src-tauri/src/mesh/consensus.rs`: `ConsensusManager` with `create_proposal()`, `cast_vote()`, `check_outcome()`
  - [x] 9.2 Implement proposal types: AddModel, RemoveModel, BanNode, ConfigChange, TrustChange
  - [x] 9.3 Implement voting eligibility: only tier-3 nodes can vote
  - [x] 9.4 Implement quorum check: >50% of eligible voters must participate
  - [x] 9.5 Implement approval threshold: >66% yes votes for normal proposals, >50% for emergency (BanNode)
  - [x] 9.6 Implement vote timeout: 24 hours normal, 1 hour emergency
  - [x] 9.7 Implement proposal execution: on pass, execute the proposed action automatically
  - [x] 9.8 Write property tests: proposals without quorum never pass; proposals below threshold never pass; expired proposals marked as expired; emergency proposals use correct threshold

- [x] 10. Rate Limiting
  - [x] 10.1 Implement `src-tauri/src/mesh/rate_limiter.rs`: `RateLimiter` with per-node and aggregate limits
  - [x] 10.2 Implement per-node rate tracking: requests per minute with minute-boundary reset
  - [x] 10.3 Implement burst allowance: 2x limit for 30 seconds, then strict enforcement
  - [x] 10.4 Implement reputation-adjusted limits: effective_limit = base * (1 + (reputation - 0.5) * bonus_multiplier)
  - [x] 10.5 Implement concurrent request limit: max 5 simultaneous requests per node
  - [x] 10.6 Implement anomaly detection: alert if node sends 10x its historical average rate
  - [x] 10.7 Implement token-weighted rate limiting: track tokens_this_minute (default max: 30,000) and compute_seconds_this_minute (default max: 45s). Reject if ANY limit exceeded. Prevents resource exhaustion via few large requests.
  - [x] 10.8 Write property tests: requests beyond limit always rejected after burst window; reputation 1.0 gets 2x limit; concurrent limit enforced; anomaly detected at 10x rate; 10 requests of 4000 tokens each hits token budget before request count limit

- [x] 11. Cross-Network Model Transfer
  - [x] 11.1 Implement `src-tauri/src/mesh/transfer.rs`: `MeshTransferCoordinator` for model transfers between mesh peers
  - [x] 11.2 Implement peer discovery for transfers: find mesh nodes that have the target model downloaded
  - [x] 11.3 Implement single-source transfer: encrypted stream from one peer to target with bandwidth limiting (30% max)
  - [x] 11.4 Implement parallel chunk transfer: for large models (>2GB), fetch different 64MB chunks from multiple peers simultaneously
  - [x] 11.5 Implement transfer accounting: record bandwidth contribution/consumption for both parties
  - [x] 11.6 Implement SHA-256 verification after transfer completion
  - [x] 11.7 Write property tests: transfer bandwidth never exceeds 30% of link; integrity check catches corruption; accounting records created for both parties

- [x] 12. Leader Election and Mesh Lifecycle
  - [x] 12.1 Implement deterministic leader election: highest reputation + longest uptime among tier-3 nodes (no consensus needed — all compute same result)
  - [x] 12.2 Implement leader failure detection: if leader misses 2 heartbeats (10 min), next-highest takes over
  - [x] 12.3 Implement mesh optimizer loop on leader: solve every 15 minutes, distribute plan, collect acknowledgments
  - [x] 12.4 Implement local-mesh interface: local optimizer exports CapacityOffer and DemandRequest, mesh optimizer consumes them
  - [x] 12.5 Write property tests: leader election is deterministic (same inputs = same leader); leader failover completes within 10 minutes; local optimizer rejection is final

- [x] 13. Mesh Observability and Tauri Commands
  - [x] 13.1 Implement Tauri commands: `join_mesh`, `create_invitation`, `get_mesh_status`, `get_my_accounting`, `update_capacity_offer`, `change_trust_tier`, `vote_on_proposal`, `leave_mesh`, `override_sensitivity`
  - [x] 13.2 Implement mesh metrics: total nodes, capacity, utility, per-node reputation, contribution leaderboard
  - [x] 13.3 Implement audit trail: log all mesh decisions (votes, bans, trust changes) with timestamps and signatures
  - [x] 13.4 Implement privacy enforcement: no node can see another's prompt content — only metadata

- [x] 14. End-to-End Integration Tests
  - [x] 14.1 Test: mesh join flow — create invitation, join, verify membership and heartbeat
  - [x] 14.2 Test: trust routing — sensitive prompt stays local, non-sensitive routes to tier-2 node
  - [x] 14.3 Test: free-rider detection — simulate 6 cycles negative balance, verify exclusion
  - [x] 14.4 Test: leader failover — kill leader, verify new leader takes over and produces plan
  - [x] 14.5 Test: consensus vote — create proposal, collect votes, verify outcome
  - [x] 14.6 Test: mesh independence — kill mesh service, verify local optimizer unaffected
