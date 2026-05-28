# Implementation Plan: Decentralized Multi-Agent RL Policies

## Overview

Per-node lightweight RL agents with gossip-based federated policy averaging. Each node observes local state, makes local priority decisions, and periodically shares compressed policy updates with peers.

**Build verification:** `cargo test --lib --no-run` from `src/resonantos-vnext/src-tauri`.

## Tasks

- [ ] 1. Configuration and types
  - [ ] 1.1 Create `integration/marl_config.rs` with `MarlConfig` and `MarlMode`
    - Define all config fields with defaults
    - Define MarlMode enum (Centralized, Decentralized, Hybrid)
    - Implement Default trait
    - _Requirements: 8.1, 8.2, 8.5, 9.1_

  - [ ] 1.2 Create `integration/marl_types.rs` with shared types
    - Define `LocalNodeState`, `LocalObservation`, `AgentAction`, `CompressedPolicy`
    - All types derive Clone, Debug
    - CompressedPolicy: delta-encoded Q-table entries as (u16, u16, i16) tuples
    - _Requirements: 2.1, 3.1, 5.3, 5.4_

  - [ ] 1.3 Register new submodules in `integration/mod.rs`
    - Add pub mod marl_config, marl_types, marl_agent, marl_reward, marl_sharer
    - _Requirements: 8.1_

- [ ] 2. Local agent
  - [ ] 2.1 Implement `integration/marl_agent.rs` with `LocalAgent`
    - `new(config, seed)` — create agent with empty Q-table (256×8)
    - `encode_state(state)` — produce 16-float feature vector from LocalNodeState
    - `select_action(state)` — epsilon-greedy action selection from Q-table
    - `update(reward, next_state)` — TD(0) Q-value update
    - `update_action_space(loaded_models)` — adapt when models change
    - State discretization: hash 16 floats into 256 buckets
    - _Requirements: 1.1, 1.2, 1.3, 1.4, 2.1, 2.2, 2.3, 2.4, 2.5, 3.1, 3.2, 3.3, 3.4, 7.1, 7.2, 7.3_

  - [ ] 2.2 Implement policy export/import
    - `export_policy()` — delta-encode Q-table, return CompressedPolicy
    - `import_policy(peer, weight)` — federated average merge
    - Delta encoding: only include entries that differ from zero by > threshold
    - Quantize to i16 (multiply by 1000, round)
    - _Requirements: 5.1, 5.3, 5.4, 5.5, 5.6_

  - [ ]* 2.3 Write property tests for local agent
    - **P1: Independence** — agent output depends only on input state (deterministic for same seed+state)
    - **P2: Bounded Actions** — all adjustments in [-0.3, +0.3]
    - **P4: Graceful Degradation** — agent with zero Q-table produces uniform priorities
    - _Validates: Requirements 1.3, 1.6, 3.2_

- [ ] 3. Reward computation
  - [ ] 3.1 Implement `integration/marl_reward.rs` with `RewardComputer`
    - `compute(obs)` — combine speed, queue, success scores minus penalties
    - Weights: speed=0.4, queue=0.3, success=0.3
    - Penalties: thermal_throttle=-0.3, queue_overflow=-0.5
    - Clamp output to [-1, +1]
    - _Requirements: 4.1, 4.2, 4.3, 4.4, 4.5_

  - [ ]* 3.2 Write property test for reward normalization
    - **P6: Reward Normalization** — reward always in [-1, +1] for any valid observation
    - _Validates: Requirements 4.4_

- [ ] 4. Policy sharing (gossip + federated averaging)
  - [ ] 4.1 Implement `integration/marl_sharer.rs` with `PolicySharer`
    - `should_share()` — check cycle counter against sharing_interval
    - `select_peers()` — pick gossip_fanout random peers from transport registry
    - `receive_update(policy)` — buffer incoming peer policy
    - `aggregate(local_agent)` — perform FedAvg on buffered policies
    - `encode_for_sharing(agent)` — serialize CompressedPolicy to bytes (< 10KB)
    - _Requirements: 5.1, 5.2, 5.5, 5.6, 5.7, 6.1, 6.2, 10.2, 10.3_

  - [ ] 4.2 Implement staleness filtering
    - Reject policies older than stale_threshold_secs
    - Weight by experience count (more experience = more influence)
    - _Requirements: 5.7, 5.6_

  - [ ]* 4.3 Write property tests for policy sharing
    - **P3: Convergence** — after N rounds of FedAvg between identical agents, policies converge
    - **P5: Payload Bound** — compressed policy always < 10KB
    - _Validates: Requirements 5.4, 5.5_

- [ ] 5. Integration with coordinator
  - [ ] 5.1 Add MARL mode to coordinator
    - Extend IntegrationCoordinator with optional LocalAgent
    - In Decentralized mode: use LocalAgent instead of OnnxRuntime
    - In Hybrid mode: combine central + local adjustments
    - In Centralized mode: existing behavior unchanged
    - _Requirements: 8.1, 8.2, 8.3, 8.4_

  - [ ] 5.2 Wire policy sharing into optimizer cycle
    - After each cycle: update agent with reward, check if sharing needed
    - On share: encode policy, send via transport to selected peers
    - On receive: buffer in PolicySharer, aggregate on next cycle
    - _Requirements: 6.1, 6.2, 6.3, 6.4, 6.5_

  - [ ] 5.3 Implement mode switching
    - `set_marl_mode(mode)` — hot-switch between Centralized/Decentralized/Hybrid
    - Preserve agent state across mode switches
    - _Requirements: 8.4_

- [ ] 6. Checkpoint - Compile verification
  - Verify `cargo test --lib --no-run` passes.

- [ ] 7. New node onboarding
  - [ ] 7.1 Implement policy bootstrap for new nodes
    - New node requests current aggregated policy from any peer
    - If no peers available: start with zero Q-table (uniform priorities)
    - _Requirements: 6.3, 10.1_

- [ ] 8. Final checkpoint
  - Verify all tests pass with `cargo test --lib --no-run`.
  - Verify MARL mode produces valid priority adjustments.

## Notes

- Q-table (256×8×8 bytes = 16KB) is small enough to fit in L1 cache — inference is fast
- Delta encoding typically compresses to <5KB (most entries near zero early in training)
- Gossip fanout of 3 means O(N) total messages per round (linear scaling)
- The tabular approach is chosen over neural networks for simplicity and speed — can upgrade to small MLP later if needed
- Epsilon decays independently per agent, creating natural exploration diversity across the mesh
