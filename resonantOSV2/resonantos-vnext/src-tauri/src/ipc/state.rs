// IPC State — shared application state for all command handlers

use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::agents::orchestrator::WorkflowOrchestrator;
use crate::companion::service::CompanionService;
use crate::network::registry::NodeRegistry;
use crate::transport::manager::TransportManager;

use super::types::PlacementHistoryEntry;

/// Optimizer state tracked for the IPC layer.
#[derive(Debug, Clone)]
pub struct OptimizerState {
    pub last_run_ms: u64,
    pub next_scheduled_ms: u64,
    pub cycle_count: u64,
    pub last_utility_score: f64,
    pub is_running: bool,
    pub current_plan: Option<CurrentPlan>,
}

/// A snapshot of the current placement plan.
#[derive(Debug, Clone)]
pub struct CurrentPlan {
    pub plan_id: String,
    pub created_at_ms: u64,
    pub solver_duration_ms: u64,
    pub utility_score: f64,
    pub unified_total: f64,
    pub model_count: u32,
    pub agent_count: u32,
    pub model_assignments: Vec<ModelAssignmentInternal>,
    pub agent_assignments: Vec<AgentAssignmentInternal>,
}

#[derive(Debug, Clone)]
pub struct ModelAssignmentInternal {
    pub model_id: String,
    pub model_name: String,
    pub node_ids: Vec<String>,
    pub protocol: String,
    pub estimated_tok_s: f32,
}

#[derive(Debug, Clone)]
pub struct AgentAssignmentInternal {
    pub agent_id: String,
    pub node_id: String,
    pub estimated_throughput: f64,
    pub ram_allocated_mb: u64,
}

impl Default for OptimizerState {
    fn default() -> Self {
        Self {
            last_run_ms: 0,
            next_scheduled_ms: 0,
            cycle_count: 0,
            last_utility_score: 0.0,
            is_running: false,
            current_plan: None,
        }
    }
}

/// Shared application state accessible by all IPC command handlers.
///
/// Each field is wrapped in a `RwLock` for concurrent read access.
/// Services are `Option<T>` because they initialize asynchronously after app start.
pub struct AppState {
    pub agent_orchestrator: RwLock<Option<WorkflowOrchestrator>>,
    pub network_registry: RwLock<Option<Arc<NodeRegistry>>>,
    pub transport_manager: RwLock<Option<TransportManager>>,
    pub companion_service: RwLock<Option<CompanionService>>,
    pub optimizer_state: RwLock<OptimizerState>,
    pub placement_history: RwLock<VecDeque<PlacementHistoryEntry>>,
}

impl AppState {
    /// Create a new AppState with all services uninitialized.
    pub fn new() -> Self {
        Self {
            agent_orchestrator: RwLock::new(None),
            network_registry: RwLock::new(None),
            transport_manager: RwLock::new(None),
            companion_service: RwLock::new(None),
            optimizer_state: RwLock::new(OptimizerState::default()),
            placement_history: RwLock::new(VecDeque::with_capacity(100)),
        }
    }

    /// Check if core services are initialized and ready.
    pub async fn is_ready(&self) -> bool {
        let orchestrator = self.agent_orchestrator.read().await;
        let registry = self.network_registry.read().await;
        let transport = self.transport_manager.read().await;
        orchestrator.is_some() && registry.is_some() && transport.is_some()
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_app_state_new_is_not_ready() {
        let state = AppState::new();
        assert!(!state.is_ready().await);
    }

    #[tokio::test]
    async fn test_optimizer_state_default() {
        let state = AppState::new();
        let opt = state.optimizer_state.read().await;
        assert_eq!(opt.last_run_ms, 0);
        assert_eq!(opt.cycle_count, 0);
        assert_eq!(opt.last_utility_score, 0.0);
        assert!(!opt.is_running);
        assert!(opt.current_plan.is_none());
    }

    #[tokio::test]
    async fn test_placement_history_starts_empty() {
        let state = AppState::new();
        let history = state.placement_history.read().await;
        assert!(history.is_empty());
    }
}
