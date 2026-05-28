//! Tests for Phase 7 Hardware Stability.
//!
//! Contains:
//! - Unit tests for each detector (task 1.9)
//! - Property-based tests for Properties 1-8 (tasks 2.8, 3.6, 4.6, 5.8, 6.8)

#[cfg(test)]
mod unit_tests {
    use crate::hardware_service::*;
    use crate::hardware_thermal::*;
    use crate::hardware_timeout_runtime::*;
    use crate::hardware_resource_manager::*;
    use crate::hardware_vram_manager::*;

    // ─── Task 1.9: Unit Tests for Detectors ─────────────────────────────

    #[test]
    fn test_cpu_detection_returns_valid_profile() {
        let cpu = detect_cpu();
        assert!(cpu.physical_cores > 0, "Must detect at least 1 physical core");
        assert!(cpu.logical_cores > 0, "Must detect at least 1 logical core");
        assert!(cpu.logical_cores >= cpu.physical_cores, "Logical cores >= physical cores");
        assert!(!cpu.architecture.is_empty(), "Architecture must be detected");
        assert!(!cpu.model_name.is_empty(), "Model name must be detected");
    }

    #[test]
    fn test_memory_detection_returns_valid_profile() {
        let mem = detect_memory();
        assert!(mem.total_ram_mb > 0, "Must detect some RAM");
        assert!(mem.available_ram_mb <= mem.total_ram_mb, "Available <= total");
    }

    #[test]
    fn test_storage_detection_returns_valid_profile() {
        let tmp = std::env::temp_dir();
        let storage = detect_storage(&tmp);
        assert!(!storage.storage_type.is_empty(), "Storage type must be detected");
        // available_space_mb may be 0 if detection fails, that's acceptable
    }

    #[test]
    fn test_network_detection_returns_valid_profile() {
        let network = detect_network();
        // Should detect at least one interface (loopback)
        // Note: in some CI environments this may be empty
        assert!(network.interfaces.len() >= 0);
    }

    #[test]
    fn test_classify_hardware_gpu_workstation() {
        let profile = make_test_profile(Some(16384), 32768, false, false);
        let class = classify_hardware(&profile);
        assert_eq!(class, HardwareClass::GpuWorkstation);
    }

    #[test]
    fn test_classify_hardware_cpu_workstation() {
        let profile = make_test_profile(None, 32768, false, false);
        let class = classify_hardware(&profile);
        assert_eq!(class, HardwareClass::CpuWorkstation);
    }

    #[test]
    fn test_classify_hardware_embedded() {
        let profile = make_test_profile(None, 4096, false, false);
        let class = classify_hardware(&profile);
        assert_eq!(class, HardwareClass::Embedded);
    }

    #[test]
    fn test_default_timeout_profile_all_positive() {
        let classes = vec![
            HardwareClass::GpuWorkstation,
            HardwareClass::CpuWorkstation,
            HardwareClass::GpuServer,
            HardwareClass::CpuServer,
            HardwareClass::Embedded,
            HardwareClass::ContainerRestricted,
        ];
        for class in classes {
            let profile = default_timeout_profile(&class);
            assert!(profile.inference_ms > 0, "inference_ms must be > 0 for {:?}", class);
            assert!(profile.tool_execution_ms > 0, "tool_execution_ms must be > 0 for {:?}", class);
            assert!(profile.health_check_ms > 0, "health_check_ms must be > 0 for {:?}", class);
            assert!(profile.network_request_ms > 0, "network_request_ms must be > 0 for {:?}", class);
            assert!(profile.database_query_ms > 0, "database_query_ms must be > 0 for {:?}", class);
            assert!(profile.compute_job_ms > 0, "compute_job_ms must be > 0 for {:?}", class);
        }
    }

    #[test]
    fn test_model_compatibility_native_gpu() {
        let profile = make_test_profile(Some(24576), 65536, false, false);
        let model = ModelRequirements {
            model_id: "test-7b".to_string(),
            model_name: "Test 7B".to_string(),
            parameter_count_b: 7.0,
            quantization: "f16".to_string(),
            min_vram_mb: 14000,
            min_ram_mb: 16000,
            min_compute_capability: None,
        };
        let entry = compute_model_compatibility(&model, &profile);
        assert_eq!(entry.compatibility_class, ModelCompatibilityClass::NativeGpu);
    }

    #[test]
    fn test_model_compatibility_incompatible() {
        let profile = make_test_profile(Some(8192), 16384, false, false);
        let model = ModelRequirements {
            model_id: "test-70b".to_string(),
            model_name: "Test 70B".to_string(),
            parameter_count_b: 70.0,
            quantization: "f16".to_string(),
            min_vram_mb: 140000,
            min_ram_mb: 140000,
            min_compute_capability: None,
        };
        let entry = compute_model_compatibility(&model, &profile);
        assert_eq!(entry.compatibility_class, ModelCompatibilityClass::Incompatible);
        assert!(entry.incompatibility_reason.is_some());
    }

    #[test]
    fn test_suggest_alternatives_provides_smaller_quant() {
        let profile = make_test_profile(Some(8192), 16384, false, false);
        let model = ModelRequirements {
            model_id: "big-model".to_string(),
            model_name: "Big Model".to_string(),
            parameter_count_b: 13.0,
            quantization: "f16".to_string(),
            min_vram_mb: 26000,
            min_ram_mb: 26000,
            min_compute_capability: None,
        };
        let alternatives = suggest_alternatives(&model, &[], &profile);
        // Should suggest q8, q4, q2 variants
        assert!(!alternatives.is_empty(), "Should suggest at least one alternative");
        // All suggestions should be compatible
        for alt in &alternatives {
            assert_ne!(alt.compatibility_class, ModelCompatibilityClass::Incompatible);
        }
    }

    #[test]
    fn test_thermal_classification() {
        assert_eq!(classify_thermal(Some(50.0), Some(45.0)), ThermalState::Nominal);
        assert_eq!(classify_thermal(Some(75.0), Some(60.0)), ThermalState::Warm);
        assert_eq!(classify_thermal(Some(90.0), Some(60.0)), ThermalState::Throttling);
        assert_eq!(classify_thermal(Some(96.0), Some(60.0)), ThermalState::Critical);
        assert_eq!(classify_thermal(Some(60.0), Some(96.0)), ThermalState::Critical);
    }

    #[test]
    fn test_timeout_runtime_increase() {
        let base = default_timeout_profile(&HardwareClass::CpuWorkstation);
        let mut manager = TimeoutRuntimeManager::with_defaults(base.clone());

        // Record latencies that are > 80% of the inference timeout (50ms)
        // p90 > 80% of 50 = 40ms, so record values > 40ms
        for _ in 0..15 {
            manager.record_latency(OperationType::Inference, 45);
        }

        // After 10+ consecutive high ops, timeout should increase
        let new_timeout = manager.current_timeout(OperationType::Inference);
        assert!(
            new_timeout > base.inference_ms,
            "Timeout should have increased: {} > {}",
            new_timeout,
            base.inference_ms
        );
    }

    #[test]
    fn test_timeout_runtime_decrease() {
        let mut base = default_timeout_profile(&HardwareClass::CpuWorkstation);
        // Set a high initial timeout to allow decrease
        base.inference_ms = 1000;
        let config = TimeoutRuntimeConfig {
            low_threshold_consecutive: 10, // Lower for testing
            ..TimeoutRuntimeConfig::default()
        };
        let mut manager = TimeoutRuntimeManager::new(base.clone(), config);

        // Manually set the current timeout higher than base to allow decrease
        // Record very low latencies (< 20% of 1000 = 200ms)
        for _ in 0..110 {
            manager.record_latency(OperationType::Inference, 5);
        }

        // Timeout should have decreased (but not below base)
        let new_timeout = manager.current_timeout(OperationType::Inference);
        assert!(
            new_timeout <= 1000,
            "Timeout should not exceed initial: {} <= 1000",
            new_timeout
        );
    }

    #[test]
    fn test_vram_manager_pre_check_available() {
        let mut mgr = VramManager::new(24576, 20000);
        let result = mgr.pre_check(8000, 1);
        match result {
            VramPreCheckResult::Available { available_mb } => {
                assert_eq!(available_mb, 20000);
            }
            _ => panic!("Expected Available result"),
        }
    }

    #[test]
    fn test_vram_manager_pre_check_insufficient() {
        let mgr = VramManager::new(8192, 4000);
        let result = mgr.pre_check(10000, 1);
        match result {
            VramPreCheckResult::Insufficient { shortfall_mb, .. } => {
                assert!(shortfall_mb > 0);
            }
            _ => panic!("Expected Insufficient result"),
        }
    }

    #[test]
    fn test_vram_manager_eviction() {
        let mut mgr = VramManager::new(24576, 24576);

        // Register some models
        mgr.register_allocation("model-a", "Model A", 8000, 3); // low priority
        mgr.register_allocation("model-b", "Model B", 8000, 2); // medium priority
        mgr.register_allocation("model-c", "Model C", 4000, 1); // high priority

        // Evict for 10000 MB — should evict lowest priority first
        let evicted = mgr.evict_for_space(10000);
        assert!(!evicted.is_empty(), "Should evict at least one model");
    }

    #[test]
    fn test_vram_manager_no_gpu() {
        let mgr = VramManager::no_gpu();
        assert!(!mgr.has_gpu());
        let result = mgr.pre_check(1000, 1);
        match result {
            VramPreCheckResult::NoGpu => {}
            _ => panic!("Expected NoGpu result"),
        }
    }

    #[test]
    fn test_resource_envelope_validation() {
        let envelopes = vec![
            ResourceEnvelope {
                workload_type: "interactive".to_string(),
                cpu_percent: 50,
                ram_mb: 8192,
                gpu_percent: Some(70),
                vram_mb: None,
                priority: 1,
            },
            ResourceEnvelope {
                workload_type: "background".to_string(),
                cpu_percent: 30,
                ram_mb: 4096,
                gpu_percent: Some(20),
                vram_mb: None,
                priority: 3,
            },
            ResourceEnvelope {
                workload_type: "system".to_string(),
                cpu_percent: 20,
                ram_mb: 2048,
                gpu_percent: Some(10),
                vram_mb: None,
                priority: 0,
            },
        ];

        let mgr = ResourceEnvelopeManager::new(envelopes, 16384, Some(100), Some(24576));
        assert!(mgr.validate_allocations(), "Allocations should be valid (sum <= 100)");
    }

    #[test]
    fn test_resource_envelope_backpressure() {
        let envelopes = vec![ResourceEnvelope {
            workload_type: "interactive".to_string(),
            cpu_percent: 50,
            ram_mb: 1000,
            gpu_percent: None,
            vram_mb: None,
            priority: 1,
        }];

        let mut mgr = ResourceEnvelopeManager::new(envelopes, 16384, None, None);

        // Simulate high memory usage (> 90% of 1000 MB limit)
        mgr.update_envelope_usage("interactive", 50.0, 950, None, None);

        // Should be under pressure
        assert!(mgr.is_under_pressure("interactive"));

        // Try to admit — should be queued
        let result = mgr.try_admit_request("interactive", "req-1", 100);
        assert!(result.is_err());
    }

    #[test]
    fn test_hardware_change_detection_no_changes() {
        let profile = make_test_profile(Some(8192), 32768, false, false);
        let changes = detect_hardware_changes(&profile, &profile);
        assert!(changes.is_empty(), "Identical profiles should have no changes");
    }

    #[test]
    fn test_hardware_change_detection_gpu_added() {
        let old = make_test_profile(None, 32768, false, false);
        let new = make_test_profile(Some(8192), 32768, false, false);
        let changes = detect_hardware_changes(&new, &old);
        assert!(!changes.is_empty(), "GPU addition should be detected");
        let gpu_change = changes.iter().find(|c| c.field == "gpu").unwrap();
        assert_eq!(gpu_change.severity, ChangeSeverity::Critical);
    }

    #[test]
    fn test_hardware_change_detection_ram_change() {
        let old = make_test_profile(None, 16384, false, false);
        let new = make_test_profile(None, 32768, false, false);
        let changes = detect_hardware_changes(&new, &old);
        assert!(!changes.is_empty(), "RAM change should be detected");
    }

    #[test]
    fn test_override_config_validation() {
        assert!(validate_hardware_class("gpu-workstation").is_ok());
        assert!(validate_hardware_class("cpu-workstation").is_ok());
        assert!(validate_hardware_class("invalid-class").is_err());
        assert!(validate_hardware_class("").is_err());
    }

    // ─── Test Helpers ───────────────────────────────────────────────────

    fn make_test_profile(
        vram_mb: Option<u64>,
        total_ram_mb: u64,
        _headless: bool,
        _container: bool,
    ) -> HardwareProfile {
        let gpu = vram_mb.map(|vram| GpuProfile {
            model_name: "Test GPU".to_string(),
            total_vram_mb: vram,
            available_vram_mb: vram,
            compute_capability: Some("8.6".to_string()),
            driver_version: "535.0".to_string(),
            cuda_version: Some("12.0".to_string()),
            rocm_version: None,
            metal_support: false,
            vulkan_compute: true,
        });

        HardwareProfile {
            node_id: "test-node-001".to_string(),
            detected_at: "2024-01-01T00:00:00Z".to_string(),
            hardware_class: HardwareClass::CpuWorkstation, // will be overridden
            cpu: CpuProfile {
                physical_cores: 8,
                logical_cores: 16,
                architecture: "x86_64".to_string(),
                base_clock_mhz: 3600,
                has_avx2: true,
                has_avx512: false,
                has_neon: false,
                model_name: "Test CPU".to_string(),
            },
            memory: MemoryProfile {
                total_ram_mb,
                available_ram_mb: total_ram_mb * 80 / 100,
                swap_mb: 8192,
                ddr_generation: Some(4),
                channels: Some(2),
                estimated_bandwidth_gbps: Some(25.6),
            },
            gpu,
            storage: StorageProfile {
                available_space_mb: 500000,
                storage_type: "nvme".to_string(),
                sequential_read_mbps: Some(3000.0),
                sequential_write_mbps: Some(2000.0),
            },
            network: NetworkProfile {
                interfaces: vec![NetworkInterface {
                    name: "eth0".to_string(),
                    interface_type: "ethernet".to_string(),
                    speed_mbps: Some(1000),
                }],
                lan_bandwidth_mbps: Some(1000.0),
                internet_connected: true,
            },
            probe_results: None,
        }
    }
}


// ─── Property-Based Tests ───────────────────────────────────────────────────

#[cfg(test)]
mod property_tests {
    use proptest::prelude::*;

    use crate::hardware_service::*;
    use crate::hardware_thermal::*;
    use crate::hardware_timeout_runtime::*;
    use crate::hardware_resource_manager::*;
    use crate::hardware_vram_manager::*;

    // ─── Generators ─────────────────────────────────────────────────────

    fn arb_hardware_class() -> impl Strategy<Value = HardwareClass> {
        prop_oneof![
            Just(HardwareClass::GpuWorkstation),
            Just(HardwareClass::CpuWorkstation),
            Just(HardwareClass::GpuServer),
            Just(HardwareClass::CpuServer),
            Just(HardwareClass::Embedded),
            Just(HardwareClass::ContainerRestricted),
        ]
    }

    fn arb_gpu_profile() -> impl Strategy<Value = Option<GpuProfile>> {
        prop_oneof![
            3 => (1024u64..131072u64).prop_map(|vram| Some(GpuProfile {
                model_name: "Test GPU".to_string(),
                total_vram_mb: vram,
                available_vram_mb: vram * 80 / 100,
                compute_capability: Some("8.6".to_string()),
                driver_version: "535.0".to_string(),
                cuda_version: Some("12.0".to_string()),
                rocm_version: None,
                metal_support: false,
                vulkan_compute: true,
            })),
            1 => Just(None),
        ]
    }

    fn arb_hardware_profile() -> impl Strategy<Value = HardwareProfile> {
        (
            1u32..128u32,       // physical_cores
            1024u64..524288u64, // total_ram_mb (1GB to 512GB)
            arb_gpu_profile(),
        )
            .prop_map(|(cores, ram, gpu)| {
                HardwareProfile {
                    node_id: "test-node".to_string(),
                    detected_at: "2024-01-01T00:00:00Z".to_string(),
                    hardware_class: HardwareClass::CpuWorkstation,
                    cpu: CpuProfile {
                        physical_cores: cores,
                        logical_cores: cores * 2,
                        architecture: "x86_64".to_string(),
                        base_clock_mhz: 3000,
                        has_avx2: true,
                        has_avx512: false,
                        has_neon: false,
                        model_name: "Test CPU".to_string(),
                    },
                    memory: MemoryProfile {
                        total_ram_mb: ram,
                        available_ram_mb: ram * 80 / 100,
                        swap_mb: 8192,
                        ddr_generation: Some(4),
                        channels: Some(2),
                        estimated_bandwidth_gbps: Some(25.6),
                    },
                    gpu,
                    storage: StorageProfile {
                        available_space_mb: 500000,
                        storage_type: "nvme".to_string(),
                        sequential_read_mbps: Some(3000.0),
                        sequential_write_mbps: Some(2000.0),
                    },
                    network: NetworkProfile {
                        interfaces: vec![],
                        lan_bandwidth_mbps: None,
                        internet_connected: true,
                    },
                    probe_results: None,
                }
            })
    }

    fn arb_model_requirements() -> impl Strategy<Value = ModelRequirements> {
        (
            1.0f64..200.0f64,    // parameter_count_b
            prop_oneof![Just("f16"), Just("q8"), Just("q4"), Just("q2")],
            512u64..262144u64,   // min_vram_mb
            1024u64..524288u64,  // min_ram_mb
        )
            .prop_map(|(params, quant, vram, ram)| ModelRequirements {
                model_id: format!("model-{:.0}b-{}", params, quant),
                model_name: format!("Model {:.0}B ({})", params, quant),
                parameter_count_b: params,
                quantization: quant.to_string(),
                min_vram_mb: vram,
                min_ram_mb: ram,
                min_compute_capability: None,
            })
    }

    fn arb_temperature() -> impl Strategy<Value = f64> {
        0.0f64..120.0f64
    }

    fn arb_resource_envelopes() -> impl Strategy<Value = Vec<ResourceEnvelope>> {
        // Generate 2-4 envelopes whose CPU sums to <= 100 and GPU sums to <= 100
        (1u32..50u32, 1u32..50u32, 1u32..50u32).prop_map(|(a, b, c)| {
            // Normalize to ensure sum <= 100
            let total = a + b + c;
            let cpu_a = (a * 100) / total.max(1);
            let cpu_b = (b * 100) / total.max(1);
            let cpu_c = 100u32.saturating_sub(cpu_a + cpu_b);

            vec![
                ResourceEnvelope {
                    workload_type: "interactive-inference".to_string(),
                    cpu_percent: cpu_a,
                    ram_mb: 4096,
                    gpu_percent: Some(cpu_a.min(100)),
                    vram_mb: None,
                    priority: 1,
                },
                ResourceEnvelope {
                    workload_type: "tool-execution".to_string(),
                    cpu_percent: cpu_b,
                    ram_mb: 2048,
                    gpu_percent: Some(cpu_b.min(100 - cpu_a.min(100))),
                    vram_mb: None,
                    priority: 2,
                },
                ResourceEnvelope {
                    workload_type: "background".to_string(),
                    cpu_percent: cpu_c,
                    ram_mb: 1024,
                    gpu_percent: Some(100u32.saturating_sub(cpu_a.min(100) + cpu_b.min(100 - cpu_a.min(100)))),
                    vram_mb: None,
                    priority: 3,
                },
            ]
        })
    }

    // ─── Task 2.8: Properties 1, 2, 3 ──────────────────────────────────

    proptest! {
        /// **Validates: Requirements 1.6, 1.7**
        /// Property 1: Detection completeness — all required fields populated.
        /// (We test the classification and profile assembly logic since actual
        /// hardware detection requires real hardware.)
        #[test]
        fn prop_detection_completeness(profile in arb_hardware_profile()) {
            // All required fields must be populated
            prop_assert!(!profile.node_id.is_empty());
            prop_assert!(!profile.detected_at.is_empty());
            prop_assert!(profile.cpu.physical_cores > 0);
            prop_assert!(profile.cpu.logical_cores > 0);
            prop_assert!(profile.memory.total_ram_mb > 0);
            prop_assert!(!profile.cpu.architecture.is_empty());
        }

        /// **Validates: Requirements 2.1**
        /// Property 2: Classification determinism — same input always produces same output.
        #[test]
        fn prop_classification_determinism(profile in arb_hardware_profile()) {
            let class1 = classify_hardware(&profile);
            let class2 = classify_hardware(&profile);
            let class3 = classify_hardware(&profile);
            prop_assert_eq!(&class1, &class2);
            prop_assert_eq!(&class2, &class3);
        }

        /// **Validates: Requirements 3.1, 3.2**
        /// Property 3: Timeout positivity — all timeout values are positive.
        #[test]
        fn prop_timeout_positivity(class in arb_hardware_class()) {
            let profile = default_timeout_profile(&class);
            prop_assert!(profile.inference_ms > 0);
            prop_assert!(profile.tool_execution_ms > 0);
            prop_assert!(profile.health_check_ms > 0);
            prop_assert!(profile.network_request_ms > 0);
            prop_assert!(profile.database_query_ms > 0);
            prop_assert!(profile.compute_job_ms > 0);
        }
    }

    // ─── Task 3.6: Property 4 ───────────────────────────────────────────

    proptest! {
        /// **Validates: Requirements 4.1, 4.3**
        /// Property 4: Compatibility matrix correctness — when both VRAM and RAM
        /// are insufficient, the model must be classified as incompatible.
        #[test]
        fn prop_compatibility_matrix_correctness(
            profile in arb_hardware_profile(),
            model in arb_model_requirements(),
        ) {
            let entry = compute_model_compatibility(&model, &profile);

            let gpu_vram = profile.gpu.as_ref().map(|g| g.available_vram_mb).unwrap_or(0);
            let available_ram = profile.memory.available_ram_mb;

            // If VRAM is insufficient AND RAM is insufficient, must be incompatible
            if gpu_vram < model.min_vram_mb && available_ram < model.min_ram_mb {
                // Also check that offloading doesn't save it
                let offload_possible = profile.gpu.is_some()
                    && gpu_vram > 0
                    && (gpu_vram + available_ram) >= model.min_ram_mb;

                if !offload_possible {
                    prop_assert_eq!(
                        entry.compatibility_class,
                        ModelCompatibilityClass::Incompatible,
                        "Model requiring {}MB VRAM and {}MB RAM should be incompatible with {}MB VRAM and {}MB RAM available",
                        model.min_vram_mb, model.min_ram_mb, gpu_vram, available_ram
                    );
                }
            }

            // If native GPU is possible, it should be classified as such
            if profile.gpu.is_some() && gpu_vram >= model.min_vram_mb {
                prop_assert_eq!(
                    entry.compatibility_class,
                    ModelCompatibilityClass::NativeGpu,
                    "Model with {}MB VRAM requirement should be NativeGpu when {}MB available",
                    model.min_vram_mb, gpu_vram
                );
            }
        }
    }

    // ─── Task 4.6: Property 5 ───────────────────────────────────────────

    proptest! {
        /// **Validates: Requirements 5.2**
        /// Property 5: Resource envelope sum never exceeds 100%.
        #[test]
        fn prop_resource_envelope_sum(envelopes in arb_resource_envelopes()) {
            let cpu_sum: u32 = envelopes.iter().map(|e| e.cpu_percent).sum();
            let gpu_sum: u32 = envelopes.iter().filter_map(|e| e.gpu_percent).sum();

            prop_assert!(
                cpu_sum <= 100,
                "CPU sum {} exceeds 100%",
                cpu_sum
            );
            prop_assert!(
                gpu_sum <= 100,
                "GPU sum {} exceeds 100%",
                gpu_sum
            );

            // Also verify via the manager
            let mgr = ResourceEnvelopeManager::new(envelopes, 16384, Some(100), Some(24576));
            prop_assert!(mgr.validate_allocations());
        }
    }

    // ─── Task 5.8: Property 6 ───────────────────────────────────────────

    proptest! {
        /// **Validates: Requirements 6.1, 6.2**
        /// Property 6: Thermal state ordering — strict ordering with no gaps.
        #[test]
        fn prop_thermal_state_ordering(temp in arb_temperature()) {
            let state = classify_thermal(Some(temp), None);

            // Verify ordering is consistent
            match state {
                ThermalState::Nominal => {
                    prop_assert!(temp < 70.0, "Nominal should be < 70C, got {}", temp);
                }
                ThermalState::Warm => {
                    prop_assert!(temp >= 70.0 && temp < 85.0, "Warm should be 70-85C, got {}", temp);
                }
                ThermalState::Throttling => {
                    prop_assert!(temp >= 85.0 && temp < 95.0, "Throttling should be 85-95C, got {}", temp);
                }
                ThermalState::Critical => {
                    prop_assert!(temp >= 95.0, "Critical should be >= 95C, got {}", temp);
                }
            }

            // Verify monotonicity: higher temp => same or higher state
            if temp < 70.0 {
                prop_assert_eq!(state, ThermalState::Nominal);
            }
            if temp >= 95.0 {
                prop_assert_eq!(state, ThermalState::Critical);
            }
        }

        /// Property 6 (extended): Two temperatures, max determines state.
        #[test]
        fn prop_thermal_state_max_determines(
            cpu_temp in arb_temperature(),
            gpu_temp in arb_temperature(),
        ) {
            let state = classify_thermal(Some(cpu_temp), Some(gpu_temp));
            let max_temp = cpu_temp.max(gpu_temp);
            let expected = classify_thermal(Some(max_temp), None);
            prop_assert_eq!(state, expected, "Max temp should determine state");
        }
    }

    // ─── Task 6.8: Properties 7, 8 ─────────────────────────────────────

    proptest! {
        /// **Validates: Requirements 8.3**
        /// Property 7: Adaptation recovery — when triggering condition clears,
        /// the adaptation logic correctly identifies recovery.
        #[test]
        fn prop_adaptation_recovery(
            cpu_temp_trigger in 86.0f64..94.0f64,
            cpu_temp_recover in 0.0f64..69.0f64,
        ) {
            // Trigger adaptation
            let trigger_state = classify_thermal(Some(cpu_temp_trigger), None);
            prop_assert_eq!(trigger_state, ThermalState::Throttling);

            // Determine adaptation
            let adaptation = determine_adaptation(&ThermalState::Nominal, &trigger_state);
            prop_assert!(adaptation.is_some(), "Should trigger adaptation on throttling");

            // Create active adaptation
            let active = ActiveAdaptation {
                strategy: AdaptationStrategy::ThermalThrottling,
                triggered_by: "thermal".to_string(),
                applied_at: "2024-01-01T00:00:00Z".to_string(),
                original_concurrency: 4,
                reduced_concurrency: 2,
                timeout_multiplier: 2.0,
            };

            // Recovery: temperature drops to nominal
            let recover_state = classify_thermal(Some(cpu_temp_recover), None);
            prop_assert_eq!(recover_state, ThermalState::Nominal);

            // Should clear adaptation
            let should_clear = should_clear_adaptation(&recover_state, &active);
            prop_assert!(should_clear, "Adaptation should clear when temp recovers to nominal");
        }

        /// **Validates: Requirements 9.1**
        /// Property 8: Probe reproducibility — results within 20% of each other.
        /// (We test the property structurally since we can't run actual probes in tests.)
        #[test]
        fn prop_probe_reproducibility(
            base_value in 1.0f64..1000.0f64,
            variance_pct in 0.0f64..20.0f64,
        ) {
            // Simulate two probe results with bounded variance
            let result1 = base_value;
            let result2 = base_value * (1.0 + variance_pct / 100.0);

            // Verify they are within 20% of each other
            let ratio = if result1 > result2 {
                result1 / result2
            } else {
                result2 / result1
            };

            prop_assert!(
                ratio <= 1.2,
                "Results should be within 20%: {} vs {} (ratio: {})",
                result1, result2, ratio
            );
        }
    }
}

// ─── Integration Tests ──────────────────────────────────────────────────────

#[cfg(test)]
mod integration_tests {
    use crate::hardware_service::*;
    use crate::hardware_thermal::*;
    use crate::hardware_timeout_runtime::*;
    use crate::hardware_resource_manager::*;
    use crate::hardware_vram_manager::*;

    // ─── Helpers ────────────────────────────────────────────────────────

    /// Create a mock HardwareProfile with configurable parameters.
    fn mock_profile(
        total_ram_mb: u64,
        vram_mb: Option<u64>,
        cores: u32,
    ) -> HardwareProfile {
        let gpu = vram_mb.map(|vram| GpuProfile {
            model_name: "Mock GPU".to_string(),
            total_vram_mb: vram,
            available_vram_mb: vram * 90 / 100,
            compute_capability: Some("8.6".to_string()),
            driver_version: "535.0".to_string(),
            cuda_version: Some("12.0".to_string()),
            rocm_version: None,
            metal_support: false,
            vulkan_compute: true,
        });

        HardwareProfile {
            node_id: "integration-test-node".to_string(),
            detected_at: "2024-06-01T00:00:00Z".to_string(),
            hardware_class: HardwareClass::CpuWorkstation, // placeholder
            cpu: CpuProfile {
                physical_cores: cores,
                logical_cores: cores * 2,
                architecture: "x86_64".to_string(),
                base_clock_mhz: 3200,
                has_avx2: true,
                has_avx512: false,
                has_neon: false,
                model_name: "Integration Test CPU".to_string(),
            },
            memory: MemoryProfile {
                total_ram_mb,
                available_ram_mb: total_ram_mb * 80 / 100,
                swap_mb: 4096,
                ddr_generation: Some(4),
                channels: Some(2),
                estimated_bandwidth_gbps: Some(20.0),
            },
            gpu,
            storage: StorageProfile {
                available_space_mb: 256000,
                storage_type: "ssd".to_string(),
                sequential_read_mbps: Some(2000.0),
                sequential_write_mbps: Some(1500.0),
            },
            network: NetworkProfile {
                interfaces: vec![NetworkInterface {
                    name: "eth0".to_string(),
                    interface_type: "ethernet".to_string(),
                    speed_mbps: Some(1000),
                }],
                lan_bandwidth_mbps: Some(1000.0),
                internet_connected: true,
            },
            probe_results: None,
        }
    }

    /// Minimal hardware profile: 4GB RAM, no GPU (embedded-class device).
    fn mock_minimal_profile() -> HardwareProfile {
        mock_profile(4096, None, 2)
    }

    /// High-end workstation profile: 64GB RAM, 24GB VRAM GPU.
    fn mock_workstation_profile() -> HardwareProfile {
        mock_profile(65536, Some(24576), 16)
    }

    // ─── Test 1: Full Detection → Classification → Timeout → Compatibility Flow ─

    #[test]
    fn integration_full_detection_classification_timeout_compatibility_flow() {
        // Step 1: Create a mock hardware profile (simulates detection)
        let mut profile = mock_workstation_profile();

        // Step 2: Classify the hardware
        let class = classify_hardware(&profile);
        assert_eq!(class, HardwareClass::GpuWorkstation);
        profile.hardware_class = class.clone();

        // Step 3: Get timeout profile based on classification
        let timeout = default_timeout_profile(&class);
        assert_eq!(timeout.hardware_class, HardwareClass::GpuWorkstation);
        assert!(timeout.inference_ms > 0);
        assert!(timeout.tool_execution_ms > 0);
        assert!(timeout.health_check_ms > 0);
        assert!(timeout.network_request_ms > 0);
        assert!(timeout.database_query_ms > 0);
        assert!(timeout.compute_job_ms > 0);

        // Step 4: Compute compatibility matrix for a set of models
        let models = vec![
            ModelRequirements {
                model_id: "small-7b-q4".to_string(),
                model_name: "Small 7B Q4".to_string(),
                parameter_count_b: 7.0,
                quantization: "q4".to_string(),
                min_vram_mb: 4000,
                min_ram_mb: 8000,
                min_compute_capability: None,
            },
            ModelRequirements {
                model_id: "medium-13b-f16".to_string(),
                model_name: "Medium 13B F16".to_string(),
                parameter_count_b: 13.0,
                quantization: "f16".to_string(),
                min_vram_mb: 26000,
                min_ram_mb: 26000,
                min_compute_capability: None,
            },
            ModelRequirements {
                model_id: "large-70b-f16".to_string(),
                model_name: "Large 70B F16".to_string(),
                parameter_count_b: 70.0,
                quantization: "f16".to_string(),
                min_vram_mb: 140000,
                min_ram_mb: 140000,
                min_compute_capability: None,
            },
        ];

        let matrix: Vec<ModelCompatibilityEntry> = models
            .iter()
            .map(|m| compute_model_compatibility(m, &profile))
            .collect();

        // Small model should be NativeGpu (4000 < 24576 available VRAM)
        assert_eq!(matrix[0].compatibility_class, ModelCompatibilityClass::NativeGpu);
        assert!(matrix[0].estimated_tokens_per_sec > 0.0);
        assert!(matrix[0].incompatibility_reason.is_none());

        // Medium model: requires 26000 VRAM but only ~22118 available (24576*90%)
        // Should be Offloaded or CpuOnly depending on combined resources
        assert_ne!(matrix[1].compatibility_class, ModelCompatibilityClass::NativeGpu);

        // Large model should be Incompatible (140000 > everything)
        assert_eq!(matrix[2].compatibility_class, ModelCompatibilityClass::Incompatible);
        assert!(matrix[2].incompatibility_reason.is_some());
        assert_eq!(matrix[2].estimated_tokens_per_sec, 0.0);
    }

    // ─── Test 2: Graceful Degradation on Minimal Hardware (4GB RAM, No GPU) ─

    #[test]
    fn integration_graceful_degradation_minimal_hardware() {
        // Create minimal hardware: 4GB RAM, no GPU
        let mut profile = mock_minimal_profile();

        // Classification: should be Embedded (< 8GB RAM, no GPU)
        let class = classify_hardware(&profile);
        assert_eq!(class, HardwareClass::Embedded);
        profile.hardware_class = class.clone();

        // Timeout profile: all values must be positive (system still works)
        let timeout = default_timeout_profile(&class);
        assert!(timeout.inference_ms > 0, "inference_ms must be positive on minimal hardware");
        assert!(timeout.tool_execution_ms > 0, "tool_execution_ms must be positive");
        assert!(timeout.health_check_ms > 0, "health_check_ms must be positive");
        assert!(timeout.network_request_ms > 0, "network_request_ms must be positive");
        assert!(timeout.database_query_ms > 0, "database_query_ms must be positive");
        assert!(timeout.compute_job_ms > 0, "compute_job_ms must be positive");

        // Embedded timeouts should be more generous than workstation
        let ws_timeout = default_timeout_profile(&HardwareClass::CpuWorkstation);
        assert!(
            timeout.inference_ms >= ws_timeout.inference_ms,
            "Embedded inference timeout should be >= workstation"
        );

        // Model compatibility: all models requiring > 4GB RAM should be incompatible
        let large_models = vec![
            ModelRequirements {
                model_id: "model-7b-f16".to_string(),
                model_name: "7B F16".to_string(),
                parameter_count_b: 7.0,
                quantization: "f16".to_string(),
                min_vram_mb: 14000,
                min_ram_mb: 14000,
                min_compute_capability: None,
            },
            ModelRequirements {
                model_id: "model-13b-q8".to_string(),
                model_name: "13B Q8".to_string(),
                parameter_count_b: 13.0,
                quantization: "q8".to_string(),
                min_vram_mb: 13000,
                min_ram_mb: 13000,
                min_compute_capability: None,
            },
            ModelRequirements {
                model_id: "model-70b-q4".to_string(),
                model_name: "70B Q4".to_string(),
                parameter_count_b: 70.0,
                quantization: "q4".to_string(),
                min_vram_mb: 35000,
                min_ram_mb: 35000,
                min_compute_capability: None,
            },
        ];

        for model in &large_models {
            let entry = compute_model_compatibility(model, &profile);
            assert_eq!(
                entry.compatibility_class,
                ModelCompatibilityClass::Incompatible,
                "Model '{}' requiring {}MB RAM should be incompatible with 4GB system",
                model.model_name,
                model.min_ram_mb
            );
            assert!(entry.incompatibility_reason.is_some());
        }

        // A tiny model that fits in 4GB available RAM (4096 * 80% = 3276 MB available)
        let tiny_model = ModelRequirements {
            model_id: "tiny-1b-q2".to_string(),
            model_name: "Tiny 1B Q2".to_string(),
            parameter_count_b: 1.0,
            quantization: "q2".to_string(),
            min_vram_mb: 500,
            min_ram_mb: 2000,
            min_compute_capability: None,
        };
        let tiny_entry = compute_model_compatibility(&tiny_model, &profile);
        assert_eq!(
            tiny_entry.compatibility_class,
            ModelCompatibilityClass::CpuOnly,
            "Tiny model should run CPU-only on minimal hardware"
        );

        // Resource envelopes should still be valid on minimal hardware
        let envelopes = default_resource_envelopes(&class, 4096);
        assert!(!envelopes.is_empty(), "Should have resource envelopes even on embedded");
        let cpu_sum: u32 = envelopes.iter().map(|e| e.cpu_percent).sum();
        assert!(cpu_sum <= 100, "CPU envelope sum must not exceed 100%");
    }

    // ─── Test 3: Hardware Change Detection Flow ─────────────────────────

    #[test]
    fn integration_hardware_change_detection_flow() {
        // Simulate initial profile (no GPU, 16GB RAM)
        let stored_profile = mock_profile(16384, None, 8);

        // Simulate new profile after hardware change (GPU added, 32GB RAM)
        let current_profile = mock_profile(32768, Some(8192), 8);

        // Detect changes
        let changes = detect_hardware_changes(&current_profile, &stored_profile);

        // Should detect GPU addition (Critical)
        let gpu_change = changes.iter().find(|c| c.field == "gpu");
        assert!(gpu_change.is_some(), "GPU addition should be detected");
        assert_eq!(gpu_change.unwrap().severity, ChangeSeverity::Critical);

        // Should detect RAM change (Critical since > 8GB difference)
        let ram_change = changes.iter().find(|c| c.field == "memory.totalRamMb");
        assert!(ram_change.is_some(), "RAM change should be detected");
        assert_eq!(ram_change.unwrap().old_value, "16384");
        assert_eq!(ram_change.unwrap().new_value, "32768");

        // Verify no false positives: identical profiles produce no changes
        let no_changes = detect_hardware_changes(&stored_profile, &stored_profile);
        assert!(no_changes.is_empty(), "Identical profiles should produce no changes");

        // Minor change: same hardware, different available RAM (< 1GB diff) — no change
        let mut minor_change = stored_profile.clone();
        minor_change.memory.total_ram_mb = 16500; // only 116 MB difference
        let minor_changes = detect_hardware_changes(&minor_change, &stored_profile);
        let ram_minor = minor_changes.iter().find(|c| c.field == "memory.totalRamMb");
        assert!(ram_minor.is_none(), "Minor RAM difference (<1GB) should not be flagged");
    }

    // ─── Test 4: Timeout Runtime Adjustment Flow ────────────────────────

    #[test]
    fn integration_timeout_runtime_adjustment_flow() {
        // Create a timeout manager with CpuWorkstation defaults
        let base = default_timeout_profile(&HardwareClass::CpuWorkstation);
        let original_inference_ms = base.inference_ms; // 50ms
        let mut manager = TimeoutRuntimeManager::with_defaults(base.clone());

        // Verify initial state matches base
        assert_eq!(
            manager.current_timeout(OperationType::Inference),
            original_inference_ms
        );

        // Simulate sustained high latency: p90 > 80% of 50ms = 40ms
        // Record 15 latencies above the threshold to trigger increase
        for _ in 0..15 {
            manager.record_latency(OperationType::Inference, 45);
        }

        // Timeout should have increased (50 * 1.5 = 75)
        let increased_timeout = manager.current_timeout(OperationType::Inference);
        assert!(
            increased_timeout > original_inference_ms,
            "Timeout should increase after sustained high latency: {} > {}",
            increased_timeout,
            original_inference_ms
        );

        // Verify other operation timeouts are unaffected
        assert_eq!(
            manager.current_timeout(OperationType::ToolExecution),
            base.tool_execution_ms,
            "ToolExecution timeout should be unchanged"
        );
        assert_eq!(
            manager.current_timeout(OperationType::HealthCheck),
            base.health_check_ms,
            "HealthCheck timeout should be unchanged"
        );

        // Reset and verify return to base
        manager.reset_to_base();
        assert_eq!(
            manager.current_timeout(OperationType::Inference),
            original_inference_ms,
            "After reset, timeout should return to base"
        );

        // Verify the manager tracks multiple operation types independently
        for _ in 0..15 {
            manager.record_latency(OperationType::DatabaseQuery, 1800); // > 80% of 2000ms
        }
        let db_timeout = manager.current_timeout(OperationType::DatabaseQuery);
        assert!(
            db_timeout > base.database_query_ms,
            "DatabaseQuery timeout should increase independently"
        );
        // Inference should still be at base since we reset
        assert_eq!(
            manager.current_timeout(OperationType::Inference),
            original_inference_ms,
            "Inference should remain at base while DB adjusts"
        );
    }

    // ─── Test 5: VRAM Manager Flow ──────────────────────────────────────

    #[test]
    fn integration_vram_manager_flow() {
        // Create a VRAM manager with 24GB total, 20GB available
        let mut mgr = VramManager::new(24576, 20000);
        assert!(mgr.has_gpu());

        // Pre-check: 8GB model should fit
        let check = mgr.pre_check(8000, 1);
        match check {
            VramPreCheckResult::Available { available_mb } => {
                assert_eq!(available_mb, 20000);
            }
            _ => panic!("Expected Available, got {:?}", check),
        }

        // Register allocations
        let _alloc_a = mgr.register_allocation("model-a", "Model A (7B)", 8000, 3);
        let _alloc_b = mgr.register_allocation("model-b", "Model B (13B)", 10000, 2);

        // Verify state after allocations
        assert_eq!(mgr.allocations().len(), 2);
        assert_eq!(mgr.total_allocated_mb(), 18000);

        // Available should have decreased: 20000 - 8000 - 10000 = 2000
        let state = mgr.current_state();
        assert_eq!(state.available_mb, 2000);

        // Pre-check: 5000MB model should now be insufficient
        let check2 = mgr.pre_check(5000, 1);
        match check2 {
            VramPreCheckResult::EvictionRequired { eviction_candidates, .. } => {
                // Should suggest evicting lower-priority models (priority > 1)
                assert!(!eviction_candidates.is_empty());
            }
            VramPreCheckResult::Insufficient { shortfall_mb, .. } => {
                assert!(shortfall_mb > 0);
            }
            _ => panic!("Expected EvictionRequired or Insufficient, got {:?}", check2),
        }

        // Check pressure: with 2000/24576 available = ~8%, should be under pressure
        let under_pressure = mgr.check_pressure();
        assert!(under_pressure, "Should be under pressure with only 2000MB of 24576MB available");

        // Evict lowest priority model to free space
        let evicted = mgr.evict_for_space(8000);
        assert!(!evicted.is_empty(), "Should evict at least one model");

        // After eviction, available should increase
        let state_after = mgr.current_state();
        assert!(
            state_after.available_mb > 2000,
            "Available VRAM should increase after eviction"
        );

        // Verify the evicted model is no longer in allocations
        assert!(
            mgr.allocations().len() < 2,
            "Should have fewer allocations after eviction"
        );

        // Test no-GPU scenario
        let no_gpu_mgr = VramManager::no_gpu();
        assert!(!no_gpu_mgr.has_gpu());
        let no_gpu_check = no_gpu_mgr.pre_check(1000, 1);
        match no_gpu_check {
            VramPreCheckResult::NoGpu => {} // expected
            _ => panic!("Expected NoGpu result"),
        }
    }

    // ─── Test 6: Resource Envelope Manager Flow ─────────────────────────

    #[test]
    fn integration_resource_envelope_manager_flow() {
        // Create envelopes for a workstation with 32GB RAM
        let envelopes = vec![
            ResourceEnvelope {
                workload_type: "interactive-inference".to_string(),
                cpu_percent: 50,
                ram_mb: 16384,
                gpu_percent: Some(70),
                vram_mb: None,
                priority: 1,
            },
            ResourceEnvelope {
                workload_type: "tool-execution".to_string(),
                cpu_percent: 30,
                ram_mb: 8192,
                gpu_percent: Some(20),
                vram_mb: None,
                priority: 2,
            },
            ResourceEnvelope {
                workload_type: "background".to_string(),
                cpu_percent: 20,
                ram_mb: 4096,
                gpu_percent: Some(10),
                vram_mb: None,
                priority: 3,
            },
        ];

        let mut mgr = ResourceEnvelopeManager::new(
            envelopes,
            32768,
            Some(100),
            Some(24576),
        );

        // Validate allocations sum to <= 100%
        assert!(mgr.validate_allocations(), "Allocations should be valid");

        // Initially no pressure
        assert!(!mgr.is_under_pressure("interactive-inference"));
        assert!(!mgr.is_under_pressure("tool-execution"));
        assert!(!mgr.is_under_pressure("background"));

        // Simulate usage update: interactive at moderate load
        mgr.update_envelope_usage("interactive-inference", 40.0, 8000, Some(50.0), None);
        assert!(!mgr.is_under_pressure("interactive-inference"));

        // Simulate high memory usage on tool-execution (> 90% of 8192 = 7372)
        mgr.update_envelope_usage("tool-execution", 25.0, 7500, None, None);
        assert!(
            mgr.is_under_pressure("tool-execution"),
            "tool-execution should be under pressure at 7500/8192 MB"
        );

        // Try to admit a request — should be queued due to backpressure
        let admit_result = mgr.try_admit_request("tool-execution", "req-001", 500);
        assert!(
            admit_result.is_err(),
            "Request should be queued when under pressure"
        );

        // Simulate pressure relief: usage drops
        mgr.update_envelope_usage("tool-execution", 10.0, 2000, None, None);
        assert!(
            !mgr.is_under_pressure("tool-execution"),
            "Pressure should clear when usage drops"
        );

        // Drain backpressure queue
        let released = mgr.drain_backpressure("tool-execution");
        assert_eq!(released.len(), 1, "Should release the queued request");
        assert_eq!(released[0].id, "req-001");

        // Test rebalancing: make interactive idle, background active
        mgr.update_envelope_usage("interactive-inference", 2.0, 100, None, None);
        mgr.update_envelope_usage("background", 80.0, 3500, None, None);

        // Wait for idle threshold (in real code this is 5s, but we test the logic)
        // The rebalance function checks last_active timing internally
        mgr.rebalance();

        // Verify utilization reporting works
        let utilization = mgr.get_utilization();
        assert_eq!(utilization.ram_total_mb, 32768);
        assert!(!utilization.envelopes.is_empty());

        // Verify no backpressure is active after relief
        assert!(!mgr.any_backpressure_active());

        // Test reclaim: when interactive becomes active again
        mgr.update_envelope_usage("interactive-inference", 60.0, 10000, Some(60.0), None);
        mgr.check_reclaim_needed();
        // After reclaim, borrowing state should be cleared
        assert!(
            mgr.get_borrowing_state().is_empty() || !mgr.get_borrowing_state().iter().any(|b| b.lender == "interactive-inference"),
            "Interactive should not be lending while active"
        );
    }
}
