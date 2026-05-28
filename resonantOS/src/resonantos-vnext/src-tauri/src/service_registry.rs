// Service Registry — holds Arc references to all backend services.
//
// Provides a single point of access for all initialized services.

use std::sync::Arc;
use tokio::sync::RwLock;

/// Status of a service in the registry.
#[derive(Debug, Clone, PartialEq)]
pub enum ServiceStatus {
    NotStarted,
    Starting,
    Running,
    Failed { reason: String },
    Stopped,
}

/// Information about a registered service.
#[derive(Debug, Clone)]
pub struct ServiceInfo {
    pub name: String,
    pub status: ServiceStatus,
    pub started_at_ms: Option<u64>,
    pub restart_count: u32,
    pub is_critical: bool,
}

/// The service registry — holds references to all backend services.
pub struct ServiceRegistry {
    pub services: Vec<ServiceInfo>,
    pub persistence_ready: bool,
    pub hardware_ready: bool,
    pub transport_ready: bool,
    pub inference_ready: bool,
    pub optimizer_ready: bool,
    pub emitters_ready: bool,
    pub is_first_run: bool,
    pub startup_time_ms: u64,
}

impl ServiceRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self {
            services: Vec::new(),
            persistence_ready: false,
            hardware_ready: false,
            transport_ready: false,
            inference_ready: false,
            optimizer_ready: false,
            emitters_ready: false,
            is_first_run: false,
            startup_time_ms: 0,
        }
    }

    /// Register a service.
    pub fn register(&mut self, name: &str, is_critical: bool) {
        self.services.push(ServiceInfo {
            name: name.to_string(),
            status: ServiceStatus::NotStarted,
            started_at_ms: None,
            restart_count: 0,
            is_critical,
        });
    }

    /// Update a service's status.
    pub fn update_status(&mut self, name: &str, status: ServiceStatus) {
        if let Some(svc) = self.services.iter_mut().find(|s| s.name == name) {
            svc.status = status.clone();
            if status == ServiceStatus::Running && svc.started_at_ms.is_none() {
                svc.started_at_ms = Some(now_ms());
            }
        }
    }

    /// Get a service's current status.
    pub fn get_status(&self, name: &str) -> Option<&ServiceStatus> {
        self.services.iter().find(|s| s.name == name).map(|s| &s.status)
    }

    /// Check if all critical services are running.
    pub fn all_critical_running(&self) -> bool {
        self.services
            .iter()
            .filter(|s| s.is_critical)
            .all(|s| s.status == ServiceStatus::Running)
    }

    /// Get count of running services.
    pub fn running_count(&self) -> usize {
        self.services.iter().filter(|s| s.status == ServiceStatus::Running).count()
    }

    /// Get count of failed services.
    pub fn failed_count(&self) -> usize {
        self.services.iter().filter(|s| matches!(s.status, ServiceStatus::Failed { .. })).count()
    }

    /// Get all service statuses for health reporting.
    pub fn health_summary(&self) -> Vec<(String, ServiceStatus)> {
        self.services.iter().map(|s| (s.name.clone(), s.status.clone())).collect()
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_registry_empty() {
        let reg = ServiceRegistry::new();
        assert_eq!(reg.services.len(), 0);
        assert_eq!(reg.running_count(), 0);
    }

    #[test]
    fn test_register_and_update() {
        let mut reg = ServiceRegistry::new();
        reg.register("persistence", true);
        reg.register("transport", true);
        reg.register("emitters", false);

        assert_eq!(reg.services.len(), 3);
        assert!(!reg.all_critical_running());

        reg.update_status("persistence", ServiceStatus::Running);
        reg.update_status("transport", ServiceStatus::Running);
        assert!(reg.all_critical_running());
    }

    #[test]
    fn test_failed_service() {
        let mut reg = ServiceRegistry::new();
        reg.register("inference", false);
        reg.update_status("inference", ServiceStatus::Failed {
            reason: "GPU not found".to_string(),
        });
        assert_eq!(reg.failed_count(), 1);
    }

    #[test]
    fn test_health_summary() {
        let mut reg = ServiceRegistry::new();
        reg.register("a", true);
        reg.register("b", false);
        reg.update_status("a", ServiceStatus::Running);

        let summary = reg.health_summary();
        assert_eq!(summary.len(), 2);
    }
}
