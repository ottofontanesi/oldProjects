// WireGuard tunnel registry — manages peer tunnels and state transitions.

use super::config::WireGuardConfig;
use crate::transport::trait_def::NodeId;
use std::collections::HashMap;
use std::net::SocketAddr;

/// Tunnel lifecycle state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TunnelState {
    /// Handshake in progress.
    Handshaking,
    /// Tunnel established, data flowing.
    Established,
    /// No data received recently, may be offline.
    Suspect,
    /// Confirmed offline, tunnel closed.
    Offline,
}

/// Per-tunnel metrics.
#[derive(Debug, Clone)]
pub struct TunnelMetrics {
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub packets_sent: u64,
    pub packets_received: u64,
    pub last_data_ms: u64,
    pub last_handshake_ms: u64,
    pub rtt_ms: f64,
    pub bandwidth_mbps: f64,
    pub error_count: u64,
}

impl Default for TunnelMetrics {
    fn default() -> Self {
        Self {
            bytes_sent: 0,
            bytes_received: 0,
            packets_sent: 0,
            packets_received: 0,
            last_data_ms: 0,
            last_handshake_ms: 0,
            rtt_ms: 0.0,
            bandwidth_mbps: 0.0,
            error_count: 0,
        }
    }
}

/// A registered tunnel entry.
#[derive(Debug, Clone)]
pub struct TunnelEntry {
    pub node_id: NodeId,
    pub public_key: [u8; 32],
    pub endpoint: SocketAddr,
    pub state: TunnelState,
    pub metrics: TunnelMetrics,
    pub created_at_ms: u64,
}

/// Manages all active WireGuard tunnels.
pub struct TunnelRegistry {
    tunnels: HashMap<NodeId, TunnelEntry>,
    max_tunnels: usize,
}

impl TunnelRegistry {
    /// Create a new empty registry.
    pub fn new(max_tunnels: usize) -> Self {
        Self {
            tunnels: HashMap::new(),
            max_tunnels,
        }
    }

    /// Register a new peer and create a tunnel entry.
    pub fn add_peer(
        &mut self,
        node_id: NodeId,
        public_key: [u8; 32],
        endpoint: SocketAddr,
    ) -> Result<(), WgTunnelError> {
        if self.tunnels.len() >= self.max_tunnels {
            return Err(WgTunnelError::TunnelLimitReached {
                max: self.max_tunnels,
            });
        }

        let entry = TunnelEntry {
            node_id,
            public_key,
            endpoint,
            state: TunnelState::Handshaking,
            metrics: TunnelMetrics::default(),
            created_at_ms: now_ms(),
        };

        self.tunnels.insert(node_id, entry);
        Ok(())
    }

    /// Remove a peer and close its tunnel.
    pub fn remove_peer(&mut self, node_id: &NodeId) -> Option<TunnelEntry> {
        self.tunnels.remove(node_id)
    }

    /// Get a tunnel entry by node ID.
    pub fn get_tunnel(&self, node_id: &NodeId) -> Option<&TunnelEntry> {
        self.tunnels.get(node_id)
    }

    /// Get a mutable tunnel entry.
    pub fn get_tunnel_mut(&mut self, node_id: &NodeId) -> Option<&mut TunnelEntry> {
        self.tunnels.get_mut(node_id)
    }

    /// Update tunnel state with valid transition check.
    pub fn update_state(&mut self, node_id: &NodeId, new_state: TunnelState) -> Result<(), WgTunnelError> {
        let entry = self.tunnels.get_mut(node_id).ok_or(WgTunnelError::PeerNotFound)?;

        // Validate state transition
        let valid = match (entry.state, new_state) {
            (TunnelState::Handshaking, TunnelState::Established) => true,
            (TunnelState::Established, TunnelState::Suspect) => true,
            (TunnelState::Suspect, TunnelState::Offline) => true,
            (_, TunnelState::Handshaking) => true, // Re-handshake always allowed
            (TunnelState::Suspect, TunnelState::Established) => true, // Recovery
            _ => false,
        };

        if !valid {
            return Err(WgTunnelError::InvalidTransition {
                from: entry.state,
                to: new_state,
            });
        }

        entry.state = new_state;
        Ok(())
    }

    /// Count of established tunnels.
    pub fn active_count(&self) -> usize {
        self.tunnels
            .values()
            .filter(|t| t.state == TunnelState::Established)
            .count()
    }

    /// Total tunnel count (all states).
    pub fn total_count(&self) -> usize {
        self.tunnels.len()
    }

    /// Get all established tunnels.
    pub fn established_tunnels(&self) -> Vec<&TunnelEntry> {
        self.tunnels
            .values()
            .filter(|t| t.state == TunnelState::Established)
            .collect()
    }

    /// Get all tunnels.
    pub fn all_tunnels(&self) -> Vec<&TunnelEntry> {
        self.tunnels.values().collect()
    }
}

/// Tunnel registry errors.
#[derive(Debug, Clone, PartialEq)]
pub enum WgTunnelError {
    TunnelLimitReached { max: usize },
    PeerNotFound,
    InvalidTransition { from: TunnelState, to: TunnelState },
}

impl std::fmt::Display for WgTunnelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TunnelLimitReached { max } => write!(f, "Tunnel limit reached: max {}", max),
            Self::PeerNotFound => write!(f, "Peer not found in registry"),
            Self::InvalidTransition { from, to } => {
                write!(f, "Invalid tunnel transition: {:?} → {:?}", from, to)
            }
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
    use std::net::{IpAddr, Ipv4Addr};

    fn test_endpoint() -> SocketAddr {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2)), 51820)
    }

    #[test]
    fn test_add_peer() {
        let mut reg = TunnelRegistry::new(20);
        let node = uuid::Uuid::new_v4();
        let result = reg.add_peer(node, [1u8; 32], test_endpoint());
        assert!(result.is_ok());
        assert_eq!(reg.total_count(), 1);
    }

    #[test]
    fn test_tunnel_limit() {
        let mut reg = TunnelRegistry::new(2);
        let n1 = uuid::Uuid::new_v4();
        let n2 = uuid::Uuid::new_v4();
        let n3 = uuid::Uuid::new_v4();

        reg.add_peer(n1, [1u8; 32], test_endpoint()).unwrap();
        reg.add_peer(n2, [2u8; 32], test_endpoint()).unwrap();
        let result = reg.add_peer(n3, [3u8; 32], test_endpoint());
        assert!(matches!(result, Err(WgTunnelError::TunnelLimitReached { .. })));
    }

    #[test]
    fn test_valid_state_transitions() {
        let mut reg = TunnelRegistry::new(20);
        let node = uuid::Uuid::new_v4();
        reg.add_peer(node, [1u8; 32], test_endpoint()).unwrap();

        // Handshaking → Established
        assert!(reg.update_state(&node, TunnelState::Established).is_ok());
        // Established → Suspect
        assert!(reg.update_state(&node, TunnelState::Suspect).is_ok());
        // Suspect → Offline
        assert!(reg.update_state(&node, TunnelState::Offline).is_ok());
    }

    #[test]
    fn test_invalid_state_transition() {
        let mut reg = TunnelRegistry::new(20);
        let node = uuid::Uuid::new_v4();
        reg.add_peer(node, [1u8; 32], test_endpoint()).unwrap();

        // Handshaking → Offline (invalid — must go through Established first)
        let result = reg.update_state(&node, TunnelState::Offline);
        assert!(matches!(result, Err(WgTunnelError::InvalidTransition { .. })));
    }

    #[test]
    fn test_re_handshake_always_allowed() {
        let mut reg = TunnelRegistry::new(20);
        let node = uuid::Uuid::new_v4();
        reg.add_peer(node, [1u8; 32], test_endpoint()).unwrap();

        reg.update_state(&node, TunnelState::Established).unwrap();
        // Re-handshake from Established
        assert!(reg.update_state(&node, TunnelState::Handshaking).is_ok());
    }

    #[test]
    fn test_active_count() {
        let mut reg = TunnelRegistry::new(20);
        let n1 = uuid::Uuid::new_v4();
        let n2 = uuid::Uuid::new_v4();

        reg.add_peer(n1, [1u8; 32], test_endpoint()).unwrap();
        reg.add_peer(n2, [2u8; 32], test_endpoint()).unwrap();

        assert_eq!(reg.active_count(), 0); // Both handshaking
        reg.update_state(&n1, TunnelState::Established).unwrap();
        assert_eq!(reg.active_count(), 1);
    }

    #[test]
    fn test_remove_peer() {
        let mut reg = TunnelRegistry::new(20);
        let node = uuid::Uuid::new_v4();
        reg.add_peer(node, [1u8; 32], test_endpoint()).unwrap();
        assert_eq!(reg.total_count(), 1);

        reg.remove_peer(&node);
        assert_eq!(reg.total_count(), 0);
    }
}

#[cfg(test)]
mod property_tests {
    use super::*;
    use proptest::prelude::*;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    fn arb_endpoint() -> impl Strategy<Value = SocketAddr> {
        (1u8..255, 1u8..255, 1u8..255, 1u8..255, 1024u16..65535)
            .prop_map(|(a, b, c, d, port)| {
                SocketAddr::new(IpAddr::V4(Ipv4Addr::new(a, b, c, d)), port)
            })
    }

    // Property: Valid transitions succeed, invalid transitions fail
    proptest! {
        #[test]
        fn prop_tunnel_state_machine_valid_transitions(endpoint in arb_endpoint()) {
            let mut reg = TunnelRegistry::new(20);
            let node = uuid::Uuid::new_v4();
            reg.add_peer(node, [1u8; 32], endpoint).unwrap();

            // Handshaking → Established (valid)
            prop_assert!(reg.update_state(&node, TunnelState::Established).is_ok());
            // Established → Suspect (valid)
            prop_assert!(reg.update_state(&node, TunnelState::Suspect).is_ok());
            // Suspect → Offline (valid)
            prop_assert!(reg.update_state(&node, TunnelState::Offline).is_ok());
            // Any → Handshaking (re-handshake, always valid)
            prop_assert!(reg.update_state(&node, TunnelState::Handshaking).is_ok());
        }

        #[test]
        fn prop_tunnel_invalid_transition_rejected(endpoint in arb_endpoint()) {
            let mut reg = TunnelRegistry::new(20);
            let node = uuid::Uuid::new_v4();
            reg.add_peer(node, [1u8; 32], endpoint).unwrap();

            // Handshaking → Offline (invalid, must go through Established)
            let result = reg.update_state(&node, TunnelState::Offline);
            prop_assert!(result.is_err());

            // Handshaking → Suspect (invalid)
            let result = reg.update_state(&node, TunnelState::Suspect);
            prop_assert!(result.is_err());
        }
    }
}
