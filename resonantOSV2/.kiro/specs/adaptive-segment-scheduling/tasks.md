# Implementation Plan: Adaptive Segment Scheduling

## Overview

Replace fixed layer-boundary split inference with variable-sized segment partitioning using Lyapunov-based dynamic scheduling. Integrates CollaPipe concepts into the existing split inference infrastructure.

**Build verification:** `cargo test --lib --no-run` from `src/resonantos-vnext/src-tauri`.

## Tasks

- [x] 1. Configuration and types
  - [ ] 1.1 Create `inference/split/segment_config.rs` with `SegmentConfig`
    - All fields with defaults (v_parameter=10.0, memory_safety_margin=0.9, micro_batch_size=2, max_segments_per_device=4, min_layers_per_segment=1, rebalance_cooldown_secs=120, queue_decay=0.95)
    - Implement Default trait
    - _Requirements: 9.1, 9.2_

  - [ ] 1.2 Create `inference/split/segment_plan.rs` with plan types
    - Define `SegmentPlan`, `Segment`, `DeviceProfile`, `DeviceObservation`
    - Define `SchedulerError` enum (Infeasible, Timeout, NoDevices, InvalidModel)
    - _Requirements: 1.1, 1.4, 1.5, 3.1_

  - [ ] 1.3 Create `inference/split/virtual_queue.rs` with `VirtualQueue`
    - Define queue struct with memory, latency, compute deficit tracking
    - Implement `update()` method with exponential decay
    - Implement `drift()` computation
    - _Requirements: 2.2, 2.3, 5.1, 5.2_

  - [ ] 1.4 Register new submodules in `inference/split/mod.rs`
    - Add pub mod declarations for segment_config, segment_plan, segment_optimizer, virtual_queue
    - _Requirements: 6.1_

- [ ] 2. Segment optimizer (Lyapunov scheduling)
  - [ ] 2.1 Implement `inference/split/segment_optimizer.rs` with `SegmentOptimizer`
    - `new(config)` — create optimizer with empty queues
    - `optimize(model_layers, layer_memory, layer_compute, devices)` — compute optimal assignment
    - Implement greedy heuristic: assign layers left-to-right minimizing drift-plus-penalty
    - Validate: all layers covered, memory feasible, segments contiguous
    - _Requirements: 1.1, 1.2, 1.3, 1.5, 1.6, 2.1, 2.4, 5.4_

  - [ ] 2.2 Implement queue update logic
    - `update_queues(observations)` — update virtual queues from observed performance
    - Apply exponential decay (queue_decay factor)
    - Bound queues to prevent unbounded growth
    - _Requirements: 2.2, 2.3, 5.1, 5.2_

  - [ ] 2.3 Implement rebalance detection
    - `needs_rebalance(devices)` — check if conditions changed enough to warrant rebalancing
    - Respect cooldown period (rebalance_cooldown_secs)
    - Trigger on: device join/leave, memory change >20%, thermal throttle
    - _Requirements: 2.5, 3.5_

  - [ ] 2.4 Implement pipeline latency estimation
    - Compute per-segment latency: compute_ms + transfer_ms
    - Pipeline latency = max(segment_latencies) + bubble overhead
    - Bubble ratio = idle_time / total_time
    - _Requirements: 4.1, 4.2, 4.3, 4.4_

  - [ ]* 2.5 Write property tests for segment optimizer
    - **P1: Segment Coverage** — all layers assigned exactly once for any valid input
    - **P2: Memory Feasibility** — no device exceeds available memory × safety margin
    - **P3: Queue Boundedness** — queues remain bounded after N update cycles
    - _Validates: Requirements 1.5, 1.6, 5.1, 5.2_

- [ ] 3. Device profiling
  - [ ] 3.1 Implement device profile collection
    - Collect available memory, compute speed, latency, battery, thermal from existing node state
    - Convert existing `NodeState` / `RlNodeFeatures` to `DeviceProfile`
    - Handle missing data with conservative defaults
    - _Requirements: 3.1, 3.2, 3.3, 3.4_

- [ ] 4. Integration with split coordinator
  - [ ] 4.1 Wire SegmentOptimizer into the optimizer cycle
    - After solver produces placement plan, run segment optimizer for split models
    - Apply segment plan to SplitCoordinator
    - Emit rebalance events
    - _Requirements: 6.1, 6.2, 6.5, 7.3_

  - [ ] 4.2 Implement fallback to fixed assignment
    - If Lyapunov scheduler fails or times out, use existing fixed-layer assigner
    - Log fallback event
    - _Requirements: 6.5_

- [ ] 5. Observability
  - [ ] 5.1 Emit scheduling metrics
    - Log segment sizes, queue lengths, drift value, bubble ratio
    - Emit Tauri event on rebalance
    - Track latency improvement vs baseline
    - _Requirements: 7.1, 7.2, 7.3, 7.4_

- [ ] 6. Final checkpoint
  - Verify `cargo test --lib --no-run` passes.
  - Verify segment optimizer produces valid plans for test scenarios.

## Notes

- The greedy heuristic is O(L × D) — fast enough for real-time scheduling
- Lyapunov V-parameter controls the latency-stability tradeoff: higher V = more aggressive optimization but potentially less stable
- The algorithm converges to optimal within O(1/V) of the theoretical minimum latency
- Virtual queues are a mathematical construct — they don't correspond to actual message queues
