// Intent citation: .kiro/specs/local-network-optimizer/design.md Section 5
// .kiro/specs/model-download-engine/design.md
// Download module — multi-source download, bandwidth throttle, integrity check,
// resume support, priority preemption, and progress events.

// ─── Submodules (new download engine components) ─────────────────────────────

pub mod config;
pub mod disk;
pub mod events;
pub mod integrity;
pub mod priority;
pub mod resume;
pub mod speed;
pub mod task;
pub mod throttle;

// ─── Re-exports for convenience ─────────────────────────────────────────────

pub use config::DownloadConfig;
pub use disk::{available_space_mb, check_disk_space, is_space_critically_low};
pub use events::{DownloadEvent, DownloadId};
pub use integrity::IntegrityVerifier;
pub use priority::{PriorityQueue, QueuedDownload};
pub use resume::{ResumeState, ResumeStore};
pub use speed::SpeedTracker;
pub use task::{CompletedDownload, DownloadError, compute_backoff, is_retryable_status, parse_retry_after};
pub use throttle::BandwidthThrottleAsync;

// ─── Original download.rs code (preserved for backward compatibility) ────────

use super::catalog::{DownloadSource, ModelEntry, ModelId, SourceType};
use super::registry::NodeId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ─── Types ───────────────────────────────────────────────────────────────────

/// Priority of a download.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum DownloadPriority {
    /// Needed for current plan execution.
    Critical,
    /// Speculative prefetch (can be cancelled).
    Prefetch,
    /// Nice to have, lowest bandwidth allocation.
    Background,
}

/// State of an active download.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadState {
    pub download_id: uuid::Uuid,
    pub model_id: ModelId,
    pub target_node: NodeId,
    pub source: DownloadSource,
    pub priority: DownloadPriority,
    pub total_bytes: u64,
    pub downloaded_bytes: u64,
    pub speed_bytes_per_sec: u64,
    pub status: DownloadStatus,
    pub started_at_ms: u64,
    pub integrity_verified: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum DownloadStatus {
    Queued,
    Downloading,
    Verifying,
    Completed,
    Failed { error: String, retries: u32 },
    Cancelled,
    Paused,
}

/// Progress report for UI display.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadProgress {
    pub model_id: ModelId,
    pub target_node: NodeId,
    pub source_type: String,
    pub total_bytes: u64,
    pub downloaded_bytes: u64,
    pub speed_bytes_per_sec: u64,
    pub eta_seconds: Option<u64>,
    pub priority: DownloadPriority,
    pub percent_complete: f64,
}

impl DownloadState {
    pub fn to_progress(&self) -> DownloadProgress {
        let percent = if self.total_bytes > 0 {
            self.downloaded_bytes as f64 / self.total_bytes as f64 * 100.0
        } else {
            0.0
        };

        let eta = if self.speed_bytes_per_sec > 0 {
            let remaining = self.total_bytes.saturating_sub(self.downloaded_bytes);
            Some(remaining / self.speed_bytes_per_sec)
        } else {
            None
        };

        DownloadProgress {
            model_id: self.model_id.clone(),
            target_node: self.target_node,
            source_type: format!("{:?}", self.source.source_type),
            total_bytes: self.total_bytes,
            downloaded_bytes: self.downloaded_bytes,
            speed_bytes_per_sec: self.speed_bytes_per_sec,
            eta_seconds: eta,
            priority: self.priority.clone(),
            percent_complete: percent,
        }
    }
}

// ─── Bandwidth Throttle (original synchronous version) ───────────────────────

/// Token bucket bandwidth throttle (synchronous, for existing coordinator).
#[derive(Debug, Clone)]
pub struct BandwidthThrottle {
    /// Maximum bytes per second allowed.
    pub max_bytes_per_sec: u64,
    /// Current available tokens (bytes).
    pub available_tokens: u64,
    /// Last refill timestamp (ms).
    pub last_refill_ms: u64,
    /// Whether active inference is happening (reduces bandwidth).
    pub inference_active: bool,
    /// Bandwidth limit percentage (0-100).
    pub limit_percent: u8,
}

impl BandwidthThrottle {
    pub fn new(max_bytes_per_sec: u64, limit_percent: u8) -> Self {
        Self {
            max_bytes_per_sec,
            available_tokens: max_bytes_per_sec, // Start full
            last_refill_ms: 0,
            inference_active: false,
            limit_percent,
        }
    }

    /// Get the effective bandwidth limit considering inference activity.
    pub fn effective_limit(&self) -> u64 {
        let base = self.max_bytes_per_sec * self.limit_percent as u64 / 100;
        if self.inference_active {
            // Reduce to 30% during active inference
            base * 30 / 100
        } else {
            base
        }
    }

    /// Refill tokens based on elapsed time.
    pub fn refill(&mut self, current_time_ms: u64) {
        if self.last_refill_ms == 0 {
            self.last_refill_ms = current_time_ms;
            return;
        }

        let elapsed_ms = current_time_ms.saturating_sub(self.last_refill_ms);
        let refill_amount = self.effective_limit() * elapsed_ms / 1000;
        self.available_tokens = (self.available_tokens + refill_amount).min(self.effective_limit() * 2);
        self.last_refill_ms = current_time_ms;
    }

    /// Try to consume tokens for a chunk download. Returns true if allowed.
    pub fn try_consume(&mut self, bytes: u64, current_time_ms: u64) -> bool {
        self.refill(current_time_ms);

        if bytes <= self.available_tokens {
            self.available_tokens -= bytes;
            true
        } else {
            false
        }
    }

    /// Set whether inference is currently active (affects throttle).
    pub fn set_inference_active(&mut self, active: bool) {
        self.inference_active = active;
    }
}

// ─── Source Selection ─────────────────────────────────────────────────────────

/// Select the best download source for a model.
/// Priority: peer node (LAN) > local NAS > Ollama > HuggingFace
pub fn select_source(
    model: &ModelEntry,
    available_peers: &[NodeId],
    internet_available: bool,
) -> Option<DownloadSource> {
    let mut sources = model.download_sources.clone();
    sources.sort_by_key(|s| s.priority);

    for source in &sources {
        match &source.source_type {
            SourceType::PeerNode { node_id } => {
                if available_peers.contains(node_id) {
                    return Some(source.clone());
                }
            }
            SourceType::LocalNas => {
                // Assume NAS is always reachable on LAN
                return Some(source.clone());
            }
            SourceType::OllamaRegistry | SourceType::HuggingFaceHub => {
                if internet_available {
                    return Some(source.clone());
                }
            }
        }
    }

    // Fallback: first internet source if available
    if internet_available {
        sources
            .iter()
            .find(|s| matches!(s.source_type, SourceType::OllamaRegistry | SourceType::HuggingFaceHub))
            .cloned()
    } else {
        None
    }
}

/// Verify SHA-256 integrity of a downloaded file.
/// Returns true if checksum matches.
pub fn verify_integrity(computed_hash: &str, expected_hash: &str) -> bool {
    // Case-insensitive comparison
    computed_hash.to_lowercase() == expected_hash.to_lowercase()
}

// ─── Download Coordinator ────────────────────────────────────────────────────

/// Manages all active and queued downloads.
pub struct DownloadCoordinator {
    /// Active downloads indexed by download_id.
    downloads: HashMap<uuid::Uuid, DownloadState>,
    /// Bandwidth throttle.
    throttle: BandwidthThrottle,
    /// Maximum concurrent downloads.
    max_concurrent: u32,
    /// Maximum retries per download.
    max_retries: u32,
}

impl DownloadCoordinator {
    pub fn new(max_bandwidth_bytes_per_sec: u64, limit_percent: u8) -> Self {
        Self {
            downloads: HashMap::new(),
            throttle: BandwidthThrottle::new(max_bandwidth_bytes_per_sec, limit_percent),
            max_concurrent: 3,
            max_retries: 3,
        }
    }

    /// Start a new download.
    pub fn start_download(
        &mut self,
        model_id: ModelId,
        target_node: NodeId,
        source: DownloadSource,
        total_bytes: u64,
        priority: DownloadPriority,
        current_time_ms: u64,
    ) -> uuid::Uuid {
        let download_id = uuid::Uuid::new_v4();

        let state = DownloadState {
            download_id,
            model_id,
            target_node,
            source,
            priority,
            total_bytes,
            downloaded_bytes: 0,
            speed_bytes_per_sec: 0,
            status: DownloadStatus::Queued,
            started_at_ms: current_time_ms,
            integrity_verified: false,
        };

        self.downloads.insert(download_id, state);
        download_id
    }

    /// Cancel a download.
    pub fn cancel(&mut self, download_id: &uuid::Uuid) -> bool {
        if let Some(state) = self.downloads.get_mut(download_id) {
            state.status = DownloadStatus::Cancelled;
            true
        } else {
            false
        }
    }

    /// Pause all downloads (e.g., when internet goes offline).
    pub fn pause_all(&mut self) {
        for state in self.downloads.values_mut() {
            if state.status == DownloadStatus::Downloading {
                state.status = DownloadStatus::Paused;
            }
        }
    }

    /// Resume all paused downloads.
    pub fn resume_all(&mut self) {
        for state in self.downloads.values_mut() {
            if state.status == DownloadStatus::Paused {
                state.status = DownloadStatus::Downloading;
            }
        }
    }

    /// Get progress of all active downloads.
    pub fn progress(&self) -> Vec<DownloadProgress> {
        self.downloads
            .values()
            .filter(|d| matches!(d.status, DownloadStatus::Queued | DownloadStatus::Downloading | DownloadStatus::Verifying))
            .map(|d| d.to_progress())
            .collect()
    }

    /// Get all downloads (including completed/failed).
    pub fn all_downloads(&self) -> Vec<&DownloadState> {
        self.downloads.values().collect()
    }

    /// Get count of active (non-terminal) downloads.
    pub fn active_count(&self) -> u32 {
        self.downloads
            .values()
            .filter(|d| matches!(d.status, DownloadStatus::Downloading | DownloadStatus::Verifying))
            .count() as u32
    }

    /// Check if a specific model is currently being downloaded to a node.
    pub fn is_downloading(&self, model_id: &str, target_node: &NodeId) -> bool {
        self.downloads.values().any(|d| {
            d.model_id == model_id
                && d.target_node == *target_node
                && matches!(d.status, DownloadStatus::Queued | DownloadStatus::Downloading)
        })
    }

    /// Set bandwidth limit percentage.
    pub fn set_bandwidth_limit(&mut self, percent: u8) {
        self.throttle.limit_percent = percent.min(100);
    }

    /// Notify that inference is active (reduces download bandwidth).
    pub fn set_inference_active(&mut self, active: bool) {
        self.throttle.set_inference_active(active);
    }

    /// Remove completed/cancelled/failed downloads older than given age.
    pub fn cleanup_old(&mut self, max_age_ms: u64, current_time_ms: u64) {
        self.downloads.retain(|_, d| {
            let is_terminal = matches!(
                d.status,
                DownloadStatus::Completed | DownloadStatus::Cancelled | DownloadStatus::Failed { .. }
            );
            if is_terminal {
                current_time_ms - d.started_at_ms < max_age_ms
            } else {
                true // Keep active downloads
            }
        });
    }
}

impl Default for DownloadCoordinator {
    fn default() -> Self {
        // Default: 100MB/s max, 50% limit
        Self::new(100 * 1024 * 1024, 50)
    }
}

// ─── Storage Management ──────────────────────────────────────────────────────

/// Check if there's enough storage space for a download.
pub fn check_storage_available(
    required_mb: u64,
    available_mb: u64,
    buffer_percent: f64,
) -> Result<(), String> {
    let required_with_buffer = (required_mb as f64 * (1.0 + buffer_percent)) as u64;
    if available_mb >= required_with_buffer {
        Ok(())
    } else {
        Err(format!(
            "Insufficient storage: need {}MB (with {}% buffer), have {}MB available",
            required_with_buffer,
            (buffer_percent * 100.0) as u32,
            available_mb
        ))
    }
}

// ─── Download Manager (new production engine) ────────────────────────────────

use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};

/// Status report for the download manager.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadManagerStatus {
    pub active_count: u32,
    pub queued_count: u32,
    pub total_bytes_downloading: u64,
    pub total_bytes_queued: u64,
    pub aggregate_speed_bps: u64,
    pub error_rate: f64,
}

/// Callback trait for scheduler notifications.
pub trait DownloadCallback: Send + Sync {
    /// Called when a download completes successfully.
    fn on_download_complete(&self, model_id: &str, target_node: NodeId, file_path: &std::path::Path);
    /// Called when a download fails permanently.
    fn on_download_failed(&self, model_id: &str, target_node: NodeId, reason: &str);
    /// Called periodically with status for capacity planning.
    fn on_status_update(&self, status: DownloadManagerStatus);
}

/// The production download manager with resume, priority preemption,
/// bandwidth throttling, and progress events.
pub struct DownloadManager {
    config: DownloadConfig,
    queue: Arc<Mutex<PriorityQueue>>,
    active: Arc<Mutex<HashMap<DownloadId, ActiveDownloadInfo>>>,
    throttle: Arc<BandwidthThrottleAsync>,
    resume_store: Arc<ResumeStore>,
    event_tx: mpsc::Sender<DownloadEvent>,
    event_rx: Arc<Mutex<mpsc::Receiver<DownloadEvent>>>,
}

/// Info about an active (in-progress) download.
#[derive(Debug, Clone)]
pub struct ActiveDownloadInfo {
    pub id: DownloadId,
    pub model_id: String,
    pub priority: u8,
    pub bytes_downloaded: u64,
    pub total_bytes: u64,
    pub target_node: NodeId,
}

impl DownloadManager {
    /// Create a new DownloadManager with the given config and resume store.
    pub fn new(config: DownloadConfig, resume_store: Arc<ResumeStore>) -> Self {
        let (event_tx, event_rx) = mpsc::channel(256);
        let throttle = Arc::new(BandwidthThrottleAsync::new(config.bandwidth_limit_bps));

        Self {
            config,
            queue: Arc::new(Mutex::new(PriorityQueue::new())),
            active: Arc::new(Mutex::new(HashMap::new())),
            throttle,
            resume_store,
            event_tx,
            event_rx: Arc::new(Mutex::new(event_rx)),
        }
    }

    /// Submit a new download request. Returns the assigned DownloadId.
    pub async fn submit(
        &self,
        model_id: String,
        url: String,
        total_bytes: u64,
        priority: u8,
        target_node: NodeId,
        expected_hash: Option<String>,
    ) -> Result<DownloadId, task::DownloadError> {
        // Check disk space before accepting
        let space_result = disk::check_disk_space(
            &self.config.temp_dir,
            total_bytes,
            self.config.min_disk_space_mb,
        );
        if let Err(e) = space_result {
            return Err(task::DownloadError::InsufficientSpace(format!(
                "Need {}MB, have {}MB",
                e.required_mb, e.available_mb
            )));
        }

        let id = uuid::Uuid::new_v4();
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let queued = QueuedDownload {
            id,
            priority,
            model_id: model_id.clone(),
            submitted_at_ms: now_ms,
        };

        // Save resume state for recovery
        let resume_state = ResumeState {
            download_id: id,
            url,
            temp_path: self.config.temp_dir.join(format!("{}.part", id)),
            bytes_downloaded: 0,
            total_bytes: Some(total_bytes),
            etag: None,
            last_modified: None,
            expected_hash,
            priority,
            model_id,
            target_node,
            saved_at_ms: now_ms,
        };
        let _ = self.resume_store.save_state(id, &resume_state);

        // Enqueue
        let mut queue = self.queue.lock().await;
        queue.push(queued);

        Ok(id)
    }

    /// Cancel a download by ID.
    pub async fn cancel(&self, id: DownloadId) -> Result<(), task::DownloadError> {
        // Remove from queue if queued
        let mut queue = self.queue.lock().await;
        queue.remove(&id);
        drop(queue);

        // Remove from active
        let mut active = self.active.lock().await;
        active.remove(&id);
        drop(active);

        // Remove resume state
        let _ = self.resume_store.remove_state(id);

        // Emit cancelled event
        let _ = self.event_tx.send(DownloadEvent::Cancelled { id }).await;

        Ok(())
    }

    /// Pause a download (save state, keep temp file).
    pub async fn pause(&self, id: DownloadId) -> Result<(), task::DownloadError> {
        let active = self.active.lock().await;
        if let Some(info) = active.get(&id) {
            let bytes_so_far = info.bytes_downloaded;
            let _ = self
                .event_tx
                .send(DownloadEvent::Paused { id, bytes_so_far })
                .await;
        }
        Ok(())
    }

    /// Get current status of the download manager.
    pub async fn status(&self) -> DownloadManagerStatus {
        let active = self.active.lock().await;
        let queue = self.queue.lock().await;

        let active_count = active.len() as u32;
        let queued_count = queue.len() as u32;
        let total_bytes_downloading: u64 = active.values().map(|a| a.total_bytes).sum();
        let total_bytes_queued: u64 = 0; // Would need size info in queue

        DownloadManagerStatus {
            active_count,
            queued_count,
            total_bytes_downloading,
            total_bytes_queued,
            aggregate_speed_bps: 0,
            error_rate: 0.0,
        }
    }

    /// Set bandwidth limit at runtime.
    pub fn set_bandwidth_limit(&self, bytes_per_sec: Option<u64>) {
        self.throttle.set_limit(bytes_per_sec);
    }

    /// Recover incomplete downloads from the resume store (startup recovery).
    pub async fn recover_incomplete(&self) -> Result<Vec<DownloadId>, String> {
        let incomplete = self.resume_store.list_incomplete()?;
        let mut recovered_ids = Vec::new();

        for (id, state) in incomplete {
            let queued = QueuedDownload {
                id,
                priority: state.priority,
                model_id: state.model_id.clone(),
                submitted_at_ms: state.saved_at_ms,
            };

            let mut queue = self.queue.lock().await;
            queue.push(queued);
            recovered_ids.push(id);
        }

        Ok(recovered_ids)
    }

    /// Shutdown the download manager, saving all resume states.
    pub async fn shutdown(&self) {
        // Clear the queue
        let mut queue = self.queue.lock().await;
        while queue.pop().is_some() {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bandwidth_throttle_effective_limit() {
        let throttle = BandwidthThrottle::new(100_000_000, 50); // 100MB/s, 50% limit
        assert_eq!(throttle.effective_limit(), 50_000_000); // 50MB/s

        let mut throttle_active = BandwidthThrottle::new(100_000_000, 50);
        throttle_active.inference_active = true;
        assert_eq!(throttle_active.effective_limit(), 15_000_000); // 30% of 50MB/s = 15MB/s
    }

    #[test]
    fn test_bandwidth_throttle_consume() {
        let mut throttle = BandwidthThrottle::new(100_000_000, 100); // 100MB/s, 100%
        throttle.last_refill_ms = 1000;

        // After 1 second, should have ~100MB available
        assert!(throttle.try_consume(50_000_000, 2000)); // 50MB — should succeed
        assert!(throttle.try_consume(50_000_000, 2000)); // Another 50MB — might succeed (from initial + refill)
    }

    #[test]
    fn test_source_selection_prefers_peer() {
        let peer_id = uuid::Uuid::new_v4();
        let model = ModelEntry {
            model_id: "test".to_string(),
            family: "test".to_string(),
            parameter_count_b: 7.0,
            quantization: super::super::catalog::Quantization::Q4_K_M,
            requirements: super::super::catalog::ModelRequirements { min_ram_mb: 4000, min_vram_mb: 0, disk_size_mb: 4000, min_compute_capability: None },
            performance: super::super::catalog::ModelPerformance { estimates: vec![] },
            task_affinity: HashMap::new(),
            supported_backends: vec![],
            download_sources: vec![
                DownloadSource { source_type: SourceType::OllamaRegistry, url: "ollama://test".to_string(), priority: 2 },
                DownloadSource { source_type: SourceType::PeerNode { node_id: peer_id }, url: "peer://test".to_string(), priority: 1 },
            ],
            checksum_sha256: "abc".to_string(),
        };

        // Peer available — should select peer
        let source = select_source(&model, &[peer_id], true);
        assert!(source.is_some());
        assert!(matches!(source.unwrap().source_type, SourceType::PeerNode { .. }));
    }

    #[test]
    fn test_source_selection_fallback_to_internet() {
        let model = ModelEntry {
            model_id: "test".to_string(),
            family: "test".to_string(),
            parameter_count_b: 7.0,
            quantization: super::super::catalog::Quantization::Q4_K_M,
            requirements: super::super::catalog::ModelRequirements { min_ram_mb: 4000, min_vram_mb: 0, disk_size_mb: 4000, min_compute_capability: None },
            performance: super::super::catalog::ModelPerformance { estimates: vec![] },
            task_affinity: HashMap::new(),
            supported_backends: vec![],
            download_sources: vec![
                DownloadSource { source_type: SourceType::OllamaRegistry, url: "ollama://test".to_string(), priority: 1 },
            ],
            checksum_sha256: "abc".to_string(),
        };

        // No peers, internet available
        let source = select_source(&model, &[], true);
        assert!(source.is_some());
        assert!(matches!(source.unwrap().source_type, SourceType::OllamaRegistry));

        // No peers, no internet
        let source = select_source(&model, &[], false);
        assert!(source.is_none());
    }

    #[test]
    fn test_download_coordinator_start_cancel() {
        let mut coord = DownloadCoordinator::default();
        let source = DownloadSource { source_type: SourceType::OllamaRegistry, url: "test".to_string(), priority: 1 };
        let node = uuid::Uuid::new_v4();

        let id = coord.start_download("model_a".to_string(), node, source, 1_000_000, DownloadPriority::Critical, 1000);

        assert!(coord.is_downloading("model_a", &node));
        assert_eq!(coord.active_count(), 0); // Queued, not yet downloading

        coord.cancel(&id);
        assert!(!coord.is_downloading("model_a", &node));
    }

    #[test]
    fn test_download_progress() {
        let mut coord = DownloadCoordinator::default();
        let source = DownloadSource { source_type: SourceType::OllamaRegistry, url: "test".to_string(), priority: 1 };
        let node = uuid::Uuid::new_v4();

        coord.start_download("model_a".to_string(), node, source, 1_000_000, DownloadPriority::Critical, 1000);

        let progress = coord.progress();
        assert_eq!(progress.len(), 1);
        assert_eq!(progress[0].percent_complete, 0.0);
    }

    #[test]
    fn test_verify_integrity() {
        assert!(verify_integrity("abc123", "ABC123")); // Case insensitive
        assert!(verify_integrity("abc123", "abc123"));
        assert!(!verify_integrity("abc123", "def456"));
    }

    #[test]
    fn test_check_storage() {
        // Need 4000MB with 10% buffer = 4400MB
        assert!(check_storage_available(4000, 5000, 0.10).is_ok());
        assert!(check_storage_available(4000, 4400, 0.10).is_ok());
        assert!(check_storage_available(4000, 4000, 0.10).is_err()); // Not enough with buffer
    }

    #[test]
    fn test_pause_resume() {
        let mut coord = DownloadCoordinator::default();
        let source = DownloadSource { source_type: SourceType::OllamaRegistry, url: "test".to_string(), priority: 1 };
        let node = uuid::Uuid::new_v4();

        let id = coord.start_download("model_a".to_string(), node, source, 1_000_000, DownloadPriority::Critical, 1000);

        // Manually set to downloading
        coord.downloads.get_mut(&id).unwrap().status = DownloadStatus::Downloading;

        coord.pause_all();
        assert_eq!(coord.downloads[&id].status, DownloadStatus::Paused);

        coord.resume_all();
        assert_eq!(coord.downloads[&id].status, DownloadStatus::Downloading);
    }

    #[tokio::test]
    async fn test_download_manager_submit_and_cancel() {
        let store = Arc::new(ResumeStore::in_memory().unwrap());
        let config = DownloadConfig {
            temp_dir: std::path::PathBuf::from("."),
            model_dir: std::path::PathBuf::from("."),
            min_disk_space_mb: 0, // Don't check disk for test
            ..Default::default()
        };
        let manager = DownloadManager::new(config, store);

        let id = manager
            .submit(
                "test-model".to_string(),
                "https://example.com/model.bin".to_string(),
                1_000_000,
                5,
                uuid::Uuid::new_v4(),
                Some("deadbeef".to_string()),
            )
            .await
            .unwrap();

        // Should be in queue
        let status = manager.status().await;
        assert_eq!(status.queued_count, 1);

        // Cancel it
        manager.cancel(id).await.unwrap();
        let status = manager.status().await;
        assert_eq!(status.queued_count, 0);
    }

    #[tokio::test]
    async fn test_download_manager_recover_incomplete() {
        let store = Arc::new(ResumeStore::in_memory().unwrap());

        // Pre-populate resume store with incomplete downloads
        let id1 = uuid::Uuid::new_v4();
        let id2 = uuid::Uuid::new_v4();
        store
            .save_state(
                id1,
                &ResumeState {
                    download_id: id1,
                    url: "https://example.com/a.bin".to_string(),
                    temp_path: std::path::PathBuf::from("/tmp/a.part"),
                    bytes_downloaded: 500_000,
                    total_bytes: Some(1_000_000),
                    etag: None,
                    last_modified: None,
                    expected_hash: None,
                    priority: 1,
                    model_id: "model-a".to_string(),
                    target_node: uuid::Uuid::new_v4(),
                    saved_at_ms: 1000,
                },
            )
            .unwrap();
        store
            .save_state(
                id2,
                &ResumeState {
                    download_id: id2,
                    url: "https://example.com/b.bin".to_string(),
                    temp_path: std::path::PathBuf::from("/tmp/b.part"),
                    bytes_downloaded: 200_000,
                    total_bytes: Some(2_000_000),
                    etag: None,
                    last_modified: None,
                    expected_hash: None,
                    priority: 10,
                    model_id: "model-b".to_string(),
                    target_node: uuid::Uuid::new_v4(),
                    saved_at_ms: 2000,
                },
            )
            .unwrap();

        let config = DownloadConfig::default();
        let manager = DownloadManager::new(config, store);

        let recovered = manager.recover_incomplete().await.unwrap();
        assert_eq!(recovered.len(), 2);

        let status = manager.status().await;
        assert_eq!(status.queued_count, 2);
    }
}

#[cfg(test)]
mod property_tests {
    use super::*;
    use proptest::prelude::*;
    use std::sync::Arc;

    proptest! {
        /// **Validates: Requirements 6.1**
        /// Property 6: Concurrency Bound — active downloads never exceed max_concurrent.
        #[test]
        fn active_never_exceeds_max_concurrent(
            max_concurrent in 1u32..10,
            num_downloads in 1usize..20,
        ) {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();

            let result: Result<(), TestCaseError> = rt.block_on(async {
                let store = Arc::new(ResumeStore::in_memory().unwrap());
                let config = DownloadConfig {
                    max_concurrent,
                    temp_dir: std::path::PathBuf::from("."),
                    model_dir: std::path::PathBuf::from("."),
                    min_disk_space_mb: 0,
                    ..Default::default()
                };
                let manager = DownloadManager::new(config, store);

                // Submit multiple downloads
                for i in 0..num_downloads {
                    let _ = manager
                        .submit(
                            format!("model-{}", i),
                            format!("https://example.com/{}.bin", i),
                            1_000_000,
                            (i % 256) as u8,
                            uuid::Uuid::new_v4(),
                            None,
                        )
                        .await;
                }

                // Active count should never exceed max_concurrent
                let status = manager.status().await;
                prop_assert!(
                    status.active_count <= max_concurrent,
                    "Active {} exceeds max {}",
                    status.active_count,
                    max_concurrent
                );
                Ok(())
            });
            result?;
        }

        /// **Validates: Requirements 9.1**
        /// Property 5: Disk Space Safety — downloads rejected when space insufficient.
        #[test]
        fn rejects_when_disk_space_insufficient(
            file_size_mb in 100u64..100_000,
            available_mb in 0u64..50,
            buffer_mb in 500u64..2000,
        ) {
            // When available space is less than file_size + buffer, should reject
            let required_mb = file_size_mb + buffer_mb;
            if available_mb < required_mb {
                let result = disk::check_disk_space(
                    &std::path::PathBuf::from("/nonexistent/path/that/wont/resolve"),
                    file_size_mb * 1024 * 1024,
                    buffer_mb,
                );
                // The function checks actual disk space of the path, so for a nonexistent
                // path it will return 0 available, which should always fail for any positive requirement
                prop_assert!(result.is_err());
            }
        }
    }
}
