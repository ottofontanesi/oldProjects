// WireGuard adapter metrics — aggregate and per-tunnel tracking.

/// Aggregate metrics for the WireGuard adapter.
#[derive(Debug, Clone)]
pub struct WgMetrics {
    pub total_bytes_sent: u64,
    pub total_bytes_received: u64,
    pub total_packets_sent: u64,
    pub total_packets_received: u64,
    pub total_errors: u64,
    pub tunnels_established: u64,
    pub tunnels_closed: u64,
    pub handshakes_completed: u64,
    pub handshakes_failed: u64,
}

impl WgMetrics {
    pub fn new() -> Self {
        Self {
            total_bytes_sent: 0,
            total_bytes_received: 0,
            total_packets_sent: 0,
            total_packets_received: 0,
            total_errors: 0,
            tunnels_established: 0,
            tunnels_closed: 0,
            handshakes_completed: 0,
            handshakes_failed: 0,
        }
    }

    /// Record a successful send.
    pub fn record_send(&mut self, bytes: u64) {
        self.total_bytes_sent += bytes;
        self.total_packets_sent += 1;
    }

    /// Record a successful receive.
    pub fn record_receive(&mut self, bytes: u64) {
        self.total_bytes_received += bytes;
        self.total_packets_received += 1;
    }

    /// Record an error.
    pub fn record_error(&mut self) {
        self.total_errors += 1;
    }

    /// Record a tunnel establishment.
    pub fn record_tunnel_established(&mut self) {
        self.tunnels_established += 1;
    }

    /// Record a tunnel closure.
    pub fn record_tunnel_closed(&mut self) {
        self.tunnels_closed += 1;
    }

    /// Compute error rate as a percentage.
    pub fn error_rate_percent(&self) -> f64 {
        let total = self.total_packets_sent + self.total_packets_received;
        if total == 0 {
            return 0.0;
        }
        (self.total_errors as f64 / total as f64) * 100.0
    }

    /// Compute total throughput in MB.
    pub fn total_throughput_mb(&self) -> f64 {
        (self.total_bytes_sent + self.total_bytes_received) as f64 / (1024.0 * 1024.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_metrics_zeroed() {
        let m = WgMetrics::new();
        assert_eq!(m.total_bytes_sent, 0);
        assert_eq!(m.error_rate_percent(), 0.0);
    }

    #[test]
    fn test_record_send() {
        let mut m = WgMetrics::new();
        m.record_send(1024);
        assert_eq!(m.total_bytes_sent, 1024);
        assert_eq!(m.total_packets_sent, 1);
    }

    #[test]
    fn test_error_rate() {
        let mut m = WgMetrics::new();
        m.record_send(100);
        m.record_send(100);
        m.record_error();
        // 1 error out of 2 packets = 50%
        assert!((m.error_rate_percent() - 50.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_throughput() {
        let mut m = WgMetrics::new();
        m.total_bytes_sent = 1024 * 1024; // 1MB sent
        m.total_bytes_received = 1024 * 1024; // 1MB received
        assert!((m.total_throughput_mb() - 2.0).abs() < f64::EPSILON);
    }
}
