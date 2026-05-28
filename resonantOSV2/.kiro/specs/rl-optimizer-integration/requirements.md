# Requirements: RL-Optimizer Integration (Phase 13)

## Overview

The RL-Optimizer Integration connects the existing Phase 4 Unified RL Policy (which routes individual requests to models) with the Phase 9A/9B Network Optimizers (which decide which models to load where). These two systems are complementary:

- **RL Policy**: "Given loaded models, which model handles THIS request?" (per-request, real-time)
- **Network Optimizer**: "Given demand patterns, which models SHOULD BE loaded?" (periodic, infrastructure)

The integration creates a bidirectional feedback loop: RL feeds demand signals to the optimizer, and the optimizer feeds model availability changes back to the RL policy. Stability mechanisms (cooldown, hysteresis, rollback) prevent oscillation between the two systems.

## Key Design Decisions

- RL Policy feeds optimizer: workload_share from inference log (which models get selected, how often, for which tasks)
- Optimizer feeds RL Policy: model set changed notification within 1 second
- Feedback loop stability: cooldown (2 cycles before re-moving a model), hysteresis (demand must drop <5% for 3 cycles before unload), rollback (revert if outcomes degrade 3 cycles)
- Training data enrichment: network topology features in RL state vector, optimizer utility in reward signal
- Clean separation: optimizer runs as periodic job, not inside RL training loop

## User Stories

### US-1: Demand-Responsive Loading
As a user whose work patterns change throughout the day (coding in morning, creative writing in evening), I want the optimizer to automatically load CodeLlama in the morning and a general model in the evening, based on what the RL policy has been selecting.

### US-2: Stable Model Availability
As a user, I want the system to not constantly load/unload models based on short-term demand fluctuations — if I use a model once, it shouldn't disappear 5 minutes later, only to be reloaded when I need it again.

### US-3: Quality Improvement Over Time
As a user, I want the RL policy to get better at routing requests as it learns which models work best for which tasks, and I want the optimizer to respond by keeping those high-performing models loaded.

### US-4: Graceful Adaptation
As a user, when the optimizer changes which models are available (loads a new model, unloads an old one), I want the RL policy to adapt smoothly without a period of degraded performance.

## Functional Requirements

### FR-1: Demand Signal (RL → Optimizer)
- FR-1.1: RL inference log records every inference request: model selected, task type, tokens generated, duration, quality score (from Phase 2 scoring)
- FR-1.2: Optimizer reads this log to compute workload_share per model and per task type over configurable time window (default 24h)
- FR-1.3: Demand signal includes: model_shares, task_shares, total_requests, quality_per_model, speed_per_model
- FR-1.4: Signal computed fresh before each optimization cycle (every 5 min local, 15 min mesh)
- FR-1.5: Cold start: when no inference history exists, optimizer uses uniform prior (all models equally likely)

### FR-2: Availability Signal (Optimizer → RL)
- FR-2.1: When optimizer executes a plan change (model loaded/unloaded/migrated), notify RL policy within 1 second
- FR-2.2: Notification includes: full list of currently available models with their capabilities (model_id, node, estimated_tok_s, task_affinity)
- FR-2.3: RL policy updates its candidate set immediately upon notification (no stale routing to unloaded models)
- FR-2.4: If notification fails (RL service temporarily unavailable), retry with exponential backoff (max 3 retries)
- FR-2.5: RL policy must handle model set changes gracefully mid-episode (don't crash if a model disappears)

### FR-3: Feedback Loop Stability
- FR-3.1: **Cooldown**: After loading a model, it cannot be unloaded for at least 2 optimization cycles (10 minutes local, 30 minutes mesh). Prevents rapid load/unload oscillation.
- FR-3.2: **Hysteresis**: A model is only unloaded if its workload_share drops below 5% for 3 consecutive cycles. Prevents unloading due to temporary demand dips.
- FR-3.3: **Rollback**: If overall utility (quality + speed) degrades for 3 consecutive cycles after a plan change, automatically revert to the previous plan. Prevents bad optimization decisions from persisting.
- FR-3.4: **Dampening**: Workload shares are exponentially smoothed (alpha=0.3) to reduce noise from short-term fluctuations.
- FR-3.5: **Change budget**: Maximum 2 model changes (load or unload) per optimization cycle. Prevents wholesale plan replacement.

### FR-4: Training Data Enrichment
- FR-4.1: Add network topology features to RL state vector:
  - Number of available models
  - Total network capacity (RAM, VRAM)
  - Average model quality score across loaded models
  - Network utilization percentage
- FR-4.2: Add optimizer utility to RL reward signal:
  - Bonus reward when RL selects a model that the optimizer has placed optimally (high tok/s, low latency)
  - Penalty when RL selects a model on a congested node (high queue depth)
- FR-4.3: RL training pipeline (Phase 4 Python) incorporates these enriched features in next training cycle
- FR-4.4: Feature normalization: all added features normalized to [0, 1] range for stable training

### FR-5: Coordination Protocol
- FR-5.1: Optimizer runs as independent periodic job (not inside RL training loop)
- FR-5.2: Sequence per cycle:
  1. Read RL inference log → compute demand
  2. Read node capabilities → compute resources
  3. Solve Problem P → produce plan
  4. Check stability constraints (cooldown, hysteresis)
  5. Execute plan changes
  6. Notify RL policy → "model set changed"
- FR-5.3: RL policy adapts within 1 episode of notification (typically <1 request)
- FR-5.4: No locking between RL and optimizer — they operate on different time scales
- FR-5.5: If optimizer and RL disagree (optimizer unloads a model RL wants), optimizer wins (infrastructure authority)

### FR-6: Observability
- FR-6.1: Log every demand signal computation with: model_shares, task_shares, changes from previous cycle
- FR-6.2: Log every availability notification with: models added, models removed, RL acknowledgment status
- FR-6.3: Track stability metrics: cooldown activations, hysteresis holds, rollback events
- FR-6.4: Track adaptation speed: time from plan change to RL policy convergence (measured by routing quality)
- FR-6.5: Dashboard integration: show demand/availability signals in Network Ops Dashboard

## Non-Functional Requirements

### NFR-1: Performance
- NFR-1.1: Demand signal computation: <500ms for 24h of inference history (up to 10,000 entries)
- NFR-1.2: Availability notification delivery: <1 second from plan execution to RL acknowledgment
- NFR-1.3: RL adaptation: <1 request after notification (immediate candidate set update)
- NFR-1.4: No impact on inference latency from integration overhead

### NFR-2: Stability
- NFR-2.1: System reaches steady state within 3 optimization cycles after startup
- NFR-2.2: No oscillation: same model not loaded and unloaded within 30 minutes
- NFR-2.3: Rollback restores previous known-good state within 1 cycle
- NFR-2.4: Integration failure (RL or optimizer crash) does not affect the other system

### NFR-3: Modularity
- NFR-3.1: Integration is a thin coordination layer — RL and optimizer remain independently functional
- NFR-3.2: Disabling integration reverts to: optimizer uses uniform demand, RL uses static model list
- NFR-3.3: Integration can be enabled/disabled at runtime without restart

## Correctness Properties

### Property 1: Notification timeliness
Every model set change SHALL be notified to the RL policy within 1 second of plan execution. Stale model references in RL routing SHALL NOT persist beyond 1 notification cycle.

### Property 2: Cooldown enforcement
A model loaded in cycle N SHALL NOT be unloaded before cycle N+2. This is a hard constraint that the optimizer SHALL NOT violate regardless of demand changes.

### Property 3: Hysteresis enforcement
A model SHALL only be unloaded if its workload_share has been below 5% for 3 consecutive cycles. A single cycle of low demand SHALL NOT trigger unloading.

### Property 4: Rollback correctness
If utility degrades for 3 consecutive cycles after a plan change, the system SHALL revert to the pre-change plan. The reverted plan SHALL be the exact plan that was active before the degrading change.

### Property 5: Change budget
No single optimization cycle SHALL make more than 2 model changes (loads + unloads combined). Excess changes SHALL be deferred to subsequent cycles.

### Property 6: Demand signal freshness
The demand signal used by the optimizer SHALL be computed from inference log data no older than the configured time window (default 24h). Stale demand data SHALL NOT influence optimization decisions.

### Property 7: RL independence
If the optimizer service crashes or becomes unavailable, the RL policy SHALL continue routing requests to the last-known model set without degradation. The RL policy SHALL NOT depend on optimizer availability for real-time operation.

### Property 8: Optimizer independence
If the RL policy crashes or becomes unavailable, the optimizer SHALL continue operating using the last-known demand signal. The optimizer SHALL NOT depend on RL availability for plan computation.

### Property 9: Feature normalization
All features added to the RL state vector from the optimizer SHALL be normalized to [0, 1]. Out-of-range values SHALL be clamped, not cause errors.

### Property 10: No oscillation
The same model SHALL NOT be loaded and unloaded (or vice versa) within a 30-minute window under stable demand conditions. Cooldown + hysteresis together SHALL prevent this.
