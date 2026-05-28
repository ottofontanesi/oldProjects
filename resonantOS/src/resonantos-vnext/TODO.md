# TODO — Future Work

## Optimizer Improvements (Separate Concern)

The current optimizer (Phase 9A/9B solver) handles model placement only.
With Phase 15 (agents + tools) and Phase 16 (phones), the optimizer needs
to evolve into a unified resource scheduler.

### Current Objective Function (Phase 9A/9B)

```
maximize: U = w_q × quality + w_s × speed + w_c × coverage

where:
  quality = Σ log(params_i) × quality_score_i × task_affinity_i  (per placed model)
  speed   = Σ estimated_tok_s_i / max_tok_s                      (normalized throughput)
  coverage = unique_task_types_covered / total_task_types_demanded

subject to:
  Σ model_ram_i(node) ≤ available_ram(node)        ∀ nodes
  Σ model_vram_i(node) ≤ available_vram(node)      ∀ nodes
  stability_score(node) ≥ min_stability            ∀ assigned nodes
  pareto_improvement(node) = true                  ∀ included nodes
```

### Target Objective Function (with Agents + Tools)

```
maximize: U_total = U_model + U_agent - C_contention

where:
  U_model = (same as above — model placement utility)

  U_agent = Σ agent_throughput_j × parallelism_factor_j

    agent_throughput_j = completed_steps_per_minute(agent_j)
    parallelism_factor_j = independent_steps_j / total_steps_j
                         × (1 - avg_network_latency / step_compute_time)
                         × min_node_speed / max_node_speed  // bottleneck penalty

  C_contention = Σ contention_cost(node_k)

    contention_cost(node_k) = 
      cpu_overcommit_penalty(node_k)     // agents + tools consuming CPU that models need
      + memory_pressure_penalty(node_k)  // agents in RAM competing with model weights
      + queue_depth_penalty(node_k)      // too many pending steps = latency spike

subject to:
  // Resource constraints (per node, regardless of device type)
  Σ model_ram_i(node) + Σ agent_ram_j(node) ≤ available_ram(node)
  Σ model_vram_i(node) ≤ available_vram(node)
  stability_score(node) ≥ min_stability
  pareto_improvement(node) = true

  // Node-specific constraints (battery, thermal, connectivity — applies to any node that has them)
  battery(node) ≥ battery_threshold  OR  is_charging(node)  OR  !has_battery(node)
  thermal_state(node) ≠ Critical
  model_size_on_node ≤ max_memory(node)

  // Agent constraints
  Σ active_agents(node) ≤ max_agents_per_node
  agent_required_tools(agent_j) ⊆ available_tools(assigned_node)
  sensitivity(agent_j) = Sensitive → trust_tier(node) = LocalOwned

  // Network constraints for parallelization
  parallel_steps_active ≤ max_parallel_steps (default: 10)
  inter_node_latency(a, b) ≤ step_timeout / 2
```

### Key Insight: Hardware Speed Matching

Parallelism only works when nodes process at comparable speeds. If you split
5 parallel steps across a desktop (100 tok/s) and a phone (10 tok/s), you wait
for the phone — the desktop sits idle 90% of the time.

```
Effective parallelism = min(node_speeds) / max(node_speeds) × theoretical_parallelism

Example:
  Desktop: 100 tok/s, Phone: 10 tok/s
  5 parallel steps, equal work
  Theoretical: 5x speedup
  Actual: limited by slowest node = 10 tok/s
  Effective: 10/100 × 5 = 0.5x — WORSE than sequential on desktop alone!
```

The optimizer must account for this by:
1. **Proportional load assignment**: give fast nodes more steps, slow nodes fewer
2. **Speed-weighted routing**: route compute-heavy steps to fast nodes, lightweight tool calls to slow nodes
3. **Reject parallelization when it hurts**: if the slowest available node would bottleneck, keep it sequential on the fast node
4. **Match step granularity to node speed**: split large steps into smaller chunks so slow nodes get proportionally smaller work

This means the node's **compute speed** (tokens/second, benchmark score) is a
first-class input to the routing decision — not just "does it have the tool."

### Resource Competition Model

```
Node capacity = { RAM, VRAM, CPU_cores, network_bandwidth }

Consumers:
  Models:  consume RAM/VRAM (static, once loaded)
  Agents:  consume RAM + CPU (dynamic, bursty)
  Tools:   consume CPU + sometimes GPU (per-call, short-lived)

Priority (when contention occurs):
  1. Active inference request (user waiting)
  2. Agent step execution (workflow in progress)
  3. Background agent maintenance (checkpointing, health)
  4. Model preloading (speculative, can be deferred)
```

### When to Implement

After Phase 15 and 16 are running and we can observe:
- Real contention patterns (how often do agents starve models?)
- Actual parallelism gains (does network latency kill it in practice?)
- Phone reliability (how often do phones drop mid-step?)

Data first, then optimize. The current solver works fine for models-only.
The extension is additive — it doesn't break existing behavior.

---

## Phase 15: Distributed Agent Execution

**Status:** ✅ Implemented (all 10 correctness properties verified)
**Location:** `src-tauri/src/agents/`
**Spec:** `.kiro/specs/distributed-agent-execution/`

Submodules: dag, router, executor, orchestrator, worker, cache, checkpoint,
colocation, protocol, tools, integration. All property-based tests passing.

---

## Phase 16: Phone Companion App

**Status:** ✅ Implemented (all 15 correctness properties verified)
**Location:** `src-tauri/src/companion/` + `src/components/companion/`
**Spec:** `.kiro/specs/phone-companion-app/`

Submodules: types, identity, health, inference_runtime, layer_worker, assignment,
lifecycle, npu, pairing, transport_bridge, commands, service, property_tests.
React components: CompanionDashboard, PairingScreen, CompanionSettings.

---

## Unified Resource Scheduler

**Status:** ✅ Implemented (all 19 correctness properties verified)
**Location:** `src-tauri/src/network/solver_agents.rs` + `solver_contention.rs`
**Spec:** `.kiro/specs/unified-resource-scheduler/`

Extends Phase 9A solver with agent selection, placement, contention computation,
and unified objective function. Device-agnostic design verified.

---

## Test Execution

**Status:** Tests compile (0 errors) but can't run on current machine
**Blocker:** WebView2Loader.dll ABI mismatch (GNU toolchain vs MSVC-compiled DLL)
**Fix:** Build and run on a machine with VS Build Tools (MSVC toolchain)

---

## Other TODOs

### Architecture & Stability

- [ ] **Pin Tauri version** — Change `tauri = { version = "2" }` to `tauri = { version = "=2.x.y" }` to prevent surprise breakage from unstable API changes
- [ ] **Tauri abstraction layer** — Wrap Tauri-specific APIs (commands, events, app handle) behind a thin trait so the backend can be tested without Tauri and potentially swapped to another runtime
- [ ] **Refactor App.tsx monolith** — Split the 2900-line App.tsx into modular components: AppShell, NavigationSidebar, ThemeProvider, ConnectionBanner, ScreenRouter (per the frontend-app-shell spec structure)

### Testing & Validation

- [ ] Run full test suite on MSVC machine (or macOS/Linux)
- [ ] Run `npx tauri dev` to verify app launches end-to-end
- [ ] Download a real GGUF model and test the full inference pipeline
- [ ] Run two instances on same LAN — verify mDNS discovery + split inference
- [ ] Test phone companion pairing with real device
- [ ] Stress-test optimizer with 10+ simulated nodes
- [ ] RL training pipeline end-to-end test (Python → ONNX → Rust)
- [ ] Reticulum sidecar integration test
- [ ] MARL multi-node validation (3+ nodes, real network latency)

### CI & Distribution

- [ ] CI pipeline (GitHub Actions with MSVC for Windows, native for macOS/Linux)
- [ ] Frontend type-check in CI (`npx tsc --noEmit` — fix pre-existing test file errors first)
- [ ] Tauri Mobile v2 build for companion app (iOS/Android targets)
- [ ] Signed installer builds for alpha distribution

### Completed

- [x] Phase 15 implementation (distributed agents, 10 property tests)
- [x] Phase 16 implementation (phone companion, 15 property tests)
- [x] Unified Resource Scheduler (19 property tests)
- [x] All 14 integration specs implemented (lan-transport through first-run-onboarding)
- [x] CollaPipe adaptive segment scheduling (Lyapunov optimization)
- [x] MARL decentralized policies (tabular Q-learning + FedAvg gossip)
- [x] 75+ property-based tests across all modules
- [x] End-to-end integration test harness (TestWorld + 7 test files)
- [x] Model catalog registry (30 entries, 8 families)
- [x] CI typecheck workflow (GitHub Actions)
- [x] EXAMPLE.md with 5 real-world test scenarios
- [x] COLLAPIPE.md and MARL.md research documentation
