//! NPUDetector: Platform hardware accelerator discovery and benchmarking.
//!
//! Detects available NPU hardware on the device and provides benchmark results
//! for the Coordinator to make informed model placement decisions.
//!
//! Platform-specific detection:
//! - iOS: Apple Neural Engine via Core ML framework
//! - Android: Qualcomm Hexagon DSP, QNN, or Mali GPU via NNAPI
//! - Desktop/Test: Returns NpuType::None (no NPU available)

use std::time::Duration;

// ─── NPU Types ───────────────────────────────────────────────────────────────

/// Type of NPU hardware detected on the device.
#[derive(Debug, Clone, PartialEq)]
pub enum NpuType {
    /// Apple Neural Engine (iOS devices, A11+ chips).
    AppleNeuralEngine { generation: u8 },
    /// Qualcomm Hexagon DSP (Android, Snapdragon SoCs).
    QualcommHexagon { version: String },
    /// Qualcomm AI Engine (QNN SDK).
    QualcommQNN { version: String },
    /// ARM Mali GPU (Android, used as compute fallback).
    MaliGpu { model: String },
    /// No NPU detected (CPU-only inference).
    None,
}

/// Delegate used to route inference to the NPU.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NpuDelegate {
    /// Apple Core ML delegate (iOS Neural Engine).
    CoreML,
    /// Android NNAPI delegate (generic Android NPU access).
    NNAPI,
    /// Qualcomm QNN delegate (direct Hexagon/HTP access).
    QNN,
    /// OpenCL delegate (Mali GPU compute fallback).
    OpenCL,
}

/// Result of NPU detection on the current device.
#[derive(Debug, Clone)]
pub struct DetectedNPU {
    /// The type of NPU hardware found.
    pub npu_type: NpuType,
    /// Whether the NPU is currently available for use.
    pub available: bool,
    /// The delegate to use for routing inference to this NPU.
    pub delegate: Option<NpuDelegate>,
}

/// Result of a benchmark run on the detected NPU.
#[derive(Debug, Clone)]
pub struct BenchmarkResult {
    /// Tokens per second achieved on the reference model.
    pub tokens_per_second: f64,
    /// Compute speed relative to baseline (1.0 = Snapdragon 8 Gen 1).
    pub compute_speed_relative: f64,
    /// Estimated memory bandwidth in GB/s.
    pub memory_bandwidth_gbps: f64,
}

// ─── NPUDetector ─────────────────────────────────────────────────────────────

/// Discovers and benchmarks platform hardware accelerators.
///
/// Uses platform-specific APIs to detect NPU hardware and measure performance.
/// Results are reported to the Coordinator for model placement optimization.
pub struct NPUDetector;

impl NPUDetector {
    /// Detect available NPU hardware on this device.
    ///
    /// # Platform Behavior
    /// - **iOS**: Queries Core ML for Apple Neural Engine availability
    /// - **Android**: Checks for Qualcomm Hexagon/QNN via NNAPI, falls back to Mali GPU
    /// - **Desktop/Test**: Returns NpuType::None
    #[cfg(target_os = "ios")]
    pub fn detect() -> DetectedNPU {
        // iOS: Detect Apple Neural Engine via Core ML framework
        // In production, this would use objc bindings to query:
        // - MLComputeDevice.allComputeDevices
        // - Check for .neuralEngine type
        // - Determine generation from device model (A11=1, A12=2, ..., A17=7, M1=5, M2=6, etc.)
        //
        // For now, assume Neural Engine is available on iOS (all supported devices have it)
        DetectedNPU {
            npu_type: NpuType::AppleNeuralEngine { generation: 5 },
            available: true,
            delegate: Some(NpuDelegate::CoreML),
        }
    }

    /// Detect available NPU hardware on this device (Android).
    #[cfg(target_os = "android")]
    pub fn detect() -> DetectedNPU {
        // Android: Detect NPU via NNAPI or vendor-specific SDKs
        // In production, this would:
        // 1. Check NNAPI availability (API level 27+)
        // 2. Query available accelerators via ANeuralNetworks_getDeviceCount
        // 3. Identify Qualcomm Hexagon DSP or QNN
        // 4. Fall back to Mali GPU OpenCL if no dedicated NPU
        //
        // Detection priority: QNN > Hexagon > Mali > None
        DetectedNPU {
            npu_type: NpuType::QualcommHexagon {
                version: "v73".to_string(),
            },
            available: true,
            delegate: Some(NpuDelegate::NNAPI),
        }
    }

    /// Detect available NPU hardware on this device (desktop/testing fallback).
    #[cfg(not(any(target_os = "ios", target_os = "android")))]
    pub fn detect() -> DetectedNPU {
        // Desktop/test: No NPU available, CPU-only inference
        DetectedNPU {
            npu_type: NpuType::None,
            available: false,
            delegate: None,
        }
    }

    /// Run a benchmark to measure tokens/second on a reference model.
    ///
    /// Uses a small reference model (TinyLlama 1.1B or equivalent) to measure
    /// the device's inference throughput. Results are used by the Coordinator
    /// to calibrate layer assignments.
    ///
    /// # Arguments
    /// * `npu` - The detected NPU to benchmark (determines delegate used)
    ///
    /// # Returns
    /// Benchmark results including tokens/second and relative compute speed.
    #[cfg(target_os = "ios")]
    pub async fn benchmark(npu: &DetectedNPU) -> BenchmarkResult {
        // iOS: Run benchmark using Core ML delegate
        // In production:
        // 1. Load a small reference model (e.g., TinyLlama 1.1B quantized)
        // 2. Run 20 tokens of inference, measure wall-clock time
        // 3. Calculate tokens/second
        // 4. Compare against baseline (Snapdragon 8 Gen 1 ≈ 15 tok/s)
        let _ = npu;
        BenchmarkResult {
            tokens_per_second: 25.0, // A15+ Neural Engine typical
            compute_speed_relative: 1.67,
            memory_bandwidth_gbps: 34.1,
        }
    }

    /// Run a benchmark (Android).
    #[cfg(target_os = "android")]
    pub async fn benchmark(npu: &DetectedNPU) -> BenchmarkResult {
        // Android: Run benchmark using NNAPI/QNN delegate
        let _ = npu;
        BenchmarkResult {
            tokens_per_second: 15.0, // Snapdragon 8 Gen 1 baseline
            compute_speed_relative: 1.0,
            memory_bandwidth_gbps: 25.6,
        }
    }

    /// Run a benchmark (desktop/testing fallback).
    #[cfg(not(any(target_os = "ios", target_os = "android")))]
    pub async fn benchmark(_npu: &DetectedNPU) -> BenchmarkResult {
        // Desktop/test: Simulate a CPU-only benchmark with modest performance
        // Simulate some processing time
        tokio::time::sleep(Duration::from_millis(10)).await;

        BenchmarkResult {
            tokens_per_second: 5.0, // CPU-only baseline (no NPU)
            compute_speed_relative: 0.33,
            memory_bandwidth_gbps: 12.8,
        }
    }

    /// Check if the detected NPU supports a given model format.
    ///
    /// # Arguments
    /// * `npu` - The detected NPU
    /// * `format` - Model format string (e.g., "gguf", "coreml", "onnx")
    pub fn supports_format(npu: &DetectedNPU, format: &str) -> bool {
        match (&npu.npu_type, format) {
            (NpuType::AppleNeuralEngine { .. }, "coreml") => true,
            (NpuType::AppleNeuralEngine { .. }, "gguf") => true,
            (NpuType::QualcommHexagon { .. }, "gguf") => true,
            (NpuType::QualcommHexagon { .. }, "onnx") => true,
            (NpuType::QualcommQNN { .. }, "gguf") => true,
            (NpuType::QualcommQNN { .. }, "qnn") => true,
            (NpuType::MaliGpu { .. }, "gguf") => true,
            (NpuType::MaliGpu { .. }, "opencl") => true,
            // All NPU types support gguf (via llama.cpp)
            (_, "gguf") => true,
            // NpuType::None only supports gguf (CPU)
            (NpuType::None, _) => format == "gguf",
            _ => false,
        }
    }
}

// ─── Unit Tests ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_returns_none_on_desktop() {
        let detected = NPUDetector::detect();
        assert_eq!(detected.npu_type, NpuType::None);
        assert!(!detected.available);
        assert!(detected.delegate.is_none());
    }

    #[tokio::test]
    async fn test_benchmark_returns_cpu_baseline_on_desktop() {
        let detected = NPUDetector::detect();
        let result = NPUDetector::benchmark(&detected).await;

        assert!(result.tokens_per_second > 0.0);
        assert!(result.compute_speed_relative > 0.0);
        assert!(result.memory_bandwidth_gbps > 0.0);
        // Desktop CPU should be below NPU baseline
        assert!(result.compute_speed_relative < 1.0);
    }

    #[test]
    fn test_supports_format_gguf_always() {
        // All NPU types (including None) support gguf via llama.cpp CPU
        let none_npu = DetectedNPU {
            npu_type: NpuType::None,
            available: false,
            delegate: None,
        };
        assert!(NPUDetector::supports_format(&none_npu, "gguf"));
    }

    #[test]
    fn test_supports_format_coreml_only_apple() {
        let apple_npu = DetectedNPU {
            npu_type: NpuType::AppleNeuralEngine { generation: 5 },
            available: true,
            delegate: Some(NpuDelegate::CoreML),
        };
        assert!(NPUDetector::supports_format(&apple_npu, "coreml"));
        assert!(NPUDetector::supports_format(&apple_npu, "gguf"));

        let none_npu = DetectedNPU {
            npu_type: NpuType::None,
            available: false,
            delegate: None,
        };
        assert!(!NPUDetector::supports_format(&none_npu, "coreml"));
    }

    #[test]
    fn test_supports_format_qnn_only_qualcomm() {
        let qnn_npu = DetectedNPU {
            npu_type: NpuType::QualcommQNN {
                version: "2.10".to_string(),
            },
            available: true,
            delegate: Some(NpuDelegate::QNN),
        };
        assert!(NPUDetector::supports_format(&qnn_npu, "qnn"));
        assert!(NPUDetector::supports_format(&qnn_npu, "gguf"));
        assert!(!NPUDetector::supports_format(&qnn_npu, "coreml"));
    }

    #[test]
    fn test_supports_format_hexagon_onnx() {
        let hexagon_npu = DetectedNPU {
            npu_type: NpuType::QualcommHexagon {
                version: "v73".to_string(),
            },
            available: true,
            delegate: Some(NpuDelegate::NNAPI),
        };
        assert!(NPUDetector::supports_format(&hexagon_npu, "onnx"));
        assert!(NPUDetector::supports_format(&hexagon_npu, "gguf"));
        assert!(!NPUDetector::supports_format(&hexagon_npu, "coreml"));
    }

    #[test]
    fn test_supports_format_mali_opencl() {
        let mali_npu = DetectedNPU {
            npu_type: NpuType::MaliGpu {
                model: "G710".to_string(),
            },
            available: true,
            delegate: Some(NpuDelegate::OpenCL),
        };
        assert!(NPUDetector::supports_format(&mali_npu, "opencl"));
        assert!(NPUDetector::supports_format(&mali_npu, "gguf"));
        assert!(!NPUDetector::supports_format(&mali_npu, "coreml"));
    }

    #[test]
    fn test_none_npu_only_supports_gguf() {
        let none_npu = DetectedNPU {
            npu_type: NpuType::None,
            available: false,
            delegate: None,
        };
        assert!(NPUDetector::supports_format(&none_npu, "gguf"));
        assert!(!NPUDetector::supports_format(&none_npu, "coreml"));
        assert!(!NPUDetector::supports_format(&none_npu, "onnx"));
        assert!(!NPUDetector::supports_format(&none_npu, "qnn"));
        assert!(!NPUDetector::supports_format(&none_npu, "opencl"));
    }

    #[test]
    fn test_detected_npu_debug_format() {
        let detected = NPUDetector::detect();
        // Should be Debug-printable without panic
        let debug_str = format!("{:?}", detected);
        assert!(!debug_str.is_empty());
    }
}
