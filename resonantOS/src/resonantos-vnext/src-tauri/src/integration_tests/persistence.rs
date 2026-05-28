// In-memory persistence for integration tests.
//
// HashMap-backed storage — same interface as real persistence.

use super::mock_node::NodeId;
use std::collections::HashMap;
use std::sync::Mutex;

/// Workflow checkpoint data.
#[derive(Debug, Clone)]
pub struct WorkflowCheckpoint {
    pub workflow_id: String,
    pub completed_steps: Vec<String>,
    pub step_results: HashMap<String, Vec<u8>>,
    pub created_at_ms: u64,
}

/// Node state for persistence.
#[derive(Debug, Clone)]
pub struct PersistedNodeState {
    pub node_id: NodeId,
    pub hostname: String,
    pub online: bool,
    pub last_seen_ms: u64,
}

/// Resume state for downloads.
#[derive(Debug, Clone)]
pub struct ResumeState {
    pub download_id: String,
    pub bytes_downloaded: u64,
    pub total_bytes: u64,
    pub chunk_hashes: Vec<String>,
}

/// In-memory persistence store for integration tests.
pub struct InMemoryPersistence {
    pub checkpoints: Mutex<HashMap<String, WorkflowCheckpoint>>,
    pub node_states: Mutex<HashMap<NodeId, PersistedNodeState>>,
    pub resume_states: Mutex<HashMap<String, ResumeState>>,
    pub kv_store: Mutex<HashMap<String, String>>,
}

impl InMemoryPersistence {
    pub fn new() -> Self {
        Self {
            checkpoints: Mutex::new(HashMap::new()),
            node_states: Mutex::new(HashMap::new()),
            resume_states: Mutex::new(HashMap::new()),
            kv_store: Mutex::new(HashMap::new()),
        }
    }

    // ─── Checkpoint Operations ───────────────────────────────────────────

    pub fn save_checkpoint(&self, checkpoint: WorkflowCheckpoint) {
        self.checkpoints
            .lock()
            .unwrap()
            .insert(checkpoint.workflow_id.clone(), checkpoint);
    }

    pub fn load_checkpoint(&self, workflow_id: &str) -> Option<WorkflowCheckpoint> {
        self.checkpoints.lock().unwrap().get(workflow_id).cloned()
    }

    pub fn remove_checkpoint(&self, workflow_id: &str) {
        self.checkpoints.lock().unwrap().remove(workflow_id);
    }

    // ─── Node State Operations ───────────────────────────────────────────

    pub fn save_node_state(&self, state: PersistedNodeState) {
        self.node_states.lock().unwrap().insert(state.node_id, state);
    }

    pub fn load_node_state(&self, node_id: &NodeId) -> Option<PersistedNodeState> {
        self.node_states.lock().unwrap().get(node_id).cloned()
    }

    pub fn all_node_states(&self) -> Vec<PersistedNodeState> {
        self.node_states.lock().unwrap().values().cloned().collect()
    }

    // ─── Resume State Operations ─────────────────────────────────────────

    pub fn save_resume_state(&self, state: ResumeState) {
        self.resume_states
            .lock()
            .unwrap()
            .insert(state.download_id.clone(), state);
    }

    pub fn load_resume_state(&self, download_id: &str) -> Option<ResumeState> {
        self.resume_states.lock().unwrap().get(download_id).cloned()
    }

    // ─── Key-Value Store ─────────────────────────────────────────────────

    pub fn get(&self, key: &str) -> Option<String> {
        self.kv_store.lock().unwrap().get(key).cloned()
    }

    pub fn set(&self, key: &str, value: &str) {
        self.kv_store
            .lock()
            .unwrap()
            .insert(key.to_string(), value.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn test_checkpoint_roundtrip() {
        let store = InMemoryPersistence::new();
        let cp = WorkflowCheckpoint {
            workflow_id: "wf-1".to_string(),
            completed_steps: vec!["step-1".to_string(), "step-2".to_string()],
            step_results: HashMap::new(),
            created_at_ms: 1000,
        };

        store.save_checkpoint(cp.clone());
        let loaded = store.load_checkpoint("wf-1").unwrap();
        assert_eq!(loaded.completed_steps.len(), 2);
        assert_eq!(loaded.created_at_ms, 1000);
    }

    #[test]
    fn test_node_state_roundtrip() {
        let store = InMemoryPersistence::new();
        let id = Uuid::new_v4();
        let state = PersistedNodeState {
            node_id: id,
            hostname: "test-node".to_string(),
            online: true,
            last_seen_ms: 5000,
        };

        store.save_node_state(state);
        let loaded = store.load_node_state(&id).unwrap();
        assert_eq!(loaded.hostname, "test-node");
        assert!(loaded.online);
    }

    #[test]
    fn test_missing_checkpoint_returns_none() {
        let store = InMemoryPersistence::new();
        assert!(store.load_checkpoint("nonexistent").is_none());
    }

    #[test]
    fn test_kv_store() {
        let store = InMemoryPersistence::new();
        store.set("key1", "value1");
        assert_eq!(store.get("key1"), Some("value1".to_string()));
        assert_eq!(store.get("missing"), None);
    }
}
