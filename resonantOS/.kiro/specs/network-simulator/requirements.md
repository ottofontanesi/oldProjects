# Requirements: Network Simulator (Phase 9A.5)

## Overview

The Network Simulator is a testing harness that creates virtual nodes with configurable hardware capabilities, network latencies, and failure modes — all running in-process on a single developer machine. It enables testing the optimizer, transport, split inference, and mesh protocols at scale (10-100 virtual nodes) without requiring actual multi-machine hardware.

This is critical infrastructure: without it, the optimizer can only be tested on the developer's actual hardware (1-2 machines), making it impossible to verify behavior at scale, under failure conditions, or with heterogeneous hardware profiles.

## User Stories

### US-1: Developer Testing
As a developer working on the optimizer, I want to spin up a 10-node virtual network with diverse hardware profiles (desktop+GPU, laptop, phone, server) in a single test, so I can verify placement decisions without owning 10 machines.

### US-2: Failure Injection
As a developer, I want to simulate node failures (disconnect, slow response, crash mid-inference) during optimizer operation, so I can verify graceful degradation and re-optimization behavior.

### US-3: Scale Testing
As a developer, I want to test the mesh optimizer with 100 virtual nodes to verify it produces valid plans within the 5-second timeout, so I can be confident it works at production scale.

### US-4: Reproducible Scenarios
As a developer, I want to define network scenarios as configuration files (node profiles + latency matrix + failure schedule) and replay them deterministically, so I can create regression tests for specific bugs.

## Functional Requirements

### FR-1: Virtual Node Creation
- FR-1.1: Create virtual nodes with configurable hardware profiles: CPU (cores, clock, architecture), RAM (total, available), GPU (model, VRAM), storage, device type
- FR-1.2: Support all device types: Desktop, Laptop, Server, Phone
- FR-1.3: Virtual nodes report utilization that changes over time (configurable utilization curves)
- FR-1.4: Virtual nodes can "load" models (track which models are loaded, simulate resource consumption)
- FR-1.5: Preset hardware profiles for common configurations: "gaming-desktop" (RTX 4090, 32GB), "office-laptop" (no GPU, 16GB), "server" (A100, 64GB), "phone" (NPU, 8GB)

### FR-2: Virtual Network Topology
- FR-2.1: Configurable latency matrix between all node pairs
- FR-2.2: Configurable bandwidth between all node pairs
- FR-2.3: Support latency profiles: "same-machine" (<1ms), "LAN-ethernet" (1-3ms), "LAN-wifi" (5-20ms), "VPN" (20-100ms), "mesh-remote" (50-200ms)
- FR-2.4: Latency can change during simulation (simulate network degradation)
- FR-2.5: Bandwidth can be asymmetric (upload != download)

### FR-3: Failure Injection
- FR-3.1: Node disconnect: simulate node going offline at a specified time
- FR-3.2: Node reconnect: simulate node coming back online after disconnect
- FR-3.3: Latency spike: temporarily increase latency to a node (simulate network congestion)
- FR-3.4: Partial failure: node responds to heartbeats but fails inference requests
- FR-3.5: Slow node: node responds but with 5x normal latency (simulate thermal throttling)
- FR-3.6: Failure schedule: define a sequence of failures with timestamps for deterministic replay

### FR-4: Optimizer Integration
- FR-4.1: The simulator implements the same interfaces the real optimizer consumes (NodeRegistry, TransportService, InferenceBackend)
- FR-4.2: The optimizer code runs unmodified against the simulator — no test-specific code paths
- FR-4.3: Simulator provides deterministic time (virtual clock) so optimizer behavior is reproducible
- FR-4.4: Simulator captures all optimizer decisions for assertion in tests

### FR-5: Scenario Definition
- FR-5.1: Scenarios defined as JSON/TOML configuration files
- FR-5.2: Scenario includes: node list with profiles, latency matrix, failure schedule, demand pattern, expected assertions
- FR-5.3: Built-in scenarios: "2-node-basic", "3-node-with-phone", "10-node-heterogeneous", "100-node-mesh", "node-failure-recovery", "network-degradation"
- FR-5.4: Custom scenarios loadable from file path

### FR-6: Assertions and Verification
- FR-6.1: Assert on placement plan: which models placed where, which protocol selected
- FR-6.2: Assert on timing: re-optimization triggered within N seconds of event
- FR-6.3: Assert on constraints: all plans satisfy memory/latency/stability constraints
- FR-6.4: Assert on Pareto: all included nodes benefit
- FR-6.5: Assert on failure recovery: after node disconnect, new valid plan produced within timeout

## Non-Functional Requirements

### NFR-1: Performance
- NFR-1.1: 100-node simulation runs in <10 seconds (not real-time — virtual clock)
- NFR-1.2: Simulator memory usage <500MB for 100 nodes
- NFR-1.3: Deterministic: same scenario always produces same results

### NFR-2: Usability
- NFR-2.1: Usable from both Rust tests (proptest) and TypeScript tests (vitest)
- NFR-2.2: Clear error messages when assertions fail (show expected vs actual placement)
- NFR-2.3: Scenario files are human-readable and editable

## Correctness Properties

### Property 1: Determinism
Given the same scenario configuration and virtual clock, the simulator SHALL produce identical results across runs.

### Property 2: Interface fidelity
The simulator SHALL implement the exact same trait interfaces as the real system. Optimizer code SHALL run unmodified.

### Property 3: Constraint preservation
All correctness properties from Phase 9A (Pareto, parsimony, memory headroom, etc.) SHALL be verifiable against simulator output.
