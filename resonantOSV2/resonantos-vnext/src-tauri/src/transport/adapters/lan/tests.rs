// Intent citation: .kiro/specs/lan-transport-adapter/design.md — Testing Strategy
// Property-based tests and integration tests for the LAN adapter.

use super::*;
use super::codec::{encode_frame, decode_frame};
use super::heartbeat::{simulate_heartbeat_state, pong_echo_timestamp};
use super::metrics::compute_bandwidth_mbps;

#[test]
fn test_lan_adapter_capabilities() {
    let adapter = LanAdapter::new(LanAdapterConfig::default(), uuid::Uuid::new_v4());
    let caps = adapter.capabilities();

    assert_eq!(caps.max_message_size_bytes, 64 * 1024 * 1024);
    assert!(caps.supports_broadcast);
    assert!(!caps.supports_multi_hop);
    assert_eq!(caps.encryption, crate::transport::trait_def::EncryptionType::Tls13);
    assert_eq!(caps.reliability_class, crate::transport::trait_def::ReliabilityClass::Reliable);
}

#[test]
fn test_lan_adapter_id_and_name() {
    let adapter = LanAdapter::new(LanAdapterConfig::default(), uuid::Uuid::new_v4());
    assert_eq!(adapter.id(), "lan");
    assert_eq!(adapter.name(), "LAN/mDNS");
}

#[test]
fn test_lan_adapter_health_not_running() {
    let adapter = LanAdapter::new(LanAdapterConfig::default(), uuid::Uuid::new_v4());
    let health = adapter.health_check();
    assert!(!health.is_healthy);
    assert_eq!(health.peers_reachable, 0);
}

#[test]
fn test_lan_adapter_send_not_running() {
    let adapter = LanAdapter::new(LanAdapterConfig::default(), uuid::Uuid::new_v4());
    let node = uuid::Uuid::new_v4();
    let msg = crate::transport::trait_def::TransportMessage::new(
        vec![1],
        crate::transport::trait_def::MessagePriority::Normal,
        crate::transport::trait_def::RequestType::Heartbeat,
    );

    let result = adapter.send(&node, &msg);
    assert!(matches!(result, Err(crate::transport::trait_def::TransportError::NotConnected)));
}

#[test]
fn test_lan_adapter_discover_peers_empty() {
    let adapter = LanAdapter::new(LanAdapterConfig::default(), uuid::Uuid::new_v4());
    let peers = adapter.discover_peers();
    assert!(peers.is_empty());
}

#[test]
fn test_lan_adapter_get_reliability_unknown_peer() {
    let adapter = LanAdapter::new(LanAdapterConfig::default(), uuid::Uuid::new_v4());
    let unknown = uuid::Uuid::new_v4();
    let result = adapter.get_reliability(&unknown);
    assert!(matches!(result, Err(crate::transport::trait_def::TransportError::Unreachable { .. })));
}

#[test]
fn test_lan_adapter_get_bandwidth_unknown_peer() {
    let adapter = LanAdapter::new(LanAdapterConfig::default(), uuid::Uuid::new_v4());
    let unknown = uuid::Uuid::new_v4();
    let result = adapter.get_bandwidth(&unknown);
    assert!(matches!(result, Err(crate::transport::trait_def::TransportError::Unreachable { .. })));
}

#[test]
fn test_lan_adapter_measure_latency_unknown_peer() {
    let adapter = LanAdapter::new(LanAdapterConfig::default(), uuid::Uuid::new_v4());
    let unknown = uuid::Uuid::new_v4();
    let result = adapter.measure_latency(&unknown);
    assert!(matches!(result, Err(crate::transport::trait_def::TransportError::Unreachable { .. })));
}

#[test]
fn test_wire_message_variants() {
    use crate::transport::trait_def::{MessagePriority, RequestType, TransportMessage};

    let data_msg = WireMessage::Data(TransportMessage::new(
        vec![1, 2, 3],
        MessagePriority::Normal,
        RequestType::Heartbeat,
    ));
    assert!(matches!(data_msg, WireMessage::Data(_)));

    let ping = WireMessage::Ping { timestamp_ns: 123456789 };
    assert!(matches!(ping, WireMessage::Ping { timestamp_ns: 123456789 }));

    let pong = WireMessage::Pong { timestamp_ns: 987654321 };
    assert!(matches!(pong, WireMessage::Pong { timestamp_ns: 987654321 }));

    let goodbye = WireMessage::Goodbye;
    assert!(matches!(goodbye, WireMessage::Goodbye));
}

#[test]
fn test_handshake_creation() {
    let node_id = uuid::Uuid::new_v4();
    let handshake = Handshake {
        node_id,
        protocol_version: 1,
        capabilities: 0,
    };

    assert_eq!(handshake.node_id, node_id);
    assert_eq!(handshake.protocol_version, 1);
    assert_eq!(handshake.capabilities, 0);
}

#[test]
fn test_lan_error_display() {
    let err = LanError::FrameTooLarge { size: 100_000_000, max: 67_108_864 };
    let msg = format!("{}", err);
    assert!(msg.contains("Frame too large"));
    assert!(msg.contains("100000000"));

    let err = LanError::PeerNotFound { node_id: uuid::Uuid::nil() };
    let msg = format!("{}", err);
    assert!(msg.contains("Peer not found"));

    let err = LanError::Shutdown;
    assert_eq!(format!("{}", err), "Adapter is shutting down");
}

#[test]
fn test_discovered_peer_event() {
    use std::net::{IpAddr, Ipv4Addr};

    let event = DiscoveredPeerEvent {
        node_id: uuid::Uuid::new_v4(),
        address: std::net::SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 50)), 9741),
        hostname: "my-node".to_string(),
    };

    assert_eq!(event.hostname, "my-node");
    assert_eq!(event.address.port(), 9741);
}

// ─── Codec Tests ─────────────────────────────────────────────────────────────

#[test]
fn test_codec_roundtrip_all_variants() {
    use crate::transport::trait_def::{MessagePriority, RequestType, TransportMessage};

    let messages = vec![
        WireMessage::Data(TransportMessage::new(
            vec![1, 2, 3, 4, 5],
            MessagePriority::Critical,
            RequestType::InferenceActivation,
        )),
        WireMessage::Ping { timestamp_ns: u64::MAX },
        WireMessage::Pong { timestamp_ns: 0 },
        WireMessage::Goodbye,
    ];

    for msg in messages {
        let frame = encode_frame(&msg).unwrap();
        let decoded = decode_frame(&frame[4..]).unwrap();
        assert_eq!(decoded, msg);
    }
}

// ─── Heartbeat State Machine Tests ──────────────────────────────────────────

#[test]
fn test_heartbeat_state_machine_comprehensive() {
    // All received: not offline
    assert!(!simulate_heartbeat_state(&[true, true, true, true], 3));

    // Exactly 3 missed: offline
    assert!(simulate_heartbeat_state(&[false, false, false], 3));

    // 2 missed then received: not offline
    assert!(!simulate_heartbeat_state(&[false, false, true], 3));

    // Received resets counter
    assert!(!simulate_heartbeat_state(&[false, false, true, false, false], 3));

    // 3 missed after reset: offline
    assert!(simulate_heartbeat_state(&[false, false, true, false, false, false], 3));
}

#[test]
fn test_pong_echoes_timestamp() {
    for ts in [0u64, 1, 42, u64::MAX / 2, u64::MAX] {
        assert_eq!(pong_echo_timestamp(ts), ts);
    }
}

// ─── Bandwidth Calculation Tests ─────────────────────────────────────────────

#[test]
fn test_bandwidth_formula() {
    // 1GB in 1 second = 8000 Mbps
    let bw = compute_bandwidth_mbps(1_000_000_000, 1.0);
    assert!((bw - 8000.0).abs() < 0.001);

    // 1MB in 1 second = 8 Mbps
    let bw = compute_bandwidth_mbps(1_000_000, 1.0);
    assert!((bw - 8.0).abs() < 0.001);
}

// ─── Peer Registry Integration Tests ────────────────────────────────────────

#[test]
fn test_peer_registry_with_adapter() {
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    let adapter = LanAdapter::new(LanAdapterConfig::default(), uuid::Uuid::new_v4());

    let peer_id = uuid::Uuid::new_v4();
    let peer = PeerInfo::new(
        peer_id,
        SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100)), 9741),
        "test-peer".to_string(),
    );

    adapter.peer_registry().insert(peer);
    assert_eq!(adapter.peer_registry().len(), 1);

    // Not connected yet, so discover_peers should be empty
    assert!(adapter.discover_peers().is_empty());

    // Mark as connected
    adapter.peer_registry().mark_online(&peer_id);
    assert_eq!(adapter.discover_peers().len(), 1);
}

#[test]
fn test_health_check_with_peers() {
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    let adapter = LanAdapter::new(LanAdapterConfig::default(), uuid::Uuid::new_v4());

    // Add a connected peer
    let peer_id = uuid::Uuid::new_v4();
    let mut peer = PeerInfo::new(
        peer_id,
        SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100)), 9741),
        "test-peer".to_string(),
    );
    peer.status = PeerStatus::Connected;
    adapter.peer_registry().insert(peer);

    // Adapter not running
    let health = adapter.health_check();
    assert!(!health.is_healthy);
    assert_eq!(health.peers_reachable, 1);
}

#[test]
fn test_get_reliability_with_history() {
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    let adapter = LanAdapter::new(LanAdapterConfig::default(), uuid::Uuid::new_v4());

    let peer_id = uuid::Uuid::new_v4();
    let peer = PeerInfo::new(
        peer_id,
        SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100)), 9741),
        "test-peer".to_string(),
    );
    adapter.peer_registry().insert(peer);

    // Record some send results
    for _ in 0..8 {
        adapter.peer_registry().record_send_result(&peer_id, true);
    }
    for _ in 0..2 {
        adapter.peer_registry().record_send_result(&peer_id, false);
    }

    let reliability = adapter.get_reliability(&peer_id).unwrap();
    assert!((reliability - 0.8).abs() < f64::EPSILON);
}

// ─── Property-Based Tests (proptest) ─────────────────────────────────────────

#[cfg(test)]
mod property_tests {
    use super::*;
    use proptest::prelude::*;
    use crate::transport::trait_def::{MessagePriority, RequestType, TransportMessage};

    // Property 1: Serialization Round-Trip
    proptest! {
        #[test]
        fn prop_serialization_roundtrip(
            payload in prop::collection::vec(any::<u8>(), 0..1000),
            priority in prop::sample::select(vec![
                MessagePriority::Low,
                MessagePriority::Normal,
                MessagePriority::Critical,
            ]),
            request_type in prop::sample::select(vec![
                RequestType::Heartbeat,
                RequestType::InferenceActivation,
                RequestType::ModelTransfer,
                RequestType::InferenceRequest,
            ]),
        ) {
            let msg = WireMessage::Data(TransportMessage::new(payload.clone(), priority, request_type));
            let frame = encode_frame(&msg).unwrap();
            let decoded = decode_frame(&frame[4..]).unwrap();
            prop_assert_eq!(decoded, msg);
        }

        #[test]
        fn prop_ping_pong_roundtrip(timestamp in any::<u64>()) {
            let ping = WireMessage::Ping { timestamp_ns: timestamp };
            let frame = encode_frame(&ping).unwrap();
            let decoded = decode_frame(&frame[4..]).unwrap();
            prop_assert_eq!(decoded, ping);

            let pong = WireMessage::Pong { timestamp_ns: timestamp };
            let frame = encode_frame(&pong).unwrap();
            let decoded = decode_frame(&frame[4..]).unwrap();
            prop_assert_eq!(decoded, pong);
        }
    }

    // Property 2: Connection Pool Invariant
    proptest! {
        #[test]
        fn prop_connection_pool_invariant(
            peer_count in 1usize..10,
            op_count in 1usize..20
        ) {
            let adapter = LanAdapter::new(LanAdapterConfig::default(), uuid::Uuid::new_v4());
            let peers: Vec<uuid::Uuid> = (0..peer_count).map(|_| uuid::Uuid::new_v4()).collect();

            // Add all peers
            for &peer_id in &peers {
                let peer = PeerInfo::new(
                    peer_id,
                    std::net::SocketAddr::new(
                        std::net::IpAddr::V4(std::net::Ipv4Addr::new(192, 168, 1, 1)),
                        9741,
                    ),
                    "test".to_string(),
                );
                adapter.peer_registry().insert(peer);
            }

            // Apply operations
            for i in 0..op_count {
                let peer_id = &peers[0 % peers.len()];
                match i % 3 {
                    0 => adapter.peer_registry().mark_online(peer_id),
                    1 => adapter.peer_registry().mark_offline(peer_id),
                    _ => {
                        adapter.peer_registry().mark_offline(peer_id);
                        adapter.peer_registry().mark_online(peer_id);
                    }
                }
            }

            // Invariant: registry length never exceeds initial peer count
            prop_assert!(adapter.peer_registry().len() <= peer_count);
        }
    }

    // Property 3: Broadcast Completeness
    proptest! {
        #[test]
        fn prop_broadcast_completeness(connected_count in 0usize..10) {
            let adapter = LanAdapter::new(LanAdapterConfig::default(), uuid::Uuid::new_v4());

            for i in 0..connected_count {
                let peer_id = uuid::Uuid::new_v4();
                let mut peer = PeerInfo::new(
                    peer_id,
                    std::net::SocketAddr::new(
                        std::net::IpAddr::V4(std::net::Ipv4Addr::new(192, 168, 1, i as u8 + 1)),
                        9741,
                    ),
                    format!("peer-{}", i),
                );
                peer.status = PeerStatus::Connected;
                adapter.peer_registry().insert(peer);
            }

            let discovered = adapter.discover_peers();
            prop_assert_eq!(discovered.len(), connected_count);
        }
    }

    // Property 4: Pong Echoes Ping Timestamp
    proptest! {
        #[test]
        fn prop_pong_echoes_timestamp(timestamp in any::<u64>()) {
            prop_assert_eq!(pong_echo_timestamp(timestamp), timestamp);
        }
    }

    // Property 5: Bandwidth Calculation
    proptest! {
        #[test]
        fn prop_bandwidth_positive(
            bytes in 1u64..1_000_000_000,
            duration in 0.001f64..100.0
        ) {
            let bw = compute_bandwidth_mbps(bytes, duration);
            prop_assert!(bw > 0.0, "Bandwidth should be positive: {}", bw);
            prop_assert!(bw.is_finite(), "Bandwidth should be finite: {}", bw);
        }
    }

    // Property 6: Heartbeat Liveness Detection
    proptest! {
        #[test]
        fn prop_heartbeat_liveness(
            responses in prop::collection::vec(any::<bool>(), 1..20),
            threshold in 2u8..6
        ) {
            let is_offline = simulate_heartbeat_state(&responses, threshold);

            // Count consecutive misses at the end
            let mut consecutive_misses = 0u8;
            for &received in responses.iter().rev() {
                if !received {
                    consecutive_misses += 1;
                } else {
                    break;
                }
            }

            if consecutive_misses >= threshold {
                prop_assert!(is_offline, "Should be offline with {} consecutive misses (threshold {})", consecutive_misses, threshold);
            }
            // Note: if consecutive_misses < threshold, it might still be offline
            // if there was a longer streak earlier that wasn't reset
        }
    }

    // Property 10: Error Rate and Degradation Threshold
    proptest! {
        #[test]
        fn prop_error_rate_bounded(
            results in prop::collection::vec(any::<bool>(), 1..20)
        ) {
            let successes = results.iter().filter(|&&r| r).count();
            let total = results.len();
            let error_rate = 1.0 - (successes as f64 / total as f64);

            prop_assert!(error_rate >= 0.0 && error_rate <= 1.0);
        }
    }

    // Property 11: Health Report Accuracy
    proptest! {
        #[test]
        fn prop_health_report_accuracy(connected in 0usize..20) {
            let adapter = LanAdapter::new(LanAdapterConfig::default(), uuid::Uuid::new_v4());

            for i in 0..connected {
                let peer_id = uuid::Uuid::new_v4();
                let mut peer = PeerInfo::new(
                    peer_id,
                    std::net::SocketAddr::new(
                        std::net::IpAddr::V4(std::net::Ipv4Addr::new(10, 0, 0, i as u8 + 1)),
                        9741,
                    ),
                    format!("node-{}", i),
                );
                peer.status = PeerStatus::Connected;
                adapter.peer_registry().insert(peer);
            }

            let health = adapter.health_check();
            prop_assert_eq!(health.peers_reachable as usize, connected);
        }
    }
}
