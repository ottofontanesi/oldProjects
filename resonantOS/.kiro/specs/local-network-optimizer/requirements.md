# Requirements: Local Network Optimizer (Phase 9A)

## Overview

The Local Network Optimizer solves the Model Placement Problem (Problem P) for a user's own trusted machines: given a heterogeneous set of local nodes (desktops, laptops, servers, phones) connected via LAN/WiFi, determine the optimal model placement that maximizes the network's AI utility while satisfying hardware, latency, stability, and incentive constraints.

This is the foundational optimizer that Phase 9B (Mesh) extends with multi-user trust and privacy layers. It operates with full trust (all nodes are user-owned), real-time responsiveness (decisions in <2 seconds), and includes phone-specific optimization, model download coordination, offline-first resilience, speculative prefetch, and task-model specialization routing.

## User Stories

### US-1: Multi-Machine User
As a user with 3 home PCs (desktop with GPU, laptop, old server), I want the system to automatically distribute AI models across my machines so that I get access to larger models than any single machine could run, faster inference through parallelism, and redundancy if one machine is off.

### US-2: Phone Integration
As a user with a phone always connected to my home WiFi, I want my phone to contribute its NPU for simple inference tasks (3B models) while respecting battery constraints, so the network gains an always-on inference node for lightweight requests.

### US-3: Offline Resilience
As a user whose internet occasionally drops, I want the local network to continue functioning perfectly without internet — model inference, routing, and optimization should all work offline since all nodes are on my LAN.

### US-4: Automatic Model Management
As a user who doesn't want to manually manage which models are loaded where, I want the optimizer to automatically download, load, unload, and migrate models based on my usage patterns, so I always have the right models available without thinking about it.

### US-5: Task-Aware Routing
As a user who does both coding and creative writing, I want the system to route my coding requests to CodeLlama and my creative requests to a general model, without me having to manually select models for each task.

## Functional Requirements

### FR-1: Node Discovery and Capability Reporting
- FR-1.1: Automatically discover local nodes via mDNS/LAN broadcast on startup and continuously
- FR-1.2: Each node reports its full Phase 7 hardware profile: CPU (cores, architecture, clock, ISA extensions), RAM (total, available, DDR gen), GPU (model, VRAM, compute capability), storage (type, available space), network interfaces
- FR-1.3: Each node reports currently loaded models with their resource consumption (RAM used, VRAM used, active request count)
- FR-1.4: Each node reports real-time utilization every 10 seconds: CPU%, RAM%, GPU%, VRAM%, active inference count, queue depth
- FR-1.5: Node capability updates propagate to all other nodes within 5 seconds of change
- FR-1.6: Detect node departure (no heartbeat for 30 seconds) and trigger re-optimization
- FR-1.7: Support manual node registration for nodes not discoverable via mDNS (e.g., VPN-connected machines)

### FR-2: Phone Node Support
- FR-2.1: Detect phone nodes via mDNS with device type "phone" (iOS/Android companion app or Termux-based agent)
- FR-2.2: Report phone-specific capabilities: NPU presence and type (Apple Neural Engine, Qualcomm Hexagon), battery level, charging state, cellular vs WiFi connection
- FR-2.3: Battery-aware scheduling: do not route inference to phone when battery < 20% unless charging
- FR-2.4: NPU utilization: detect and use hardware accelerators (CoreML on iOS, NNAPI on Android) for supported model formats
- FR-2.5: Background process management: respect iOS/Android background execution limits, use push notifications to wake for inference when needed
- FR-2.6: Cellular vs WiFi routing preference: prefer WiFi for model downloads and large context inference; allow cellular for small requests only if user opts in
- FR-2.7: Phone stability scoring: lower default stability weight (phones sleep, move between networks) — treat as "best-effort" nodes

### FR-3: Model Catalog and Requirements
- FR-3.1: Maintain a catalog of available models with resource requirements: parameter count, quantization, minimum RAM, minimum VRAM, estimated tok/s per hardware class, supported inference backends (llama.cpp, ollama, vLLM, CoreML, ONNX)
- FR-3.2: Support model families with automatic size selection: given a family (e.g., Qwen2.5), select the largest variant that fits the available hardware
- FR-3.3: Track which models are currently downloaded on which nodes (avoid re-download)
- FR-3.4: Track model-task affinity scores: which models perform best on which task types (code, creative, reasoning, translation, summarization) based on benchmark data and historical RL outcomes
- FR-3.5: Support quantization variants: for each model, know all available quantizations (f16, q8, q4, q2) with their quality/speed/size tradeoffs

### FR-4: Model Download Coordination
- FR-4.1: When optimizer decides to place a model on a node that doesn't have it, coordinate the download automatically
- FR-4.2: Download from configurable sources: Ollama registry, HuggingFace Hub, local NAS/shared drive, peer node (LAN transfer)
- FR-4.3: Bandwidth-aware scheduling: don't saturate the network during active inference — throttle downloads to configurable % of available bandwidth (default 50%)
- FR-4.4: Resumable downloads: support resume after interruption (HTTP range requests)
- FR-4.5: Integrity verification: SHA-256 checksum validation after download, reject corrupted files
- FR-4.6: Progress tracking: report download progress per model per node to the dashboard
- FR-4.7: Peer-to-peer transfer: if model exists on another local node, transfer via LAN (faster than internet download)
- FR-4.8: Storage management: before downloading, verify sufficient disk space; if not, suggest models to evict

### FR-5: Workload Demand Estimation
- FR-5.1: Read historical workload distribution from Phase 4 RL inference log: which models were selected, how often, for which task types, with what quality outcomes
- FR-5.2: Compute workload_share per model type: fraction of requests over configurable time window (default 24 hours)
- FR-5.3: Compute workload_share per task type: fraction of requests by task category (code, chat, research, creative, system)
- FR-5.4: Demand forecasting: exponential smoothing over historical shares to predict next-period demand
- FR-5.5: Time-of-day patterns: detect recurring usage patterns (e.g., coding during work hours, creative in evenings) for speculative prefetch
- FR-5.6: Cold start handling: when no history exists, use uniform prior weighted by model parameter count (larger models get slightly higher prior)
- FR-5.7: Exploration budget: reserve 10% of network capacity for models not yet in demand history (fewer than 10 requests ever). Rotate exploration candidates weekly. This prevents the bootstrap problem where new models can never enter the system.
- FR-5.8: User satisfaction signal (enabled by default, all data strictly local): track regeneration rate, edit distance, conversation engagement as aggregate metrics only (never raw content). Satisfaction data never leaves the local node, never shared with mesh. Feeds into demand estimation as a quality multiplier. Can be disabled via `satisfaction_tracking_enabled: false` config.
- FR-5.8: User satisfaction signal: track regeneration rate, edit distance, conversation engagement as proxies for subjective satisfaction. Feed into demand estimation as a quality multiplier.

### FR-6: Model Specialization Routing
- FR-6.1: Maintain task-model affinity matrix: for each (task_type, model) pair, store a quality score based on historical outcomes
- FR-6.2: Affinity sources: Phase 4 RL outcomes (actual logician scores per model per task), published benchmarks (MMLU, HumanEval, etc.), user feedback
- FR-6.3: When selecting models to load, consider task distribution: if 60% of requests are code tasks, prioritize models with high code affinity
- FR-6.4: Expose affinity data to Phase 4 RL policy: RL can use affinity as a feature in its state vector for better routing decisions
- FR-6.5: Handle unknown affinity: for new models or rare task types, use parameter-count-based estimate (larger = better, as baseline)

### FR-7: Speculative Prefetch
- FR-7.1: Detect time-of-day usage patterns from historical demand data (minimum 7 days of history)
- FR-7.2: Before a predicted demand spike, pre-load models that will be needed (e.g., load CodeLlama at 8:55 AM if coding starts at 9:00 AM)
- FR-7.3: Prefetch confidence threshold: only prefetch if pattern confidence > 70% (avoid wasting resources on uncertain predictions)
- FR-7.4: Prefetch budget: limit prefetch to models that fit in currently unused capacity (never evict an actively-used model for prefetch)
- FR-7.5: Prefetch cancellation: if the predicted demand doesn't materialize within 15 minutes, unload the prefetched model

### FR-8: Optimization Objective
- FR-8.1: Maximize weighted utility function: U = w1×Quality + w2×Speed + w3×Mass
- FR-8.2: Quality metric (log-scaled + measured quality, workload-weighted):
  ```
  Quality = Σ(effective_quality_i × workload_share_i)
  
  effective_quality_i = 0.3 × normalized_params_i + 0.5 × actual_quality_score_i + 0.2 × task_affinity_match_i
  normalized_params_i = log2(params_i) / log2(max_network_params)
  actual_quality_score_i = avg(logician_scores) from Phase 2 (fallback: benchmark estimate)
  task_affinity_match_i = Σ(task_affinity(model_i, t) × task_share(t))
  ```
  Log-scaling ensures small models remain visible (3B=0.26, 7B=0.46, 14B=0.62 on a 70B-capable network). Actual quality scores mean a well-tuned 7B can outscore a generic 14B.
- FR-8.3: Speed metric (workload-weighted aggregate throughput):
  ```
  Speed = Σ(estimated_tok_s_i × workload_share_i) / max_possible_tok_s
  ```
  Normalized by the theoretical maximum throughput if all capacity were used optimally.
- FR-8.4: Mass metric (total loaded intelligence as fraction of capacity):
  ```
  Mass = Σ(params_i for all loaded models) / max_loadable_params
  ```
- FR-8.5: Weights w1, w2, w3 are user-configurable with defaults (0.4, 0.4, 0.2)
- FR-8.6: Task-affinity bonus: add bonus to utility when a loaded model has high affinity for the dominant task type

### FR-9: Placement Constraints
- FR-9.1: Parsimony — if a model fits on k nodes, do not use k+n nodes (minimize unnecessary splits). Penalty for each additional node used beyond minimum.
- FR-9.2: Stability — only place models on nodes with rolling 24h uptime > configurable threshold (default 90% for desktops, 50% for phones)
- FR-9.3: Hop distance — inter-node latency for split models must be below protocol threshold (tensor parallel: <5ms, pipeline parallel: <50ms)
- FR-9.4: Hardware compatibility — for split models, inference speed variance across participating nodes must be < 2× (slowest can't be >2× slower than fastest)
- FR-9.5: Memory headroom — never allocate >90% of a node's available RAM/VRAM (leave room for OS, other apps, and inference KV-cache growth)
- FR-9.6: Phone constraints — phone nodes limited to models ≤3B parameters, only when battery >20% or charging, only on WiFi (unless user opts in to cellular)

### FR-10: Incentive Constraint (Pareto Improvement)
- FR-10.1: For each node, compute utility_with_network vs utility_alone (what the node could achieve independently)
- FR-10.2: The placement plan must satisfy: every participating node gains at least one of: access to larger models, faster inference (via offloading simple tasks), or more model variety
- FR-10.3: If Pareto improvement is impossible for a node (it's already optimal alone), exclude it from the plan — it operates independently
- FR-10.4: Report per-node incentive: human-readable explanation of what each node gains ("Your laptop gains access to 14B model running on your desktop's GPU")

### FR-11: Offline-First Resilience
- FR-11.1: The optimizer operates entirely on LAN — no internet dependency for optimization, routing, or inference
- FR-11.2: When internet is unavailable: model downloads pause (resume when back), but all loaded models continue serving inference
- FR-11.3: When a node disconnects from LAN: optimizer re-solves within 30 seconds, redistributes that node's models to remaining nodes if capacity allows
- FR-11.4: When all remote nodes are unreachable: seamlessly fall back to single-node mode (current machine only), no user intervention needed
- FR-11.5: State persistence: optimizer state (current plan, node registry, model catalog) persisted to local DB, survives app restart
- FR-11.6: No cloud dependency: the entire local network optimizer stack works without any internet connection, ever

### FR-12: KV-Cache Sharing
- FR-12.1: Compute SHA-256 hash of prompt prefixes (system prompt + first N tokens) as cache keys
- FR-12.2: Each node maintains a local KV-cache registry: which prompt prefixes are cached, for which model, with what size
- FR-12.3: Cache-aware routing: when routing a request, prefer nodes that already have the relevant prefix cached (skip expensive prefill computation)
- FR-12.4: Cache hit reporting: track hit rate per model per node, feed into optimizer (nodes with high cache hit rates are better placement targets for that model)
- FR-12.5: Cache eviction: LRU policy when cache exceeds configurable size limit per node (default: 50% of available RAM beyond model weight)
- FR-12.6: Cross-node cache awareness: nodes advertise their cached prefixes to the optimizer so routing decisions can exploit cache locality
- FR-12.7: Cache warming on model load: when a model is loaded on a new node, proactively compute KV-cache for the top-5 most frequently hit prefixes (from the global cache registry). This bootstraps the new node's cache and prevents the cold-start chicken-and-egg problem where new nodes never get routed to because they have no cache.
- FR-12.8: Cache warming runs in background after model load completes, does not block inference availability

### FR-13: User Preferences
- FR-13.1: Model family preferences: "I prefer Gemma" → optimizer weights Gemma variants higher in selection
- FR-13.2: Model vetoes: "Never use model X" → hard constraint, model excluded from all plans
- FR-13.3: Task-model overrides: "Always use CodeLlama for code tasks" → hard constraint for that task type
- FR-13.4: Utility weight adjustment: user can shift quality/speed/mass balance via UI sliders
- FR-13.5: No-preference default: users who set nothing get the optimizer's pure optimal choice
- FR-13.6: Transparency: when user's preference is overridden, explain why ("Gemma-7B unavailable, using Qwen-7B which is 15% faster for your workload")

### FR-14: Execution and Lifecycle
- FR-14.1: Optimizer runs every 5 minutes (configurable) AND on-demand when: node joins, node leaves, model download completes, user changes preferences
- FR-14.2: Produce a placement plan: list of (model, node_assignment, protocol, instance_count) tuples with utility scores
- FR-14.3: Execute plan changes incrementally: compute minimal diff from current state, apply changes one at a time
- FR-14.4: Graceful migration: before unloading a model, drain active requests (wait up to 30s for in-flight requests to complete)
- FR-14.5: Notify Phase 4 RL Policy when available model set changes (within 1 second of plan execution)
- FR-14.6: Solver timeout: if optimization takes >2 seconds, return best partial solution found so far

### FR-15: Observability
- FR-15.1: Report current placement plan with per-model metrics (tok/s, utilization, node assignment, cache hit rate)
- FR-15.2: Report network-level utility scores: Quality, Speed, Mass, Total
- FR-15.3: Report per-node contribution and incentive status (what each node gains)
- FR-15.4: Report download progress for models being fetched
- FR-15.5: Log all placement decisions with reasoning to audit trail (why each model was placed where)
- FR-15.6: Report speculative prefetch activity (what was prefetched, whether prediction was correct)
- FR-15.7: Explain Placement API: `explain_placement(model_id) -> Vec<PlacementFactor>` returns the scoring breakdown for any model's placement decision — which nodes were considered, their individual scores (speed, stability, cache, headroom), why the winner won, and why alternatives lost. Queryable from dashboard and CLI.

## Non-Functional Requirements

### NFR-1: Performance
- NFR-1.1: Optimization solve time < 2 seconds for networks up to 10 nodes and 20 model candidates
- NFR-1.2: Node discovery latency < 3 seconds on LAN
- NFR-1.3: Zero impact on active inference during optimization (runs on background thread)
- NFR-1.4: Model download does not degrade inference latency by more than 10% (bandwidth throttling)
- NFR-1.5: KV-cache lookup adds < 1ms to routing decision

### NFR-2: Reliability
- NFR-2.1: Graceful degradation when nodes disconnect (redistribute within 30 seconds)
- NFR-2.2: No data loss during migration (in-flight requests complete before model unload)
- NFR-2.3: Optimizer failure does not affect currently loaded models (fail-safe: keep current placement)
- NFR-2.4: Corrupted model download detected and rejected (never serve inference from corrupted weights)
- NFR-2.5: Phone node disconnection (sleep, out of WiFi range) handled without error propagation

### NFR-3: Modularity
- NFR-3.1: Same optimization algorithm reusable by Mesh Optimizer (Phase 9B) with different constraint parameters
- NFR-3.2: Clean interface between optimizer and execution layer (optimizer produces plans, executor applies them)
- NFR-3.3: Pluggable solver (can swap greedy heuristic for more sophisticated solver later)
- NFR-3.4: Phone support is optional — system works identically without any phone nodes
- NFR-3.5: KV-cache sharing is optional — system works without it, just with slightly higher latency

### NFR-4: Privacy and Security
- NFR-4.1: All inter-node communication on LAN encrypted (TLS or equivalent)
- NFR-4.2: No telemetry or data sent outside the local network
- NFR-4.3: Model weights stored encrypted at rest (optional, user-configurable)

## Correctness Properties

### Property 1: Utility monotonicity
For any valid placement plan, adding a capable node to the network SHALL NOT decrease the total utility score. More resources = same or better outcome.

### Property 2: Parsimony enforcement
For any model in the placement plan, if the model fits entirely on a single node with sufficient memory headroom, it SHALL NOT be split across multiple nodes.

### Property 3: Constraint satisfaction
Every placement plan produced by the optimizer SHALL satisfy ALL hard constraints: memory headroom ≤90%, latency thresholds per protocol, stability thresholds, phone battery/connectivity constraints.

### Property 4: Pareto improvement
For every node included in the placement plan, utility_with_network ≥ utility_alone. No node is made worse off by participating.

### Property 5: Quality metric bounds
The Quality metric SHALL always be in [0.0, 1.0] for any valid network configuration and workload distribution.

### Property 6: Speed metric correctness
The Speed metric SHALL equal the workload-weighted sum of per-instance tokens/sec normalized by maximum possible throughput, and SHALL be in [0.0, 1.0].

### Property 7: Placement plan completeness
Every model instance in the plan SHALL have: a valid node assignment, a selected parallelism protocol, resource allocation that fits within assigned node capacity, and an estimated tok/s value.

### Property 8: Migration safety
During plan execution, no inference request SHALL be dropped or interrupted. Requests in flight complete on the old placement before migration proceeds.

### Property 9: Incentive transparency
For each node in the plan, the optimizer SHALL produce a human-readable explanation of what benefit the node gains from network participation.

### Property 10: Determinism
Given identical inputs (node capabilities, workload history, model catalog, preferences), the optimizer SHALL produce the same placement plan.

### Property 11: Offline independence
The optimizer SHALL produce valid placement plans and execute them without any internet connectivity. Only model downloads require internet.

### Property 12: Phone safety
Phone nodes SHALL never receive inference requests when battery < 20% (unless charging) or when on cellular (unless user opted in).

### Property 13: Download integrity
Every model loaded for inference SHALL have passed SHA-256 integrity verification. Corrupted downloads SHALL be rejected and retried.

### Property 14: Speculative prefetch budget
Speculative prefetch SHALL never evict an actively-used model. Prefetch only uses currently-idle capacity.

### Property 15: Cache-aware routing correctness
When a node has a cached KV prefix matching the incoming request, routing SHALL prefer that node (all else being equal) to avoid redundant prefill computation.
