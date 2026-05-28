# ResonantOS vNext — Architecture & System Guide

## Overview

ResonantOS vNext is a desktop-first distributed AI operating system. It transforms
a collection of personal devices (desktops, laptops, phones) into a unified
inference cluster — pooling RAM, VRAM, and compute to run AI models that no
single device could handle alone.

The system operates in three modes:
1. **Single PC** — one machine running models locally
2. **Local Network** — multiple devices on the same LAN collaborating
3. **Mesh Network** — devices across different networks connected via encrypted transport

---

## System Architecture

```
┌─────────────────────────────────────────────────────────────────────┐
│                         TAURI DESKTOP APP                            │
│                                                                     │
│  ┌─────────────────────────┐    ┌────────────────────────────────┐  │
│  │   FRONTEND (React/TS)   │    │      BACKEND (Rust/Tauri)      │  │
│  │                         │    │                                │  │
│  │  • Network Dashboard    │◄──►│  • Provider Service (LLM API)  │  │
│  │  • Onboarding Wizard    │    │  • Hardware Detection          │  │
│  │  • Debug Panels         │    │  • Network Optimizer (9A)      │  │
│  │  • Model Controls       │    │  • Mesh Coordinator (9B)       │  │
│  │  • Companion Dashboard  │    │  • Transport Layer (10)        │  │
│  │  • Live Data Hooks      │    │  • Split Inference (11)        │  │
│  └─────────────────────────┘    │  • Local Inference (llama.cpp) │  │
│                                 │  • RL Policy Inference (ONNX)  │  │
│                                 │  • Distributed Agents (15)     │  │
│                                 │  • Phone Companion (16)        │  │
│                                 │  • Unified Scheduler           │  │
│                                 │  • Model Catalog Registry      │  │
│                                 │  • Startup Orchestrator        │  │
│                                 │  • Event Emitters (IPC)        │  │
│                                 │  • WireGuard Transport         │  │
│                                 └────────────────────────────────┘  │
│                                 └────────────────────────────────┘  │
│                                                                     │
│  ┌─────────────────────────────────────────────────────────────┐    │
│  │                    ADDON PROCESSES                            │    │
│  │  • Browser Host (Playwright/Electron)                        │    │
│  │  • Reticulum Sidecar (Python/RNS — mesh radio transport)     │    │
│  │  • Living Archive Memory Service                             │    │
│  │  • Hermes Agent Bridge                                       │    │
│  └─────────────────────────────────────────────────────────────┘    │
│                                                                     │
│  ┌─────────────────────────────────────────────────────────────┐    │
│  │                  OFFLINE TRAINING (Python)                    │    │
│  │  • DQN Trainer (PyTorch)                                     │    │
│  │  • ONNX Exporter → Rust inference                            │    │
│  └─────────────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────────────┘
```

---

## Module Map

### Core Services (pre-existing)

| Module | Purpose |
|--------|---------|
| `provider_service` | LLM API routing (OpenAI, local, remote nodes) |
| `hardware_service` | GPU detection, VRAM management, thermal monitoring |
| `health_monitor` | System health, heartbeats, uptime tracking |
| `cost_ledger_service` | Token usage accounting, cost projections |
| `federated_memory_service` | Distributed knowledge store |
| `experience_buffer_service` | RL training data collection |
| `tool_call_tracker_service` | Agent tool usage analytics |
| `agent_evaluator_service` | Model quality scoring |
| `backtest_service` | Historical decision validation |

### Network Layer (Phase 9A — Local Network Optimizer)

| Module | Purpose |
|--------|---------|
| `network/registry` | Node discovery, heartbeats, topology |
| `network/catalog` | Model catalog (sizes, quantizations, requirements) |
| `network/demand` | Workload demand estimation from usage patterns |
| `network/solver` | Two-phase placement optimizer (knapsack + bin-packing) |
| `network/executor` | Plan execution with circuit breaker |
| `network/phone` | Phone node constraints (battery, NPU, connectivity) |
| `network/preferences` | User model preferences, family boosts |
| `network/incentive` | Pareto improvement validation |
| `network/download` | Model download orchestration |
| `network/kv_cache` | KV-cache warming for fast first inference |
| `network/lifecycle` | Node join/leave, graceful shutdown |
| `network/resilience` | Failure recovery, plan rollback |
| `network/observability` | Metrics, decision explanations |
| `network/satisfaction` | User satisfaction tracking (local, aggregate) |
| `network/simulator` | Virtual network testing harness |

### Mesh Layer (Phase 9B — Mesh Network Optimizer)

| Module | Purpose |
|--------|---------|
| `mesh/identity` | Mesh creation, cryptographic identity (Ed25519) |
| `mesh/membership` | Join/leave with invite tokens, graceful retirement |
| `mesh/trust` | 3-tier trust system (LocalOwned → Trusted → Untrusted) |
| `mesh/classifier` | Hardware classification for cross-mesh optimization |
| `mesh/accounting` | Resource contribution tracking |
| `mesh/reputation` | Reputation scoring (uptime, contribution, quality) |
| `mesh/incentive` | Free-rider detection and enforcement |
| `mesh/solver` | Mesh-wide placement (same algorithm, different constraints) |
| `mesh/consensus` | Proposal voting for mesh-wide decisions |
| `mesh/rate_limiter` | Token-weighted rate limiting per node |
| `mesh/transfer` | Secure model transfer between mesh nodes |
| `mesh/leader` | Deterministic leader election, optimizer scheduling |
| `mesh/observability` | Privacy-safe metrics (no prompt content ever shared) |

### Transport Layer (Phase 10 — Unified Mesh Transport)

| Module | Purpose |
|--------|---------|
| `transport/trait_def` | Transport trait (send/receive/metrics) |
| `transport/registry` | Multi-transport topology, path discovery |
| `transport/selector` | Best-path selection (latency, bandwidth, reliability) |
| `transport/failover` | Automatic failover between transports |
| `transport/metrics` | Per-path latency/bandwidth tracking |
| `transport/router` | Message routing across transport boundaries |
| `transport/security` | End-to-end encryption, key exchange |
| `transport/manager` | Transport lifecycle management |
| `transport/qos` | QoS: priority queue, DSCP marking, token bucket, congestion detection, fast-path |
| `transport/adapters/lan` | LAN adapter (mDNS discovery, TCP) |
| `transport/adapters/reticulum` | Reticulum adapter (LoRa, packet radio) |
| `transport/adapters/wireguard` | WireGuard adapter (encrypted tunnels) |

### Split Inference (Phase 11 — Split Inference Protocol)

| Module | Purpose |
|--------|---------|
| `inference/split/codec` | Tensor serialization (f16/f32, CRC32 integrity) |
| `inference/split/assigner` | Layer-to-node assignment (memory-aware) |
| `inference/split/coordinator` | Session management, negotiation |
| `inference/split/sync_protocol` | Token synchronization between nodes |
| `inference/split/worker` | Per-node inference execution |
| `inference/split/failure` | Failure detection, session recovery |
| `inference/split/kv_cache` | Distributed KV-cache management |
| `inference/split/backend` | Backend abstraction (llama.cpp, etc.) |
| `inference/split/protocol` | Protocol selection (tensor vs pipeline parallel) |

### RL Integration (Phase 13 — RL-Optimizer Integration)

| Module | Purpose |
|--------|---------|
| `integration/demand` | Demand signal with exponential smoothing |
| `integration/notifier` | Availability notifications to RL (retry + backoff) |
| `integration/stability` | Change controller (cooldown, hysteresis, rollback) |
| `integration/enrichment` | Feature enrichment for RL state encoding |
| `integration/coordinator` | Full cycle orchestration + RL inference step |
| `integration/metrics` | Integration health metrics |
| `integration/rl_config` | RlConfig + RlError types |
| `integration/rl_encoder` | StateEncoder (network state → 64-float vector) |
| `integration/rl_runtime` | OnnxRuntime (tract-onnx, hot-swap, graceful absence) |
| `integration/rl_decoder` | ActionDecoder (epsilon-greedy, persistence) |
| `integration/rl_metrics` | InferenceMetrics (running averages, exploration rate) |

### Distributed Agent Execution (Phase 15)

| Module | Purpose |
|--------|---------|
| `agents/dag` | DAG construction and validation for multi-step workflows |
| `agents/router` | Step-to-node routing (model + tool affinity matching) |
| `agents/executor` | Parallel step execution engine with timeout/retry |
| `agents/orchestrator` | Top-level workflow lifecycle management |
| `agents/worker` | Per-node step worker (receives and executes steps) |
| `agents/cache` | Intermediate result caching between steps |
| `agents/checkpoint` | Workflow checkpoint save/restore for fault tolerance |
| `agents/colocation` | Model+tool co-location scoring for placement optimization |
| `agents/protocol` | Wire protocol for inter-node step dispatch |
| `agents/tools` | Tool registry and capability advertisement |
| `agents/integration` | Integration with mesh solver and transport layer |

Enables agentic workloads (multi-step AI workflows with tool calls) to execute
across multiple mesh nodes. Agent plans are decomposed into a DAG of steps;
independent steps run in parallel on different nodes, each routed to the node
with the required model AND tools. Supports speculative execution, checkpointing,
and automatic retry with exponential backoff.

**Key config:** `DistributedAgentConfig` — max 10 parallel steps, 50 total steps,
30s step timeout, 2 retries, 5-min checkpoint interval, 100MB intermediate results,
0.15 co-location bonus weight.

### Unified Resource Scheduler (Phase 9A Extension)

| Module | Purpose |
|--------|---------|
| `network/solver_agents` | Agent selection, placement, and throughput estimation |
| `network/solver_contention` | Contention computation (CPU, memory, queue, speed, latency penalties) |

Extends the Phase 9A solver to handle agents alongside models in a unified
objective function. The scheduler jointly optimizes model placement utility,
agent throughput, and contention costs. Device-agnostic design — all decisions
use per-node numeric constraints (RAM, CPU, tools, battery, thermal) with no
device-type branching.

**Unified objective:** `U_total = U_model + U_agent - C_contention`

### Phone Companion App (Phase 16)

| Module | Purpose |
|--------|---------|
| `companion/types` | Core types (DeviceId, CompanionState, capabilities) |
| `companion/identity` | Mesh identity generation (Ed25519 keypair) |
| `companion/health` | Health reporting (battery, thermal, connectivity) |
| `companion/inference_runtime` | llama.cpp ARM64 inference runtime wrapper |
| `companion/layer_worker` | Split inference layer execution on phone |
| `companion/assignment` | Layer assignment acceptance/rejection logic |
| `companion/lifecycle` | App lifecycle (foreground/background/suspended) |
| `companion/npu` | NPU detection (Apple Neural Engine, Qualcomm Hexagon) |
| `companion/pairing` | QR-code pairing protocol (token, subnet verify) |
| `companion/transport_bridge` | Bridge to unified transport layer |
| `companion/commands` | Tauri mobile commands (IPC surface) |
| `companion/service` | Background service orchestration |
| `companion/property_tests` | 15 property-based tests for correctness |

Turns iOS/Android phones into active compute nodes in the ResonantOS mesh.
Built with Tauri Mobile v2, reusing existing transport, split inference, and
pairing infrastructure. Supports multi-phone split inference — a 7B model
split across 2-3 phones (each holding ≤3GB of layers) using pipeline parallel.

**React components:**
- `CompanionDashboard.tsx` — Phone node status, resource usage, active layers
- `PairingScreen.tsx` — QR code scanning and pairing flow
- `CompanionSettings.tsx` — Battery thresholds, network preferences, NPU config

### Onboarding (Phase 9C — Network Onboarding Wizard)

| Module | Purpose |
|--------|---------|
| `wizard/state` | Wizard session persistence (SQLite, 24h cleanup) |
| `wizard/discovery` | mDNS network scanner + manual entry |
| `wizard/health` | Traffic-light health classification |
| `wizard/preview` | Capacity preview (what models become available) |
| `wizard/pairing` | Phone pairing (QR code, 5-min token, subnet verify) |

### RL Policy Inference (Phase 17)

| Module | Purpose |
|--------|---------|
| `integration/rl_config` | RlConfig with all tuning parameters + RlError enum |
| `integration/rl_metrics` | InferenceMetrics with running averages |
| `integration/rl_encoder` | StateEncoder: network state → 64-float feature vector |
| `integration/rl_runtime` | OnnxRuntime: tract-onnx model load, infer, hot-swap |
| `integration/rl_decoder` | ActionDecoder: epsilon-greedy Q-value → priority adjustments |

Wires the ONNX DQN model into the optimizer cycle. Feature-gated behind
`tract-onnx`. Without the feature, the module compiles but inference returns
graceful errors. Epsilon decays from 0.3 to 0.05 over ~28 hours.

### Local Inference Backend (Phase 18)

| Module | Purpose |
|--------|---------|
| `inference/local/config` | InferenceConfig, GenerationParams, GpuLayerStrategy |
| `inference/local/model` | GpuDetector, ModelManager (load/unload GGUF) |
| `inference/local/session` | KV cache session pool (timeout, eviction) |
| `inference/local/generate` | TokenGenerator (streaming events, cancellation) |
| `inference/local/queue` | RequestQueue (FIFO per-model, concurrency limit) |

Feature-gated behind `local-inference`. Without the feature, the engine
compiles with a mock backend for testing. Supports GPU layer offloading
(Auto/None/Fixed/MaxFit strategies).

### Dashboard Data Polling (Phase 19)

| Module | Purpose |
|--------|---------|
| `ipc/emitter` | EventEmitterService (6 periodic tasks, cancellation) |
| `ipc/payloads` | All event payload structs (nodes, plans, transport, utility) |
| `ipc/delta` | Delta computation (only send changed nodes) |
| `ipc/trend` | Utility trend (improving/stable/declining) |
| `ipc/rl` | RL metrics and epsilon reset commands |

Frontend hooks: `useTauriEvent`, `useNodeStatus`, `usePlacementPlan`,
`useTransportHealth`, `useUtilityScores`, `useDownloadProgress`,
`useCompanionStatus`, `useConnectionStatus`, `DashboardProvider`.

### App Startup & Lifecycle (Phase 20)

| Module | Purpose |
|--------|---------|
| `service_registry` | Service status tracking, health summary |
| `startup` | StartupOrchestrator (10-service dependency-ordered init) |
| `optimizer_timer` | 60-second cycle timer (pause/resume/skip/metrics) |
| `shutdown` | Graceful shutdown (reverse order, 5s budget, force-exit) |

### End-to-End Integration Tests

| Module | Purpose |
|--------|---------|
| `integration_tests/harness` | TestWorld (mock transport, nodes, persistence) |
| `integration_tests/mock_transport` | Message capture, failure injection, secondary failover |
| `integration_tests/mock_node` | Desktop/laptop/phone configs with helpers |
| `integration_tests/persistence` | In-memory HashMap-backed persistence |
| `integration_tests/test_*` | 7 test files: pairing, agent, transport, optimizer, recovery, concurrent, errors |

### Model Catalog Registry

| Module | Purpose |
|--------|---------|
| `network/catalog_store` | CatalogStore (load, save, merge, user models, Ollama) |
| `assets/model_catalog.json` | Bundled catalog (30 entries, 8 families: Qwen, Llama, DeepSeek, Phi, Mistral, CodeLlama, Gemma, Whisper) |

### WireGuard Transport Adapter

| Module | Purpose |
|--------|---------|
| `transport/adapters/wireguard/config` | WireGuardConfig with validation |
| `transport/adapters/wireguard/keys` | X25519 keypair generation and management |
| `transport/adapters/wireguard/tunnel` | TunnelRegistry with state machine (Handshaking→Established→Suspect→Offline) |
| `transport/adapters/wireguard/handshake` | Key exchange with nonce and signature verification |
| `transport/adapters/wireguard/socket` | Message framing (4-byte length + payload), endpoint roaming |
| `transport/adapters/wireguard/keepalive` | Liveness detection (25s keepalive, 60s suspect, 120s offline) |
| `transport/adapters/wireguard/metrics` | Per-tunnel and aggregate metrics |

### Adaptive Segment Scheduling (CollaPipe — arXiv:2509.19855)

| Module | Purpose |
|--------|---------|
| `inference/split/segment_config` | SegmentConfig (V-parameter, safety margin, cooldown) |
| `inference/split/segment_plan` | SegmentPlan, Segment, DeviceProfile, validation |
| `inference/split/virtual_queue` | VirtualQueue (Lyapunov drift), QueueManager |
| `inference/split/segment_optimizer` | SegmentOptimizer (greedy Lyapunov heuristic, O(L×D)) |

Variable-sized model segment partitioning with Lyapunov-based dynamic scheduling.
Replaces fixed layer-boundary split inference with adaptive segments that optimize
for device heterogeneity. See `docs/COLLAPIPE.md` for full mathematical framework.

### Decentralized MARL Policies (arXiv:2504.21048)

| Module | Purpose |
|--------|---------|
| `integration/marl_config` | MarlConfig, MarlMode (Centralized/Decentralized/Hybrid) |
| `integration/marl_types` | LocalNodeState, AgentAction, CompressedPolicy |
| `integration/marl_agent` | LocalAgent (tabular Q-learning, 256×8 Q-table, <2ms) |
| `integration/marl_reward` | RewardComputer (speed + queue + success - penalties) |
| `integration/marl_sharer` | PolicySharer (gossip fanout=3, FedAvg, staleness filter) |

Per-node lightweight RL agents with gossip-based federated policy averaging.
Each node observes local state, makes local priority decisions, and shares
compressed policy updates. See `docs/MARL.md` for full mathematical framework.

### Hardware Abstraction Layer (Universal Inference Backend)

| Module | Purpose |
|--------|---------|
| `backends/types` | InferenceBackend trait, HardwareCapabilities, BackendError, TokenEvent |
| `backends/registry` | BackendRegistry (detection, selection, routing) |
| `backends/llamacpp` | llama.cpp backend (CUDA/Metal/Vulkan/CPU via GGUF) |
| `backends/ollama` | Ollama bridge (auto-discovery at localhost:11434) |
| `backends/openai_api` | OpenAI-compatible API (vLLM, TGI, tt-inference-server) |
| `backends/onnx_runtime` | ONNX Runtime (CPU, CUDA, DirectML, CoreML) |
| `backends/tenstorrent` | Tenstorrent tt-metal (Wormhole/Blackhole, tt-forge compilation) |
| `backends/ascend` | Huawei Ascend CANN (910B/310P, ATC compilation) |
| `backends/sidecar` | Community plugins (stdio JSON-RPC, any language) |
| `backends/preparation` | Model compilation pipeline (cache, progress, invalidation) |

Makes ResonantOS fully hardware-agnostic. The optimizer sees capabilities
(memory, speed, latency) — never chips. Six built-in backends cover NVIDIA,
AMD, Apple, Intel, Tenstorrent, and Huawei Ascend. Sidecar protocol enables
community plugins for any future hardware in any language.

### Headless Node Daemon

| Module | Purpose |
|--------|---------|
| `daemon/mod` | NodeDaemon orchestrator (start, shutdown, model management) |
| `daemon/config` | NodeConfig (TOML parsing, CLI overrides, low-power settings) |
| `daemon/health_reporter` | Periodic health broadcast to mesh (CPU, RAM, GPU, models) |
| `daemon/optimizer_client` | Receive and dispatch commands from mesh optimizer |
| `daemon/control_api` | Minimal localhost HTTP API (status, load, unload, shutdown) |
| `bin/node_daemon` | Binary entry point (CLI parsing, signal handling, main loop) |

Standalone binary (`resonantos-node`) that runs without GUI. Joins the mesh
as a compute node for hardware pooling. Targets: old PCs, headless servers,
phones in background mode. Same code as desktop app, different entry point.

---

## End-to-End Message Lifecycles

### Example 1: Single PC — User Sends a Chat Message

```
User types "Explain quantum computing" in the UI
    │
    ▼
Frontend sends Tauri command: provider_service_chat_completion
    │
    ▼
provider_service routes to best available model:
    • Checks local runtime (llama.cpp running locally?)
    • Checks remote providers (OpenAI API key configured?)
    • Selects model based on task type + available resources
    │
    ▼
Response streams back via Tauri event: "chat-stream-chunk"
    │
    ▼
tool_call_tracker records: model used, tokens, latency, quality
    │
    ▼
experience_buffer stores episode for RL training
    │
    ▼
cost_ledger records token cost
```

### Example 2: Local Network — Model Placement Optimization

```
Timer fires every 60 seconds (optimizer cycle)
    │
    ▼
network/registry collects heartbeats from all LAN nodes:
    • Desktop: 64GB RAM, RTX 4090 (24GB VRAM), online
    • Laptop: 16GB RAM, no GPU, online
    • Phone: 8GB RAM, Apple Neural Engine, on WiFi
    │
    ▼
network/demand computes workload demand:
    • "User runs coding tasks 60%, chat 30%, image 10%"
    • Exponential smoothing (alpha=0.3) over recent history
    │
    ▼
network/solver runs two-phase optimization:
    │
    ├─ Phase A (Knapsack): Select which models to host
    │   • Score = log(params) * quality * task_affinity
    │   • Exploration budget: 10% capacity for untried models
    │   • Constraint: total VRAM + RAM across all nodes
    │
    ├─ Phase B (Bin-Packing): Assign models to nodes
    │   • Affinity clustering (keep related models together)
    │   • Phone constraints (max 3B params, battery > 20%)
    │   • Stability weighting (prefer reliable nodes)
    │
    ▼
network/incentive validates Pareto improvement:
    • Every node must benefit vs running alone
    • If laptop loses access to models → exclude it from plan
    │
    ▼
network/executor applies the plan:
    • Diff current vs target placement
    • Load new models, unload removed ones
    • Circuit breaker: 3 failures → exclude node (exponential backoff)
    │
    ▼
network/kv_cache warms top-5 prefixes on newly loaded models
    │
    ▼
network/observability logs decision + explanation:
    "Placed deepseek-33b on Desktop (24GB VRAM fits, coding affinity 0.9)"
```

### Example 3: Mesh Network — Cross-Network Inference Request

```
User on Node A (home) asks for a 70B model (needs 40GB+ RAM)
    │
    ▼
Local optimizer: "Can't fit locally, checking mesh..."
    │
    ▼
mesh/leader checks: "Am I the mesh leader?"
    • Leader election: highest reputation + longest uptime
    • Deterministic — all nodes compute same result
    │
    ▼
mesh/solver runs mesh-wide placement:
    • Node A offers: 32GB spare RAM
    • Node B (friend's server) offers: 64GB spare RAM, RTX 3090
    • Combined: enough for 70B model split across both
    │
    ▼
mesh/trust validates: Node B is Tier 2 (Trusted)
    • Allowed to receive non-sensitive prompts
    • Sensitive prompts stay on Tier 1 (LocalOwned) only
    │
    ▼
mesh/consensus: No vote needed (routine placement, leader decides)
    │
    ▼
inference/split/assigner divides the model:
    • Layers 0-31 → Node A (local, 32GB RAM)
    • Layers 32-63 → Node B (remote, 64GB RAM)
    • Protocol: Pipeline Parallel (latency > 5ms between nodes)
    │
    ▼
transport/selector picks best path to Node B:
    • LAN adapter: not available (different network)
    • WireGuard adapter: 15ms latency, 100Mbps — SELECTED
    • Reticulum adapter: 200ms latency — fallback only
    │
    ▼
inference/split/coordinator creates session:
    • 5-token calibration warmup
    • Sync protocol: Node A processes layers 0-31, sends activations
    • Node B processes layers 32-63, sends logits back
    │
    ▼
transport/failover monitors:
    • If WireGuard drops → automatic switch to Reticulum
    • If Node B goes offline → session fails, local fallback model used
    │
    ▼
Response arrives at Node A, streamed to user
    │
    ▼
mesh/accounting records: Node B contributed 32 layers of compute
mesh/reputation updates: Node B reliability +0.01
```

### Example 4: Phone Pairing Flow

```
User opens Onboarding Wizard → selects "Pair Phone"
    │
    ▼
wizard/pairing generates:
    • 6-character token (expires in 5 minutes)
    • QR code containing: token + local IP + mesh ID
    │
    ▼
User scans QR code on phone app
    │
    ▼
Phone sends pairing request:
    • Token validation (single-use, not expired)
    • Subnet verification (same network?)
    • Capabilities report (RAM, NPU, battery, OS)
    │
    ▼
wizard/health runs checks:
    • Battery: 75% ✅ (threshold: 20%)
    • Connection: WiFi ✅ (cellular requires opt-in)
    • NPU: Apple Neural Engine Gen 5 ✅
    │
    ▼
wizard/preview shows capacity gain:
    "Adding iPhone enables: whisper-small (speech-to-text on NPU)"
    │
    ▼
User confirms → phone joins as Tier 1 (LocalOwned) node
    │
    ▼
network/registry adds phone with constraints:
    • Max model: 3B params
    • Excluded when battery < 20% or on cellular
    • Default stability: 0.6 (phones sleep/move)
```

---

## Transport Layer Detail

```
┌─────────────────────────────────────────────────────────┐
│                   TRANSPORT MANAGER                       │
│                                                         │
│  ┌─────────┐  ┌──────────────┐  ┌───────────────────┐  │
│  │   LAN   │  │  WireGuard   │  │    Reticulum      │  │
│  │ Adapter │  │   Adapter    │  │     Adapter       │  │
│  │         │  │              │  │                   │  │
│  │ • mDNS  │  │ • Encrypted  │  │ • LoRa/Radio     │  │
│  │ • TCP   │  │ • Tunnel     │  │ • Store-forward   │  │
│  │ • <1ms  │  │ • 5-50ms     │  │ • 100-5000ms     │  │
│  └────┬────┘  └──────┬───────┘  └────────┬──────────┘  │
│       │              │                    │             │
│       └──────────────┼────────────────────┘             │
│                      ▼                                  │
│              PATH SELECTOR                              │
│       (latency × reliability × bandwidth)               │
│                      │                                  │
│                      ▼                                  │
│              FAILOVER MANAGER                           │
│       (auto-switch on failure, exponential backoff)      │
│                      │                                  │
│                      ▼                                  │
│              SECURITY LAYER                             │
│       (E2E encryption, key rotation, forward secrecy)   │
└─────────────────────────────────────────────────────────┘
```

**Failover behavior:**
- Primary path fails → switch to secondary in <100ms
- 3 consecutive failures → mark path as degraded
- Exponential backoff before retry (1s, 2s, 4s, 8s...)
- Recovery: probe every 30s, restore when 3 consecutive successes

---

## RL Integration Cycle

```
┌──────────────────────────────────────────────────────────┐
│                    EVERY 60 SECONDS                        │
│                                                          │
│  1. DEMAND SIGNAL                                        │
│     • Read experience buffer (last hour)                 │
│     • Exponential smoothing (alpha=0.3)                  │
│     • Output: task_type → demand_weight map              │
│                                                          │
│  2. FEATURE ENRICHMENT                                   │
│     • Network state → normalized [0,1] features          │
│     • Node utilization, model availability, latency      │
│     • Task affinity scores from historical data          │
│                                                          │
│  3. RL POLICY QUERY                                      │
│     • DQN model (ONNX) predicts optimal action           │
│     • Action = which models to prioritize                │
│     • Epsilon-greedy exploration (decays over time)       │
│                                                          │
│  4. STABILITY CONTROLLER                                 │
│     • Cooldown: 2 cycles between changes                 │
│     • Hysteresis: 3 cycles before confirming trend       │
│     • Rollback: if quality drops >5% for 3 cycles        │
│     • Change budget: max 2 model swaps per cycle         │
│                                                          │
│  5. OPTIMIZER CYCLE                                      │
│     • Solver runs with RL-adjusted demand weights        │
│     • Pareto validation                                  │
│     • Executor applies diff                              │
│                                                          │
│  6. NOTIFICATION                                         │
│     • RL receives: "models X,Y now available"            │
│     • Retry with exponential backoff (3 attempts)        │
│     • Failure isolated: optimizer continues regardless   │
└──────────────────────────────────────────────────────────┘
```

---

## Dashboard & Debug Panels

### Main Dashboard

| Panel | Shows |
|-------|-------|
| **Utility Gauges** | Quality/Speed/Coverage scores with sparklines |
| **Topology View** | SVG network graph, zoom/pan, node status colors |
| **Model Placement** | Cards per model, which nodes host it, family colors |
| **Transport Health** | Per-path badges (latency, reliability, bandwidth) |
| **Node Contribution** | Table with expandable rows, resource usage |
| **Download Progress** | Active model downloads with cancel button |
| **Controls** | Weight sliders (quality vs speed), model preferences |

### Debug Mode (6 panels)

| Panel | Purpose |
|-------|---------|
| **Request Trace** | Waterfall view of inference requests (timing breakdown) |
| **Model Heatmap** | Which models are hot/cold across nodes |
| **Node Execution** | Per-node CPU/RAM/VRAM timeline |
| **Topology Debug** | Latency matrix, path states, failover history |
| **Optimizer Transparency** | Explain last decision, what-if scenarios |
| **Network Stats** | Aggregate throughput, error rates, uptime |

---

## Observability & Logging

### What Gets Logged

```rust
// Every optimizer cycle:
ObservabilityEvent::PlanCreated {
    plan_id, utility_scores, num_models, num_nodes,
    solver_duration_ms, explanation: "Placed 5 models across 3 nodes..."
}

// Every model load/unload:
ObservabilityEvent::ModelAction {
    action: Load|Unload, model_id, node_id, duration_ms, success
}

// Every transport event:
ObservabilityEvent::TransportEvent {
    path_id, event: Connected|Disconnected|Failover,
    latency_ms, transport_type
}

// Every inference request:
ObservabilityEvent::InferenceComplete {
    request_id, model_id, node_id, tokens, duration_ms,
    was_split: bool, num_nodes_involved
}
```

### Privacy Guarantees

- **NEVER logged**: prompt content, conversation history, user data
- **NEVER shared with mesh**: anything beyond aggregate counts
- **Local only**: satisfaction scores, usage patterns, model preferences
- **Mesh-safe**: node online/offline, model availability, capacity %

---

## Key Design Decisions

| Decision | Rationale |
|----------|-----------|
| Same algorithm local/mesh | Reduces complexity, consistent behavior |
| Pareto improvement hard constraint | No node should lose by joining |
| 10% exploration budget | Discover better models without disrupting service |
| 3-tier trust | Balance security with usability |
| Deterministic leader election | No consensus overhead for routine operations |
| Pipeline parallel for >5ms latency | Tensor parallel needs <5ms for efficiency |
| 5-token calibration warmup | Measure actual split overhead before real requests |
| Exponential smoothing (alpha=0.3) | Responsive to changes, resistant to noise |
| Stability controller (2-cycle cooldown) | Prevent oscillation between plans |
| Token-weighted rate limiting | Fair: heavy users limited by tokens, not just requests |
| Privacy by struct design | Impossible to accidentally include prompt in metrics |

---

## Data Flow Summary

```
User Activity
    │
    ├──► Experience Buffer ──► RL Training (offline, PyTorch)
    │                                    │
    │                                    ▼
    │                              ONNX Model
    │                                    │
    │                                    ▼
    ├──► Demand Signal ◄──── RL Policy (online, tract-onnx)
    │         │
    │         ▼
    │    Optimizer Cycle
    │         │
    │         ├──► Placement Plan ──► Executor ──► Model Load/Unload
    │         │
    │         └──► Explanation ──► Dashboard
    │
    ├──► Cost Ledger (token accounting)
    │
    └──► Tool Call Tracker (agent behavior analytics)
```

---

## File Counts & Scale

| Component | Files | Approx Lines |
|-----------|-------|-------------|
| Network Optimizer (9A) | 17 .rs | ~8,000 |
| Mesh Optimizer (9B) | 14 .rs | ~6,500 |
| Transport Layer (10) | 14 .rs | ~5,000 |
| Split Inference (11) | 11 .rs | ~4,500 |
| Dashboard (12) | 21 .tsx | ~5,000 |
| RL Integration (13) | 8 .rs | ~3,000 |
| Distributed Agents (15) | 11 .rs | ~5,500 |
| Phone Companion (16) | 15 .rs + 3 .tsx | ~6,000 |
| Unified Scheduler (9A ext) | 2 .rs | ~1,500 |
| Onboarding Wizard (9C) | 15 .rs/.tsx | ~4,000 |
| Schema Versioning | 3 .rs | ~800 |
| Network Simulator | 8 .rs | ~2,500 |
| **Total new code** | **~142 files** | **~52,300 lines** |

---

## Implemented Phases

All major phases are now implemented with property-based tests:

| Phase | Status | Property Tests |
|-------|--------|---------------|
| Schema Versioning | ✅ Complete | — |
| 9A: Local Network Optimizer | ✅ Complete | proptest |
| 9B: Mesh Network Optimizer | ✅ Complete | proptest |
| 9C: Onboarding Wizard | ✅ Complete | — |
| 10: Unified Mesh Transport | ✅ Complete | proptest |
| 11: Split Inference Protocol | ✅ Complete | proptest |
| 12: Network Dashboard | ✅ Complete | fast-check (frontend) |
| 13: RL-Optimizer Integration | ✅ Complete | proptest |
| 15: Distributed Agent Execution | ✅ Complete | 10 property-based tests |
| 16: Phone Companion App | ✅ Complete | 15 property-based tests |
| Unified Resource Scheduler | ✅ Complete | 19 property-based tests |

---

## How to Run

See [BUILD.md](./BUILD.md) for compilation and [RUN.md](./RUN.md) for execution.

**Quick start:**
```powershell
$env:PATH = "C:\Users\fontanesio\Documents\mingw64\bin;C:\Users\fontanesio\.cargo\bin;C:\Users\fontanesio\Documents\node-v26.1.0-win-x64;$env:PATH"
$env:CARGO_HTTP_CHECK_REVOKE = "false"
npx tauri dev
```
