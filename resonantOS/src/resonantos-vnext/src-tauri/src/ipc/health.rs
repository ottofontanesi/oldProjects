// IPC Health Commands — node health, node list, network topology
//
// 3 commands for querying node health and network topology.

use super::state::AppState;
use super::types::{
    NetworkTopologyResponse, NodeHealthResponse, NodeSummary, TopologyConnection, TopologyNode,
};
use crate::network::registry::DeviceType;

fn device_type_to_string(dt: &DeviceType) -> String {
    match dt {
        DeviceType::Desktop => "desktop".to_string(),
        DeviceType::Laptop => "laptop".to_string(),
        DeviceType::Server => "server".to_string(),
        DeviceType::Phone => "phone".to_string(),
    }
}

/// Get health data for a specific node.
pub async fn get_node_health(
    state: &AppState,
    node_id: String,
) -> Result<NodeHealthResponse, String> {
    let registry_guard = state.network_registry.read().await;
    let registry = registry_guard
        .as_ref()
        .ok_or_else(|| "Network registry not initialized. Please wait for startup to complete.".to_string())?;

    let node_uuid: uuid::Uuid = node_id
        .parse()
        .map_err(|_| format!("Invalid node_id: '{}'", node_id))?;

    let node_state = registry
        .get_node(&node_uuid)
        .await
        .ok_or_else(|| format!("Node '{}' not found in registry", node_id))?;

    let vram_used_mb = node_state.utilization.vram_used_mb.unwrap_or(0);
    let vram_total_mb = node_state
        .capabilities
        .gpu
        .as_ref()
        .map(|g| g.vram_mb)
        .unwrap_or(0);

    Ok(NodeHealthResponse {
        node_id,
        hostname: node_state.capabilities.hostname.clone(),
        device_type: device_type_to_string(&node_state.capabilities.device_type),
        cpu_percent: node_state.utilization.cpu_percent as f64,
        ram_used_mb: node_state.utilization.ram_used_mb,
        ram_total_mb: node_state.capabilities.ram.total_mb,
        vram_used_mb,
        vram_total_mb,
        online: node_state.is_online,
        last_seen_ms: node_state.last_heartbeat_ms,
        stability_score: node_state.stability_score,
        models_loaded: node_state
            .loaded_models
            .iter()
            .map(|m| m.model_id.clone())
            .collect(),
        tools_available: node_state
            .capabilities
            .available_tools
            .iter()
            .map(|t| t.tool_id.clone())
            .collect(),
    })
}

/// List all known nodes with summary info.
pub async fn list_all_nodes(
    state: &AppState,
) -> Result<Vec<NodeSummary>, String> {
    let registry_guard = state.network_registry.read().await;
    let registry = registry_guard
        .as_ref()
        .ok_or_else(|| "Network registry not initialized. Please wait for startup to complete.".to_string())?;

    let all_nodes = registry.all_nodes().await;
    let summaries: Vec<NodeSummary> = all_nodes
        .iter()
        .map(|n| NodeSummary {
            node_id: n.capabilities.node_id.to_string(),
            hostname: n.capabilities.hostname.clone(),
            device_type: device_type_to_string(&n.capabilities.device_type),
            online: n.is_online,
            ram_total_mb: n.capabilities.ram.total_mb,
            gpu_name: n.capabilities.gpu.as_ref().map(|g| g.model.clone()),
            models_loaded_count: n.loaded_models.len() as u32,
        })
        .collect();

    Ok(summaries)
}

/// Get the full network topology (nodes + connections).
///
/// Builds topology from the node registry's latency measurements.
pub async fn get_network_topology(
    state: &AppState,
) -> Result<NetworkTopologyResponse, String> {
    let registry_guard = state.network_registry.read().await;
    let registry = registry_guard
        .as_ref()
        .ok_or_else(|| "Network registry not initialized. Please wait for startup to complete.".to_string())?;

    let all_nodes = registry.all_nodes().await;

    // Build topology nodes with simple grid layout
    let nodes: Vec<TopologyNode> = all_nodes
        .iter()
        .enumerate()
        .map(|(i, n)| {
            let x = (i % 4) as f64 * 200.0;
            let y = (i / 4) as f64 * 200.0;
            TopologyNode {
                node_id: n.capabilities.node_id.to_string(),
                hostname: n.capabilities.hostname.clone(),
                device_type: device_type_to_string(&n.capabilities.device_type),
                online: n.is_online,
                x,
                y,
            }
        })
        .collect();

    // Build connections from latency measurements
    let mut connections = Vec::new();
    for node_state in &all_nodes {
        for (peer_id, measurement) in &node_state.latency_to_peers {
            connections.push(TopologyConnection {
                source_node_id: node_state.capabilities.node_id.to_string(),
                target_node_id: peer_id.to_string(),
                transport_type: "lan".to_string(),
                latency_ms: measurement.rtt_ms,
                bandwidth_mbps: measurement.bandwidth_mbps,
                is_active: node_state.is_online,
            });
        }
    }

    Ok(NetworkTopologyResponse { nodes, connections })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use crate::network::registry::*;

    fn sample_capabilities(node_id: uuid::Uuid) -> NodeCapabilities {
        NodeCapabilities {
            node_id,
            hostname: "test-node".to_string(),
            device_type: DeviceType::Desktop,
            cpu: CpuProfile {
                cores: 8,
                architecture: "x86_64".to_string(),
                clock_mhz: 4000,
                isa_extensions: vec![],
            },
            ram: RamProfile {
                total_mb: 32768,
                available_mb: 24000,
                ddr_generation: 4,
            },
            gpu: Some(GpuProfile {
                model: "RTX 4090".to_string(),
                vram_mb: 24576,
                vram_available_mb: 20000,
                compute_capability: 8.9,
                backend: GpuBackend::Cuda,
            }),
            storage: StorageProfile {
                storage_type: StorageType::Nvme,
                available_mb: 500000,
                read_speed_mbps: 7000,
            },
            network_interfaces: vec![],
            phone_info: None,
            available_tools: vec![],
        }
    }

    async fn make_state_with_registry() -> AppState {
        let state = AppState::new();
        let registry = Arc::new(NodeRegistry::new());
        *state.network_registry.write().await = Some(registry);
        state
    }

    #[tokio::test]
    async fn test_get_node_health_valid_node() {
        let state = make_state_with_registry().await;
        let node_id = uuid::Uuid::new_v4();

        {
            let registry_guard = state.network_registry.read().await;
            let registry = registry_guard.as_ref().unwrap();
            registry.register(sample_capabilities(node_id)).await;
        }

        let result = get_node_health(&state, node_id.to_string()).await;
        assert!(result.is_ok());
        let health = result.unwrap();
        assert_eq!(health.node_id, node_id.to_string());
        assert_eq!(health.hostname, "test-node");
        assert_eq!(health.device_type, "desktop");
        assert_eq!(health.ram_total_mb, 32768);
        assert!(health.online);
    }

    #[tokio::test]
    async fn test_get_node_health_unknown_node() {
        let state = make_state_with_registry().await;
        let fake_id = uuid::Uuid::new_v4().to_string();

        let result = get_node_health(&state, fake_id).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not found"));
    }

    #[tokio::test]
    async fn test_get_node_health_uninitialized() {
        let state = AppState::new();
        let result = get_node_health(&state, "some-id".to_string()).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not initialized"));
    }

    #[tokio::test]
    async fn test_list_all_nodes_returns_registered() {
        let state = make_state_with_registry().await;
        let node1 = uuid::Uuid::new_v4();
        let node2 = uuid::Uuid::new_v4();

        {
            let registry_guard = state.network_registry.read().await;
            let registry = registry_guard.as_ref().unwrap();
            registry.register(sample_capabilities(node1)).await;
            registry.register(sample_capabilities(node2)).await;
        }

        let result = list_all_nodes(&state).await;
        assert!(result.is_ok());
        let nodes = result.unwrap();
        assert_eq!(nodes.len(), 2);
    }

    #[tokio::test]
    async fn test_get_network_topology_includes_connections() {
        let state = make_state_with_registry().await;
        let node1 = uuid::Uuid::new_v4();
        let node2 = uuid::Uuid::new_v4();

        {
            let registry_guard = state.network_registry.read().await;
            let registry = registry_guard.as_ref().unwrap();
            registry.register(sample_capabilities(node1)).await;
            registry.register(sample_capabilities(node2)).await;
            registry
                .update_latency(
                    &node1,
                    LatencyMeasurement {
                        peer_id: node2,
                        rtt_ms: 5.0,
                        bandwidth_mbps: 1000.0,
                        measured_at_ms: 1000,
                    },
                )
                .await;
        }

        let result = get_network_topology(&state).await;
        assert!(result.is_ok());
        let topology = result.unwrap();
        assert_eq!(topology.nodes.len(), 2);
        assert_eq!(topology.connections.len(), 1);
        assert_eq!(topology.connections[0].latency_ms, 5.0);
    }
}
