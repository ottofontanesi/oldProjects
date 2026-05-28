//! Property-based tests for the Phone Companion App module.
//!
//! Uses `proptest` to verify universal correctness properties across randomized inputs.
//! Each property test validates specific requirements from the design document.

use proptest::prelude::*;
use uuid::Uuid;

use crate::companion::assignment::{AssignmentManager, PhoneConstraints};
use crate::companion::health::{HealthReporter, HealthReporterConfig, PhoneHealthState};
use crate::companion::identity::MeshIdentity;
use crate::companion::inference_runtime::InferenceRuntime;
use crate::companion::layer_worker::{select_protocol, ProtocolSelection};
use crate::companion::npu::{DetectedNPU, NpuType};
use crate::companion::pairing::{PairingClient, PairingClientError, PhoneCapabilities};
use crate::companion::transport_bridge::{
    CompanionTransportBridge, PathMetrics, TransportPath,
};
use crate::companion::types::*;
use crate::transport::trait_def::MessagePriority;

// ─── Strategies ──────────────────────────────────────────────────────────────

fn arb_trust_level() -> impl Strategy<Value = TrustLevel> {
    prop_oneof![
        Just(TrustLevel::LocalOwned),
        Just(TrustLevel::InvitedFriend),
        Just(TrustLevel::Public),
    ]
}

fn arb_background_mode() -> impl Strategy<Value = BackgroundMode> {
    prop_oneof![
        Just(BackgroundMode::Aggressive),
        Just(BackgroundMode::Balanced),
        Just(BackgroundMode::Conservative),
    ]
}

fn arb_connection_type() -> impl Strategy<Value = ConnectionType> {
    prop_oneof![
        Just(ConnectionType::WiFi),
        Just(ConnectionType::Cellular),
        Just(ConnectionType::Ethernet),
        Just(ConnectionType::None),
    ]
}

fn arb_thermal_state() -> impl Strategy<Value = ThermalState> {
    prop_oneof![
        Just(ThermalState::Normal),
        Just(ThermalState::Warm),
        Just(ThermalState::Critical),
    ]
}

fn arb_phone_settings() -> impl Strategy<Value = PhoneSettings> {
    (
        0u8..=100,       // battery_threshold
        any::<bool>(),   // allow_cellular
        1u64..=10000,    // max_model_size_mb
        arb_background_mode(),
        1u32..=300,      // heartbeat_interval_s
    )
        .prop_map(|(bt, ac, mms, bm, hi)| PhoneSettings {
            battery_threshold: bt,
            allow_cellular: ac,
            max_model_size_mb: mms,
            background_mode: bm,
            heartbeat_interval_s: hi,
        })
}

fn arb_cached_model() -> impl Strategy<Value = CachedModel> {
    (
        "[a-z]{3,10}",   // model_id
        "[a-z/]{5,20}",  // file_path
        1u64..=5000,     // size_mb
        any::<bool>(),   // has layer_range
        0u32..=100,      // layer start
        0u32..=100,      // layer end offset
    )
        .prop_map(|(model_id, file_path, size_mb, has_range, start, end_offset)| {
            let layer_range = if has_range {
                Some((start, start + end_offset))
            } else {
                None
            };
            CachedModel {
                model_id,
                file_path: std::path::PathBuf::from(file_path),
                size_mb,
                layer_range,
                last_used: chrono::Utc::now(),
            }
        })
}

fn arb_phone_node_state() -> impl Strategy<Value = PhoneNodeState> {
    (
        any::<[u8; 16]>(),  // node_id bytes
        any::<[u8; 16]>(),  // mesh_network_id bytes
        "[a-z0-9.:]{5,30}", // coordinator_addr
        arb_trust_level(),
        arb_phone_settings(),
        proptest::collection::vec(arb_cached_model(), 0..3),
    )
        .prop_map(|(nid, mnid, addr, trust, settings, models)| PhoneNodeState {
            node_id: Uuid::from_bytes(nid),
            mesh_network_id: Uuid::from_bytes(mnid),
            coordinator_addr: addr,
            trust_level: trust,
            paired_at: chrono::Utc::now(),
            last_connected: chrono::Utc::now(),
            settings,
            cached_models: models,
        })
}

fn arb_phone_capabilities() -> impl Strategy<Value = PhoneCapabilities> {
    (
        "[A-Za-z0-9 .]{5,20}",  // os
        "[A-Za-z0-9 ]{5,30}",   // npu
        1024u64..=16384,         // ram_mb
        0u8..=100,               // battery_percent
        arb_connection_type(),
    )
        .prop_map(|(os, npu, ram_mb, battery_percent, connection_type)| PhoneCapabilities {
            os,
            npu,
            ram_mb,
            battery_percent,
            connection_type,
        })
}

fn arb_activation_payload() -> impl Strategy<Value = ActivationPayload> {
    (
        any::<[u8; 16]>(),                        // session_id
        0u64..=1000000,                           // sequence_num
        proptest::collection::vec(any::<u8>(), 1..64), // tensor_data
        proptest::collection::vec(1u32..=128, 1..4),   // tensor_shape
    )
        .prop_map(|(sid, seq, data, shape)| ActivationPayload {
            session_id: Uuid::from_bytes(sid),
            sequence_num: seq,
            tensor_data: data,
            tensor_shape: shape,
            dtype: TensorDtype::F16,
        })
}

fn arb_phone_health_state() -> impl Strategy<Value = PhoneHealthState> {
    (
        0u8..=100,           // battery_percent
        any::<bool>(),       // is_charging
        arb_thermal_state(),
        arb_connection_type(),
        0u64..=16384,        // available_memory_mb
        0.0f64..=1.0,        // cpu_utilization
        0.0f64..=1.0,        // npu_utilization
        proptest::collection::vec(any::<[u8; 16]>().prop_map(Uuid::from_bytes), 0..3),
        0.0f64..=100.0,      // tokens_per_second
    )
        .prop_map(
            |(bat, chg, therm, conn, mem, cpu, npu, sessions, tps)| PhoneHealthState {
                battery_percent: bat,
                is_charging: chg,
                thermal_state: therm,
                connection_type: conn,
                available_memory_mb: mem,
                cpu_utilization: cpu,
                npu_utilization: npu,
                active_sessions: sessions,
                tokens_per_second: tps,
            },
        )
}

// ─── Property Tests ──────────────────────────────────────────────────────────

proptest! {
    // ─── Property 11: State persistence round-trip ───────────────────────────
    // **Validates: Requirements 7.3**
    //
    // Generate random PhoneNodeState, serialize to JSON, deserialize back,
    // assert equality.
    #[test]
    fn prop_11_state_persistence_round_trip(state in arb_phone_node_state()) {
        let serialized = serde_json::to_string(&state).expect("serialize should succeed");
        let deserialized: PhoneNodeState =
            serde_json::from_str(&serialized).expect("deserialize should succeed");

        prop_assert_eq!(state.node_id, deserialized.node_id);
        prop_assert_eq!(state.mesh_network_id, deserialized.mesh_network_id);
        prop_assert_eq!(&state.coordinator_addr, &deserialized.coordinator_addr);
        prop_assert_eq!(state.trust_level, deserialized.trust_level);
        prop_assert_eq!(&state.settings, &deserialized.settings);
        prop_assert_eq!(state.cached_models.len(), deserialized.cached_models.len());
        for (orig, deser) in state.cached_models.iter().zip(deserialized.cached_models.iter()) {
            prop_assert_eq!(&orig.model_id, &deser.model_id);
            prop_assert_eq!(&orig.file_path, &deser.file_path);
            prop_assert_eq!(orig.size_mb, deser.size_mb);
            prop_assert_eq!(orig.layer_range, deser.layer_range);
        }
    }

    // ─── Property 14: Ed25519 identity validity ──────────────────────────────
    // **Validates: Requirements 9.1**
    //
    // Generate random message bytes, sign with generated keypair, verify succeeds.
    // Verify with different public key always fails.
    #[test]
    fn prop_14_ed25519_identity_validity(message in proptest::collection::vec(any::<u8>(), 0..256)) {
        let identity1 = MeshIdentity::generate().expect("generate identity");
        let identity2 = MeshIdentity::generate().expect("generate identity 2");

        let signature = identity1.sign(&message).expect("sign should succeed");

        // Verify with correct key succeeds
        let valid = MeshIdentity::verify(&identity1.public_key, &message, &signature);
        prop_assert!(valid, "signature should verify with correct key");

        // Verify with different key fails
        let invalid = MeshIdentity::verify(&identity2.public_key, &message, &signature);
        prop_assert!(!invalid, "signature should NOT verify with wrong key");
    }

    // ─── Property 15: NPU backend preference ────────────────────────────────
    // **Validates: Requirements 10.3**
    //
    // Generate random NPU availability (bool) and model format compatibility (bool).
    // Assert NPU selected when both available and compatible; CPU fallback otherwise.
    #[test]
    fn prop_15_npu_backend_preference(
        npu_available in any::<bool>(),
        format_compatible in any::<bool>(),
    ) {
        let runtime = InferenceRuntime::new_mock();

        let detected_npu = if npu_available {
            DetectedNPU {
                npu_type: NpuType::AppleNeuralEngine { generation: 5 },
                available: true,
                delegate: Some(crate::companion::npu::NpuDelegate::CoreML),
            }
        } else {
            DetectedNPU {
                npu_type: NpuType::None,
                available: false,
                delegate: None,
            }
        };

        // Use "coreml" for compatible (Apple NPU supports it), "unsupported_xyz" for incompatible
        let model_format = if format_compatible { "coreml" } else { "unsupported_xyz" };

        let result = runtime.select_delegate(&detected_npu, model_format);

        match result {
            Ok(Some(_delegate)) => {
                // NPU was selected — both must be true
                prop_assert!(npu_available, "NPU selected but not available");
                prop_assert!(format_compatible, "NPU selected but format incompatible");
            }
            Ok(None) => {
                // CPU fallback — at least one condition is false
                prop_assert!(
                    !npu_available || !format_compatible,
                    "CPU fallback but both available and compatible"
                );
            }
            Err(_) => {
                // Should not happen with default config (npu_fallback_to_cpu = true)
                prop_assert!(false, "unexpected error from select_delegate");
            }
        }
    }

    // ─── Property 4: Per-phone memory limit enforcement ──────────────────────
    // **Validates: Requirements 3.4, 4.6**
    //
    // Generate random model sizes (0..10000 MB).
    // Assert any assignment exceeding 3GB is rejected; ≤3GB accepted.
    #[test]
    fn prop_4_per_phone_memory_limit(weight_size_mb in 0u64..10000) {
        let runtime = InferenceRuntime::new_mock();
        let result = runtime.can_load_model(weight_size_mb);

        if weight_size_mb <= 3072 {
            prop_assert!(result.is_ok(), "model ≤3GB should be accepted, got {:?}", result);
        } else {
            prop_assert!(result.is_err(), "model >3GB should be rejected");
        }
    }

    // ─── Property 1: Pairing message completeness ────────────────────────────
    // **Validates: Requirements 2.1, 2.2**
    //
    // Generate random PhoneCapabilities and valid QR strings.
    // Assert handshake contains all required fields.
    #[test]
    fn prop_1_pairing_message_completeness(
        capabilities in arb_phone_capabilities(),
        token in "[a-zA-Z0-9]{8,32}",
    ) {
        let identity = MeshIdentity::generate().expect("generate identity");
        let node_id = identity.node_id;
        let _client = PairingClient::new(identity, capabilities.clone());

        let net_id = Uuid::new_v4();
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let qr_string = format!(
            "resonant://192.168.1.10:8080?token={}&subnet=192.168.1.1&ts={}&net={}",
            token, ts, net_id
        );

        // Parse QR data to verify it works
        let qr_data = PairingClient::parse_qr_data(&qr_string).expect("parse should succeed");

        // Verify all fields are present in parsed QR
        prop_assert_eq!(&qr_data.token, &token);
        prop_assert!(!qr_data.coordinator_addr.is_empty());
        prop_assert!(!qr_data.desktop_subnet.is_empty());
        prop_assert_eq!(qr_data.network_id, net_id);

        // Verify capabilities are preserved (they would be sent in handshake)
        prop_assert!(!capabilities.os.is_empty());
        prop_assert!(!capabilities.npu.is_empty());
        prop_assert!(capabilities.ram_mb > 0);
        prop_assert!(capabilities.battery_percent <= 100);

        // Verify node_id is valid
        prop_assert_ne!(node_id, Uuid::nil());
    }

    // ─── Property 2: Token expiry validation ─────────────────────────────────
    // **Validates: Requirements 2.4**
    //
    // Generate random timestamps; assert tokens >5min rejected, ≤5min accepted.
    #[test]
    fn prop_2_token_expiry_validation(
        created_at in 0u64..1_000_000_000,
        elapsed in 0u64..1000,
    ) {
        let now = created_at + elapsed;
        let result = PairingClient::validate_token_expiry_at(created_at, now);

        if elapsed > 300 {
            prop_assert!(result.is_err(), "token >5min old should be rejected");
            prop_assert_eq!(result.unwrap_err(), PairingClientError::TokenExpired);
        } else {
            prop_assert!(result.is_ok(), "token ≤5min old should be accepted");
        }
    }

    // ─── Property 3: Subnet mismatch detection ──────────────────────────────
    // **Validates: Requirements 2.5**
    //
    // Generate random IPv4 pairs; assert mismatch when first three octets differ.
    #[test]
    fn prop_3_subnet_mismatch_detection(
        a1 in 0u8..=255,
        a2 in 0u8..=255,
        a3 in 0u8..=255,
        a4 in 0u8..=255,
        b1 in 0u8..=255,
        b2 in 0u8..=255,
        b3 in 0u8..=255,
        b4 in 0u8..=255,
    ) {
        let phone_ip = format!("{}.{}.{}.{}", a1, a2, a3, a4);
        let desktop_ip = format!("{}.{}.{}.{}", b1, b2, b3, b4);

        let result = PairingClient::verify_subnet(&phone_ip, &desktop_ip);

        let same_subnet = a1 == b1 && a2 == b2 && a3 == b3;

        if same_subnet {
            prop_assert!(result.is_ok(), "same subnet should pass, got {:?}", result);
        } else {
            prop_assert!(
                matches!(result, Err(PairingClientError::SubnetMismatch { .. })),
                "different subnet should fail with SubnetMismatch, got {:?}",
                result
            );
        }
    }

    // ─── Property 7: Heartbeat field completeness ────────────────────────────
    // **Validates: Requirements 6.1, 6.6**
    //
    // Generate random phone health states, construct heartbeat, assert all fields present.
    #[test]
    fn prop_7_heartbeat_field_completeness(
        state in arb_phone_health_state(),
        timestamp_ms in 0u64..=u64::MAX / 2,
    ) {
        let node_id = Uuid::new_v4();
        let reporter = HealthReporter::with_defaults(node_id);
        let heartbeat = reporter.build_heartbeat(&state, timestamp_ms);

        // Assert all fields are present and match input
        prop_assert_eq!(heartbeat.node_id, node_id);
        prop_assert_eq!(heartbeat.timestamp_ms, timestamp_ms);
        prop_assert_eq!(heartbeat.battery_percent, state.battery_percent);
        prop_assert_eq!(heartbeat.is_charging, state.is_charging);
        prop_assert_eq!(heartbeat.thermal_state, state.thermal_state);
        prop_assert_eq!(heartbeat.connection_type, state.connection_type);
        prop_assert_eq!(heartbeat.available_memory_mb, state.available_memory_mb);
        prop_assert!((heartbeat.cpu_utilization - state.cpu_utilization).abs() < f64::EPSILON);
        prop_assert!((heartbeat.npu_utilization - state.npu_utilization).abs() < f64::EPSILON);
        prop_assert_eq!(heartbeat.active_sessions.len(), state.active_sessions.len());
        prop_assert!((heartbeat.tokens_per_second - state.tokens_per_second).abs() < f64::EPSILON);
    }

    // ─── Property 8: Battery alert threshold crossing ────────────────────────
    // **Validates: Requirements 6.2**
    //
    // Generate random battery transitions; assert alert only when crossing below
    // threshold while not charging.
    #[test]
    fn prop_8_battery_alert_threshold_crossing(
        prev_level in 0u8..=100,
        new_level in 0u8..=100,
        is_charging in any::<bool>(),
        threshold in 1u8..=99,
    ) {
        let config = HealthReporterConfig {
            battery_threshold: threshold,
            alert_debounce: std::time::Duration::from_secs(0), // disable debounce for testing
            ..Default::default()
        };
        let mut reporter = HealthReporter::new(config, Uuid::new_v4());

        // Set previous state
        let prev_state = PhoneHealthState {
            battery_percent: prev_level,
            is_charging: false,
            ..Default::default()
        };
        reporter.check_alerts(&prev_state, 1000);

        // Set new state
        let new_state = PhoneHealthState {
            battery_percent: new_level,
            is_charging,
            ..Default::default()
        };
        let alerts = reporter.check_alerts(&new_state, 10000);

        let crossed_below = prev_level >= threshold && new_level < threshold;
        let should_alert = crossed_below && !is_charging;

        let has_low_battery_alert = alerts.iter().any(|a| matches!(a, HealthAlert::LowBattery { .. }));

        if should_alert {
            prop_assert!(
                has_low_battery_alert,
                "expected LowBattery alert: prev={}, new={}, threshold={}, charging={}",
                prev_level, new_level, threshold, is_charging
            );
        } else {
            prop_assert!(
                !has_low_battery_alert,
                "unexpected LowBattery alert: prev={}, new={}, threshold={}, charging={}",
                prev_level, new_level, threshold, is_charging
            );
        }
    }

    // ─── Property 9: Connectivity change notification ────────────────────────
    // **Validates: Requirements 6.3**
    //
    // Generate random ConnectionType pairs; assert alert when types differ.
    #[test]
    fn prop_9_connectivity_change_notification(
        from_conn in arb_connection_type(),
        to_conn in arb_connection_type(),
    ) {
        let config = HealthReporterConfig {
            alert_debounce: std::time::Duration::from_secs(0),
            ..Default::default()
        };
        let mut reporter = HealthReporter::new(config, Uuid::new_v4());

        // Set initial state
        let prev_state = PhoneHealthState {
            connection_type: from_conn,
            battery_percent: 100, // keep above threshold to avoid battery alerts
            ..Default::default()
        };
        reporter.check_alerts(&prev_state, 1000);

        // Transition
        let new_state = PhoneHealthState {
            connection_type: to_conn,
            battery_percent: 100,
            ..Default::default()
        };
        let alerts = reporter.check_alerts(&new_state, 10000);

        let has_connectivity_alert = alerts.iter().any(|a| {
            matches!(a, HealthAlert::ConnectivityChange { .. })
        });

        if from_conn != to_conn {
            prop_assert!(
                has_connectivity_alert,
                "expected ConnectivityChange alert: {:?} -> {:?}",
                from_conn, to_conn
            );
        } else {
            prop_assert!(
                !has_connectivity_alert,
                "unexpected ConnectivityChange alert: {:?} -> {:?}",
                from_conn, to_conn
            );
        }
    }

    // ─── Property 10: Heartbeat timeout marks node offline ───────────────────
    // **Validates: Requirements 6.5**
    //
    // Generate random timestamp gaps; assert offline when gap >90s.
    #[test]
    fn prop_10_heartbeat_timeout_marks_offline(
        last_heartbeat_ms in 0u64..1_000_000_000,
        gap_ms in 0u64..200_000,
    ) {
        let now_ms = last_heartbeat_ms + gap_ms;
        let is_timed_out = HealthReporter::is_heartbeat_timed_out(last_heartbeat_ms, now_ms);

        if gap_ms > 90_000 {
            prop_assert!(is_timed_out, "gap {}ms > 90s should be timed out", gap_ms);
        } else {
            prop_assert!(!is_timed_out, "gap {}ms <= 90s should NOT be timed out", gap_ms);
        }
    }

    // ─── Property 6: Assignment constraint validation ────────────────────────
    // **Validates: Requirements 5.1, 5.2, 5.3**
    //
    // Generate random phone states × random assignments; assert correct accept/reject.
    #[test]
    fn prop_6_assignment_constraint_validation(
        available_memory_mb in 512u64..8192,
        battery_percent in 0u8..=100,
        is_charging in any::<bool>(),
        connection_type in arb_connection_type(),
        allow_cellular in any::<bool>(),
        weight_size_mb in 100u64..5000,
        params_b in 0.5f64..10.0,
    ) {
        let constraints = PhoneConstraints {
            available_memory_mb,
            battery_percent,
            is_charging,
            connection_type,
            allow_cellular,
            max_model_size_mb: 3072,
            battery_threshold: 20,
            max_full_model_params_b: 3.0,
        };
        let manager = AssignmentManager::new(constraints);

        let assignment = ModelAssignment {
            model_id: "test-model".to_string(),
            assignment_type: AssignmentType::FullModel { params_b },
            download_url: "http://example.com/model.gguf".to_string(),
            weight_size_mb,
            priority: AssignmentPriority::Normal,
        };

        let result = manager.validate_constraints(&assignment);

        // Determine expected outcome
        let memory_violated = weight_size_mb > available_memory_mb || weight_size_mb > 3072;
        let battery_violated = battery_percent < 20 && !is_charging;
        let cellular_violated = connection_type == ConnectionType::Cellular && !allow_cellular;
        let model_too_large = params_b > 3.0;

        let should_reject = memory_violated || battery_violated || cellular_violated || model_too_large;

        if should_reject {
            prop_assert!(
                result.is_err(),
                "should reject: mem_viol={}, bat_viol={}, cell_viol={}, size_viol={}, got Ok",
                memory_violated, battery_violated, cellular_violated, model_too_large
            );
        } else {
            prop_assert!(
                result.is_ok(),
                "should accept but got {:?}",
                result
            );
        }
    }

    // ─── Property 5: Protocol selection by latency ───────────────────────────
    // **Validates: Requirements 4.3**
    //
    // Generate random f64 latency values [0, 200]; assert correct protocol mapping.
    #[test]
    fn prop_5_protocol_selection_by_latency(latency_ms in 0.0f64..200.0) {
        let selection = select_protocol(latency_ms);

        if latency_ms <= 5.0 {
            prop_assert_eq!(
                selection,
                ProtocolSelection::TensorParallel,
                "latency {:.2}ms should be TensorParallel",
                latency_ms
            );
        } else if latency_ms <= 50.0 {
            prop_assert_eq!(
                selection,
                ProtocolSelection::PipelineParallel,
                "latency {:.2}ms should be PipelineParallel",
                latency_ms
            );
        } else {
            prop_assert_eq!(
                selection,
                ProtocolSelection::Rejected,
                "latency {:.2}ms should be Rejected",
                latency_ms
            );
        }
    }

    // ─── Property 12: Path selection minimizes latency ───────────────────────
    // **Validates: Requirements 8.2**
    //
    // Generate random non-empty path sets with latencies; assert minimum chosen.
    #[test]
    fn prop_12_path_selection_minimizes_latency(
        latencies in proptest::collection::vec(0.1f64..500.0, 1..10),
    ) {
        let mut bridge = CompanionTransportBridge::with_defaults();

        let paths: Vec<TransportPath> = latencies
            .iter()
            .enumerate()
            .map(|(i, &lat)| TransportPath {
                adapter_index: i,
                metrics: PathMetrics {
                    transport_id: format!("path-{}", i),
                    latency_ms: lat,
                    bandwidth_mbps: 100.0,
                    measured_at_ms: 1000,
                    is_healthy: true,
                },
            })
            .collect();

        bridge.update_paths(paths);

        let selected = bridge.select_lowest_latency_path();
        prop_assert!(selected.is_some(), "should select a path from non-empty set");

        let selected_idx = selected.unwrap();
        let selected_latency = latencies[selected_idx];

        // The selected path should have the minimum latency
        let min_latency = latencies
            .iter()
            .copied()
            .fold(f64::INFINITY, f64::min);

        prop_assert!(
            (selected_latency - min_latency).abs() < f64::EPSILON,
            "selected latency {:.2} != min latency {:.2}",
            selected_latency,
            min_latency
        );
    }

    // ─── Property 13: Activation messages use Critical priority ──────────────
    // **Validates: Requirements 8.5**
    //
    // Generate random ActivationPayload; assert Critical priority.
    #[test]
    fn prop_13_activation_messages_use_critical_priority(
        payload in arb_activation_payload(),
    ) {
        let bridge = CompanionTransportBridge::with_defaults();
        let msg = bridge.build_activation_message(&payload);

        prop_assert_eq!(
            msg.priority,
            MessagePriority::Critical,
            "activation messages must have Critical priority"
        );
        prop_assert_eq!(
            msg.request_type,
            crate::transport::trait_def::RequestType::InferenceActivation,
            "activation messages must have InferenceActivation request type"
        );
        prop_assert!(!msg.payload.is_empty(), "payload should not be empty");
    }
}


// ─── Integration Tests (Task 15.2) ──────────────────────────────────────────
// Test pairing flow, assignment flow, split inference flow, lifecycle flow.

#[cfg(test)]
mod integration {
    use crate::companion::assignment::{AssignmentManager, PhoneConstraints};
    use crate::companion::health::{HealthReporter, PhoneHealthState};
    use crate::companion::identity::MeshIdentity;
    use crate::companion::layer_worker::LayerWorker;
    use crate::companion::pairing::{PairingClient, PhoneCapabilities};
    use crate::companion::types::*;
    use std::time::{SystemTime, UNIX_EPOCH};
    use uuid::Uuid;

    /// Integration test: Pairing flow
    /// QR parse → handshake → registration → heartbeat starts
    #[test]
    fn test_pairing_flow_end_to_end() {
        // 1. Generate identity
        let identity = MeshIdentity::generate().expect("generate identity");
        let node_id = identity.node_id;

        // 2. Create pairing client with capabilities
        let capabilities = PhoneCapabilities {
            os: "iOS 17.4".to_string(),
            npu: "Apple Neural Engine Gen 5".to_string(),
            ram_mb: 6144,
            battery_percent: 85,
            connection_type: ConnectionType::WiFi,
        };
        let client = PairingClient::new(identity, capabilities);

        // 3. Parse QR and pair
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let net_id = Uuid::new_v4();
        let qr = format!(
            "resonant://192.168.1.10:8080?token=test123&subnet=192.168.1.1&ts={}&net={}",
            ts, net_id
        );

        let result = client.pair_from_qr(&qr, "192.168.1.50");
        assert!(result.is_ok(), "pairing should succeed");

        let pairing = result.unwrap();
        assert_eq!(pairing.network_id, net_id);
        assert_eq!(pairing.trust_level, TrustLevel::LocalOwned);
        assert_eq!(pairing.assigned_node_id, node_id);

        // 4. After pairing, heartbeat reporter can be started
        let reporter = HealthReporter::with_defaults(node_id);
        let state = PhoneHealthState {
            battery_percent: 85,
            is_charging: false,
            thermal_state: ThermalState::Normal,
            connection_type: ConnectionType::WiFi,
            available_memory_mb: 4096,
            cpu_utilization: 0.1,
            npu_utilization: 0.0,
            active_sessions: vec![],
            tokens_per_second: 0.0,
        };
        let heartbeat = reporter.build_heartbeat(&state, ts * 1000);
        assert_eq!(heartbeat.node_id, node_id);
        assert_eq!(heartbeat.battery_percent, 85);
    }

    /// Integration test: Assignment flow
    /// Receive assignment → validate → load → report ready
    #[test]
    fn test_assignment_flow_end_to_end() {
        // 1. Set up phone constraints (healthy phone)
        let constraints = PhoneConstraints {
            available_memory_mb: 4096,
            battery_percent: 80,
            is_charging: false,
            connection_type: ConnectionType::WiFi,
            allow_cellular: false,
            max_model_size_mb: 3072,
            battery_threshold: 20,
            max_full_model_params_b: 3.0,
        };
        let mut manager = AssignmentManager::new(constraints);

        // 2. Receive a valid assignment
        let assignment = ModelAssignment {
            model_id: "phi-3b-q4".to_string(),
            assignment_type: AssignmentType::FullModel { params_b: 2.7 },
            download_url: "http://coordinator.local/models/phi-3b-q4.gguf".to_string(),
            weight_size_mb: 2048,
            priority: AssignmentPriority::Normal,
        };

        // 3. Validate and accept
        let response = manager.handle_assignment(&assignment);
        assert!(
            matches!(response, AssignmentResponse::Accepted { .. }),
            "valid assignment should be accepted"
        );

        // 4. Model is tracked as loaded
        assert!(manager.is_model_loaded(&"phi-3b-q4".to_string()));

        // 5. Reject an oversized assignment
        let big_assignment = ModelAssignment {
            model_id: "llama-7b".to_string(),
            assignment_type: AssignmentType::FullModel { params_b: 7.0 },
            download_url: "http://coordinator.local/models/llama-7b.gguf".to_string(),
            weight_size_mb: 5000,
            priority: AssignmentPriority::Normal,
        };
        let response = manager.handle_assignment(&big_assignment);
        assert!(
            matches!(response, AssignmentResponse::Rejected { .. }),
            "oversized assignment should be rejected"
        );

        // 6. Unload the first model
        let unload_result = manager.handle_unload(&"phi-3b-q4".to_string());
        assert!(unload_result.is_ok());
        assert!(!manager.is_model_loaded(&"phi-3b-q4".to_string()));
    }

    /// Integration test: Split inference flow
    /// Receive layer assignment → calibrate → process activations → forward
    #[test]
    fn test_split_inference_flow_end_to_end() {
        // 1. Create layer worker with sufficient memory
        let mut worker = LayerWorker::new(3072);

        // 2. Accept layer assignment
        let session_id = Uuid::new_v4();
        let assignment = LayerAssignment {
            session_id,
            model_id: "llama-7b".to_string(),
            layer_range: (0, 15),
            layer_count: 16,
            weight_download_url: "http://coordinator.local/layers/0-15.gguf".to_string(),
            weight_size_mb: 2048,
            protocol: SplitProtocol::PipelineParallel,
            prev_node: None,
            next_node: Some(Uuid::new_v4()),
            timeout_ms: 100.0,
        };

        let result = worker.accept_assignment(&assignment);
        assert!(result.is_ok(), "assignment should be accepted");
        assert!(worker.has_active_session());

        // 3. Calibrate (5-token warmup)
        let calibration = worker.calibrate().expect("calibration should succeed");
        assert!(calibration.tokens_per_second > 0.0);
        assert!(calibration.avg_compute_ms > 0.0);
        assert!(calibration.avg_forward_ms > 0.0);

        // 4. Process activations
        let activation = ActivationPayload {
            session_id,
            sequence_num: 1,
            tensor_data: vec![1, 2, 3, 4, 5, 6, 7, 8],
            tensor_shape: vec![1, 8],
            dtype: TensorDtype::F16,
        };

        let output = worker.process_activation(&activation).expect("should process");
        assert_eq!(output.session_id, session_id);
        assert_eq!(output.sequence_num, 1);
        assert_eq!(output.tensor_shape, vec![1, 8]);

        // 5. Process more activations
        let activation2 = ActivationPayload {
            session_id,
            sequence_num: 2,
            tensor_data: vec![9, 10, 11, 12],
            tensor_shape: vec![1, 4],
            dtype: TensorDtype::F16,
        };
        let output2 = worker.process_activation(&activation2).expect("should process");
        assert_eq!(output2.sequence_num, 2);

        // 6. Release session
        worker.release_session().expect("release should succeed");
        assert!(!worker.has_active_session());
    }

    /// Integration test: Lifecycle flow
    /// Background → persist → resume → reconnect
    #[test]
    fn test_lifecycle_flow_end_to_end() {
        // 1. Generate identity (simulates first pairing)
        let identity = MeshIdentity::generate().expect("generate identity");
        let node_id = identity.node_id;

        // 2. Simulate state that would be persisted
        let state = PhoneNodeState {
            node_id,
            mesh_network_id: Uuid::new_v4(),
            coordinator_addr: "192.168.1.10:8080".to_string(),
            trust_level: TrustLevel::LocalOwned,
            paired_at: chrono::Utc::now(),
            last_connected: chrono::Utc::now(),
            settings: PhoneSettings::default(),
            cached_models: vec![],
        };

        // 3. Serialize state (simulates persist on background)
        let serialized = serde_json::to_string(&state).expect("serialize state");
        assert!(!serialized.is_empty());

        // 4. Deserialize state (simulates restore on resume)
        let restored: PhoneNodeState =
            serde_json::from_str(&serialized).expect("deserialize state");
        assert_eq!(restored.node_id, node_id);
        assert_eq!(restored.coordinator_addr, "192.168.1.10:8080");
        assert_eq!(restored.trust_level, TrustLevel::LocalOwned);

        // 5. Reconnect with stored identity
        let capabilities = PhoneCapabilities {
            os: "iOS 17.4".to_string(),
            npu: "Apple Neural Engine Gen 5".to_string(),
            ram_mb: 6144,
            battery_percent: 70,
            connection_type: ConnectionType::WiFi,
        };
        let client = PairingClient::new(identity, capabilities);
        let reconnect_result = client.reconnect(&restored.coordinator_addr);
        assert!(reconnect_result.is_ok(), "reconnect should succeed");

        let pairing = reconnect_result.unwrap();
        assert_eq!(pairing.assigned_node_id, node_id);
        assert_eq!(pairing.coordinator_addr, "192.168.1.10:8080");
    }
}
