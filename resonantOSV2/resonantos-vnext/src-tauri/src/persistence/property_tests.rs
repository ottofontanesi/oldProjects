// Intent citation: .kiro/specs/node-persistence-layer/tasks.md Tasks 3.5, 4.4, 5.4, 6.2, 7.3, 8.4
// Property-based tests for the persistence layer

use proptest::prelude::*;
use std::collections::HashMap;

use crate::network::registry::*;
use crate::persistence::error::PersistenceError;
use crate::persistence::manager::PersistenceManager;
use crate::persistence::models::*;
use crate::persistence::placement_store::PlacementPlan;

// ─── Strategies ──────────────────────────────────────────────────────────────

fn arb_device_type() -> impl Strategy<Value = DeviceType> {
    prop_oneof![
        Just(DeviceType::Desktop),
        Just(DeviceType::Laptop),
        Just(DeviceType::Server),
        Just(DeviceType::Phone),
    ]
}

fn arb_phone_os() -> impl Strategy<Value = PhoneOs> {
    prop_oneof![Just(PhoneOs::Ios), Just(PhoneOs::Android),]
}

fn arb_npu_type() -> impl Strategy<Value = NpuType> {
    prop_oneof![
        (1u8..=5).prop_map(|gen| NpuType::AppleNeuralEngine { generation: gen }),
        "[a-z0-9]{2,6}".prop_map(|v| NpuType::QualcommHexagon { version: v }),
        "[a-z0-9]{2,6}".prop_map(|v| NpuType::MediaTekApu { version: v }),
    ]
}

fn arb_connection_type() -> impl Strategy<Value = ConnectionType> {
    prop_oneof![
        Just(ConnectionType::Wifi),
        Just(ConnectionType::Cellular),
        Just(ConnectionType::Ethernet),
    ]
}

fn arb_phone_info() -> impl Strategy<Value = Option<PhoneInfo>> {
    prop_oneof![
        3 => Just(None),
        1 => (arb_phone_os(), proptest::option::of(arb_npu_type()), 0u8..=100, any::<bool>(), arb_connection_type())
            .prop_map(|(os, npu, battery, charging, conn)| Some(PhoneInfo {
                os,
                npu,
                battery_percent: battery,
                is_charging: charging,
                connection_type: conn,
            })),
    ]
}

fn arb_gpu_backend() -> impl Strategy<Value = GpuBackend> {
    prop_oneof![
        Just(GpuBackend::Cuda),
        Just(GpuBackend::Rocm),
        Just(GpuBackend::Metal),
        Just(GpuBackend::Vulkan),
    ]
}

fn arb_gpu_profile() -> impl Strategy<Value = Option<GpuProfile>> {
    prop_oneof![
        2 => Just(None),
        1 => ("[a-zA-Z0-9 ]{3,20}", 1024u64..=49152, 512u64..=49152, 3.0f32..=9.0, arb_gpu_backend())
            .prop_map(|(model, vram, avail, cc, backend)| Some(GpuProfile {
                model,
                vram_mb: vram,
                vram_available_mb: avail.min(vram),
                compute_capability: cc,
                backend,
            })),
    ]
}

fn arb_storage_type() -> impl Strategy<Value = StorageType> {
    prop_oneof![
        Just(StorageType::Nvme),
        Just(StorageType::Ssd),
        Just(StorageType::Hdd),
    ]
}

fn arb_interface_type() -> impl Strategy<Value = InterfaceType> {
    prop_oneof![
        Just(InterfaceType::Ethernet),
        Just(InterfaceType::Wifi),
        Just(InterfaceType::Cellular),
    ]
}

fn arb_node_state() -> impl Strategy<Value = NodeState> {
    (
        // Group 1: identity
        any::<u128>(), // for UUID
        "[a-zA-Z0-9\\-]{1,30}", // hostname
        arb_device_type(),
        1u32..=128, // cores
        "[a-z0-9_]{3,10}", // architecture
        800u32..=6000, // clock_mhz
        1024u64..=1048576, // total_ram_mb
        arb_gpu_profile(),
        arb_storage_type(),
        100u64..=10000000, // storage available
    )
        .prop_flat_map(|(uuid_bits, hostname, device_type, cores, arch, clock, ram_total, gpu, st, storage_avail)| {
            // Group 2: network + state
            (
                Just((uuid_bits, hostname, device_type, cores, arch, clock, ram_total, gpu, st, storage_avail)),
                arb_interface_type(),
                100u32..=100000, // bandwidth
                arb_phone_info(),
                1u64..=999_999_999_999u64, // last_heartbeat_ms
                any::<bool>(), // is_online
            )
        })
        .prop_map(
            |((uuid_bits, hostname, device_type, cores, arch, clock, ram_total, gpu, st, storage_avail), iface_type, bw, phone_info, heartbeat, online)| {
                let node_id = uuid::Uuid::from_u128(uuid_bits);
                NodeState {
                    capabilities: NodeCapabilities {
                        node_id,
                        hostname,
                        device_type,
                        cpu: CpuProfile {
                            cores,
                            architecture: arch,
                            clock_mhz: clock,
                            isa_extensions: vec![],
                        },
                        ram: RamProfile {
                            total_mb: ram_total,
                            available_mb: ram_total / 2,
                            ddr_generation: 4,
                        },
                        gpu,
                        storage: StorageProfile {
                            storage_type: st,
                            available_mb: storage_avail,
                            read_speed_mbps: 1000,
                        },
                        network_interfaces: vec![NetworkInterface {
                            name: "iface0".to_string(),
                            interface_type: iface_type,
                            bandwidth_mbps: bw,
                        }],
                        phone_info,
                        available_tools: vec![],
                    },
                    utilization: NodeUtilization {
                        node_id,
                        ..Default::default()
                    },
                    loaded_models: Vec::new(),
                    stability_score: 0.95,
                    last_heartbeat_ms: heartbeat,
                    is_online: online,
                    latency_to_peers: HashMap::new(),
                    thermal_state: ThermalState::Normal,
                }
            },
        )
}

fn arb_workflow_status() -> impl Strategy<Value = WorkflowPersistenceStatus> {
    prop_oneof![
        Just(WorkflowPersistenceStatus::Pending),
        Just(WorkflowPersistenceStatus::Running),
        Just(WorkflowPersistenceStatus::Completed),
        Just(WorkflowPersistenceStatus::Failed),
    ]
}

fn arb_json_value() -> impl Strategy<Value = serde_json::Value> {
    prop_oneof![
        any::<bool>().prop_map(serde_json::Value::Bool),
        any::<i64>().prop_map(|n| serde_json::Value::Number(n.into())),
        "[a-zA-Z0-9 ]{0,50}".prop_map(|s| serde_json::Value::String(s)),
        Just(serde_json::Value::Null),
        proptest::collection::vec("[a-zA-Z0-9]{1,10}".prop_map(|s| serde_json::Value::String(s)), 0..5)
            .prop_map(serde_json::Value::Array),
    ]
}

// ─── Property 1: Node State Serialization Round-Trip ─────────────────────────
// **Validates: Requirements 2.2, 2.3, 9.1, 9.4**

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn prop_node_state_round_trip(node in arb_node_state()) {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        rt.block_on(async {
            let pm = PersistenceManager::initialize_in_memory().unwrap();
            let original_id = node.capabilities.node_id;
            let original_hostname = node.capabilities.hostname.clone();
            let original_device_type = node.capabilities.device_type.clone();
            let original_online = node.is_online;
            let original_heartbeat = node.last_heartbeat_ms;
            let original_phone_info = node.capabilities.phone_info.clone();

            pm.upsert_node(&node).await.unwrap();
            let loaded = pm.load_all_nodes().await.unwrap();

            prop_assert_eq!(loaded.len(), 1);
            prop_assert_eq!(loaded[0].capabilities.node_id, original_id);
            prop_assert_eq!(&loaded[0].capabilities.hostname, &original_hostname);
            prop_assert_eq!(&loaded[0].capabilities.device_type, &original_device_type);
            prop_assert_eq!(loaded[0].is_online, original_online);
            prop_assert_eq!(loaded[0].last_heartbeat_ms, original_heartbeat);

            // Verify phone info round-trips
            match (&loaded[0].capabilities.phone_info, &original_phone_info) {
                (Some(loaded_pi), Some(orig_pi)) => {
                    prop_assert_eq!(&loaded_pi.os, &orig_pi.os);
                    prop_assert_eq!(loaded_pi.battery_percent, orig_pi.battery_percent);
                    prop_assert_eq!(loaded_pi.is_charging, orig_pi.is_charging);
                }
                (None, None) => {}
                _ => prop_assert!(false, "Phone info mismatch"),
            }

            Ok(())
        })?;
    }
}

// ─── Property 12: Stale Node Cleanup ─────────────────────────────────────────
// **Validates: Requirements 11.3**

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn prop_stale_node_cleanup(
        nodes in proptest::collection::vec(arb_node_state(), 1..20),
        now_ms in 1_000_000_000u64..999_999_999_999u64,
    ) {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        rt.block_on(async {
            let pm = PersistenceManager::initialize_in_memory().unwrap();
            let max_age_days = 30u32;
            let cutoff_ms = now_ms.saturating_sub(max_age_days as u64 * 86400 * 1000);

            // Insert all nodes (deduplicate by node_id)
            let mut inserted: HashMap<uuid::Uuid, u64> = HashMap::new();
            for node in &nodes {
                pm.upsert_node(node).await.unwrap();
                inserted.insert(node.capabilities.node_id, node.last_heartbeat_ms);
            }

            let expected_remaining: usize = inserted.values()
                .filter(|&&heartbeat| heartbeat >= cutoff_ms)
                .count();

            pm.cleanup_stale_nodes(max_age_days, now_ms).await.unwrap();

            let remaining = pm.load_all_nodes().await.unwrap();
            prop_assert_eq!(remaining.len(), expected_remaining);

            // All remaining nodes should have last_heartbeat_ms >= cutoff
            for node in &remaining {
                prop_assert!(node.last_heartbeat_ms >= cutoff_ms,
                    "Node {} has heartbeat {} but cutoff is {}",
                    node.capabilities.node_id, node.last_heartbeat_ms, cutoff_ms);
            }

            Ok(())
        })?;
    }
}

// ─── Property 2: Unexpired Checkpoint Load Filtering ─────────────────────────
// **Validates: Requirements 3.3**

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn prop_unexpired_checkpoint_filtering(
        expires_times in proptest::collection::vec(1000u64..100_000u64, 1..20),
        now_ms in 1000u64..100_000u64,
    ) {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        rt.block_on(async {
            let pm = PersistenceManager::initialize_in_memory().unwrap();

            for (i, &expires) in expires_times.iter().enumerate() {
                let cp = PersistedCheckpoint {
                    checkpoint_id: format!("cp-{}", i),
                    workflow_id: "wf-1".to_string(),
                    step_index: i as u32,
                    state_json: r#"{"data": "test"}"#.to_string(),
                    created_at_ms: 500,
                    expires_at_ms: expires,
                };
                pm.save_checkpoint(&cp).await.unwrap();
            }

            let loaded = pm.load_unexpired_checkpoints(now_ms).await.unwrap();
            let expected_count = expires_times.iter().filter(|&&e| e >= now_ms).count();

            prop_assert_eq!(loaded.len(), expected_count);

            // All loaded checkpoints should have expires_at_ms >= now_ms
            for cp in &loaded {
                prop_assert!(cp.expires_at_ms >= now_ms,
                    "Checkpoint {} has expires {} but now is {}",
                    cp.checkpoint_id, cp.expires_at_ms, now_ms);
            }

            Ok(())
        })?;
    }
}

// ─── Property 3: Expired Checkpoint Cleanup ──────────────────────────────────
// **Validates: Requirements 3.4, 11.1**

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn prop_expired_checkpoint_cleanup(
        expires_times in proptest::collection::vec(1000u64..100_000u64, 1..20),
        now_ms in 1000u64..100_000u64,
    ) {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        rt.block_on(async {
            let pm = PersistenceManager::initialize_in_memory().unwrap();

            for (i, &expires) in expires_times.iter().enumerate() {
                let cp = PersistedCheckpoint {
                    checkpoint_id: format!("cp-{}", i),
                    workflow_id: "wf-1".to_string(),
                    step_index: i as u32,
                    state_json: r#"{"data": "test"}"#.to_string(),
                    created_at_ms: 500,
                    expires_at_ms: expires,
                };
                pm.save_checkpoint(&cp).await.unwrap();
            }

            let expected_deleted = expires_times.iter().filter(|&&e| e < now_ms).count() as u64;
            let expected_remaining = expires_times.iter().filter(|&&e| e >= now_ms).count();

            let deleted = pm.cleanup_expired_checkpoints(now_ms).await.unwrap();
            prop_assert_eq!(deleted, expected_deleted);

            let remaining = pm.load_unexpired_checkpoints(0).await.unwrap();
            prop_assert_eq!(remaining.len(), expected_remaining);

            Ok(())
        })?;
    }
}

// ─── Property 4: Single Active Plan Invariant ────────────────────────────────
// **Validates: Requirements 4.2**

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn prop_single_active_plan(
        plan_count in 1usize..15,
    ) {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        rt.block_on(async {
            let pm = PersistenceManager::initialize_in_memory().unwrap();

            for i in 0..plan_count {
                let plan = PlacementPlan {
                    plan_id: format!("plan-{}", i),
                    created_at_ms: (i as u64 + 1) * 1000,
                    plan_json: format!(r#"{{"id": "plan-{}"}}"#, i),
                    utility_score: 0.5 + (i as f64 * 0.01),
                };
                pm.save_plan(&plan).await.unwrap();

                // After each insertion, verify exactly one active plan
                let conn = pm.writer.lock().await;
                let active_count: i64 = conn
                    .query_row("SELECT COUNT(*) FROM placements WHERE is_active = 1", [], |r| r.get(0))
                    .unwrap();
                drop(conn);
                prop_assert_eq!(active_count, 1, "Expected 1 active plan after inserting plan {}", i);
            }

            // The active plan should be the last one inserted
            let active = pm.load_active_plan().await.unwrap().unwrap();
            prop_assert_eq!(active.plan_id, format!("plan-{}", plan_count - 1));

            Ok(())
        })?;
    }
}

// ─── Property 5: Plan Retention Bounded ──────────────────────────────────────
// **Validates: Requirements 4.4, 11.2**

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn prop_plan_retention_bounded(
        plan_count in 11usize..30,
    ) {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        rt.block_on(async {
            let pm = PersistenceManager::initialize_in_memory().unwrap();

            for i in 0..plan_count {
                let plan = PlacementPlan {
                    plan_id: format!("plan-{}", i),
                    created_at_ms: (i as u64 + 1) * 1000,
                    plan_json: format!(r#"{{"id": "plan-{}"}}"#, i),
                    utility_score: 0.5,
                };
                pm.save_plan(&plan).await.unwrap();
            }

            let deleted = pm.enforce_plan_retention(10).await.unwrap();
            prop_assert_eq!(deleted, (plan_count - 10) as u64);

            // Verify exactly 10 remain
            let conn = pm.writer.lock().await;
            let remaining: i64 = conn
                .query_row("SELECT COUNT(*) FROM placements", [], |r| r.get(0))
                .unwrap();
            prop_assert_eq!(remaining, 10);

            // Verify they are the 10 most recent (highest created_at_ms)
            let min_created: i64 = conn
                .query_row("SELECT MIN(created_at_ms) FROM placements", [], |r| r.get(0))
                .unwrap();
            let expected_min = ((plan_count - 10 + 1) as i64) * 1000;
            prop_assert_eq!(min_created, expected_min);

            Ok(())
        })?;
    }
}

// ─── Property 6: Settings Round-Trip ─────────────────────────────────────────
// **Validates: Requirements 5.2**

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn prop_settings_round_trip(
        key in "[a-zA-Z][a-zA-Z0-9_]{0,30}",
        value in arb_json_value(),
    ) {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        rt.block_on(async {
            let pm = PersistenceManager::initialize_in_memory().unwrap();

            pm.set_setting(&key, value.clone()).await.unwrap();
            let loaded = pm.get_setting(&key).await.unwrap();

            prop_assert_eq!(loaded, Some(value));

            Ok(())
        })?;
    }
}

// ─── Property 7: Settings Cache Coherence ────────────────────────────────────
// **Validates: Requirements 5.5**

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn prop_settings_cache_coherence(
        values in proptest::collection::vec(arb_json_value(), 2..10),
    ) {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        rt.block_on(async {
            let pm = PersistenceManager::initialize_in_memory().unwrap();
            let key = "test_key";

            // Write multiple values to the same key
            for value in &values {
                pm.set_setting(key, value.clone()).await.unwrap();
            }

            // get_setting should return the last value written
            let loaded = pm.get_setting(key).await.unwrap();
            prop_assert_eq!(loaded, Some(values.last().unwrap().clone()));

            Ok(())
        })?;
    }
}

// ─── Property 8: Workflow State Round-Trip ───────────────────────────────────
// **Validates: Requirements 6.2**

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn prop_workflow_round_trip(
        status in arb_workflow_status(),
        created_at in 1000u64..999_999_999u64,
        updated_at in 1000u64..999_999_999u64,
        owner in "[a-zA-Z0-9\\-]{5,20}",
    ) {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        rt.block_on(async {
            let pm = PersistenceManager::initialize_in_memory().unwrap();

            let wf = PersistedWorkflow {
                workflow_id: "wf-test".to_string(),
                status: status.clone(),
                dag_json: r#"{"steps": [{"id": "s1"}]}"#.to_string(),
                created_at_ms: created_at,
                updated_at_ms: updated_at,
                owner_node_id: owner.clone(),
            };

            pm.upsert_workflow(&wf).await.unwrap();

            // Load all workflows to verify round-trip
            let conn = pm.writer.lock().await;
            let mut stmt = conn.prepare(
                "SELECT workflow_id, status, dag_json, created_at_ms, updated_at_ms, owner_node_id FROM workflows WHERE workflow_id = ?1"
            ).unwrap();

            let result = stmt.query_row(rusqlite::params!["wf-test"], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, String>(5)?,
                ))
            }).unwrap();

            prop_assert_eq!(&result.0, "wf-test");
            prop_assert_eq!(&result.1, status.as_str());
            prop_assert_eq!(result.3 as u64, created_at);
            prop_assert_eq!(result.4 as u64, updated_at);
            prop_assert_eq!(&result.5, &owner);

            Ok(())
        })?;
    }
}

// ─── Property 9: Running Workflow Load Filtering ─────────────────────────────
// **Validates: Requirements 6.3**

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn prop_running_workflow_filtering(
        statuses in proptest::collection::vec(arb_workflow_status(), 1..15),
    ) {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        rt.block_on(async {
            let pm = PersistenceManager::initialize_in_memory().unwrap();

            for (i, status) in statuses.iter().enumerate() {
                let wf = PersistedWorkflow {
                    workflow_id: format!("wf-{}", i),
                    status: status.clone(),
                    dag_json: r#"{"steps": []}"#.to_string(),
                    created_at_ms: 1000,
                    updated_at_ms: 5000,
                    owner_node_id: "node-1".to_string(),
                };
                pm.upsert_workflow(&wf).await.unwrap();
            }

            let running = pm.load_running_workflows().await.unwrap();
            let expected_count = statuses.iter()
                .filter(|s| **s == WorkflowPersistenceStatus::Running)
                .count();

            prop_assert_eq!(running.len(), expected_count);

            // All loaded workflows should have Running status
            for wf in &running {
                prop_assert_eq!(&wf.status, &WorkflowPersistenceStatus::Running);
            }

            Ok(())
        })?;
    }
}

// ─── Property 10: Stale Workflow Timeout ─────────────────────────────────────
// **Validates: Requirements 6.4**

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn prop_stale_workflow_timeout(
        updated_times in proptest::collection::vec(1000u64..200_000_000u64, 1..15),
        now_ms in 100_000_000u64..200_000_000u64,
    ) {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        rt.block_on(async {
            let pm = PersistenceManager::initialize_in_memory().unwrap();
            let max_age_hours = 24u64;
            let cutoff_ms = now_ms.saturating_sub(max_age_hours * 3600 * 1000);

            for (i, &updated) in updated_times.iter().enumerate() {
                let wf = PersistedWorkflow {
                    workflow_id: format!("wf-{}", i),
                    status: WorkflowPersistenceStatus::Running,
                    dag_json: r#"{"steps": []}"#.to_string(),
                    created_at_ms: 1000,
                    updated_at_ms: updated,
                    owner_node_id: "node-1".to_string(),
                };
                pm.upsert_workflow(&wf).await.unwrap();
            }

            let expected_timed_out = updated_times.iter()
                .filter(|&&t| t < cutoff_ms)
                .count() as u64;

            let timed_out = pm.timeout_stale_workflows(max_age_hours, now_ms).await.unwrap();
            prop_assert_eq!(timed_out, expected_timed_out);

            // Remaining running workflows should all have updated_at_ms >= cutoff
            let running = pm.load_running_workflows().await.unwrap();
            for wf in &running {
                prop_assert!(wf.updated_at_ms >= cutoff_ms,
                    "Workflow {} has updated_at {} but cutoff is {}",
                    wf.workflow_id, wf.updated_at_ms, cutoff_ms);
            }

            Ok(())
        })?;
    }
}

// ─── Property 11: JSON Validation Rejects Malformed Input ────────────────────
// **Validates: Requirements 10.3**

proptest! {
    #![proptest_config(ProptestConfig::with_cases(100))]

    #[test]
    fn prop_json_validation(
        input in "[^{}\\[\\]\",:]{1,50}|\\{[^}]*|\\[[^\\]]*",
    ) {
        // Test that invalid JSON is rejected
        let is_valid = serde_json::from_str::<serde_json::Value>(&input).is_ok();

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        rt.block_on(async {
            let pm = PersistenceManager::initialize_in_memory().unwrap();

            let cp = PersistedCheckpoint {
                checkpoint_id: "cp-test".to_string(),
                workflow_id: "wf-1".to_string(),
                step_index: 0,
                state_json: input.clone(),
                created_at_ms: 1000,
                expires_at_ms: 5000,
            };

            let result = pm.save_checkpoint(&cp).await;

            if is_valid {
                prop_assert!(result.is_ok(), "Valid JSON '{}' was rejected", input);
            } else {
                prop_assert!(matches!(result, Err(PersistenceError::InvalidJson(_))),
                    "Invalid JSON '{}' was not rejected", input);
            }

            Ok(())
        })?;
    }
}
