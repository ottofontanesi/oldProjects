//! AssignmentManager: validates and executes model assignments.
//!
//! Implements:
//! - `validate_constraints()` — checks memory, battery, cellular, model size limits
//! - `handle_assignment()` — validates constraints and returns accept/reject response
//! - `handle_unload()` — releases model memory and confirms unload
//!
//! The AssignmentManager enforces phone-specific constraints before accepting
//! any model assignment from the Coordinator.

use crate::companion::types::{
    AssignmentResponse, AssignmentType, ConnectionType, ConstraintViolation, ModelAssignment,
    ModelId,
};

// ─── Error Types ─────────────────────────────────────────────────────────────

/// Errors that can occur during assignment operations.
#[derive(Debug, Clone, PartialEq)]
pub enum AssignmentError {
    /// The model is not currently loaded (nothing to unload).
    ModelNotLoaded(String),
    /// An internal error occurred during assignment handling.
    InternalError(String),
}

impl std::fmt::Display for AssignmentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ModelNotLoaded(id) => write!(f, "Model not loaded: {}", id),
            Self::InternalError(msg) => write!(f, "Internal error: {}", msg),
        }
    }
}

impl std::error::Error for AssignmentError {}

// ─── Phone Constraints ───────────────────────────────────────────────────────

/// Current phone constraints used for assignment validation.
#[derive(Debug, Clone)]
pub struct PhoneConstraints {
    /// Available memory in MB.
    pub available_memory_mb: u64,
    /// Current battery percentage (0-100).
    pub battery_percent: u8,
    /// Whether the phone is currently charging.
    pub is_charging: bool,
    /// Current connection type.
    pub connection_type: ConnectionType,
    /// Whether cellular data is allowed for inference.
    pub allow_cellular: bool,
    /// Maximum model size in MB (default: 3072 = 3GB).
    pub max_model_size_mb: u64,
    /// Battery threshold below which assignments are rejected (default: 20%).
    pub battery_threshold: u8,
    /// Maximum full model size in billions of parameters (default: 3.0B).
    pub max_full_model_params_b: f64,
}

impl Default for PhoneConstraints {
    fn default() -> Self {
        Self {
            available_memory_mb: 3072,
            battery_percent: 100,
            is_charging: false,
            connection_type: ConnectionType::WiFi,
            allow_cellular: false,
            max_model_size_mb: 3072,
            battery_threshold: 20,
            max_full_model_params_b: 3.0,
        }
    }
}

// ─── AssignmentManager ───────────────────────────────────────────────────────

/// Manages model assignments from the Coordinator.
///
/// Validates assignments against phone constraints and tracks loaded models.
pub struct AssignmentManager {
    /// Current phone constraints.
    constraints: PhoneConstraints,
    /// Currently loaded model IDs (for unload tracking).
    loaded_models: Vec<ModelId>,
}

impl AssignmentManager {
    /// Create a new AssignmentManager with the given constraints.
    pub fn new(constraints: PhoneConstraints) -> Self {
        Self {
            constraints,
            loaded_models: Vec::new(),
        }
    }

    /// Create a new AssignmentManager with default constraints.
    pub fn with_defaults() -> Self {
        Self::new(PhoneConstraints::default())
    }

    /// Update the current phone constraints (called when state changes).
    pub fn update_constraints(&mut self, constraints: PhoneConstraints) {
        self.constraints = constraints;
    }

    /// Get the current constraints.
    pub fn constraints(&self) -> &PhoneConstraints {
        &self.constraints
    }

    /// Validate a model assignment against current phone constraints.
    ///
    /// Checks:
    /// 1. Memory: weight_size_mb must not exceed available memory
    /// 2. Battery: must be above threshold OR charging
    /// 3. Cellular: if on cellular, allow_cellular must be true
    /// 4. Model size: full models must not exceed max_full_model_params_b
    ///
    /// # Returns
    /// `Ok(())` if all constraints pass, or `Err(ConstraintViolation)` with the reason.
    pub fn validate_constraints(
        &self,
        assignment: &ModelAssignment,
    ) -> Result<(), ConstraintViolation> {
        // Check 1: Memory constraint
        if assignment.weight_size_mb > self.constraints.available_memory_mb {
            return Err(ConstraintViolation::InsufficientMemory {
                required_mb: assignment.weight_size_mb,
                available_mb: self.constraints.available_memory_mb,
            });
        }

        // Also check against max model size limit (3GB hard cap)
        if assignment.weight_size_mb > self.constraints.max_model_size_mb {
            return Err(ConstraintViolation::InsufficientMemory {
                required_mb: assignment.weight_size_mb,
                available_mb: self.constraints.max_model_size_mb,
            });
        }

        // Check 2: Battery constraint (reject if below threshold AND not charging)
        if self.constraints.battery_percent < self.constraints.battery_threshold
            && !self.constraints.is_charging
        {
            return Err(ConstraintViolation::BatteryTooLow {
                current: self.constraints.battery_percent,
                threshold: self.constraints.battery_threshold,
            });
        }

        // Check 3: Cellular constraint
        if self.constraints.connection_type == ConnectionType::Cellular
            && !self.constraints.allow_cellular
        {
            return Err(ConstraintViolation::CellularNotAllowed);
        }

        // Check 4: Model size constraint (only for full models)
        if let AssignmentType::FullModel { params_b } = &assignment.assignment_type {
            if *params_b > self.constraints.max_full_model_params_b {
                return Err(ConstraintViolation::ModelTooLarge {
                    params_b: *params_b,
                    max_b: self.constraints.max_full_model_params_b,
                });
            }
        }

        Ok(())
    }

    /// Handle a model assignment from the Coordinator.
    ///
    /// Validates constraints and returns an accept/reject response.
    /// If accepted, the model ID is tracked for future unload operations.
    pub fn handle_assignment(&mut self, assignment: &ModelAssignment) -> AssignmentResponse {
        match self.validate_constraints(assignment) {
            Ok(()) => {
                // Track the loaded model
                self.loaded_models.push(assignment.model_id.clone());

                // Estimate ready time based on weight size (rough: 10ms per MB for download)
                let estimated_ready_ms = assignment.weight_size_mb * 10;

                AssignmentResponse::Accepted { estimated_ready_ms }
            }
            Err(violation) => AssignmentResponse::Rejected { reason: violation },
        }
    }

    /// Handle an unload command from the Coordinator.
    ///
    /// Removes the model from the loaded models list and confirms the unload.
    pub fn handle_unload(&mut self, model_id: &ModelId) -> Result<(), AssignmentError> {
        let initial_len = self.loaded_models.len();
        self.loaded_models.retain(|id| id != model_id);

        if self.loaded_models.len() == initial_len {
            return Err(AssignmentError::ModelNotLoaded(model_id.clone()));
        }

        Ok(())
    }

    /// Get the list of currently loaded model IDs.
    pub fn loaded_models(&self) -> &[ModelId] {
        &self.loaded_models
    }

    /// Check if a specific model is currently loaded.
    pub fn is_model_loaded(&self, model_id: &ModelId) -> bool {
        self.loaded_models.contains(model_id)
    }
}

// ─── Unit Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::companion::types::AssignmentPriority;
    use uuid::Uuid;

    fn make_full_model_assignment(weight_mb: u64, params_b: f64) -> ModelAssignment {
        ModelAssignment {
            model_id: "test-model".to_string(),
            assignment_type: AssignmentType::FullModel { params_b },
            download_url: "http://example.com/model.gguf".to_string(),
            weight_size_mb: weight_mb,
            priority: AssignmentPriority::Normal,
        }
    }

    fn make_split_assignment(weight_mb: u64) -> ModelAssignment {
        ModelAssignment {
            model_id: "test-split-model".to_string(),
            assignment_type: AssignmentType::SplitLayers {
                layer_range: (0, 15),
                session_id: Uuid::new_v4(),
            },
            download_url: "http://example.com/layers.gguf".to_string(),
            weight_size_mb: weight_mb,
            priority: AssignmentPriority::High,
        }
    }

    // ─── Constraint Validation Tests ─────────────────────────────────────────

    #[test]
    fn test_validate_accepts_valid_assignment() {
        let manager = AssignmentManager::with_defaults();
        let assignment = make_full_model_assignment(2048, 2.0);
        assert!(manager.validate_constraints(&assignment).is_ok());
    }

    #[test]
    fn test_validate_rejects_insufficient_memory() {
        let constraints = PhoneConstraints {
            available_memory_mb: 1024,
            ..Default::default()
        };
        let manager = AssignmentManager::new(constraints);
        let assignment = make_full_model_assignment(2048, 2.0);

        let result = manager.validate_constraints(&assignment);
        assert!(matches!(
            result,
            Err(ConstraintViolation::InsufficientMemory {
                required_mb: 2048,
                available_mb: 1024
            })
        ));
    }

    #[test]
    fn test_validate_rejects_exceeding_max_model_size() {
        let constraints = PhoneConstraints {
            available_memory_mb: 4096, // Plenty of RAM
            max_model_size_mb: 3072,   // But 3GB hard cap
            ..Default::default()
        };
        let manager = AssignmentManager::new(constraints);
        let assignment = make_full_model_assignment(3500, 2.5); // 3.5GB exceeds cap

        let result = manager.validate_constraints(&assignment);
        assert!(matches!(
            result,
            Err(ConstraintViolation::InsufficientMemory { .. })
        ));
    }

    #[test]
    fn test_validate_rejects_low_battery() {
        let constraints = PhoneConstraints {
            battery_percent: 15,
            is_charging: false,
            battery_threshold: 20,
            ..Default::default()
        };
        let manager = AssignmentManager::new(constraints);
        let assignment = make_full_model_assignment(1024, 1.0);

        let result = manager.validate_constraints(&assignment);
        assert!(matches!(
            result,
            Err(ConstraintViolation::BatteryTooLow {
                current: 15,
                threshold: 20
            })
        ));
    }

    #[test]
    fn test_validate_accepts_low_battery_when_charging() {
        let constraints = PhoneConstraints {
            battery_percent: 15,
            is_charging: true, // Charging overrides battery check
            battery_threshold: 20,
            ..Default::default()
        };
        let manager = AssignmentManager::new(constraints);
        let assignment = make_full_model_assignment(1024, 1.0);

        assert!(manager.validate_constraints(&assignment).is_ok());
    }

    #[test]
    fn test_validate_rejects_cellular_when_not_allowed() {
        let constraints = PhoneConstraints {
            connection_type: ConnectionType::Cellular,
            allow_cellular: false,
            ..Default::default()
        };
        let manager = AssignmentManager::new(constraints);
        let assignment = make_full_model_assignment(1024, 1.0);

        let result = manager.validate_constraints(&assignment);
        assert!(matches!(result, Err(ConstraintViolation::CellularNotAllowed)));
    }

    #[test]
    fn test_validate_accepts_cellular_when_allowed() {
        let constraints = PhoneConstraints {
            connection_type: ConnectionType::Cellular,
            allow_cellular: true,
            ..Default::default()
        };
        let manager = AssignmentManager::new(constraints);
        let assignment = make_full_model_assignment(1024, 1.0);

        assert!(manager.validate_constraints(&assignment).is_ok());
    }

    #[test]
    fn test_validate_rejects_model_too_large() {
        let constraints = PhoneConstraints {
            max_full_model_params_b: 3.0,
            ..Default::default()
        };
        let manager = AssignmentManager::new(constraints);
        let assignment = make_full_model_assignment(2048, 7.0); // 7B exceeds 3B limit

        let result = manager.validate_constraints(&assignment);
        assert!(matches!(
            result,
            Err(ConstraintViolation::ModelTooLarge {
                params_b,
                max_b
            }) if (params_b - 7.0).abs() < f64::EPSILON && (max_b - 3.0).abs() < f64::EPSILON
        ));
    }

    #[test]
    fn test_validate_split_layers_no_params_check() {
        // Split layer assignments don't check params_b (only full models do)
        let constraints = PhoneConstraints {
            max_full_model_params_b: 3.0,
            ..Default::default()
        };
        let manager = AssignmentManager::new(constraints);
        let assignment = make_split_assignment(2048);

        assert!(manager.validate_constraints(&assignment).is_ok());
    }

    #[test]
    fn test_validate_at_exact_memory_boundary() {
        let constraints = PhoneConstraints {
            available_memory_mb: 3072,
            max_model_size_mb: 3072,
            ..Default::default()
        };
        let manager = AssignmentManager::new(constraints);
        let assignment = make_full_model_assignment(3072, 2.5); // Exactly at limit

        assert!(manager.validate_constraints(&assignment).is_ok());
    }

    #[test]
    fn test_validate_one_over_memory_boundary() {
        let constraints = PhoneConstraints {
            available_memory_mb: 3072,
            max_model_size_mb: 3072,
            ..Default::default()
        };
        let manager = AssignmentManager::new(constraints);
        let assignment = make_full_model_assignment(3073, 2.5); // One over

        assert!(manager.validate_constraints(&assignment).is_err());
    }

    // ─── Handle Assignment Tests ─────────────────────────────────────────────

    #[test]
    fn test_handle_assignment_accepted() {
        let mut manager = AssignmentManager::with_defaults();
        let assignment = make_full_model_assignment(1024, 2.0);

        let response = manager.handle_assignment(&assignment);
        assert!(matches!(response, AssignmentResponse::Accepted { .. }));
        assert!(manager.is_model_loaded(&"test-model".to_string()));
    }

    #[test]
    fn test_handle_assignment_rejected() {
        let constraints = PhoneConstraints {
            available_memory_mb: 512,
            ..Default::default()
        };
        let mut manager = AssignmentManager::new(constraints);
        let assignment = make_full_model_assignment(2048, 2.0);

        let response = manager.handle_assignment(&assignment);
        assert!(matches!(response, AssignmentResponse::Rejected { .. }));
        assert!(!manager.is_model_loaded(&"test-model".to_string()));
    }

    #[test]
    fn test_handle_assignment_estimated_ready_time() {
        let mut manager = AssignmentManager::with_defaults();
        let assignment = make_full_model_assignment(1000, 2.0);

        let response = manager.handle_assignment(&assignment);
        match response {
            AssignmentResponse::Accepted { estimated_ready_ms } => {
                assert_eq!(estimated_ready_ms, 10000); // 1000 MB * 10 ms/MB
            }
            _ => panic!("Expected Accepted response"),
        }
    }

    // ─── Handle Unload Tests ─────────────────────────────────────────────────

    #[test]
    fn test_handle_unload_success() {
        let mut manager = AssignmentManager::with_defaults();
        let assignment = make_full_model_assignment(1024, 2.0);
        manager.handle_assignment(&assignment);

        let result = manager.handle_unload(&"test-model".to_string());
        assert!(result.is_ok());
        assert!(!manager.is_model_loaded(&"test-model".to_string()));
    }

    #[test]
    fn test_handle_unload_model_not_loaded() {
        let mut manager = AssignmentManager::with_defaults();

        let result = manager.handle_unload(&"nonexistent".to_string());
        assert!(matches!(result, Err(AssignmentError::ModelNotLoaded(_))));
    }

    #[test]
    fn test_multiple_models_loaded() {
        let mut manager = AssignmentManager::with_defaults();

        let assignment1 = ModelAssignment {
            model_id: "model-a".to_string(),
            assignment_type: AssignmentType::FullModel { params_b: 1.0 },
            download_url: "http://example.com/a.gguf".to_string(),
            weight_size_mb: 512,
            priority: AssignmentPriority::Normal,
        };
        let assignment2 = ModelAssignment {
            model_id: "model-b".to_string(),
            assignment_type: AssignmentType::FullModel { params_b: 2.0 },
            download_url: "http://example.com/b.gguf".to_string(),
            weight_size_mb: 1024,
            priority: AssignmentPriority::Normal,
        };

        manager.handle_assignment(&assignment1);
        manager.handle_assignment(&assignment2);

        assert_eq!(manager.loaded_models().len(), 2);
        assert!(manager.is_model_loaded(&"model-a".to_string()));
        assert!(manager.is_model_loaded(&"model-b".to_string()));

        // Unload one
        manager.handle_unload(&"model-a".to_string()).unwrap();
        assert!(!manager.is_model_loaded(&"model-a".to_string()));
        assert!(manager.is_model_loaded(&"model-b".to_string()));
    }

    // ─── WiFi/Ethernet Tests ─────────────────────────────────────────────────

    #[test]
    fn test_validate_accepts_wifi_connection() {
        let constraints = PhoneConstraints {
            connection_type: ConnectionType::WiFi,
            allow_cellular: false,
            ..Default::default()
        };
        let manager = AssignmentManager::new(constraints);
        let assignment = make_full_model_assignment(1024, 1.0);
        assert!(manager.validate_constraints(&assignment).is_ok());
    }

    #[test]
    fn test_validate_accepts_ethernet_connection() {
        let constraints = PhoneConstraints {
            connection_type: ConnectionType::Ethernet,
            allow_cellular: false,
            ..Default::default()
        };
        let manager = AssignmentManager::new(constraints);
        let assignment = make_full_model_assignment(1024, 1.0);
        assert!(manager.validate_constraints(&assignment).is_ok());
    }
}
