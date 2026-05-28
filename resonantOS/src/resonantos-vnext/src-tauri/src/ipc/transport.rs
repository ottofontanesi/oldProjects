// IPC Transport Commands — adapter status, paths, failover history
//
// 3 commands for querying transport layer health and connectivity.

use super::state::AppState;
use super::types::{FailoverEvent, TransportAdapterStatus, TransportPathResponse};

/// Get per-adapter transport health.
pub async fn get_transport_status(
    state: &AppState,
) -> Result<Vec<TransportAdapterStatus>, String> {
    let manager_guard = state.transport_manager.read().await;
    let manager = manager_guard
        .as_ref()
        .ok_or_else(|| "Transport manager not initialized. Please wait for startup to complete.".to_string())?;

    let health_reports = manager.check_all_health();
    let statuses: Vec<TransportAdapterStatus> = health_reports
        .iter()
        .map(|h| {
            let reason = if !h.is_healthy {
                Some(h.details.clone())
            } else {
                None
            };
            TransportAdapterStatus {
                adapter_id: h.transport_id.clone(),
                adapter_name: h.transport_id.clone(), // Name same as ID for now
                is_healthy: h.is_healthy,
                peers_reachable: h.peers_reachable,
                error_rate_percent: h.error_rate_percent,
                latency_avg_ms: 0.0, // Not available from health check directly
                bandwidth_avg_mbps: 0.0,
                reason,
            }
        })
        .collect();

    Ok(statuses)
}

/// Get all known transport paths between nodes.
///
/// Reads topology from the transport manager's unified registry.
pub async fn get_transport_paths(
    state: &AppState,
) -> Result<Vec<TransportPathResponse>, String> {
    let manager_guard = state.transport_manager.read().await;
    let manager = manager_guard
        .as_ref()
        .ok_or_else(|| "Transport manager not initialized. Please wait for startup to complete.".to_string())?;

    let topology = manager.registry.topology().await;
    let paths: Vec<TransportPathResponse> = topology
        .paths
        .iter()
        .map(|p| {
            let status = if p.metrics.reliability >= 0.95 {
                "active"
            } else if p.metrics.reliability >= 0.5 {
                "degraded"
            } else {
                "failed"
            };
            TransportPathResponse {
                source_node_id: p.source.to_string(),
                target_node_id: p.destination.to_string(),
                transport_type: p.transport_id.clone(),
                latency_ms: p.metrics.latency_ms,
                bandwidth_mbps: p.metrics.bandwidth_mbps,
                reliability: p.metrics.reliability,
                status: status.to_string(),
            }
        })
        .collect();

    Ok(paths)
}

/// Get recent failover events.
///
/// Reads from the failover manager's current failover states.
/// Returns nodes that are currently in failover as events.
pub async fn get_failover_history(
    state: &AppState,
    limit: Option<u32>,
) -> Result<Vec<FailoverEvent>, String> {
    let manager_guard = state.transport_manager.read().await;
    let manager = manager_guard
        .as_ref()
        .ok_or_else(|| "Transport manager not initialized. Please wait for startup to complete.".to_string())?;

    let limit = limit.unwrap_or(20) as usize;

    // Get currently failed-over nodes as "events"
    let failed_nodes = manager.failover.failed_over_nodes();
    let result: Vec<FailoverEvent> = failed_nodes
        .iter()
        .take(limit)
        .map(|s| FailoverEvent {
            timestamp_ms: s.failover_at_ms.unwrap_or(0),
            node_id: s.node_id.to_string(),
            from_transport: s.primary_transport.clone(),
            to_transport: s.current_transport.clone(),
            reason: format!("{} consecutive failures", s.consecutive_failures),
        })
        .collect();

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::manager::TransportManager;

    async fn make_state_with_transport() -> AppState {
        let state = AppState::new();
        let manager = TransportManager::new(uuid::Uuid::new_v4());
        *state.transport_manager.write().await = Some(manager);
        state
    }

    #[tokio::test]
    async fn test_get_transport_status_empty() {
        let state = make_state_with_transport().await;
        let result = get_transport_status(&state).await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_get_transport_status_uninitialized() {
        let state = AppState::new();
        let result = get_transport_status(&state).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not initialized"));
    }

    #[tokio::test]
    async fn test_get_transport_paths_empty() {
        let state = make_state_with_transport().await;
        let result = get_transport_paths(&state).await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_get_failover_history_empty() {
        let state = make_state_with_transport().await;
        let result = get_failover_history(&state, Some(10)).await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }
}
