// Intent citation: .kiro/specs/local-network-optimizer/requirements.md FR-11
// Offline-First Resilience — internet detection, node disconnection handling, single-node fallback

use super::registry::NodeId;
use serde::{Deserialize, Serialize};

/// Network connectivity state.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ConnectivityState {
    /// Full connectivity: LAN + internet available.
    FullyConnected,
    /// LAN only: local nodes reachable, no internet (downloads paused).
    LanOnly,
    /// Single node: all remote nodes unreachable, operating independently.
    SingleNode,
}

/// Resilience manager tracks connectivity and triggers appropriate responses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResilienceState {
    pub connectivity: ConnectivityState,
    pub internet_available: bool,
    pub lan_nodes_reachable: u32,
    pub last_internet_check_ms: u64,
    pub last_connectivity_change_ms: u64,
    /// Nodes that were online but are now unreachable.
    pub recently_departed: Vec<(NodeId, u64)>, // (node_id, departed_at_ms)
}

impl Default for ResilienceState {
    fn default() -> Self {
        Self {
            connectivity: ConnectivityState::FullyConnected,
            internet_available: true,
            lan_nodes_reachable: 0,
            last_internet_check_ms: 0,
            last_connectivity_change_ms: 0,
            recently_departed: Vec::new(),
        }
    }
}

impl ResilienceState {
    /// Update connectivity state based on current conditions.
    pub fn update(
        &mut self,
        internet_available: bool,
        lan_nodes_online: u32,
        current_time_ms: u64,
    ) {
        self.internet_available = internet_available;
        self.lan_nodes_reachable = lan_nodes_online;
        self.last_internet_check_ms = current_time_ms;

        let new_state = if lan_nodes_online == 0 {
            ConnectivityState::SingleNode
        } else if !internet_available {
            ConnectivityState::LanOnly
        } else {
            ConnectivityState::FullyConnected
        };

        if new_state != self.connectivity {
            self.last_connectivity_change_ms = current_time_ms;
            self.connectivity = new_state;
        }
    }

    /// Record a node departure.
    pub fn record_departure(&mut self, node_id: NodeId, time_ms: u64) {
        self.recently_departed.push((node_id, time_ms));
        // Keep only last 10 departures
        if self.recently_departed.len() > 10 {
            self.recently_departed.remove(0);
        }
    }

    /// Check if we should trigger emergency re-optimization (node departed).
    pub fn needs_emergency_reoptimize(&self, current_time_ms: u64, max_delay_ms: u64) -> bool {
        self.recently_departed
            .iter()
            .any(|(_, departed_at)| current_time_ms - departed_at <= max_delay_ms)
    }

    /// Check if downloads should be paused (no internet).
    pub fn should_pause_downloads(&self) -> bool {
        !self.internet_available
    }

    /// Check if we're in single-node fallback mode.
    pub fn is_single_node_mode(&self) -> bool {
        self.connectivity == ConnectivityState::SingleNode
    }

    /// Get a human-readable status description.
    pub fn status_description(&self) -> String {
        match self.connectivity {
            ConnectivityState::FullyConnected => format!(
                "Fully connected: {} LAN nodes, internet available",
                self.lan_nodes_reachable
            ),
            ConnectivityState::LanOnly => format!(
                "LAN only: {} nodes reachable, no internet (downloads paused)",
                self.lan_nodes_reachable
            ),
            ConnectivityState::SingleNode => {
                "Single-node mode: all remote nodes unreachable, operating independently".to_string()
            }
        }
    }
}

/// Determine what actions to take when a node disconnects.
#[derive(Debug, Clone)]
pub struct DisconnectionResponse {
    pub trigger_reoptimize: bool,
    pub pause_downloads_to_node: bool,
    pub redistribute_models: bool,
    pub notify_user: bool,
    pub message: String,
}

/// Compute the appropriate response to a node disconnection.
pub fn handle_node_disconnection(
    _departed_node: NodeId,
    remaining_online: u32,
    models_on_departed: u32,
) -> DisconnectionResponse {
    if remaining_online == 0 {
        // All remote nodes gone — fall back to single-node
        DisconnectionResponse {
            trigger_reoptimize: true,
            pause_downloads_to_node: true,
            redistribute_models: false, // Nothing to redistribute to
            notify_user: true,
            message: "All remote nodes offline. Operating in single-node mode.".to_string(),
        }
    } else if models_on_departed > 0 {
        // Node had models — need to redistribute
        DisconnectionResponse {
            trigger_reoptimize: true,
            pause_downloads_to_node: true,
            redistribute_models: true,
            notify_user: true,
            message: format!(
                "Node disconnected with {} models. Re-optimizing placement.",
                models_on_departed
            ),
        }
    } else {
        // Node had no models — minimal impact
        DisconnectionResponse {
            trigger_reoptimize: false,
            pause_downloads_to_node: true,
            redistribute_models: false,
            notify_user: false,
            message: "Node disconnected (no models affected).".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connectivity_transitions() {
        let mut state = ResilienceState::default();

        // Start fully connected
        state.update(true, 3, 1000);
        assert_eq!(state.connectivity, ConnectivityState::FullyConnected);

        // Internet drops
        state.update(false, 3, 2000);
        assert_eq!(state.connectivity, ConnectivityState::LanOnly);
        assert!(state.should_pause_downloads());

        // All nodes drop
        state.update(false, 0, 3000);
        assert_eq!(state.connectivity, ConnectivityState::SingleNode);
        assert!(state.is_single_node_mode());

        // Recovery
        state.update(true, 2, 4000);
        assert_eq!(state.connectivity, ConnectivityState::FullyConnected);
        assert!(!state.is_single_node_mode());
    }

    #[test]
    fn test_emergency_reoptimize() {
        let mut state = ResilienceState::default();
        let node = uuid::Uuid::new_v4();

        state.record_departure(node, 5000);

        // Within 30s window
        assert!(state.needs_emergency_reoptimize(10_000, 30_000));

        // After 30s window
        assert!(!state.needs_emergency_reoptimize(40_000, 30_000));
    }

    #[test]
    fn test_disconnection_response_all_offline() {
        let response = handle_node_disconnection(uuid::Uuid::new_v4(), 0, 2);
        assert!(response.trigger_reoptimize);
        assert!(response.notify_user);
        assert!(!response.redistribute_models); // Nothing to redistribute to
    }

    #[test]
    fn test_disconnection_response_has_models() {
        let response = handle_node_disconnection(uuid::Uuid::new_v4(), 2, 3);
        assert!(response.trigger_reoptimize);
        assert!(response.redistribute_models);
        assert!(response.notify_user);
    }

    #[test]
    fn test_disconnection_response_no_models() {
        let response = handle_node_disconnection(uuid::Uuid::new_v4(), 2, 0);
        assert!(!response.trigger_reoptimize); // No urgency
        assert!(!response.redistribute_models);
        assert!(!response.notify_user);
    }

    #[test]
    fn test_status_description() {
        let mut state = ResilienceState::default();

        state.update(true, 3, 1000);
        assert!(state.status_description().contains("Fully connected"));

        state.update(false, 2, 2000);
        assert!(state.status_description().contains("LAN only"));

        state.update(false, 0, 3000);
        assert!(state.status_description().contains("Single-node"));
    }
}
