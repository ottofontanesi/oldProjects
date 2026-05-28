# Design Document: Decentralized Multi-Agent RL Policies

## Overview

Each node in the ResonantOS mesh runs its own lightweight RL agent that observes local state and produces priority adjustments for locally-hosted models. Agents share compressed policy updates via gossip-based federated averaging, converging toward a shared optimal policy without centralized coordination.

### Design Principles

1. **Fully decentralized**: No coordinator node, no single point of failure
2. **Lightweight**: 16-float state, tabular/small-network policy, <2ms inference
3. **Asynchronous**: Agents operate independently, share updates opportunistically
4. **Backward compatible**: Coexists with centralized DQN via configurable mode
5. **Privacy-preserving**: Only policy parameters shared, never raw state or rewards

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                    Node A (Desktop)                               │
│  ┌──────────────────────────────────────────────────────────┐    │
│  │  LocalAgent                                              │    │
│  │  • Observe: CPU=45%, RAM=60%, VRAM=80%, queue=3          │    │
│  │  • Encode: [0.45, 0.60, 0.80, 0.15, ...]  (16 floats)   │    │
│  │  • Infer: policy(state) → [+0.2, -0.1, +0.05]           │    │
│  │  • Apply: boost llama +0.2, reduce qwen -0.1             │    │
│  │  • Reward: tok/s=45, queue_wait=20ms → r=0.7             │    │
│  └──────────────────────────────────────────────────────────┘    │
│                            │ every 10 cycles                     │
│                            ▼                                     │
│  ┌──────────────────────────────────────────────────────────┐    │
│  │  PolicySharer (gossip)                                    │    │
│  │  • Compress policy weights (delta + quantize)             │    │
│  │  • Send to 2-3 random peers via transport                 │    │
│  │  • Receive peer updates, federated average                │    │
│  └──────────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────┐
│                    Node B (Laptop)                                │
│  ┌──────────────────────────────────────────────────────────┐    │
│  │  LocalAgent (same architecture, different weights)        │    │
│  │  • Observe: CPU=70%, RAM=85%, VRAM=0%, queue=5            │    │
│  │  • Encode: [0.70, 0.85, 0.00, 0.25, ...]                 │    │
│  │  • Infer: policy(state) → [-0.1, +0.15]                  │    │
│  │  • Reward: tok/s=20, queue_wait=80ms → r=0.3             │    │
│  └──────────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────────┘
```

## Components

### MarlConfig

```rust
pub struct MarlConfig {
    pub mode: MarlMode,                    // Centralized | Decentralized | Hybrid
    pub state_size: usize,                 // Default: 16
    pub max_actions: usize,                // Default: 8 (max loaded models)
    pub max_adjustment: f64,               // Default: 0.3
    pub learning_rate: f64,                // Default: 0.01
    pub discount_factor: f64,              // Default: 0.95
    pub epsilon_initial: f64,              // Default: 0.2
    pub epsilon_min: f64,                  // Default: 0.02
    pub epsilon_decay: f64,                // Default: 0.998
    pub sharing_interval_cycles: u32,      // Default: 10
    pub gossip_fanout: u32,                // Default: 3
    pub update_payload_max_bytes: usize,   // Default: 10240
    pub aggregation_weight_by_experience: bool, // Default: true
    pub stale_threshold_secs: u64,         // Default: 1800 (30 min)
}

pub enum MarlMode {
    Centralized,   // Use existing single DQN
    Decentralized, // Per-node agents only
    Hybrid,        // Central baseline + local adjustments
}
```

### LocalAgent

```rust
pub struct LocalAgent {
    config: MarlConfig,
    /// Tabular Q-values: state_bucket × action → value
    q_table: Vec<Vec<f64>>,
    /// Current epsilon for this agent
    epsilon: f64,
    /// Experience counter (for aggregation weighting)
    experience_count: u64,
    /// Random seed (unique per agent)
    rng: StdRng,
    /// Loaded model IDs (defines action space)
    action_models: Vec<String>,
    /// Last state for TD update
    last_state: Option<Vec<f32>>,
    /// Last action for TD update
    last_action: Option<usize>,
}

impl LocalAgent {
    pub fn new(config: MarlConfig, seed: u64) -> Self;

    /// Encode local node state into compact feature vector.
    pub fn encode_state(&self, state: &LocalNodeState) -> Vec<f32>;

    /// Select action using epsilon-greedy on Q-table.
    pub fn select_action(&mut self, state: &[f32]) -> AgentAction;

    /// Update Q-values with observed reward (TD(0) update).
    pub fn update(&mut self, reward: f64, next_state: &[f32]);

    /// Get policy parameters for sharing (compressed).
    pub fn export_policy(&self) -> CompressedPolicy;

    /// Import and merge peer policy via federated averaging.
    pub fn import_policy(&mut self, peer_policy: &CompressedPolicy, peer_weight: f64);

    /// Update action space when models change.
    pub fn update_action_space(&mut self, loaded_models: &[String]);

    /// Compute reward from local observations.
    pub fn compute_reward(&self, obs: &LocalObservation) -> f64;
}
```

### LocalNodeState

```rust
pub struct LocalNodeState {
    pub cpu_utilization: f64,
    pub ram_pressure: f64,       // used/total
    pub vram_pressure: f64,      // used/total (0 if no GPU)
    pub queue_depth: u32,
    pub request_rate_per_min: f64,
    pub avg_tok_s: f64,
    pub avg_queue_wait_ms: f64,
    pub hour_of_day: u8,
    pub loaded_model_count: u8,
}
```

### AgentAction

```rust
pub struct AgentAction {
    pub adjustments: HashMap<String, f64>,  // model_id → priority adjustment
    pub was_exploration: bool,
    pub selected_index: usize,
}
```

### CompressedPolicy

```rust
pub struct CompressedPolicy {
    pub agent_id: NodeId,
    pub experience_count: u64,
    pub timestamp_ms: u64,
    /// Delta-encoded Q-table (only non-zero entries)
    pub q_deltas: Vec<(u16, u16, i16)>,  // (state_bucket, action, quantized_value)
    /// Current epsilon
    pub epsilon: f64,
}
```

### PolicySharer

```rust
pub struct PolicySharer {
    config: MarlConfig,
    /// Peers to share with (rotated each round)
    peer_list: Vec<NodeId>,
    /// Received policies pending aggregation
    inbox: Vec<CompressedPolicy>,
    /// Last sharing timestamp
    last_share_ms: u64,
    /// Cycle counter
    cycle_count: u32,
}

impl PolicySharer {
    pub fn new(config: MarlConfig) -> Self;

    /// Check if it's time to share policy.
    pub fn should_share(&self) -> bool;

    /// Select peers for this sharing round (gossip fanout).
    pub fn select_peers(&self) -> Vec<NodeId>;

    /// Receive a peer's policy update.
    pub fn receive_update(&mut self, policy: CompressedPolicy);

    /// Perform federated averaging on received policies.
    pub fn aggregate(&mut self, local_agent: &mut LocalAgent);

    /// Encode local policy for transmission.
    pub fn encode_for_sharing(&self, agent: &LocalAgent) -> Vec<u8>;
}
```

### RewardComputer

```rust
pub struct RewardComputer;

impl RewardComputer {
    /// Compute normalized reward from local observations.
    /// reward = w1*speed_score + w2*queue_score + w3*success_score - penalties
    /// Clamped to [-1, +1]
    pub fn compute(obs: &LocalObservation) -> f64 {
        let speed_score = (obs.avg_tok_s / obs.target_tok_s).min(1.0);
        let queue_score = 1.0 - (obs.avg_queue_wait_ms / 1000.0).min(1.0);
        let success_score = obs.success_rate;

        let penalty = if obs.thermal_throttling { 0.3 } else { 0.0 }
            + if obs.queue_overflow { 0.5 } else { 0.0 };

        let raw = 0.4 * speed_score + 0.3 * queue_score + 0.3 * success_score - penalty;
        raw.clamp(-1.0, 1.0)
    }
}
```

## Federated Averaging Protocol

```
Every 10 optimizer cycles (10 minutes):

1. Local agent exports compressed policy (delta-encoded Q-table)
2. PolicySharer selects 2-3 random peers from transport registry
3. Send compressed policy via transport (< 10KB)
4. On receiving peer policy:
   a. Check staleness (reject if > 30 min old)
   b. Compute weight: peer_experience / (local_experience + peer_experience)
   c. For each Q-value: Q_new = (1-w)*Q_local + w*Q_peer
   d. Do NOT reset epsilon (preserve local exploration progress)
```

## State Encoding (16 floats)

| Index | Feature | Normalization |
|-------|---------|---------------|
| 0 | CPU utilization | [0, 1] |
| 1 | RAM pressure | [0, 1] |
| 2 | VRAM pressure | [0, 1] (0 if no GPU) |
| 3 | Queue depth | / 20, cap at 1.0 |
| 4 | Request rate | / 100 req/min, cap at 1.0 |
| 5 | Avg tok/s | / 100, cap at 1.0 |
| 6 | Avg queue wait | / 1000ms, cap at 1.0 |
| 7 | Hour sin | [0, 1] |
| 8 | Hour cos | [0, 1] |
| 9 | Loaded model count | / 8, cap at 1.0 |
| 10-15 | Per-model load factors (top 6) | [0, 1] |

## Q-Table Design

- State space: discretized into 256 buckets (4-bit per feature × 16 features → 64-bit hash → mod 256)
- Action space: up to 8 actions (one per loaded model)
- Table size: 256 × 8 = 2048 entries × 8 bytes = 16KB per agent
- Compressed for sharing: delta encoding → typically < 5KB

## File Structure

```
src/resonantos-vnext/src-tauri/src/integration/
├── marl_config.rs       # MarlConfig, MarlMode
├── marl_agent.rs        # LocalAgent (Q-table, encode, select, update)
├── marl_reward.rs       # RewardComputer
├── marl_sharer.rs       # PolicySharer (gossip, federated averaging)
├── marl_types.rs        # LocalNodeState, AgentAction, CompressedPolicy
└── coordinator.rs       # [MODIFIED] Add MARL mode support
```
