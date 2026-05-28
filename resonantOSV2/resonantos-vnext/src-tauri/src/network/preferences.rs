// Intent citation: .kiro/specs/local-network-optimizer/design.md Section 2.7
// User Preferences — preference store, veto enforcement, weight overrides

use super::catalog::{ModelId, TaskType};
use super::solver::UtilityWeights;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// User preferences that influence the optimizer's decisions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserPreferences {
    /// Custom utility weights (None = use defaults).
    pub utility_weights: Option<UtilityWeights>,
    /// Model family preferences (family name → weight boost, e.g., 1.2 = 20% boost).
    pub model_family_preferences: Vec<FamilyPreference>,
    /// Models the user never wants loaded (hard exclusion).
    pub model_vetoes: Vec<ModelId>,
    /// Force specific model for specific task type (hard constraint).
    pub task_model_overrides: HashMap<TaskType, ModelId>,
    /// Whether phone can use cellular data for inference.
    pub phone_cellular_opt_in: bool,
    /// Whether speculative prefetch is enabled.
    pub prefetch_enabled: bool,
    /// Whether user satisfaction tracking is enabled.
    pub satisfaction_tracking_enabled: bool,
    /// Exploration budget percentage (0.0 - 0.20).
    pub exploration_budget_percent: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FamilyPreference {
    pub family: String,
    /// Weight boost multiplier (e.g., 1.2 = 20% boost in selection score).
    pub weight_boost: f64,
}

impl Default for UserPreferences {
    fn default() -> Self {
        Self {
            utility_weights: None,
            model_family_preferences: Vec::new(),
            model_vetoes: Vec::new(),
            task_model_overrides: HashMap::new(),
            phone_cellular_opt_in: false,
            prefetch_enabled: true,
            satisfaction_tracking_enabled: true,
            exploration_budget_percent: 0.10,
        }
    }
}

impl UserPreferences {
    /// Get effective utility weights (user override or defaults).
    pub fn effective_weights(&self) -> UtilityWeights {
        self.utility_weights.clone().unwrap_or_default()
    }

    /// Get the weight boost for a model family (1.0 = no boost).
    pub fn family_boost(&self, family: &str) -> f64 {
        self.model_family_preferences
            .iter()
            .find(|p| p.family == family)
            .map(|p| p.weight_boost)
            .unwrap_or(1.0)
    }

    /// Check if a model is vetoed.
    pub fn is_vetoed(&self, model_id: &str) -> bool {
        self.model_vetoes.iter().any(|v| v == model_id)
    }

    /// Check if a task has a model override.
    pub fn get_override(&self, task_type: &TaskType) -> Option<&ModelId> {
        self.task_model_overrides.get(task_type)
    }

    /// Add a model veto.
    pub fn add_veto(&mut self, model_id: ModelId) {
        if !self.model_vetoes.contains(&model_id) {
            self.model_vetoes.push(model_id);
        }
    }

    /// Remove a model veto.
    pub fn remove_veto(&mut self, model_id: &str) {
        self.model_vetoes.retain(|v| v != model_id);
    }

    /// Set a family preference.
    pub fn set_family_preference(&mut self, family: String, weight_boost: f64) {
        // Clamp boost to reasonable range
        let boost = weight_boost.clamp(0.5, 3.0);
        self.model_family_preferences.retain(|p| p.family != family);
        self.model_family_preferences.push(FamilyPreference {
            family,
            weight_boost: boost,
        });
    }

    /// Remove a family preference.
    pub fn remove_family_preference(&mut self, family: &str) {
        self.model_family_preferences.retain(|p| p.family != family);
    }

    /// Set a task-model override.
    pub fn set_task_override(&mut self, task_type: TaskType, model_id: ModelId) {
        self.task_model_overrides.insert(task_type, model_id);
    }

    /// Remove a task-model override.
    pub fn remove_task_override(&mut self, task_type: &TaskType) {
        self.task_model_overrides.remove(task_type);
    }

    /// Set utility weights (normalizes to sum to 1.0).
    pub fn set_weights(&mut self, quality: f64, speed: f64, mass: f64) {
        let sum = quality + speed + mass;
        if sum <= 0.0 {
            self.utility_weights = None; // Reset to defaults
            return;
        }
        self.utility_weights = Some(UtilityWeights {
            w_quality: quality / sum,
            w_speed: speed / sum,
            w_mass: mass / sum,
        });
    }

    /// Validate preferences (check for inconsistencies).
    pub fn validate(&self) -> Result<(), String> {
        // Check weights sum to ~1.0 if set
        if let Some(ref w) = self.utility_weights {
            let sum = w.w_quality + w.w_speed + w.w_mass;
            if (sum - 1.0).abs() > 0.01 {
                return Err(format!("Utility weights must sum to 1.0, got {:.3}", sum));
            }
        }

        // Check exploration budget in range
        if self.exploration_budget_percent < 0.0 || self.exploration_budget_percent > 0.20 {
            return Err(format!(
                "Exploration budget must be 0-20%, got {:.1}%",
                self.exploration_budget_percent * 100.0
            ));
        }

        // Check family boosts in range
        for pref in &self.model_family_preferences {
            if pref.weight_boost < 0.5 || pref.weight_boost > 3.0 {
                return Err(format!(
                    "Family boost for '{}' must be 0.5-3.0, got {:.2}",
                    pref.family, pref.weight_boost
                ));
            }
        }

        Ok(())
    }

    /// Convert to SolverPreferences (used by the optimizer).
    pub fn to_solver_preferences(&self) -> super::solver::SolverPreferences {
        super::solver::SolverPreferences {
            weights: self.effective_weights(),
            model_vetoes: self.model_vetoes.clone(),
            task_model_overrides: self.task_model_overrides.clone(),
            family_boosts: self
                .model_family_preferences
                .iter()
                .map(|p| (p.family.clone(), p.weight_boost))
                .collect(),
            exploration_budget_percent: self.exploration_budget_percent,
        }
    }
}

/// Explanation for when a user's preference is overridden by the optimizer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreferenceOverrideExplanation {
    pub preference_type: String,
    pub requested: String,
    pub actual: String,
    pub reason: String,
}

/// Generate explanation when a preferred model family isn't available.
pub fn explain_override(
    preferred_family: &str,
    actual_model: &str,
    reason: &str,
) -> PreferenceOverrideExplanation {
    PreferenceOverrideExplanation {
        preference_type: "model_family".to_string(),
        requested: preferred_family.to_string(),
        actual: actual_model.to_string(),
        reason: reason.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_preferences() {
        let prefs = UserPreferences::default();
        assert!(prefs.model_vetoes.is_empty());
        assert!(prefs.prefetch_enabled);
        assert!(prefs.satisfaction_tracking_enabled);
        assert_eq!(prefs.exploration_budget_percent, 0.10);
        assert!(!prefs.phone_cellular_opt_in);
    }

    #[test]
    fn test_veto_management() {
        let mut prefs = UserPreferences::default();

        prefs.add_veto("bad_model".to_string());
        assert!(prefs.is_vetoed("bad_model"));
        assert!(!prefs.is_vetoed("good_model"));

        // No duplicates
        prefs.add_veto("bad_model".to_string());
        assert_eq!(prefs.model_vetoes.len(), 1);

        prefs.remove_veto("bad_model");
        assert!(!prefs.is_vetoed("bad_model"));
    }

    #[test]
    fn test_family_preference() {
        let mut prefs = UserPreferences::default();

        prefs.set_family_preference("gemma".to_string(), 1.3);
        assert_eq!(prefs.family_boost("gemma"), 1.3);
        assert_eq!(prefs.family_boost("qwen"), 1.0); // No preference = 1.0

        // Update existing
        prefs.set_family_preference("gemma".to_string(), 1.5);
        assert_eq!(prefs.family_boost("gemma"), 1.5);
        assert_eq!(prefs.model_family_preferences.len(), 1); // No duplicate
    }

    #[test]
    fn test_family_boost_clamped() {
        let mut prefs = UserPreferences::default();

        prefs.set_family_preference("test".to_string(), 10.0); // Above max
        assert_eq!(prefs.family_boost("test"), 3.0); // Clamped to 3.0

        prefs.set_family_preference("test".to_string(), 0.1); // Below min
        assert_eq!(prefs.family_boost("test"), 0.5); // Clamped to 0.5
    }

    #[test]
    fn test_set_weights_normalizes() {
        let mut prefs = UserPreferences::default();

        prefs.set_weights(2.0, 2.0, 1.0); // Sum = 5, should normalize
        let w = prefs.effective_weights();
        assert!((w.w_quality - 0.4).abs() < 0.001);
        assert!((w.w_speed - 0.4).abs() < 0.001);
        assert!((w.w_mass - 0.2).abs() < 0.001);
    }

    #[test]
    fn test_set_weights_zero_resets() {
        let mut prefs = UserPreferences::default();
        prefs.set_weights(0.5, 0.3, 0.2);
        assert!(prefs.utility_weights.is_some());

        prefs.set_weights(0.0, 0.0, 0.0); // All zero = reset
        assert!(prefs.utility_weights.is_none());
    }

    #[test]
    fn test_task_override() {
        let mut prefs = UserPreferences::default();

        prefs.set_task_override(TaskType::Code, "codellama:7b".to_string());
        assert_eq!(prefs.get_override(&TaskType::Code), Some(&"codellama:7b".to_string()));
        assert_eq!(prefs.get_override(&TaskType::Chat), None);

        prefs.remove_task_override(&TaskType::Code);
        assert_eq!(prefs.get_override(&TaskType::Code), None);
    }

    #[test]
    fn test_validate_valid() {
        let prefs = UserPreferences::default();
        assert!(prefs.validate().is_ok());
    }

    #[test]
    fn test_validate_bad_weights() {
        let mut prefs = UserPreferences::default();
        prefs.utility_weights = Some(UtilityWeights {
            w_quality: 0.5,
            w_speed: 0.5,
            w_mass: 0.5, // Sum = 1.5, not 1.0
        });
        assert!(prefs.validate().is_err());
    }

    #[test]
    fn test_validate_bad_exploration_budget() {
        let mut prefs = UserPreferences::default();
        prefs.exploration_budget_percent = 0.50; // 50% — too high
        assert!(prefs.validate().is_err());
    }

    #[test]
    fn test_to_solver_preferences() {
        let mut prefs = UserPreferences::default();
        prefs.add_veto("bad".to_string());
        prefs.set_family_preference("gemma".to_string(), 1.2);
        prefs.set_task_override(TaskType::Code, "codellama".to_string());

        let solver_prefs = prefs.to_solver_preferences();
        assert!(solver_prefs.model_vetoes.contains(&"bad".to_string()));
        assert_eq!(solver_prefs.family_boosts.get("gemma"), Some(&1.2));
        assert_eq!(solver_prefs.task_model_overrides.get(&TaskType::Code), Some(&"codellama".to_string()));
    }
}
