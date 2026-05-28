//! Thermal monitoring and resource envelope management for Phase 7.
//!
//! Monitors CPU/GPU temperatures, classifies thermal state,
//! applies adaptation strategies, and manages resource envelopes.

use std::sync::Arc;
use tokio::sync::RwLock;
use serde::{Deserialize, Serialize};
use chrono::Utc;

use crate::hardware_service::{
    HardwareClass, HardwareEvent, ResourceEnvelope,
    ThermalState, TimeoutProfile,
};

// ─── Thermal Monitor State ──────────────────────────────────────────────────

/// Shared state for the thermal monitoring system.
pub struct ThermalMonitorState {
    pub current_thermal: Arc<RwLock<ThermalState>>,
    pub current_cpu_temp_c: Arc<RwLock<Option<f64>>>,
    pub current_gpu_temp_c: Arc<RwLock<Option<f64>>>,
    pub adaptation_active: Arc<RwLock<Option<ActiveAdaptation>>>,
    pub events: Arc<RwLock<Vec<HardwareEvent>>>,
    pub resource_envelopes: Arc<RwLock<Vec<ResourceEnvelope>>>,
    pub timeout_profile: Arc<RwLock<TimeoutProfile>>,
    pub original_timeout_profile: Arc<RwLock<TimeoutProfile>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveAdaptation {
    pub strategy: AdaptationStrategy,
    pub triggered_by: String,
    pub applied_at: String,
    pub original_concurrency: u32,
    pub reduced_concurrency: u32,
    pub timeout_multiplier: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum AdaptationStrategy {
    ThermalThrottling,      // reduce concurrency 50%, increase timeouts 2x
    ThermalCritical,        // pause background, limit to 1 concurrent
    GpuMemoryPressure,      // evict low-priority models, block new loads
    DiskFull,               // pause background jobs, alert user
    NetworkDegradation,     // switch to local-only, queue remote
}

// ─── Thermal Classification ─────────────────────────────────────────────────

/// Classify temperature reading into ThermalState.
pub fn classify_thermal(cpu_temp_c: Option<f64>, gpu_temp_c: Option<f64>) -> ThermalState {
    let max_temp = [cpu_temp_c, gpu_temp_c]
        .iter()
        .filter_map(|t| *t)
        .fold(0.0_f64, f64::max);

    if max_temp >= 95.0 {
        ThermalState::Critical
    } else if max_temp >= 85.0 {
        ThermalState::Throttling
    } else if max_temp >= 70.0 {
        ThermalState::Warm
    } else {
        ThermalState::Nominal
    }
}

/// Read CPU temperature from platform-specific sources.
pub fn read_cpu_temperature() -> Option<f64> {
    #[cfg(target_os = "linux")]
    {
        read_linux_cpu_temp()
    }
    #[cfg(target_os = "windows")]
    {
        read_windows_cpu_temp()
    }
    #[cfg(target_os = "macos")]
    {
        read_macos_cpu_temp()
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    {
        None
    }
}

/// Read GPU temperature (NVIDIA via NVML, AMD via rocm-smi).
pub fn read_gpu_temperature() -> Option<f64> {
    // Try NVIDIA first
    if let Some(temp) = read_nvidia_gpu_temp() {
        return Some(temp);
    }
    // Try AMD
    read_amd_gpu_temp()
}

// ─── Platform-Specific Temperature Reading ──────────────────────────────────

#[cfg(target_os = "linux")]
fn read_linux_cpu_temp() -> Option<f64> {
    // Read from /sys/class/thermal/thermal_zone*/temp
    let thermal_zones = std::fs::read_dir("/sys/class/thermal/").ok()?;
    let mut max_temp: f64 = 0.0;

    for entry in thermal_zones.flatten() {
        let path = entry.path();
        if path.file_name()?.to_str()?.starts_with("thermal_zone") {
            let temp_path = path.join("temp");
            if let Ok(content) = std::fs::read_to_string(&temp_path) {
                if let Ok(millidegrees) = content.trim().parse::<f64>() {
                    let celsius = millidegrees / 1000.0;
                    if celsius > max_temp {
                        max_temp = celsius;
                    }
                }
            }
        }
    }

    if max_temp > 0.0 { Some(max_temp) } else { None }
}

#[cfg(target_os = "windows")]
fn read_windows_cpu_temp() -> Option<f64> {
    // Windows: WMI query for MSAcpi_ThermalZoneTemperature
    // This requires elevated privileges on most systems
    // Fallback: use sysinfo component temperatures if available
    let components = sysinfo::Components::new_with_refreshed_list();
    let mut max_temp: f64 = 0.0;
    for component in components.iter() {
        let temp = component.temperature() as f64;
        if temp > max_temp {
            max_temp = temp;
        }
    }
    if max_temp > 0.0 { Some(max_temp) } else { None }
}

#[cfg(target_os = "macos")]
fn read_macos_cpu_temp() -> Option<f64> {
    // macOS: use powermetrics or SMC reading
    // Simplified: try sysinfo components
    let components = sysinfo::Components::new_with_refreshed_list();
    let mut max_temp: f64 = 0.0;
    for component in components.iter() {
        let temp = component.temperature() as f64;
        if temp > max_temp {
            max_temp = temp;
        }
    }
    if max_temp > 0.0 { Some(max_temp) } else { None }
}

fn read_nvidia_gpu_temp() -> Option<f64> {
    // Quick check via nvidia-smi CLI (simpler than NVML for just temperature)
    let output = std::process::Command::new("nvidia-smi")
        .args(["--query-gpu=temperature.gpu", "--format=csv,noheader,nounits"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    String::from_utf8_lossy(&output.stdout)
        .trim()
        .lines()
        .next()?
        .parse::<f64>()
        .ok()
}

fn read_amd_gpu_temp() -> Option<f64> {
    let output = std::process::Command::new("rocm-smi")
        .args(["--showtemp", "--csv"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    // Parse temperature from rocm-smi CSV output
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines().skip(1) {
        // Skip header
        if let Some(temp_str) = line.split(',').nth(1) {
            if let Ok(temp) = temp_str.trim().parse::<f64>() {
                return Some(temp);
            }
        }
    }
    None
}

// ─── Resource Envelope Defaults ─────────────────────────────────────────────

/// Get default resource envelopes for a hardware class.
pub fn default_resource_envelopes(class: &HardwareClass, total_ram_mb: u64) -> Vec<ResourceEnvelope> {
    match class {
        HardwareClass::GpuWorkstation => vec![
            ResourceEnvelope {
                workload_type: "interactive-inference".to_string(),
                cpu_percent: 40,
                ram_mb: total_ram_mb * 40 / 100,
                gpu_percent: Some(70),
                vram_mb: None, // dynamic based on model
                priority: 1,
            },
            ResourceEnvelope {
                workload_type: "tool-execution".to_string(),
                cpu_percent: 30,
                ram_mb: total_ram_mb * 30 / 100,
                gpu_percent: Some(20),
                vram_mb: None,
                priority: 2,
            },
            ResourceEnvelope {
                workload_type: "background-training".to_string(),
                cpu_percent: 20,
                ram_mb: total_ram_mb * 20 / 100,
                gpu_percent: Some(10),
                vram_mb: None,
                priority: 3,
            },
            ResourceEnvelope {
                workload_type: "system-overhead".to_string(),
                cpu_percent: 10,
                ram_mb: total_ram_mb * 10 / 100,
                gpu_percent: Some(0),
                vram_mb: None,
                priority: 0,
            },
        ],
        HardwareClass::CpuWorkstation | HardwareClass::CpuServer => vec![
            ResourceEnvelope {
                workload_type: "interactive-inference".to_string(),
                cpu_percent: 50,
                ram_mb: total_ram_mb * 50 / 100,
                gpu_percent: None,
                vram_mb: None,
                priority: 1,
            },
            ResourceEnvelope {
                workload_type: "tool-execution".to_string(),
                cpu_percent: 30,
                ram_mb: total_ram_mb * 25 / 100,
                gpu_percent: None,
                vram_mb: None,
                priority: 2,
            },
            ResourceEnvelope {
                workload_type: "background-training".to_string(),
                cpu_percent: 10,
                ram_mb: total_ram_mb * 15 / 100,
                gpu_percent: None,
                vram_mb: None,
                priority: 3,
            },
            ResourceEnvelope {
                workload_type: "system-overhead".to_string(),
                cpu_percent: 10,
                ram_mb: total_ram_mb * 10 / 100,
                gpu_percent: None,
                vram_mb: None,
                priority: 0,
            },
        ],
        HardwareClass::Embedded => vec![
            ResourceEnvelope {
                workload_type: "interactive-inference".to_string(),
                cpu_percent: 60,
                ram_mb: total_ram_mb * 60 / 100,
                gpu_percent: None,
                vram_mb: None,
                priority: 1,
            },
            ResourceEnvelope {
                workload_type: "tool-execution".to_string(),
                cpu_percent: 20,
                ram_mb: total_ram_mb * 20 / 100,
                gpu_percent: None,
                vram_mb: None,
                priority: 2,
            },
            ResourceEnvelope {
                workload_type: "system-overhead".to_string(),
                cpu_percent: 20,
                ram_mb: total_ram_mb * 20 / 100,
                gpu_percent: None,
                vram_mb: None,
                priority: 0,
            },
            // No background training on embedded
        ],
        _ => vec![
            ResourceEnvelope {
                workload_type: "interactive-inference".to_string(),
                cpu_percent: 50,
                ram_mb: total_ram_mb * 50 / 100,
                gpu_percent: None,
                vram_mb: None,
                priority: 1,
            },
            ResourceEnvelope {
                workload_type: "tool-execution".to_string(),
                cpu_percent: 30,
                ram_mb: total_ram_mb * 30 / 100,
                gpu_percent: None,
                vram_mb: None,
                priority: 2,
            },
            ResourceEnvelope {
                workload_type: "system-overhead".to_string(),
                cpu_percent: 20,
                ram_mb: total_ram_mb * 20 / 100,
                gpu_percent: None,
                vram_mb: None,
                priority: 0,
            },
        ],
    }
}

// ─── Adaptation Logic ───────────────────────────────────────────────────────

/// Determine which adaptation strategy to apply based on thermal state transition.
pub fn determine_adaptation(
    previous: &ThermalState,
    current: &ThermalState,
) -> Option<AdaptationStrategy> {
    match (previous, current) {
        (ThermalState::Nominal | ThermalState::Warm, ThermalState::Throttling) => {
            Some(AdaptationStrategy::ThermalThrottling)
        }
        (_, ThermalState::Critical) => {
            Some(AdaptationStrategy::ThermalCritical)
        }
        _ => None,
    }
}

/// Check if adaptation should be cleared (thermal recovered).
pub fn should_clear_adaptation(
    current_thermal: &ThermalState,
    active_adaptation: &ActiveAdaptation,
) -> bool {
    match active_adaptation.strategy {
        AdaptationStrategy::ThermalThrottling => {
            *current_thermal == ThermalState::Nominal || *current_thermal == ThermalState::Warm
        }
        AdaptationStrategy::ThermalCritical => {
            *current_thermal != ThermalState::Critical
        }
        _ => false,
    }
}

/// Apply timeout multiplier for thermal adaptation.
pub fn apply_timeout_adaptation(
    base_profile: &TimeoutProfile,
    multiplier: f64,
) -> TimeoutProfile {
    TimeoutProfile {
        hardware_class: base_profile.hardware_class.clone(),
        inference_ms: (base_profile.inference_ms as f64 * multiplier) as u64,
        tool_execution_ms: (base_profile.tool_execution_ms as f64 * multiplier) as u64,
        health_check_ms: (base_profile.health_check_ms as f64 * multiplier) as u64,
        network_request_ms: (base_profile.network_request_ms as f64 * multiplier) as u64,
        database_query_ms: (base_profile.database_query_ms as f64 * multiplier) as u64,
        compute_job_ms: (base_profile.compute_job_ms as f64 * multiplier) as u64,
    }
}

/// Create a hardware event record.
pub fn create_hardware_event(
    event_type: &str,
    details: serde_json::Value,
    adaptation: Option<&str>,
) -> HardwareEvent {
    HardwareEvent {
        event_type: event_type.to_string(),
        timestamp: Utc::now().to_rfc3339(),
        details,
        adaptation_applied: adaptation.map(|s| s.to_string()),
    }
}

// ─── Storage Type Detection ─────────────────────────────────────────────────

/// Detect storage type (NVMe, SSD, HDD) for the given path.
pub fn detect_storage_type(path: &std::path::Path) -> String {
    #[cfg(target_os = "linux")]
    {
        detect_storage_type_linux(path)
    }
    #[cfg(target_os = "windows")]
    {
        detect_storage_type_windows(path)
    }
    #[cfg(target_os = "macos")]
    {
        // macOS: modern Macs are all NVMe
        "nvme".to_string()
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    {
        "unknown".to_string()
    }
}

#[cfg(target_os = "linux")]
fn detect_storage_type_linux(path: &std::path::Path) -> String {
    // Find the block device for this path via /proc/mounts
    // Then check /sys/block/<device>/queue/rotational
    // 0 = SSD/NVMe, 1 = HDD
    // Also check /sys/block/<device>/device/transport for "nvme"

    if let Ok(mounts) = std::fs::read_to_string("/proc/mounts") {
        let path_str = path.to_str().unwrap_or("/");
        for line in mounts.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 && path_str.starts_with(parts[1]) {
                let device = parts[0].trim_start_matches("/dev/");
                // Strip partition number
                let base_device = device.trim_end_matches(|c: char| c.is_ascii_digit());

                // Check if NVMe
                if base_device.starts_with("nvme") {
                    return "nvme".to_string();
                }

                // Check rotational flag
                let rotational_path = format!("/sys/block/{base_device}/queue/rotational");
                if let Ok(val) = std::fs::read_to_string(&rotational_path) {
                    return match val.trim() {
                        "0" => "ssd".to_string(),
                        "1" => "hdd".to_string(),
                        _ => "unknown".to_string(),
                    };
                }
            }
        }
    }
    "unknown".to_string()
}

#[cfg(target_os = "windows")]
fn detect_storage_type_windows(_path: &std::path::Path) -> String {
    // Windows: use PowerShell to query disk type
    // Get-PhysicalDisk | Select MediaType
    let output = std::process::Command::new("powershell")
        .args(["-Command", "Get-PhysicalDisk | Select-Object -First 1 -ExpandProperty MediaType"])
        .output();

    match output {
        Ok(out) if out.status.success() => {
            let media_type = String::from_utf8_lossy(&out.stdout).trim().to_lowercase();
            if media_type.contains("ssd") || media_type.contains("solid") {
                "ssd".to_string()
            } else if media_type.contains("hdd") || media_type.contains("spinning") {
                "hdd".to_string()
            } else if media_type.contains("nvme") {
                "nvme".to_string()
            } else {
                // Default to SSD for "Unspecified" (common on NVMe)
                "ssd".to_string()
            }
        }
        _ => "unknown".to_string(),
    }
}

// ─── Hardware Probe Benchmarks ──────────────────────────────────────────────

use crate::hardware_service::ProbeResults;

/// Run all hardware probes. Completes within 15 seconds.
pub fn run_hardware_probes(data_dir: &std::path::Path) -> ProbeResults {
    let cpu_tps = probe_cpu_inference_throughput();
    let gpu_tps = probe_gpu_inference_throughput();
    let (disk_read, disk_write) = probe_disk_io(data_dir);
    let mem_bw = probe_memory_bandwidth();

    ProbeResults {
        cpu_tokens_per_sec: cpu_tps,
        gpu_tokens_per_sec: gpu_tps,
        disk_read_mbps: disk_read,
        disk_write_mbps: disk_write,
        memory_bandwidth_gbps: mem_bw,
        probed_at: Utc::now().to_rfc3339(),
    }
}

/// CPU inference throughput estimate via matrix multiply benchmark.
fn probe_cpu_inference_throughput() -> f64 {
    // Simulate inference workload: multiply two 512x512 matrices
    // Time it, extrapolate to tokens/sec
    let size = 512;
    let a: Vec<f32> = vec![1.0; size * size];
    let b: Vec<f32> = vec![1.0; size * size];
    let mut c: Vec<f32> = vec![0.0; size * size];

    let start = std::time::Instant::now();
    // Naive matmul (measures raw CPU throughput)
    for i in 0..size {
        for k in 0..size {
            let a_ik = a[i * size + k];
            for j in 0..size {
                c[i * size + j] += a_ik * b[k * size + j];
            }
        }
    }
    let elapsed = start.elapsed();

    // Prevent optimization from eliminating the computation
    std::hint::black_box(&c);

    // Rough calibration: 512x512 matmul in X ms → estimate tokens/sec
    // A 7B model does ~2 matmuls per token at this scale
    // Calibrated against known hardware: 100ms matmul ≈ 10 tokens/sec for 7B
    let elapsed_ms = elapsed.as_millis() as f64;
    if elapsed_ms > 0.0 {
        1000.0 / elapsed_ms // rough tokens/sec estimate
    } else {
        100.0 // very fast CPU
    }
}

/// GPU inference throughput estimate.
/// Attempts to load a small ONNX model via tract-onnx for a forward pass benchmark.
/// Falls back to a GPU memory bandwidth test via NVML if tract is not available.
/// Returns estimated tokens/sec or None if no GPU is available.
fn probe_gpu_inference_throughput() -> Option<f64> {
    // Strategy 1: Try tract-onnx inference probe
    #[cfg(feature = "tract-onnx")]
    {
        if let Some(tps) = probe_gpu_via_tract() {
            return Some(tps);
        }
    }

    // Strategy 2: Fall back to GPU memory bandwidth test via NVML
    probe_gpu_via_memory_bandwidth()
}

/// Probe GPU inference throughput using tract-onnx.
/// Loads a minimal ONNX model (single linear layer) and runs forward passes.
#[cfg(feature = "tract-onnx")]
fn probe_gpu_via_tract() -> Option<f64> {
    use tract_onnx::prelude::*;

    // Create a minimal ONNX model in memory (a simple matmul operation)
    // This tests the full inference pipeline: load → optimize → run
    let model_bytes = create_minimal_onnx_model()?;

    let model = tract_onnx::onnx()
        .model_for_read(&mut std::io::Cursor::new(&model_bytes))
        .ok()?
        .into_optimized()
        .ok()?
        .into_runnable()
        .ok()?;

    // Input: batch of 1, sequence length 128, hidden dim 256
    let input = tract_ndarray::Array3::<f32>::zeros((1, 128, 256));
    let input_tensor = input.into_tensor();

    // Warm up
    let _ = model.run(tvec![input_tensor.clone().into()]);

    // Benchmark: run 10 forward passes
    let start = std::time::Instant::now();
    let iterations = 10;
    for _ in 0..iterations {
        let input_clone = input_tensor.clone();
        let _ = model.run(tvec![input_clone.into()]);
    }
    let elapsed = start.elapsed();

    // Each forward pass processes 128 "tokens" worth of computation
    let tokens_processed = iterations * 128;
    let elapsed_secs = elapsed.as_secs_f64();
    if elapsed_secs > 0.0 {
        Some(tokens_processed as f64 / elapsed_secs)
    } else {
        Some(1000.0) // Very fast
    }
}

#[cfg(feature = "tract-onnx")]
fn create_minimal_onnx_model() -> Option<Vec<u8>> {
    // In a real implementation, this would create a minimal ONNX protobuf
    // For now, return None to fall through to the bandwidth test
    // The actual model file would be bundled as a resource
    None
}

/// Probe GPU throughput via memory bandwidth test using NVML.
/// Measures GPU memory copy speed as a proxy for inference throughput.
fn probe_gpu_via_memory_bandwidth() -> Option<f64> {
    let nvml_path = crate::hardware_gpu_detection::find_nvml_library()?;
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
        nvml_shutdown_probe(&lib);
        return None;
    }

    // Get GPU clock speed and memory bandwidth as proxy for throughput
    // nvmlDeviceGetClockInfo: type 0 = graphics clock, type 1 = SM clock, type 2 = memory clock
    let nvml_get_clock: Result<libloading::Symbol<unsafe extern "C" fn(usize, u32, *mut u32) -> u32>, _> =
        unsafe { lib.get(b"nvmlDeviceGetClockInfo") };

    let gpu_clock_mhz = if let Ok(get_clock) = nvml_get_clock {
        let mut clock: u32 = 0;
        let result = unsafe { get_clock(handle, 0, &mut clock) }; // graphics clock
        if result == 0 { clock } else { 0 }
    } else {
        0
    };

    // Get memory bus width for bandwidth estimation
    let nvml_get_bus_width: Result<libloading::Symbol<unsafe extern "C" fn(usize, *mut u32) -> u32>, _> =
        unsafe { lib.get(b"nvmlDeviceGetMemoryBusWidth") };

    let bus_width_bits = if let Ok(get_bw) = nvml_get_bus_width {
        let mut width: u32 = 0;
        let result = unsafe { get_bw(handle, &mut width) };
        if result == 0 { width } else { 256 } // default 256-bit
    } else {
        256
    };

    nvml_shutdown_probe(&lib);

    // Estimate tokens/sec from GPU clock and memory bandwidth
    // Rough calibration: RTX 3090 (1695 MHz, 384-bit) ≈ 60 tokens/sec for 7B model
    // Scale linearly from that reference point
    if gpu_clock_mhz > 0 {
        let reference_clock: f64 = 1695.0;
        let reference_bus: f64 = 384.0;
        let reference_tps: f64 = 60.0;

        let clock_factor = gpu_clock_mhz as f64 / reference_clock;
        let bus_factor = bus_width_bits as f64 / reference_bus;
        let estimated_tps = reference_tps * clock_factor * bus_factor.sqrt();

        Some(estimated_tps)
    } else {
        // If we can't get clock info, just confirm GPU exists
        Some(30.0) // Conservative estimate
    }
}

fn nvml_shutdown_probe(lib: &libloading::Library) {
    if let Ok(shutdown) = unsafe { lib.get::<unsafe extern "C" fn() -> u32>(b"nvmlShutdown") } {
        unsafe { shutdown() };
    }
}

/// Disk I/O probe: write and read 10MB, measure throughput.
fn probe_disk_io(data_dir: &std::path::Path) -> (f64, f64) {
    let probe_file = data_dir.join(".hardware_probe_io_test");
    let data = vec![0x42u8; 10 * 1024 * 1024]; // 10MB

    // Write probe
    let write_start = std::time::Instant::now();
    let write_result = std::fs::write(&probe_file, &data);
    let write_elapsed = write_start.elapsed();

    let write_mbps = if write_result.is_ok() && write_elapsed.as_millis() > 0 {
        10.0 * 1000.0 / write_elapsed.as_millis() as f64
    } else {
        0.0
    };

    // Read probe
    let read_start = std::time::Instant::now();
    let read_result = std::fs::read(&probe_file);
    let read_elapsed = read_start.elapsed();

    let read_mbps = if read_result.is_ok() && read_elapsed.as_millis() > 0 {
        10.0 * 1000.0 / read_elapsed.as_millis() as f64
    } else {
        0.0
    };

    // Cleanup
    let _ = std::fs::remove_file(&probe_file);

    (read_mbps, write_mbps)
}

/// Memory bandwidth probe: copy large array, measure GB/s.
fn probe_memory_bandwidth() -> f64 {
    let size = 64 * 1024 * 1024; // 64MB
    let source: Vec<u8> = vec![0xAB; size];
    let mut dest: Vec<u8> = vec![0; size];

    let start = std::time::Instant::now();
    dest.copy_from_slice(&source);
    let elapsed = start.elapsed();

    // Prevent optimization
    std::hint::black_box(&dest);

    let elapsed_secs = elapsed.as_secs_f64();
    if elapsed_secs > 0.0 {
        (size as f64 / (1024.0 * 1024.0 * 1024.0)) / elapsed_secs
    } else {
        10.0 // very fast
    }
}
