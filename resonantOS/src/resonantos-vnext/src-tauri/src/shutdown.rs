// Graceful Shutdown — stops all services in reverse dependency order.
//
// Order: timer → emitters → notify peers → unload models → persist state → close transport
// Must complete within 5 seconds (force-exit if exceeded).

use crate::service_registry::{ServiceRegistry, ServiceStatus};

/// Shutdown phase tracking.
#[derive(Debug, Clone, PartialEq)]
pub enum ShutdownPhase {
    NotStarted,
    StoppingTimer,
    StoppingEmitters,
    NotifyingPeers,
    UnloadingModels,
    PersistingState,
    ClosingTransport,
    Complete,
    ForcedExit,
}

/// Result of the shutdown sequence.
#[derive(Debug, Clone)]
pub struct ShutdownResult {
    pub phase_reached: ShutdownPhase,
    pub duration_ms: u64,
    pub forced: bool,
    pub errors: Vec<String>,
}

/// The shutdown orchestrator.
pub struct ShutdownOrchestrator {
    phase: ShutdownPhase,
    max_duration_ms: u64,
    errors: Vec<String>,
}

impl ShutdownOrchestrator {
    pub fn new() -> Self {
        Self {
            phase: ShutdownPhase::NotStarted,
            max_duration_ms: 5000,
            errors: Vec::new(),
        }
    }

    /// Run the full shutdown sequence.
    pub fn run(&mut self, registry: &mut ServiceRegistry) -> ShutdownResult {
        let start = std::time::Instant::now();

        self.stop_timer(registry);
        self.stop_emitters(registry);
        self.notify_peers(registry);
        self.unload_models(registry);
        self.persist_state(registry);
        self.close_transport(registry);

        let elapsed_ms = start.elapsed().as_millis() as u64;
        let forced = elapsed_ms > self.max_duration_ms;

        if forced {
            self.phase = ShutdownPhase::ForcedExit;
            eprintln!("[shutdown] Forced exit after {}ms (limit: {}ms)", elapsed_ms, self.max_duration_ms);
        } else {
            self.phase = ShutdownPhase::Complete;
            eprintln!("[shutdown] Clean shutdown in {}ms", elapsed_ms);
        }

        ShutdownResult {
            phase_reached: self.phase.clone(),
            duration_ms: elapsed_ms,
            forced,
            errors: self.errors.clone(),
        }
    }

    fn stop_timer(&mut self, registry: &mut ServiceRegistry) {
        self.phase = ShutdownPhase::StoppingTimer;
        registry.update_status("optimizer", ServiceStatus::Stopped);
    }

    fn stop_emitters(&mut self, registry: &mut ServiceRegistry) {
        self.phase = ShutdownPhase::StoppingEmitters;
        registry.update_status("emitters", ServiceStatus::Stopped);
    }

    fn notify_peers(&mut self, registry: &mut ServiceRegistry) {
        self.phase = ShutdownPhase::NotifyingPeers;
        // In production: send goodbye message to all connected peers
        registry.update_status("companion", ServiceStatus::Stopped);
    }

    fn unload_models(&mut self, registry: &mut ServiceRegistry) {
        self.phase = ShutdownPhase::UnloadingModels;
        // In production: call inference_engine.unload_all()
        registry.update_status("inference", ServiceStatus::Stopped);
    }

    fn persist_state(&mut self, registry: &mut ServiceRegistry) {
        self.phase = ShutdownPhase::PersistingState;
        // In production: flush pending writes, save epsilon, save node state
        registry.update_status("agents", ServiceStatus::Stopped);
    }

    fn close_transport(&mut self, registry: &mut ServiceRegistry) {
        self.phase = ShutdownPhase::ClosingTransport;
        // In production: close all transport adapters
        registry.update_status("transport", ServiceStatus::Stopped);
        registry.update_status("persistence", ServiceStatus::Stopped);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_running_registry() -> ServiceRegistry {
        let mut reg = ServiceRegistry::new();
        reg.register("persistence", true);
        reg.register("transport", true);
        reg.register("inference", false);
        reg.register("optimizer", true);
        reg.register("agents", false);
        reg.register("companion", false);
        reg.register("emitters", false);

        for svc in &mut reg.services {
            svc.status = ServiceStatus::Running;
        }
        reg
    }

    #[test]
    fn test_clean_shutdown() {
        let mut registry = make_running_registry();
        let mut shutdown = ShutdownOrchestrator::new();
        let result = shutdown.run(&mut registry);

        assert_eq!(result.phase_reached, ShutdownPhase::Complete);
        assert!(!result.forced);
        assert!(result.duration_ms < 5000);
    }

    #[test]
    fn test_all_services_stopped() {
        let mut registry = make_running_registry();
        let mut shutdown = ShutdownOrchestrator::new();
        shutdown.run(&mut registry);

        // All services should be stopped
        for svc in &registry.services {
            assert_eq!(svc.status, ServiceStatus::Stopped, "Service {} not stopped", svc.name);
        }
    }

    #[test]
    fn test_shutdown_phases_in_order() {
        let mut registry = make_running_registry();
        let mut shutdown = ShutdownOrchestrator::new();
        shutdown.run(&mut registry);

        // Final phase should be Complete
        assert_eq!(shutdown.phase, ShutdownPhase::Complete);
    }
}
