// Intent citation: .kiro/specs/local-network-optimizer/design.md Section 9
// Optimizer Lifecycle — periodic timer, event-driven triggers, debouncing, state persistence

use super::registry::NodeId;
use serde::{Deserialize, Serialize};

/// Events that trigger an optimization cycle.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum OptimizerEvent {
    /// Periodic timer fired (every 5 minutes by default).
    Timer,
    /// A new node joined the network.
    NodeJoined(NodeId),
    /// A node left the network (heartbeat timeout).
    NodeDeparted(NodeId),
    /// A model download completed on a node.
    DownloadCompleted { model_id: String, node_id: NodeId },
    /// User changed preferences.
    PreferencesChanged,
    /// Significant workload shift detected (>20% change in model shares).
    WorkloadShift,
    /// Manual trigger from UI ("Re-optimize Now" button).
    ManualTrigger,
    /// App startup (initial optimization).
    Startup,
}

/// Configuration for the optimizer lifecycle.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LifecycleConfig {
    /// Optimization interval in seconds (default: 300 = 5 minutes).
    pub optimization_interval_secs: u64,
    /// Debounce window: batch events within this window before triggering (default: 2s).
    pub debounce_window_secs: u64,
    /// Maximum solver timeout in milliseconds (default: 2000).
    pub solver_timeout_ms: u64,
    /// Whether the optimizer is enabled.
    pub enabled: bool,
}

impl Default for LifecycleConfig {
    fn default() -> Self {
        Self {
            optimization_interval_secs: 300,
            debounce_window_secs: 2,
            solver_timeout_ms: 2000,
            enabled: true,
        }
    }
}

/// Tracks optimizer lifecycle state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LifecycleState {
    /// Number of optimization cycles completed.
    pub cycles_completed: u64,
    /// Timestamp of last optimization (ms).
    pub last_optimization_ms: u64,
    /// Last event that triggered optimization.
    pub last_trigger: Option<OptimizerEvent>,
    /// Whether the optimizer is currently running a solve.
    pub is_solving: bool,
    /// Pending events waiting for debounce window to close.
    pub pending_events: Vec<OptimizerEvent>,
    /// Timestamp when debounce window opened (ms).
    pub debounce_start_ms: Option<u64>,
}

impl Default for LifecycleState {
    fn default() -> Self {
        Self {
            cycles_completed: 0,
            last_optimization_ms: 0,
            last_trigger: None,
            is_solving: false,
            pending_events: Vec::new(),
            debounce_start_ms: None,
        }
    }
}

impl LifecycleState {
    /// Check if the periodic timer should fire.
    pub fn should_trigger_timer(&self, current_time_ms: u64, interval_secs: u64) -> bool {
        let interval_ms = interval_secs * 1000;
        current_time_ms - self.last_optimization_ms >= interval_ms
    }

    /// Add an event to the pending queue (starts debounce if not already started).
    pub fn queue_event(&mut self, event: OptimizerEvent, current_time_ms: u64) {
        if self.debounce_start_ms.is_none() {
            self.debounce_start_ms = Some(current_time_ms);
        }
        self.pending_events.push(event);
    }

    /// Check if the debounce window has closed and events should be processed.
    pub fn should_process_events(&self, current_time_ms: u64, debounce_secs: u64) -> bool {
        if self.pending_events.is_empty() {
            return false;
        }
        match self.debounce_start_ms {
            None => false,
            Some(start) => current_time_ms - start >= debounce_secs * 1000,
        }
    }

    /// Drain pending events (called when debounce window closes).
    pub fn drain_events(&mut self) -> Vec<OptimizerEvent> {
        self.debounce_start_ms = None;
        std::mem::take(&mut self.pending_events)
    }

    /// Mark that an optimization cycle started.
    pub fn mark_solving(&mut self) {
        self.is_solving = true;
    }

    /// Mark that an optimization cycle completed.
    pub fn mark_completed(&mut self, current_time_ms: u64, trigger: OptimizerEvent) {
        self.is_solving = false;
        self.cycles_completed += 1;
        self.last_optimization_ms = current_time_ms;
        self.last_trigger = Some(trigger);
    }

    /// Check if the optimizer is idle (not solving, no pending events).
    pub fn is_idle(&self) -> bool {
        !self.is_solving && self.pending_events.is_empty()
    }
}

/// Determine the highest-priority trigger from a batch of events.
pub fn prioritize_trigger(events: &[OptimizerEvent]) -> OptimizerEvent {
    // Priority order: ManualTrigger > NodeDeparted > NodeJoined > PreferencesChanged > others
    if events.iter().any(|e| matches!(e, OptimizerEvent::ManualTrigger)) {
        return OptimizerEvent::ManualTrigger;
    }
    if let Some(e) = events.iter().find(|e| matches!(e, OptimizerEvent::NodeDeparted(_))) {
        return e.clone();
    }
    if let Some(e) = events.iter().find(|e| matches!(e, OptimizerEvent::NodeJoined(_))) {
        return e.clone();
    }
    if events.iter().any(|e| matches!(e, OptimizerEvent::PreferencesChanged)) {
        return OptimizerEvent::PreferencesChanged;
    }
    events.first().cloned().unwrap_or(OptimizerEvent::Timer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_timer_trigger() {
        let state = LifecycleState {
            last_optimization_ms: 1000,
            ..Default::default()
        };

        // Not enough time passed (5 min = 300_000ms)
        assert!(!state.should_trigger_timer(100_000, 300));

        // Enough time passed
        assert!(state.should_trigger_timer(301_001, 300));
    }

    #[test]
    fn test_debounce_window() {
        let mut state = LifecycleState::default();

        // Queue event
        state.queue_event(OptimizerEvent::NodeJoined(uuid::Uuid::new_v4()), 1000);
        assert!(!state.should_process_events(1000, 2)); // Just started

        // Within debounce window
        assert!(!state.should_process_events(2500, 2)); // 1.5s < 2s

        // After debounce window
        assert!(state.should_process_events(3001, 2)); // 2s+ elapsed
    }

    #[test]
    fn test_drain_events() {
        let mut state = LifecycleState::default();
        let node = uuid::Uuid::new_v4();

        state.queue_event(OptimizerEvent::NodeJoined(node), 1000);
        state.queue_event(OptimizerEvent::PreferencesChanged, 1500);

        let events = state.drain_events();
        assert_eq!(events.len(), 2);
        assert!(state.pending_events.is_empty());
        assert!(state.debounce_start_ms.is_none());
    }

    #[test]
    fn test_mark_completed() {
        let mut state = LifecycleState::default();
        state.mark_solving();
        assert!(state.is_solving);

        state.mark_completed(5000, OptimizerEvent::Timer);
        assert!(!state.is_solving);
        assert_eq!(state.cycles_completed, 1);
        assert_eq!(state.last_optimization_ms, 5000);
    }

    #[test]
    fn test_prioritize_trigger() {
        let node = uuid::Uuid::new_v4();

        // Manual trigger wins
        let events = vec![
            OptimizerEvent::Timer,
            OptimizerEvent::ManualTrigger,
            OptimizerEvent::NodeJoined(node),
        ];
        assert_eq!(prioritize_trigger(&events), OptimizerEvent::ManualTrigger);

        // NodeDeparted wins over NodeJoined
        let events = vec![
            OptimizerEvent::NodeJoined(node),
            OptimizerEvent::NodeDeparted(node),
        ];
        assert!(matches!(prioritize_trigger(&events), OptimizerEvent::NodeDeparted(_)));

        // Timer is lowest priority
        let events = vec![OptimizerEvent::Timer];
        assert_eq!(prioritize_trigger(&events), OptimizerEvent::Timer);
    }

    #[test]
    fn test_is_idle() {
        let mut state = LifecycleState::default();
        assert!(state.is_idle());

        state.mark_solving();
        assert!(!state.is_idle());

        state.is_solving = false;
        state.queue_event(OptimizerEvent::Timer, 1000);
        assert!(!state.is_idle()); // Has pending events
    }
}
