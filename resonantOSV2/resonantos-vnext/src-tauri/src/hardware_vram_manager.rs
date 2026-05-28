//! GPU VRAM Management for Phase 7 Hardware Stability.
//!
//! - Polls available VRAM at 5s intervals via NVML dynamic loading
//! - Maintains a VRAM allocation registry tracking model occupancy
//! - Supports priority-based eviction of cached models
//! - Pre-checks VRAM before model load, rejects with shortfall if insufficient
//! - Emits gpu-memory-pressure event when available VRAM < 15%

use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::hardware_gpu_detection::find_nvml_library;

// ─── VRAM State ─────────────────────────────────────────────────────────────

/// Current VRAM state as reported by the GPU driver.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VramState {
    /// Total VRAM in MB.
    pub total_mb: u64,
    /// Currently available (free) VRAM in MB.
    pub available_mb: u64,
    /// Currently used VRAM in MB.
    pub used_mb: u64,
    /// Whether memory pressure is active (available < 15% of total).
    pub pressure_active: bool,
    /// Last poll timestamp.
    pub last_polled: String,
}

impl VramState {
    /// Check if VRAM is under pressure (< 15% available).
    pub fn is_under_pressure(&self) -> bool {
        if self.total_mb == 0 {
            return false;
        }
        let available_fraction = self.available_mb as f64 / self.total_mb as f64;
        available_fraction < 0.15
    }

    /// Get the available fraction as a percentage.
    pub fn available_percent(&self) -> f64 {
        if self.total_mb == 0 {
            return 0.0;
        }
        (self.available_mb as f64 / self.total_mb as f64) * 100.0
    }
}

// ─── VRAM Allocation Registry ───────────────────────────────────────────────

/// A model currently loaded in VRAM.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VramAllocation {
    /// Unique identifier for this allocation.
    pub allocation_id: String,
    /// Model ID occupying VRAM.
    pub model_id: String,
    /// Model name for display.
    pub model_name: String,
    /// Amount of VRAM occupied in MB.
    pub vram_mb: u64,
    /// Priority (lower = higher priority, less likely to be evicted).
    pub priority: u32,
    /// When this model was loaded.
    pub loaded_at: String,
    /// When this model was last used for inference.
    pub last_used: String,
}

/// Result of a VRAM pre-check.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum VramPreCheckResult {
    /// Sufficient VRAM available.
    Available { available_mb: u64 },
    /// Insufficient VRAM — includes shortfall amount.
    Insufficient {
        available_mb: u64,
        required_mb: u64,
        shortfall_mb: u64,
    },
    /// Can be made available by evicting lower-priority models.
    EvictionRequired {
        available_mb: u64,
        required_mb: u64,
        eviction_candidates: Vec<String>,
        eviction_frees_mb: u64,
    },
    /// No GPU present.
    NoGpu,
}

/// Event emitted when VRAM pressure is detected.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VramPressureEvent {
    pub total_mb: u64,
    pub available_mb: u64,
    pub available_percent: f64,
    pub allocations: Vec<VramAllocation>,
    pub timestamp: String,
    pub new_loads_blocked: bool,
}

// ─── VRAM Manager ───────────────────────────────────────────────────────────

/// Manages GPU VRAM allocation, monitoring, and eviction.
pub struct VramManager {
    /// Current VRAM state from last poll.
    state: VramState,
    /// Registry of models currently in VRAM.
    allocations: Vec<VramAllocation>,
    /// Whether new model loads are blocked due to pressure.
    loads_blocked: bool,
    /// Poll interval (5 seconds).
    poll_interval: Duration,
    /// Last poll time.
    last_poll: Option<Instant>,
    /// Whether a GPU is available.
    gpu_available: bool,
    /// Pressure threshold (fraction of total below which pressure is active).
    pressure_threshold: f64,
    /// Next allocation ID counter.
    next_alloc_id: u64,
}

impl VramManager {
    /// Create a new VRAM manager.
    /// If no GPU is detected, the manager operates in no-op mode.
    pub fn new(total_vram_mb: u64, available_vram_mb: u64) -> Self {
        let gpu_available = total_vram_mb > 0;
        Self {
            state: VramState {
                total_mb: total_vram_mb,
                available_mb: available_vram_mb,
                used_mb: total_vram_mb.saturating_sub(available_vram_mb),
                pressure_active: false,
                last_polled: chrono::Utc::now().to_rfc3339(),
            },
            allocations: Vec::new(),
            loads_blocked: false,
            poll_interval: Duration::from_secs(5),
            last_poll: None,
            gpu_available,
            pressure_threshold: 0.15,
            next_alloc_id: 1,
        }
    }

    /// Create a manager with no GPU.
    pub fn no_gpu() -> Self {
        Self::new(0, 0)
    }

    /// Check if a GPU is available.
    pub fn has_gpu(&self) -> bool {
        self.gpu_available
    }

    // ─── VRAM Monitoring (Task 5.4) ─────────────────────────────────────

    /// Poll VRAM state from the GPU driver via NVML.
    /// Should be called at 5-second intervals.
    pub fn poll_vram(&mut self) -> Option<VramPressureEvent> {
        if !self.gpu_available {
            return None;
        }

        // Attempt to read VRAM via NVML
        if let Some((total, available)) = query_nvml_vram() {
            self.state.total_mb = total;
            self.state.available_mb = available;
            self.state.used_mb = total.saturating_sub(available);
            self.state.last_polled = chrono::Utc::now().to_rfc3339();
            self.last_poll = Some(Instant::now());
        }

        // Check pressure state
        let was_under_pressure = self.state.pressure_active;
        self.state.pressure_active = self.state.is_under_pressure();

        // Emit event if pressure just became active
        if self.state.pressure_active && !was_under_pressure {
            self.loads_blocked = true;
            return Some(VramPressureEvent {
                total_mb: self.state.total_mb,
                available_mb: self.state.available_mb,
                available_percent: self.state.available_percent(),
                allocations: self.allocations.clone(),
                timestamp: chrono::Utc::now().to_rfc3339(),
                new_loads_blocked: true,
            });
        }

        // Clear blocked state if pressure resolved
        if !self.state.pressure_active && was_under_pressure {
            self.loads_blocked = false;
        }

        None
    }

    /// Check if polling is due (5s since last poll).
    pub fn should_poll(&self) -> bool {
        match self.last_poll {
            Some(last) => last.elapsed() >= self.poll_interval,
            None => true,
        }
    }

    /// Get current VRAM state.
    pub fn current_state(&self) -> &VramState {
        &self.state
    }

    // ─── VRAM Allocation Registry (Task 5.5) ────────────────────────────

    /// Register a model as loaded in VRAM.
    pub fn register_allocation(
        &mut self,
        model_id: &str,
        model_name: &str,
        vram_mb: u64,
        priority: u32,
    ) -> String {
        let alloc_id = format!("vram-alloc-{}", self.next_alloc_id);
        self.next_alloc_id += 1;

        let now = chrono::Utc::now().to_rfc3339();
        self.allocations.push(VramAllocation {
            allocation_id: alloc_id.clone(),
            model_id: model_id.to_string(),
            model_name: model_name.to_string(),
            vram_mb,
            priority,
            loaded_at: now.clone(),
            last_used: now,
        });

        // Update available VRAM estimate
        self.state.available_mb = self.state.available_mb.saturating_sub(vram_mb);
        self.state.used_mb = self.state.total_mb.saturating_sub(self.state.available_mb);

        alloc_id
    }

    /// Unregister a model (freed from VRAM).
    pub fn unregister_allocation(&mut self, allocation_id: &str) -> Option<VramAllocation> {
        if let Some(pos) = self.allocations.iter().position(|a| a.allocation_id == allocation_id) {
            let alloc = self.allocations.remove(pos);
            // Update available VRAM estimate
            self.state.available_mb += alloc.vram_mb;
            self.state.used_mb = self.state.total_mb.saturating_sub(self.state.available_mb);
            Some(alloc)
        } else {
            None
        }
    }

    /// Mark a model as recently used (updates last_used timestamp).
    pub fn touch_allocation(&mut self, allocation_id: &str) {
        if let Some(alloc) = self.allocations.iter_mut().find(|a| a.allocation_id == allocation_id) {
            alloc.last_used = chrono::Utc::now().to_rfc3339();
        }
    }

    /// Get all current allocations.
    pub fn allocations(&self) -> &[VramAllocation] {
        &self.allocations
    }

    /// Get total VRAM occupied by registered allocations.
    pub fn total_allocated_mb(&self) -> u64 {
        self.allocations.iter().map(|a| a.vram_mb).sum()
    }

    /// Evict models by priority to free the specified amount of VRAM.
    /// Evicts lowest-priority (highest priority number) models first.
    /// Returns the list of evicted allocation IDs.
    pub fn evict_for_space(&mut self, needed_mb: u64) -> Vec<String> {
        let mut evicted = Vec::new();
        let mut freed: u64 = 0;

        // Sort candidates by priority (highest number = lowest priority = evict first)
        // Then by last_used (oldest first)
        let mut candidates: Vec<usize> = (0..self.allocations.len()).collect();
        candidates.sort_by(|&a, &b| {
            let pa = &self.allocations[a];
            let pb = &self.allocations[b];
            pb.priority.cmp(&pa.priority)
                .then_with(|| pa.last_used.cmp(&pb.last_used))
        });

        for idx in candidates {
            if freed >= needed_mb {
                break;
            }
            let alloc = &self.allocations[idx];
            freed += alloc.vram_mb;
            evicted.push(alloc.allocation_id.clone());
        }

        // Actually remove evicted allocations
        for id in &evicted {
            self.unregister_allocation(id);
        }

        evicted
    }

    // ─── VRAM Pre-Check (Task 5.6) ──────────────────────────────────────

    /// Pre-check whether sufficient VRAM is available for a model load.
    /// Returns detailed result including shortfall amount if insufficient.
    pub fn pre_check(&self, required_mb: u64, model_priority: u32) -> VramPreCheckResult {
        if !self.gpu_available {
            return VramPreCheckResult::NoGpu;
        }

        // Check if loads are blocked due to pressure
        if self.loads_blocked {
            return VramPreCheckResult::Insufficient {
                available_mb: self.state.available_mb,
                required_mb,
                shortfall_mb: required_mb.saturating_sub(self.state.available_mb),
            };
        }

        // Direct availability check
        if self.state.available_mb >= required_mb {
            return VramPreCheckResult::Available {
                available_mb: self.state.available_mb,
            };
        }

        // Check if eviction could free enough space
        let evictable: u64 = self
            .allocations
            .iter()
            .filter(|a| a.priority > model_priority) // Only evict lower-priority models
            .map(|a| a.vram_mb)
            .sum();

        let potential_available = self.state.available_mb + evictable;

        if potential_available >= required_mb {
            let candidates: Vec<String> = self
                .allocations
                .iter()
                .filter(|a| a.priority > model_priority)
                .map(|a| a.allocation_id.clone())
                .collect();

            VramPreCheckResult::EvictionRequired {
                available_mb: self.state.available_mb,
                required_mb,
                eviction_candidates: candidates,
                eviction_frees_mb: evictable,
            }
        } else {
            VramPreCheckResult::Insufficient {
                available_mb: self.state.available_mb,
                required_mb,
                shortfall_mb: required_mb.saturating_sub(potential_available),
            }
        }
    }

    // ─── Pressure Event (Task 5.7) ──────────────────────────────────────

    /// Check if new model loads should be blocked.
    pub fn are_loads_blocked(&self) -> bool {
        self.loads_blocked
    }

    /// Manually unblock loads (e.g., after eviction frees space).
    pub fn unblock_loads(&mut self) {
        self.loads_blocked = false;
    }

    /// Force a pressure check and potentially block loads.
    pub fn check_pressure(&mut self) -> bool {
        self.state.pressure_active = self.state.is_under_pressure();
        if self.state.pressure_active {
            self.loads_blocked = true;
        } else {
            self.loads_blocked = false;
        }
        self.state.pressure_active
    }
}

// ─── NVML VRAM Query ────────────────────────────────────────────────────────

/// Query VRAM via NVML dynamic loading.
/// Returns (total_mb, available_mb) or None if NVML is not available.
fn query_nvml_vram() -> Option<(u64, u64)> {
    let nvml_path = find_nvml_library()?;
    let lib = unsafe { libloading::Library::new(&nvml_path).ok()? };

    // Initialize NVML
    let nvml_init: libloading::Symbol<unsafe extern "C" fn() -> u32> =
        unsafe { lib.get(b"nvmlInit_v2").ok()? };
    let result = unsafe { nvml_init() };
    if result != 0 {
        return None;
    }

    // Get device handle
    let nvml_get_handle: libloading::Symbol<unsafe extern "C" fn(u32, *mut usize) -> u32> =
        unsafe { lib.get(b"nvmlDeviceGetHandleByIndex_v2").ok()? };
    let mut handle: usize = 0;
    let result = unsafe { nvml_get_handle(0, &mut handle) };
    if result != 0 {
        nvml_shutdown_lib(&lib);
        return None;
    }

    // Get memory info
    #[repr(C)]
    struct NvmlMemory {
        total: u64,
        free: u64,
        used: u64,
    }

    let nvml_get_mem: libloading::Symbol<unsafe extern "C" fn(usize, *mut NvmlMemory) -> u32> =
        unsafe { lib.get(b"nvmlDeviceGetMemoryInfo").ok()? };
    let mut mem = NvmlMemory { total: 0, free: 0, used: 0 };
    let result = unsafe { nvml_get_mem(handle, &mut mem) };

    nvml_shutdown_lib(&lib);

    if result != 0 {
        return None;
    }

    Some((mem.total / (1024 * 1024), mem.free / (1024 * 1024)))
}

fn nvml_shutdown_lib(lib: &libloading::Library) {
    if let Ok(shutdown) = unsafe { lib.get::<unsafe extern "C" fn() -> u32>(b"nvmlShutdown") } {
        unsafe { shutdown() };
    }
}

// ─── Thread-Safe Wrapper ────────────────────────────────────────────────────

/// Thread-safe shared VRAM manager.
pub struct SharedVramManager {
    inner: Arc<RwLock<VramManager>>,
}

impl SharedVramManager {
    pub fn new(manager: VramManager) -> Self {
        Self {
            inner: Arc::new(RwLock::new(manager)),
        }
    }

    pub async fn poll(&self) -> Option<VramPressureEvent> {
        let mut mgr = self.inner.write().await;
        mgr.poll_vram()
    }

    pub async fn pre_check(&self, required_mb: u64, priority: u32) -> VramPreCheckResult {
        let mgr = self.inner.read().await;
        mgr.pre_check(required_mb, priority)
    }

    pub async fn register(
        &self,
        model_id: &str,
        model_name: &str,
        vram_mb: u64,
        priority: u32,
    ) -> String {
        let mut mgr = self.inner.write().await;
        mgr.register_allocation(model_id, model_name, vram_mb, priority)
    }

    pub async fn unregister(&self, allocation_id: &str) -> Option<VramAllocation> {
        let mut mgr = self.inner.write().await;
        mgr.unregister_allocation(allocation_id)
    }

    pub async fn evict_for_space(&self, needed_mb: u64) -> Vec<String> {
        let mut mgr = self.inner.write().await;
        mgr.evict_for_space(needed_mb)
    }

    pub async fn state(&self) -> VramState {
        let mgr = self.inner.read().await;
        mgr.current_state().clone()
    }

    pub async fn are_loads_blocked(&self) -> bool {
        let mgr = self.inner.read().await;
        mgr.are_loads_blocked()
    }
}

impl Clone for SharedVramManager {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}
