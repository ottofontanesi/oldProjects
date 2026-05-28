// Optimizer Timer — spawns the 60-second optimizer cycle.
//
// First cycle after 5-second delay. Skips if previous cycle still running.
// Supports pause/resume/stop.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

/// State of the optimizer timer.
#[derive(Debug, Clone, PartialEq)]
pub enum TimerState {
    Stopped,
    Running,
    Paused,
}

/// Configuration for the optimizer timer.
#[derive(Debug, Clone)]
pub struct OptimizerTimerConfig {
    /// Interval between cycles in seconds.
    pub interval_secs: u64,
    /// Delay before first cycle in seconds.
    pub initial_delay_secs: u64,
    /// Maximum cycle duration before warning (ms).
    pub max_cycle_duration_ms: u64,
}

impl Default for OptimizerTimerConfig {
    fn default() -> Self {
        Self {
            interval_secs: 60,
            initial_delay_secs: 5,
            max_cycle_duration_ms: 30_000,
        }
    }
}

/// Metrics for the optimizer timer.
#[derive(Debug, Clone)]
pub struct TimerMetrics {
    pub total_cycles: u64,
    pub skipped_cycles: u64,
    pub avg_cycle_ms: f64,
    pub max_cycle_ms: u64,
    pub last_cycle_ms: u64,
    pub last_cycle_at_ms: u64,
}

/// The optimizer timer — manages the periodic optimizer cycle.
pub struct OptimizerTimer {
    config: OptimizerTimerConfig,
    state: TimerState,
    is_cycle_running: Arc<AtomicBool>,
    total_cycles: u64,
    skipped_cycles: u64,
    avg_cycle_ms: f64,
    max_cycle_ms: u64,
    last_cycle_ms: u64,
    last_cycle_at_ms: u64,
}

impl OptimizerTimer {
    /// Create a new timer (not started).
    pub fn new(config: OptimizerTimerConfig) -> Self {
        Self {
            config,
            state: TimerState::Stopped,
            is_cycle_running: Arc::new(AtomicBool::new(false)),
            total_cycles: 0,
            skipped_cycles: 0,
            avg_cycle_ms: 0.0,
            max_cycle_ms: 0,
            last_cycle_ms: 0,
            last_cycle_at_ms: 0,
        }
    }

    /// Start the timer.
    pub fn start(&mut self) {
        self.state = TimerState::Running;
        eprintln!(
            "[optimizer_timer] Started: {}s interval, {}s initial delay",
            self.config.interval_secs, self.config.initial_delay_secs
        );
    }

    /// Pause the timer (cycles stop but state preserved).
    pub fn pause(&mut self) {
        self.state = TimerState::Paused;
    }

    /// Resume a paused timer.
    pub fn resume(&mut self) {
        if self.state == TimerState::Paused {
            self.state = TimerState::Running;
        }
    }

    /// Stop the timer completely.
    pub fn stop(&mut self) {
        self.state = TimerState::Stopped;
    }

    /// Check if a cycle should run now.
    pub fn should_run_cycle(&self) -> bool {
        if self.state != TimerState::Running {
            return false;
        }
        !self.is_cycle_running.load(Ordering::Relaxed)
    }

    /// Record that a cycle started.
    pub fn begin_cycle(&self) {
        self.is_cycle_running.store(true, Ordering::Relaxed);
    }

    /// Record that a cycle completed.
    pub fn end_cycle(&mut self, duration_ms: u64) {
        self.is_cycle_running.store(false, Ordering::Relaxed);
        self.total_cycles += 1;
        self.last_cycle_ms = duration_ms;
        self.last_cycle_at_ms = now_ms();

        // Update running average
        self.avg_cycle_ms += (duration_ms as f64 - self.avg_cycle_ms) / self.total_cycles as f64;

        if duration_ms > self.max_cycle_ms {
            self.max_cycle_ms = duration_ms;
        }

        if duration_ms > self.config.max_cycle_duration_ms {
            eprintln!(
                "[optimizer_timer] WARNING: Cycle took {}ms (limit: {}ms)",
                duration_ms, self.config.max_cycle_duration_ms
            );
        }
    }

    /// Record a skipped cycle (previous still running).
    pub fn record_skip(&mut self) {
        self.skipped_cycles += 1;
    }

    /// Get current state.
    pub fn state(&self) -> &TimerState {
        &self.state
    }

    /// Get metrics.
    pub fn metrics(&self) -> TimerMetrics {
        TimerMetrics {
            total_cycles: self.total_cycles,
            skipped_cycles: self.skipped_cycles,
            avg_cycle_ms: self.avg_cycle_ms,
            max_cycle_ms: self.max_cycle_ms,
            last_cycle_ms: self.last_cycle_ms,
            last_cycle_at_ms: self.last_cycle_at_ms,
        }
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_timer_stopped() {
        let timer = OptimizerTimer::new(OptimizerTimerConfig::default());
        assert_eq!(*timer.state(), TimerState::Stopped);
        assert!(!timer.should_run_cycle());
    }

    #[test]
    fn test_start_enables_cycles() {
        let mut timer = OptimizerTimer::new(OptimizerTimerConfig::default());
        timer.start();
        assert_eq!(*timer.state(), TimerState::Running);
        assert!(timer.should_run_cycle());
    }

    #[test]
    fn test_pause_prevents_cycles() {
        let mut timer = OptimizerTimer::new(OptimizerTimerConfig::default());
        timer.start();
        timer.pause();
        assert!(!timer.should_run_cycle());
    }

    #[test]
    fn test_resume_after_pause() {
        let mut timer = OptimizerTimer::new(OptimizerTimerConfig::default());
        timer.start();
        timer.pause();
        timer.resume();
        assert!(timer.should_run_cycle());
    }

    #[test]
    fn test_cycle_running_prevents_overlap() {
        let mut timer = OptimizerTimer::new(OptimizerTimerConfig::default());
        timer.start();
        timer.begin_cycle();
        assert!(!timer.should_run_cycle()); // Blocked while running
    }

    #[test]
    fn test_end_cycle_records_metrics() {
        let mut timer = OptimizerTimer::new(OptimizerTimerConfig::default());
        timer.start();
        timer.begin_cycle();
        timer.end_cycle(150);

        let metrics = timer.metrics();
        assert_eq!(metrics.total_cycles, 1);
        assert_eq!(metrics.last_cycle_ms, 150);
        assert!((metrics.avg_cycle_ms - 150.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_skip_recorded() {
        let mut timer = OptimizerTimer::new(OptimizerTimerConfig::default());
        timer.record_skip();
        timer.record_skip();
        assert_eq!(timer.metrics().skipped_cycles, 2);
    }
}
