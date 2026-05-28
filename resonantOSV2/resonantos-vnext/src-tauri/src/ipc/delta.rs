// IPC Delta — delta computation for node status updates
//
// Computes the difference between previous and current node snapshots
// to minimize payload size. Only changed or new nodes are included in
// delta emissions. Full syncs send all nodes periodically.

use super::payloads::NodeSnapshot;

/// Compute the delta between previous and current node snapshots.
///
/// Returns only nodes that are new or have changed since the previous snapshot.
/// If `previous` is `None` (first emission), returns all current nodes.
pub fn compute_delta(
    previous: &Option<Vec<NodeSnapshot>>,
    current: &[NodeSnapshot],
) -> Vec<NodeSnapshot> {
    match previous {
        None => current.to_vec(),
        Some(prev) => current
            .iter()
            .filter(|node| {
                match prev.iter().find(|p| p.node_id == node.node_id) {
                    None => true, // New node
                    Some(prev_node) => has_changed(prev_node, node),
                }
            })
            .cloned()
            .collect(),
    }
}

/// Check if a node's state has changed between two snapshots.
///
/// Compares online status, CPU, RAM usage, and loaded models.
pub fn has_changed(prev: &NodeSnapshot, curr: &NodeSnapshot) -> bool {
    prev.online != curr.online
        || prev.cpu_percent != curr.cpu_percent
        || prev.ram_used_mb != curr.ram_used_mb
        || prev.vram_used_mb != curr.vram_used_mb
        || prev.models_loaded != curr.models_loaded
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_node(id: &str, cpu: f64, ram: u64, online: bool) -> NodeSnapshot {
        NodeSnapshot {
            node_id: id.into(),
            hostname: format!("host-{}", id),
            device_type: "desktop".into(),
            online,
            cpu_percent: cpu,
            ram_used_mb: ram,
            ram_total_mb: 16384,
            vram_used_mb: 0,
            vram_total_mb: 8192,
            models_loaded: vec![],
        }
    }

    #[test]
    fn test_compute_delta_first_emission_returns_all() {
        let current = vec![make_node("a", 50.0, 8000, true), make_node("b", 30.0, 4000, true)];
        let delta = compute_delta(&None, &current);
        assert_eq!(delta.len(), 2);
    }

    #[test]
    fn test_compute_delta_no_changes_returns_empty() {
        let prev = vec![make_node("a", 50.0, 8000, true)];
        let current = vec![make_node("a", 50.0, 8000, true)];
        let delta = compute_delta(&Some(prev), &current);
        assert_eq!(delta.len(), 0);
    }

    #[test]
    fn test_compute_delta_cpu_change_detected() {
        let prev = vec![make_node("a", 50.0, 8000, true)];
        let current = vec![make_node("a", 75.0, 8000, true)];
        let delta = compute_delta(&Some(prev), &current);
        assert_eq!(delta.len(), 1);
        assert_eq!(delta[0].node_id, "a");
    }

    #[test]
    fn test_compute_delta_new_node_included() {
        let prev = vec![make_node("a", 50.0, 8000, true)];
        let current = vec![
            make_node("a", 50.0, 8000, true),
            make_node("b", 30.0, 4000, true),
        ];
        let delta = compute_delta(&Some(prev), &current);
        assert_eq!(delta.len(), 1);
        assert_eq!(delta[0].node_id, "b");
    }

    #[test]
    fn test_compute_delta_online_status_change() {
        let prev = vec![make_node("a", 50.0, 8000, true)];
        let current = vec![make_node("a", 50.0, 8000, false)];
        let delta = compute_delta(&Some(prev), &current);
        assert_eq!(delta.len(), 1);
        assert!(!delta[0].online);
    }

    #[test]
    fn test_compute_delta_models_loaded_change() {
        let prev = vec![NodeSnapshot {
            models_loaded: vec!["llama".into()],
            ..make_node("a", 50.0, 8000, true)
        }];
        let current = vec![NodeSnapshot {
            models_loaded: vec!["llama".into(), "mistral".into()],
            ..make_node("a", 50.0, 8000, true)
        }];
        let delta = compute_delta(&Some(prev), &current);
        assert_eq!(delta.len(), 1);
    }

    #[test]
    fn test_has_changed_identical_nodes() {
        let a = make_node("a", 50.0, 8000, true);
        let b = make_node("a", 50.0, 8000, true);
        assert!(!has_changed(&a, &b));
    }

    #[test]
    fn test_has_changed_ram_change() {
        let a = make_node("a", 50.0, 8000, true);
        let b = make_node("a", 50.0, 9000, true);
        assert!(has_changed(&a, &b));
    }

    #[test]
    fn test_has_changed_vram_change() {
        let a = make_node("a", 50.0, 8000, true);
        let mut b = make_node("a", 50.0, 8000, true);
        b.vram_used_mb = 1024;
        assert!(has_changed(&a, &b));
    }
}

#[cfg(test)]
mod property_tests {
    use super::*;
    use proptest::prelude::*;

    fn arb_node_snapshot() -> impl Strategy<Value = NodeSnapshot> {
        (
            "[a-f0-9]{8}",
            "host-[a-z]{3,6}",
            prop::sample::select(vec!["desktop", "laptop", "phone"]),
            any::<bool>(),
            0.0..100.0f64,
            0u64..32768,
            0u64..32768,
            0u64..24576,
            0u64..24576,
            prop::collection::vec("[a-z]{3,8}", 0..5),
        )
            .prop_map(|(id, host, dev, online, cpu, ram_used, ram_total, vram_used, vram_total, models)| {
                NodeSnapshot {
                    node_id: id,
                    hostname: host,
                    device_type: dev.to_string(),
                    online,
                    cpu_percent: cpu,
                    ram_used_mb: ram_used,
                    ram_total_mb: ram_total.max(ram_used),
                    vram_used_mb: vram_used,
                    vram_total_mb: vram_total.max(vram_used),
                    models_loaded: models,
                }
            })
    }

    // Property 2: Delta Correctness — delta contains exactly changed nodes
    proptest! {
        #[test]
        fn prop_delta_contains_only_changed_nodes(
            prev_nodes in prop::collection::vec(arb_node_snapshot(), 1..10),
            changes in prop::collection::vec(0usize..10, 0..5)
        ) {
            let mut current = prev_nodes.clone();

            // Apply some changes
            for &idx in &changes {
                if idx < current.len() {
                    current[idx].cpu_percent += 1.0; // Force a change
                }
            }

            let delta = compute_delta(&Some(prev_nodes.clone()), &current);

            // Every node in delta should actually be changed or new
            for node in &delta {
                if let Some(prev_node) = prev_nodes.iter().find(|p| p.node_id == node.node_id) {
                    prop_assert!(
                        has_changed(prev_node, node),
                        "Delta contains unchanged node: {}",
                        node.node_id
                    );
                }
                // else: new node, which is correct to include
            }
        }

        // Property 5: Payload Size Bound — payload < 50KB for ≤20 nodes
        #[test]
        fn prop_payload_size_bounded(
            nodes in prop::collection::vec(arb_node_snapshot(), 1..20)
        ) {
            let payload = super::super::payloads::NodeStatusPayload {
                nodes,
                is_full_sync: true,
                timestamp_ms: 1000,
            };

            let serialized = serde_json::to_string(&payload).unwrap();
            prop_assert!(
                serialized.len() < 50_000,
                "Payload too large: {} bytes",
                serialized.len()
            );
        }
    }
}
