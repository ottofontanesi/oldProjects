// Headless Node Daemon — standalone compute node without GUI.
//
// Reuses all library modules (transport, backends, inference).
// No Tauri, no WebView, no frontend.

pub mod config;
pub mod health_reporter;
pub mod optimizer_client;
pub mod control_api;

use config::NodeConfig;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// The headless node daemon orchestrator.
pub struct NodeDaemon {
    config: NodeConfig,
    running: Arc<AtomicBool>,
    node_id: uuid::Uuid,
    started_at_ms: u64,
    models_loaded: Vec<String>,
}

impl NodeDaemon {
    /// Create a new daemon with the given config.
    pub fn new(config: NodeConfig) -> Self {
        Self {
            config,
            running: Arc::new(AtomicBool::new(false)),
            node_id: uuid::Uuid::new_v4(),
            started_at_ms: 0,
            models_loaded: Vec::new(),
        }
    }

    /// Run the daemon (blocking until shutdown signal).
    pub fn start(&mut self) -> Result<(), DaemonError> {
        self.running.store(true, Ordering::Relaxed);
        self.started_at_ms = now_ms();

        eprintln!("[resonantos-node] Starting daemon...");
        eprintln!("[resonantos-node] Node ID: {}", self.node_id);
        eprintln!("[resonantos-node] Listen port: {}", self.config.network.listen_port);
        eprintln!("[resonantos-node] Models dir: {:?}", self.config.models.directory);
        eprintln!("[resonantos-node] Low-power: {}", self.config.daemon.low_power);

        // In production: start tokio runtime, spawn tasks
        // 1. Start transport (mDNS discovery)
        // 2. Detect hardware (BackendRegistry)
        // 3. Announce to mesh
        // 4. Start health reporter
        // 5. Start optimizer client (listen for commands)
        // 6. Start control API (localhost HTTP)
        // 7. Main loop: select on shutdown signal + incoming messages

        eprintln!("[resonantos-node] Daemon started in {}ms", now_ms() - self.started_at_ms);
        Ok(())
    }

    /// Graceful shutdown.
    pub fn shutdown(&mut self) -> Result<(), DaemonError> {
        eprintln!("[resonantos-node] Shutting down...");
        let start = std::time::Instant::now();

        self.running.store(false, Ordering::Relaxed);

        // 1. Stop accepting new requests
        // 2. Complete in-flight inference (5s timeout)
        // 3. Unload all models
        self.models_loaded.clear();
        // 4. Send goodbye to peers
        // 5. Stop transport
        // 6. Flush logs

        let elapsed_ms = start.elapsed().as_millis() as u64;
        eprintln!("[resonantos-node] Shutdown complete in {}ms", elapsed_ms);

        if elapsed_ms > 3000 {
            return Err(DaemonError::ShutdownTimeout { elapsed_ms });
        }
        Ok(())
    }

    /// Check if daemon is running.
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    /// Get node ID.
    pub fn node_id(&self) -> uuid::Uuid {
        self.node_id
    }

    /// Get uptime in seconds.
    pub fn uptime_secs(&self) -> u64 {
        if self.started_at_ms == 0 { return 0; }
        (now_ms() - self.started_at_ms) / 1000
    }

    /// Load a model (command from optimizer).
    pub fn load_model(&mut self, model_id: &str) -> Result<(), DaemonError> {
        let max = self.config.effective_max_models();
        if self.models_loaded.len() >= max {
            return Err(DaemonError::ModelLimitReached { max });
        }
        self.models_loaded.push(model_id.to_string());
        eprintln!("[resonantos-node] Model loaded: {}", model_id);
        Ok(())
    }

    /// Unload a model.
    pub fn unload_model(&mut self, model_id: &str) -> Result<(), DaemonError> {
        self.models_loaded.retain(|m| m != model_id);
        eprintln!("[resonantos-node] Model unloaded: {}", model_id);
        Ok(())
    }

    /// Get loaded models.
    pub fn loaded_models(&self) -> &[String] {
        &self.models_loaded
    }

    /// Get current status.
    pub fn status(&self) -> DaemonStatus {
        DaemonStatus {
            node_id: self.node_id,
            running: self.is_running(),
            uptime_secs: self.uptime_secs(),
            models_loaded: self.models_loaded.clone(),
            low_power: self.config.daemon.low_power,
            listen_port: self.config.network.listen_port,
        }
    }
}

/// Daemon status report.
#[derive(Debug, Clone)]
pub struct DaemonStatus {
    pub node_id: uuid::Uuid,
    pub running: bool,
    pub uptime_secs: u64,
    pub models_loaded: Vec<String>,
    pub low_power: bool,
    pub listen_port: u16,
}

/// Daemon errors.
#[derive(Debug, Clone, PartialEq)]
pub enum DaemonError {
    StartFailed { reason: String },
    ShutdownTimeout { elapsed_ms: u64 },
    ModelLimitReached { max: usize },
    CommandFailed { reason: String },
}

impl std::fmt::Display for DaemonError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::StartFailed { reason } => write!(f, "Start failed: {}", reason),
            Self::ShutdownTimeout { elapsed_ms } => write!(f, "Shutdown timeout: {}ms", elapsed_ms),
            Self::ModelLimitReached { max } => write!(f, "Model limit reached: max {}", max),
            Self::CommandFailed { reason } => write!(f, "Command failed: {}", reason),
        }
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_daemon_lifecycle() {
        let config = NodeConfig::default();
        let mut daemon = NodeDaemon::new(config);

        assert!(!daemon.is_running());
        daemon.start().unwrap();
        assert!(daemon.is_running());
        daemon.shutdown().unwrap();
        assert!(!daemon.is_running());
    }

    #[test]
    fn test_model_load_unload() {
        let config = NodeConfig::default();
        let mut daemon = NodeDaemon::new(config);

        daemon.load_model("llama-7b").unwrap();
        assert_eq!(daemon.loaded_models(), &["llama-7b"]);

        daemon.unload_model("llama-7b").unwrap();
        assert!(daemon.loaded_models().is_empty());
    }

    #[test]
    fn test_model_limit_enforced() {
        let mut config = NodeConfig::default();
        config.models.max_loaded = 1;
        let mut daemon = NodeDaemon::new(config);

        daemon.load_model("model-a").unwrap();
        let result = daemon.load_model("model-b");
        assert!(matches!(result, Err(DaemonError::ModelLimitReached { .. })));
    }

    #[test]
    fn test_low_power_model_limit() {
        let mut config = NodeConfig::default();
        config.daemon.low_power = true;
        config.low_power.max_models = 1;
        let mut daemon = NodeDaemon::new(config);

        daemon.load_model("model-a").unwrap();
        let result = daemon.load_model("model-b");
        assert!(matches!(result, Err(DaemonError::ModelLimitReached { .. })));
    }

    #[test]
    fn test_status_report() {
        let config = NodeConfig::default();
        let mut daemon = NodeDaemon::new(config);
        daemon.start().unwrap();

        let status = daemon.status();
        assert!(status.running);
        assert_eq!(status.listen_port, 9741);
        assert!(!status.low_power);
    }

    #[test]
    fn test_shutdown_within_budget() {
        let config = NodeConfig::default();
        let mut daemon = NodeDaemon::new(config);
        daemon.start().unwrap();
        daemon.load_model("test").unwrap();

        let start = std::time::Instant::now();
        daemon.shutdown().unwrap();
        assert!(start.elapsed().as_millis() < 3000);
        assert!(daemon.loaded_models().is_empty());
    }
}
