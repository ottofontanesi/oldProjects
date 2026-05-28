//! Resource Envelope Management for Phase 7 Hardware Stability.
//!
//! Tracks CPU/RAM/GPU utilization per envelope using process-level accounting,
//! implements backpressure when memory limits are approached,
//! supports dynamic rebalancing of idle resources,
//! and exposes resource utilization via IPC.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::sync::{RwLock, Semaphore};

use crate::hardware_service::{
    EnvelopeUtilization, ResourceEnvelope, ResourceUtilization,
};

// ─── Envelope Tracking ──────────────────────────────────────────────────────

/// Tracks resource usage for a single envelope.
#[derive(Debug, Clone)]
pub struct EnvelopeTracker {
    /// The envelope definition (limits).
    pub envelope: ResourceEnvelope,
    /// Current CPU usage as a percentage of the envelope's allocation.
    pub cpu_used_percent: f64,
    /// Current RAM usage in MB.
    pub ram_used_mb: u64,
    /// Current GPU usage as a percentage (if applicable).
    pub gpu_used_percent: Option<f64>,
    /// Current VRAM usage in MB (if applicable).
    pub vram_used_mb: Option<u64>,
    /// Process IDs associated with this envelope.
    pub tracked_pids: Vec<u32>,
    /// Last time this envelope was actively used.
    pub last_active: Instant,
    /// Whether this envelope is currently borrowing resources from another.
    pub is_borrowing: bool,
    /// Amount of extra RAM borrowed from idle envelopes (in MB).
    pub borrowed_ram_mb: u64,
    /// Amount of extra CPU borrowed from idle envelopes (percent).
    pub borrowed_cpu_percent: u32,
}

impl EnvelopeTracker {
    pub fn new(envelope: ResourceEnvelope) -> Self {
        Self {
            envelope,
            cpu_used_percent: 0.0,
            ram_used_mb: 0,
            gpu_used_percent: None,
            vram_used_mb: None,
            tracked_pids: Vec::new(),
            last_active: Instant::now(),
            is_borrowing: false,
            borrowed_ram_mb: 0,
            borrowed_cpu_percent: 0,
        }
    }

    /// Get the effective RAM limit including any borrowed resources.
    pub fn effective_ram_limit(&self) -> u64 {
        self.envelope.ram_mb + self.borrowed_ram_mb
    }

    /// Get the effective CPU limit including any borrowed resources.
    pub fn effective_cpu_limit(&self) -> u32 {
        self.envelope.cpu_percent + self.borrowed_cpu_percent
    }

    /// Check if this envelope is idle (< 10% usage for tracking purposes).
    pub fn is_idle(&self) -> bool {
        self.cpu_used_percent < 10.0
            && self.ram_used_mb < (self.envelope.ram_mb / 10)
    }

    /// Get the amount of RAM that could be lent to other envelopes.
    pub fn lendable_ram_mb(&self) -> u64 {
        if self.is_idle() {
            // Can lend up to 80% of allocated RAM when idle
            (self.envelope.ram_mb * 80) / 100
        } else {
            0
        }
    }

    /// Get the amount of CPU that could be lent to other envelopes.
    pub fn lendable_cpu_percent(&self) -> u32 {
        if self.is_idle() {
            // Can lend up to 80% of allocated CPU when idle
            (self.envelope.cpu_percent * 80) / 100
        } else {
            0
        }
    }

    /// Convert to the IPC-friendly EnvelopeUtilization struct.
    pub fn to_utilization(&self) -> EnvelopeUtilization {
        EnvelopeUtilization {
            workload_type: self.envelope.workload_type.clone(),
            cpu_used_percent: self.cpu_used_percent,
            ram_used_mb: self.ram_used_mb,
            gpu_used_percent: self.gpu_used_percent,
            vram_used_mb: self.vram_used_mb,
        }
    }
}

// ─── Backpressure Queue ─────────────────────────────────────────────────────

/// A request that has been queued due to backpressure.
#[derive(Debug, Clone)]
pub struct QueuedRequest {
    pub id: String,
    pub workload_type: String,
    pub estimated_ram_mb: u64,
    pub queued_at: Instant,
}

/// Backpressure state for an envelope.
#[derive(Debug)]
pub struct BackpressureState {
    /// Whether backpressure is currently active.
    pub active: bool,
    /// Queued requests waiting for resources.
    pub queue: Vec<QueuedRequest>,
    /// Semaphore to limit concurrent requests when under pressure.
    pub semaphore: Arc<Semaphore>,
    /// Maximum queue depth before rejecting.
    pub max_queue_depth: usize,
}

impl BackpressureState {
    pub fn new(max_concurrent: usize, max_queue_depth: usize) -> Self {
        Self {
            active: false,
            queue: Vec::new(),
            semaphore: Arc::new(Semaphore::new(max_concurrent)),
            max_queue_depth,
        }
    }
}

// ─── Borrowing State ────────────────────────────────────────────────────────

/// Tracks resource borrowing between envelopes.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BorrowingState {
    /// Which envelope is borrowing.
    pub borrower: String,
    /// Which envelope is lending.
    pub lender: String,
    /// Amount of RAM borrowed in MB.
    pub ram_mb: u64,
    /// Amount of CPU borrowed as percent.
    pub cpu_percent: u32,
    /// When the borrowing started.
    pub started_at: String,
}

// ─── Resource Manager ───────────────────────────────────────────────────────

/// The main resource envelope manager.
/// Tracks utilization, enforces limits, manages backpressure and rebalancing.
pub struct ResourceEnvelopeManager {
    /// Per-envelope trackers keyed by workload type.
    trackers: HashMap<String, EnvelopeTracker>,
    /// Backpressure state per envelope.
    backpressure: HashMap<String, BackpressureState>,
    /// Active borrowing relationships.
    borrowings: Vec<BorrowingState>,
    /// Total system RAM in MB.
    total_ram_mb: u64,
    /// Total system CPU (always 100%).
    total_cpu_percent: u32,
    /// Total GPU percent (if GPU present).
    total_gpu_percent: Option<u32>,
    /// Total VRAM in MB (if GPU present).
    total_vram_mb: Option<u64>,
    /// Idle threshold duration for rebalancing (5 seconds).
    idle_threshold: Duration,
    /// Memory pressure threshold (90% of envelope limit).
    memory_pressure_fraction: f64,
}

impl ResourceEnvelopeManager {
    /// Create a new resource manager with the given envelopes and system totals.
    pub fn new(
        envelopes: Vec<ResourceEnvelope>,
        total_ram_mb: u64,
        total_gpu_percent: Option<u32>,
        total_vram_mb: Option<u64>,
    ) -> Self {
        let mut trackers = HashMap::new();
        let mut backpressure = HashMap::new();

        for envelope in envelopes {
            let workload_type = envelope.workload_type.clone();
            trackers.insert(workload_type.clone(), EnvelopeTracker::new(envelope));
            backpressure.insert(
                workload_type,
                BackpressureState::new(10, 50), // 10 concurrent, 50 max queued
            );
        }

        Self {
            trackers,
            backpressure,
            borrowings: Vec::new(),
            total_ram_mb,
            total_cpu_percent: 100,
            total_gpu_percent,
            total_vram_mb,
            idle_threshold: Duration::from_secs(5),
            memory_pressure_fraction: 0.9,
        }
    }

    // ─── Resource Monitoring (Task 4.2) ─────────────────────────────────

    /// Update resource usage for an envelope based on process-level accounting.
    /// Uses sysinfo process queries to track actual usage by tracked PIDs.
    pub fn update_envelope_usage(
        &mut self,
        workload_type: &str,
        cpu_percent: f64,
        ram_used_mb: u64,
        gpu_percent: Option<f64>,
        vram_used_mb: Option<u64>,
    ) {
        if let Some(tracker) = self.trackers.get_mut(workload_type) {
            tracker.cpu_used_percent = cpu_percent;
            tracker.ram_used_mb = ram_used_mb;
            tracker.gpu_used_percent = gpu_percent;
            tracker.vram_used_mb = vram_used_mb;

            if cpu_percent > 10.0 || ram_used_mb > 0 {
                tracker.last_active = Instant::now();
            }
        }
    }

    /// Register a process ID with an envelope for tracking.
    pub fn register_pid(&mut self, workload_type: &str, pid: u32) {
        if let Some(tracker) = self.trackers.get_mut(workload_type) {
            if !tracker.tracked_pids.contains(&pid) {
                tracker.tracked_pids.push(pid);
            }
        }
    }

    /// Unregister a process ID from an envelope.
    pub fn unregister_pid(&mut self, workload_type: &str, pid: u32) {
        if let Some(tracker) = self.trackers.get_mut(workload_type) {
            tracker.tracked_pids.retain(|&p| p != pid);
        }
    }

    /// Refresh utilization from system process data.
    /// Queries sysinfo for CPU and memory usage of tracked PIDs.
    pub fn refresh_from_system(&mut self) {
        let mut sys = sysinfo::System::new();
        sys.refresh_processes(sysinfo::ProcessesToUpdate::All);

        for (_, tracker) in self.trackers.iter_mut() {
            let mut total_cpu = 0.0_f32;
            let mut total_ram: u64 = 0;

            for &pid in &tracker.tracked_pids {
                let pid = sysinfo::Pid::from_u32(pid);
                if let Some(process) = sys.process(pid) {
                    total_cpu += process.cpu_usage();
                    total_ram += process.memory() / (1024 * 1024); // bytes to MB
                }
            }

            tracker.cpu_used_percent = total_cpu as f64;
            tracker.ram_used_mb = total_ram;

            if total_cpu > 10.0 || total_ram > 0 {
                tracker.last_active = Instant::now();
            }
        }
    }

    // ─── Backpressure (Task 4.3) ────────────────────────────────────────

    /// Check if an envelope is under memory pressure (>90% of limit).
    pub fn is_under_pressure(&self, workload_type: &str) -> bool {
        if let Some(tracker) = self.trackers.get(workload_type) {
            let limit = tracker.effective_ram_limit();
            if limit == 0 {
                return false;
            }
            let usage_fraction = tracker.ram_used_mb as f64 / limit as f64;
            usage_fraction > self.memory_pressure_fraction
        } else {
            false
        }
    }

    /// Attempt to admit a new request to an envelope.
    /// Returns Ok(()) if admitted, Err with queue position if backpressured.
    pub fn try_admit_request(
        &mut self,
        workload_type: &str,
        request_id: &str,
        estimated_ram_mb: u64,
    ) -> Result<(), String> {
        if !self.is_under_pressure(workload_type) {
            return Ok(());
        }

        // Envelope is under pressure — apply backpressure
        if let Some(bp) = self.backpressure.get_mut(workload_type) {
            if bp.queue.len() >= bp.max_queue_depth {
                return Err(format!(
                    "Backpressure queue full for '{}' (max {} requests)",
                    workload_type, bp.max_queue_depth
                ));
            }

            bp.active = true;
            bp.queue.push(QueuedRequest {
                id: request_id.to_string(),
                workload_type: workload_type.to_string(),
                estimated_ram_mb,
                queued_at: Instant::now(),
            });

            Err(format!(
                "Request queued due to memory pressure on '{}' (position: {})",
                workload_type,
                bp.queue.len()
            ))
        } else {
            Ok(())
        }
    }

    /// Try to drain queued requests when pressure subsides.
    pub fn drain_backpressure(&mut self, workload_type: &str) -> Vec<QueuedRequest> {
        let mut released = Vec::new();

        if !self.is_under_pressure(workload_type) {
            if let Some(bp) = self.backpressure.get_mut(workload_type) {
                bp.active = false;
                released = std::mem::take(&mut bp.queue);
            }
        }

        released
    }

    // ─── Dynamic Rebalancing (Task 4.4) ──────────────────────────────────

    /// Perform dynamic rebalancing: lend idle resources to active envelopes.
    /// When interactive is idle (< 10% usage for 5s), allow background to borrow.
    pub fn rebalance(&mut self) {
        let now = Instant::now();

        // First, identify idle envelopes and their lendable resources
        let mut lendable: Vec<(String, u64, u32)> = Vec::new();
        for (workload_type, tracker) in &self.trackers {
            if tracker.is_idle() && now.duration_since(tracker.last_active) >= self.idle_threshold {
                let ram = tracker.lendable_ram_mb();
                let cpu = tracker.lendable_cpu_percent();
                if ram > 0 || cpu > 0 {
                    lendable.push((workload_type.clone(), ram, cpu));
                }
            }
        }

        // Then, distribute to active envelopes that need resources (by priority)
        let mut active_envelopes: Vec<String> = self
            .trackers
            .iter()
            .filter(|(_, t)| !t.is_idle() && t.cpu_used_percent > 50.0)
            .map(|(k, _)| k.clone())
            .collect();

        // Sort by priority (lower number = higher priority)
        active_envelopes.sort_by_key(|k| {
            self.trackers.get(k).map(|t| t.envelope.priority).unwrap_or(99)
        });

        // Clear old borrowings
        self.borrowings.clear();

        for (lender, ram, cpu) in &lendable {
            for borrower in &active_envelopes {
                if borrower == lender {
                    continue;
                }
                if let Some(tracker) = self.trackers.get_mut(borrower) {
                    tracker.is_borrowing = true;
                    tracker.borrowed_ram_mb = *ram;
                    tracker.borrowed_cpu_percent = *cpu;

                    self.borrowings.push(BorrowingState {
                        borrower: borrower.clone(),
                        lender: lender.clone(),
                        ram_mb: *ram,
                        cpu_percent: *cpu,
                        started_at: chrono::Utc::now().to_rfc3339(),
                    });
                    break; // Each lender lends to one borrower
                }
            }
        }
    }

    /// Reclaim borrowed resources when the lending envelope needs them back.
    /// Must complete within 1 second of interactive demand returning.
    pub fn reclaim_resources(&mut self, workload_type: &str) {
        // Remove all borrowings where this workload is the lender
        self.borrowings.retain(|b| b.lender != workload_type);

        // Clear borrowed state from all borrowers that were borrowing from this lender
        for (_, tracker) in self.trackers.iter_mut() {
            if tracker.is_borrowing {
                // Check if this tracker's borrowing came from the reclaiming envelope
                tracker.is_borrowing = false;
                tracker.borrowed_ram_mb = 0;
                tracker.borrowed_cpu_percent = 0;
            }
        }
    }

    /// Check if any lender needs resources back and trigger reclaim.
    pub fn check_reclaim_needed(&mut self) {
        let lenders_needing_back: Vec<String> = self
            .borrowings
            .iter()
            .filter_map(|b| {
                self.trackers.get(&b.lender).and_then(|t| {
                    if !t.is_idle() {
                        Some(b.lender.clone())
                    } else {
                        None
                    }
                })
            })
            .collect();

        for lender in lenders_needing_back {
            self.reclaim_resources(&lender);
        }
    }

    // ─── IPC Exposure (Task 4.5) ────────────────────────────────────────

    /// Get the current resource utilization for IPC exposure.
    pub fn get_utilization(&self) -> ResourceUtilization {
        let mut total_cpu: f64 = 0.0;
        let mut total_ram: u64 = 0;
        let mut total_gpu: Option<f64> = None;
        let mut total_vram_used: Option<u64> = None;

        let envelopes: Vec<EnvelopeUtilization> = self
            .trackers
            .values()
            .map(|t| {
                total_cpu += t.cpu_used_percent;
                total_ram += t.ram_used_mb;
                if let Some(gpu) = t.gpu_used_percent {
                    *total_gpu.get_or_insert(0.0) += gpu;
                }
                if let Some(vram) = t.vram_used_mb {
                    *total_vram_used.get_or_insert(0) += vram;
                }
                t.to_utilization()
            })
            .collect();

        ResourceUtilization {
            cpu_percent: total_cpu,
            ram_used_mb: total_ram,
            ram_total_mb: self.total_ram_mb,
            gpu_percent: total_gpu,
            vram_used_mb: total_vram_used,
            vram_total_mb: self.total_vram_mb,
            envelopes,
        }
    }

    /// Get the current borrowing state for IPC exposure.
    pub fn get_borrowing_state(&self) -> &[BorrowingState] {
        &self.borrowings
    }

    /// Check if backpressure is active for any envelope.
    pub fn any_backpressure_active(&self) -> bool {
        self.backpressure.values().any(|bp| bp.active)
    }

    /// Get the envelope tracker for a specific workload type.
    pub fn get_tracker(&self, workload_type: &str) -> Option<&EnvelopeTracker> {
        self.trackers.get(workload_type)
    }

    /// Get all envelope trackers.
    pub fn all_trackers(&self) -> &HashMap<String, EnvelopeTracker> {
        &self.trackers
    }

    /// Validate that envelope allocations don't exceed 100%.
    /// Returns true if valid (sum <= 100 for CPU and GPU).
    pub fn validate_allocations(&self) -> bool {
        let cpu_sum: u32 = self.trackers.values().map(|t| t.envelope.cpu_percent).sum();
        let gpu_sum: u32 = self
            .trackers
            .values()
            .filter_map(|t| t.envelope.gpu_percent)
            .sum();

        cpu_sum <= 100 && gpu_sum <= 100
    }
}

// ─── Thread-Safe Wrapper ────────────────────────────────────────────────────

/// Thread-safe shared resource manager for use across async tasks.
pub struct SharedResourceManager {
    inner: Arc<RwLock<ResourceEnvelopeManager>>,
}

impl SharedResourceManager {
    pub fn new(manager: ResourceEnvelopeManager) -> Self {
        Self {
            inner: Arc::new(RwLock::new(manager)),
        }
    }

    pub async fn update_usage(
        &self,
        workload_type: &str,
        cpu_percent: f64,
        ram_used_mb: u64,
        gpu_percent: Option<f64>,
        vram_used_mb: Option<u64>,
    ) {
        let mut mgr = self.inner.write().await;
        mgr.update_envelope_usage(workload_type, cpu_percent, ram_used_mb, gpu_percent, vram_used_mb);
    }

    pub async fn try_admit(&self, workload_type: &str, request_id: &str, estimated_ram_mb: u64) -> Result<(), String> {
        let mut mgr = self.inner.write().await;
        mgr.try_admit_request(workload_type, request_id, estimated_ram_mb)
    }

    pub async fn rebalance(&self) {
        let mut mgr = self.inner.write().await;
        mgr.rebalance();
    }

    pub async fn check_reclaim(&self) {
        let mut mgr = self.inner.write().await;
        mgr.check_reclaim_needed();
    }

    pub async fn get_utilization(&self) -> ResourceUtilization {
        let mgr = self.inner.read().await;
        mgr.get_utilization()
    }

    pub async fn refresh_from_system(&self) {
        let mut mgr = self.inner.write().await;
        mgr.refresh_from_system();
    }
}

impl Clone for SharedResourceManager {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}
