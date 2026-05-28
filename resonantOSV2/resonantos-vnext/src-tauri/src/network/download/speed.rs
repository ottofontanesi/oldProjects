// Intent citation: .kiro/specs/model-download-engine/design.md — SpeedTracker
// Sliding-window speed calculation and ETA estimation.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// Tracks download speed using a sliding window of byte samples.
/// Provides current speed (bytes/sec) and ETA estimation.
pub struct SpeedTracker {
    /// Samples of (timestamp, cumulative_bytes_at_that_time).
    samples: VecDeque<(Instant, u64)>,
    /// Window duration for speed calculation (default: 5 seconds).
    window: Duration,
}

impl SpeedTracker {
    /// Create a new speed tracker with a 5-second sliding window.
    pub fn new() -> Self {
        Self {
            samples: VecDeque::new(),
            window: Duration::from_secs(5),
        }
    }

    /// Create a speed tracker with a custom window duration.
    pub fn with_window(window: Duration) -> Self {
        Self {
            samples: VecDeque::new(),
            window,
        }
    }

    /// Record a cumulative byte count at the current time.
    pub fn record(&mut self, cumulative_bytes: u64) {
        let now = Instant::now();
        self.samples.push_back((now, cumulative_bytes));
        // Remove samples older than the window
        while self
            .samples
            .front()
            .map(|(t, _)| now.duration_since(*t) > self.window)
            .unwrap_or(false)
        {
            self.samples.pop_front();
        }
    }

    /// Get the current download speed in bytes per second.
    /// Returns 0 if insufficient samples.
    pub fn speed_bps(&self) -> u64 {
        if self.samples.len() < 2 {
            return 0;
        }
        let (first_time, first_bytes) = self.samples.front().unwrap();
        let (last_time, last_bytes) = self.samples.back().unwrap();
        let elapsed = last_time.duration_since(*first_time).as_secs_f64();
        if elapsed < 0.01 {
            return 0;
        }
        let bytes_transferred = last_bytes.saturating_sub(*first_bytes);
        (bytes_transferred as f64 / elapsed) as u64
    }

    /// Estimate time remaining in seconds given remaining bytes.
    /// Returns u64::MAX if speed is zero (unknown ETA).
    pub fn eta_secs(&self, remaining_bytes: u64) -> u64 {
        let speed = self.speed_bps();
        if speed == 0 {
            return u64::MAX;
        }
        remaining_bytes / speed
    }

    /// Reset the tracker (e.g., after a pause/resume).
    pub fn reset(&mut self) {
        self.samples.clear();
    }
}

impl Default for SpeedTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_tracker_returns_zero() {
        let tracker = SpeedTracker::new();
        assert_eq!(tracker.speed_bps(), 0);
        assert_eq!(tracker.eta_secs(1000), u64::MAX);
    }

    #[test]
    fn test_single_sample_returns_zero() {
        let mut tracker = SpeedTracker::new();
        tracker.record(1000);
        assert_eq!(tracker.speed_bps(), 0);
    }

    #[test]
    fn test_speed_calculation() {
        let mut tracker = SpeedTracker::with_window(Duration::from_secs(10));

        // Manually insert samples with known timing
        let now = Instant::now();
        tracker.samples.push_back((now - Duration::from_secs(2), 0));
        tracker.samples.push_back((now - Duration::from_secs(1), 1_000_000));
        tracker.samples.push_back((now, 2_000_000));

        let speed = tracker.speed_bps();
        // 2MB over 2 seconds = ~1MB/s
        assert!(speed > 900_000 && speed < 1_100_000, "Speed was {}", speed);
    }

    #[test]
    fn test_eta_calculation() {
        let mut tracker = SpeedTracker::with_window(Duration::from_secs(10));

        let now = Instant::now();
        tracker.samples.push_back((now - Duration::from_secs(1), 0));
        tracker.samples.push_back((now, 1_000_000));

        // Speed is ~1MB/s, 10MB remaining = ~10s ETA
        let eta = tracker.eta_secs(10_000_000);
        assert!(eta >= 9 && eta <= 11, "ETA was {}", eta);
    }

    #[test]
    fn test_reset_clears_samples() {
        let mut tracker = SpeedTracker::new();
        tracker.record(1000);
        tracker.record(2000);
        assert!(tracker.samples.len() >= 2);

        tracker.reset();
        assert_eq!(tracker.samples.len(), 0);
        assert_eq!(tracker.speed_bps(), 0);
    }

    #[test]
    fn test_window_eviction() {
        let mut tracker = SpeedTracker::with_window(Duration::from_millis(100));

        // Insert an old sample
        let now = Instant::now();
        tracker
            .samples
            .push_back((now - Duration::from_secs(5), 0));

        // Record a new sample — should evict the old one
        tracker.record(1000);
        assert_eq!(tracker.samples.len(), 1);
    }
}
