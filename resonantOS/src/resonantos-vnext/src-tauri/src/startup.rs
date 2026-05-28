// Startup Orchestrator — initializes all services in dependency order.
//
// Order: persistence → hardware → catalog → registry → transport →
//        inference → optimizer → agents → companion → emitters
//
// Non-critical failures are logged and skipped.
// Critical failures abort startup with user-visible error.

use crate::service_registry::{ServiceRegistry, ServiceStatus};

/// Result of the startup sequence.
#[derive(Debug, Clone)]
pub enum StartupResult {
    /// All services started successfully.
    Success { time_ms: u64, is_first_run: bool },
    /// Startup completed with some non-critical failures.
    Partial { time_ms: u64, failed_services: Vec<String>, is_first_run: bool },
    /// Critical failure — app cannot start.
    CriticalFailure { reason: String },
}

/// Routing decision after startup.
#[derive(Debug, Clone, PartialEq)]
pub enum InitialRoute {
    /// Show the first-run wizard.
    Wizard,
    /// Show the main dashboard.
    Dashboard,
}

/// The startup orchestrator.
pub struct StartupOrchestrator {
    pub registry: ServiceRegistry,
}

impl StartupOrchestrator {
    pub fn new() -> Self {
        Self {
            registry: ServiceRegistry::new(),
        }
    }

    /// Run the full startup sequence.
    pub fn run(&mut self) -> StartupResult {
        let start = std::time::Instant::now();

        // Register all services
        self.registry.register("persistence", true);
        self.registry.register("hardware", false);
        self.registry.register("catalog", false);
        self.registry.register("node_registry", true);
        self.registry.register("transport", true);
        self.registry.register("inference", false);
        self.registry.register("optimizer", true);
        self.registry.register("agents", false);
        self.registry.register("companion", false);
        self.registry.register("emitters", false);

        // Initialize in order
        self.init_persistence();
        self.init_hardware();
        self.init_catalog();
        self.init_node_registry();
        self.init_transport();
        self.init_inference();
        self.init_optimizer();
        self.init_agents();
        self.init_companion();
        self.init_emitters();

        // Check first-run
        let is_first_run = self.detect_first_run();
        self.registry.is_first_run = is_first_run;

        let elapsed_ms = start.elapsed().as_millis() as u64;
        self.registry.startup_time_ms = elapsed_ms;

        // Check for critical failures
        let critical_failures: Vec<String> = self.registry.services.iter()
            .filter(|s| s.is_critical && matches!(s.status, ServiceStatus::Failed { .. }))
            .map(|s| s.name.clone())
            .collect();

        if !critical_failures.is_empty() {
            return StartupResult::CriticalFailure {
                reason: format!("Critical services failed: {}", critical_failures.join(", ")),
            };
        }

        let non_critical_failures: Vec<String> = self.registry.services.iter()
            .filter(|s| !s.is_critical && matches!(s.status, ServiceStatus::Failed { .. }))
            .map(|s| s.name.clone())
            .collect();

        if non_critical_failures.is_empty() {
            StartupResult::Success { time_ms: elapsed_ms, is_first_run }
        } else {
            StartupResult::Partial {
                time_ms: elapsed_ms,
                failed_services: non_critical_failures,
                is_first_run,
            }
        }
    }

    /// Determine initial route based on first-run status.
    pub fn initial_route(&self) -> InitialRoute {
        if self.registry.is_first_run {
            InitialRoute::Wizard
        } else {
            InitialRoute::Dashboard
        }
    }

    // ─── Service Initialization ──────────────────────────────────────────

    fn init_persistence(&mut self) {
        self.registry.update_status("persistence", ServiceStatus::Starting);
        // In production: open SQLite, run migrations
        self.registry.update_status("persistence", ServiceStatus::Running);
        self.registry.persistence_ready = true;
    }

    fn init_hardware(&mut self) {
        self.registry.update_status("hardware", ServiceStatus::Starting);
        // In production: detect GPU, classify hardware tier
        self.registry.update_status("hardware", ServiceStatus::Running);
        self.registry.hardware_ready = true;
    }

    fn init_catalog(&mut self) {
        self.registry.update_status("catalog", ServiceStatus::Starting);
        // In production: load model_catalog.json
        self.registry.update_status("catalog", ServiceStatus::Running);
    }

    fn init_node_registry(&mut self) {
        self.registry.update_status("node_registry", ServiceStatus::Starting);
        // In production: create NodeRegistry, register local node
        self.registry.update_status("node_registry", ServiceStatus::Running);
    }

    fn init_transport(&mut self) {
        self.registry.update_status("transport", ServiceStatus::Starting);
        // In production: start LAN adapter, optionally WireGuard
        self.registry.update_status("transport", ServiceStatus::Running);
        self.registry.transport_ready = true;
    }

    fn init_inference(&mut self) {
        self.registry.update_status("inference", ServiceStatus::Starting);
        // In production: create LocalInferenceEngine, attempt model load
        self.registry.update_status("inference", ServiceStatus::Running);
        self.registry.inference_ready = true;
    }

    fn init_optimizer(&mut self) {
        self.registry.update_status("optimizer", ServiceStatus::Starting);
        // In production: create IntegrationCoordinator, load RL model
        self.registry.update_status("optimizer", ServiceStatus::Running);
        self.registry.optimizer_ready = true;
    }

    fn init_agents(&mut self) {
        self.registry.update_status("agents", ServiceStatus::Starting);
        // In production: create WorkflowOrchestrator
        self.registry.update_status("agents", ServiceStatus::Running);
    }

    fn init_companion(&mut self) {
        self.registry.update_status("companion", ServiceStatus::Starting);
        // In production: start CompanionService, listen for pairing
        self.registry.update_status("companion", ServiceStatus::Running);
    }

    fn init_emitters(&mut self) {
        self.registry.update_status("emitters", ServiceStatus::Starting);
        // In production: start EventEmitterService
        self.registry.update_status("emitters", ServiceStatus::Running);
        self.registry.emitters_ready = true;
    }

    // ─── First-Run Detection ─────────────────────────────────────────────

    fn detect_first_run(&self) -> bool {
        // In production: check persistence for `setup_complete` flag
        // For now: always return false (not first run)
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_startup_succeeds() {
        let mut orchestrator = StartupOrchestrator::new();
        let result = orchestrator.run();
        assert!(matches!(result, StartupResult::Success { .. }));
    }

    #[test]
    fn test_all_services_registered() {
        let mut orchestrator = StartupOrchestrator::new();
        orchestrator.run();
        assert_eq!(orchestrator.registry.services.len(), 10);
        assert_eq!(orchestrator.registry.running_count(), 10);
    }

    #[test]
    fn test_initial_route_dashboard() {
        let mut orchestrator = StartupOrchestrator::new();
        orchestrator.run();
        assert_eq!(orchestrator.initial_route(), InitialRoute::Dashboard);
    }

    #[test]
    fn test_startup_time_recorded() {
        let mut orchestrator = StartupOrchestrator::new();
        orchestrator.run();
        // Should complete nearly instantly in tests
        assert!(orchestrator.registry.startup_time_ms < 1000);
    }
}
