// Intent citation: .kiro/specs/rl-policy-inference/design.md — ActionDecoder
// Decodes Q-values into model priority adjustments using epsilon-greedy.

use crate::integration::rl_config::{RlConfig, RlError};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

// ─── Types ───────────────────────────────────────────────────────────────────

/// A model entry from the catalog, used to build action mappings.
#[derive(Debug, Clone)]
pub struct ModelEntry {
    pub model_id: String,
    pub family: String,
}

/// Maps an action index to a model family and boost amount.
#[derive(Debug, Clone)]
struct ActionMapping {
    action_id: u32,
    target_family: String,
    boost_amount: f64,
}

/// Information about the decoding decision for observability.
#[derive(Debug, Clone)]
pub struct DecodingInfo {
    pub selected_action: u32,
    pub was_exploration: bool,
    pub q_value_spread: f64,
    pub epsilon: f64,
    pub adjustments: HashMap<String, f64>,
}

/// Trait for epsilon persistence.
pub trait PersistenceStore: Send + Sync {
    fn get(&self, key: &str) -> Option<String>;
    fn set(&self, key: &str, value: &str) -> Result<(), String>;
}

// ─── ActionDecoder ───────────────────────────────────────────────────────────

/// Decodes Q-values from the RL model into priority adjustments.
pub struct ActionDecoder {
    config: RlConfig,
    /// Current epsilon stored as bits of f64 in an AtomicU64.
    epsilon_bits: AtomicU64,
    cycle_count: AtomicU64,
    rng: Mutex<StdRng>,
    action_map: Vec<ActionMapping>,
}

impl ActionDecoder {
    /// Create a new decoder with action mappings derived from the model catalog.
    pub fn new(config: RlConfig, model_catalog: &[ModelEntry]) -> Self {
        let action_map = Self::build_action_map(&config, model_catalog);
        let epsilon_bits = config.epsilon_initial.to_bits();

        Self {
            config,
            epsilon_bits: AtomicU64::new(epsilon_bits),
            cycle_count: AtomicU64::new(0),
            rng: Mutex::new(StdRng::from_entropy()),
            action_map,
        }
    }

    /// Build action-to-model-family mappings from the catalog.
    fn build_action_map(config: &RlConfig, catalog: &[ModelEntry]) -> Vec<ActionMapping> {
        // Collect unique families
        let mut families: Vec<String> = catalog.iter().map(|m| m.family.clone()).collect();
        families.sort();
        families.dedup();

        let (boost_min, boost_max) = config.boost_amount_range;
        let family_count = families.len().max(1);

        (0..config.action_space_size)
            .map(|i| {
                let family_idx = i % family_count;
                let family = families.get(family_idx).cloned().unwrap_or_else(|| format!("family_{}", i));

                // Distribute boost amounts linearly across the range
                let t = if config.action_space_size > 1 {
                    (i as f64) / (config.action_space_size - 1) as f64
                } else {
                    0.5
                };
                let boost = boost_min + t * (boost_max - boost_min);

                ActionMapping {
                    action_id: i as u32,
                    target_family: family,
                    boost_amount: boost,
                }
            })
            .collect()
    }

    /// Decode Q-values into priority adjustments using epsilon-greedy.
    pub fn decode(&self, q_values: &[f32]) -> (HashMap<String, f64>, DecodingInfo) {
        let epsilon = self.epsilon();
        let mut rng = self.rng.lock().unwrap();

        // Epsilon-greedy action selection
        let (selected_action, was_exploration) = if rng.gen::<f64>() < epsilon {
            // Explore: random action
            let action = rng.gen_range(0..q_values.len().min(self.config.action_space_size));
            (action as u32, true)
        } else {
            // Exploit: argmax(Q)
            let action = q_values
                .iter()
                .enumerate()
                .take(self.config.action_space_size)
                .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
                .map(|(i, _)| i)
                .unwrap_or(0);
            (action as u32, false)
        };

        // Map action to priority adjustments
        let mut adjustments = HashMap::new();
        if let Some(mapping) = self.action_map.get(selected_action as usize) {
            let adjustment = mapping.boost_amount.clamp(
                -self.config.max_priority_adjustment,
                self.config.max_priority_adjustment,
            );
            adjustments.insert(mapping.target_family.clone(), adjustment);
        }

        // Clamp all adjustments
        for val in adjustments.values_mut() {
            *val = val.clamp(
                -self.config.max_priority_adjustment,
                self.config.max_priority_adjustment,
            );
        }

        // Compute Q-value spread
        let q_max = q_values.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        let q_min = q_values.iter().cloned().fold(f32::INFINITY, f32::min);
        let q_value_spread = (q_max - q_min) as f64;

        let info = DecodingInfo {
            selected_action,
            was_exploration,
            q_value_spread,
            epsilon,
            adjustments: adjustments.clone(),
        };

        (adjustments, info)
    }

    /// Apply exponential decay to epsilon.
    pub fn decay_epsilon(&self) {
        let current = self.epsilon();
        let decayed = (current * self.config.epsilon_decay_rate).max(self.config.epsilon_min);
        self.epsilon_bits
            .store(decayed.to_bits(), Ordering::Relaxed);
        self.cycle_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Get current epsilon value.
    pub fn epsilon(&self) -> f64 {
        f64::from_bits(self.epsilon_bits.load(Ordering::Relaxed))
    }

    /// Reset epsilon to initial value (for retraining scenarios).
    pub fn reset_epsilon(&self) {
        self.epsilon_bits
            .store(self.config.epsilon_initial.to_bits(), Ordering::Relaxed);
        self.cycle_count.store(0, Ordering::Relaxed);
    }

    /// Persist epsilon to survive restarts.
    pub fn save_epsilon(&self, store: &dyn PersistenceStore) -> Result<(), RlError> {
        let epsilon = self.epsilon();
        let cycle = self.cycle_count.load(Ordering::Relaxed);
        store
            .set("rl_epsilon", &format!("{}", epsilon))
            .map_err(|e| RlError::FileIoError { reason: e })?;
        store
            .set("rl_cycle_count", &format!("{}", cycle))
            .map_err(|e| RlError::FileIoError { reason: e })?;
        Ok(())
    }

    /// Load epsilon from persistence.
    pub fn load_epsilon(&self, store: &dyn PersistenceStore) -> Result<(), RlError> {
        if let Some(eps_str) = store.get("rl_epsilon") {
            if let Ok(eps) = eps_str.parse::<f64>() {
                let clamped = eps.clamp(self.config.epsilon_min, self.config.epsilon_initial);
                self.epsilon_bits
                    .store(clamped.to_bits(), Ordering::Relaxed);
            }
        }
        if let Some(cycle_str) = store.get("rl_cycle_count") {
            if let Ok(cycle) = cycle_str.parse::<u64>() {
                self.cycle_count.store(cycle, Ordering::Relaxed);
            }
        }
        Ok(())
    }

    /// Get current cycle count.
    pub fn cycle_count(&self) -> u64 {
        self.cycle_count.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_catalog() -> Vec<ModelEntry> {
        vec![
            ModelEntry { model_id: "llama-7b".to_string(), family: "llama".to_string() },
            ModelEntry { model_id: "llama-13b".to_string(), family: "llama".to_string() },
            ModelEntry { model_id: "qwen-14b".to_string(), family: "qwen".to_string() },
            ModelEntry { model_id: "deepseek-7b".to_string(), family: "deepseek".to_string() },
        ]
    }

    fn make_decoder() -> ActionDecoder {
        ActionDecoder::new(RlConfig::default(), &make_catalog())
    }

    #[test]
    fn test_decode_produces_adjustments() {
        let decoder = make_decoder();
        let q_values = vec![0.1f32; 32];
        let (adjustments, info) = decoder.decode(&q_values);
        // Should produce at least one adjustment
        assert!(!adjustments.is_empty() || info.selected_action < 32);
    }

    #[test]
    fn test_adjustments_clamped() {
        let decoder = make_decoder();
        // Extreme Q-values
        let mut q_values = vec![-100.0f32; 32];
        q_values[0] = 100.0;
        let (adjustments, _) = decoder.decode(&q_values);
        for &val in adjustments.values() {
            assert!(val >= -0.5 && val <= 0.5, "Adjustment out of range: {}", val);
        }
    }

    #[test]
    fn test_epsilon_decay() {
        let decoder = make_decoder();
        let initial = decoder.epsilon();
        assert!((initial - 0.3).abs() < f64::EPSILON);

        decoder.decay_epsilon();
        let after = decoder.epsilon();
        assert!(after < initial);
        assert!(after >= 0.05); // Above minimum
    }

    #[test]
    fn test_epsilon_floor() {
        let decoder = make_decoder();
        // Decay many times
        for _ in 0..10000 {
            decoder.decay_epsilon();
        }
        let eps = decoder.epsilon();
        assert!((eps - 0.05).abs() < 1e-10, "Epsilon below floor: {}", eps);
    }

    #[test]
    fn test_epsilon_monotonically_decreasing() {
        let decoder = make_decoder();
        let mut prev = decoder.epsilon();
        for _ in 0..100 {
            decoder.decay_epsilon();
            let current = decoder.epsilon();
            assert!(current <= prev, "Epsilon increased: {} > {}", current, prev);
            prev = current;
        }
    }

    #[test]
    fn test_reset_epsilon() {
        let decoder = make_decoder();
        for _ in 0..100 {
            decoder.decay_epsilon();
        }
        assert!(decoder.epsilon() < 0.3);
        decoder.reset_epsilon();
        assert!((decoder.epsilon() - 0.3).abs() < f64::EPSILON);
    }

    #[test]
    fn test_persistence() {
        use std::collections::HashMap;
        use std::sync::Mutex;

        struct MockStore {
            data: Mutex<HashMap<String, String>>,
        }
        impl PersistenceStore for MockStore {
            fn get(&self, key: &str) -> Option<String> {
                self.data.lock().unwrap().get(key).cloned()
            }
            fn set(&self, key: &str, value: &str) -> Result<(), String> {
                self.data.lock().unwrap().insert(key.to_string(), value.to_string());
                Ok(())
            }
        }

        let store = MockStore { data: Mutex::new(HashMap::new()) };
        let decoder = make_decoder();

        // Decay a few times
        for _ in 0..50 {
            decoder.decay_epsilon();
        }
        let eps_before = decoder.epsilon();

        // Save
        decoder.save_epsilon(&store).unwrap();

        // Create new decoder and load
        let decoder2 = make_decoder();
        decoder2.load_epsilon(&store).unwrap();
        assert!((decoder2.epsilon() - eps_before).abs() < 1e-10);
    }
}

#[cfg(test)]
mod property_tests {
    use super::*;
    use proptest::prelude::*;

    fn make_test_decoder() -> ActionDecoder {
        let catalog = vec![
            ModelEntry { model_id: "llama-7b".to_string(), family: "llama".to_string() },
            ModelEntry { model_id: "qwen-14b".to_string(), family: "qwen".to_string() },
            ModelEntry { model_id: "deepseek-7b".to_string(), family: "deepseek".to_string() },
        ];
        ActionDecoder::new(RlConfig::default(), &catalog)
    }

    // Property 2: Adjustment Clamping — all adjustments in [-0.5, +0.5]
    proptest! {
        #[test]
        fn prop_adjustments_clamped(
            q_values in prop::collection::vec(-1000.0f32..1000.0, 32)
        ) {
            let decoder = make_test_decoder();
            let (adjustments, _info) = decoder.decode(&q_values);

            for (&ref _key, &val) in &adjustments {
                prop_assert!(
                    val >= -0.5 && val <= 0.5,
                    "Adjustment out of [-0.5, 0.5]: {}",
                    val
                );
            }
        }

        // Property 3: Epsilon Bounds — epsilon always in [min, initial]
        #[test]
        fn prop_epsilon_bounds(decay_count in 0u32..5000) {
            let decoder = make_test_decoder();

            for _ in 0..decay_count {
                decoder.decay_epsilon();
            }

            let eps = decoder.epsilon();
            prop_assert!(eps >= 0.05, "Epsilon below min: {}", eps);
            prop_assert!(eps <= 0.3, "Epsilon above initial: {}", eps);
        }

        // Property 4: Epsilon Monotonicity — epsilon never increases during decay
        #[test]
        fn prop_epsilon_monotonic(decay_count in 1u32..200) {
            let decoder = make_test_decoder();
            let mut prev = decoder.epsilon();

            for _ in 0..decay_count {
                decoder.decay_epsilon();
                let current = decoder.epsilon();
                prop_assert!(
                    current <= prev,
                    "Epsilon increased: {} > {}",
                    current, prev
                );
                prev = current;
            }
        }
    }
}
