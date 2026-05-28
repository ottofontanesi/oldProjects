// Intent citation: .kiro/specs/network-simulator/design.md
// VirtualClock — controllable time source for deterministic simulation

/// A controllable clock for deterministic simulation.
/// All simulator components use this instead of system time.
/// Time is represented as seconds since simulation start (epoch = 0).
#[derive(Debug, Clone)]
pub struct VirtualClock {
    /// Current virtual time in seconds since simulation start.
    current_secs: u64,
}

impl VirtualClock {
    /// Create a new clock starting at time 0.
    pub fn new() -> Self {
        Self { current_secs: 0 }
    }

    /// Create a clock starting at a specific time.
    pub fn new_at(start_secs: u64) -> Self {
        Self {
            current_secs: start_secs,
        }
    }

    /// Get current virtual time in seconds.
    pub fn now_secs(&self) -> u64 {
        self.current_secs
    }

    /// Get current virtual time in milliseconds.
    pub fn now_ms(&self) -> u64 {
        self.current_secs * 1000
    }

    /// Advance the clock by the given number of seconds.
    pub fn advance(&mut self, secs: u64) {
        self.current_secs += secs;
    }

    /// Advance the clock by the given number of milliseconds.
    pub fn advance_ms(&mut self, ms: u64) {
        // For sub-second precision, we store in seconds but accept ms input
        // Since our simulation granularity is seconds, we round up
        self.current_secs += (ms + 999) / 1000;
    }

    /// Set the clock to a specific time (for test setup).
    pub fn set_secs(&mut self, secs: u64) {
        self.current_secs = secs;
    }

    /// Check if a given timestamp (in seconds) has passed.
    pub fn has_passed(&self, timestamp_secs: u64) -> bool {
        self.current_secs >= timestamp_secs
    }

    /// Get seconds elapsed since a given timestamp.
    pub fn elapsed_since(&self, timestamp_secs: u64) -> u64 {
        self.current_secs.saturating_sub(timestamp_secs)
    }
}

impl Default for VirtualClock {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_starts_at_zero() {
        let clock = VirtualClock::new();
        assert_eq!(clock.now_secs(), 0);
        assert_eq!(clock.now_ms(), 0);
    }

    #[test]
    fn test_new_at_specific_time() {
        let clock = VirtualClock::new_at(100);
        assert_eq!(clock.now_secs(), 100);
        assert_eq!(clock.now_ms(), 100_000);
    }

    #[test]
    fn test_advance() {
        let mut clock = VirtualClock::new();
        clock.advance(30);
        assert_eq!(clock.now_secs(), 30);

        clock.advance(15);
        assert_eq!(clock.now_secs(), 45);
    }

    #[test]
    fn test_set() {
        let mut clock = VirtualClock::new();
        clock.set_secs(500);
        assert_eq!(clock.now_secs(), 500);
    }

    #[test]
    fn test_has_passed() {
        let mut clock = VirtualClock::new();
        clock.set_secs(100);

        assert!(clock.has_passed(50));
        assert!(clock.has_passed(100));
        assert!(!clock.has_passed(101));
    }

    #[test]
    fn test_elapsed_since() {
        let mut clock = VirtualClock::new();
        clock.set_secs(100);

        assert_eq!(clock.elapsed_since(80), 20);
        assert_eq!(clock.elapsed_since(100), 0);
        assert_eq!(clock.elapsed_since(150), 0); // saturating_sub
    }

    #[test]
    fn test_deterministic() {
        // Same operations produce same results (no system time dependency)
        let mut clock1 = VirtualClock::new();
        let mut clock2 = VirtualClock::new();

        clock1.advance(10);
        clock1.advance(20);

        clock2.advance(10);
        clock2.advance(20);

        assert_eq!(clock1.now_secs(), clock2.now_secs());
    }
}
