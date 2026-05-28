# Tasks: Mesh Compute Network

## Phase 1: Mesh Agent and Identity

- [ ] 1.1 Create `src-tauri/src/mesh_agent.rs` with struct definitions: MeshAgentConfig, NetworkIdentity, ContributionMode, MeshNodeState
- [ ] 1.2 Implement Network Identity management: use Reticulum identity (Phase 6) as mesh identity, announce to network, handle identity verification
- [ ] 1.3 Implement contribution mode configuration: "full" (all idle resources), "partial" (configurable percentage), "idle-only" (contribute only when no local workloads), "off" (consume only)
- [ ] 1.4 Implement status reporting to coordinator: hardware profile, current utilization, loaded models, thermal state, contribution mode — via Reticulum TCP
- [ ] 1.5 Implement workload execution for network requests: receive encrypted payload, decrypt with local key, execute inference, encrypt response, return with attestation
- [ ] 1.6 Implement local priority enforcement: always preempt network workloads for local interactive requests, resume network work after local completes
- [ ] 1.7 Write unit tests for identity management, contribution modes, local priority enforcement

## Phase 2: Network Manager Core

- [ ] 2.1 Create `src-tauri/src/mesh_network_manager.rs` with struct definitions: NetworkState, ModelTier, ContributionScore, InferenceRequest, RequestPriority, ComputeAttestation, ScalingDecision, QoSMetrics
- [ ] 2.2 Implement capacity pool computation: sum all online nodes' Compute_Units weighted by uptime probability
- [ ] 2.3 Implement demand tracking: record inference requests per second, compute trailing 5-minute average, detect trends
- [ ] 2.4 Implement contribution score calculation: (hours_contributed × hardware_quality) / hours_consumed, with 7-day grace period for new users
- [ ] 2.5 Implement fair share quota enforcement: compute per-user quota proportional to contribution score, deprioritize (don't reject) over-quota users
- [ ] 2.6 Implement request routing: evaluate priority, quota, model availability, node proximity, current load — select best serving node
- [ ] 2.7 Register IPC commands: mesh_get_network_state, mesh_get_contribution, mesh_submit_inference, mesh_get_qos_metrics, mesh_withdraw, mesh_set_contribution_mode, mesh_join_network
- [ ] 2.8 Write property-based tests (proptest) for Properties 1, 3, 6: reserve buffer, fair share proportionality, local priority

## Phase 3: Dynamic Model Scaling

- [ ] 3.1 Create `training/mesh_scaling/` directory with scaling_engine.py
- [ ] 3.2 Implement demand history recording: store demand observations with timestamps, maintain 1-week rolling window
- [ ] 3.3 Implement demand forecasting: time-series prediction using day-of-week and hour-of-day patterns (simple seasonal decomposition)
- [ ] 3.4 Implement scaling recommendation: map demand/capacity ratio to model tier (heavy < 50%, medium 50-80%, light > 80%), use forecast to anticipate transitions
- [ ] 3.5 Implement scaling execution: coordinate model unload/load across nodes, respect 2-minute downscale and 5-minute upscale deadlines
- [ ] 3.6 Implement scaling safety: never scale during active interactive requests, maintain at least one instance of user's guaranteed tier
- [ ] 3.7 Implement adaptive fractional reserve ratio: compute from observed peak-concurrent / registered-users ratio over 30-day window
- [ ] 3.8 Write property-based tests (hypothesis) for Property 2: scaling threshold correctness

## Phase 4: Compute Attestation

- [ ] 4.1 Implement attestation generation: after inference completion, compute SHA-256 of (request_payload + response_payload), sign with Reticulum identity, include model_id and duration
- [ ] 4.2 Implement attestation verification: requester checks signature validity, hash matches received response, duration is plausible for model size
- [ ] 4.3 Implement anomaly detection: flag responses too fast (< minimum possible for model), flag duplicate attestations, flag mismatched hashes
- [ ] 4.4 Implement suspension mechanism: after 3 consecutive invalid attestations, suspend node from receiving workloads, notify coordinators
- [ ] 4.5 Implement contribution credit: only attested computations count toward Contribution_Score, unattested work earns zero credit
- [ ] 4.6 Write property-based tests (proptest) for Property 4: attestation verification correctness

## Phase 5: Distributed Coordination

- [ ] 5.1 Implement coordinator election: score nodes by (uptime_95pct × contribution_top20pct × hardware_sufficient), elect top 3 (or all nodes if < 5 total)
- [ ] 5.2 Implement coordinator consensus: scaling decisions, quota enforcement, identity registration require majority agreement among coordinators
- [ ] 5.3 Implement coordinator rotation: monthly rotation to distribute workload, re-election on coordinator failure
- [ ] 5.4 Implement network partition handling: each partition operates independently with local coordinator(s), merge state on reconnection
- [ ] 5.5 Implement coordination overhead monitoring: track CU spent on coordination, alert if > 5% of total capacity
- [ ] 5.6 Write integration tests: coordinator election with varying node counts, partition/merge scenarios, rotation

## Phase 6: Security and Privacy

- [ ] 6.1 Implement E2E encryption for inference payloads: requester encrypts with serving node's Reticulum public key, only serving node can decrypt
- [ ] 6.2 Implement ephemeral data policy: serving nodes delete request/response data immediately after attestation generation
- [ ] 6.3 Implement workload isolation: network inference runs in sandboxed context with no access to serving user's local files, credentials, or conversations
- [ ] 6.4 Implement rate limiting: max 100 requests/minute per Network_Identity, configurable by coordinators
- [ ] 6.5 Implement Sybil detection: hardware fingerprinting (HardwareProfile hash), contribution pattern analysis, flag identities with identical hardware profiles
- [ ] 6.6 Implement network blocklist: coordinator consensus to block malicious identities, propagate to all nodes within 60 seconds
- [ ] 6.7 Implement traffic indistinguishability: pad all compute packets to standardized size buckets (512B, 2KB, 8KB, 32KB) so packet size doesn't reveal message type
- [ ] 6.8 Implement protocol steganography: ensure no unencrypted headers or markers distinguish compute traffic from LXMF chat — all traffic uses identical Reticulum packet format
- [ ] 6.9 Implement abstract capability sharing: share only capability classes ("gpu-heavy", "gpu-medium", "cpu-large", "cpu-small") to mesh — never exact hardware specs that could fingerprint
- [ ] 6.10 Implement attestation duration quantization: bucket durations to (< 1s, 1-5s, 5-30s, > 30s) to prevent prompt length inference from timing
- [ ] 6.11 Implement contribution aggregation: aggregate scores over 24h windows, never expose per-request contribution data to the network
- [ ] 6.12 Implement routing decision privacy: routing metadata (who requested what tier) visible only to requester + assigned coordinator, never broadcast
- [ ] 6.13 Write property-based tests (proptest) for Property 7: E2E encryption verification, plus new tests for traffic indistinguishability and metadata minimization

## Phase 6b: Reticulum Path-Aware Routing

- [ ] 6b.1 Implement Reticulum path query interface: wrap RNS.Transport.hops_to(), link establishment rate, path freshness into a Rust-accessible API via the sidecar JSON-RPC protocol
- [ ] 6b.2 Add JSON-RPC methods to Phase 6 sidecar: "get_path_quality" (returns hops, link_rate, freshness for a destination), "get_known_paths" (returns all known destinations with quality metrics)
- [ ] 6b.3 Implement unified routing score: combine (model_availability × 0.4 + path_quality × 0.3 + node_load_inverse × 0.3) into a single placement score per candidate node
- [ ] 6b.4 Replace separate latency probes with Reticulum path data: remove Phase 11's active latency pinging for mesh nodes, use Reticulum's native path quality instead
- [ ] 6b.5 Implement path-quality-aware model placement: when multiple nodes offer same model, prefer the node with fewest Reticulum hops and highest link establishment rate
- [ ] 6b.6 Write integration tests: routing prefers closer nodes, path quality degrades → routing shifts to alternative node, Reticulum path data matches actual latency

## Phase 7: User Sovereignty and QoS

- [ ] 7.1 Implement instant withdrawal: stop accepting network workloads, transfer in-flight to other nodes within 30s, remove from registry, reclaim all local resources
- [ ] 7.2 Implement partial withdrawal: reduce contributed percentage, update capacity pool, maintain network membership
- [ ] 7.3 Implement contribution score preservation: retain score for 90 days after withdrawal, restore on rejoin
- [ ] 7.4 Implement QoS tracking: per-user latency percentiles, quality tier received, SLA violation counting
- [ ] 7.5 Implement QoS guarantees: interactive requests from within-quota users get < 5s TTFT when demand < 80%, tier guarantee based on contribution level
- [ ] 7.6 Implement QoS degradation notification: when guarantees can't be met, notify user with wait time estimate and local fallback option
- [ ] 7.7 Write property-based tests (proptest) for Properties 5, 8: withdrawal immediacy, QoS latency guarantee

## Phase 8: Network Joining and Capacity Reporting

- [ ] 8.1 Implement network join flow: generate Reticulum identity, announce to mesh, request confirmation from coordinator (invitation model)
- [ ] 8.2 Implement invitation system: existing users can generate invite tokens, new users present token to join
- [ ] 8.3 Implement capacity reporting: human-readable "N users at X quality" metric, real-time updates, per-user contribution display
- [ ] 8.4 Implement demand forecasting display: predicted capacity needs for next 24h, alerts when demand expected to exceed capacity
- [ ] 8.5 Implement Cost Dashboard integration: add "Mesh Network" section with contribution, consumption, QoS metrics, scaling state
- [ ] 8.6 Create `src/core/mesh-network.ts` with typed IPC wrappers for all mesh commands

## Phase 9: Behavioral Contracts and Integration

- [ ] 9.1 Create behavioral contract JSON files: contract-mesh-reserve-buffer, contract-mesh-scaling-thresholds, contract-mesh-fair-share, contract-mesh-attestation-verified
- [ ] 9.2 Create behavioral contract JSON files: contract-mesh-withdrawal-30s, contract-mesh-local-priority, contract-mesh-e2e-encrypted, contract-mesh-qos-latency
- [ ] 9.3 Implement graceful degradation: mesh unavailable → local-only with zero impact, quota exhausted → local fallback, serving node failure → retry/local within 5s
- [ ] 9.4 Write integration tests: full flow (join → contribute → request → serve → attest → credit), scaling transition under load, withdrawal mid-workload, partition handling
- [ ] 9.5 Write performance tests: routing decision < 50ms, attestation overhead < 5ms, coordinator consensus < 1s for scaling decisions
