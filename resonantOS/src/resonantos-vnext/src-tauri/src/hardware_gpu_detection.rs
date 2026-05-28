//! GPU detection module for Phase 7 Hardware Stability.
//!
//! Detects GPU capabilities using platform-specific APIs:
//! - NVIDIA: via nvml-sys/nvml-wrapper (NVML library)
//! - AMD: via rocm-smi CLI parsing
//! - macOS: via IOKit Metal device enumeration
//! - Vulkan: via ash/vulkan-sys for compute capability
//!
//! Falls back gracefully when no GPU or no driver is available.


use crate::hardware_service::GpuProfile;

/// Attempt to detect GPU using all available methods.
/// Returns None if no discrete GPU is found or all detection methods fail.
pub fn detect_gpu_full() -> Option<GpuProfile> {
    // Try NVIDIA first (most common for AI workloads)
    if let Some(gpu) = detect_nvidia_gpu() {
        return Some(gpu);
    }

    // Try macOS Metal
    #[cfg(target_os = "macos")]
    if let Some(gpu) = detect_metal_gpu() {
        return Some(gpu);
    }

    // Try AMD ROCm
    if let Some(gpu) = detect_amd_gpu() {
        return Some(gpu);
    }

    // No GPU detected
    None
}

/// Detect NVIDIA GPU via NVML library.
/// NVML is available when NVIDIA drivers are installed.
fn detect_nvidia_gpu() -> Option<GpuProfile> {
    // Attempt to load NVML dynamically (don't hard-link — graceful if not present)
    // On Windows: nvml.dll in System32 or NVIDIA driver path
    // On Linux: libnvidia-ml.so.1
    let nvml_path = find_nvml_library()?;

    // Use libloading to dynamically load NVML
    let lib = unsafe { libloading::Library::new(&nvml_path).ok()? };

    // nvmlInit_v2
    let nvml_init: libloading::Symbol<unsafe extern "C" fn() -> u32> =
        unsafe { lib.get(b"nvmlInit_v2").ok()? };
    let result = unsafe { nvml_init() };
    if result != 0 {
        return None; // NVML init failed
    }

    // nvmlDeviceGetCount_v2
    let nvml_device_count: libloading::Symbol<unsafe extern "C" fn(*mut u32) -> u32> =
        unsafe { lib.get(b"nvmlDeviceGetCount_v2").ok()? };
    let mut count: u32 = 0;
    let result = unsafe { nvml_device_count(&mut count) };
    if result != 0 || count == 0 {
        nvml_shutdown(&lib);
        return None;
    }

    // nvmlDeviceGetHandleByIndex_v2
    let nvml_get_handle: libloading::Symbol<unsafe extern "C" fn(u32, *mut usize) -> u32> =
        unsafe { lib.get(b"nvmlDeviceGetHandleByIndex_v2").ok()? };
    let mut handle: usize = 0;
    let result = unsafe { nvml_get_handle(0, &mut handle) };
    if result != 0 {
        nvml_shutdown(&lib);
        return None;
    }

    // Get device name
    let model_name = nvml_get_device_name(&lib, handle).unwrap_or_else(|| "NVIDIA GPU".to_string());

    // Get memory info
    let (total_vram_mb, available_vram_mb) = nvml_get_memory_info(&lib, handle).unwrap_or((0, 0));

    // Get driver version
    let driver_version = nvml_get_driver_version(&lib).unwrap_or_else(|| "unknown".to_string());

    // Get CUDA version
    let cuda_version = nvml_get_cuda_version(&lib);

    // Get compute capability
    let compute_capability = nvml_get_compute_capability(&lib, handle);

    nvml_shutdown(&lib);

    Some(GpuProfile {
        model_name,
        total_vram_mb,
        available_vram_mb,
        compute_capability,
        driver_version,
        cuda_version,
        rocm_version: None,
        metal_support: false,
        vulkan_compute: true, // NVIDIA GPUs support Vulkan compute
    })
}

/// Detect AMD GPU via rocm-smi CLI.
fn detect_amd_gpu() -> Option<GpuProfile> {
    // Try running rocm-smi to get GPU info
    let output = std::process::Command::new("rocm-smi")
        .arg("--showproductname")
        .arg("--showmeminfo")
        .arg("vram")
        .arg("--csv")
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Parse rocm-smi CSV output for GPU name and VRAM
    let model_name = parse_rocm_product_name(&stdout)?;
    let (total_vram_mb, available_vram_mb) = parse_rocm_vram(&stdout)?;

    // Get ROCm version
    let rocm_version = get_rocm_version();

    Some(GpuProfile {
        model_name,
        total_vram_mb,
        available_vram_mb,
        compute_capability: None,
        driver_version: rocm_version.clone().unwrap_or_else(|| "unknown".to_string()),
        cuda_version: None,
        rocm_version,
        metal_support: false,
        vulkan_compute: true,
    })
}

/// Detect macOS Metal GPU.
#[cfg(target_os = "macos")]
fn detect_metal_gpu() -> Option<GpuProfile> {
    // Use system_profiler to get GPU info on macOS
    let output = std::process::Command::new("system_profiler")
        .arg("SPDisplaysDataType")
        .arg("-json")
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let json: serde_json::Value = serde_json::from_str(&stdout).ok()?;

    // Parse the display data for GPU info
    let displays = json.get("SPDisplaysDataType")?.as_array()?;
    let gpu = displays.first()?;

    let model_name = gpu.get("sppci_model")?.as_str()?.to_string();

    // macOS unified memory — VRAM is shared with system RAM
    // Report the Metal-accessible portion (typically total RAM for Apple Silicon)
    let vram_str = gpu.get("spdisplays_vram")
        .and_then(|v| v.as_str())
        .unwrap_or("0");
    let total_vram_mb = parse_vram_string(vram_str);

    Some(GpuProfile {
        model_name,
        total_vram_mb,
        available_vram_mb: total_vram_mb, // approximate — macOS doesn't expose free VRAM easily
        compute_capability: None,
        driver_version: "macOS-native".to_string(),
        cuda_version: None,
        rocm_version: None,
        metal_support: true,
        vulkan_compute: false, // MoltenVK exists but Metal is primary
    })
}

// ─── NVML Helper Functions ──────────────────────────────────────────────────

pub fn find_nvml_library() -> Option<String> {
    #[cfg(target_os = "windows")]
    {
        // Windows: nvml.dll in System32 or NVIDIA path
        let system32 = std::env::var("SystemRoot")
            .map(|r| format!("{r}\\System32\\nvml.dll"))
            .unwrap_or_else(|_| "C:\\Windows\\System32\\nvml.dll".to_string());
        if std::path::Path::new(&system32).exists() {
            return Some(system32);
        }
        // Try NVIDIA program files path
        let nvidia_path = "C:\\Program Files\\NVIDIA Corporation\\NVSMI\\nvml.dll";
        if std::path::Path::new(nvidia_path).exists() {
            return Some(nvidia_path.to_string());
        }
        None
    }
    #[cfg(target_os = "linux")]
    {
        let paths = [
            "/usr/lib/x86_64-linux-gnu/libnvidia-ml.so.1",
            "/usr/lib64/libnvidia-ml.so.1",
            "/usr/lib/libnvidia-ml.so.1",
        ];
        for path in &paths {
            if std::path::Path::new(path).exists() {
                return Some(path.to_string());
            }
        }
        None
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        None
    }
}

fn nvml_get_device_name(lib: &libloading::Library, handle: usize) -> Option<String> {
    let nvml_get_name: libloading::Symbol<unsafe extern "C" fn(usize, *mut u8, u32) -> u32> =
        unsafe { lib.get(b"nvmlDeviceGetName").ok()? };
    let mut name_buf = [0u8; 256];
    let result = unsafe { nvml_get_name(handle, name_buf.as_mut_ptr(), 256) };
    if result != 0 {
        return None;
    }
    let name = std::ffi::CStr::from_bytes_until_nul(&name_buf)
        .ok()?
        .to_str()
        .ok()?
        .to_string();
    Some(name)
}

fn nvml_get_memory_info(lib: &libloading::Library, handle: usize) -> Option<(u64, u64)> {
    // nvmlMemory_t struct: { total: u64, free: u64, used: u64 }
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
    if result != 0 {
        return None;
    }
    Some((mem.total / (1024 * 1024), mem.free / (1024 * 1024)))
}

fn nvml_get_driver_version(lib: &libloading::Library) -> Option<String> {
    let nvml_get_driver: libloading::Symbol<unsafe extern "C" fn(*mut u8, u32) -> u32> =
        unsafe { lib.get(b"nvmlSystemGetDriverVersion").ok()? };
    let mut buf = [0u8; 128];
    let result = unsafe { nvml_get_driver(buf.as_mut_ptr(), 128) };
    if result != 0 {
        return None;
    }
    std::ffi::CStr::from_bytes_until_nul(&buf)
        .ok()
        .and_then(|s| s.to_str().ok())
        .map(|s| s.to_string())
}

fn nvml_get_cuda_version(lib: &libloading::Library) -> Option<String> {
    let nvml_get_cuda: libloading::Symbol<unsafe extern "C" fn(*mut i32) -> u32> =
        unsafe { lib.get(b"nvmlSystemGetCudaDriverVersion_v2").ok()? };
    let mut version: i32 = 0;
    let result = unsafe { nvml_get_cuda(&mut version) };
    if result != 0 {
        return None;
    }
    let major = version / 1000;
    let minor = (version % 1000) / 10;
    Some(format!("{major}.{minor}"))
}

fn nvml_get_compute_capability(lib: &libloading::Library, handle: usize) -> Option<String> {
    let nvml_get_cc: libloading::Symbol<unsafe extern "C" fn(usize, *mut i32, *mut i32) -> u32> =
        unsafe { lib.get(b"nvmlDeviceGetCudaComputeCapability").ok()? };
    let mut major: i32 = 0;
    let mut minor: i32 = 0;
    let result = unsafe { nvml_get_cc(handle, &mut major, &mut minor) };
    if result != 0 {
        return None;
    }
    Some(format!("{major}.{minor}"))
}

fn nvml_shutdown(lib: &libloading::Library) {
    if let Ok(shutdown) = unsafe { lib.get::<unsafe extern "C" fn() -> u32>(b"nvmlShutdown") } {
        unsafe { shutdown() };
    }
}

// ─── ROCm Helper Functions ──────────────────────────────────────────────────

fn parse_rocm_product_name(output: &str) -> Option<String> {
    // rocm-smi --showproductname outputs lines like:
    // GPU[0] : Card Series: AMD Radeon RX 7900 XTX
    for line in output.lines() {
        if line.contains("Card Series") || line.contains("Card series") {
            return line.split(':').last().map(|s| s.trim().to_string());
        }
    }
    None
}

fn parse_rocm_vram(output: &str) -> Option<(u64, u64)> {
    // Parse VRAM total and used from rocm-smi output
    let mut total: u64 = 0;
    let mut used: u64 = 0;
    for line in output.lines() {
        if line.contains("Total") && line.contains("VRAM") {
            if let Some(val) = extract_mb_value(line) {
                total = val;
            }
        }
        if line.contains("Used") && line.contains("VRAM") {
            if let Some(val) = extract_mb_value(line) {
                used = val;
            }
        }
    }
    if total > 0 {
        Some((total, total.saturating_sub(used)))
    } else {
        None
    }
}

fn extract_mb_value(line: &str) -> Option<u64> {
    // Extract numeric MB value from a line
    line.split_whitespace()
        .find_map(|word| word.parse::<u64>().ok())
}

fn get_rocm_version() -> Option<String> {
    let output = std::process::Command::new("rocm-smi")
        .arg("--showversion")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        if line.contains("ROCm") || line.contains("Driver") {
            return Some(line.trim().to_string());
        }
    }
    None
}

#[cfg(target_os = "macos")]
fn parse_vram_string(s: &str) -> u64 {
    // Parse strings like "16 GB" or "8192 MB"
    let parts: Vec<&str> = s.split_whitespace().collect();
    if parts.len() >= 2 {
        let value: u64 = parts[0].parse().unwrap_or(0);
        match parts[1].to_uppercase().as_str() {
            "GB" => value * 1024,
            "MB" => value,
            _ => value,
        }
    } else {
        0
    }
}
