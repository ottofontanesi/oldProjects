# Requirements Document

## Introduction

Local Cluster is Phase 9 of the ResonantOS vNext improvement plan. It extends the Compute Fabric to orchestrate multiple heterogeneous machines on a local network (LAN) as a unified compute pool. Machine A might have a powerful GPU, Machine B might have 128GB RAM, Machine C might be a lightweight ARM device — the Local Cluster Manager treats them as a single resource pool, routing workloads to the machine best suited for each task based on hardware capabilities, current load, and model availability.

The system uses zero-configuration LAN discovery (mDNS/Avahi) to find peer nodes, the Phase 7 HardwareProfile for capability assessment, and the existing Compute Fabric job submission infrastructure for workload dispatch. Each node runs a lightweight Cluster Agent that reports its HardwareProfile, current resource utilization, loaded models, and availability. The orchestrator (running on the user's primary desktop) makes all scheduling decisions.

The Local Cluster operates within a single trust boundary — all nodes are owned by the same user and share credentials. This distinguishes it from Phase 10 (Mesh Compute Network) which operates across trust boundaries with multiple users. The cluster adds zero latency to interactive operations when all nodes are local (< 1ms LAN latency) and degrades gracefully when nodes become unavailable.

## Glossary

- **Local_Cluster**: The set of all discovered and registered machines on the LAN operating as a unified compute pool
- **Cluster_Node**: A single machine participating in the Local Cluster, running the Cluster Agent
- **Cluster_Agent**: The lightweight service running on each node that reports capabilities, accepts workloads, and manages local execution
- **Cluster_Orchestrator**: The scheduling component running on the primary desktop that makes workload placement decisions
- **Node_Registry**: The persistent record of all known cluster nodes with their HardwareProfiles, status, and capabilities
- **Workload_Placement**: The decision of which node should execute a given workload based on requirements and available resources
- **Placement_Strategy**: The algorithm used for workload placement: "best-fit" (most capable node), "spread" (distribute evenly), "pack" (fill one node before using next)
- **Node_Health**: The current operational status of a cluster node: "ready", "busy", "degraded", "offline"
- **Resource_Request**: A workload's hardware requirements: minimum VRAM, minimum RAM, required GPU, required model, estimated duration
- **Model_Registry**: The cluster-wide index of which models are loaded/cached on which nodes
- **Workload_Migration**: The ability to move a queued workload from one node to another when conditions change
- **Node_Affinity**: A preference for scheduling certain workload types on specific nodes (e.g., always run coding tasks on the GPU node)
- **Cluster_Event**: A significant change in cluster state: node joined, node left, node degraded, workload completed, workload failed
- **LAN_Discovery**: The zero-configuration mechanism (mDNS) for finding cluster nodes on the local network

## Requirements

### Requirement 1: Zero-Configuration Node Discovery

**User Story:** As a user, I want machines on my LAN to automatically discover each other, so that adding a new machine to the cluster requires no manual configuration.

#### Acceptance Criteria

1. THE Cluster_Agent SHALL announce its presence on the LAN using mDNS (service type "_resonantos-cluster._tcp") with TXT records containing: node_id, hardware_class, available_vram, available_ram, and cluster_protocol_version
2. THE Cluster_Orchestrator SHALL continuously listen for mDNS announcements and automatically register newly discovered nodes in the Node_Registry
3. WHEN a new node is discovered, THE Cluster_Orchestrator SHALL request the full HardwareProfile from the node via the cluster protocol and store it in the Node_Registry
4. WHEN a node stops announcing (mDNS TTL expires without refresh), THE Cluster_Orchestrator SHALL transition the node to "offline" status after a configurable grace period (default: 60 seconds)
5. THE system SHALL support manual node registration via IP address for environments where mDNS is unavailable or blocked
6. THE system SHALL require explicit user confirmation before a newly discovered node is activated for workload scheduling (security gate)

### Requirement 2: Cluster Agent Service

**User Story:** As the system, I want a lightweight agent on each node that reports capabilities and executes workloads, so that the orchestrator can make informed scheduling decisions.

#### Acceptance Criteria

1. THE Cluster_Agent SHALL run as a background service on each participating node, consuming less than 50MB RAM and negligible CPU when idle
2. THE Cluster_Agent SHALL report to the orchestrator at configurable intervals (default: 10 seconds): current CPU utilization, current RAM utilization, current GPU utilization and VRAM usage, loaded models list, active workload count, and Thermal_State
3. THE Cluster_Agent SHALL accept workload execution requests from the orchestrator via the cluster protocol and execute them using the local Compute Fabric infrastructure
4. THE Cluster_Agent SHALL report workload completion (success or failure) back to the orchestrator with execution results, duration, and resource consumption metrics
5. THE Cluster_Agent SHALL enforce Resource_Envelope limits locally: reject workloads that would exceed available resources rather than attempting execution that would OOM
6. THE Cluster_Agent SHALL authenticate all communication with the orchestrator using a shared cluster secret (generated during initial cluster setup)

### Requirement 3: Workload Placement and Scheduling

**User Story:** As the system, I want intelligent workload placement that routes tasks to the best-suited node, so that GPU tasks go to GPU nodes and memory-intensive tasks go to high-RAM nodes.

#### Acceptance Criteria

1. THE Cluster_Orchestrator SHALL evaluate each incoming workload's Resource_Request against all "ready" nodes' available resources and select the optimal node using the configured Placement_Strategy
2. THE Cluster_Orchestrator SHALL support three Placement_Strategies: "best-fit" (select the node with the most appropriate resources for the workload), "spread" (distribute workloads evenly across nodes by count), "pack" (fill the most-loaded node that still has capacity before using others)
3. THE Cluster_Orchestrator SHALL implement Capability_Gates: never schedule a GPU workload on a CPU-only node, never schedule a workload requiring more VRAM than available, never schedule a workload requiring a model not present on the node (unless model transfer is faster than waiting)
4. THE Cluster_Orchestrator SHALL support Node_Affinity rules: user-configurable preferences that bias certain workload types toward specific nodes (e.g., "prefer gpu-node-1 for coding tasks")
5. IF no node can satisfy a workload's Resource_Request, THE Cluster_Orchestrator SHALL queue the workload and retry when resources become available, with a configurable timeout (default: 5 minutes)
6. THE Cluster_Orchestrator SHALL make placement decisions within 10 milliseconds to avoid adding perceptible latency to interactive operations

### Requirement 4: Model-Aware Scheduling

**User Story:** As the system, I want scheduling that considers which models are already loaded on which nodes, so that inference requests are routed to nodes with the model already in memory.

#### Acceptance Criteria

1. THE Cluster_Orchestrator SHALL maintain a cluster-wide Model_Registry tracking which models are currently loaded in memory (GPU or CPU) on each node
2. WHEN an inference workload requires a specific model, THE Cluster_Orchestrator SHALL prefer nodes that already have the model loaded (avoiding cold-start model loading latency)
3. IF no node has the required model loaded, THE Cluster_Orchestrator SHALL select the node with the best hardware for that model and account for model loading time in the scheduling decision
4. THE Cluster_Orchestrator SHALL support pre-loading: when a model is frequently requested, proactively load it on the most suitable node during idle periods
5. THE Model_Registry SHALL be updated in real-time as nodes load and unload models, with updates propagated within 5 seconds of the change
6. THE Cluster_Orchestrator SHALL consider model loading time in placement decisions: if Node A has the model loaded (0ms startup) and Node B is faster but needs to load (30s startup), prefer Node A for latency-sensitive requests

### Requirement 5: Scalability at Model and Agent Layers

**User Story:** As the system, I want the cluster to scale model inference and agent execution across nodes, so that I can run larger models and more concurrent agents than any single machine supports.

#### Acceptance Criteria

1. THE system SHALL support model-layer scaling: route inference requests for different models to different nodes based on hardware fit (70B model on GPU node, 7B model on CPU node)
2. THE system SHALL support agent-layer scaling: different agents can execute on different nodes based on their resource requirements (coding agent on GPU node, research agent on high-RAM node)
3. THE system SHALL support concurrent execution: multiple agents can run simultaneously on different nodes without resource contention
4. THE system SHALL NOT support tensor parallelism (splitting model layers across multiple nodes for per-token communication) — each model instance's forward pass runs entirely on one node. Pipeline parallelism (prefill on one node, decode on another with one-time KV cache transfer) is supported separately under Requirement 12
5. THE system SHALL track per-node capacity in terms of concurrent model instances and concurrent agent executions, preventing overcommit
6. THE system SHALL report cluster-wide capacity metrics: total available compute, current utilization percentage, and estimated capacity for additional workloads

### Requirement 6: Fault Tolerance and Node Failure

**User Story:** As a user, I want the cluster to handle node failures gracefully, so that a crashed machine doesn't break my workflow.

#### Acceptance Criteria

1. WHEN a node transitions to "offline" status, THE Cluster_Orchestrator SHALL reassign any queued (not yet started) workloads for that node to other available nodes
2. WHEN a node fails mid-execution of a workload, THE Cluster_Orchestrator SHALL detect the failure within 30 seconds (via missed heartbeats), mark the workload as failed, and offer retry on another node
3. THE Cluster_Orchestrator SHALL maintain a minimum of one "ready" node (the primary desktop) that can handle all workload types at reduced performance — the cluster enhances performance but is never required
4. WHEN all remote nodes are offline, THE system SHALL operate identically to a single-machine installation with zero errors or degradation beyond the loss of remote compute capacity
5. WHEN a previously offline node returns, THE Cluster_Orchestrator SHALL automatically re-register it, refresh its HardwareProfile, and resume scheduling workloads to it within 30 seconds
6. THE system SHALL log all node state transitions as Cluster_Events with timestamp, node_id, previous state, new state, and reason

### Requirement 7: Cluster Communication Protocol

**User Story:** As the system, I want a well-defined protocol between orchestrator and agents, so that nodes can be developed and updated independently.

#### Acceptance Criteria

1. THE cluster protocol SHALL use gRPC over TLS for all orchestrator-agent communication, with mutual authentication using the shared cluster secret
2. THE cluster protocol SHALL support the following RPCs from orchestrator to agent: SubmitWorkload, CancelWorkload, GetStatus, GetHardwareProfile, LoadModel, UnloadModel, Ping
3. THE cluster protocol SHALL support the following RPCs from agent to orchestrator: ReportStatus (streaming), WorkloadCompleted, WorkloadFailed, ModelLoaded, ModelUnloaded
4. THE cluster protocol SHALL support bidirectional streaming for real-time status updates (agent streams utilization metrics to orchestrator every 10 seconds)
5. ALL cluster communication SHALL be encrypted in transit (TLS 1.3) and authenticated (mutual TLS or shared secret)
6. THE cluster protocol SHALL include a version field enabling rolling upgrades where nodes running different protocol versions can coexist with graceful feature negotiation

### Requirement 8: Resource Monitoring and Dashboard

**User Story:** As a user, I want visibility into cluster resource usage, so that I can see which nodes are busy, which are idle, and where my workloads are running.

#### Acceptance Criteria

1. THE system SHALL expose cluster-wide resource metrics via IPC: per-node CPU/RAM/GPU utilization, per-node active workloads, per-node loaded models, and aggregate cluster utilization
2. THE system SHALL integrate cluster metrics with the Phase 1 Cost Dashboard, adding a "Cluster" section showing: node count, total capacity, current utilization, workload distribution, and per-node health status
3. THE system SHALL display workload placement history: which workloads ran on which nodes, with duration and resource consumption
4. THE system SHALL alert the user when cluster utilization exceeds 80% sustained for 5 minutes (capacity planning signal)
5. THE system SHALL display model distribution across the cluster: which models are loaded where, with VRAM consumption per model per node

### Requirement 9: Security and Trust Boundary

**User Story:** As a user, I want all cluster communication secured, so that my models, data, and credentials are protected even on a shared LAN.

#### Acceptance Criteria

1. THE system SHALL generate a unique cluster secret during initial cluster setup, shared only with explicitly approved nodes
2. ALL workload data transmitted between nodes SHALL be encrypted in transit — no plaintext model weights, inference inputs, or outputs on the wire
3. THE Cluster_Agent SHALL NOT have access to provider credentials stored on other nodes — each node uses its own credential store or receives scoped tokens for specific operations
4. THE system SHALL support node revocation: removing a node from the cluster immediately invalidates its authentication and prevents further workload scheduling
5. THE system SHALL log all authentication failures and unauthorized access attempts as security events in the Compute Fabric audit trail

### Requirement 10: Graceful Degradation

**User Story:** As a user, I want the system to work perfectly as a single machine if no cluster nodes are available, so that clustering is purely additive.

#### Acceptance Criteria

1. IF no remote cluster nodes are discovered or configured, THE system SHALL operate identically to a single-machine installation with zero overhead from the clustering infrastructure
2. THE Cluster_Orchestrator overhead SHALL be less than 1ms per workload scheduling decision and less than 10MB RAM when no remote nodes are active
3. IF the Cluster_Orchestrator crashes, THE system SHALL fall back to local-only execution with zero impact on interactive operations
4. THE system SHALL NOT require cluster configuration to function — clustering is opt-in and purely additive to single-machine capabilities
5. WHEN transitioning from clustered to single-machine operation (all nodes offline), THE system SHALL complete the transition within 5 seconds with no dropped workloads (queued workloads execute locally)

### Requirement 11: Behavioral Contract Integration

**User Story:** As a developer, I want the local cluster to ship with behavioral contracts, so that the Phase 0 backtest mode can verify its correctness.

#### Acceptance Criteria

1. THE system SHALL register Behavioral_Contracts covering: node discovery correctly identifies LAN peers, workload placement respects Capability_Gates, and the Model_Registry accurately reflects loaded models
2. THE system SHALL register Behavioral_Contracts covering: node failure detection occurs within 30 seconds, workload reassignment on failure completes without data loss, and single-machine fallback operates with zero overhead
3. THE system SHALL register Behavioral_Contracts covering: cluster communication is always encrypted, authentication failures are logged, and node revocation immediately prevents scheduling
4. WHEN a Behavioral_Contract for the local cluster fails, THE Regression_Gate SHALL block the merge and produce a Diagnostic_Report

### Requirement 12: Pipeline Parallelism on High-Bandwidth LAN

**User Story:** As a user with multiple machines on a fast LAN, I want to run models that don't fit on any single machine by splitting prefill and decode across nodes, so that I can access larger models than any one machine supports.

#### Acceptance Criteria

1. THE system SHALL support optional pipeline parallelism for model inference where: Node A (GPU) handles the compute-intensive prefill phase (processing the full prompt) and Node B (high RAM or second GPU) handles the memory-intensive decode phase (generating tokens)
2. THE system SHALL enable pipeline parallelism ONLY when the inter-node connection bandwidth exceeds 1 Gbps (10GbE or faster) as measured by the Phase 7 network probe — gigabit ethernet is insufficient
3. THE system SHALL transfer the KV cache from the prefill node to the decode node as a one-time transfer after prefill completes (not per-token), making the bandwidth requirement manageable
4. THE system SHALL restrict pipeline parallelism to batch and background workloads by default — interactive workloads use whole-model routing unless the user explicitly enables pipeline mode for interactive
5. THE system SHALL estimate the latency overhead of pipeline parallelism (KV cache transfer time based on prompt length and available bandwidth) and include it in placement decisions — only use pipeline when the quality gain justifies the latency cost
6. THE system SHALL support pipeline parallelism ONLY within the Local Cluster (Phase 9) trust boundary — never across the Mesh Network (Phase 10) where latency and trust make it impractical
