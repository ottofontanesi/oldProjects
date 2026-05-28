// Intent citation: .kiro/specs/node-persistence-layer/tasks.md Task 10
// Integration tests — concurrent access, transaction atomicity, lifecycle, error handling

use std::sync::Arc;

use crate::network::registry::*;
use crate::persistence::manager::PersistenceManager;
use crate::persistence::models::*;
use crate::persistence::placement_store::PlacementPlan;

use uuid::Uuid;

fn make_test_node_with_id(node_id: Uuid, heartbeat: u64) -> NodeState {
    NodeState {
        capabilities: NodeCapabilities {
            node_id,
            hostname: format!("node-{}", node_id),
            device_type: DeviceType::Desktop,
            cpu: CpuProfile {
                cores: 8,
                architecture: "x86_64".to_string(),
                clock_mhz: 3600,
                isa_extensions: vec![],
            },
            ram: RamProfile {
                total_mb: 32768,
                available_mb: 16384,
                ddr_generation: 4,
            },
            gpu: None,
            storage: StorageProfile {
                storage_type: StorageType::Nvme,
                available_mb: 500000,
                read_speed_mbps: 3500,
            },
            network_interfaces: vec![NetworkInterface {
                name: "eth0".to_string(),
                interface_type: InterfaceType::Ethernet,
                bandwidth_mbps: 1000,
            }],
            phone_info: None,
            available_tools: vec![],
        },
        utilization: NodeUtilization {
            node_id,
            ..Default::default()
        },
        loaded_models: Vec::new(),
        stability_score: 0.95,
        last_heartbeat_ms: heartbeat,
        is_online: true,
        latency_to_peers: std::collections::HashMap::new(),
        thermal_state: ThermalState::Normal,
    }
}

// ─── Task 10.1: Concurrent Access Tests ──────────────────────────────────────

#[tokio::test]
async fn test_concurrent_writes_and_reads() {
    let pm = Arc::new(PersistenceManager::initialize_in_memory().unwrap());

    // Spawn writer task: insert 20 nodes sequentially
    let pm_writer = pm.clone();
    let writer_handle = tokio::spawn(async move {
        for i in 0..20 {
            let node_id = Uuid::from_u128(i as u128 + 1);
            let node = make_test_node_with_id(node_id, 1_000_000);
            pm_writer.upsert_node(&node).await.unwrap();
        }
    });

    // Wait for writer to complete
    writer_handle.await.unwrap();

    // Verify all 20 nodes are present
    let loaded = pm.load_all_nodes().await.unwrap();
    assert_eq!(loaded.len(), 20);
}

#[tokio::test]
async fn test_concurrent_settings_operations() {
    let pm = Arc::new(PersistenceManager::initialize_in_memory().unwrap());

    // Write multiple settings concurrently
    let mut handles = Vec::new();
    for i in 0..10 {
        let pm_clone = pm.clone();
        handles.push(tokio::spawn(async move {
            pm_clone
                .set_setting(
                    &format!("key-{}", i),
                    serde_json::json!({"index": i}),
                )
                .await
                .unwrap();
        }));
    }

    for handle in handles {
        handle.await.unwrap();
    }

    // Verify all settings are present
    for i in 0..10 {
        let value = pm.get_setting(&format!("key-{}", i)).await.unwrap();
        assert!(value.is_some());
        assert_eq!(value.unwrap()["index"], i);
    }
}

// ─── Task 10.2: Transaction Atomicity Test ───────────────────────────────────

#[tokio::test]
async fn test_plan_save_is_atomic() {
    let pm = PersistenceManager::initialize_in_memory().unwrap();

    // Save first plan
    let plan1 = PlacementPlan {
        plan_id: "plan-1".to_string(),
        created_at_ms: 1000,
        plan_json: r#"{"assignments": ["a"]}"#.to_string(),
        utility_score: 0.8,
    };
    pm.save_plan(&plan1).await.unwrap();

    // Save second plan — this should atomically deactivate plan-1 and activate plan-2
    let plan2 = PlacementPlan {
        plan_id: "plan-2".to_string(),
        created_at_ms: 2000,
        plan_json: r#"{"assignments": ["b"]}"#.to_string(),
        utility_score: 0.9,
    };
    pm.save_plan(&plan2).await.unwrap();

    // Verify only plan-2 is active
    let active = pm.load_active_plan().await.unwrap().unwrap();
    assert_eq!(active.plan_id, "plan-2");

    // Verify plan-1 is still in DB but inactive
    let conn = pm.writer.lock().await;
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM placements", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 2);

    let plan1_active: i64 = conn
        .query_row(
            "SELECT is_active FROM placements WHERE plan_id = 'plan-1'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(plan1_active, 0);
}

// ─── Task 10.3: Startup Lifecycle Integration Test ───────────────────────────

#[tokio::test]
async fn test_lifecycle_with_tempfile() {
    // Use a temp directory for a real file-based test
    let tmp_dir = tempfile::tempdir().unwrap();
    let db_path = tmp_dir.path().join("state.db");

    // First initialization
    {
        let pm = PersistenceManager::initialize(&db_path).unwrap();

        // Write some data
        pm.set_setting("lifecycle_test", serde_json::json!("hello"))
            .await
            .unwrap();

        let node = make_test_node_with_id(Uuid::from_u128(42), 5_000_000);
        pm.upsert_node(&node).await.unwrap();

        let plan = PlacementPlan {
            plan_id: "plan-lifecycle".to_string(),
            created_at_ms: 1000,
            plan_json: r#"{"test": true}"#.to_string(),
            utility_score: 0.75,
        };
        pm.save_plan(&plan).await.unwrap();

        // Shutdown
        pm.shutdown().await.unwrap();
    }

    // Second initialization — data should persist
    {
        let pm = PersistenceManager::initialize(&db_path).unwrap();

        let setting = pm.get_setting("lifecycle_test").await.unwrap();
        assert_eq!(setting, Some(serde_json::json!("hello")));

        let nodes = pm.load_all_nodes().await.unwrap();
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].capabilities.node_id, Uuid::from_u128(42));

        let plan = pm.load_active_plan().await.unwrap();
        assert!(plan.is_some());
        assert_eq!(plan.unwrap().plan_id, "plan-lifecycle");
    }
}

#[tokio::test]
async fn test_migrations_dont_rerun() {
    let tmp_dir = tempfile::tempdir().unwrap();
    let db_path = tmp_dir.path().join("state.db");

    // First init
    {
        let _pm = PersistenceManager::initialize(&db_path).unwrap();
    }

    // Second init — should not fail (migrations already applied)
    {
        let pm = PersistenceManager::initialize(&db_path).unwrap();
        // Verify tables still exist
        let conn = pm.writer.lock().await;
        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert!(tables.contains(&"nodes".to_string()));
        assert!(tables.contains(&"checkpoints".to_string()));
        assert!(tables.contains(&"placements".to_string()));
        assert!(tables.contains(&"settings".to_string()));
        assert!(tables.contains(&"workflows".to_string()));
    }
}

// ─── Task 10.4: Error Handling Integration Tests ─────────────────────────────

#[tokio::test]
async fn test_health_status_updates_on_write() {
    let pm = PersistenceManager::initialize_in_memory().unwrap();

    // Initially no successful write
    let health = pm.health_status().await;
    assert!(health.last_successful_write_ms.is_none());
    assert_eq!(health.error_count, 0);

    // Perform a write
    pm.set_setting("test", serde_json::json!(1)).await.unwrap();

    // Health should reflect successful write
    let health = pm.health_status().await;
    assert!(health.last_successful_write_ms.is_some());
}

#[tokio::test]
async fn test_read_only_mode_rejects_writes() {
    let pm = PersistenceManager::initialize_in_memory().unwrap();

    // Force read-only mode
    pm.set_read_only().await;

    // Writes should fail with ReadOnly error
    let result = pm.set_setting("key", serde_json::json!("value")).await;
    assert!(matches!(result, Err(super::PersistenceError::ReadOnly)));

    // Reads should still work (from cache or empty)
    let result = pm.get_setting("nonexistent").await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_corrupt_database_recovery() {
    let tmp_dir = tempfile::tempdir().unwrap();
    let db_path = tmp_dir.path().join("state.db");

    // Write garbage to the file
    std::fs::write(&db_path, b"this is not a valid sqlite database file!!!").unwrap();

    // Initialize should detect corruption and create fresh DB
    let pm = PersistenceManager::initialize(&db_path).unwrap();

    // Should work with fresh database
    pm.set_setting("recovered", serde_json::json!(true))
        .await
        .unwrap();
    let value = pm.get_setting("recovered").await.unwrap();
    assert_eq!(value, Some(serde_json::json!(true)));

    // Corrupt file should have been renamed
    let entries: Vec<_> = std::fs::read_dir(tmp_dir.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_name()
                .to_string_lossy()
                .contains("state.db.corrupt")
        })
        .collect();
    assert!(!entries.is_empty(), "Corrupt file should have been renamed");
}

#[tokio::test]
async fn test_full_cleanup_cycle() {
    let pm = PersistenceManager::initialize_in_memory().unwrap();

    // Add some data that should be cleaned up
    let old_node = make_test_node_with_id(Uuid::from_u128(1), 1000); // Very old
    pm.upsert_node(&old_node).await.unwrap();

    let fresh_node = make_test_node_with_id(Uuid::from_u128(2), 999_999_999_999);
    pm.upsert_node(&fresh_node).await.unwrap();

    // Add expired checkpoint
    let expired_cp = PersistedCheckpoint {
        checkpoint_id: "cp-expired".to_string(),
        workflow_id: "wf-1".to_string(),
        step_index: 0,
        state_json: r#"{"data": "old"}"#.to_string(),
        created_at_ms: 1000,
        expires_at_ms: 1000, // Already expired
    };
    pm.save_checkpoint(&expired_cp).await.unwrap();

    // Add many plans
    for i in 0..15 {
        let plan = PlacementPlan {
            plan_id: format!("plan-{}", i),
            created_at_ms: (i + 1) * 1000,
            plan_json: format!(r#"{{"id": {}}}"#, i),
            utility_score: 0.5,
        };
        pm.save_plan(&plan).await.unwrap();
    }

    // Run cleanup
    let report = pm.run_cleanup().await.unwrap();

    assert!(report.expired_checkpoints_deleted >= 1);
    assert!(report.stale_nodes_deleted >= 1);
    assert_eq!(report.old_plans_deleted, 5); // 15 - 10 = 5

    // Verify fresh node still exists
    let nodes = pm.load_all_nodes().await.unwrap();
    assert_eq!(nodes.len(), 1);
    assert_eq!(nodes[0].capabilities.node_id, Uuid::from_u128(2));
}
