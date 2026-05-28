# Requirements: Transport QoS (Application-Level Traffic Prioritization)

## Overview

Application-level Quality of Service for the transport layer. Prioritizes latency-critical traffic (inference activations) over bulk transfers (model downloads, checkpoints). No admin/root required, no third-party tools, works on all platforms.

## Functional Requirements

### 1. Priority Queue

- 1.1 The transport layer SHALL maintain a priority queue for outgoing messages
- 1.2 Messages SHALL be dequeued in priority order: Critical > Normal > Low
- 1.3 Within the same priority level, messages SHALL be FIFO
- 1.4 The queue SHALL have configurable max depth per priority (default: Critical=unlimited, Normal=1000, Low=100)
- 1.5 When a lower-priority queue is full, new messages SHALL be dropped (not block higher priority)

### 2. DSCP Packet Marking

- 2.1 Critical messages SHALL be marked with DSCP EF (0xB8 / 46) — Expedited Forwarding
- 2.2 Normal messages SHALL be marked with DSCP AF21 (0x48 / 18) — Assured Forwarding
- 2.3 Low messages SHALL be marked with DSCP BE (0x00 / 0) — Best Effort
- 2.4 DSCP marking SHALL work on TCP and UDP sockets
- 2.5 If DSCP marking fails (permission denied), the system SHALL continue without marking (graceful degradation)

### 3. Token Bucket Rate Limiter

- 3.1 Low-priority traffic SHALL be rate-limited when Critical traffic is active
- 3.2 The rate limiter SHALL use a token bucket algorithm (configurable rate + burst)
- 3.3 Default rate limit for Low traffic during active inference: 1 MB/s
- 3.4 Default rate limit for Low traffic when idle: unlimited (no throttling)
- 3.5 The rate limiter SHALL transition between active/idle within 100ms of state change
- 3.6 Normal traffic SHALL NOT be rate-limited (only Low)

### 4. Congestion Detection

- 4.1 The system SHALL track RTT (round-trip time) per peer
- 4.2 Congestion SHALL be detected when RTT exceeds 2× the baseline (moving average)
- 4.3 On congestion detection: pause Low-priority sends, reduce Normal send rate by 50%
- 4.4 On congestion recovery (RTT returns to baseline): resume normal sending
- 4.5 Congestion state SHALL be reported per-peer in transport health

### 5. Activation Fast-Path

- 5.1 Split inference activation tensors SHALL bypass the normal message queue
- 5.2 Activations SHALL be sent on a dedicated connection (separate TCP stream or UDP)
- 5.3 The fast-path SHALL have zero queuing delay (send immediately)
- 5.4 If the fast-path connection fails, fall back to the priority queue (Critical priority)

### 6. Integration

- 6.1 QoS SHALL be transparent to callers (no API changes to send/receive)
- 6.2 The existing `MessagePriority` enum SHALL drive all QoS decisions
- 6.3 QoS SHALL apply to all transport adapters (LAN, WireGuard, Reticulum)
- 6.4 QoS metrics SHALL be visible in the dashboard (queue depths, throttle state, congestion)
- 6.5 QoS SHALL be configurable (enable/disable, rate limits, thresholds) via settings

### 7. Observability

- 7.1 The system SHALL track: messages sent per priority, bytes throttled, congestion events
- 7.2 The system SHALL emit events on: congestion detected, congestion recovered, throttle activated
- 7.3 Per-peer QoS state SHALL be queryable via Tauri command

## Non-Functional Requirements

### 8. Performance

- 8.1 QoS overhead SHALL be < 0.1ms per message (queue + priority check)
- 8.2 DSCP marking SHALL add zero latency (set once per socket)
- 8.3 Token bucket check SHALL be O(1) per message

### 9. Platform Support

- 9.1 DSCP marking SHALL work on Linux (setsockopt IP_TOS)
- 9.2 DSCP marking SHALL work on macOS (setsockopt IP_TOS)
- 9.3 DSCP marking SHALL work on Windows (setsockopt IP_TOS or QoS API)
- 9.4 If platform doesn't support DSCP, degrade gracefully (priority queue still works)

## Correctness Properties

- P1: Priority Ordering — Critical messages always sent before Normal, Normal before Low
- P2: No Starvation — Low-priority messages eventually sent (within 5 seconds of idle)
- P3: Rate Bound — Low-priority throughput never exceeds configured rate during active inference
- P4: Congestion Response — RTT spike triggers throttle within 100ms
- P5: Fast-Path Bypass — Activation tensors never wait in queue
