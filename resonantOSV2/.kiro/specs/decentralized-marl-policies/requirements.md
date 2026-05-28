# Requirements: Decentralized Multi-Agent RL Policies

## Overview

Replace the single centralized DQN policy with per-node lightweight RL agents that make local priority decisions. Each node runs its own policy, observes local state, and periodically shares compressed policy updates with peers via federated aggregation.

## Functional Requirements

### 1. Per-Node Policy Agent

- 1.1 Each node SHALL run its own lightweight RL policy (independent ONNX model or tabular policy)
- 1.2 Each agent SHALL observe only local state (own CPU, RAM, VRAM, queue depth, loaded models)
- 1.3 Each agent SHALL produce local priority adjustments for models it hosts
- 1.4 Agent inference SHALL complete in < 2ms (lighter than centralized 5ms budget)
- 1.5 Agents SHALL operate independently — no single point of failure
- 1.6 If an agent fails, the node SHALL use uniform priorities (graceful degradation)

### 2. Local State Observation

- 2.1 Each agent SHALL encode its local state into a compact feature vector (≤16 floats)
- 2.2 Features SHALL include: CPU utilization, RAM pressure, VRAM pressure, queue depth, model load factors
- 2.3 Features SHALL include: time-of-day encoding (sin/cos), recent request rate
- 2.4 Features SHALL NOT include other nodes' state (decentralized — no global view)
- 2.5 Feature encoding SHALL be deterministic for the same input state

### 3. Action Space

- 3.1 Each agent's action space SHALL be the set of loaded models on that node (variable size, max 8)
- 3.2 Actions SHALL represent priority boosts for each loaded model (continuous [-0.3, +0.3])
- 3.3 The action space SHALL adapt when models are loaded/unloaded (dynamic action space)
- 3.4 A "no-op" action (all zeros) SHALL always be available

### 4. Reward Signal

- 4.1 Each agent SHALL compute its own reward from local observations
- 4.2 Reward SHALL combine: inference speed (tok/s), queue wait time, request success rate
- 4.3 Reward SHALL penalize: queue overflow, request timeout, thermal throttling
- 4.4 Reward SHALL be normalized to [-1, +1] range
- 4.5 Reward computation SHALL use only locally available data (no network queries)

### 5. Federated Policy Aggregation

- 5.1 Agents SHALL periodically share compressed policy updates with peers
- 5.2 Sharing interval SHALL be configurable (default: every 10 optimizer cycles = 10 minutes)
- 5.3 Policy updates SHALL be compressed (delta encoding, quantized weights)
- 5.4 Update payload SHALL be < 10KB per agent per sharing round
- 5.5 Aggregation SHALL use federated averaging (FedAvg): weighted average of policy parameters
- 5.6 Aggregation SHALL weight by each node's experience count (more experience = more influence)
- 5.7 Nodes that haven't shared in 30 minutes SHALL be excluded from aggregation

### 6. Coordination Protocol

- 6.1 Agents SHALL NOT require synchronous communication (fully asynchronous)
- 6.2 Policy sharing SHALL use the existing transport layer (LAN/WireGuard/Reticulum)
- 6.3 A node joining the network SHALL receive the current aggregated policy from any peer
- 6.4 A node leaving SHALL NOT disrupt other agents' operation
- 6.5 The protocol SHALL handle network partitions gracefully (agents continue with local policy)

### 7. Exploration Strategy

- 7.1 Each agent SHALL maintain its own epsilon for exploration
- 7.2 Epsilon SHALL decay independently per agent based on local experience
- 7.3 Agents SHALL use different random seeds (diverse exploration across the network)
- 7.4 After federated aggregation, epsilon SHALL NOT reset (preserve individual exploration progress)

### 8. Integration with Existing RL

- 8.1 The decentralized system SHALL coexist with the centralized policy (configurable mode)
- 8.2 Mode selection: "centralized" (current DQN), "decentralized" (MARL), "hybrid" (central + local)
- 8.3 In hybrid mode: central policy provides baseline, local agents provide adjustments
- 8.4 Switching modes SHALL NOT require restart (hot-switchable)
- 8.5 The existing `RlConfig` SHALL be extended with MARL-specific fields

## Non-Functional Requirements

### 9. Performance

- 9.1 Per-node agent overhead SHALL be < 2ms per cycle
- 9.2 Memory overhead per agent SHALL be < 500KB
- 9.3 Network overhead for policy sharing SHALL be < 10KB per node per 10 minutes
- 9.4 System SHALL support up to 50 concurrent agents (50-node mesh)

### 10. Scalability

- 10.1 Adding a new node SHALL NOT require retraining existing agents
- 10.2 The system SHALL scale linearly with node count (no quadratic communication)
- 10.3 Federated aggregation SHALL be gossip-based (each node shares with 2-3 peers, not all)

## Correctness Properties

- P1: Independence — each agent's decision depends only on local state
- P2: Bounded Actions — all priority adjustments in [-0.3, +0.3]
- P3: Convergence — federated averaging converges to a shared policy over time
- P4: Graceful Degradation — agent failure produces uniform priorities (no worse than no-RL)
- P5: Payload Bound — policy update messages always < 10KB
- P6: Reward Normalization — reward always in [-1, +1]
