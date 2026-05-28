# Tasks: Network Simulator (Phase 9A.5)

## Task Instructions
- Test: Vitest 3.2 + fast-check (TS), proptest (Rust)
- No Rust toolchain reliably available — write correct code without compiling
- This is testing infrastructure — must be implemented BEFORE Phase 9A integration tests

## Tasks

- [x] 1. Core Simulator Infrastructure
  - [x] 1.1 Create `src-tauri/src/network/simulator/mod.rs` module structure with submodules: node, network, clock, failure, scenario, log, presets
  - [x] 1.2 Implement `VirtualClock`: controllable time source with `now()`, `advance(duration)`, `set(timestamp)`. All simulator components use this instead of system clock.
  - [x] 1.3 Implement `NetworkSimulator` struct: holds nodes, network, clock, failure injector, decision log. Constructor takes `SimulationScenario`.
  - [x] 1.4 Implement `advance_time(secs)`: advance clock, apply failure events, update utilization curves
  - [x] 1.5 Implement `run_optimizer_cycle()`: construct SolverInputs from virtual state, call real optimizer, record decision
  - [x] 1.6 Write tests: clock advances correctly; advance_time triggers scheduled events; optimizer runs against virtual state without modification

- [x] 2. Virtual Nodes
  - [x] 2.1 Implement `VirtualNode` struct: holds NodeCapabilities, current utilization, loaded models, stability score, online status
  - [x] 2.2 Implement hardware presets: GamingDesktop (RTX 4090, 32GB), OfficeLaptop (no GPU, 16GB), Server (A100, 64GB), Phone (NPU, 8GB), OldDesktop (GTX 1060, 16GB)
  - [x] 2.3 Implement utilization curves: Constant, Sine (oscillating), Step (discrete changes at timestamps)
  - [x] 2.4 Implement `to_node_state()`: convert virtual node to the same `NodeState` struct the real registry produces
  - [x] 2.5 Implement model loading simulation: track loaded models, update RAM/VRAM usage, report via `LoadedModelInfo`
  - [x] 2.6 Write tests: presets produce valid NodeCapabilities; utilization curves produce correct values at each timestamp; model loading correctly updates resource usage

- [x] 3. Virtual Network
  - [x] 3.1 Implement `VirtualNetwork` struct: latency matrix + bandwidth matrix between all node pairs
  - [x] 3.2 Implement `measure_latency(from, to)`: return configured RTT (with optional jitter)
  - [x] 3.3 Implement `get_bandwidth(from, to)`: return configured bandwidth
  - [x] 3.4 Implement dynamic latency: support changing latency at runtime (for failure injection)
  - [x] 3.5 Implement `VirtualNetwork` as `TransportService` trait: `send()` simulates delivery with configured latency, `topology()` returns virtual topology
  - [x] 3.6 Write tests: latency measurements match configuration; bandwidth reports match; dynamic changes take effect immediately

- [x] 4. Failure Injection
  - [x] 4.1 Implement `FailureInjector`: holds scheduled `FailureEvent` list sorted by timestamp
  - [x] 4.2 Implement `apply_events(current_time)`: apply all events whose timestamp <= current_time
  - [x] 4.3 Implement Disconnect: mark node offline, remove from registry
  - [x] 4.4 Implement Reconnect: mark node online, re-add to registry
  - [x] 4.5 Implement LatencySpike: temporarily change latency in virtual network
  - [x] 4.6 Implement SlowResponse: multiply node's compute time by configured factor
  - [x] 4.7 Implement PartialFailure: node responds to heartbeat but fails inference
  - [x] 4.8 Write tests: events fire at correct virtual time; disconnect makes node unreachable; reconnect restores node; latency spike affects routing decisions

- [x] 5. Scenario Loading
  - [x] 5.1 Implement `ScenarioLoader`: parse TOML scenario files into `SimulationScenario` struct
  - [x] 5.2 Implement scenario validation: check all node_ids in latency matrix exist, check no duplicate nodes, check failure targets exist
  - [x] 5.3 Create built-in scenarios as embedded TOML: "2-node-basic", "3-node-with-phone", "10-node-heterogeneous", "100-node-mesh", "node-failure-recovery", "network-degradation"
  - [x] 5.4 Implement custom scenario loading from file path
  - [x] 5.5 Write tests: all built-in scenarios parse successfully; invalid scenarios produce clear errors; custom scenarios load from disk

- [x] 6. Decision Log and Assertions
  - [x] 6.1 Implement `DecisionLog`: records every PlacementPlan produced by the optimizer during simulation
  - [x] 6.2 Implement assertion helpers: `satisfies_pareto()`, `satisfies_memory_headroom()`, `satisfies_parsimony()`, `satisfies_phone_constraints()`
  - [x] 6.3 Implement timing assertions: `reoptimized_within(event_time, max_delay)`
  - [x] 6.4 Implement placement assertions: `model_placed_on(model_id, node_id)`, `model_uses_protocol(model_id, protocol)`
  - [x] 6.5 Implement comparison: `utility_improved_vs(previous_plan)`
  - [x] 6.6 Write tests: assertions correctly detect violations; all built-in scenarios produce valid plans

- [x] 7. Integration with Property Tests
  - [x] 7.1 Implement proptest strategy for random scenarios: `arb_scenario(nodes: 1..20, models: 1..10)` generates valid random scenarios
  - [x] 7.2 Implement property: "any random scenario produces a plan satisfying all constraints"
  - [x] 7.3 Implement property: "adding a node never decreases utility" (monotonicity)
  - [x] 7.4 Implement property: "node failure triggers re-optimization within 30s virtual time"
  - [x] 7.5 Implement property: "100-node scenario solves within 5 seconds wall-clock time"
  - [x] 7.6 Write benchmark: measure solver time for 10, 50, 100 node scenarios
