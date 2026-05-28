# Requirements Document

## Introduction

Hardware Stability is Phase 7 of the ResonantOS vNext improvement plan. It delivers a formal hardware abstraction layer that detects, profiles, and adapts to the specific capabilities of each machine running ResonantOS — whether a GPU-equipped workstation, a CPU-only laptop, or a headless server node. The system produces a structured HardwareProfile at startup, calibrates timeouts and resource limits per hardware class, maintains a model compatibility matrix that maps model requirements to hardware capabilities, and provides runtime adaptation so that inference, training, and tool execution degrade gracefully rather than crash when hardware limits are reached.

This phase is foundational — it provides the hardware awareness that Phase 9 (Local Cluster) and Phase 10 (Mesh Compute Network) depend on for heterogeneous resource orchestration. Without a reliable HardwareProfile, multi-node scheduling cannot make correct placement decisions.

The system must be zero-configuration for the common case (auto-detect everything), while allowing manual overrides for edge cases (virtualized environments, custom GPU drivers, restricted containers). Hardware detection runs once at startup and can be re-triggered on demand. The profile is persisted locally and shared with the Compute Fabric for job scheduling decisions.

## Glossary

- **HardwareProfile**: A structured description of a machine's compute capabilities including CPU, memory, GPU, disk, and network characteristics
- **HardwareClass**: A classification of a machine into one of: "gpu-workstation", "cpu-workstation", "gpu-server", "cpu-server", "embedded", "container-restricted"
- **GPU_Capability**: The detected GPU characteristics including VRAM, compute capability, driver version, and supported frameworks (CUDA, ROCm, Metal, Vulkan)
- **Model_Compatibility_Matrix**: A mapping from model requirements (parameter count, quantization, VRAM needed) to hardware capabilities that can serve them
- **Timeout_Profile**: A set of calibrated timeout values for inference, tool execution, health checks, and network operations tuned to the detected hardware class
- **Resource_Envelope**: The maximum CPU, memory, GPU, and disk resources available for a specific workload type on the current hardware
- **Hardware_Probe**: A lightweight benchmark that measures actual throughput (tokens/sec for inference, IOPS for disk, bandwidth for network) rather than relying on spec sheets
- **Thermal_State**: The current thermal condition of the hardware: "nominal", "warm", "throttling", or "critical"
- **Adaptation_Strategy**: The runtime behavior adjustment applied when hardware limits are approached (reduce batch size, switch to smaller model, queue requests)
- **Hardware_Event**: A significant change in hardware state (GPU memory pressure, thermal throttling, disk full, network degradation) that triggers adaptation
- **Capability_Gate**: A check that prevents a workload from being scheduled on hardware that cannot support it (e.g., 70B model on 8GB VRAM machine)

## Requirements

### Requirement 1: Hardware Detection and Profiling

**User Story:** As the system, I want automatic detection of all hardware capabilities at startup, so that resource allocation decisions are based on actual machine specifications rather than assumptions.

#### Acceptance Criteria

1. THE system SHALL detect CPU capabilities at startup including: core count (physical and logical), architecture (x86_64, aarch64), base clock speed, and available instruction set extensions (AVX2, AVX-512, NEON)
2. THE system SHALL detect memory capabilities including: total physical RAM, available RAM, swap size, and memory bandwidth (estimated from DDR generation and channel count)
3. THE system SHALL detect GPU capabilities including: presence/absence of discrete GPU, GPU model name, total VRAM, available VRAM, compute capability version, driver version, and supported frameworks (CUDA version, ROCm version, Metal support, Vulkan compute support)
4. THE system SHALL detect storage capabilities including: available disk space on the data directory, estimated sequential read/write speed (via a 10MB probe write), and storage type classification (SSD/NVMe/HDD)
5. THE system SHALL detect network capabilities including: available network interfaces, estimated LAN bandwidth (via loopback probe), and internet connectivity status
6. THE system SHALL complete all hardware detection within 5 seconds on a typical machine, with individual probes timing out after 2 seconds each
7. THE system SHALL persist the detected HardwareProfile to local storage and make it available via IPC for all system components

### Requirement 2: Hardware Classification

**User Story:** As the system, I want each machine classified into a hardware class, so that default configurations and timeout profiles can be applied without per-machine tuning.

#### Acceptance Criteria

1. THE system SHALL classify each machine into exactly one HardwareClass based on detected capabilities: "gpu-workstation" (discrete GPU with >= 8GB VRAM + >= 16GB RAM), "cpu-workstation" (no GPU or < 8GB VRAM, >= 16GB RAM), "gpu-server" (discrete GPU + >= 64GB RAM + headless), "cpu-server" (no GPU + >= 64GB RAM + headless), "embedded" (< 8GB RAM or ARM SBC), "container-restricted" (detected container environment with resource limits)
2. THE system SHALL allow manual override of the detected HardwareClass via configuration file when auto-detection produces incorrect results (virtualized environments, passthrough GPU)
3. THE system SHALL log the detected HardwareClass and the detection rationale (which criteria matched) at startup
4. THE system SHALL re-detect and potentially reclassify when hardware changes are detected (GPU added/removed, RAM changed) or when manually triggered

### Requirement 3: Timeout Calibration

**User Story:** As the system, I want timeouts calibrated to actual hardware performance, so that operations neither fail prematurely on slow hardware nor waste time waiting on fast hardware.

#### Acceptance Criteria

1. THE system SHALL maintain a Timeout_Profile per HardwareClass containing calibrated values for: model inference timeout, tool execution timeout, health check timeout, network request timeout, database query timeout, and compute job timeout
2. THE system SHALL set default timeout values based on HardwareClass: gpu-workstation (inference: 5ms, tool: 30s), cpu-workstation (inference: 50ms, tool: 60s), gpu-server (inference: 3ms, tool: 30s), cpu-server (inference: 100ms, tool: 120s), embedded (inference: 500ms, tool: 300s), container-restricted (inference: 200ms, tool: 120s)
3. THE system SHALL support runtime timeout adjustment based on observed performance: if 90th percentile latency exceeds 80% of the timeout for 10 consecutive operations, the timeout SHALL be increased by 50% (up to a configurable maximum)
4. THE system SHALL support runtime timeout tightening: if 99th percentile latency is below 20% of the timeout for 100 consecutive operations, the timeout SHALL be decreased by 25% (down to the hardware class default)
5. THE system SHALL expose the current Timeout_Profile via IPC so that all system components use consistent, hardware-aware timeouts

### Requirement 4: Model Compatibility Matrix

**User Story:** As the system, I want to know which models can run on the current hardware, so that model selection never attempts to load a model that will OOM or fail.

#### Acceptance Criteria

1. THE system SHALL maintain a Model_Compatibility_Matrix mapping model requirements (parameter count, quantization level, minimum VRAM, minimum RAM, required compute capability) to boolean compatibility with the current HardwareProfile
2. THE system SHALL compute compatibility for each known model at startup and update when the HardwareProfile changes
3. THE system SHALL classify each model into one of: "native-gpu" (fits in VRAM, full speed), "offloaded" (partially in VRAM + RAM, reduced speed), "cpu-only" (fits in RAM, no GPU needed), "incompatible" (exceeds available resources)
4. THE system SHALL expose the compatibility matrix via IPC so that model-strategy.ts can filter candidate models before selection
5. THE system SHALL include estimated inference speed (tokens/second) per model per compatibility class, calibrated from the Hardware_Probe results
6. WHEN a model is classified as "incompatible", THE system SHALL provide a human-readable explanation of why (e.g., "requires 24GB VRAM, only 8GB available") and suggest alternatives (smaller quantization, different model)

### Requirement 5: Resource Envelope Management

**User Story:** As the system, I want resource limits enforced per workload type, so that inference doesn't starve tool execution and background jobs don't impact interactive responsiveness.

#### Acceptance Criteria

1. THE system SHALL define Resource_Envelopes for each workload type: "interactive-inference" (highest priority, reserved resources), "tool-execution" (medium priority), "background-training" (lowest priority, best-effort), "system-overhead" (fixed allocation for OS and shell)
2. THE system SHALL allocate resources across envelopes based on HardwareClass: on gpu-workstation, reserve 70% GPU for interactive inference, 20% for tool execution, 10% for background; on cpu-workstation, reserve 50% CPU for interactive, 30% for tools, 20% for background
3. THE system SHALL enforce memory limits per envelope: if a workload exceeds its memory allocation, the system SHALL apply backpressure (queue requests) rather than allowing OOM
4. THE system SHALL monitor resource usage per envelope in real-time and expose current utilization via IPC
5. THE system SHALL support dynamic rebalancing: when interactive workload is idle, background workloads MAY temporarily use the freed resources, releasing them within 1 second when interactive demand returns

### Requirement 6: Thermal and Throttling Awareness

**User Story:** As the system, I want awareness of thermal throttling, so that the system can proactively reduce load before hardware forces degraded performance.

#### Acceptance Criteria

1. THE system SHALL monitor Thermal_State by reading CPU and GPU temperature sensors (where available) at 10-second intervals
2. THE system SHALL classify Thermal_State as: "nominal" (< 70C), "warm" (70-85C), "throttling" (> 85C or clock speed reduced), "critical" (> 95C or hardware protection active)
3. WHEN Thermal_State transitions to "throttling", THE system SHALL reduce concurrent workloads by 50% and increase timeouts by 2x until thermal state returns to "warm" or below
4. WHEN Thermal_State transitions to "critical", THE system SHALL pause all background workloads and limit interactive workloads to one concurrent request until thermal state recovers
5. THE system SHALL log all Thermal_State transitions as Hardware_Events with timestamp, temperature readings, and applied adaptation

### Requirement 7: GPU Memory Management

**User Story:** As the system, I want proactive GPU memory management, so that model loading and inference never crash due to VRAM exhaustion.

#### Acceptance Criteria

1. THE system SHALL monitor GPU VRAM usage at 5-second intervals when a GPU is present
2. THE system SHALL maintain a VRAM allocation registry tracking which models and workloads currently occupy VRAM
3. WHEN available VRAM drops below 15% of total, THE system SHALL emit a Hardware_Event "gpu-memory-pressure" and prevent new model loads until VRAM is freed
4. THE system SHALL support model eviction: when a higher-priority model needs VRAM, lower-priority cached models SHALL be evicted (unloaded) to make space
5. THE system SHALL pre-compute VRAM requirements before model loading and reject load requests that would exceed available VRAM, returning an error with the shortfall amount
6. WHEN no GPU is present, THE system SHALL route all inference to CPU-compatible models and never attempt GPU operations

### Requirement 8: Runtime Adaptation Strategies

**User Story:** As the system, I want automatic adaptation when hardware limits are reached, so that the system degrades gracefully rather than crashing or hanging.

#### Acceptance Criteria

1. THE system SHALL implement Adaptation_Strategies for each Hardware_Event type: gpu-memory-pressure (evict low-priority models, switch to smaller quantization), thermal-throttling (reduce concurrency, increase timeouts), disk-full (pause background jobs, alert user), network-degradation (switch to local-only models, queue remote requests)
2. THE system SHALL apply adaptations automatically without user intervention, logging each adaptation with the triggering event and the action taken
3. THE system SHALL recover automatically when the triggering condition resolves: restore original concurrency, timeouts, and model selections within 30 seconds of condition clearing
4. THE system SHALL never crash or hang due to resource exhaustion — all resource limits SHALL result in graceful degradation (queuing, model downgrade, or timeout) rather than process termination
5. THE system SHALL expose current adaptation state via IPC so that the UI can display hardware status and any active degradations to the user

### Requirement 9: Hardware Probe Benchmarks

**User Story:** As the system, I want lightweight benchmarks that measure actual performance, so that timeout calibration and model compatibility are based on measured throughput rather than theoretical specs.

#### Acceptance Criteria

1. THE system SHALL run a Hardware_Probe at first startup that measures: CPU inference throughput (matrix multiply benchmark, tokens/sec estimate), GPU inference throughput (if GPU present, small ONNX model forward pass), disk sequential I/O (10MB write/read), and memory bandwidth (large array copy)
2. THE system SHALL complete the full Hardware_Probe within 15 seconds
3. THE system SHALL persist probe results alongside the HardwareProfile and use them for timeout calibration and model speed estimation
4. THE system SHALL support re-running the Hardware_Probe on demand (user-triggered or after hardware changes)
5. THE system SHALL NOT run the Hardware_Probe on every startup — only on first run, after hardware changes, or when manually triggered

### Requirement 10: Graceful Degradation Without Hardware Features

**User Story:** As the system, I want full functionality even without GPU or with minimal hardware, so that ResonantOS works on any machine from a Raspberry Pi to a workstation.

#### Acceptance Criteria

1. IF no GPU is detected, THE system SHALL operate using CPU-only inference paths with appropriately calibrated timeouts, never attempting GPU operations or displaying GPU-related errors
2. IF available RAM is below 8GB, THE system SHALL restrict model selection to models requiring less than 4GB RAM and disable background training workloads
3. IF available disk space is below 5GB, THE system SHALL disable model caching and operate in streaming mode, alerting the user about limited functionality
4. THE system SHALL function correctly in all HardwareClasses without requiring manual configuration — auto-detection and default profiles SHALL provide a working system on any supported hardware
5. THE system SHALL log a clear summary at startup indicating: detected hardware class, available resources, any limitations applied, and models compatible with the current hardware

### Requirement 11: Behavioral Contract Integration

**User Story:** As a developer, I want the hardware stability layer to ship with behavioral contracts, so that the Phase 0 backtest mode can verify its correctness across future changes.

#### Acceptance Criteria

1. THE system SHALL register Behavioral_Contracts in the Phase 0 Contract_Registry covering: HardwareProfile detection completes within 5 seconds, HardwareClass classification is deterministic for the same hardware, and Timeout_Profile values are always positive and within configured bounds
2. THE system SHALL register Behavioral_Contracts covering: Model_Compatibility_Matrix correctly rejects models exceeding available resources, Resource_Envelopes never allocate more than 100% of available resources, and GPU memory management prevents VRAM overcommit
3. THE system SHALL register Behavioral_Contracts covering: Thermal adaptation reduces load on throttling, adaptation recovery restores original state within 30 seconds, and the system never crashes due to resource exhaustion
4. WHEN a Behavioral_Contract for the hardware stability layer fails, THE Regression_Gate SHALL block the merge and produce a Diagnostic_Report identifying the failing contract
