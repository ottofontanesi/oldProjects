// IPC Payloads — event payload structs for dashboard data polling
//
// All payloads are pushed from backend emitter tasks to the frontend
// via Tauri's event system. Timestamps are u64 milliseconds since epoch.

use serde::Serialize;

/// Payload for the `node-status-update` event channel.
#[derive(Debug, Serialize, Clone)]
pub struct NodeStatusPayload {
    pub nodes: Vec<NodeSnapshot>,
    pub is_full_sync: bool,
    pub timestamp_ms: u64,
}

/// A snapshot of a single node's current state.
#[derive(Debug, Serialize, Clone, PartialEq)]
pub struct NodeSnapshot {
    pub node_id: String,
    pub hostname: String,
    pub device_type: String,
    pub online: bool,
    pub cpu_percent: f64,
    pub ram_used_mb: u64,
    pub ram_total_mb: u64,
    pub vram_used_mb: u64,
    pub vram_total_mb: u64,
    pub models_loaded: Vec<String>,
}

/// Payload for the `placement-update` event channel.
#[derive(Debug, Serialize, Clone)]
pub struct PlacementPayload {
    pub plan_id: String,
    pub utility_score: f64,
    pub created_at_ms: u64,
    pub is_new_plan: bool,
}

/// Snapshot of a transport adapter's health.
#[derive(Debug, Serialize, Clone)]
pub struct AdapterSnapshot {
    pub adapter_id: String,
    pub adapter_name: String,
    pub is_healthy: bool,
    pub peers_reachable: u32,
    pub latency_avg_ms: f64,
}

/// Snapshot of a transport path between two nodes.
#[derive(Debug, Serialize, Clone)]
pub struct PathSnapshot {
    pub source_node_id: String,
    pub target_node_id: String,
    pub transport_type: String,
    pub latency_ms: f64,
    pub bandwidth_mbps: f64,
    pub status: String,
}

/// Payload for the `transport-health-update` event channel.
#[derive(Debug, Serialize, Clone)]
pub struct TransportHealthPayload {
    pub adapters: Vec<AdapterSnapshot>,
    pub paths: Vec<PathSnapshot>,
    pub timestamp_ms: u64,
}

/// Payload for the `utility-update` event channel.
#[derive(Debug, Serialize, Clone)]
pub struct UtilityPayload {
    pub quality: f64,
    pub speed: f64,
    pub coverage: f64,
    pub total: f64,
    pub trend: String,
    pub timestamp_ms: u64,
}

/// Payload for the `download-progress` event channel.
#[derive(Debug, Serialize, Clone)]
pub struct DownloadProgressPayload {
    pub id: String,
    pub model_id: String,
    pub bytes_downloaded: u64,
    pub total_bytes: u64,
    pub speed_bps: u64,
    pub eta_secs: u64,
    pub percent: f64,
}

/// Snapshot of a paired companion phone.
#[derive(Debug, Serialize, Clone)]
pub struct CompanionSnapshot {
    pub node_id: String,
    pub device_name: String,
    pub os: String,
    pub battery_percent: u8,
    pub is_charging: bool,
    pub online: bool,
    pub tokens_per_second: f64,
}

/// Payload for the `companion-status-update` event channel.
#[derive(Debug, Serialize, Clone)]
pub struct CompanionPayload {
    pub phones: Vec<CompanionSnapshot>,
    pub timestamp_ms: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_status_payload_serializes() {
        let payload = NodeStatusPayload {
            nodes: vec![NodeSnapshot {
                node_id: "node-1".into(),
                hostname: "desktop".into(),
                device_type: "desktop".into(),
                online: true,
                cpu_percent: 45.0,
                ram_used_mb: 8192,
                ram_total_mb: 16384,
                vram_used_mb: 4096,
                vram_total_mb: 8192,
                models_loaded: vec!["llama-7b".into()],
            }],
            is_full_sync: true,
            timestamp_ms: 1700000000000,
        };
        let json = serde_json::to_string(&payload).unwrap();
        assert!(json.contains("node-1"));
        assert!(json.contains("is_full_sync"));
    }

    #[test]
    fn test_placement_payload_serializes() {
        let payload = PlacementPayload {
            plan_id: "plan-abc".into(),
            utility_score: 0.85,
            created_at_ms: 1700000000000,
            is_new_plan: true,
        };
        let json = serde_json::to_string(&payload).unwrap();
        assert!(json.contains("plan-abc"));
        assert!(json.contains("0.85"));
    }

    #[test]
    fn test_transport_health_payload_serializes() {
        let payload = TransportHealthPayload {
            adapters: vec![AdapterSnapshot {
                adapter_id: "tcp-1".into(),
                adapter_name: "TCP Direct".into(),
                is_healthy: true,
                peers_reachable: 3,
                latency_avg_ms: 12.5,
            }],
            paths: vec![PathSnapshot {
                source_node_id: "node-1".into(),
                target_node_id: "node-2".into(),
                transport_type: "tcp".into(),
                latency_ms: 15.0,
                bandwidth_mbps: 100.0,
                status: "active".into(),
            }],
            timestamp_ms: 1700000000000,
        };
        let json = serde_json::to_string(&payload).unwrap();
        assert!(json.contains("TCP Direct"));
        assert!(json.contains("node-2"));
    }

    #[test]
    fn test_utility_payload_serializes() {
        let payload = UtilityPayload {
            quality: 0.9,
            speed: 0.8,
            coverage: 0.7,
            total: 0.8,
            trend: "improving".into(),
            timestamp_ms: 1700000000000,
        };
        let json = serde_json::to_string(&payload).unwrap();
        assert!(json.contains("improving"));
    }

    #[test]
    fn test_download_progress_payload_serializes() {
        let payload = DownloadProgressPayload {
            id: "dl-1".into(),
            model_id: "llama-7b".into(),
            bytes_downloaded: 5_000_000_000,
            total_bytes: 10_000_000_000,
            speed_bps: 50_000_000,
            eta_secs: 100,
            percent: 50.0,
        };
        let json = serde_json::to_string(&payload).unwrap();
        assert!(json.contains("dl-1"));
        assert!(json.contains("50"));
    }

    #[test]
    fn test_companion_payload_serializes() {
        let payload = CompanionPayload {
            phones: vec![CompanionSnapshot {
                node_id: "phone-1".into(),
                device_name: "Pixel 8".into(),
                os: "Android 14".into(),
                battery_percent: 85,
                is_charging: false,
                online: true,
                tokens_per_second: 12.5,
            }],
            timestamp_ms: 1700000000000,
        };
        let json = serde_json::to_string(&payload).unwrap();
        assert!(json.contains("Pixel 8"));
        assert!(json.contains("85"));
    }
}
