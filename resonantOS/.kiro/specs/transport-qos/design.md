# Design Document: Transport QoS

## Overview

Application-level traffic prioritization that ensures split inference activations always have lowest latency, even when model downloads or checkpoint syncs are saturating the link. Uses priority queuing, DSCP marking, token bucket rate limiting, and congestion detection — all without admin privileges or third-party tools.

### Design Principles

1. **Zero configuration** — works out of the box with sensible defaults
2. **No admin required** — DSCP marking is best-effort, priority queue always works
3. **Transparent** — callers use existing `MessagePriority`, QoS is automatic
4. **Adaptive** — responds to congestion in real-time, not static rules
5. **Platform-agnostic** — same behavior on Linux, macOS, Windows

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                    Transport Send Path                            │
│                                                                  │
│  Caller: transport.send(target, message, priority)               │
│                            │                                     │
│                            ▼                                     │
│  ┌──────────────────────────────────────────────────────────┐    │
│  │  QoS Layer (transport/qos.rs)                            │    │
│  │                                                          │    │
│  │  1. Check: is this an activation tensor?                 │    │
│  │     YES → Fast-path (bypass queue, send immediately)     │    │
│  │     NO  → Continue to priority queue                     │    │
│  │                                                          │    │
│  │  2. Priority Queue                                       │    │
│  │     ┌──────────┐ ┌──────────┐ ┌──────────┐              │    │
│  │     │ Critical │ │  Normal  │ │   Low    │              │    │
│  │     │ (unbounded)│ │ (max 1000)│ │ (max 100)│              │    │
│  │     └────┬─────┘ └────┬─────┘ └────┬─────┘              │    │
│  │          │             │             │                    │    │
│  │  3. Dequeue: Critical first, then Normal, then Low       │    │
│  │                                                          │    │
│  │  4. Rate Limiter (Low priority only)                     │    │
│  │     Token bucket: if inference active → 1 MB/s cap       │    │
│  │                   if idle → unlimited                    │    │
│  │                                                          │    │
│  │  5. Congestion Check                                     │    │
│  │     If RTT > 2× baseline → pause Low, throttle Normal   │    │
│  │                                                          │    │
│  │  6. DSCP Mark                                            │    │
│  │     Critical → EF (0xB8)                                 │    │
│  │     Normal   → AF21 (0x48)                               │    │
│  │     Low      → BE (0x00)                                 │    │
│  └──────────────────────────────────────────────────────────┘    │
│                            │                                     │
│                            ▼                                     │
│  Adapter: LAN / WireGuard / Reticulum (actual send)              │
└─────────────────────────────────────────────────────────────────┘
```

## Components

### QosConfig

```rust
pub struct QosConfig {
    pub enabled: bool,
    pub critical_queue_max: usize,      // 0 = unlimited
    pub normal_queue_max: usize,        // Default: 1000
    pub low_queue_max: usize,           // Default: 100
    pub low_rate_limit_bytes_sec: u64,  // During active inference. Default: 1_000_000 (1 MB/s)
    pub congestion_rtt_multiplier: f64, // Default: 2.0 (trigger at 2× baseline)
    pub congestion_recovery_ratio: f64, // Default: 1.2 (recover when RTT < 1.2× baseline)
    pub dscp_enabled: bool,             // Default: true (best-effort)
    pub fast_path_enabled: bool,        // Default: true
    pub starvation_timeout_ms: u64,     // Default: 5000 (Low gets sent after 5s idle)
}
```

### PriorityQueue

```rust
pub struct PriorityQueue {
    critical: VecDeque<QueuedMessage>,
    normal: VecDeque<QueuedMessage>,
    low: VecDeque<QueuedMessage>,
    config: QosConfig,
    metrics: QosMetrics,
}

impl PriorityQueue {
    pub fn enqueue(&mut self, message: QueuedMessage, priority: MessagePriority) -> Result<(), QueueFull>;
    pub fn dequeue(&mut self) -> Option<QueuedMessage>;
    pub fn peek_priority(&self) -> Option<MessagePriority>;
    pub fn depth(&self, priority: MessagePriority) -> usize;
    pub fn total_depth(&self) -> usize;
}
```

### TokenBucket

```rust
pub struct TokenBucket {
    tokens: f64,
    max_tokens: f64,
    refill_rate: f64,  // tokens per second (= bytes per second)
    last_refill_ms: u64,
}

impl TokenBucket {
    pub fn new(rate_bytes_sec: u64, burst_bytes: u64) -> Self;
    pub fn try_consume(&mut self, bytes: u64) -> bool;
    pub fn set_rate(&mut self, rate_bytes_sec: u64);
    pub fn set_unlimited(&mut self);
    pub fn available(&self) -> u64;
}
```

### CongestionDetector

```rust
pub struct CongestionDetector {
    baseline_rtt_ms: f64,       // Exponential moving average
    current_rtt_ms: f64,
    congested: bool,
    config: QosConfig,
    last_update_ms: u64,
}

impl CongestionDetector {
    pub fn new(config: QosConfig) -> Self;
    pub fn record_rtt(&mut self, rtt_ms: f64);
    pub fn is_congested(&self) -> bool;
    pub fn congestion_ratio(&self) -> f64;  // current_rtt / baseline_rtt
}
```

### DscpMarker

```rust
pub struct DscpMarker;

impl DscpMarker {
    /// Set DSCP on a socket. Returns Ok(()) or Err if not supported.
    pub fn mark_socket(socket: &impl AsRawFd, priority: MessagePriority) -> Result<(), std::io::Error>;

    /// Get the TOS byte for a priority level.
    pub fn tos_byte(priority: MessagePriority) -> u8;
}
```

### QosLayer (main orchestrator)

```rust
pub struct QosLayer {
    config: QosConfig,
    queue: PriorityQueue,
    rate_limiter: TokenBucket,
    congestion: HashMap<NodeId, CongestionDetector>,
    inference_active: AtomicBool,
    metrics: QosMetrics,
}

impl QosLayer {
    pub fn new(config: QosConfig) -> Self;

    /// Submit a message for sending (goes through QoS pipeline).
    pub fn submit(&mut self, target: NodeId, message: TransportMessage, priority: MessagePriority) -> SendDecision;

    /// Check if a message should use the fast-path (activation tensors).
    pub fn is_fast_path(&self, message: &TransportMessage) -> bool;

    /// Notify that inference is active (triggers Low throttling).
    pub fn set_inference_active(&self, active: bool);

    /// Record an RTT measurement for congestion detection.
    pub fn record_rtt(&mut self, peer: NodeId, rtt_ms: f64);

    /// Get current QoS state for observability.
    pub fn state(&self) -> QosState;
}

pub enum SendDecision {
    SendNow,                    // Fast-path or Critical with empty queue
    Queued { position: usize }, // Enqueued, will be sent in order
    Throttled { wait_ms: u64 }, // Rate-limited, try again later
    Dropped { reason: String }, // Queue full, message dropped
    Congested { peer: NodeId }, // Peer congested, Low paused
}
```

### QosMetrics

```rust
pub struct QosMetrics {
    pub messages_sent_critical: u64,
    pub messages_sent_normal: u64,
    pub messages_sent_low: u64,
    pub messages_dropped: u64,
    pub bytes_throttled: u64,
    pub congestion_events: u64,
    pub fast_path_sends: u64,
    pub avg_queue_wait_ms: f64,
}
```

## Integration Points

### With Transport Manager

```rust
// In transport/manager.rs send path:
pub fn send(&self, target: &NodeId, message: &TransportMessage) -> Result<(), TransportError> {
    let priority = message.priority;

    // QoS fast-path check
    if self.qos.is_fast_path(message) {
        return self.send_immediate(target, message);  // Bypass queue
    }

    // Submit to QoS layer
    match self.qos.submit(*target, message.clone(), priority) {
        SendDecision::SendNow => self.send_immediate(target, message),
        SendDecision::Queued { .. } => Ok(()),  // Will be sent by drain loop
        SendDecision::Throttled { .. } => Ok(()),  // Retry later
        SendDecision::Dropped { reason } => Err(TransportError::QueueFull { reason }),
        SendDecision::Congested { .. } => Ok(()),  // Deferred
    }
}
```

### With Split Inference

```rust
// In inference/split/coordinator.rs:
// When sending activation tensors, mark as fast-path
let message = TransportMessage::new(
    activation_bytes,
    MessagePriority::Critical,
    RequestType::InferenceActivation,  // This triggers fast-path
);
```

### With Download Engine

```rust
// In network/download/:
// Model downloads use Low priority
let message = TransportMessage::new(
    chunk_bytes,
    MessagePriority::Low,
    RequestType::ModelTransfer,
);
```

## DSCP Values

| Priority | DSCP Name | Value | TOS Byte | Meaning |
|----------|-----------|-------|----------|---------|
| Critical | EF | 46 | 0xB8 | Expedited Forwarding (real-time) |
| Normal | AF21 | 18 | 0x48 | Assured Forwarding (important) |
| Low | BE | 0 | 0x00 | Best Effort (background) |

## File Structure

```
src/resonantos-vnext/src-tauri/src/transport/
├── qos.rs              # QosLayer, PriorityQueue, TokenBucket, CongestionDetector, DscpMarker
└── ... (existing files unchanged)
```
