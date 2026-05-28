# Tasks: Hardware Stability

## Phase 1: Hardware Detection and Profiling

- [x] 1.1 Create `src-tauri/src/hardware_service.rs` with all struct definitions: HardwareProfile, HardwareClass, CpuProfile, MemoryProfile, GpuProfile, StorageProfile, NetworkProfile, ProbeResults
- [x] 1.2 Implement CPU detection using `sysinfo` and `raw-cpuid` crates: core count, architecture, clock speed, instruction set extensions (AVX2, AVX-512, NEON)
- [x] 1.3 Implement memory detection: total RAM, available RAM, swap, DDR generation estimation, bandwidth estimation
- [x] 1.4 Implement GPU detection using `nvml-wrapper` (NVIDIA) with fallback to system queries for AMD/Metal: model, VRAM, compute capability, driver version, framework support
- [x] 1.5 Implement storage detection: available space on data directory, storage type classification (NVMe/SSD/HDD via rotational flag), sequential I/O probe (10MB write/read)
- [x] 1.6 Implement network detection: enumerate interfaces, classify types, loopback bandwidth probe, internet connectivity check (DNS resolve)
- [x] 1.7 Implement `detect_hardware` orchestrator: run all detectors with 2s individual timeouts, assemble HardwareProfile, complete within 5s total
- [x] 1.8 Implement profile persistence: save to `$APPDATA/resonantos-vnext/hardware_profile.json`, load on subsequent startups, compare for changes
- [x] 1.9 Write unit tests for each detector with mocked system calls, timeout enforcement, and profile assembly

## Phase 2: Classification and Timeout Calibration

- [x] 2.1 Implement `classify_hardware`: match against 6 HardwareClass criteria (gpu-workstation, cpu-workstation, gpu-server, cpu-server, embedded, container-restricted)
- [x] 2.2 Implement container detection: check for /.dockerenv, cgroup limits, /proc/1/cgroup contents
- [x] 2.3 Implement headless detection: check DISPLAY/WAYLAND_DISPLAY env vars, TTY availability
- [x] 2.4 Implement `default_timeout_profile`: return calibrated TimeoutProfile per HardwareClass with all 6 timeout values
- [x] 2.5 Implement runtime timeout adjustment: track p90 latency per operation, increase timeout by 50% when p90 > 80% of limit for 10 consecutive ops
- [x] 2.6 Implement runtime timeout tightening: decrease by 25% when p99 < 20% of limit for 100 consecutive ops
- [x] 2.7 Implement manual class override: read from config file, validate against known classes, log override
- [x] 2.8 Write property-based tests (proptest) for Properties 1, 2, 3: detection completeness, classification determinism, timeout positivity

## Phase 3: Model Compatibility Matrix

- [x] 3.1 Define `ModelRequirements` struct: model_id, parameter_count, quantization, min_vram_mb, min_ram_mb, min_compute_capability
- [x] 3.2 Implement `compute_compatibility_matrix`: for each known model, classify as native-gpu/offloaded/cpu-only/incompatible based on hardware profile
- [x] 3.3 Implement speed estimation: use probe results to estimate tokens/sec per model per compatibility class (linear scaling from probe benchmark)
- [x] 3.4 Implement incompatibility explanation: generate human-readable reason string ("requires 24GB VRAM, only 8GB available")
- [x] 3.5 Implement alternative suggestions: when incompatible, suggest smaller quantization or different model that fits
- [x] 3.6 Write property-based tests (proptest) for Property 4: compatibility matrix correctness

## Phase 4: Resource Envelope Management

- [x] 4.1 Define ResourceEnvelope struct and default allocations per HardwareClass per workload type (interactive-inference, tool-execution, background-training, system-overhead)
- [x] 4.2 Implement resource monitoring: track CPU/RAM/GPU utilization per envelope using process-level accounting
- [x] 4.3 Implement backpressure mechanism: when envelope memory limit approached, queue new requests rather than allowing OOM
- [x] 4.4 Implement dynamic rebalancing: when interactive is idle, allow background to borrow freed resources; reclaim within 1s on interactive demand
- [x] 4.5 Expose resource utilization via IPC: per-envelope current usage, total available, borrowing state
- [x] 4.6 Write property-based tests (proptest) for Property 5: resource envelope sum never exceeds 100%

## Phase 5: Thermal Monitoring and GPU Memory

- [x] 5.1 Implement thermal monitoring: read CPU/GPU temperature sensors at 10s intervals using platform-specific APIs (Linux: /sys/class/thermal, macOS: IOKit, Windows: WMI)
- [x] 5.2 Implement ThermalState classification: nominal (<70C), warm (70-85C), throttling (>85C or clock reduced), critical (>95C)
- [x] 5.3 Implement thermal adaptation: on throttling reduce concurrency 50% + increase timeouts 2x; on critical pause background + limit to 1 concurrent
- [x] 5.4 Implement GPU VRAM monitoring: poll available VRAM at 5s intervals via NVML or equivalent
- [x] 5.5 Implement VRAM allocation registry: track which models occupy VRAM, support priority-based eviction
- [x] 5.6 Implement VRAM pre-check: before model load, verify sufficient VRAM available, reject with shortfall amount if not
- [x] 5.7 Implement gpu-memory-pressure event: emit when available VRAM < 15%, block new model loads
- [x] 5.8 Write property-based tests (proptest) for Property 6: thermal state ordering

## Phase 6: Hardware Probes and Adaptation

- [x] 6.1 Implement CPU inference probe: matrix multiply benchmark calibrated to estimate tokens/sec
- [x] 6.2 Implement GPU inference probe: load small ONNX model (from Phase 4 tract infrastructure), run forward pass, measure throughput
- [x] 6.3 Implement disk I/O probe: 10MB sequential write + read, measure throughput
- [x] 6.4 Implement memory bandwidth probe: large array copy, measure GB/s
- [x] 6.5 Implement probe orchestrator: run all probes within 15s total, persist results, skip on subsequent startups unless triggered
- [x] 6.6 Implement adaptation strategies: map each HardwareEvent type to response (gpu-memory-pressure → evict, thermal-throttling → reduce, disk-full → pause, network-degradation → local-only)
- [x] 6.7 Implement adaptation recovery: monitor triggering condition, restore original state within 30s of clearing
- [x] 6.8 Write property-based tests (proptest) for Properties 7, 8: adaptation recovery, probe reproducibility

## Phase 7: IPC Commands and Integration

- [x] 7.1 Register all IPC commands in Tauri app setup: hardware_get_profile, hardware_get_timeout_profile, hardware_get_compatibility_matrix, hardware_get_thermal_state, hardware_get_resource_utilization, hardware_run_probes, hardware_override_class
- [x] 7.2 Create `src/core/hardware.ts` with typed IPC wrappers and TimeoutProfile resolver
- [x] 7.3 Integrate timeout resolver into existing components: Phase 4 RL inference, Phase 2 scoring engine, Phase 3 tool tracker — replace hardcoded timeouts with hardware-aware values
- [x] 7.4 Implement hardware change detection on startup: compare current detection vs stored profile, notify user on significant changes
- [x] 7.5 Create behavioral contract JSON files: contract-hardware-detection-5s, contract-hardware-classification-deterministic, contract-hardware-timeout-positive, contract-hardware-envelope-sum, contract-hardware-no-crash-on-exhaustion
- [x] 7.6 Write integration tests: full detection → classification → timeout → compatibility flow, graceful degradation on minimal hardware (mock 4GB RAM, no GPU)
