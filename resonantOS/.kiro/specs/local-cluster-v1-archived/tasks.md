# Tasks: Local Cluster

## Phase 1: Cluster Orchestrator Foundation

- [ ] 1.1 Create `src-tauri/src/cluster_orchestrator.rs` with struct definitions: ClusterNode, NodeHealth, WorkloadRequest, PlacementDecision, PlacementStrategy, NodeAffinity, ClusterCapacity, ModelInstance
- [ ] 1.2 Implement Node Registry: in-memory store of ClusterNode records with persistence to `cluster_state.db`, CRUD operations, status transitions
- [ ] 1.3 Implement Model Registry: cluster-wide index of ModelInstance records, updated on agent status reports, queryable by model_id or node_id
- [ ] 1.4 Implement Workload Queue: priority queue for pending placements, timeout-based expiry, retry logic
- [ ] 1.5 Implement `place_workload` function: evaluate all ready nodes against WorkloadRequest requirements, apply PlacementStrategy, respect NodeAffinity, prefer model-loaded nodes, complete within 10ms
- [ ] 1.6 Implement three placement strategies: BestFit (most capable node matching requirements), Spread (least-loaded node), Pack (most-loaded node with remaining capacity)
- [ ] 1.7 Register IPC commands: cluster_get_nodes, cluster_get_capacity, cluster_submit_workload, cluster_confirm_node, cluster_remove_node, cluster_set_strategy, cluster_set_affinity, cluster_load_model, cluster_unload_model
- [ ] 1.8 Write property-based tests (proptest) for Properties 1, 2, 3, 7: capability gates, model preference, placement speed, overcommit prevention

## Phase 2: Node Discovery (mDNS)

- [ ] 2.1 Implement mDNS announcement: advertise `_resonantos-cluster._tcp` service with TXT records (node_id, hardware_class, vram, ram, protocol_version)
- [ ] 2.2 Implement mDNS listener: continuously discover peers, extract service records, add to pending-confirmation list
- [ ] 2.3 Implement discovery grace period: wait 60s after mDNS TTL expires before transitioning node to offline
- [ ] 2.4 Implement manual node registration: accept IP:port input, probe for cluster agent, add to registry
- [ ] 2.5 Implement user confirmation gate: newly discovered nodes require explicit user approval before receiving workloads
- [ ] 2.6 Write integration tests: mDNS announcement/discovery round-trip, TTL expiry handling, manual registration

## Phase 3: Cluster Agent Service

- [ ] 3.1 Create `src-tauri/src/cluster_agent.rs` with agent configuration, workload executor, and status reporter
- [ ] 3.2 Implement status reporting: stream CPU/RAM/GPU utilization, loaded models, active workloads, thermal state to orchestrator every 10 seconds via gRPC streaming
- [ ] 3.3 Implement workload execution: receive WorkloadRequest, validate against local Resource_Envelope (Phase 7), execute using local Compute Fabric, return result
- [ ] 3.4 Implement model management: LoadModel (download from artifact store or peer transfer), UnloadModel (free VRAM/RAM), report changes to orchestrator
- [ ] 3.5 Implement resource limit enforcement: reject workloads that would exceed local envelope, return error with available capacity
- [ ] 3.6 Implement agent authentication: validate cluster secret on all incoming gRPC calls, reject unauthenticated requests
- [ ] 3.7 Write unit tests for workload execution, resource limit enforcement, authentication

## Phase 4: gRPC Communication Layer

- [ ] 4.1 Define `proto/cluster.proto` with all service methods: SubmitWorkload, CancelWorkload, LoadModel, UnloadModel, GetStatus, Ping, StreamStatus, TransferModel
- [ ] 4.2 Implement gRPC server in cluster agent using `tonic` crate with TLS configuration
- [ ] 4.3 Implement gRPC client in cluster orchestrator using `tonic` with connection pooling per node
- [ ] 4.4 Implement mutual TLS: generate cluster CA + per-node certificates during cluster setup, validate on both sides
- [ ] 4.5 Implement model transfer protocol: peer-to-peer streaming of model files via TransferModel RPC (chunked, resumable)
- [ ] 4.6 Implement connection health: detect dropped gRPC streams, trigger reconnection with backoff
- [ ] 4.7 Write property-based tests (proptest) for Property 6: authentication enforcement

## Phase 5: Fault Tolerance

- [ ] 5.1 Implement heartbeat monitoring: expect StatusUpdate stream from each agent every 10s, track last_heartbeat timestamp
- [ ] 5.2 Implement failure detection: transition node to "offline" after 30s without heartbeat (3 missed updates)
- [ ] 5.3 Implement workload reassignment: on node failure, requeue any pending (not started) workloads for that node to other available nodes
- [ ] 5.4 Implement in-flight failure handling: detect workload failure via stream disconnect, mark as failed, offer retry on another node
- [ ] 5.5 Implement node recovery: when offline node resumes heartbeats, refresh its HardwareProfile, transition to "ready", resume scheduling
- [ ] 5.6 Implement single-machine fallback: when all remote nodes offline, route all workloads to local execution with zero overhead
- [ ] 5.7 Write property-based tests (proptest) for Properties 4, 5: fault detection timing, single-machine fallback

## Phase 6: TypeScript Client and Dashboard Integration

- [ ] 6.1 Create `src/core/cluster.ts` with typed IPC wrappers for all orchestrator commands
- [ ] 6.2 Implement cluster status subscription: poll cluster_get_nodes and cluster_get_capacity at 5s intervals when dashboard visible
- [ ] 6.3 Integrate with Phase 1 Cost Dashboard: add "Cluster" section showing node count, capacity, utilization, workload distribution
- [ ] 6.4 Implement node management UI: list nodes with status badges, confirm/remove buttons, affinity configuration
- [ ] 6.5 Implement model distribution view: which models on which nodes, VRAM usage per model, load/unload actions
- [ ] 6.6 Write Vitest component tests for cluster dashboard rendering

## Phase 7: Behavioral Contracts and Integration

- [ ] 7.1 Create behavioral contract JSON files: contract-cluster-capability-gate, contract-cluster-model-preference, contract-cluster-placement-10ms, contract-cluster-fault-30s, contract-cluster-fallback-zero-overhead, contract-cluster-auth-enforced
- [ ] 7.2 Implement cluster secret generation: on first cluster setup, generate 256-bit secret, distribute to confirmed nodes via secure channel
- [ ] 7.3 Implement node revocation: remove node from registry, invalidate its certificate, prevent further communication
- [ ] 7.4 Write integration tests: full flow (discover → confirm → submit workload → execute → return result), fault tolerance (kill agent mid-workload → detect → reassign), model transfer between nodes
- [ ] 7.5 Write performance tests: placement decision < 10ms with 20 nodes, gRPC round-trip < 5ms on LAN, status stream overhead < 1% CPU

## Phase 8: Pipeline Parallelism (Advanced)

- [ ] 8.1 Implement bandwidth detection between node pairs: measure actual throughput via gRPC streaming probe (send 100MB, measure time), classify as "pipeline-capable" (>= 1 Gbps) or "whole-model-only"
- [ ] 8.2 Implement prefill/decode split scheduling: when a model doesn't fit on any single node but can be split, assign prefill to GPU node and decode to high-RAM node
- [ ] 8.3 Implement KV cache transfer: after prefill completes on Node A, serialize KV cache and stream to Node B via gRPC TransferModel-style chunked streaming
- [ ] 8.4 Implement decode continuation: Node B loads KV cache into memory, continues token generation using CPU inference (or second GPU)
- [ ] 8.5 Implement pipeline latency estimation: compute expected overhead (KV cache size × transfer time) and include in placement decision — only use pipeline when quality gain > latency cost
- [ ] 8.6 Implement pipeline restriction: only enable for batch/background by default, require explicit user opt-in for interactive, never enable across mesh (Phase 10)
- [ ] 8.7 Write integration tests: prefill on GPU node → KV transfer → decode on CPU node, latency measurement, bandwidth gating (reject pipeline on gigabit)
