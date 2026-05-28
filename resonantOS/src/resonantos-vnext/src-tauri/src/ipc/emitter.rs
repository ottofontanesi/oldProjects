// IPC Emitter — EventEmitterService for dashboard data polling
//
// Spawns one tokio task per event channel. Each task loops on a configured
// interval, collects state, computes payloads, and sends them via a channel.
// Uses a watch channel for cancellation (clean shutdown).

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

use super::delta::compute_delta;
use super::payloads::*;
use super::state::AppState;
use super::trend::compute_trend;

/// Configuration for all emitter intervals and behavior.
#[derive(Debug, Clone)]
pub struct EmitterConfig {
    pub node_interval_ms: u64,
    pub plan_interval_ms: u64,
    pub transport_interval_ms: u64,
    pub utility_interval_ms: u64,
    pub download_interval_ms: u64,
    pub companion_interval_ms: u64,
    pub startup_delay_ms: u64,
    pub full_sync_every_n: u32,
}

impl Default for EmitterConfig {
    fn default() -> Self {
        Self {
            node_interval_ms: 2000,
            plan_interval_ms: 10000,
            transport_interval_ms: 5000,
            utility_interval_ms: 5000,
            download_interval_ms: 500,
            companion_interval_ms: 5000,
            startup_delay_ms: 2000,
            full_sync_every_n: 5,
        }
    }
}

/// The event emitter service that manages all periodic emitter tasks.
pub struct EventEmitterService {
    config: EmitterConfig,
    cancel_tx: watch::Sender<bool>,
    cancel_rx: watch::Receiver<bool>,
    tasks: Vec<JoinHandle<()>>,
}

impl EventEmitterService {
    /// Create a new EventEmitterService with the given configuration.
    pub fn new(config: EmitterConfig) -> Self {
        let (cancel_tx, cancel_rx) = watch::channel(false);
        Self {
            config,
            cancel_tx,
            cancel_rx,
            tasks: Vec::new(),
        }
    }

    /// Start all emitter tasks. Each task sends payloads via the provided senders.
    pub fn start(
        &mut self,
        state: Arc<AppState>,
        node_tx: mpsc::Sender<NodeStatusPayload>,
        placement_tx: mpsc::Sender<PlacementPayload>,
        transport_tx: mpsc::Sender<TransportHealthPayload>,
        utility_tx: mpsc::Sender<UtilityPayload>,
        download_tx: mpsc::Sender<DownloadProgressPayload>,
        companion_tx: mpsc::Sender<CompanionPayload>,
    ) {
        let startup_delay = Duration::from_millis(self.config.startup_delay_ms);

        // Node status emitter
        let handle = tokio::spawn(node_status_emitter(
            state.clone(),
            Duration::from_millis(self.config.node_interval_ms),
            self.config.full_sync_every_n,
            startup_delay,
            self.cancel_rx.clone(),
            node_tx,
        ));
        self.tasks.push(handle);

        // Placement plan emitter
        let handle = tokio::spawn(placement_emitter(
            state.clone(),
            Duration::from_millis(self.config.plan_interval_ms),
            startup_delay,
            self.cancel_rx.clone(),
            placement_tx,
        ));
        self.tasks.push(handle);

        // Transport health emitter
        let handle = tokio::spawn(transport_health_emitter(
            state.clone(),
            Duration::from_millis(self.config.transport_interval_ms),
            startup_delay,
            self.cancel_rx.clone(),
            transport_tx,
        ));
        self.tasks.push(handle);

        // Utility score emitter
        let handle = tokio::spawn(utility_emitter(
            state.clone(),
            Duration::from_millis(self.config.utility_interval_ms),
            startup_delay,
            self.cancel_rx.clone(),
            utility_tx,
        ));
        self.tasks.push(handle);

        // Download progress emitter
        let handle = tokio::spawn(download_progress_emitter(
            state.clone(),
            Duration::from_millis(self.config.download_interval_ms),
            startup_delay,
            self.cancel_rx.clone(),
            download_tx,
        ));
        self.tasks.push(handle);

        // Companion status emitter
        let handle = tokio::spawn(companion_status_emitter(
            state.clone(),
            Duration::from_millis(self.config.companion_interval_ms),
            startup_delay,
            self.cancel_rx.clone(),
            companion_tx,
        ));
        self.tasks.push(handle);
    }

    /// Stop all emitter tasks by signaling cancellation.
    pub fn stop(&self) {
        let _ = self.cancel_tx.send(true);
    }
}

/// Helper: get current timestamp in milliseconds.
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Helper: check if cancelled.
async fn is_cancelled(cancel_rx: &mut watch::Receiver<bool>) -> bool {
    // Check current value without waiting
    *cancel_rx.borrow()
}

// ─── Emitter Task Functions ──────────────────────────────────────────────────

/// Node status emitter task (2s interval by default).
///
/// Collects node data, computes delta vs previous snapshot, and sends payload.
/// Every Nth emission is a full sync.
pub async fn node_status_emitter(
    state: Arc<AppState>,
    interval: Duration,
    full_sync_every_n: u32,
    startup_delay: Duration,
    mut cancel_rx: watch::Receiver<bool>,
    tx: mpsc::Sender<NodeStatusPayload>,
) {
    tokio::time::sleep(startup_delay).await;
    if is_cancelled(&mut cancel_rx).await {
        return;
    }

    let mut cycle: u32 = 0;
    let mut previous_snapshot: Option<Vec<NodeSnapshot>> = None;

    loop {
        tokio::select! {
            _ = cancel_rx.changed() => {
                if *cancel_rx.borrow() { break; }
            }
            _ = tokio::time::sleep(interval) => {
                if is_cancelled(&mut cancel_rx).await { break; }
                cycle += 1;

                let current = collect_node_status(&state).await;

                let (nodes, is_full_sync) = if cycle % full_sync_every_n == 0
                    || previous_snapshot.is_none()
                {
                    (current.clone(), true)
                } else {
                    (compute_delta(&previous_snapshot, &current), false)
                };

                let payload = NodeStatusPayload {
                    nodes,
                    is_full_sync,
                    timestamp_ms: now_ms(),
                };

                // Payload size bound check (< 50KB for node payloads)
                if let Ok(serialized) = serde_json::to_string(&payload) {
                    let size_bytes = serialized.len();
                    if size_bytes > 50_000 {
                        eprintln!(
                            "[emitter] Node status payload exceeds 50KB bound: {} bytes ({} nodes)",
                            size_bytes,
                            payload.nodes.len()
                        );
                    }
                }

                let _ = tx.send(payload).await;
                previous_snapshot = Some(current);
            }
        }
    }
}

/// Placement plan emitter task (10s interval by default).
pub async fn placement_emitter(
    state: Arc<AppState>,
    interval: Duration,
    startup_delay: Duration,
    mut cancel_rx: watch::Receiver<bool>,
    tx: mpsc::Sender<PlacementPayload>,
) {
    tokio::time::sleep(startup_delay).await;
    if is_cancelled(&mut cancel_rx).await {
        return;
    }

    let mut last_plan_id: Option<String> = None;

    loop {
        tokio::select! {
            _ = cancel_rx.changed() => {
                if *cancel_rx.borrow() { break; }
            }
            _ = tokio::time::sleep(interval) => {
                if is_cancelled(&mut cancel_rx).await { break; }

                let payload = collect_placement(&state, &mut last_plan_id).await;
                let _ = tx.send(payload).await;
            }
        }
    }
}

/// Transport health emitter task (5s interval by default).
pub async fn transport_health_emitter(
    state: Arc<AppState>,
    interval: Duration,
    startup_delay: Duration,
    mut cancel_rx: watch::Receiver<bool>,
    tx: mpsc::Sender<TransportHealthPayload>,
) {
    tokio::time::sleep(startup_delay).await;
    if is_cancelled(&mut cancel_rx).await {
        return;
    }

    loop {
        tokio::select! {
            _ = cancel_rx.changed() => {
                if *cancel_rx.borrow() { break; }
            }
            _ = tokio::time::sleep(interval) => {
                if is_cancelled(&mut cancel_rx).await { break; }

                let payload = collect_transport_health(&state).await;
                let _ = tx.send(payload).await;
            }
        }
    }
}

/// Utility score emitter task (5s interval by default).
pub async fn utility_emitter(
    state: Arc<AppState>,
    interval: Duration,
    startup_delay: Duration,
    mut cancel_rx: watch::Receiver<bool>,
    tx: mpsc::Sender<UtilityPayload>,
) {
    tokio::time::sleep(startup_delay).await;
    if is_cancelled(&mut cancel_rx).await {
        return;
    }

    let mut history: VecDeque<f64> = VecDeque::with_capacity(60);

    loop {
        tokio::select! {
            _ = cancel_rx.changed() => {
                if *cancel_rx.borrow() { break; }
            }
            _ = tokio::time::sleep(interval) => {
                if is_cancelled(&mut cancel_rx).await { break; }

                let payload = collect_utility(&state, &mut history).await;
                let _ = tx.send(payload).await;
            }
        }
    }
}

/// Download progress emitter task (500ms interval by default).
///
/// Only emits when there are active downloads.
pub async fn download_progress_emitter(
    state: Arc<AppState>,
    interval: Duration,
    startup_delay: Duration,
    mut cancel_rx: watch::Receiver<bool>,
    tx: mpsc::Sender<DownloadProgressPayload>,
) {
    tokio::time::sleep(startup_delay).await;
    if is_cancelled(&mut cancel_rx).await {
        return;
    }

    loop {
        tokio::select! {
            _ = cancel_rx.changed() => {
                if *cancel_rx.borrow() { break; }
            }
            _ = tokio::time::sleep(interval) => {
                if is_cancelled(&mut cancel_rx).await { break; }

                // In production, this would check for active downloads
                // and only emit if there are any. For now, this is a no-op
                // placeholder that demonstrates the pattern.
                let downloads = collect_downloads(&state).await;
                for dl in downloads {
                    let _ = tx.send(dl).await;
                }
            }
        }
    }
}

/// Companion status emitter task (5s interval by default).
pub async fn companion_status_emitter(
    state: Arc<AppState>,
    interval: Duration,
    startup_delay: Duration,
    mut cancel_rx: watch::Receiver<bool>,
    tx: mpsc::Sender<CompanionPayload>,
) {
    tokio::time::sleep(startup_delay).await;
    if is_cancelled(&mut cancel_rx).await {
        return;
    }

    loop {
        tokio::select! {
            _ = cancel_rx.changed() => {
                if *cancel_rx.borrow() { break; }
            }
            _ = tokio::time::sleep(interval) => {
                if is_cancelled(&mut cancel_rx).await { break; }

                let payload = collect_companion_status(&state).await;
                let _ = tx.send(payload).await;
            }
        }
    }
}

// ─── Data Collection Functions ───────────────────────────────────────────────

/// Collect current node status from the registry.
async fn collect_node_status(state: &AppState) -> Vec<NodeSnapshot> {
    let registry_guard = state.network_registry.read().await;
    match registry_guard.as_ref() {
        Some(registry) => {
            let nodes = registry.all_nodes().await;
            nodes
                .iter()
                .map(|node| NodeSnapshot {
                    node_id: node.capabilities.node_id.to_string(),
                    hostname: node.capabilities.hostname.clone(),
                    device_type: format!("{:?}", node.capabilities.device_type),
                    online: node.is_online,
                    cpu_percent: node.utilization.cpu_percent as f64,
                    ram_used_mb: node.utilization.ram_used_mb,
                    ram_total_mb: node.capabilities.ram.total_mb,
                    vram_used_mb: node.utilization.vram_used_mb.unwrap_or(0),
                    vram_total_mb: node
                        .capabilities
                        .gpu
                        .as_ref()
                        .map(|g| g.vram_mb)
                        .unwrap_or(0),
                    models_loaded: node
                        .loaded_models
                        .iter()
                        .map(|m| m.model_id.clone())
                        .collect(),
                })
                .collect()
        }
        None => Vec::new(),
    }
}

/// Collect current placement plan.
async fn collect_placement(
    state: &AppState,
    last_plan_id: &mut Option<String>,
) -> PlacementPayload {
    let opt_state = state.optimizer_state.read().await;
    match &opt_state.current_plan {
        Some(plan) => {
            let is_new = last_plan_id.as_ref() != Some(&plan.plan_id);
            *last_plan_id = Some(plan.plan_id.clone());
            PlacementPayload {
                plan_id: plan.plan_id.clone(),
                utility_score: plan.utility_score,
                created_at_ms: plan.created_at_ms,
                is_new_plan: is_new,
            }
        }
        None => PlacementPayload {
            plan_id: String::new(),
            utility_score: 0.0,
            created_at_ms: 0,
            is_new_plan: false,
        },
    }
}

/// Collect transport health data.
async fn collect_transport_health(state: &AppState) -> TransportHealthPayload {
    let transport_guard = state.transport_manager.read().await;
    match transport_guard.as_ref() {
        Some(_manager) => {
            // In production, query the transport manager for adapter/path status.
            // For now, return empty payload.
            TransportHealthPayload {
                adapters: Vec::new(),
                paths: Vec::new(),
                timestamp_ms: now_ms(),
            }
        }
        None => TransportHealthPayload {
            adapters: Vec::new(),
            paths: Vec::new(),
            timestamp_ms: now_ms(),
        },
    }
}

/// Collect utility scores and compute trend.
async fn collect_utility(state: &AppState, history: &mut VecDeque<f64>) -> UtilityPayload {
    let opt_state = state.optimizer_state.read().await;
    let total = opt_state.last_utility_score;

    history.push_back(total);
    if history.len() > 60 {
        history.pop_front();
    }

    let trend = compute_trend(history).to_string();

    UtilityPayload {
        quality: total * 0.4,
        speed: total * 0.3,
        coverage: total * 0.3,
        total,
        trend,
        timestamp_ms: now_ms(),
    }
}

/// Collect active downloads (placeholder — no download manager in AppState yet).
async fn collect_downloads(_state: &AppState) -> Vec<DownloadProgressPayload> {
    // In production, this would query a DownloadManager service.
    // Returns empty when no downloads are active.
    Vec::new()
}

/// Collect companion phone status.
async fn collect_companion_status(state: &AppState) -> CompanionPayload {
    let companion_guard = state.companion_service.read().await;
    match companion_guard.as_ref() {
        Some(_service) => {
            // In production, query the companion service for phone status.
            CompanionPayload {
                phones: Vec::new(),
                timestamp_ms: now_ms(),
            }
        }
        None => CompanionPayload {
            phones: Vec::new(),
            timestamp_ms: now_ms(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_emitter_config_defaults() {
        let config = EmitterConfig::default();
        assert_eq!(config.node_interval_ms, 2000);
        assert_eq!(config.plan_interval_ms, 10000);
        assert_eq!(config.transport_interval_ms, 5000);
        assert_eq!(config.utility_interval_ms, 5000);
        assert_eq!(config.download_interval_ms, 500);
        assert_eq!(config.companion_interval_ms, 5000);
        assert_eq!(config.startup_delay_ms, 2000);
        assert_eq!(config.full_sync_every_n, 5);
    }

    #[tokio::test]
    async fn test_emitter_service_stop_cancels() {
        let config = EmitterConfig {
            startup_delay_ms: 0,
            node_interval_ms: 50,
            ..EmitterConfig::default()
        };
        let state = Arc::new(AppState::new());
        let (node_tx, mut node_rx) = mpsc::channel(16);
        let (placement_tx, _) = mpsc::channel(16);
        let (transport_tx, _) = mpsc::channel(16);
        let (utility_tx, _) = mpsc::channel(16);
        let (download_tx, _) = mpsc::channel(16);
        let (companion_tx, _) = mpsc::channel(16);

        let mut service = EventEmitterService::new(config);
        service.start(
            state,
            node_tx,
            placement_tx,
            transport_tx,
            utility_tx,
            download_tx,
            companion_tx,
        );

        // Wait for at least one emission
        let payload = tokio::time::timeout(Duration::from_millis(200), node_rx.recv()).await;
        assert!(payload.is_ok());

        // Stop and verify tasks wind down
        service.stop();
        tokio::time::sleep(Duration::from_millis(100)).await;

        // After stop, no more emissions should arrive
        let result = tokio::time::timeout(Duration::from_millis(200), node_rx.recv()).await;
        // Either timeout or channel closed is acceptable
        if let Ok(Some(_)) = result {
            // One more might have been in-flight, but subsequent should stop
            let result2 = tokio::time::timeout(Duration::from_millis(200), node_rx.recv()).await;
            assert!(result2.is_err() || result2.unwrap().is_none());
        }
    }

    #[tokio::test]
    async fn test_node_emitter_produces_payload() {
        let state = Arc::new(AppState::new());
        let (tx, mut rx) = mpsc::channel(16);
        let (_cancel_tx, cancel_rx) = watch::channel(false);

        let handle = tokio::spawn(node_status_emitter(
            state,
            Duration::from_millis(50),
            5,
            Duration::from_millis(0),
            cancel_rx,
            tx,
        ));

        let payload = tokio::time::timeout(Duration::from_millis(200), rx.recv())
            .await
            .expect("should receive payload")
            .expect("channel should not be closed");

        // First emission is always a full sync (previous_snapshot is None)
        assert!(payload.is_full_sync);
        assert!(payload.timestamp_ms > 0);

        handle.abort();
    }

    #[tokio::test]
    async fn test_node_emitter_full_sync_every_n() {
        let state = Arc::new(AppState::new());
        let (tx, mut rx) = mpsc::channel(32);
        let (cancel_tx, cancel_rx) = watch::channel(false);

        let handle = tokio::spawn(node_status_emitter(
            state,
            Duration::from_millis(20),
            3, // full sync every 3rd
            Duration::from_millis(0),
            cancel_rx,
            tx,
        ));

        // Collect 6 emissions
        let mut payloads = Vec::new();
        for _ in 0..6 {
            if let Ok(Some(p)) = tokio::time::timeout(Duration::from_millis(200), rx.recv()).await {
                payloads.push(p);
            }
        }

        let _ = cancel_tx.send(true);
        handle.abort();

        // First is always full sync, then every 3rd (cycle 3, 6, ...)
        assert!(payloads[0].is_full_sync); // cycle 1, previous is None
        // cycle 2: delta
        assert!(!payloads[1].is_full_sync);
        // cycle 3: full sync (3 % 3 == 0)
        assert!(payloads[2].is_full_sync);
    }

    #[tokio::test]
    async fn test_placement_emitter_produces_payload() {
        let state = Arc::new(AppState::new());
        let (tx, mut rx) = mpsc::channel(16);
        let (_cancel_tx, cancel_rx) = watch::channel(false);

        let handle = tokio::spawn(placement_emitter(
            state,
            Duration::from_millis(50),
            Duration::from_millis(0),
            cancel_rx,
            tx,
        ));

        let payload = tokio::time::timeout(Duration::from_millis(200), rx.recv())
            .await
            .expect("should receive payload")
            .expect("channel should not be closed");

        // No plan set, so empty plan_id
        assert!(payload.plan_id.is_empty());
        assert_eq!(payload.utility_score, 0.0);

        handle.abort();
    }

    #[tokio::test]
    async fn test_utility_emitter_produces_payload() {
        let state = Arc::new(AppState::new());
        let (tx, mut rx) = mpsc::channel(16);
        let (_cancel_tx, cancel_rx) = watch::channel(false);

        let handle = tokio::spawn(utility_emitter(
            state,
            Duration::from_millis(50),
            Duration::from_millis(0),
            cancel_rx,
            tx,
        ));

        let payload = tokio::time::timeout(Duration::from_millis(200), rx.recv())
            .await
            .expect("should receive payload")
            .expect("channel should not be closed");

        assert_eq!(payload.trend, "stable");
        assert!(payload.timestamp_ms > 0);

        handle.abort();
    }

    #[tokio::test]
    async fn test_transport_emitter_produces_payload() {
        let state = Arc::new(AppState::new());
        let (tx, mut rx) = mpsc::channel(16);
        let (_cancel_tx, cancel_rx) = watch::channel(false);

        let handle = tokio::spawn(transport_health_emitter(
            state,
            Duration::from_millis(50),
            Duration::from_millis(0),
            cancel_rx,
            tx,
        ));

        let payload = tokio::time::timeout(Duration::from_millis(200), rx.recv())
            .await
            .expect("should receive payload")
            .expect("channel should not be closed");

        assert!(payload.timestamp_ms > 0);

        handle.abort();
    }

    #[tokio::test]
    async fn test_companion_emitter_produces_payload() {
        let state = Arc::new(AppState::new());
        let (tx, mut rx) = mpsc::channel(16);
        let (_cancel_tx, cancel_rx) = watch::channel(false);

        let handle = tokio::spawn(companion_status_emitter(
            state,
            Duration::from_millis(50),
            Duration::from_millis(0),
            cancel_rx,
            tx,
        ));

        let payload = tokio::time::timeout(Duration::from_millis(200), rx.recv())
            .await
            .expect("should receive payload")
            .expect("channel should not be closed");

        assert!(payload.phones.is_empty());
        assert!(payload.timestamp_ms > 0);

        handle.abort();
    }

    #[tokio::test]
    async fn test_download_emitter_no_active_downloads() {
        let state = Arc::new(AppState::new());
        let (tx, mut rx) = mpsc::channel(16);
        let (cancel_tx, cancel_rx) = watch::channel(false);

        let handle = tokio::spawn(download_progress_emitter(
            state,
            Duration::from_millis(50),
            Duration::from_millis(0),
            cancel_rx,
            tx,
        ));

        // No downloads active, so nothing should be sent
        let result = tokio::time::timeout(Duration::from_millis(150), rx.recv()).await;
        // Should timeout since no downloads are active
        assert!(result.is_err());

        let _ = cancel_tx.send(true);
        handle.abort();
    }
}
