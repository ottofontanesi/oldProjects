// Intent citation: .kiro/specs/network-onboarding-wizard/design.md Section 2.1
// Wizard State — persistence, lifecycle, resume support

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

// ─── Wizard Types ────────────────────────────────────────────────────────────

pub type NodeId = Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum WizardType {
    LocalSetup,
    MeshJoin,
    PhonePairing,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum WizardStatus {
    InProgress,
    Completed,
    Cancelled,
    Failed { error: String },
}

/// Data stored per wizard step.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StepData {
    NetworkScan(serde_json::Value),
    NodeSelection(Vec<NodeId>),
    HealthCheck(serde_json::Value),
    CapacityPreview(serde_json::Value),
    OptimizationPreview(serde_json::Value),
    InvitationDecode(serde_json::Value),
    CapacityOffer(serde_json::Value),
    PrivacySettings(serde_json::Value),
    PhonePairingInit(serde_json::Value),
    PhoneSettings(serde_json::Value),
    Confirmation(bool),
}

/// Complete wizard state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WizardState {
    pub wizard_id: Uuid,
    pub wizard_type: WizardType,
    pub current_step: u32,
    pub total_steps: u32,
    pub started_at: DateTime<Utc>,
    pub last_updated: DateTime<Utc>,
    pub step_data: HashMap<u32, StepData>,
    pub status: WizardStatus,
}

impl WizardState {
    /// Create a new wizard state.
    pub fn new(wizard_type: WizardType) -> Self {
        let total_steps = match wizard_type {
            WizardType::LocalSetup => 6,
            WizardType::MeshJoin => 7,
            WizardType::PhonePairing => 4,
        };

        Self {
            wizard_id: Uuid::new_v4(),
            wizard_type,
            current_step: 1,
            total_steps,
            started_at: Utc::now(),
            last_updated: Utc::now(),
            step_data: HashMap::new(),
            status: WizardStatus::InProgress,
        }
    }

    /// Advance to the next step.
    pub fn advance(&mut self) {
        if self.current_step < self.total_steps {
            self.current_step += 1;
            self.last_updated = Utc::now();
        }
    }

    /// Go back to the previous step.
    pub fn go_back(&mut self) {
        if self.current_step > 1 {
            self.current_step -= 1;
            self.last_updated = Utc::now();
        }
    }

    /// Save data for the current step.
    pub fn save_step_data(&mut self, data: StepData) {
        self.step_data.insert(self.current_step, data);
        self.last_updated = Utc::now();
    }

    /// Mark as completed.
    pub fn complete(&mut self) {
        self.status = WizardStatus::Completed;
        self.last_updated = Utc::now();
    }

    /// Mark as cancelled.
    pub fn cancel(&mut self) {
        self.status = WizardStatus::Cancelled;
        self.last_updated = Utc::now();
    }

    /// Mark as failed.
    pub fn fail(&mut self, error: String) {
        self.status = WizardStatus::Failed { error };
        self.last_updated = Utc::now();
    }

    /// Check if this wizard should be cleaned up (completed > 24h ago).
    pub fn should_cleanup(&self) -> bool {
        match self.status {
            WizardStatus::Completed | WizardStatus::Cancelled => {
                (Utc::now() - self.last_updated) > Duration::hours(24)
            }
            _ => false,
        }
    }

    /// Check if this wizard can be resumed.
    pub fn can_resume(&self) -> bool {
        self.status == WizardStatus::InProgress
    }
}

// ─── State Manager ───────────────────────────────────────────────────────────

/// Manages wizard state persistence to SQLite.
pub struct WizardStateManager {
    /// Active wizard states.
    states: HashMap<Uuid, WizardState>,
}

impl WizardStateManager {
    pub fn new() -> Self {
        Self {
            states: HashMap::new(),
        }
    }

    /// Create a new wizard and return its state.
    pub fn create(&mut self, wizard_type: WizardType) -> WizardState {
        let state = WizardState::new(wizard_type);
        self.states.insert(state.wizard_id, state.clone());
        state
    }

    /// Save/update wizard state.
    pub fn save(&mut self, state: WizardState) {
        self.states.insert(state.wizard_id, state);
    }

    /// Load wizard state by ID.
    pub fn load(&self, wizard_id: &Uuid) -> Option<&WizardState> {
        self.states.get(wizard_id)
    }

    /// Load mutable wizard state by ID.
    pub fn load_mut(&mut self, wizard_id: &Uuid) -> Option<&mut WizardState> {
        self.states.get_mut(wizard_id)
    }

    /// Find any in-progress wizard of a given type (for resume).
    pub fn find_resumable(&self, wizard_type: &WizardType) -> Option<&WizardState> {
        self.states
            .values()
            .find(|s| s.wizard_type == *wizard_type && s.can_resume())
    }

    /// Delete a wizard state.
    pub fn delete(&mut self, wizard_id: &Uuid) {
        self.states.remove(wizard_id);
    }

    /// Cleanup old completed/cancelled wizards (>24h).
    pub fn cleanup_old(&mut self) -> u32 {
        let to_remove: Vec<Uuid> = self
            .states
            .values()
            .filter(|s| s.should_cleanup())
            .map(|s| s.wizard_id)
            .collect();

        let count = to_remove.len() as u32;
        for id in to_remove {
            self.states.remove(&id);
        }
        count
    }

    /// Persist state to SQLite (for app restart survival).
    pub fn persist_to_db(&self, conn: &rusqlite::Connection) -> Result<(), String> {
        conn.execute(
            "CREATE TABLE IF NOT EXISTS wizard_state (
                wizard_id TEXT PRIMARY KEY,
                data TEXT NOT NULL,
                updated_at TEXT NOT NULL
            )",
            [],
        )
        .map_err(|e| format!("Failed to create wizard_state table: {}", e))?;

        for state in self.states.values() {
            let json = serde_json::to_string(state)
                .map_err(|e| format!("Failed to serialize wizard state: {}", e))?;
            conn.execute(
                "INSERT OR REPLACE INTO wizard_state (wizard_id, data, updated_at) VALUES (?1, ?2, ?3)",
                rusqlite::params![
                    state.wizard_id.to_string(),
                    json,
                    state.last_updated.to_rfc3339(),
                ],
            )
            .map_err(|e| format!("Failed to persist wizard state: {}", e))?;
        }

        Ok(())
    }

    /// Load all states from SQLite.
    pub fn load_from_db(&mut self, conn: &rusqlite::Connection) -> Result<(), String> {
        let exists: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='wizard_state'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .map(|c| c > 0)
            .unwrap_or(false);

        if !exists {
            return Ok(());
        }

        let mut stmt = conn
            .prepare("SELECT data FROM wizard_state")
            .map_err(|e| format!("Failed to prepare query: {}", e))?;

        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|e| format!("Failed to query wizard states: {}", e))?;

        for row in rows {
            if let Ok(json) = row {
                if let Ok(state) = serde_json::from_str::<WizardState>(&json) {
                    self.states.insert(state.wizard_id, state);
                }
            }
        }

        Ok(())
    }
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wizard_state_lifecycle() {
        let mut state = WizardState::new(WizardType::LocalSetup);
        assert_eq!(state.current_step, 1);
        assert_eq!(state.total_steps, 6);
        assert_eq!(state.status, WizardStatus::InProgress);

        state.advance();
        assert_eq!(state.current_step, 2);

        state.save_step_data(StepData::Confirmation(true));
        assert!(state.step_data.contains_key(&2));

        state.complete();
        assert_eq!(state.status, WizardStatus::Completed);
    }

    #[test]
    fn test_wizard_resume() {
        let mut manager = WizardStateManager::new();
        let state = manager.create(WizardType::MeshJoin);
        let id = state.wizard_id;

        // Should find resumable
        let found = manager.find_resumable(&WizardType::MeshJoin);
        assert!(found.is_some());
        assert_eq!(found.unwrap().wizard_id, id);

        // Complete it — no longer resumable
        manager.load_mut(&id).unwrap().complete();
        let found = manager.find_resumable(&WizardType::MeshJoin);
        assert!(found.is_none());
    }

    #[test]
    fn test_wizard_persistence_roundtrip() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        let mut manager = WizardStateManager::new();

        let state = manager.create(WizardType::PhonePairing);
        let id = state.wizard_id;

        manager.persist_to_db(&conn).unwrap();

        // Load into fresh manager
        let mut manager2 = WizardStateManager::new();
        manager2.load_from_db(&conn).unwrap();

        let loaded = manager2.load(&id);
        assert!(loaded.is_some());
        assert_eq!(loaded.unwrap().wizard_type, WizardType::PhonePairing);
    }

    #[test]
    fn test_cleanup_old_wizards() {
        let mut manager = WizardStateManager::new();
        let mut state = manager.create(WizardType::LocalSetup);
        state.complete();
        // Backdate to 25 hours ago
        state.last_updated = Utc::now() - Duration::hours(25);
        manager.save(state);

        let cleaned = manager.cleanup_old();
        assert_eq!(cleaned, 1);
    }

    #[test]
    fn test_go_back() {
        let mut state = WizardState::new(WizardType::LocalSetup);
        state.advance();
        state.advance();
        assert_eq!(state.current_step, 3);

        state.go_back();
        assert_eq!(state.current_step, 2);

        // Can't go below 1
        state.go_back();
        state.go_back();
        assert_eq!(state.current_step, 1);
    }
}
