use chrono::Utc;
use serde::{Deserialize, Serialize};
use sysinfo::System;

// ─── Hardware Profile ───────────────────────────────────────────────────────

/// Complete hardware profile for a machine.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HardwareProfile {
    pub node_id: String,
    pub detected_at: String,
    pub hardware_class: HardwareClass,
    pub cpu: CpuProfile,
    pub memory: MemoryProfile,
    pub gpu: Option<GpuProfile>,
    pub storage: StorageProfile,
    pub network: NetworkProfile,
    pub probe_results: Option<ProbeResults>,
}

/// Classification of a machine into one of 6 hardware classes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum HardwareClass {
    GpuWorkstation,
    CpuWorkstation,
    GpuServer,
    CpuServer,
    Embedded,
    ContainerRestricted,
}

// ─── CPU ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CpuProfile {
    pub physical_cores: u32,
    pub logical_cores: u32,
    pub architecture: String,
    pub base_clock_mhz: u32,
    pub has_avx2: bool,
    pub has_avx512: bool,
    pub has_neon: bool,
    pub model_name: String,
}

// ─── Memory ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryProfile {
    pub total_ram_mb: u64,
    pub available_ram_mb: u64,
    pub swap_mb: u64,
    pub ddr_generation: Option<u32>,
    pub channels: Option<u32>,
    pub estimated_bandwidth_gbps: Option<f64>,
}

// ─── GPU ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GpuProfile {
    pub model_name: String,
    pub total_vram_mb: u64,
    pub available_vram_mb: u64,
    pub compute_capability: Option<String>,
    pub driver_version: String,
    pub cuda_version: Option<String>,
    pub rocm_version: Option<String>,
    pub metal_support: bool,
    pub vulkan_compute: bool,
}

// ─── Storage ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageProfile {
    pub available_space_mb: u64,
    pub storage_type: String,
    pub sequential_read_mbps: Option<f64>,
    pub sequential_write_mbps: Option<f64>,
}

// ─── Network ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkProfile {
    pub interfaces: Vec<NetworkInterface>,
    pub lan_bandwidth_mbps: Option<f64>,
    pub internet_connected: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NetworkInterface {
    pub name: String,
    pub interface_type: String,
    pub speed_mbps: Option<u32>,
}

// ─── Probe Results ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeResults {
    pub cpu_tokens_per_sec: f64,
    pub gpu_tokens_per_sec: Option<f64>,
    pub disk_read_mbps: f64,
    pub disk_write_mbps: f64,
    pub memory_bandwidth_gbps: f64,
    pub probed_at: String,
}

// ─── Timeout Profile ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimeoutProfile {
    pub hardware_class: HardwareClass,
    pub inference_ms: u64,
    pub tool_execution_ms: u64,
    pub health_check_ms: u64,
    pub network_request_ms: u64,
    pub database_query_ms: u64,
    pub compute_job_ms: u64,
}

// ─── Model Compatibility ────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelCompatibilityEntry {
    pub model_id: String,
    pub model_name: String,
    pub parameter_count_b: f64,
    pub quantization: String,
    pub required_vram_mb: u64,
    pub required_ram_mb: u64,
    pub compatibility_class: ModelCompatibilityClass,
    pub estimated_tokens_per_sec: f64,
    pub incompatibility_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ModelCompatibilityClass {
    NativeGpu,
    Offloaded,
    CpuOnly,
    Incompatible,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelRequirements {
    pub model_id: String,
    pub model_name: String,
    pub parameter_count_b: f64,
    pub quantization: String,
    pub min_vram_mb: u64,
    pub min_ram_mb: u64,
    pub min_compute_capability: Option<String>,
}

// ─── Resource Envelopes ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceEnvelope {
    pub workload_type: String,
    pub cpu_percent: u32,
    pub ram_mb: u64,
    pub gpu_percent: Option<u32>,
    pub vram_mb: Option<u64>,
    pub priority: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceUtilization {
    pub cpu_percent: f64,
    pub ram_used_mb: u64,
    pub ram_total_mb: u64,
    pub gpu_percent: Option<f64>,
    pub vram_used_mb: Option<u64>,
    pub vram_total_mb: Option<u64>,
    pub envelopes: Vec<EnvelopeUtilization>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvelopeUtilization {
    pub workload_type: String,
    pub cpu_used_percent: f64,
    pub ram_used_mb: u64,
    pub gpu_used_percent: Option<f64>,
    pub vram_used_mb: Option<u64>,
}

// ─── Thermal State ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ThermalState {
    Nominal,
    Warm,
    Throttling,
    Critical,
}

// ─── Hardware Events ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HardwareEvent {
    pub event_type: String,
    pub timestamp: String,
    pub details: serde_json::Value,
    pub adaptation_applied: Option<String>,
}

// ─── Detection Functions ────────────────────────────────────────────────────

/// Detect CPU capabilities using sysinfo.
pub fn detect_cpu() -> CpuProfile {
    let mut sys = System::new();
    sys.refresh_cpu_all();

    let cpus = sys.cpus();
    let physical_cores = sys.physical_core_count().unwrap_or(1) as u32;
    let logical_cores = cpus.len() as u32;
    let base_clock_mhz = cpus.first().map(|c| c.frequency() as u32).unwrap_or(0);
    let model_name = sys.cpus().first()
        .map(|c| c.brand().to_string())
        .unwrap_or_else(|| "Unknown CPU".to_string());

    let architecture = if cfg!(target_arch = "x86_64") {
        "x86_64".to_string()
    } else if cfg!(target_arch = "aarch64") {
        "aarch64".to_string()
    } else {
        std::env::consts::ARCH.to_string()
    };

    // Instruction set detection (compile-time for now)
    let has_avx2 = cfg!(target_feature = "avx2");
    let has_avx512 = cfg!(target_feature = "avx512f");
    let has_neon = cfg!(target_arch = "aarch64"); // NEON is mandatory on aarch64

    CpuProfile {
        physical_cores,
        logical_cores,
        architecture,
        base_clock_mhz,
        has_avx2,
        has_avx512,
        has_neon,
        model_name,
    }
}

/// Detect memory capabilities using sysinfo.
pub fn detect_memory() -> MemoryProfile {
    let mut sys = System::new();
    sys.refresh_memory();

    MemoryProfile {
        total_ram_mb: sys.total_memory() / (1024 * 1024),
        available_ram_mb: sys.available_memory() / (1024 * 1024),
        swap_mb: sys.total_swap() / (1024 * 1024),
        ddr_generation: None, // requires platform-specific detection
        channels: None,
        estimated_bandwidth_gbps: None,
    }
}

/// Detect storage capabilities for the given data directory.
pub fn detect_storage(data_dir: &std::path::Path) -> StorageProfile {
    let available_space_mb = fs_available_space_mb(data_dir);
    let storage_type = crate::hardware_thermal::detect_storage_type(data_dir);

    StorageProfile {
        available_space_mb,
        storage_type,
        sequential_read_mbps: None,          // populated by probe
        sequential_write_mbps: None,         // populated by probe
    }
}

/// Detect network interfaces.
pub fn detect_network() -> NetworkProfile {
    // Basic detection — enumerate interfaces via sysinfo networks
    let mut sys = System::new();
    sys.refresh_all();

    let networks = sysinfo::Networks::new_with_refreshed_list();
    let interfaces: Vec<NetworkInterface> = networks.iter().map(|(name, _data)| {
        let interface_type = if name.contains("lo") || name.contains("Loopback") {
            "loopback"
        } else if name.contains("wl") || name.contains("Wi-Fi") || name.contains("wlan") {
            "wifi"
        } else {
            "ethernet"
        };
        NetworkInterface {
            name: name.to_string(),
            interface_type: interface_type.to_string(),
            speed_mbps: None,
        }
    }).collect();

    NetworkProfile {
        interfaces,
        lan_bandwidth_mbps: None,
        internet_connected: false, // TODO: DNS probe
    }
}

/// Detect GPU capabilities. Returns None if no discrete GPU found.
pub fn detect_gpu() -> Option<GpuProfile> {
    crate::hardware_gpu_detection::detect_gpu_full()
}

/// Classify hardware into one of 6 classes based on detected capabilities.
pub fn classify_hardware(profile: &HardwareProfile) -> HardwareClass {
    let has_gpu = profile.gpu.is_some();
    let vram_mb = profile.gpu.as_ref().map(|g| g.total_vram_mb).unwrap_or(0);
    let ram_mb = profile.memory.total_ram_mb;
    let is_headless = is_headless_environment();
    let is_container = is_container_environment();

    if is_container {
        return HardwareClass::ContainerRestricted;
    }

    if ram_mb < 8 * 1024 {
        return HardwareClass::Embedded;
    }

    match (has_gpu && vram_mb >= 8 * 1024, ram_mb >= 64 * 1024, is_headless) {
        (true, true, true) => HardwareClass::GpuServer,
        (true, _, _) => HardwareClass::GpuWorkstation,
        (false, true, true) => HardwareClass::CpuServer,
        (false, _, _) => HardwareClass::CpuWorkstation,
    }
}

/// Get default timeout profile for a hardware class.
pub fn default_timeout_profile(class: &HardwareClass) -> TimeoutProfile {
    match class {
        HardwareClass::GpuWorkstation => TimeoutProfile {
            hardware_class: class.clone(),
            inference_ms: 5,
            tool_execution_ms: 30_000,
            health_check_ms: 5_000,
            network_request_ms: 10_000,
            database_query_ms: 1_000,
            compute_job_ms: 3_600_000,
        },
        HardwareClass::CpuWorkstation => TimeoutProfile {
            hardware_class: class.clone(),
            inference_ms: 50,
            tool_execution_ms: 60_000,
            health_check_ms: 5_000,
            network_request_ms: 10_000,
            database_query_ms: 2_000,
            compute_job_ms: 7_200_000,
        },
        HardwareClass::GpuServer => TimeoutProfile {
            hardware_class: class.clone(),
            inference_ms: 3,
            tool_execution_ms: 30_000,
            health_check_ms: 5_000,
            network_request_ms: 10_000,
            database_query_ms: 1_000,
            compute_job_ms: 3_600_000,
        },
        HardwareClass::CpuServer => TimeoutProfile {
            hardware_class: class.clone(),
            inference_ms: 100,
            tool_execution_ms: 120_000,
            health_check_ms: 10_000,
            network_request_ms: 15_000,
            database_query_ms: 3_000,
            compute_job_ms: 14_400_000,
        },
        HardwareClass::Embedded => TimeoutProfile {
            hardware_class: class.clone(),
            inference_ms: 500,
            tool_execution_ms: 300_000,
            health_check_ms: 15_000,
            network_request_ms: 30_000,
            database_query_ms: 5_000,
            compute_job_ms: 28_800_000,
        },
        HardwareClass::ContainerRestricted => TimeoutProfile {
            hardware_class: class.clone(),
            inference_ms: 200,
            tool_execution_ms: 120_000,
            health_check_ms: 10_000,
            network_request_ms: 15_000,
            database_query_ms: 3_000,
            compute_job_ms: 7_200_000,
        },
    }
}

/// Compute model compatibility for a single model against the hardware profile.
pub fn compute_model_compatibility(
    model: &ModelRequirements,
    profile: &HardwareProfile,
) -> ModelCompatibilityEntry {
    let gpu_vram = profile.gpu.as_ref().map(|g| g.available_vram_mb).unwrap_or(0);
    let available_ram = profile.memory.available_ram_mb;

    let (compatibility_class, reason) = if profile.gpu.is_some() && gpu_vram >= model.min_vram_mb {
        (ModelCompatibilityClass::NativeGpu, None)
    } else if profile.gpu.is_some() && gpu_vram > 0 && (gpu_vram + available_ram) >= model.min_ram_mb {
        (ModelCompatibilityClass::Offloaded, None)
    } else if available_ram >= model.min_ram_mb {
        (ModelCompatibilityClass::CpuOnly, None)
    } else {
        let reason = format!(
            "Requires {}MB VRAM or {}MB RAM; available: {}MB VRAM, {}MB RAM",
            model.min_vram_mb, model.min_ram_mb, gpu_vram, available_ram
        );
        (ModelCompatibilityClass::Incompatible, Some(reason))
    };

    // Rough speed estimation based on compatibility class
    let estimated_tps = match &compatibility_class {
        ModelCompatibilityClass::NativeGpu => 30.0 / model.parameter_count_b.max(1.0),
        ModelCompatibilityClass::Offloaded => 15.0 / model.parameter_count_b.max(1.0),
        ModelCompatibilityClass::CpuOnly => 5.0 / model.parameter_count_b.max(1.0),
        ModelCompatibilityClass::Incompatible => 0.0,
    };

    ModelCompatibilityEntry {
        model_id: model.model_id.clone(),
        model_name: model.model_name.clone(),
        parameter_count_b: model.parameter_count_b,
        quantization: model.quantization.clone(),
        required_vram_mb: model.min_vram_mb,
        required_ram_mb: model.min_ram_mb,
        compatibility_class,
        estimated_tokens_per_sec: estimated_tps,
        incompatibility_reason: reason,
    }
}

/// Run full hardware detection. Completes within 5 seconds.
pub fn detect_hardware(data_dir: &std::path::Path) -> HardwareProfile {
    let cpu = detect_cpu();
    let memory = detect_memory();
    let gpu = detect_gpu();
    let storage = detect_storage(data_dir);
    let network = detect_network();

    let mut profile = HardwareProfile {
        node_id: generate_node_id(),
        detected_at: Utc::now().to_rfc3339(),
        hardware_class: HardwareClass::CpuWorkstation, // placeholder
        cpu,
        memory,
        gpu,
        storage,
        network,
        probe_results: None,
    };

    profile.hardware_class = classify_hardware(&profile);
    profile
}

// ─── IPC Commands ───────────────────────────────────────────────────────────

#[tauri::command]
pub fn hardware_get_profile(app: tauri::AppHandle) -> Result<HardwareProfile, String> {
    let data_dir = app.path().app_data_dir()
        .map_err(|e| format!("Failed to resolve app data dir: {e}"))?;
    Ok(detect_hardware(&data_dir))
}

#[tauri::command]
pub fn hardware_get_timeout_profile(app: tauri::AppHandle) -> Result<TimeoutProfile, String> {
    let data_dir = app.path().app_data_dir()
        .map_err(|e| format!("Failed to resolve app data dir: {e}"))?;
    let profile = detect_hardware(&data_dir);
    Ok(default_timeout_profile(&profile.hardware_class))
}

#[tauri::command]
pub fn hardware_get_compatibility_matrix(
    app: tauri::AppHandle,
    models: Vec<ModelRequirements>,
) -> Result<Vec<ModelCompatibilityEntry>, String> {
    let data_dir = app.path().app_data_dir()
        .map_err(|e| format!("Failed to resolve app data dir: {e}"))?;
    let profile = detect_hardware(&data_dir);
    let matrix = models.iter()
        .map(|m| compute_model_compatibility(m, &profile))
        .collect();
    Ok(matrix)
}

#[tauri::command]
pub fn hardware_get_thermal_state() -> Result<ThermalState, String> {
    // Placeholder — thermal monitoring requires platform-specific APIs
    Ok(ThermalState::Nominal)
}

#[tauri::command]
pub fn hardware_get_resource_utilization() -> Result<ResourceUtilization, String> {
    let mut sys = System::new();
    sys.refresh_cpu_all();
    sys.refresh_memory();

    Ok(ResourceUtilization {
        cpu_percent: sys.global_cpu_usage() as f64,
        ram_used_mb: sys.used_memory() / (1024 * 1024),
        ram_total_mb: sys.total_memory() / (1024 * 1024),
        gpu_percent: None,
        vram_used_mb: None,
        vram_total_mb: None,
        envelopes: vec![],
    })
}

#[tauri::command]
pub fn hardware_override_class(class: String) -> Result<(), String> {
    // Validate the class string
    match class.as_str() {
        "gpu-workstation" | "cpu-workstation" | "gpu-server" | "cpu-server" | "embedded" | "container-restricted" => Ok(()),
        _ => Err(format!("Invalid hardware class: {class}. Must be one of: gpu-workstation, cpu-workstation, gpu-server, cpu-server, embedded, container-restricted")),
    }
    // TODO: persist override to config file
}

// ─── Helpers ────────────────────────────────────────────────────────────────

fn generate_node_id() -> String {
    use sha2::{Digest, Sha256};
    let mut sys = System::new();
    sys.refresh_cpu_all();

    let hostname = System::host_name().unwrap_or_else(|| "unknown".to_string());
    let cpu_brand = sys.cpus().first()
        .map(|c| c.brand().to_string())
        .unwrap_or_default();

    let mut hasher = Sha256::new();
    hasher.update(hostname.as_bytes());
    hasher.update(cpu_brand.as_bytes());
    hasher.update(sys.total_memory().to_le_bytes());

    let hash = hasher.finalize();
    format!("{:x}", hash)[..16].to_string()
}

fn is_headless_environment() -> bool {
    std::env::var("DISPLAY").is_err()
        && std::env::var("WAYLAND_DISPLAY").is_err()
        && !cfg!(target_os = "windows")
        && !cfg!(target_os = "macos")
}

fn is_container_environment() -> bool {
    std::path::Path::new("/.dockerenv").exists()
        || std::fs::read_to_string("/proc/1/cgroup")
            .map(|s| s.contains("docker") || s.contains("kubepods") || s.contains("containerd"))
            .unwrap_or(false)
}

fn fs_available_space_mb(path: &std::path::Path) -> u64 {
    // Use sysinfo disk info
    let disks = sysinfo::Disks::new_with_refreshed_list();
    for disk in disks.list() {
        if path.starts_with(disk.mount_point()) {
            return disk.available_space() / (1024 * 1024);
        }
    }
    // Fallback: return 0 if we can't determine
    0
}

use tauri::Manager;

// ─── Alternative Model Suggestions ─────────────────────────────────────────

/// A suggested alternative model when the requested model is incompatible.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AlternativeModelSuggestion {
    pub model_id: String,
    pub model_name: String,
    pub parameter_count_b: f64,
    pub quantization: String,
    pub required_vram_mb: u64,
    pub required_ram_mb: u64,
    pub compatibility_class: ModelCompatibilityClass,
    pub estimated_tokens_per_sec: f64,
    pub reason: String,
}

/// Quantization levels ordered from highest quality to most compressed.
const QUANTIZATION_LEVELS: &[&str] = &["f16", "q8", "q4", "q2"];

/// Approximate VRAM/RAM reduction factors for each quantization step down.
fn quantization_reduction_factor(from: &str, to: &str) -> f64 {
    let index_of = |q: &str| -> usize {
        QUANTIZATION_LEVELS.iter().position(|&x| x == q).unwrap_or(0)
    };
    let from_idx = index_of(from);
    let to_idx = index_of(to);
    if to_idx <= from_idx {
        return 1.0;
    }
    // Each step roughly halves the memory requirement
    // f16 -> q8: ~0.5x, q8 -> q4: ~0.5x, q4 -> q2: ~0.5x
    0.5_f64.powi((to_idx - from_idx) as i32)
}

/// Suggest alternative models when the requested model is incompatible.
///
/// Strategy:
/// 1. Try smaller quantization variants of the same model
/// 2. Suggest smaller models from the known model list that are compatible
pub fn suggest_alternatives(
    incompatible_model: &ModelRequirements,
    all_models: &[ModelRequirements],
    profile: &HardwareProfile,
) -> Vec<AlternativeModelSuggestion> {
    let mut suggestions = Vec::new();
    let _gpu_vram = profile.gpu.as_ref().map(|g| g.available_vram_mb).unwrap_or(0);
    let _available_ram = profile.memory.available_ram_mb;

    // Strategy 1: Suggest smaller quantization variants of the same model
    let current_quant_idx = QUANTIZATION_LEVELS
        .iter()
        .position(|&q| q == incompatible_model.quantization)
        .unwrap_or(0);

    for &quant in &QUANTIZATION_LEVELS[current_quant_idx + 1..] {
        let factor = quantization_reduction_factor(&incompatible_model.quantization, quant);
        let reduced_vram = (incompatible_model.min_vram_mb as f64 * factor) as u64;
        let reduced_ram = (incompatible_model.min_ram_mb as f64 * factor) as u64;

        // Check if this quantization would be compatible
        let synthetic_req = ModelRequirements {
            model_id: format!("{}-{}", incompatible_model.model_id, quant),
            model_name: format!("{} ({})", incompatible_model.model_name, quant),
            parameter_count_b: incompatible_model.parameter_count_b,
            quantization: quant.to_string(),
            min_vram_mb: reduced_vram,
            min_ram_mb: reduced_ram,
            min_compute_capability: incompatible_model.min_compute_capability.clone(),
        };

        let entry = compute_model_compatibility(&synthetic_req, profile);
        if entry.compatibility_class != ModelCompatibilityClass::Incompatible {
            suggestions.push(AlternativeModelSuggestion {
                model_id: synthetic_req.model_id,
                model_name: synthetic_req.model_name,
                parameter_count_b: synthetic_req.parameter_count_b,
                quantization: quant.to_string(),
                required_vram_mb: reduced_vram,
                required_ram_mb: reduced_ram,
                compatibility_class: entry.compatibility_class,
                estimated_tokens_per_sec: entry.estimated_tokens_per_sec,
                reason: format!(
                    "Same model with {} quantization fits in available resources",
                    quant
                ),
            });
        }
    }

    // Strategy 2: Suggest smaller models from the known list that are compatible
    for model in all_models {
        // Skip the incompatible model itself
        if model.model_id == incompatible_model.model_id {
            continue;
        }
        // Only suggest models that are smaller
        if model.parameter_count_b >= incompatible_model.parameter_count_b {
            continue;
        }

        let entry = compute_model_compatibility(model, profile);
        if entry.compatibility_class != ModelCompatibilityClass::Incompatible {
            suggestions.push(AlternativeModelSuggestion {
                model_id: model.model_id.clone(),
                model_name: model.model_name.clone(),
                parameter_count_b: model.parameter_count_b,
                quantization: model.quantization.clone(),
                required_vram_mb: model.min_vram_mb,
                required_ram_mb: model.min_ram_mb,
                compatibility_class: entry.compatibility_class,
                estimated_tokens_per_sec: entry.estimated_tokens_per_sec,
                reason: format!(
                    "Smaller model ({:.1}B params) that fits current hardware",
                    model.parameter_count_b
                ),
            });
        }
    }

    // Sort by estimated speed (descending) — best alternatives first
    suggestions.sort_by(|a, b| {
        b.estimated_tokens_per_sec
            .partial_cmp(&a.estimated_tokens_per_sec)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Limit to top 5 suggestions
    suggestions.truncate(5);
    suggestions
}

// ─── Hardware Change Detection ──────────────────────────────────────────────

/// Severity of a hardware change.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ChangeSeverity {
    /// Informational — no action needed (e.g., minor clock speed variance).
    Info,
    /// Warning — may affect performance (e.g., less RAM available).
    Warning,
    /// Critical — requires re-classification (e.g., GPU added/removed).
    Critical,
}

/// A detected change between two hardware profiles.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HardwareChange {
    pub field: String,
    pub old_value: String,
    pub new_value: String,
    pub severity: ChangeSeverity,
}

/// Compare two hardware profiles and return a list of significant changes.
/// Used at startup to detect hardware modifications since last run.
pub fn detect_hardware_changes(
    current: &HardwareProfile,
    stored: &HardwareProfile,
) -> Vec<HardwareChange> {
    let mut changes = Vec::new();

    // CPU changes
    if current.cpu.physical_cores != stored.cpu.physical_cores {
        changes.push(HardwareChange {
            field: "cpu.physicalCores".to_string(),
            old_value: stored.cpu.physical_cores.to_string(),
            new_value: current.cpu.physical_cores.to_string(),
            severity: ChangeSeverity::Warning,
        });
    }

    if current.cpu.model_name != stored.cpu.model_name {
        changes.push(HardwareChange {
            field: "cpu.modelName".to_string(),
            old_value: stored.cpu.model_name.clone(),
            new_value: current.cpu.model_name.clone(),
            severity: ChangeSeverity::Critical,
        });
    }

    if current.cpu.architecture != stored.cpu.architecture {
        changes.push(HardwareChange {
            field: "cpu.architecture".to_string(),
            old_value: stored.cpu.architecture.clone(),
            new_value: current.cpu.architecture.clone(),
            severity: ChangeSeverity::Critical,
        });
    }

    // Memory changes
    let ram_diff = (current.memory.total_ram_mb as i64 - stored.memory.total_ram_mb as i64).unsigned_abs();
    if ram_diff > 1024 {
        // More than 1GB difference
        let severity = if ram_diff > 8 * 1024 {
            ChangeSeverity::Critical
        } else {
            ChangeSeverity::Warning
        };
        changes.push(HardwareChange {
            field: "memory.totalRamMb".to_string(),
            old_value: stored.memory.total_ram_mb.to_string(),
            new_value: current.memory.total_ram_mb.to_string(),
            severity,
        });
    }

    // GPU changes
    match (&current.gpu, &stored.gpu) {
        (Some(curr_gpu), Some(stored_gpu)) => {
            if curr_gpu.model_name != stored_gpu.model_name {
                changes.push(HardwareChange {
                    field: "gpu.modelName".to_string(),
                    old_value: stored_gpu.model_name.clone(),
                    new_value: curr_gpu.model_name.clone(),
                    severity: ChangeSeverity::Critical,
                });
            }
            let vram_diff = (curr_gpu.total_vram_mb as i64 - stored_gpu.total_vram_mb as i64).unsigned_abs();
            if vram_diff > 1024 {
                changes.push(HardwareChange {
                    field: "gpu.totalVramMb".to_string(),
                    old_value: stored_gpu.total_vram_mb.to_string(),
                    new_value: curr_gpu.total_vram_mb.to_string(),
                    severity: ChangeSeverity::Warning,
                });
            }
            if curr_gpu.driver_version != stored_gpu.driver_version {
                changes.push(HardwareChange {
                    field: "gpu.driverVersion".to_string(),
                    old_value: stored_gpu.driver_version.clone(),
                    new_value: curr_gpu.driver_version.clone(),
                    severity: ChangeSeverity::Info,
                });
            }
        }
        (Some(_), None) => {
            changes.push(HardwareChange {
                field: "gpu".to_string(),
                old_value: "none".to_string(),
                new_value: "detected".to_string(),
                severity: ChangeSeverity::Critical,
            });
        }
        (None, Some(_)) => {
            changes.push(HardwareChange {
                field: "gpu".to_string(),
                old_value: "detected".to_string(),
                new_value: "none".to_string(),
                severity: ChangeSeverity::Critical,
            });
        }
        (None, None) => {}
    }

    // Storage type change
    if current.storage.storage_type != stored.storage.storage_type {
        changes.push(HardwareChange {
            field: "storage.storageType".to_string(),
            old_value: stored.storage.storage_type.clone(),
            new_value: current.storage.storage_type.clone(),
            severity: ChangeSeverity::Warning,
        });
    }

    // Hardware class change (derived, but important to flag)
    if current.hardware_class != stored.hardware_class {
        changes.push(HardwareChange {
            field: "hardwareClass".to_string(),
            old_value: format!("{:?}", stored.hardware_class),
            new_value: format!("{:?}", current.hardware_class),
            severity: ChangeSeverity::Critical,
        });
    }

    changes
}
