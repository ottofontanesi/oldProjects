// Intent citation: .kiro/specs/rl-policy-inference/design.md — StateEncoder
// Encodes network state into a fixed-size feature vector for RL inference.
// Full implementation in Task 2.

use crate::integration::rl_config::RlConfig;
use std::collections::HashMap;

/// Raw network state collected for encoding into the RL feature vector.
#[derive(Debug, Clone)]
pub struct RlNetworkState {
    pub nodes: Vec<RlNodeFeatures>,
    pub demand_weights: HashMap<String, f64>,
    pub model_availability: HashMap<String, bool>,
    pub avg_latency_ms: f64,
    pub node_count: u32,
    pub hour_of_day: u8,
    pub day_of_week: u8,
}

/// Per-node features for RL encoding.
#[derive(Debug, Clone)]
pub struct RlNodeFeatures {
    pub cpu_utilization: f64,
    pub ram_utilization: f64,
    pub vram_utilization: f64,
    pub queue_depth: u32,
    pub stability_score: f64,
    pub is_online: bool,
}

/// Encodes network state into a fixed-size feature vector.
pub struct StateEncoder {
    config: RlConfig,
}

impl StateEncoder {
    pub fn new(config: RlConfig) -> Self {
        Self { config }
    }

    /// Encode the current network state into a fixed-size feature vector.
    /// Handles variable node counts by aggregating per-feature.
    pub fn encode(&self, state: &RlNetworkState) -> Vec<f32> {
        let mut features = vec![0.0f32; self.config.feature_vector_size];

        // Indices 0-23: Node utilization stats (CPU, RAM, VRAM × 8 stats each)
        self.encode_node_utilization(state, &mut features);

        // Indices 24-27: Queue depth stats
        self.encode_queue_depth(state, &mut features);

        // Indices 28-31: Stability scores
        self.encode_stability(state, &mut features);

        // Indices 32-39: Demand weights (top-8 task types)
        self.encode_demand_weights(state, &mut features);

        // Indices 40-47: Model availability flags (top-8 models)
        self.encode_model_availability(state, &mut features);

        // Indices 48-51: Network stats
        self.encode_network_stats(state, &mut features);

        // Indices 52-55: Time encoding (sin/cos)
        self.encode_time(state, &mut features);

        // Indices 56-63: Reserved (zeros) — already zero

        // Final clamp: ensure all values in [0.0, 1.0]
        for f in features.iter_mut() {
            *f = f.clamp(0.0, 1.0);
        }

        features
    }

    fn encode_node_utilization(&self, state: &RlNetworkState, features: &mut [f32]) {
        if state.nodes.is_empty() {
            // Default 0.5 for missing data
            for i in 0..24 {
                features[i] = 0.5;
            }
            return;
        }

        let cpu: Vec<f64> = state.nodes.iter().map(|n| n.cpu_utilization).collect();
        let ram: Vec<f64> = state.nodes.iter().map(|n| n.ram_utilization).collect();
        let vram: Vec<f64> = state.nodes.iter().map(|n| n.vram_utilization).collect();

        self.write_stats(&cpu, &mut features[0..8]);
        self.write_stats(&ram, &mut features[8..16]);
        self.write_stats(&vram, &mut features[16..24]);
    }

    fn encode_queue_depth(&self, state: &RlNetworkState, features: &mut [f32]) {
        if state.nodes.is_empty() {
            for i in 24..28 {
                features[i] = 0.5;
            }
            return;
        }

        let depths: Vec<f64> = state.nodes.iter().map(|n| n.queue_depth as f64).collect();
        let stats = self.compute_stats(&depths);
        // Normalize by dividing by 20, cap at 1.0
        features[24] = (stats.0 / 20.0).min(1.0) as f32;
        features[25] = (stats.1 / 20.0).min(1.0) as f32;
        features[26] = (stats.2 / 20.0).min(1.0) as f32;
        features[27] = (stats.3 / 20.0).min(1.0) as f32;
    }

    fn encode_stability(&self, state: &RlNetworkState, features: &mut [f32]) {
        if state.nodes.is_empty() {
            for i in 28..32 {
                features[i] = 0.5;
            }
            return;
        }

        let scores: Vec<f64> = state.nodes.iter().map(|n| n.stability_score).collect();
        let stats = self.compute_stats(&scores);
        features[28] = stats.0 as f32;
        features[29] = stats.1 as f32;
        features[30] = stats.2 as f32;
        features[31] = stats.3 as f32;
    }

    fn encode_demand_weights(&self, state: &RlNetworkState, features: &mut [f32]) {
        let mut weights: Vec<f64> = state.demand_weights.values().copied().collect();
        weights.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));

        let max_demand = weights.first().copied().unwrap_or(1.0).max(1.0);

        for (i, &w) in weights.iter().take(8).enumerate() {
            features[32 + i] = (w / max_demand) as f32;
        }
    }

    fn encode_model_availability(&self, state: &RlNetworkState, features: &mut [f32]) {
        let mut avail: Vec<bool> = state.model_availability.values().copied().collect();
        avail.truncate(8);

        for (i, &available) in avail.iter().enumerate() {
            features[40 + i] = if available { 1.0 } else { 0.0 };
        }
    }

    fn encode_network_stats(&self, state: &RlNetworkState, features: &mut [f32]) {
        // avg_latency / 100 (cap at 1.0)
        features[48] = (state.avg_latency_ms / 100.0).min(1.0) as f32;
        // node_count / 50 (cap at 1.0)
        features[49] = (state.node_count as f64 / 50.0).min(1.0) as f32;
        // online ratio
        if !state.nodes.is_empty() {
            let online = state.nodes.iter().filter(|n| n.is_online).count() as f64;
            features[50] = (online / state.nodes.len() as f64) as f32;
            // utilization ratio (average of all utilizations)
            let avg_util: f64 = state
                .nodes
                .iter()
                .map(|n| (n.cpu_utilization + n.ram_utilization + n.vram_utilization) / 3.0)
                .sum::<f64>()
                / state.nodes.len() as f64;
            features[51] = avg_util as f32;
        } else {
            features[50] = 0.5;
            features[51] = 0.5;
        }
    }

    fn encode_time(&self, state: &RlNetworkState, features: &mut [f32]) {
        let hour_rad = 2.0 * std::f64::consts::PI * state.hour_of_day as f64 / 24.0;
        let day_rad = 2.0 * std::f64::consts::PI * state.day_of_week as f64 / 7.0;

        // Map sin/cos from [-1,1] to [0,1]
        features[52] = ((hour_rad.sin() + 1.0) / 2.0) as f32;
        features[53] = ((hour_rad.cos() + 1.0) / 2.0) as f32;
        features[54] = ((day_rad.sin() + 1.0) / 2.0) as f32;
        features[55] = ((day_rad.cos() + 1.0) / 2.0) as f32;
    }

    /// Compute 8 statistics: mean, max, min, std, p25, p50, p75, p90
    fn write_stats(&self, values: &[f64], out: &mut [f32]) {
        let stats = self.compute_stats(values);
        let percentiles = self.compute_percentiles(values);
        out[0] = stats.0 as f32; // mean
        out[1] = stats.1 as f32; // max
        out[2] = stats.2 as f32; // min
        out[3] = stats.3 as f32; // std
        out[4] = percentiles.0 as f32; // p25
        out[5] = percentiles.1 as f32; // p50
        out[6] = percentiles.2 as f32; // p75
        out[7] = percentiles.3 as f32; // p90
    }

    /// Returns (mean, max, min, std)
    fn compute_stats(&self, values: &[f64]) -> (f64, f64, f64, f64) {
        if values.is_empty() {
            return (0.5, 0.5, 0.5, 0.0);
        }
        let n = values.len() as f64;
        let mean = values.iter().sum::<f64>() / n;
        let max = values.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let min = values.iter().cloned().fold(f64::INFINITY, f64::min);
        let variance = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n;
        let std = variance.sqrt();
        (mean, max, min, std)
    }

    /// Returns (p25, p50, p75, p90)
    fn compute_percentiles(&self, values: &[f64]) -> (f64, f64, f64, f64) {
        if values.is_empty() {
            return (0.5, 0.5, 0.5, 0.5);
        }
        let mut sorted = values.to_vec();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let percentile = |p: f64| -> f64 {
            let idx = (p * (sorted.len() - 1) as f64).round() as usize;
            sorted[idx.min(sorted.len() - 1)]
        };

        (percentile(0.25), percentile(0.50), percentile(0.75), percentile(0.90))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_config() -> RlConfig {
        RlConfig::default()
    }

    fn make_state(node_count: usize) -> RlNetworkState {
        let nodes = (0..node_count)
            .map(|i| RlNodeFeatures {
                cpu_utilization: (i as f64) / (node_count as f64).max(1.0),
                ram_utilization: 0.5,
                vram_utilization: 0.3,
                queue_depth: i as u32,
                stability_score: 0.9,
                is_online: true,
            })
            .collect();

        let mut demand_weights = HashMap::new();
        demand_weights.insert("chat".to_string(), 0.8);
        demand_weights.insert("code".to_string(), 0.6);

        let mut model_availability = HashMap::new();
        model_availability.insert("llama-7b".to_string(), true);
        model_availability.insert("qwen-14b".to_string(), false);

        RlNetworkState {
            nodes,
            demand_weights,
            model_availability,
            avg_latency_ms: 25.0,
            node_count: node_count as u32,
            hour_of_day: 14,
            day_of_week: 3,
        }
    }

    #[test]
    fn test_encode_produces_correct_size() {
        let encoder = StateEncoder::new(make_config());
        let state = make_state(5);
        let features = encoder.encode(&state);
        assert_eq!(features.len(), 64);
    }

    #[test]
    fn test_all_features_in_range() {
        let encoder = StateEncoder::new(make_config());
        let state = make_state(10);
        let features = encoder.encode(&state);
        for (i, &f) in features.iter().enumerate() {
            assert!(
                f >= 0.0 && f <= 1.0,
                "Feature {} out of range: {}",
                i,
                f
            );
        }
    }

    #[test]
    fn test_empty_nodes_uses_defaults() {
        let encoder = StateEncoder::new(make_config());
        let state = make_state(0);
        let features = encoder.encode(&state);
        assert_eq!(features.len(), 64);
        // With no nodes, utilization defaults to 0.5
        assert!((features[0] - 0.5).abs() < f64::EPSILON as f32);
    }

    #[test]
    fn test_time_encoding_boundaries() {
        let encoder = StateEncoder::new(make_config());
        // Test midnight
        let mut state = make_state(1);
        state.hour_of_day = 0;
        state.day_of_week = 0;
        let features = encoder.encode(&state);
        assert!(features[52] >= 0.0 && features[52] <= 1.0);
        assert!(features[53] >= 0.0 && features[53] <= 1.0);
        assert!(features[54] >= 0.0 && features[54] <= 1.0);
        assert!(features[55] >= 0.0 && features[55] <= 1.0);
    }
}

#[cfg(test)]
mod property_tests {
    use super::*;
    use proptest::prelude::*;

    fn arb_node_features() -> impl Strategy<Value = RlNodeFeatures> {
        (0.0..1.0f64, 0.0..1.0f64, 0.0..1.0f64, 0u32..50, 0.0..1.0f64, any::<bool>())
            .prop_map(|(cpu, ram, vram, queue, stability, online)| RlNodeFeatures {
                cpu_utilization: cpu,
                ram_utilization: ram,
                vram_utilization: vram,
                queue_depth: queue,
                stability_score: stability,
                is_online: online,
            })
    }

    fn arb_network_state() -> impl Strategy<Value = RlNetworkState> {
        (
            prop::collection::vec(arb_node_features(), 0..20),
            prop::collection::hash_map("[a-z]{3,8}".prop_map(|s| s), 0.0..2.0f64, 0..10),
            prop::collection::hash_map("[a-z]{3,8}".prop_map(|s| s), any::<bool>(), 0..10),
            0.0..500.0f64,
            0u32..100,
            0u8..24,
            0u8..7,
        )
            .prop_map(|(nodes, demand, avail, latency, count, hour, day)| RlNetworkState {
                nodes,
                demand_weights: demand,
                model_availability: avail,
                avg_latency_ms: latency,
                node_count: count,
                hour_of_day: hour,
                day_of_week: day,
            })
    }

    // Property 1: Feature Vector Normalization — all features in [0.0, 1.0]
    proptest! {
        #[test]
        fn prop_all_features_normalized(state in arb_network_state()) {
            let encoder = StateEncoder::new(RlConfig::default());
            let features = encoder.encode(&state);

            prop_assert_eq!(features.len(), 64);
            for (i, &f) in features.iter().enumerate() {
                prop_assert!(
                    f >= 0.0 && f <= 1.0,
                    "Feature {} out of [0,1]: {}",
                    i, f
                );
            }
        }
    }
}
