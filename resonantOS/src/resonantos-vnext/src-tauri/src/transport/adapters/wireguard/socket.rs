// WireGuard UDP socket — send/receive encrypted packets.
//
// In production with boringtun feature enabled, this handles:
// - Binding UdpSocket on listen_port
// - Receive loop: decrypt via boringtun, deserialize, deliver
// - Send: serialize, encrypt via boringtun, send UDP
// - Endpoint roaming detection
//
// Without boringtun, this module provides the framing logic only.

/// Message framing: 4-byte big-endian length prefix + MessagePack payload.
/// Same format as the LAN adapter for consistency.
pub struct MessageFramer;

impl MessageFramer {
    /// Frame a message: prepend 4-byte length header.
    pub fn frame(payload: &[u8]) -> Vec<u8> {
        let len = payload.len() as u32;
        let mut framed = Vec::with_capacity(4 + payload.len());
        framed.extend_from_slice(&len.to_be_bytes());
        framed.extend_from_slice(payload);
        framed
    }

    /// Unframe a message: read 4-byte length, extract payload.
    /// Returns (payload, remaining_bytes).
    pub fn unframe(data: &[u8]) -> Option<(&[u8], &[u8])> {
        if data.len() < 4 {
            return None;
        }
        let len = u32::from_be_bytes([data[0], data[1], data[2], data[3]]) as usize;
        if data.len() < 4 + len {
            return None;
        }
        Some((&data[4..4 + len], &data[4 + len..]))
    }

    /// Check if a buffer contains a complete framed message.
    pub fn is_complete(data: &[u8]) -> bool {
        if data.len() < 4 {
            return false;
        }
        let len = u32::from_be_bytes([data[0], data[1], data[2], data[3]]) as usize;
        data.len() >= 4 + len
    }
}

/// Endpoint information for a peer.
#[derive(Debug, Clone, PartialEq)]
pub struct PeerEndpoint {
    pub address: std::net::SocketAddr,
    pub last_seen_ms: u64,
    pub roamed: bool,
}

impl PeerEndpoint {
    pub fn new(address: std::net::SocketAddr) -> Self {
        Self {
            address,
            last_seen_ms: 0,
            roamed: false,
        }
    }

    /// Update endpoint if source address changed (roaming detection).
    pub fn update_if_roamed(&mut self, new_address: std::net::SocketAddr) -> bool {
        if self.address != new_address {
            self.address = new_address;
            self.roamed = true;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    #[test]
    fn test_frame_unframe_roundtrip() {
        let payload = b"hello world";
        let framed = MessageFramer::frame(payload);
        let (unframed, remaining) = MessageFramer::unframe(&framed).unwrap();
        assert_eq!(unframed, payload);
        assert!(remaining.is_empty());
    }

    #[test]
    fn test_frame_length_prefix() {
        let payload = vec![0u8; 256];
        let framed = MessageFramer::frame(&payload);
        assert_eq!(framed.len(), 4 + 256);
        assert_eq!(&framed[0..4], &[0, 0, 1, 0]); // 256 in big-endian
    }

    #[test]
    fn test_unframe_incomplete_returns_none() {
        let data = vec![0, 0, 0, 10, 1, 2, 3]; // Says 10 bytes but only 3
        assert!(MessageFramer::unframe(&data).is_none());
    }

    #[test]
    fn test_unframe_too_short_returns_none() {
        let data = vec![0, 0]; // Less than 4 bytes
        assert!(MessageFramer::unframe(&data).is_none());
    }

    #[test]
    fn test_is_complete() {
        let payload = b"test";
        let framed = MessageFramer::frame(payload);
        assert!(MessageFramer::is_complete(&framed));
        assert!(!MessageFramer::is_complete(&framed[..5])); // Truncated
    }

    #[test]
    fn test_endpoint_roaming() {
        let addr1 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)), 51820);
        let addr2 = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 5)), 51820);

        let mut ep = PeerEndpoint::new(addr1);
        assert!(!ep.roamed);

        let roamed = ep.update_if_roamed(addr2);
        assert!(roamed);
        assert!(ep.roamed);
        assert_eq!(ep.address, addr2);
    }

    #[test]
    fn test_endpoint_no_roam_same_address() {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)), 51820);
        let mut ep = PeerEndpoint::new(addr);

        let roamed = ep.update_if_roamed(addr);
        assert!(!roamed);
        assert!(!ep.roamed);
    }
}
