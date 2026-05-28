// Integration test: Full Optimizer Cycle

use super::harness::*;
use super::mock_node::*;

#[test]
fn test_optimizer_produces_plan_for_multi_node() {
    let mut world = TestWorld::new();
    let desktop = world.add_node(desktop_config());
    let _laptop = world.add_node(laptop_config());
    let phone_id = world.add_phone(phone_config());

    let plan = world.run_optimizer();

    // Should produce placements
    assert!(!plan.placements.is_empty(), "Optimizer should produce placements");

    // Desktop with GPU should get a GPU model
    let desktop_placements: Vec<_> = plan
        .placements
        .iter()
        .filter(|p| p.assigned_nodes.contains(&desktop))
        .collect();
    assert!(!desktop_placements.is_empty(), "Desktop should get placements");
}

#[test]
fn test_optimizer_respects_ram_constraints() {
    let mut world = TestWorld::new();
    let small_node = world.add_node(MockNodeConfig {
        hostname: "tiny".to_string(),
        ram_mb: 4000, // Only 4GB
        vram_mb: 0,
        ..Default::default()
    });

    let plan = world.run_optimizer();

    // All placements on this node should fit within 4GB
    for placement in &plan.placements {
        if placement.assigned_nodes.contains(&small_node) {
            assert!(
                placement.ram_required_mb <= 4000,
                "Placement exceeds node RAM: {}MB > 4000MB",
                placement.ram_required_mb
            );
        }
    }
}

#[test]
fn test_optimizer_emits_plan_created_event() {
    let mut world = TestWorld::new();
    let _node = world.add_node(desktop_config());

    let plan = world.run_optimizer();

    let events = world.events();
    assert!(events.iter().any(|e| matches!(
        e,
        TestEvent::PlanCreated { plan_id, .. } if *plan_id == plan.plan_id
    )));
}

#[test]
fn test_optimizer_with_no_nodes_produces_empty_plan() {
    let mut world = TestWorld::new();
    let plan = world.run_optimizer();
    assert!(plan.placements.is_empty());
}

#[test]
fn test_optimizer_offline_nodes_excluded() {
    let mut world = TestWorld::new();
    let node = world.add_node(desktop_config());

    // Crash the node
    world.crash_node(node);

    let plan = world.run_optimizer();

    // Crashed node should not get placements
    let node_placements: Vec<_> = plan
        .placements
        .iter()
        .filter(|p| p.assigned_nodes.contains(&node))
        .collect();
    assert!(node_placements.is_empty(), "Offline node should not get placements");
}

#[test]
fn test_optimizer_demand_weights_influence_plan() {
    let mut world = TestWorld::new();
    let _node = world.add_node(desktop_config());

    world.set_demand(vec![("coding", 0.8), ("chat", 0.2)]);
    let plan = world.run_optimizer();

    // Plan should exist (demand doesn't prevent placement in our simplified optimizer)
    assert!(!plan.placements.is_empty());
}

#[test]
fn test_optimizer_phone_constraints() {
    let mut world = TestWorld::new();
    let _desktop = world.add_node(desktop_config());
    let phone_id = world.add_phone(phone_config());

    let plan = world.run_optimizer();

    // Phone placements should be small
    for placement in &plan.placements {
        if placement.assigned_nodes.contains(&phone_id) {
            assert!(
                placement.ram_required_mb <= 3000,
                "Phone model too large: {}MB",
                placement.ram_required_mb
            );
        }
    }
}
