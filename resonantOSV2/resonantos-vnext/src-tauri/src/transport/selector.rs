// Intent citation: .kiro/specs/unified-mesh-transport/design.md Section 3.1
// Path Selector — request-aware path selection algorithm

use super::registry::{PathStatus, TransportPath, UnifiedTopology};
use super::trait_def::{NodeId, RequestType, TransportId};
use serde::{Deserialize, Serialize};

/// Criteria for selecting a path.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathCriteria {
    pub request_type: RequestType,
    pub min_bandwidth_mbps: Option<f64>,
    pub max_latency_ms: Option<f64>,
    pub min_reliability: Option<f64>,
    pub preferred_transport: Option<TransportId>,
    pub message_size_bytes: u64,
}

/// Result of path selection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathSelection {
    pub selected_path: TransportPath,
    pub reason: String,
    pub alternatives: Vec<TransportPath>,
    pub selection_time_us: u64,
}

/// Score a path based on request type.
/// Returns a score in [0.0, 1.0] — higher is better.
pub fn score_path(path: &TransportPath, request_type: &RequestType) -> f64 {
    let m = &path.metrics;

    match request_type {
        RequestType::InferenceActivation => {
            // Lowest latency is king
            let latency_score = 1.0 / (1.0 + m.latency_ms / 5.0);
            let reliability_score = m.reliability;
            latency_score * 0.7 + reliability_score * 0.3
        }
        RequestType::InferenceRequest | RequestType::InferenceResponse => {
            let latency_score = 1.0 / (1.0 + m.latency_ms / 50.0);
            let bandwidth_score = (m.bandwidth_mbps / 100.0).min(1.0);
            let reliability_score = m.reliability;
            latency_score * 0.5 + bandwidth_score * 0.2 + reliability_score * 0.3
        }
        RequestType::ModelTransfer | RequestType::KvCacheData => {
            // Highest bandwidth is king
            let bandwidth_score = (m.bandwidth_mbps / 1000.0).min(1.0);
            let reliability_score = m.reliability;
            let latency_score = 1.0 / (1.0 + m.latency_ms / 200.0);
            bandwidth_score * 0.6 + reliability_score * 0.3 + latency_score * 0.1
        }
        RequestType::Heartbeat | RequestType::MetricProbe | RequestType::Announcement => {
            // Cheapest path (least resource usage)
            let cheapness = 1.0 / (1.0 + m.bandwidth_mbps / 10.0);
            let reliability_score = m.reliability;
            cheapness * 0.4 + reliability_score * 0.6
        }
        RequestType::AgentStepDispatch | RequestType::AgentStepResult => {
            // Similar to inference request
            let latency_score = 1.0 / (1.0 + m.latency_ms / 50.0);
            let reliability_score = m.reliability;
            latency_score * 0.5 + reliability_score * 0.5
        }
        RequestType::AgentStepData => {
            // Similar to model transfer (large data)
            let bandwidth_score = (m.bandwidth_mbps / 1000.0).min(1.0);
            let reliability_score = m.reliability;
            bandwidth_score * 0.5 + reliability_score * 0.5
        }
    }
}

/// Select the best path to a target node given criteria.
pub fn select_path(
    target: &NodeId,
    criteria: &PathCriteria,
    topology: &UnifiedTopology,
) -> Result<PathSelection, String> {
    let start = std::time::Instant::now();

    // Get all active paths to target
    let all_paths: Vec<&TransportPath> = topology
        .paths
        .iter()
        .filter(|p| p.destination == *target && p.status == PathStatus::Active)
        .collect();

    if all_paths.is_empty() {
        return Err(format!("No active paths to node {}", target));
    }

    // Filter by hard constraints
    let feasible: Vec<&TransportPath> = all_paths
        .iter()
        .filter(|p| {
            if let Some(min_bw) = criteria.min_bandwidth_mbps {
                if p.metrics.bandwidth_mbps < min_bw {
                    return false;
                }
            }
            if let Some(max_lat) = criteria.max_latency_ms {
                if p.metrics.latency_ms > max_lat {
                    return false;
                }
            }
            if let Some(min_rel) = criteria.min_reliability {
                if p.metrics.reliability < min_rel {
                    return false;
                }
            }
            true
        })
        .copied()
        .collect();

    // If no feasible paths, relax constraints and use all active paths
    let candidates = if feasible.is_empty() { &all_paths } else { &feasible };

    // Check for transport pinning
    if let Some(ref preferred) = criteria.preferred_transport {
        if let Some(pinned) = candidates.iter().find(|p| p.transport_id == *preferred) {
            let elapsed = start.elapsed().as_micros() as u64;
            let alternatives: Vec<TransportPath> = candidates
                .iter()
                .filter(|p| p.path_id != pinned.path_id)
                .map(|p| (*p).clone())
                .collect();

            return Ok(PathSelection {
                selected_path: (*pinned).clone(),
                reason: format!("Pinned to transport: {}", preferred),
                alternatives,
                selection_time_us: elapsed,
            });
        }
    }

    // Score all candidates
    let mut scored: Vec<(&TransportPath, f64)> = candidates
        .iter()
        .map(|p| (*p, score_path(p, &criteria.request_type)))
        .collect();

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    let elapsed = start.elapsed().as_micros() as u64;

    let winner = scored[0].0;
    let alternatives: Vec<TransportPath> = scored[1..]
        .iter()
        .map(|(p, _)| (*p).clone())
        .collect();

    let reason = format!(
        "Best score {:.3} for {:?}: latency={:.1}ms, bw={:.0}Mbps, rel={:.2}",
        scored[0].1,
        criteria.request_type,
        winner.metrics.latency_ms,
        winner.metrics.bandwidth_mbps,
        winner.metrics.reliability,
    );

    Ok(PathSelection {
        selected_path: winner.clone(),
        reason,
        alternatives,
        selection_time_us: elapsed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::registry::PathMetrics;

    fn make_path(transport: &str, latency: f64, bandwidth: f64, reliability: f64) -> TransportPath {
        TransportPath {
            path_id: uuid::Uuid::new_v4(),
            source: uuid::Uuid::new_v4(),
            destination: uuid::Uuid::new_v4(),
            transport_id: transport.to_string(),
            hops: vec![],
            metrics: PathMetrics {
                latency_ms: latency,
                bandwidth_mbps: bandwidth,
                reliability,
                jitter_ms: 1.0,
                last_measured_ms: 1000,
                measurement_count: 10,
            },
            status: PathStatus::Active,
        }
    }

    #[test]
    fn test_inference_activation_prefers_low_latency() {
        let fast = make_path("lan", 2.0, 1000.0, 0.99);
        let slow = make_path("wireguard", 50.0, 500.0, 0.95);

        let score_fast = score_path(&fast, &RequestType::InferenceActivation);
        let score_slow = score_path(&slow, &RequestType::InferenceActivation);

        assert!(score_fast > score_slow, "Fast path ({}) should score higher than slow ({})", score_fast, score_slow);
    }

    #[test]
    fn test_model_transfer_prefers_high_bandwidth() {
        let high_bw = make_path("lan", 5.0, 10000.0, 0.95);
        let low_bw = make_path("reticulum", 100.0, 1.0, 0.99);

        let score_high = score_path(&high_bw, &RequestType::ModelTransfer);
        let score_low = score_path(&low_bw, &RequestType::ModelTransfer);

        assert!(score_high > score_low);
    }

    #[test]
    fn test_heartbeat_prefers_cheapest() {
        let expensive = make_path("lan", 1.0, 10000.0, 0.99); // High bandwidth = expensive
        let cheap = make_path("reticulum", 100.0, 0.1, 0.90); // Low bandwidth = cheap

        let score_expensive = score_path(&expensive, &RequestType::Heartbeat);
        let score_cheap = score_path(&cheap, &RequestType::Heartbeat);

        assert!(score_cheap > score_expensive, "Cheap ({}) should beat expensive ({})", score_cheap, score_expensive);
    }

    #[test]
    fn test_select_path_basic() {
        let target = uuid::Uuid::new_v4();
        let src = uuid::Uuid::new_v4();

        let mut topology = UnifiedTopology::new();
        topology.paths.push(TransportPath {
            path_id: uuid::Uuid::new_v4(),
            source: src,
            destination: target,
            transport_id: "lan".to_string(),
            hops: vec![],
            metrics: PathMetrics {
                latency_ms: 2.0, bandwidth_mbps: 1000.0, reliability: 0.99,
                jitter_ms: 0.5, last_measured_ms: 1000, measurement_count: 5,
            },
            status: PathStatus::Active,
        });

        let criteria = PathCriteria {
            request_type: RequestType::InferenceRequest,
            min_bandwidth_mbps: None,
            max_latency_ms: None,
            min_reliability: None,
            preferred_transport: None,
            message_size_bytes: 1000,
        };

        let result = select_path(&target, &criteria, &topology);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().selected_path.transport_id, "lan");
    }

    #[test]
    fn test_select_path_no_paths() {
        let target = uuid::Uuid::new_v4();
        let topology = UnifiedTopology::new();

        let criteria = PathCriteria {
            request_type: RequestType::InferenceRequest,
            min_bandwidth_mbps: None,
            max_latency_ms: None,
            min_reliability: None,
            preferred_transport: None,
            message_size_bytes: 1000,
        };

        let result = select_path(&target, &criteria, &topology);
        assert!(result.is_err());
    }

    #[test]
    fn test_select_path_pinning() {
        let target = uuid::Uuid::new_v4();
        let src = uuid::Uuid::new_v4();

        let mut topology = UnifiedTopology::new();
        // LAN is faster
        topology.paths.push(TransportPath {
            path_id: uuid::Uuid::new_v4(), source: src, destination: target,
            transport_id: "lan".to_string(), hops: vec![],
            metrics: PathMetrics { latency_ms: 2.0, bandwidth_mbps: 1000.0, reliability: 0.99, jitter_ms: 0.5, last_measured_ms: 1000, measurement_count: 5 },
            status: PathStatus::Active,
        });
        // WireGuard is slower but pinned
        topology.paths.push(TransportPath {
            path_id: uuid::Uuid::new_v4(), source: src, destination: target,
            transport_id: "wireguard".to_string(), hops: vec![],
            metrics: PathMetrics { latency_ms: 50.0, bandwidth_mbps: 500.0, reliability: 0.95, jitter_ms: 5.0, last_measured_ms: 1000, measurement_count: 5 },
            status: PathStatus::Active,
        });

        let criteria = PathCriteria {
            request_type: RequestType::InferenceRequest,
            min_bandwidth_mbps: None,
            max_latency_ms: None,
            min_reliability: None,
            preferred_transport: Some("wireguard".to_string()), // Pin to wireguard
            message_size_bytes: 1000,
        };

        let result = select_path(&target, &criteria, &topology).unwrap();
        assert_eq!(result.selected_path.transport_id, "wireguard"); // Pinned wins
        assert!(result.reason.contains("Pinned"));
    }

    #[test]
    fn test_selection_completes_fast() {
        let target = uuid::Uuid::new_v4();
        let src = uuid::Uuid::new_v4();

        let mut topology = UnifiedTopology::new();
        // Add 10 paths
        for i in 0..10 {
            topology.paths.push(TransportPath {
                path_id: uuid::Uuid::new_v4(), source: src, destination: target,
                transport_id: format!("transport_{}", i), hops: vec![],
                metrics: PathMetrics { latency_ms: (i + 1) as f64 * 5.0, bandwidth_mbps: 1000.0 / (i + 1) as f64, reliability: 0.9, jitter_ms: 1.0, last_measured_ms: 1000, measurement_count: 5 },
                status: PathStatus::Active,
            });
        }

        let criteria = PathCriteria {
            request_type: RequestType::InferenceActivation,
            min_bandwidth_mbps: None, max_latency_ms: None, min_reliability: None,
            preferred_transport: None, message_size_bytes: 1000,
        };

        let result = select_path(&target, &criteria, &topology).unwrap();
        // Should complete in < 1ms (1000 microseconds)
        assert!(result.selection_time_us < 1000, "Selection took {}us", result.selection_time_us);
    }
}
