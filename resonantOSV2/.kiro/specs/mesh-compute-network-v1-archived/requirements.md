# Requirements Document

## Introduction

Mesh Compute Network is Phase 10 of the ResonantOS vNext improvement plan. It extends the Local Cluster (Phase 9) across trust boundaries, enabling multiple users to pool their hardware into a shared inference mesh connected via TCP Reticulum (Phase 6). The system applies a fractional-reserve model to compute resources: since users rarely need their full hardware simultaneously, the network can serve more users than the raw hardware would suggest — similar to how banks operate on the principle that depositors won't all withdraw simultaneously.

The Mesh Compute Network introduces a Network Manager that dynamically scales model instances based on demand. When demand is low (users < capacity), the network runs fewer instances of larger, more powerful models. When demand spikes (users > capacity), the network scales to more instances of smaller models to maintain availability. This elastic scaling happens automatically without user intervention.

Network capacity is measured in standardized Compute Units normalized to a reference model (e.g., "this network can serve X instances of qwen3.6:35b-a3b for N concurrent users"). The Network Manager continuously monitors demand, available hardware, and model performance to optimize the quality/availability tradeoff.

The system builds on Phase 6 (Reticulum Channel) for encrypted peer-to-peer communication, Phase 7 (Hardware Stability) for hardware profiling, and Phase 9 (Local Cluster) for workload orchestration within each node. It adds: multi-user identity and authentication, compute contribution tracking, fair-share scheduling, dynamic model scaling, and network-wide capacity planning.

All communication is end-to-end encrypted via Reticulum. No central server exists — the network is fully decentralized with elected coordinators. Users retain full sovereignty over their hardware and can withdraw from the network at any time with zero impact on their local operation.

## Glossary

- **Mesh_Compute_Network**: The decentralized network of user nodes sharing compute resources for LLM inference
- **Network_Node**: A user's machine (or Local Cluster) participating in the mesh, contributing and consuming compute resources
- **Network_Manager**: The distributed coordination layer that manages capacity, scheduling, and scaling decisions (runs on elected coordinator nodes)
- **Compute_Unit**: A standardized measure of compute capacity normalized to a reference model's inference throughput (1 CU = 1 token/sec of reference model)
- **Fractional_Reserve_Ratio**: The ratio of total network capacity to simultaneously active users — the network operates on the assumption that not all users need compute at the same time (typical ratio: 3:1 to 5:1)
- **Capacity_Pool**: The aggregate compute resources available across all Network_Nodes, measured in Compute_Units
- **Demand_Level**: The current aggregate compute demand from all active users, measured in Compute_Units requested per second
- **Scaling_Decision**: The Network_Manager's choice to scale up (more instances of smaller models) or scale down (fewer instances of larger models) based on demand vs capacity
- **Model_Tier**: A classification of models by resource requirements: "heavy" (35B+, requires GPU with 24GB+ VRAM), "medium" (7B-14B, requires GPU with 8GB+ VRAM or high RAM), "light" (1B-3B, runs on any hardware)
- **Contribution_Score**: A per-user metric tracking how much compute they contribute to the network vs how much they consume (fairness enforcement)
- **Fair_Share_Quota**: The maximum compute a user can consume per time period, proportional to their Contribution_Score
- **Network_Identity**: A Reticulum cryptographic identity representing a user on the mesh network
- **Coordinator_Node**: A Network_Node elected to run Network_Manager coordination duties for a region of the mesh
- **Coordinator_Election**: The consensus mechanism for selecting Coordinator_Nodes (based on uptime, contribution, and hardware capability)
- **Inference_Request**: A user's request for model inference routed through the mesh to an available node
- **Request_Priority**: The scheduling priority of an Inference_Request: "interactive" (user waiting, < 1s target), "batch" (background, best-effort), "preemptible" (can be interrupted if higher priority arrives)
- **Network_Partition**: A condition where some nodes cannot communicate with others, requiring independent operation of each partition
- **Withdrawal**: A user removing their node from the network, reclaiming all local resources immediately
- **Compute_Attestation**: Cryptographic proof that a node actually performed the claimed computation (prevents freeloading)
- **Quality_of_Service (QoS)**: The guaranteed performance level for a user's requests based on their Fair_Share_Quota and current network load

## Requirements

### Requirement 1: Fractional Reserve Capacity Model

**User Story:** As the network, I want to serve more users than raw hardware suggests by leveraging the statistical fact that users don't all need compute simultaneously, so that shared hardware is used efficiently.

#### Acceptance Criteria

1. THE Network_Manager SHALL maintain a Fractional_Reserve_Ratio representing the ratio of registered users to simultaneously serviceable users (configurable, default 4:1 — meaning hardware for N users can serve 4N registered users)
2. THE Network_Manager SHALL continuously monitor the actual concurrent usage ratio and adjust the Fractional_Reserve_Ratio based on observed demand patterns (time-of-day, day-of-week)
3. THE Network_Manager SHALL compute the Capacity_Pool in Compute_Units by summing all contributing nodes' available resources, weighted by their online probability (historical uptime percentage)
4. WHEN actual concurrent demand exceeds the Capacity_Pool (reserve breach), THE Network_Manager SHALL activate degraded mode: queue lower-priority requests, scale to smaller models, and notify affected users of increased latency
5. THE Network_Manager SHALL maintain a reserve buffer of 20% capacity that is never allocated to batch/preemptible workloads, ensuring interactive requests always have resources available
6. THE system SHALL measure and report network capacity in human-readable terms: "This network can serve X concurrent users at Y quality level" (e.g., "12 concurrent users at qwen3.6:35b quality" or "48 concurrent users at qwen3.6:7b quality")

### Requirement 2: Dynamic Model Scaling

**User Story:** As the network, I want to automatically scale between fewer large models and more small models based on demand, so that quality is maximized when demand is low and availability is maintained when demand is high.

#### Acceptance Criteria

1. THE Network_Manager SHALL implement a scaling algorithm that transitions between Model_Tiers based on Demand_Level: when demand < 50% capacity, prefer "heavy" tier models (fewer instances, higher quality); when demand is 50-80% capacity, use "medium" tier; when demand > 80% capacity, scale to "light" tier (more instances, lower quality but higher throughput)
2. THE Network_Manager SHALL make Scaling_Decisions at configurable intervals (default: every 60 seconds) based on the trailing 5-minute demand average
3. WHEN scaling down (heavy → medium → light), THE Network_Manager SHALL complete the transition within 2 minutes: unload heavy models, load lighter models on freed resources, begin serving with new configuration
4. WHEN scaling up (light → medium → heavy), THE Network_Manager SHALL complete the transition within 5 minutes: wait for in-flight requests to complete, unload light models, load heavier models
5. THE Network_Manager SHALL never scale during active interactive requests — scaling transitions SHALL only affect idle or batch capacity
6. THE Network_Manager SHALL maintain at least one instance of the user's preferred model tier when the user is actively making requests (personal quality guarantee)

### Requirement 3: Multi-User Identity and Authentication

**User Story:** As a user, I want a cryptographic identity on the mesh that proves who I am without a central authority, so that my contributions and consumption are tracked fairly.

#### Acceptance Criteria

1. THE system SHALL use Reticulum cryptographic identities (from Phase 6) as Network_Identities for mesh authentication — no additional identity system required
2. THE system SHALL authenticate all Inference_Requests using the sender's Network_Identity, verifying the request originated from a registered network participant
3. THE system SHALL maintain a distributed registry of Network_Identities with their associated Contribution_Scores, accessible to all Coordinator_Nodes
4. THE system SHALL support identity revocation: a user can be removed from the network by coordinator consensus, immediately invalidating their ability to submit requests or receive workloads
5. THE system SHALL NOT require any central server for identity management — all identity operations use Reticulum's decentralized cryptographic infrastructure
6. A new user SHALL be able to join the network by: generating a Reticulum identity, announcing to the mesh, and being confirmed by an existing Coordinator_Node (invitation model)

### Requirement 4: Contribution Tracking and Fair Share

**User Story:** As a user, I want fair access to network compute proportional to what I contribute, so that freeloaders can't consume resources without giving back.

#### Acceptance Criteria

1. THE system SHALL track each user's Contribution_Score as: (compute_hours_contributed × hardware_quality_factor) / (compute_hours_consumed)
2. THE system SHALL compute hardware_quality_factor based on the contributing node's Compute_Units: a GPU node contributing 100 CU/hour gets more credit than a CPU node contributing 10 CU/hour
3. THE system SHALL enforce Fair_Share_Quotas: a user's maximum consumption rate is proportional to their Contribution_Score, with a minimum baseline for new users (grace period of 7 days)
4. WHEN a user exceeds their Fair_Share_Quota, THE system SHALL deprioritize their requests (move to "batch" priority) rather than rejecting them outright
5. THE system SHALL publish Contribution_Scores to all Coordinator_Nodes at hourly intervals, enabling consistent fair-share enforcement across the mesh
6. THE system SHALL support "contribution-only" mode: a user can contribute hardware without consuming network resources, building up Contribution_Score for future use

### Requirement 5: Distributed Coordination and Consensus

**User Story:** As the network, I want coordination without a single point of failure, so that no single node's failure can bring down the network.

#### Acceptance Criteria

1. THE system SHALL elect Coordinator_Nodes using a consensus mechanism based on: node uptime (> 95% over 30 days), Contribution_Score (top 20%), and hardware capability (sufficient to run coordination workload)
2. THE system SHALL maintain a minimum of 3 Coordinator_Nodes for redundancy when the network has 5 or more nodes, with automatic re-election when a coordinator goes offline; for networks with fewer than 5 nodes, all nodes participate in coordination (full consensus)
3. THE Coordinator_Nodes SHALL reach consensus on: Scaling_Decisions, Fair_Share_Quota enforcement, identity registration/revocation, and network-wide capacity reporting
4. THE system SHALL handle Network_Partitions gracefully: each partition operates independently with its own coordinator(s), and partitions merge automatically when connectivity is restored
5. THE coordination overhead SHALL be less than 5% of total network Compute_Units — coordination does not significantly reduce available inference capacity
6. THE system SHALL support coordinator rotation: no single node is permanently a coordinator, rotation occurs monthly to distribute the coordination workload

### Requirement 6: Inference Request Routing

**User Story:** As a user, I want my inference requests routed to the best available node, so that I get fast, high-quality responses regardless of which node serves them.

#### Acceptance Criteria

1. THE system SHALL route Inference_Requests based on: request priority, required model tier, user's Fair_Share_Quota remaining, node proximity (network hops), and node current load
2. THE system SHALL prefer routing to nodes with the requested model already loaded (avoid cold-start latency)
3. THE system SHALL support request priority levels: "interactive" (target < 1 second to first token, preempts batch), "batch" (best-effort, no latency guarantee), "preemptible" (can be interrupted and requeued)
4. THE system SHALL implement request queuing when all suitable nodes are busy: queue with estimated wait time, notify user if wait exceeds 30 seconds
5. THE system SHALL support request cancellation: a user can cancel a queued or in-flight request, freeing the allocated resources immediately
6. THE system SHALL route requests over Reticulum TCP transport (from Phase 6), with end-to-end encryption and no plaintext inference data on the wire; LoRa and serial transports are excluded from compute mesh routing due to insufficient bandwidth — they remain available only for the Phase 6 messaging channel

### Requirement 7: Compute Attestation and Verification

**User Story:** As the network, I want proof that nodes actually performed claimed computations, so that freeloading nodes can't claim contribution credit without doing work.

#### Acceptance Criteria

1. THE system SHALL implement Compute_Attestation: after completing an inference request, the serving node produces a cryptographic attestation containing: request hash, response hash, model identifier, computation duration, and node signature
2. THE requesting user SHALL verify the attestation before accepting the response, confirming the response was produced by the claimed model on the claimed node
3. THE system SHALL use attestations to update Contribution_Scores: only attested computations count toward a node's contribution
4. THE system SHALL detect and flag anomalous attestations: responses that are too fast (impossible given model size), responses that don't match expected output distribution, or duplicate attestations
5. IF a node produces invalid attestations repeatedly (3 consecutive failures), THE system SHALL temporarily suspend the node from receiving workloads and notify coordinators for review
6. THE attestation overhead SHALL be less than 5ms per request — attestation generation and verification must not significantly impact inference latency

### Requirement 8: Network Capacity Planning and Reporting

**User Story:** As a user, I want to understand the network's current capacity and my position in it, so that I can make informed decisions about contributing more hardware or adjusting my usage.

#### Acceptance Criteria

1. THE system SHALL report network capacity in user-friendly terms: "Network can currently serve N users at [heavy/medium/light] quality" with real-time updates
2. THE system SHALL report per-user metrics: current Contribution_Score, Fair_Share_Quota remaining, requests served today, average response latency, and quality tier received
3. THE system SHALL provide capacity forecasting: based on historical demand patterns, predict capacity needs for the next 24 hours and alert if demand is expected to exceed capacity
4. THE system SHALL report per-node contribution: how much each of the user's nodes is contributing, uptime percentage, and compute hours served
5. THE system SHALL display the current Scaling_Decision state: which model tier is active, why (demand level), and when the next scaling evaluation occurs
6. THE system SHALL integrate capacity metrics with the Phase 1 Cost Dashboard, adding a "Mesh Network" section

### Requirement 9: User Sovereignty and Withdrawal

**User Story:** As a user, I want full control over my hardware at all times, so that I can withdraw from the network instantly without losing local functionality.

#### Acceptance Criteria

1. THE system SHALL support instant Withdrawal: a user can remove their node from the network at any time, immediately reclaiming all local resources with zero delay
2. UPON Withdrawal, THE system SHALL: stop accepting new workloads from the network, complete any in-flight requests (or transfer them to other nodes within 30 seconds), and remove the node from the Network_Registry
3. AFTER Withdrawal, THE system SHALL operate identically to a standalone installation — all local functionality continues without the network
4. THE system SHALL preserve the user's Contribution_Score after withdrawal for 90 days, allowing them to rejoin with their earned credit intact
5. THE system SHALL support partial withdrawal: a user can reduce their contributed resources (e.g., "contribute only when idle" or "contribute only 50% of GPU") without fully leaving the network
6. THE system SHALL never prevent a user from using their own hardware for local tasks — local interactive requests ALWAYS take priority over network workloads

### Requirement 10: Quality of Service Guarantees

**User Story:** As a user, I want predictable response quality and latency, so that the mesh network provides a reliable service rather than unpredictable best-effort.

#### Acceptance Criteria

1. THE system SHALL guarantee that interactive requests from users within their Fair_Share_Quota receive a response within 5 seconds (time to first token) under normal network conditions (demand < 80% capacity)
2. THE system SHALL guarantee a minimum model quality tier for each user based on their Contribution_Score: high contributors (top 25%) always receive "heavy" tier, medium contributors receive "medium" tier minimum, low contributors receive "light" tier minimum
3. WHEN the network cannot meet QoS guarantees (overload), THE system SHALL notify affected users with: estimated wait time, current quality tier available, and option to fall back to local-only execution
4. THE system SHALL support QoS degradation preferences: users can configure whether they prefer "wait for quality" (queue until heavy model available) or "fast response" (accept lighter model immediately)
5. THE system SHALL track QoS metrics per user: percentage of requests meeting latency target, average quality tier received, and any SLA violations over the past 30 days

### Requirement 11: Network Security and Privacy

**User Story:** As a user, I want my inference data private and the network secure against attacks, so that sharing hardware doesn't compromise my data or my machine.

#### Acceptance Criteria

1. ALL inference request content and response content SHALL be end-to-end encrypted between the requesting user and the serving node — no intermediate node (including coordinators) can read the content
2. THE system SHALL NOT store inference content on serving nodes after the response is delivered — all request/response data is ephemeral
3. THE system SHALL isolate network workloads from local workloads: a network inference request cannot access the serving user's local files, credentials, or conversation history
4. THE system SHALL implement rate limiting per Network_Identity to prevent denial-of-service attacks: maximum 100 requests per minute per user (configurable)
5. THE system SHALL detect and mitigate Sybil attacks: a single user creating multiple identities to exceed Fair_Share_Quotas, detected via hardware fingerprinting and contribution pattern analysis
6. THE system SHALL support network-wide blocklists: coordinators can consensus-block malicious identities, preventing them from submitting requests or receiving workloads

### Requirement 11b: Traffic Indistinguishability and Metadata Privacy

**User Story:** As a user, I want compute traffic to be indistinguishable from chat traffic to any intermediate node or network observer, so that nobody can determine what I'm using the network for.

#### Acceptance Criteria

1. THE system SHALL pad all compute request and response packets to standardized size buckets (512B, 2KB, 8KB, 32KB) so that packet size does not reveal whether a message is chat or inference
2. THE system SHALL use Reticulum's native initiator anonymity: intermediate transport nodes SHALL NOT know the original sender, final destination, or full path of any packet — they know only the next hop
3. THE system SHALL NOT add any unencrypted headers, metadata fields, or protocol markers that would allow an intermediate node to distinguish compute traffic from LXMF chat traffic
4. THE Coordinator_Nodes SHALL receive only abstract scheduling metadata (requested tier, not prompt content; capacity class, not exact hardware specs; aggregate contribution, not per-request history) — minimizing coordinator knowledge to the minimum required for scheduling
5. THE system SHALL share only abstract Hardware_Capability_Classes ("gpu-heavy", "gpu-medium", "cpu-large", "cpu-small") to the mesh network — never exact GPU model, VRAM amount, or CPU model that could fingerprint a machine
6. THE system SHALL quantize attestation duration to coarse buckets (< 1s, 1-5s, 5-30s, > 30s) rather than exact milliseconds, preventing inference of prompt length from timing metadata
7. THE system SHALL aggregate Contribution_Scores over 24-hour windows rather than per-request granularity, preventing usage pattern analysis from contribution data
8. THE system SHALL ensure routing decisions (which user requested which tier) are visible only to the requesting user and the assigned coordinator — never broadcast to other nodes or coordinators

### Requirement 11c: Reticulum Path-Aware Routing

**User Story:** As the system, I want to leverage Reticulum's existing path knowledge for compute routing decisions, so that the orchestrator doesn't maintain a parallel network topology view.

#### Acceptance Criteria

1. THE system SHALL query Reticulum's transport layer for path quality metrics when making routing decisions: hop count (via Transport.hops_to), link establishment rate, path freshness, and interface bitrate
2. THE system SHALL prefer serving nodes with fewer Reticulum hops and higher link quality, combining path quality with model availability and node load in a unified scoring function
3. THE system SHALL NOT maintain separate latency probes for compute routing — Reticulum's path table and link quality metrics ARE the network proximity data
4. THE system SHALL use Reticulum's native multi-hop routing for inference payload delivery — the orchestrator selects the destination, Reticulum optimizes the path
5. WHEN multiple nodes offer the same model at similar load, THE system SHALL prefer the node with the best Reticulum path quality (fewest hops, highest link rate) to minimize latency

### Requirement 12: Graceful Degradation

**User Story:** As a user, I want the system to work perfectly without the mesh network, so that network issues never break my local experience.

#### Acceptance Criteria

1. IF the mesh network is unavailable (no peers reachable, coordinator offline, network partition), THE system SHALL fall back to local-only operation with zero impact on local functionality
2. IF the user's Fair_Share_Quota is exhausted, THE system SHALL fall back to local inference rather than blocking the user
3. IF a serving node fails mid-inference, THE system SHALL transparently retry on another node or fall back to local execution within 5 seconds
4. THE mesh network overhead SHALL be less than 50MB RAM and negligible CPU when no network requests are active
5. THE system SHALL function identically to a standalone installation if the mesh network feature is disabled — it is purely opt-in and additive

### Requirement 13: Behavioral Contract Integration

**User Story:** As a developer, I want the mesh compute network to ship with behavioral contracts, so that the Phase 0 backtest mode can verify its correctness.

#### Acceptance Criteria

1. THE system SHALL register Behavioral_Contracts covering: fractional reserve never overcommits beyond configured ratio, scaling decisions respect demand thresholds, and interactive requests always have reserved capacity
2. THE system SHALL register Behavioral_Contracts covering: fair share enforcement is proportional to contribution, attestations are verified before accepting responses, and withdrawal completes within 30 seconds
3. THE system SHALL register Behavioral_Contracts covering: all network communication is encrypted, inference content is ephemeral on serving nodes, and local workloads are isolated from network workloads
4. THE system SHALL register Behavioral_Contracts covering: coordinator election maintains minimum redundancy, network partitions are handled independently, and QoS guarantees are met when demand is below capacity
5. WHEN a Behavioral_Contract for the mesh compute network fails, THE Regression_Gate SHALL block the merge and produce a Diagnostic_Report
