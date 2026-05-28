// Intent citation: .kiro/specs/lan-transport-adapter/design.md — Heartbeat Monitor
// HeartbeatMonitor, ping/pong protocol for peer liveness detection.

use super::codec::encode_frame;
use super::connection::ConnectionPool;
use super::peer::PeerRegistry;
use super::WireMessage;
use crate::transport::trait_def::NodeId;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

/// Tracks pending pings and computes RTT.
#[derive(Debug, Clone)]
pub struct PingState {
    /// Timestamp (ns) of the last sent ping.
    pub last_ping_ns: u64,
    /// When the ping was sent (for RTT).
    pub sent_at: Instant,
    /// Number of consecutive missed pongs.
    pub missed_count: u8,
}

/// Heartbeat monitor that periodically pings connected peers and detects offline nodes.
pub struct HeartbeatMonitor {
    /// Interval between heartbeat pings.
    heartbeat_interval: Duration,
    /// Number of missed pings before marking offline.
    timeout_count: u8,
    /// Idle keepalive duration.
    idle_keepalive: Duration,
    /// Pending ping states per peer.
    ping_states: Arc<Mutex<HashMap<NodeId, PingState>>>,
    /// Handle to the heartbeat task.
    task_handle: Mutex<Option<JoinHandle<()>>>,
    /// Whether the monitor is running.
    is_running: Arc<std::sync::atomic::AtomicBool>,
}

impl HeartbeatMonitor {
    /// Create a new HeartbeatMonitor.
    pub fn new(
        heartbeat_interval: Duration,
        timeout_count: u8,
        idle_keepalive: Duration,
    ) -> Self {
        Self {
            heartbeat_interval,
            timeout_count,
            idle_keepalive,
            ping_states: Arc::new(Mutex::new(HashMap::new())),
            task_handle: Mutex::new(None),
            is_running: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    /// Start the heartbeat monitor.
    /// Spawns a tokio task that periodically pings all connected peers.
    pub async fn start(
        &self,
        peers: Arc<PeerRegistry>,
        pool: Arc<ConnectionPool>,
    ) {
        self.is_running
            .store(true, std::sync::atomic::Ordering::SeqCst);

        let interval = self.heartbeat_interval;
        let timeout_count = self.timeout_count;
        let idle_keepalive = self.idle_keepalive;
        let ping_states = self.ping_states.clone();
        let is_running = self.is_running.clone();

        let handle = tokio::spawn(async move {
            Self::heartbeat_loop(
                peers,
                pool,
                ping_states,
                is_running,
                interval,
                timeout_count,
                idle_keepalive,
            )
            .await;
        });

        let mut h = self.task_handle.lock().await;
        *h = Some(handle);
    }

    /// The main heartbeat loop.
    async fn heartbeat_loop(
        peers: Arc<PeerRegistry>,
        pool: Arc<ConnectionPool>,
        ping_states: Arc<Mutex<HashMap<NodeId, PingState>>>,
        is_running: Arc<std::sync::atomic::AtomicBool>,
        interval: Duration,
        timeout_count: u8,
        idle_keepalive: Duration,
    ) {
        let mut ticker = tokio::time::interval(interval);

        while is_running.load(std::sync::atomic::Ordering::SeqCst) {
            ticker.tick().await;

            if !is_running.load(std::sync::atomic::Ordering::SeqCst) {
                break;
            }

            let connected = peers.connected_peers();

            for peer_id in &connected {
                // Check idle keepalive
                if let Some(conn) = pool.get_connection(peer_id) {
                    let idle = conn.idle_duration().await;
                    if idle < idle_keepalive && idle < interval {
                        // Connection is active, skip ping this round
                        continue;
                    }
                }

                // Send ping
                let timestamp_ns = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos() as u64;

                let ping = WireMessage::Ping { timestamp_ns };
                if let Ok(frame) = encode_frame(&ping) {
                    let payload = &frame[4..]; // Skip length header, send_framed adds its own
                    let send_result = pool.send_framed(peer_id, payload).await;

                    let mut states = ping_states.lock().await;
                    let state = states.entry(*peer_id).or_insert(PingState {
                        last_ping_ns: 0,
                        sent_at: Instant::now(),
                        missed_count: 0,
                    });

                    if send_result.is_ok() {
                        // Increment missed count (will be reset on pong)
                        state.missed_count += 1;
                        state.last_ping_ns = timestamp_ns;
                        state.sent_at = Instant::now();
                    } else {
                        state.missed_count += 1;
                    }

                    // Check if peer should be marked offline
                    if state.missed_count >= timeout_count {
                        peers.mark_offline(peer_id);
                        pool.close(peer_id).await;
                        states.remove(peer_id);
                    }
                }
            }
        }
    }

    /// Record a pong received from a peer.
    /// Computes RTT and resets the missed heartbeat counter.
    pub async fn record_pong(&self, peer_id: &NodeId, _timestamp_ns: u64) -> Option<f64> {
        let mut states = self.ping_states.lock().await;
        if let Some(state) = states.get_mut(peer_id) {
            let rtt_ms = state.sent_at.elapsed().as_secs_f64() * 1000.0;
            state.missed_count = 0;
            Some(rtt_ms)
        } else {
            None
        }
    }

    /// Stop the heartbeat monitor.
    pub async fn stop(&self) {
        self.is_running
            .store(false, std::sync::atomic::Ordering::SeqCst);

        if let Some(handle) = self.task_handle.lock().await.take() {
            handle.abort();
        }
    }

    /// Check if the monitor is running.
    pub fn is_running(&self) -> bool {
        self.is_running.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Get the missed heartbeat count for a peer.
    pub async fn missed_count(&self, peer_id: &NodeId) -> u8 {
        let states = self.ping_states.lock().await;
        states.get(peer_id).map(|s| s.missed_count).unwrap_or(0)
    }
}

/// Simulates the heartbeat state machine for testing.
/// Given a sequence of heartbeat responses (true = pong received, false = missed),
/// returns whether the peer should be marked offline.
pub fn simulate_heartbeat_state(responses: &[bool], timeout_count: u8) -> bool {
    let mut missed = 0u8;
    for &received in responses {
        if received {
            missed = 0;
        } else {
            missed += 1;
        }
        if missed >= timeout_count {
            return true;
        }
    }
    false
}

/// Given a ping timestamp, produce the pong response timestamp (echo).
pub fn pong_echo_timestamp(ping_timestamp_ns: u64) -> u64 {
    ping_timestamp_ns
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simulate_heartbeat_all_received() {
        let responses = vec![true, true, true, true, true];
        assert!(!simulate_heartbeat_state(&responses, 3));
    }

    #[test]
    fn test_simulate_heartbeat_three_missed() {
        let responses = vec![true, false, false, false];
        assert!(simulate_heartbeat_state(&responses, 3));
    }

    #[test]
    fn test_simulate_heartbeat_reset_on_pong() {
        // Miss 2, then receive, then miss 2 more — should NOT be offline
        let responses = vec![false, false, true, false, false];
        assert!(!simulate_heartbeat_state(&responses, 3));
    }

    #[test]
    fn test_simulate_heartbeat_exactly_at_threshold() {
        let responses = vec![false, false, false];
        assert!(simulate_heartbeat_state(&responses, 3));
    }

    #[test]
    fn test_simulate_heartbeat_below_threshold() {
        let responses = vec![false, false];
        assert!(!simulate_heartbeat_state(&responses, 3));
    }

    #[test]
    fn test_pong_echo_timestamp() {
        assert_eq!(pong_echo_timestamp(123456789), 123456789);
        assert_eq!(pong_echo_timestamp(0), 0);
        assert_eq!(pong_echo_timestamp(u64::MAX), u64::MAX);
    }

    #[test]
    fn test_heartbeat_monitor_creation() {
        let monitor = HeartbeatMonitor::new(
            Duration::from_secs(10),
            3,
            Duration::from_secs(60),
        );
        assert!(!monitor.is_running());
    }

    #[tokio::test]
    async fn test_heartbeat_monitor_record_pong_no_state() {
        let monitor = HeartbeatMonitor::new(
            Duration::from_secs(10),
            3,
            Duration::from_secs(60),
        );
        let peer_id = uuid::Uuid::new_v4();
        let result = monitor.record_pong(&peer_id, 12345).await;
        assert!(result.is_none());
    }
}
