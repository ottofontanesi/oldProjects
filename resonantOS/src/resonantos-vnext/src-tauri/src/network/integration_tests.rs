// Intent citation: .kiro/specs/local-network-optimizer/tasks.md Task 16
// End-to-End Integration Tests for the Local Network Optimizer

#[cfg(test)]
mod tests {
    use crate::network::catalog::*;
    use crate::network::demand::*;
    use crate::network::executor::*;
    use crate::network::incentive::*;
    use crate::network::phone::*;
    use crate::network::preferences::*;
    use crate::network::registry::*;
    use crate::network::solver::*;
    use std::collections::HashMap;

    // ─── Helper Functions ────────────────────────────────────────────────────

    fn make_desktop_node(ram_mb: u64, vram_mb: u64) -> NodeState {
        let node_id = uuid::Uuid::new_v4();
        NodeState {
            capabilities: NodeCapabilities {
                node_id,
                hostname: "desktop".to_string(),
                device_type: DeviceType::Desktop,
                cpu: CpuProfile { cores: 16, architecture: "x86_64".to_string(), clock_mhz: 5000, isa_extensions: vec!["avx2".to_string()] },
                ram: RamProfile { total_mb: ram_mb, available_mb: ram_mb, ddr_generation: 4 },
                gpu: if vram_mb > 0 { Some(GpuProfile { model: "RTX 4090".to_string(), vram_mb, vram_available_mb: vram_mb, compute_capability: 8.9, backend: GpuBackend::Cuda }) } else { None },
                storage: StorageProfile { storage_type: StorageType::Nvme, available_mb: 500_000, read_speed_mbps: 7000 },
                network_interfaces: vec![],
                phone_info: None,
                available_tools: vec![],
            },
            utilization: NodeUtilization { node_id, ..Default::default() },
            loaded_models: vec![],
            stability_score: 0.95,
            last_heartbeat_ms: 0,
            is_online: true,
            latency_to_peers: HashMap::new(),
            thermal_state: ThermalState::default(),
        }
    }

    fn make_laptop_node(ram_mb: u64) -> NodeState {
        let node_id = uuid::Uuid::new_v4();
        NodeState {
            capabilities: NodeCapabilities {
                node_id,
                hostname: "laptop".to_string(),
                device_type: DeviceType::Laptop,
                cpu: CpuProfile { cores: 8, architecture: "x86_64".to_string(), clock_mhz: 3500, isa_extensions: vec![] },
                ram: RamProfile { total_mb: ram_mb, available_mb: ram_mb, ddr_generation: 4 },
                gpu: None,
                storage: StorageProfile { storage_type: StorageType::Ssd, available_mb: 200_000, read_speed_mbps: 3000 },
                network_interfaces: vec![],
                phone_info: None,
                available_tools: vec![],
            },
            utilization: NodeUtilization { node_id, ..Default::default() },
            loaded_models: vec![],
            stability_score: 0.90,
            last_heartbeat_ms: 0,
            is_online: true,
            latency_to_peers: HashMap::new(),
            thermal_state: ThermalState::default(),
        }
    }

    fn make_phone_node() -> NodeState {
        let node_id = uuid::Uuid::new_v4();
        NodeState {
            capabilities: NodeCapabilities {
                node_id,
                hostname: "phone".to_string(),
                device_type: DeviceType::Phone,
                cpu: CpuProfile { cores: 6, architecture: "aarch64".to_string(), clock_mhz: 3400, isa_extensions: vec![] },
                ram: RamProfile { total_mb: 8192, available_mb: 4096, ddr_generation: 5 },
                gpu: None,
                storage: StorageProfile { storage_type: StorageType::Nvme, available_mb: 50_000, read_speed_mbps: 2000 },
                network_interfaces: vec![],
                phone_info: Some(PhoneInfo {
                    os: PhoneOs::Ios,
                    npu: Some(NpuType::AppleNeuralEngine { generation: 5 }),
                    battery_percent: 75,
                    is_charging: false,
                    connection_type: ConnectionType::Wifi,
                }),
                available_tools: vec![],
            },
            utilization: NodeUtilization { node_id, ..Default::default() },
            loaded_models: vec![],
            stability_score: 0.60,
            last_heartbeat_ms: 0,
            is_online: true,
            latency_to_peers: HashMap::new(),
            thermal_state: ThermalState::default(),
        }
    }

    fn default_catalog() -> Vec<ModelEntry> {
        ModelCatalog::with_defaults().all_models().to_vec()
    }

    fn make_demand(shares: &[(&str, f64)]) -> WorkloadDemand {
        WorkloadDemand {
            computed_at_ms: 1000,
            time_window_hours: 24,
            model_shares: shares.iter().map(|(id, s)| (id.to_string(), *s)).collect(),
            task_shares: HashMap::from([(TaskType::Chat, 0.5), (TaskType::Code, 0.3), (TaskType::Creative, 0.2)]),
            total_requests: 200,
            forecast: DemandForecast {
                next_period_model_shares: HashMap::new(),
                next_period_task_shares: HashMap::new(),
                confidence: 0.8,
                prefetch_signals: vec![],
            },
        }
    }

    // ─── Test 16.1: 2-node network ──────────────────────────────────────────

    #[test]
    fn test_two_node_model_placed_on_gpu_node() {
        let desktop = make_desktop_node(32_000, 24_000);
        let laptop = make_laptop_node(16_000);

        let catalog = default_catalog();
        let demand = make_demand(&[("qwen2.5:7b-q4_K_M", 0.7), ("llama3.2:3b-q4_K_M", 0.3)]);

        let inputs = SolverInputs {
            node_states: vec![desktop.clone(), laptop.clone()],
            model_catalog: catalog.clone(),
            workload_demand: demand,
            preferences: SolverPreferences::new(),
            max_network_params_b: 14.0,
            agent_catalog: vec![],
            agent_demand: Default::default(),
        };

        let config = SolverConfig::default();
        let plan = solve(&inputs, &config, 1000);

        // Plan should have models placed
        assert!(!plan.placements.is_empty());

        // GPU-requiring models should prefer the desktop (has GPU)
        let qwen_placement = plan.placements.iter().find(|p| p.model_id == "qwen2.5:7b-q4_K_M");
        if let Some(placement) = qwen_placement {
            // Should be on desktop (has GPU = faster)
            assert!(placement.assigned_nodes.contains(&desktop.capabilities.node_id));
        }

        // Utility should be positive
        assert!(plan.utility_scores.total > 0.0);
    }

    // ─── Test 16.2: 3-node with phone ───────────────────────────────────────

    #[test]
    fn test_three_node_phone_gets_small_model_only() {
        let desktop = make_desktop_node(32_000, 24_000);
        let laptop = make_laptop_node(16_000);
        let phone = make_phone_node();

        let catalog = default_catalog();
        let demand = make_demand(&[
            ("qwen2.5:14b-q4_K_M", 0.3),
            ("qwen2.5:7b-q4_K_M", 0.4),
            ("llama3.2:3b-q4_K_M", 0.3),
        ]);

        let inputs = SolverInputs {
            node_states: vec![desktop.clone(), laptop.clone(), phone.clone()],
            model_catalog: catalog,
            workload_demand: demand,
            preferences: SolverPreferences::new(),
            max_network_params_b: 14.0,
            agent_catalog: vec![],
            agent_demand: Default::default(),
        };

        let config = SolverConfig::default();
        let plan = solve(&inputs, &config, 1000);

        // Phone should NEVER have a model > 3B
        for placement in &plan.placements {
            if placement.assigned_nodes.contains(&phone.capabilities.node_id) {
                let model = inputs.model_catalog.iter().find(|m| m.model_id == placement.model_id).unwrap();
                assert!(
                    model.parameter_count_b <= 3.0,
                    "Phone got model {}B (max 3B allowed)",
                    model.parameter_count_b
                );
            }
        }
    }

    // ─── Test 16.3: Node departure ──────────────────────────────────────────

    #[test]
    fn test_node_departure_produces_valid_plan() {
        let desktop = make_desktop_node(32_000, 24_000);
        let laptop = make_laptop_node(16_000);

        let catalog = default_catalog();
        let demand = make_demand(&[("qwen2.5:7b-q4_K_M", 1.0)]);

        // Initial plan with both nodes
        let inputs = SolverInputs {
            node_states: vec![desktop.clone(), laptop.clone()],
            model_catalog: catalog.clone(),
            workload_demand: demand.clone(),
            preferences: SolverPreferences::new(),
            max_network_params_b: 14.0,
            agent_catalog: vec![],
            agent_demand: Default::default(),
        };
        let config = SolverConfig::default();
        let plan_before = solve(&inputs, &config, 1000);

        // Desktop departs — re-solve with only laptop
        let inputs_after = SolverInputs {
            node_states: vec![laptop.clone()], // Only laptop remains
            model_catalog: catalog,
            workload_demand: demand,
            preferences: SolverPreferences::new(),
            max_network_params_b: 14.0,
            agent_catalog: vec![],
            agent_demand: Default::default(),
        };
        let plan_after = solve(&inputs_after, &config, 2000);

        // Plan should still be valid (may have fewer/smaller models)
        // All placements should reference only the laptop
        for placement in &plan_after.placements {
            assert!(placement.assigned_nodes.contains(&laptop.capabilities.node_id));
        }
    }

    // ─── Test 16.4: Cold start ──────────────────────────────────────────────

    #[test]
    fn test_cold_start_produces_valid_plan() {
        let desktop = make_desktop_node(32_000, 24_000);
        let catalog = default_catalog();

        // Empty demand (cold start)
        let demand = WorkloadDemand {
            computed_at_ms: 1000,
            time_window_hours: 24,
            model_shares: HashMap::new(), // No history
            task_shares: HashMap::new(),
            total_requests: 0,
            forecast: DemandForecast {
                next_period_model_shares: HashMap::new(),
                next_period_task_shares: HashMap::new(),
                confidence: 0.0,
                prefetch_signals: vec![],
            },
        };

        let inputs = SolverInputs {
            node_states: vec![desktop],
            model_catalog: catalog,
            workload_demand: demand,
            preferences: SolverPreferences::new(),
            max_network_params_b: 14.0,
            agent_catalog: vec![],
            agent_demand: Default::default(),
        };

        let config = SolverConfig::default();
        let plan = solve(&inputs, &config, 1000);

        // Should still produce a plan (exploration budget kicks in)
        // Utility scores should be valid
        assert!(plan.utility_scores.quality >= 0.0 && plan.utility_scores.quality <= 1.0);
        assert!(plan.utility_scores.speed >= 0.0 && plan.utility_scores.speed <= 1.0);
    }

    // ─── Test 16.5: Preference veto ─────────────────────────────────────────

    #[test]
    fn test_veto_excludes_model() {
        let desktop = make_desktop_node(32_000, 24_000);
        let catalog = default_catalog();
        let demand = make_demand(&[("qwen2.5:7b-q4_K_M", 0.8), ("gemma3:7b-q4_K_M", 0.2)]);

        let mut prefs = SolverPreferences::new();
        prefs.model_vetoes = vec!["qwen2.5:7b-q4_K_M".to_string()];

        let inputs = SolverInputs {
            node_states: vec![desktop],
            model_catalog: catalog,
            workload_demand: demand,
            preferences: prefs,
            max_network_params_b: 14.0,
            agent_catalog: vec![],
            agent_demand: Default::default(),
        };

        let config = SolverConfig::default();
        let plan = solve(&inputs, &config, 1000);

        // Vetoed model should NEVER appear in plan
        assert!(
            !plan.placements.iter().any(|p| p.model_id == "qwen2.5:7b-q4_K_M"),
            "Vetoed model appeared in plan!"
        );
    }

    // ─── Test 16.6: Determinism ─────────────────────────────────────────────

    #[test]
    fn test_determinism_same_inputs_same_plan() {
        let desktop = make_desktop_node(32_000, 24_000);
        let catalog = default_catalog();
        let demand = make_demand(&[("qwen2.5:7b-q4_K_M", 0.6), ("llama3.2:3b-q4_K_M", 0.4)]);

        let inputs = SolverInputs {
            node_states: vec![desktop],
            model_catalog: catalog,
            workload_demand: demand,
            preferences: SolverPreferences::new(),
            max_network_params_b: 14.0,
            agent_catalog: vec![],
            agent_demand: Default::default(),
        };

        let config = SolverConfig::default();
        let plan1 = solve(&inputs, &config, 1000);
        let plan2 = solve(&inputs, &config, 1000);

        // Same models selected
        let models1: Vec<&str> = plan1.placements.iter().map(|p| p.model_id.as_str()).collect();
        let models2: Vec<&str> = plan2.placements.iter().map(|p| p.model_id.as_str()).collect();
        assert_eq!(models1, models2, "Solver is not deterministic!");

        // Same utility scores
        assert_eq!(plan1.utility_scores.total, plan2.utility_scores.total);
    }
}
