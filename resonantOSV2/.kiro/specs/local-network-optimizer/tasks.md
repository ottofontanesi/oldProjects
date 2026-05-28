# Tasks: Local Network Optimizer (Phase 9A)

## Task Instructions
- Test: Vitest 3.2 + fast-check (TS), proptest (Rust)
- No Rust toolchain reliably available — write correct code without compiling
- Property-based tests validate correctness properties from requirements

## Tasks

- [x] 1. Node Discovery and Registry
  - [x] 1.1 Create `src-tauri/src/network/mod.rs` module structure with pub mod declarations for all submodules (discovery, registry, catalog, demand, solver, executor, download, kv_cache, preferences, incentive, observability)
  - [x] 1.2 Implement `src-tauri/src/network/discovery.rs`: mDNS scanner using `mdns-sd` crate — `scan_lan(timeout)` returns `Vec<DiscoveredNode>`, registers `_resonantos._tcp.local` service on startup
  - [x] 1.3 Implement heartbeat protocol: each node sends `NodeUtilization` every 10 seconds via UDP broadcast on port 9741, detect departure after 30s silence
  - [x] 1.4 Implement `src-tauri/src/network/registry.rs`: `NodeRegistry` struct with `register()`, `unregister()`, `update_utilization()`, `all_nodes()`, `online_nodes()`, thread-safe via `Arc<RwLock<>>`
  - [x] 1.5 Implement manual node registration: `register_manual(address: String)` that connects via TCP to a specified IP, exchanges capabilities, adds to registry
  - [x] 1.6 Implement stability score computation: rolling 24h uptime ratio from heartbeat history, stored as `Vec<(DateTime, bool)>` with 10s granularity
  - [x] 1.7 Implement latency measurement: periodic ping/pong between all node pairs every 60 seconds, store in `latency_to_peers: HashMap<NodeId, LatencyMeasurement>`
  - [x] 1.8 Write property tests: node departure detection fires within 30-35s of last heartbeat; stability score always in [0.0, 1.0]; latency measurements are symmetric within 20%

- [x] 2. Phone Node Support
  - [x] 2.1 Extend `NodeCapabilities` with `PhoneInfo` struct: os, npu type, battery_percent, is_charging, connection_type
  - [x] 2.2 Implement phone detection in discovery: parse mDNS TXT record for `device_type=phone`, extract phone-specific fields
  - [x] 2.3 Implement battery-aware filtering: `is_phone_available(phone_info, config) -> bool` checks battery > threshold AND (charging OR above threshold) AND (wifi OR cellular_opt_in)
  - [x] 2.4 Implement phone stability scoring: default stability weight 0.5 for phones (vs 0.9 for desktops), configurable
  - [x] 2.5 Write property tests: phone never marked available when battery < 20% and not charging; phone never available on cellular unless opt-in enabled

- [x] 3. Model Catalog
  - [x] 3.1 Implement `src-tauri/src/network/catalog.rs`: `ModelCatalog` struct with CRUD operations, persistence to SQLite via existing Phase 1 DB infrastructure
  - [x] 3.2 Define `ModelEntry` with all fields: model_id, family, parameter_count_b, quantization, requirements, performance estimates, task_affinity, supported_backends, download_sources, checksum
  - [x] 3.3 Implement model family auto-selection: `best_variant_for_capacity(family, available_ram, available_vram) -> Option<ModelEntry>` selects largest quantization that fits
  - [x] 3.4 Implement task-affinity tracking: `update_affinity(model_id, task_type, quality_score)` with exponential moving average
  - [x] 3.5 Implement model download tracking: `downloaded_on(model_id) -> Vec<NodeId>`, `mark_downloaded(model_id, node_id)`, `mark_removed(model_id, node_id)`
  - [x] 3.6 Seed initial catalog with common models: Qwen2.5 (3B/7B/14B), Gemma3 (2B/7B), Llama3.2 (3B/8B), CodeLlama (7B/13B) with realistic performance estimates per hardware class
  - [x] 3.7 Write property tests: best_variant always returns model that fits in given capacity; task_affinity always in [0.0, 1.0]; no model returned if none fits

- [x] 4. Workload Demand Estimation
  - [x] 4.1 Implement `src-tauri/src/network/demand.rs`: `DemandEstimator` struct that reads Phase 4 RL inference log
  - [x] 4.2 Implement `compute_workload_demand(time_window_hours)`: group log entries by model_id and task_type, compute shares as fractions
  - [x] 4.3 Implement exponential smoothing: `smooth(current_shares, previous_shares, alpha=0.3)` for dampened demand signal
  - [x] 4.4 Implement cold start: when no history exists, return uniform prior weighted by parameter count
  - [x] 4.5 Implement time-of-day pattern detection: group entries by (weekday, hour), find dominant model per slot, emit PrefetchSignal if frequency > 0.7 across 7+ days
  - [x] 4.6 Implement forecast confidence: based on history depth and variance — more history + lower variance = higher confidence
  - [x] 4.7 Write property tests: all shares sum to 1.0; smoothed shares converge to true distribution over time; cold start shares sum to 1.0; prefetch signals only emitted with >= 7 days history and >= 70% confidence

- [x] 5. Optimization Solver — Phase A (Model Selection)
  - [x] 5.1 Implement `src-tauri/src/network/solver.rs`: `OptimizerSolver` trait and `GreedySolver` struct implementing it
  - [x] 5.2 Implement utility scoring: `compute_utility(model, demand, preferences) -> f64` combining quality_contribution + speed_contribution + mass_contribution + affinity_bonus + preference_boost
  - [x] 5.3 Implement quality_contribution (log-scaled + measured): `effective_quality = 0.3 * log2(params)/log2(max_params) + 0.5 * actual_quality_score + 0.2 * task_affinity_match`. actual_quality_score from Phase 2 logician scores (fallback: benchmark estimate). This ensures small models remain visible (3B=0.26 not 0.002).
  - [x] 5.4 Implement speed_contribution: `(estimated_tok_s * workload_share) / max_possible_tok_s`
  - [x] 5.5 Implement mass_contribution: `params / max_loadable_params`
  - [x] 5.6 Implement greedy knapsack: sort candidates by utility descending, add until capacity exhausted, respect vetoes and task-model overrides as hard constraints
  - [x] 5.7 Implement desired instance count: based on workload_share and estimated throughput per instance
  - [x] 5.8 Implement exploration budget: reserve 10% of capacity for models with <10 requests in history. Score unexplored models by task-affinity match to current task distribution + novelty bonus. Load one exploration model per cycle. Rotate weekly if no organic demand materializes.
  - [x] 5.9 Write property tests: selected models never exceed 90% of total network RAM/VRAM; vetoed models never selected; utility scores always in [0.0, 1.0]; task-model overrides always included if capacity allows; exploration budget never exceeds 10% of capacity; at least one unexplored model attempted if capacity allows

- [x] 6. Optimization Solver — Phase B (Node Assignment)
  - [x] 6.1 Implement affinity clustering: `build_affinity_clusters(nodes, latency_matrix)` groups nodes by latency thresholds (<5ms = tensor parallel, <50ms = pipeline parallel)
  - [x] 6.2 Implement single-node placement scoring: `score_single_placement(model, node)` considering speed, stability, KV-cache locality, headroom
  - [x] 6.3 Implement split placement scoring: `score_split_placement(model, cluster)` with parsimony penalty per extra node
  - [x] 6.4 Implement constraint validation: `satisfies_constraints(model, target)` checking memory headroom ≤90%, stability threshold, hardware speed variance <2x, phone constraints
  - [x] 6.5 Implement bin-packing assignment: sort models by size descending, for each find best placement (prefer single-node, fall back to split), update remaining capacity
  - [x] 6.6 Implement placement plan construction: assemble `PlacementPlan` with all placements, utility scores, and protocol assignments
  - [x] 6.7 Write property tests: parsimony — models fitting single node never split; memory headroom never exceeds 90% on any node; phone nodes never assigned models >3B; hardware speed variance <2x for all splits

- [x] 7. Incentive Validation (Pareto Check)
  - [x] 7.1 Implement `src-tauri/src/network/incentive.rs`: `compute_utility_alone(node, catalog)` — best utility this node achieves independently
  - [x] 7.2 Implement `compute_utility_with_network(node, plan)` — utility considering access to all models in the plan
  - [x] 7.3 Implement Pareto validation: for each node, if utility_with_network < utility_alone, exclude node and re-solve
  - [x] 7.4 Implement benefit determination: classify benefits as AccessToLargerModels, FasterInference, MoreModelVariety, TaskOffloading
  - [x] 7.5 Implement human-readable explanation generation: plain language description of what each node gains
  - [x] 7.6 Write property tests: every included node has utility_with >= utility_alone; excluded nodes have valid reason; explanation is non-empty for every included node

- [x] 8. Plan Executor
  - [x] 8.1 Implement `src-tauri/src/network/executor.rs`: `PlanExecutor` struct with `compute_diff(current, target) -> PlanDiff`
  - [x] 8.2 Implement incremental execution: apply changes one at a time (download → load → migrate → unload), track progress
  - [x] 8.3 Implement graceful migration: drain active requests (wait up to 30s for in-flight to complete) before unloading
  - [x] 8.4 Implement RL notification: after plan execution, call `notify_model_set_changed()` within 1 second
  - [x] 8.5 Implement fail-safe: if new plan utility is <80% of current, keep current plan unchanged
  - [x] 8.6 Implement solver timeout: if solve takes >2 seconds, return best partial solution found so far
  - [x] 8.7 Implement executor circuit breaker: track consecutive execution failures per node. After 3 failures, exclude node from optimizer's eligible set with exponential backoff (5min, 15min, 45min, max 2h). Re-include after cooldown expires and next execution succeeds.
  - [x] 8.8 Write property tests: diff is minimal (no unnecessary changes); migration never drops in-flight requests (simulated); RL notification sent within 1s of execution; excluded nodes never appear in new plans; circuit breaker resets after successful execution post-cooldown

- [x] 9. Download Coordinator
  - [x] 9.1 Implement `src-tauri/src/network/download.rs`: `DownloadCoordinator` struct with `start_download()`, `progress()`, `cancel()`
  - [x] 9.2 Implement source selection: peer node (LAN) > local NAS > Ollama registry > HuggingFace Hub, based on availability and bandwidth
  - [x] 9.3 Implement bandwidth throttling: token bucket algorithm, dynamic limit (30% during active inference, 100% when idle)
  - [x] 9.4 Implement resumable downloads: track bytes_downloaded, use HTTP Range headers for resume after interruption
  - [x] 9.5 Implement SHA-256 integrity verification: stream-compute hash during download, reject and retry on mismatch
  - [x] 9.6 Implement peer-to-peer transfer: if model exists on another local node, transfer via TCP on port 9742
  - [x] 9.7 Implement storage management: check available space before download, suggest evictions if insufficient
  - [x] 9.8 Write property tests: corrupted downloads always rejected (inject bit flips); bandwidth never exceeds configured limit; peer transfer preferred when peer has model

- [x] 10. Speculative Prefetch
  - [x] 10.1 Implement prefetch scheduler: read PrefetchSignals from demand estimator, schedule downloads for models predicted to be needed within 10 minutes
  - [x] 10.2 Implement idle capacity check: only prefetch if target node has free capacity (never evict active models)
  - [x] 10.3 Implement cancellation timer: if predicted demand doesn't materialize within 15 minutes, unload prefetched model
  - [x] 10.4 Implement confidence threshold: only act on signals with confidence >= 0.70
  - [x] 10.5 Write property tests: prefetch never evicts active models; prefetch cancelled after 15 min of no demand; only signals with confidence >= 0.70 trigger prefetch

- [x] 11. KV-Cache Registry
  - [x] 11.1 Implement `src-tauri/src/network/kv_cache.rs`: `KvCacheRegistry` struct tracking prefix hashes per node per model
  - [x] 11.2 Implement prefix hashing: SHA-256 of first 256 tokens, truncated to 16 bytes
  - [x] 11.3 Implement cache-aware routing hint: `best_node_for_prefix(prefix_hash, model_id) -> Option<NodeId>`
  - [x] 11.4 Implement LRU eviction: when cache exceeds 50% of free RAM, evict least-recently-hit entries
  - [x] 11.5 Implement cross-node cache advertisement: nodes broadcast their cached prefixes periodically (every 30s)
  - [x] 11.6 Implement cache warming on model load: when a model is loaded on a new node, fetch top-5 most-hit prefix hashes from global registry, run silent prefill for each to populate the new node's KV-cache. Run in background, don't block inference availability. Solves cold-start routing bias.
  - [x] 11.7 Write property tests: cache size never exceeds configured limit; LRU evicts oldest entries first; routing prefers cache-hit nodes; warming populates cache for top prefixes within 60s of model load

- [x] 12. User Preferences
  - [x] 12.1 Implement `src-tauri/src/network/preferences.rs`: `UserPreferences` struct with persistence to SQLite
  - [x] 12.2 Implement model family preferences: weight boost applied during Phase A scoring
  - [x] 12.3 Implement model vetoes: hard exclusion from all plans
  - [x] 12.4 Implement task-model overrides: hard constraint forcing specific model for specific task type
  - [x] 12.5 Implement utility weight adjustment: user-configurable w1/w2/w3 with defaults (0.4, 0.4, 0.2)
  - [x] 12.6 Implement preference change trigger: any preference update triggers immediate re-optimization
  - [x] 12.7 Write property tests: vetoed models never appear in any plan; overrides always respected if capacity allows; weights always sum to 1.0 after normalization

- [x] 13. Optimizer Lifecycle and Triggers
  - [x] 13.1 Implement optimizer main loop: periodic timer (5 min default) + event-driven triggers (node join/leave, download complete, preferences changed)
  - [x] 13.2 Implement event debouncing: batch events within 2-second window before triggering solve
  - [x] 13.3 Implement `OptimizerEvent` enum and event channel (tokio mpsc)
  - [x] 13.4 Implement state persistence: save current plan, node registry, demand history to SQLite on every plan change
  - [x] 13.5 Implement startup recovery: load persisted state, re-discover nodes, resume partial downloads, re-run optimizer
  - [x] 13.6 Write integration test: simulate node join → verify re-optimization triggered within 2s debounce window

- [x] 14. Observability
  - [x] 14.1 Implement `src-tauri/src/network/observability.rs`: `OptimizerMetrics` struct with all network-level and per-node metrics
  - [x] 14.2 Implement audit trail: log every placement decision with reasoning to append-only audit table in SQLite
  - [x] 14.3 Implement Tauri commands: `get_network_state`, `trigger_optimization`, `update_preferences`, `get_node_incentives`, `register_node`, `get_download_progress`, `get_kv_cache_stats`
  - [x] 14.4 Implement Explain Placement API: `explain_placement(model_id)` returns per-node scoring breakdown (speed_score, stability_score, cache_score, headroom_score, total) for all candidate nodes, showing why the winner was chosen and why alternatives lost. Exposed as Tauri command and available in dashboard.
  - [x] 14.5 Write integration test: full cycle — discover nodes → compute demand → solve → execute → verify metrics updated; explain_placement returns valid breakdown for every model in plan

- [x] 15. Offline-First Resilience
  - [x] 15.1 Implement offline detection: monitor internet connectivity, pause downloads when offline, resume when back
  - [x] 15.2 Implement node disconnection handling: on departure, re-solve within 30s with remaining nodes
  - [x] 15.3 Implement single-node fallback: when all remote nodes unreachable, seamlessly operate in local-only mode
  - [x] 15.4 Write property tests: optimizer produces valid plans with zero internet; node departure triggers re-solve within 30s; single-node mode produces valid (degenerate) plan

- [x] 16. End-to-End Integration Tests
  - [x] 16.1 Test: 2-node network (desktop + laptop), verify model placed on GPU node, utility > single-node utility
  - [x] 16.2 Test: 3-node network with phone, verify phone gets ≤3B model only, battery constraints respected
  - [x] 16.3 Test: node departure mid-operation, verify re-optimization and graceful degradation
  - [x] 16.4 Test: cold start with no history, verify valid plan produced with uniform prior
  - [x] 16.5 Test: preference change (add veto), verify immediate re-optimization excludes vetoed model
  - [x] 16.6 Test: determinism — same inputs produce same plan (run solver twice, compare)

- [x]* 17. User Satisfaction Signal (enabled by default, all data strictly local)
  - [ ]* 17.1 Implement satisfaction tracker: record regeneration events (user re-asks same question), edit distance (how much user modifies AI output), conversation engagement (time-to-next-request). Store only aggregate metrics, never raw content.
  - [ ]* 17.2 Implement satisfaction score computation: `(1 - regen_rate) * 0.4 + (1 - edit_dist) * 0.3 + engagement * 0.2 + explicit_feedback * 0.1`
  - [ ]* 17.3 Implement privacy enforcement: all satisfaction data stored locally only, never shared with mesh, never leaves the node, only aggregate numbers stored (not prompts or edits)
  - [ ]* 17.4 Implement config flag: `satisfaction_tracking_enabled: true` by default. When disabled, no data collected, no storage used, no CPU overhead.
  - [ ]* 17.5 When enabled, feed satisfaction score into demand estimator as quality multiplier for model selection
  - [ ]* 17.6 Write property tests: satisfaction data never appears in mesh messages or transport payloads; only aggregate numbers stored (no raw text); score always in [0.0, 1.0]; disabling flag stops all collection immediately
