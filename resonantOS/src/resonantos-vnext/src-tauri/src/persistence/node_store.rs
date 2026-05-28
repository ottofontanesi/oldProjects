// Intent citation: .kiro/specs/node-persistence-layer/tasks.md Task 3
// NodeStore — CRUD operations for the nodes table

use rusqlite::params;

use crate::network::registry::{DeviceType, NodeCapabilities, NodeState, NodeUtilization};
use crate::network::registry::NodeId;

use super::error::PersistenceError;
use super::manager::PersistenceManager;

/// Validate that a string is valid JSON.
fn validate_json(input: &str) -> Result<(), PersistenceError> {
    serde_json::from_str::<serde_json::Value>(input)
        .map(|_| ())
        .map_err(|e| PersistenceError::InvalidJson(format!("JSON validation failed: {}", e)))
}

impl PersistenceManager {
    /// Upsert a node record (insert or update on conflict).
    pub async fn upsert_node(&self, state: &NodeState) -> Result<(), PersistenceError> {
        let node_id = state.capabilities.node_id.to_string();
        let hostname = state.capabilities.hostname.clone();
        let node_type = match state.capabilities.device_type {
            DeviceType::Desktop => "desktop",
            DeviceType::Laptop => "laptop",
            DeviceType::Server => "server",
            DeviceType::Phone => "phone",
        }
        .to_string();

        let capabilities_json = serde_json::to_string(&state.capabilities)?;
        validate_json(&capabilities_json)?;

        let last_seen_ms = state.last_heartbeat_ms as i64;
        let status = if state.is_online { "online" } else { "offline" }.to_string();
        let address = state
            .capabilities
            .network_interfaces
            .first()
            .map(|ni| ni.name.clone());
        let trust_tier: i64 = 0; // Default trust tier

        self.retry_write(move |conn| {
            conn.execute(
                "INSERT OR REPLACE INTO nodes (node_id, hostname, node_type, capabilities_json, last_seen_ms, status, address, trust_tier)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    node_id,
                    hostname,
                    node_type,
                    capabilities_json,
                    last_seen_ms,
                    status,
                    address,
                    trust_tier,
                ],
            )?;
            Ok(())
        })
        .await
    }

    /// Load all persisted node records.
    pub async fn load_all_nodes(&self) -> Result<Vec<NodeState>, PersistenceError> {
        let conn = self.writer.lock().await;
        let mut stmt = conn.prepare(
            "SELECT node_id, hostname, node_type, capabilities_json, last_seen_ms, status, address, trust_tier FROM nodes"
        )?;

        let rows = stmt.query_map([], |row| {
            let capabilities_json: String = row.get(3)?;
            let last_seen_ms: i64 = row.get(4)?;
            let status: String = row.get(5)?;
            Ok((capabilities_json, last_seen_ms, status))
        })?;

        let mut nodes = Vec::new();
        for row_result in rows {
            match row_result {
                Ok((capabilities_json, last_seen_ms, status)) => {
                    match serde_json::from_str::<NodeCapabilities>(&capabilities_json) {
                        Ok(capabilities) => {
                            let node_id = capabilities.node_id;
                            nodes.push(NodeState {
                                capabilities,
                                utilization: NodeUtilization {
                                    node_id,
                                    ..Default::default()
                                },
                                loaded_models: Vec::new(),
                                stability_score: 0.95,
                                last_heartbeat_ms: last_seen_ms as u64,
                                is_online: status == "online",
                                latency_to_peers: std::collections::HashMap::new(),
                                thermal_state: Default::default(),
                            });
                        }
                        Err(e) => {
                            eprintln!("Warning: skipping node with invalid capabilities JSON: {}", e);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Warning: skipping node row with error: {}", e);
                }
            }
        }

        Ok(nodes)
    }

    /// Delete a node record by ID.
    pub async fn delete_node(&self, node_id: &NodeId) -> Result<(), PersistenceError> {
        let id_str = node_id.to_string();
        self.retry_write(move |conn| {
            conn.execute("DELETE FROM nodes WHERE node_id = ?1", params![id_str])?;
            Ok(())
        })
        .await
    }

    /// Delete nodes not seen for more than `max_age_days` days.
    /// Returns the number of rows deleted.
    pub async fn cleanup_stale_nodes(&self, max_age_days: u32, now_ms: u64) -> Result<u64, PersistenceError> {
        let cutoff_ms = now_ms as i64 - (max_age_days as i64 * 86400 * 1000);
        self.retry_write(move |conn| {
            let deleted = conn.execute(
                "DELETE FROM nodes WHERE last_seen_ms < ?1",
                params![cutoff_ms],
            )?;
            Ok(deleted as u64)
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::registry::*;
    use uuid::Uuid;

    fn make_test_node(device_type: DeviceType) -> NodeState {
        let node_id = Uuid::new_v4();
        NodeState {
            capabilities: NodeCapabilities {
                node_id,
                hostname: "test-host".to_string(),
                device_type,
                cpu: CpuProfile {
                    cores: 8,
                    architecture: "x86_64".to_string(),
                    clock_mhz: 3600,
                    isa_extensions: vec!["avx2".to_string()],
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
            last_heartbeat_ms: 1000000,
            is_online: true,
            latency_to_peers: std::collections::HashMap::new(),
            thermal_state: ThermalState::Normal,
        }
    }

    #[tokio::test]
    async fn test_upsert_and_load_node() {
        let pm = PersistenceManager::initialize_in_memory().unwrap();
        let node = make_test_node(DeviceType::Desktop);
        let node_id = node.capabilities.node_id;

        pm.upsert_node(&node).await.unwrap();

        let loaded = pm.load_all_nodes().await.unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].capabilities.node_id, node_id);
        assert_eq!(loaded[0].capabilities.hostname, "test-host");
        assert_eq!(loaded[0].capabilities.device_type, DeviceType::Desktop);
        assert!(loaded[0].is_online);
    }

    #[tokio::test]
    async fn test_upsert_updates_existing() {
        let pm = PersistenceManager::initialize_in_memory().unwrap();
        let mut node = make_test_node(DeviceType::Laptop);

        pm.upsert_node(&node).await.unwrap();

        // Update the node
        node.is_online = false;
        node.last_heartbeat_ms = 2000000;
        pm.upsert_node(&node).await.unwrap();

        let loaded = pm.load_all_nodes().await.unwrap();
        assert_eq!(loaded.len(), 1);
        assert!(!loaded[0].is_online);
        assert_eq!(loaded[0].last_heartbeat_ms, 2000000);
    }

    #[tokio::test]
    async fn test_delete_node() {
        let pm = PersistenceManager::initialize_in_memory().unwrap();
        let node = make_test_node(DeviceType::Server);
        let node_id = node.capabilities.node_id;

        pm.upsert_node(&node).await.unwrap();
        pm.delete_node(&node_id).await.unwrap();

        let loaded = pm.load_all_nodes().await.unwrap();
        assert_eq!(loaded.len(), 0);
    }

    #[tokio::test]
    async fn test_cleanup_stale_nodes() {
        let pm = PersistenceManager::initialize_in_memory().unwrap();

        let mut old_node = make_test_node(DeviceType::Desktop);
        old_node.last_heartbeat_ms = 1000; // Very old

        let mut fresh_node = make_test_node(DeviceType::Laptop);
        fresh_node.last_heartbeat_ms = 99_000_000_000; // Recent

        pm.upsert_node(&old_node).await.unwrap();
        pm.upsert_node(&fresh_node).await.unwrap();

        // Cleanup with now = 100_000_000_000, max_age = 30 days
        let now_ms = 100_000_000_000u64;
        let deleted = pm.cleanup_stale_nodes(30, now_ms).await.unwrap();
        assert_eq!(deleted, 1);

        let loaded = pm.load_all_nodes().await.unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].capabilities.device_type, DeviceType::Laptop);
    }

    #[tokio::test]
    async fn test_phone_node_with_phone_info() {
        let pm = PersistenceManager::initialize_in_memory().unwrap();
        let node_id = Uuid::new_v4();
        let node = NodeState {
            capabilities: NodeCapabilities {
                node_id,
                hostname: "pixel-7".to_string(),
                device_type: DeviceType::Phone,
                cpu: CpuProfile {
                    cores: 8,
                    architecture: "arm64".to_string(),
                    clock_mhz: 2800,
                    isa_extensions: vec![],
                },
                ram: RamProfile {
                    total_mb: 8192,
                    available_mb: 4096,
                    ddr_generation: 5,
                },
                gpu: None,
                storage: StorageProfile {
                    storage_type: StorageType::Ssd,
                    available_mb: 64000,
                    read_speed_mbps: 1500,
                },
                network_interfaces: vec![],
                phone_info: Some(PhoneInfo {
                    os: PhoneOs::Android,
                    npu: Some(NpuType::QualcommHexagon {
                        version: "v73".to_string(),
                    }),
                    battery_percent: 85,
                    is_charging: true,
                    connection_type: ConnectionType::Wifi,
                }),
                available_tools: vec![],
            },
            utilization: NodeUtilization {
                node_id,
                ..Default::default()
            },
            loaded_models: Vec::new(),
            stability_score: 0.9,
            last_heartbeat_ms: 5000000,
            is_online: true,
            latency_to_peers: std::collections::HashMap::new(),
            thermal_state: ThermalState::Normal,
        };

        pm.upsert_node(&node).await.unwrap();

        let loaded = pm.load_all_nodes().await.unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].capabilities.device_type, DeviceType::Phone);
        let phone_info = loaded[0].capabilities.phone_info.as_ref().unwrap();
        assert_eq!(phone_info.os, PhoneOs::Android);
        assert_eq!(phone_info.battery_percent, 85);
        assert!(phone_info.is_charging);
    }
}
