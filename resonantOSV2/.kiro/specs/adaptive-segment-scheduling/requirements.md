# Requirements: Adaptive Segment Scheduling (CollaPipe Integration)

## Overview

Replace fixed layer-boundary split inference with variable-sized segment partitioning and Lyapunov-based dynamic scheduling. Enables adaptive model splitting across heterogeneous devices with provable stability guarantees.

## Functional Requirements

### 1. Variable Segment Partitioning

- 1.1 The system SHALL partition model layers into variable-sized segments (not fixed 1-layer boundaries)
- 1.2 Segment sizes SHALL be computed based on each device's current available memory (RAM/VRAM)
- 1.3 Segment sizes SHALL adapt when device conditions change (battery drain, thermal throttle, new device joins)
- 1.4 A segment SHALL contain 1 to N contiguous layers (never split a single layer across devices)
- 1.5 The total segments SHALL cover all model layers exactly once (no gaps, no overlaps)
- 1.6 Segment assignment SHALL respect device memory constraints: segment_memory ≤ device_available_memory × 0.9

### 2. Dynamic Segment Scheduling (DSSDA)

- 2.1 The scheduler SHALL use Lyapunov optimization to determine segment assignments
- 2.2 The scheduler SHALL maintain a virtual queue per device tracking resource deficit
- 2.3 The scheduler SHALL minimize a drift-plus-penalty objective: minimize latency while keeping queues stable
- 2.4 The scheduler SHALL produce assignments that are provably stable under long-term constraints
- 2.5 The scheduler SHALL adapt within one optimizer cycle (≤60 seconds) when conditions change
- 2.6 The V-parameter (latency-stability tradeoff) SHALL be configurable (default: 10.0)

### 3. Heterogeneous Device Awareness

- 3.1 The scheduler SHALL account for device compute speed (tokens/second per layer)
- 3.2 The scheduler SHALL account for inter-device communication latency
- 3.3 The scheduler SHALL account for device battery level (phones: reduce segments when battery < 30%)
- 3.4 The scheduler SHALL account for thermal state (reduce segments when throttling detected)
- 3.5 The scheduler SHALL handle devices joining/leaving mid-session gracefully

### 4. Pipeline Efficiency

- 4.1 The system SHALL support micro-batching within segments (configurable batch size 1-4)
- 4.2 Pipeline bubble ratio SHALL be < 20% (time wasted waiting for upstream segments)
- 4.3 The system SHALL overlap computation and communication where possible
- 4.4 End-to-end latency SHALL improve by ≥15% vs fixed-layer assignment for heterogeneous networks

### 5. Convergence and Stability

- 5.1 The Lyapunov drift SHALL be bounded: E[L(t+1) - L(t)] ≤ B - ε·Σ|Q_i(t)| for some B, ε > 0
- 5.2 Virtual queues SHALL remain bounded (never grow unbounded over time)
- 5.3 The system SHALL converge to a stable assignment within 5 optimizer cycles after a topology change
- 5.4 If no feasible assignment exists (total model > total cluster memory), the system SHALL report infeasibility

### 6. Integration with Existing Split Inference

- 6.1 The adaptive scheduler SHALL produce assignments compatible with the existing `LayerWorker` interface
- 6.2 The existing `SplitCoordinator` SHALL accept variable-sized segment assignments
- 6.3 Activation tensors between segments SHALL use the existing codec (f16/f32, CRC32)
- 6.4 Session management SHALL remain unchanged (create/continue/destroy)
- 6.5 Fallback: if Lyapunov scheduler fails, fall back to the existing fixed-layer assigner

### 7. Observability

- 7.1 Each scheduling decision SHALL log: segment sizes, device assignments, queue lengths, drift value
- 7.2 Pipeline bubble ratio SHALL be reported per inference session
- 7.3 Segment rebalancing events SHALL be emitted as Tauri events
- 7.4 Latency improvement vs baseline SHALL be tracked and reported

## Non-Functional Requirements

### 8. Performance

- 8.1 Scheduling computation SHALL complete in < 50ms for up to 10 devices
- 8.2 Memory overhead of the scheduler SHALL be < 1MB
- 8.3 No additional network round-trips for scheduling (uses existing node state)

### 9. Configuration

- 9.1 All parameters SHALL have sensible defaults (V=10, micro_batch=2, memory_safety_margin=0.9)
- 9.2 Configuration SHALL be hot-reloadable without restart

## Correctness Properties

- P1: Segment Coverage — all layers assigned exactly once (no gaps, no overlaps)
- P2: Memory Feasibility — no device assigned more memory than available × safety margin
- P3: Queue Boundedness — virtual queues remain bounded over any sequence of scheduling decisions
- P4: Monotonic Improvement — latency never increases after rebalancing (or reverts within 1 cycle)
- P5: Graceful Degradation — device removal produces valid (possibly suboptimal) assignment within 1 cycle
