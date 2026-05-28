// Intent citation: .kiro/specs/unified-mesh-transport/tasks.md Task 12
// Tauri Commands — expose transport state to frontend

use super::registry::UnifiedTopology;
use super::trait_def::TransportHealth;
use serde::{Deserialize, Serialize};

/// Response for get_network_topology command.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopologyResponse {
    pub topology: UnifiedTopology,
}

/// Response for get_transport_health command.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransportHealthResponse {
    pub transports: Vec<TransportHealth>,
}

/// Response for force_path_probe command.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeResponse {
    pub node_id: String,
    pub latency_ms: Option<f64>,
    pub bandwidth_mbps: Option<f64>,
    pub reliability: Option<f64>,
    pub transport_id: String,
}

// Tauri command implementations would be:
//
// #[tauri::command]
// pub async fn get_network_topology(state: State<'_, TransportState>) -> Result<TopologyResponse, String> {
//     let topology = state.manager.registry.topology().await;
//     Ok(TopologyResponse { topology })
// }
//
// #[tauri::command]
// pub async fn get_transport_health(state: State<'_, TransportState>) -> Result<TransportHealthResponse, String> {
//     let health = state.manager.check_all_health();
//     Ok(TransportHealthResponse { transports: health })
// }
//
// #[tauri::command]
// pub async fn force_path_probe(target_node: String, state: State<'_, TransportState>) -> Result<Vec<ProbeResponse>, String> {
//     // Probe all transports for the target node
//     ...
// }

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_response_types_serializable() {
        let response = TransportHealthResponse {
            transports: vec![TransportHealth {
                transport_id: "lan".to_string(),
                is_healthy: true,
                peers_reachable: 3,
                last_successful_send_ms: Some(1000),
                error_rate_percent: 0.5,
                details: "Running".to_string(),
            }],
        };

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("lan"));
        assert!(json.contains("true"));
    }
}
