// Transport QoS — application-level traffic prioritization.
//
// Priority queuing, DSCP marking, token bucket rate limiting,
// congestion detection, and activation fast-path.

use crate::transport::trait_def::{MessagePriority, RequestType, TransportMessage};
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use uuid::Uuid;

pub type NodeId = Uuid;

// ─── Configuration ───────────────────────────────────────────────────────────

/// QoS configuration.
#[derive(Debug, Clone)]
pub struct QosConfig {
    pub enabled: bool,
    pub critical_queue_max: usize,
    pub normal_queue_max: usize,
    pub low_queue_max: usize,
    pub low_rate_limit_bytes_sec: u64,
    pub congestion_rtt_multiplier: f64,
    pub congestion_recovery_ratio: f64,
    pub dscp_enabled: bool,
    pub fast_path_enabled: bool,
    pub starvation_timeout_ms: u64,
}

impl Default for QosConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            critical_queue_max: 0, // unlimited
            normal_queue_max: 1000,
            low_queue_max: 100,
            low_rate_limit_bytes_sec: 1_000_000, // 1 MB/s
            congestion_rtt_multiplier: 2.0,
            congestion_recovery_ratio: 1.2,
            dscp_enabled: true,
            fast_path_enabled: true,
            starvation_timeout_ms: 5000,
        }
    }
}

// ─── Queued Message ──────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct QueuedMessage {
    pub target: NodeId,
    pub payload_bytes: usize,
    pub priority: MessagePriority,
    pub request_type: RequestType,
    pub enqueued_at_ms: u64,
}

// ─── Priority Queue ──────────────────────────────────────────────────────────

pub struct PriorityQueue {
    critical: VecDeque<QueuedMessage>,
    normal: VecDeque<QueuedMessage>,
    low: VecDeque<QueuedMessage>,
    config: QosConfig,
    last_low_send_ms: u64,
}

impl PriorityQueue {
    pub fn new(config: QosConfig) -> Self {
        Self {
            critical: VecDeque::new(),
            normal: VecDeque::new(),
            low: VecDeque::new(),
            config,
            last_low_send_ms: 0,
        }
    }

    pub fn enqueue(&mut self, msg: QueuedMessage) -> Result<usize, QueueFullError> {
        match msg.priority {
            MessagePriority::Critical => {
                if self.config.critical_queue_max > 0 && self.critical.len() >= self.config.critical_queue_max {
                    return Err(QueueFullError { priority: MessagePriority::Critical });
                }
                self.critical.push_back(msg);
                Ok(self.critical.len() - 1)
            }
            MessagePriority::Normal => {
                if self.normal.len() >= self.config.normal_queue_max {
                    self.normal.pop_front(); // Drop oldest
                }
                self.normal.push_back(msg);
                Ok(self.normal.len() - 1)
            }
            MessagePriority::Low => {
                if self.low.len() >= self.config.low_queue_max {
                    return Err(QueueFullError { priority: MessagePriority::Low });
                }
                self.low.push_back(msg);
                Ok(self.low.len() - 1)
            }
        }
    }

    pub fn dequeue(&mut self) -> Option<QueuedMessage> {
        // Strict priority: Critical > Normal > Low
        if let Some(msg) = self.critical.pop_front() {
            return Some(msg);
        }
        if let Some(msg) = self.normal.pop_front() {
            return Some(msg);
        }
        // Starvation check for Low
        if let Some(msg) = self.low.pop_front() {
            self.last_low_send_ms = now_ms();
            return Some(msg);
        }
        None
    }

    /// Check if Low is starving (hasn't been sent in starvation_timeout).
    pub fn low_starving(&self) -> bool {
        if self.low.is_empty() { return false; }
        let elapsed = now_ms().saturating_sub(self.last_low_send_ms);
        elapsed > self.config.starvation_timeout_ms
    }

    /// Force dequeue one Low message (starvation prevention).
    pub fn dequeue_low_forced(&mut self) -> Option<QueuedMessage> {
        let msg = self.low.pop_front();
        if msg.is_some() { self.last_low_send_ms = now_ms(); }
        msg
    }

    pub fn depth(&self, priority: MessagePriority) -> usize {
        match priority {
            MessagePriority::Critical => self.critical.len(),
            MessagePriority::Normal => self.normal.len(),
            MessagePriority::Low => self.low.len(),
        }
    }

    pub fn total_depth(&self) -> usize {
        self.critical.len() + self.normal.len() + self.low.len()
    }

    pub fn is_empty(&self) -> bool {
        self.critical.is_empty() && self.normal.is_empty() && self.low.is_empty()
    }
}

#[derive(Debug, Clone)]
pub struct QueueFullError {
    pub priority: MessagePriority,
}

// ─── Token Bucket Rate Limiter ───────────────────────────────────────────────

pub struct TokenBucket {
    tokens: f64,
    max_tokens: f64,
    refill_rate: f64,
    last_refill_ms: u64,
    unlimited: bool,
}

impl TokenBucket {
    pub fn new(rate_bytes_sec: u64, burst_bytes: u64) -> Self {
        Self {
            tokens: burst_bytes as f64,
            max_tokens: burst_bytes as f64,
            refill_rate: rate_bytes_sec as f64,
            last_refill_ms: now_ms(),
            unlimited: false,
        }
    }

    pub fn try_consume(&mut self, bytes: u64) -> bool {
        if self.unlimited { return true; }
        self.refill();
        if self.tokens >= bytes as f64 {
            self.tokens -= bytes as f64;
            true
        } else {
            false
        }
    }

    pub fn set_rate(&mut self, rate_bytes_sec: u64) {
        self.refill_rate = rate_bytes_sec as f64;
        self.unlimited = false;
    }

    pub fn set_unlimited(&mut self) {
        self.unlimited = true;
    }

    pub fn available(&self) -> u64 {
        if self.unlimited { return u64::MAX; }
        self.tokens as u64
    }

    fn refill(&mut self) {
        let now = now_ms();
        let elapsed_sec = (now.saturating_sub(self.last_refill_ms)) as f64 / 1000.0;
        self.tokens = (self.tokens + self.refill_rate * elapsed_sec).min(self.max_tokens);
        self.last_refill_ms = now;
    }
}

// ─── Congestion Detector ─────────────────────────────────────────────────────

pub struct CongestionDetector {
    baseline_rtt_ms: f64,
    current_rtt_ms: f64,
    congested: bool,
    multiplier: f64,
    recovery_ratio: f64,
    samples: u64,
}

impl CongestionDetector {
    pub fn new(multiplier: f64, recovery_ratio: f64) -> Self {
        Self {
            baseline_rtt_ms: 0.0,
            current_rtt_ms: 0.0,
            congested: false,
            multiplier,
            recovery_ratio,
            samples: 0,
        }
    }

    pub fn record_rtt(&mut self, rtt_ms: f64) {
        self.samples += 1;
        let alpha = 0.125; // EMA smoothing factor
        if self.samples == 1 {
            self.baseline_rtt_ms = rtt_ms;
            self.current_rtt_ms = rtt_ms;
        } else {
            self.baseline_rtt_ms += alpha * (rtt_ms - self.baseline_rtt_ms);
            self.current_rtt_ms = rtt_ms;
        }

        // Check congestion
        if self.baseline_rtt_ms > 0.0 {
            if self.current_rtt_ms > self.multiplier * self.baseline_rtt_ms {
                self.congested = true;
            } else if self.current_rtt_ms < self.recovery_ratio * self.baseline_rtt_ms {
                self.congested = false;
            }
        }
    }

    pub fn is_congested(&self) -> bool { self.congested }
    pub fn baseline_rtt(&self) -> f64 { self.baseline_rtt_ms }
    pub fn current_rtt(&self) -> f64 { self.current_rtt_ms }
    pub fn congestion_ratio(&self) -> f64 {
        if self.baseline_rtt_ms > 0.0 { self.current_rtt_ms / self.baseline_rtt_ms } else { 1.0 }
    }
}

// ─── DSCP Marker ─────────────────────────────────────────────────────────────

pub struct DscpMarker;

impl DscpMarker {
    pub fn tos_byte(priority: MessagePriority) -> u8 {
        match priority {
            MessagePriority::Critical => 0xB8, // EF (46 << 2)
            MessagePriority::Normal => 0x48,   // AF21 (18 << 2)
            MessagePriority::Low => 0x00,      // BE
        }
    }

    /// Mark a raw socket fd with DSCP. Best-effort (may fail without admin).
    #[cfg(unix)]
    pub fn mark_fd(fd: i32, priority: MessagePriority) -> Result<(), std::io::Error> {
        let tos = Self::tos_byte(priority) as i32;
        let result = unsafe {
            libc::setsockopt(
                fd, libc::IPPROTO_IP, libc::IP_TOS,
                &tos as *const i32 as *const libc::c_void, 4,
            )
        };
        if result == 0 { Ok(()) } else { Err(std::io::Error::last_os_error()) }
    }

    #[cfg(not(unix))]
    pub fn mark_fd(_fd: i32, _priority: MessagePriority) -> Result<(), std::io::Error> {
        // Windows: would use WSA QoS API or IP_TOS via winsock
        // For now: no-op (graceful degradation)
        Ok(())
    }
}

// ─── Send Decision ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub enum SendDecision {
    SendNow,
    Queued { position: usize },
    Throttled { wait_ms: u64 },
    Dropped { reason: String },
    Congested { peer: NodeId },
}

// ─── QoS Metrics ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct QosMetrics {
    pub messages_sent_critical: u64,
    pub messages_sent_normal: u64,
    pub messages_sent_low: u64,
    pub messages_dropped: u64,
    pub bytes_throttled: u64,
    pub congestion_events: u64,
    pub fast_path_sends: u64,
    pub starvation_promotions: u64,
}

// ─── QoS Layer (orchestrator) ────────────────────────────────────────────────

pub struct QosLayer {
    pub config: QosConfig,
    pub queue: PriorityQueue,
    pub rate_limiter: TokenBucket,
    pub congestion: HashMap<NodeId, CongestionDetector>,
    pub inference_active: AtomicBool,
    pub metrics: QosMetrics,
}

impl QosLayer {
    pub fn new(config: QosConfig) -> Self {
        let rate = config.low_rate_limit_bytes_sec;
        Self {
            queue: PriorityQueue::new(config.clone()),
            rate_limiter: TokenBucket::new(rate, rate * 2), // 2s burst
            congestion: HashMap::new(),
            inference_active: AtomicBool::new(false),
            metrics: QosMetrics::default(),
            config,
        }
    }

    /// Check if message should use fast-path (bypass queue entirely).
    pub fn is_fast_path(&self, request_type: &RequestType) -> bool {
        self.config.fast_path_enabled && *request_type == RequestType::InferenceActivation
    }

    /// Submit a message to the QoS pipeline.
    pub fn submit(&mut self, target: NodeId, payload_bytes: usize, priority: MessagePriority, request_type: RequestType) -> SendDecision {
        if !self.config.enabled {
            return SendDecision::SendNow;
        }

        // Fast-path bypass
        if self.is_fast_path(&request_type) {
            self.metrics.fast_path_sends += 1;
            return SendDecision::SendNow;
        }

        // Congestion check for Low priority
        if priority == MessagePriority::Low {
            if let Some(detector) = self.congestion.get(&target) {
                if detector.is_congested() {
                    self.metrics.bytes_throttled += payload_bytes as u64;
                    return SendDecision::Congested { peer: target };
                }
            }
        }

        // Rate limiter for Low priority during active inference
        if priority == MessagePriority::Low && self.inference_active.load(Ordering::Relaxed) {
            if !self.rate_limiter.try_consume(payload_bytes as u64) {
                self.metrics.bytes_throttled += payload_bytes as u64;
                return SendDecision::Throttled { wait_ms: 100 };
            }
        }

        // Enqueue
        let msg = QueuedMessage {
            target,
            payload_bytes,
            priority,
            request_type,
            enqueued_at_ms: now_ms(),
        };

        match self.queue.enqueue(msg) {
            Ok(pos) => SendDecision::Queued { position: pos },
            Err(_) => {
                self.metrics.messages_dropped += 1;
                SendDecision::Dropped { reason: "Queue full".to_string() }
            }
        }
    }

    /// Set inference active state (triggers Low throttling).
    pub fn set_inference_active(&self, active: bool) {
        self.inference_active.store(active, Ordering::Relaxed);
    }

    /// Record RTT for a peer.
    pub fn record_rtt(&mut self, peer: NodeId, rtt_ms: f64) {
        let detector = self.congestion.entry(peer).or_insert_with(|| {
            CongestionDetector::new(self.config.congestion_rtt_multiplier, self.config.congestion_recovery_ratio)
        });
        let was_congested = detector.is_congested();
        detector.record_rtt(rtt_ms);
        if !was_congested && detector.is_congested() {
            self.metrics.congestion_events += 1;
        }
    }

    /// Dequeue next message to send.
    pub fn next_to_send(&mut self) -> Option<QueuedMessage> {
        // Starvation prevention
        if self.queue.low_starving() {
            if let Some(msg) = self.queue.dequeue_low_forced() {
                self.metrics.starvation_promotions += 1;
                self.metrics.messages_sent_low += 1;
                return Some(msg);
            }
        }

        if let Some(msg) = self.queue.dequeue() {
            match msg.priority {
                MessagePriority::Critical => self.metrics.messages_sent_critical += 1,
                MessagePriority::Normal => self.metrics.messages_sent_normal += 1,
                MessagePriority::Low => self.metrics.messages_sent_low += 1,
            }
            Some(msg)
        } else {
            None
        }
    }

    /// Get current state for observability.
    pub fn state(&self) -> QosState {
        QosState {
            enabled: self.config.enabled,
            inference_active: self.inference_active.load(Ordering::Relaxed),
            queue_depth_critical: self.queue.depth(MessagePriority::Critical),
            queue_depth_normal: self.queue.depth(MessagePriority::Normal),
            queue_depth_low: self.queue.depth(MessagePriority::Low),
            congested_peers: self.congestion.iter().filter(|(_, d)| d.is_congested()).count(),
            rate_limiter_available_bytes: self.rate_limiter.available(),
            metrics: self.metrics.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct QosState {
    pub enabled: bool,
    pub inference_active: bool,
    pub queue_depth_critical: usize,
    pub queue_depth_normal: usize,
    pub queue_depth_low: usize,
    pub congested_peers: usize,
    pub rate_limiter_available_bytes: u64,
    pub metrics: QosMetrics,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}


// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_priority_queue_ordering() {
        let mut q = PriorityQueue::new(QosConfig::default());
        q.enqueue(QueuedMessage { target: Uuid::new_v4(), payload_bytes: 10, priority: MessagePriority::Low, request_type: RequestType::MetricProbe, enqueued_at_ms: 0 }).unwrap();
        q.enqueue(QueuedMessage { target: Uuid::new_v4(), payload_bytes: 10, priority: MessagePriority::Normal, request_type: RequestType::InferenceRequest, enqueued_at_ms: 0 }).unwrap();
        q.enqueue(QueuedMessage { target: Uuid::new_v4(), payload_bytes: 10, priority: MessagePriority::Critical, request_type: RequestType::InferenceActivation, enqueued_at_ms: 0 }).unwrap();

        assert_eq!(q.dequeue().unwrap().priority, MessagePriority::Critical);
        assert_eq!(q.dequeue().unwrap().priority, MessagePriority::Normal);
        assert_eq!(q.dequeue().unwrap().priority, MessagePriority::Low);
    }

    #[test]
    fn test_queue_max_depth_low() {
        let mut config = QosConfig::default();
        config.low_queue_max = 2;
        let mut q = PriorityQueue::new(config);

        q.enqueue(QueuedMessage { target: Uuid::new_v4(), payload_bytes: 10, priority: MessagePriority::Low, request_type: RequestType::Heartbeat, enqueued_at_ms: 0 }).unwrap();
        q.enqueue(QueuedMessage { target: Uuid::new_v4(), payload_bytes: 10, priority: MessagePriority::Low, request_type: RequestType::Heartbeat, enqueued_at_ms: 0 }).unwrap();
        let result = q.enqueue(QueuedMessage { target: Uuid::new_v4(), payload_bytes: 10, priority: MessagePriority::Low, request_type: RequestType::Heartbeat, enqueued_at_ms: 0 });
        assert!(result.is_err()); // Queue full
    }

    #[test]
    fn test_token_bucket_consume() {
        let mut bucket = TokenBucket::new(1000, 2000); // 1KB/s, 2KB burst
        assert!(bucket.try_consume(1000)); // Use 1KB of burst
        assert!(bucket.try_consume(1000)); // Use remaining burst
        assert!(!bucket.try_consume(1)); // Empty
    }

    #[test]
    fn test_token_bucket_unlimited() {
        let mut bucket = TokenBucket::new(100, 100);
        bucket.set_unlimited();
        assert!(bucket.try_consume(999_999_999)); // Always succeeds
    }

    #[test]
    fn test_congestion_detection() {
        let mut detector = CongestionDetector::new(2.0, 1.2);
        // Establish baseline
        for _ in 0..10 { detector.record_rtt(10.0); }
        assert!(!detector.is_congested());

        // Spike RTT
        detector.record_rtt(25.0); // > 2× baseline
        assert!(detector.is_congested());

        // Recover
        for _ in 0..20 { detector.record_rtt(10.0); }
        assert!(!detector.is_congested());
    }

    #[test]
    fn test_dscp_tos_bytes() {
        assert_eq!(DscpMarker::tos_byte(MessagePriority::Critical), 0xB8);
        assert_eq!(DscpMarker::tos_byte(MessagePriority::Normal), 0x48);
        assert_eq!(DscpMarker::tos_byte(MessagePriority::Low), 0x00);
    }

    #[test]
    fn test_qos_layer_fast_path() {
        let mut qos = QosLayer::new(QosConfig::default());
        assert!(qos.is_fast_path(&RequestType::InferenceActivation));
        assert!(!qos.is_fast_path(&RequestType::Heartbeat));
        assert!(!qos.is_fast_path(&RequestType::ModelTransfer));
    }

    #[test]
    fn test_qos_layer_submit_and_dequeue() {
        let mut qos = QosLayer::new(QosConfig::default());
        let target = Uuid::new_v4();

        qos.submit(target, 100, MessagePriority::Normal, RequestType::InferenceRequest);
        qos.submit(target, 50, MessagePriority::Critical, RequestType::InferenceActivation);

        // Fast-path: Critical InferenceActivation returns SendNow (not queued)
        // So only Normal is in queue
        let msg = qos.next_to_send().unwrap();
        assert_eq!(msg.priority, MessagePriority::Normal);
    }

    #[test]
    fn test_qos_layer_throttle_during_inference() {
        let mut qos = QosLayer::new(QosConfig::default());
        qos.set_inference_active(true);

        // Exhaust rate limiter
        let target = Uuid::new_v4();
        for _ in 0..100 {
            qos.rate_limiter.try_consume(100_000); // Drain tokens
        }

        let decision = qos.submit(target, 1000, MessagePriority::Low, RequestType::ModelTransfer);
        assert!(matches!(decision, SendDecision::Throttled { .. }));
    }

    #[test]
    fn test_qos_layer_congestion_blocks_low() {
        let mut qos = QosLayer::new(QosConfig::default());
        let target = Uuid::new_v4();

        // Create congestion
        let detector = CongestionDetector::new(2.0, 1.2);
        qos.congestion.insert(target, detector);
        for _ in 0..10 { qos.congestion.get_mut(&target).unwrap().record_rtt(5.0); }
        qos.congestion.get_mut(&target).unwrap().record_rtt(15.0); // Spike

        let decision = qos.submit(target, 100, MessagePriority::Low, RequestType::Heartbeat);
        assert!(matches!(decision, SendDecision::Congested { .. }));
    }

    #[test]
    fn test_qos_disabled_always_send_now() {
        let mut config = QosConfig::default();
        config.enabled = false;
        let mut qos = QosLayer::new(config);

        let decision = qos.submit(Uuid::new_v4(), 100, MessagePriority::Low, RequestType::Heartbeat);
        assert_eq!(decision, SendDecision::SendNow);
    }

    #[test]
    fn test_qos_metrics_tracking() {
        let mut qos = QosLayer::new(QosConfig::default());
        let target = Uuid::new_v4();

        qos.submit(target, 100, MessagePriority::Normal, RequestType::InferenceRequest);
        qos.next_to_send();

        assert_eq!(qos.metrics.messages_sent_normal, 1);
    }

    #[test]
    fn test_qos_state_reporting() {
        let qos = QosLayer::new(QosConfig::default());
        let state = qos.state();
        assert!(state.enabled);
        assert!(!state.inference_active);
        assert_eq!(state.queue_depth_critical, 0);
    }

    #[test]
    fn test_low_starvation_detection() {
        let mut config = QosConfig::default();
        config.starvation_timeout_ms = 0; // Immediate starvation for testing
        let mut q = PriorityQueue::new(config);

        q.enqueue(QueuedMessage { target: Uuid::new_v4(), payload_bytes: 10, priority: MessagePriority::Low, request_type: RequestType::Heartbeat, enqueued_at_ms: 0 }).unwrap();

        // With timeout=0, Low is immediately "starving"
        assert!(q.low_starving());

        // Force dequeue should work
        let msg = q.dequeue_low_forced();
        assert!(msg.is_some());
        assert_eq!(msg.unwrap().priority, MessagePriority::Low);
    }

    #[test]
    fn test_starvation_promotion_in_qos_layer() {
        let mut config = QosConfig::default();
        config.starvation_timeout_ms = 0; // Immediate
        let mut qos = QosLayer::new(config);
        let target = Uuid::new_v4();

        // Enqueue Low message
        qos.submit(target, 100, MessagePriority::Low, RequestType::Heartbeat);

        // next_to_send should promote it due to starvation
        let msg = qos.next_to_send();
        assert!(msg.is_some());
        assert_eq!(msg.unwrap().priority, MessagePriority::Low);
        assert_eq!(qos.metrics.starvation_promotions, 1);
    }
}

#[cfg(test)]
mod property_tests {
    use super::*;
    use proptest::prelude::*;

    // P1: Priority Ordering — Critical always dequeued before Normal/Low
    proptest! {
        #[test]
        fn prop_priority_ordering(
            critical_count in 0usize..5,
            normal_count in 0usize..10,
            low_count in 0usize..10
        ) {
            let mut q = PriorityQueue::new(QosConfig::default());
            let target = Uuid::new_v4();

            for _ in 0..low_count {
                let _ = q.enqueue(QueuedMessage { target, payload_bytes: 10, priority: MessagePriority::Low, request_type: RequestType::Heartbeat, enqueued_at_ms: 0 });
            }
            for _ in 0..normal_count {
                let _ = q.enqueue(QueuedMessage { target, payload_bytes: 10, priority: MessagePriority::Normal, request_type: RequestType::InferenceRequest, enqueued_at_ms: 0 });
            }
            for _ in 0..critical_count {
                let _ = q.enqueue(QueuedMessage { target, payload_bytes: 10, priority: MessagePriority::Critical, request_type: RequestType::InferenceActivation, enqueued_at_ms: 0 });
            }

            let mut last_priority = MessagePriority::Critical;
            while let Some(msg) = q.dequeue() {
                // Priority should never increase (Critical=2 > Normal=1 > Low=0)
                prop_assert!(msg.priority <= last_priority || last_priority == MessagePriority::Low);
                last_priority = msg.priority;
            }
        }
    }

    // P3: Rate Bound — token bucket never allows more than rate + burst
    proptest! {
        #[test]
        fn prop_rate_bound(
            rate in 100u64..10000,
            burst in 100u64..10000,
            consume_amounts in prop::collection::vec(1u64..1000, 1..50)
        ) {
            let mut bucket = TokenBucket::new(rate, burst);
            let mut total_consumed: u64 = 0;

            for amount in &consume_amounts {
                if bucket.try_consume(*amount) {
                    total_consumed += amount;
                }
            }

            // Total consumed should never exceed burst (since no time passes in test)
            prop_assert!(total_consumed <= burst, "Consumed {} > burst {}", total_consumed, burst);
        }
    }

    // P4: Congestion Response — RTT spike always triggers congested state
    proptest! {
        #[test]
        fn prop_congestion_response(
            baseline in 1.0f64..100.0,
            multiplier in 1.5f64..5.0
        ) {
            let mut detector = CongestionDetector::new(multiplier, 1.2);

            // Establish baseline
            for _ in 0..20 { detector.record_rtt(baseline); }
            prop_assert!(!detector.is_congested());

            // Spike above threshold
            let spike = baseline * multiplier * 1.5; // Well above threshold
            detector.record_rtt(spike);
            prop_assert!(detector.is_congested(), "Should be congested at RTT {} (baseline {}, multiplier {})", spike, baseline, multiplier);
        }
    }

    // P5: Fast-Path Bypass — InferenceActivation never queued
    proptest! {
        #[test]
        fn prop_fast_path_bypass(_seed in any::<u64>()) {
            let mut qos = QosLayer::new(QosConfig::default());
            let target = Uuid::new_v4();

            let decision = qos.submit(target, 1000, MessagePriority::Critical, RequestType::InferenceActivation);
            prop_assert_eq!(decision, SendDecision::SendNow);

            // Queue should be empty (fast-path bypassed it)
            prop_assert!(qos.queue.is_empty());
        }
    }
}
