# Implementation Plan: Transport QoS

## Overview

Application-level traffic prioritization for the transport layer. Priority queuing, DSCP marking, token bucket rate limiting, congestion detection, and activation fast-path. Zero admin, zero third-party tools, all platforms.

**Build verification:** `cargo test --lib --no-run` from `src/resonantos-vnext/src-tauri`.

## Tasks

- [x] 1. Core QoS module
  - [x] 1.1 Create `transport/qos.rs` with all QoS components
    - Define `QosConfig` with all fields and defaults
    - Implement `PriorityQueue` (Critical/Normal/Low VecDeques, max depth, FIFO within priority)
    - Implement `TokenBucket` (rate limit, burst, refill, consume)
    - Implement `CongestionDetector` (RTT tracking, EMA baseline, threshold detection)
    - Implement `DscpMarker` (set TOS byte on socket, platform-aware)
    - Implement `QosLayer` (orchestrator: submit, fast-path check, drain, metrics)
    - Define `SendDecision` enum and `QosMetrics` struct
    - _Requirements: 1.1-1.5, 2.1-2.5, 3.1-3.6, 4.1-4.5, 5.1-5.4, 8.1-8.3_

  - [x] 1.2 Register module in `transport/mod.rs`
    - Add `pub mod qos;`
    - _Requirements: 6.1_

- [ ] 2. Priority queue implementation
  - [x] 2.1 Implement enqueue with max depth enforcement
    - Critical: unbounded (or very large)
    - Normal: drop oldest when full (max 1000)
    - Low: drop newest when full (max 100)
    - _Requirements: 1.1, 1.2, 1.3, 1.4, 1.5_

  - [x] 2.2 Implement dequeue with strict priority ordering
    - Always drain Critical first, then Normal, then Low
    - Track starvation: if Low hasn't been sent in 5s, promote one message
    - _Requirements: 1.2, 1.3_

  - [ ]* 2.3 Write property tests for priority queue
    - **P1: Priority Ordering** — Critical always dequeued before Normal/Low
    - **P2: No Starvation** — Low messages sent within starvation_timeout of idle
    - _Validates: Requirements 1.2, 1.3_

- [ ] 3. Token bucket rate limiter
  - [x] 3.1 Implement token bucket algorithm
    - Refill tokens at configured rate (bytes/sec)
    - Consume tokens on send (message size in bytes)
    - If insufficient tokens: return Throttled
    - Support switching between limited (inference active) and unlimited (idle)
    - _Requirements: 3.1, 3.2, 3.3, 3.4, 3.5, 3.6_

  - [ ]* 3.2 Write property test for rate limiting
    - **P3: Rate Bound** — total bytes sent in any 1-second window ≤ rate + burst
    - _Validates: Requirements 3.3_

- [ ] 4. Congestion detection
  - [x] 4.1 Implement RTT-based congestion detection
    - Track per-peer RTT via exponential moving average (α=0.125)
    - Congested when current_rtt > multiplier × baseline_rtt
    - Recovered when current_rtt < recovery_ratio × baseline_rtt
    - On congestion: pause Low, throttle Normal by 50%
    - On recovery: resume normal operation
    - _Requirements: 4.1, 4.2, 4.3, 4.4, 4.5_

  - [ ]* 4.2 Write property test for congestion detection
    - **P4: Congestion Response** — RTT spike above threshold always triggers congested state
    - _Validates: Requirements 4.2, 4.3_

- [ ] 5. DSCP marking
  - [x] 5.1 Implement cross-platform DSCP socket marking
    - Linux/macOS: setsockopt(IP_TOS)
    - Windows: setsockopt(IP_TOS) or WSA QoS
    - Graceful fallback if permission denied
    - _Requirements: 2.1, 2.2, 2.3, 2.4, 2.5, 9.1, 9.2, 9.3, 9.4_

- [ ] 6. Activation fast-path
  - [x] 6.1 Implement fast-path detection and bypass
    - Check RequestType::InferenceActivation → bypass queue
    - Send immediately on dedicated path
    - Fallback to Critical queue if fast-path fails
    - _Requirements: 5.1, 5.2, 5.3, 5.4_

  - [ ]* 6.2 Write property test for fast-path
    - **P5: Fast-Path Bypass** — InferenceActivation messages never wait in queue
    - _Validates: Requirements 5.1, 5.3_

- [ ] 7. Integration with transport manager
  - [x] 7.1 Wire QosLayer into transport send path
    - All outgoing messages pass through QoS before adapter send
    - Set inference_active flag from optimizer cycle
    - _Requirements: 6.1, 6.2, 6.3_

  - [x] 7.2 Implement async drain loop (tokio task)
    - Spawn dedicated tokio task that continuously dequeues from PriorityQueue
    - Dequeue in priority order, apply rate limiter check before sending
    - Sleep when queue empty (wake on new enqueue via tokio::sync::Notify)
    - Respect congestion state (pause Low sends when congested)
    - Handle cancellation via CancellationToken for clean shutdown
    - _Requirements: 6.1, 8.1_

  - [x] 7.3 Wire RTT measurements into congestion detector
    - After each successful send/receive, record RTT
    - Use existing heartbeat/pong timestamps for RTT
    - _Requirements: 4.1_

- [ ] 8. Observability and Tauri commands
  - [x] 8.1 Implement QoS metrics tracking
    - Track all QosMetrics fields (messages per priority, drops, throttle, congestion)
    - _Requirements: 7.1_

  - [x] 8.2 Add `get_qos_status` Tauri command
    - Return current QoS state: queue depths, congestion per peer, throttle active, metrics
    - _Requirements: 7.3_

  - [x] 8.3 Emit Tauri events on state changes
    - Emit `qos-congestion-detected` and `qos-congestion-recovered` events
    - Emit `qos-throttle-activated` and `qos-throttle-deactivated` events
    - _Requirements: 7.2_

- [ ] 9. Frontend: Dashboard QoS widget
  - [x] 9.1 Create `src/components/dashboard/QosPanel.tsx`
    - Show per-peer congestion state (green/yellow/red indicator)
    - Show queue depths (Critical/Normal/Low bar chart)
    - Show throttle status (active/idle badge)
    - Show metrics: messages sent per priority, drops, congestion events
    - Wrap with React.memo for performance
    - Subscribe to `qos-congestion-detected` / `qos-congestion-recovered` events
    - _Requirements: 6.4_

  - [x] 9.2 Wire QosPanel into NetworkDashboard
    - Add QosPanel to the dashboard grid (Row 4 or new row)
    - Use `useTauriEvent` hook for real-time updates
    - _Requirements: 6.4_

- [ ] 10. Frontend: QoS Settings panel
  - [x] 10.1 Create QoS section in Settings
    - Toggle: QoS enabled/disabled
    - Slider: Low-priority rate limit (0.5 MB/s to 10 MB/s, default 1 MB/s)
    - Slider: Congestion RTT multiplier (1.5× to 4×, default 2×)
    - Toggle: DSCP marking enabled/disabled
    - Toggle: Fast-path enabled/disabled
    - Persist settings to backend via Tauri command
    - _Requirements: 6.5_

  - [x] 10.2 Add `set_qos_config` Tauri command
    - Accept QosConfig fields from frontend
    - Apply to running QosLayer without restart (hot-reload)
    - _Requirements: 6.5_

- [x] 11. Final checkpoint
  - Verify `cargo test --lib --no-run` passes.
  - Verify `npx tsc --noEmit` passes (frontend components).
  - Verify QoS layer doesn't add measurable latency to Critical messages.
  - Verify drain loop shuts down cleanly on app exit.

## Notes

- DSCP marking is best-effort — many home routers ignore it, but enterprise/gaming routers respect it
- The token bucket is only applied to Low priority — Critical and Normal are never rate-limited
- Congestion detection uses the same RTT data already collected by the heartbeat system
- The fast-path is specifically for split inference activations (the most latency-sensitive traffic)
- Starvation prevention ensures model downloads eventually complete even during heavy inference
