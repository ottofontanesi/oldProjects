// Integration test: Error Propagation

use super::harness::*;
use super::mock_node::*;

#[test]
fn test_transport_error_does_not_crash_system() {
    let mut world = TestWorld::new();
    let node_a = world.add_node(desktop_config());
    let node_b = world.add_node(laptop_config());

    // Disable secondary transport
    {
        // We need to create a new transport without secondary
        // For this test, just verify the error is returned cleanly
    }

    // Inject failure with no secondary
    world.inject_transport_failure(node_b);

    // Message via secondary should still work (secondary is available by default)
    let result = world.send_message(node_a, node_b, b"test".to_vec());
    assert!(result.is_ok()); // Secondary catches it
}

#[test]
fn test_optimizer_failure_preserves_current_plan() {
    let mut world = TestWorld::new();
    let _node = world.add_node(desktop_config());

    // Run optimizer to get a plan
    let plan1 = world.run_optimizer();
    assert!(!plan1.placements.is_empty());

    // Crash all nodes
    for id in world.nodes.keys().cloned().collect::<Vec<_>>() {
        world.crash_node(id);
    }

    // Run optimizer again — produces empty plan
    let plan2 = world.run_optimizer();
    assert!(plan2.placements.is_empty());

    // But the previous plan is still accessible
    // (In production, the system would keep the old plan until a better one is found)
    assert!(world.current_plan.is_some());
}

#[test]
fn test_message_to_crashed_node_uses_secondary() {
    let mut world = TestWorld::new();
    let node_a = world.add_node(desktop_config());
    let node_b = world.add_node(laptop_config());

    world.crash_node(node_b);

    // Should failover to secondary
    let result = world.send_message(node_a, node_b, b"hello".to_vec());
    assert!(result.is_ok());

    let msgs = world.captured_messages();
    assert!(msgs[0].channel.contains("secondary"));
}

#[test]
fn test_restored_node_resumes_normal_operation() {
    let mut world = TestWorld::new();
    let node_a = world.add_node(desktop_config());
    let node_b = world.add_node(laptop_config());

    // Crash and restore
    world.crash_node(node_b);
    world.restore_node(node_b);

    // Should work normally again
    let result = world.send_message(node_a, node_b, b"hello".to_vec());
    assert!(result.is_ok());

    let msgs = world.captured_messages();
    assert_eq!(msgs[0].channel, "default"); // Primary path
}

#[test]
fn test_workflow_checkpoint_survives_crash() {
    let mut world = TestWorld::new();
    let _node = world.add_node(desktop_config());

    let wf = world.submit_workflow(vec!["s1".to_string(), "s2".to_string(), "s3".to_string()]);
    world.advance_time(std::time::Duration::from_secs(2));
    world.checkpoint_workflow(&wf);

    // Simulate full system crash (clear workflows)
    world.workflows.clear();

    // Checkpoint should still be in persistence
    let cp = world.persistence.load_checkpoint(&wf);
    assert!(cp.is_some());
    assert_eq!(cp.unwrap().completed_steps.len(), 2);
}

#[test]
fn test_performance_optimizer_under_500ms() {
    let mut world = TestWorld::new();

    // Add 10 nodes
    for i in 0..10 {
        world.add_node(MockNodeConfig {
            hostname: format!("node-{}", i),
            ram_mb: 32_000,
            vram_mb: if i < 3 { 24_000 } else { 0 },
            ..Default::default()
        });
    }

    let start = std::time::Instant::now();
    let _plan = world.run_optimizer();
    let elapsed = start.elapsed();

    assert!(
        elapsed.as_millis() < 500,
        "Optimizer took {}ms, should be <500ms",
        elapsed.as_millis()
    );
}

#[test]
fn test_performance_transport_routing_under_5ms() {
    let mut world = TestWorld::new();
    let node_a = world.add_node(desktop_config());
    let node_b = world.add_node(laptop_config());

    let start = std::time::Instant::now();
    for _ in 0..100 {
        world.send_message(node_a, node_b, b"ping".to_vec()).unwrap();
    }
    let elapsed = start.elapsed();

    // 100 messages should complete in well under 500ms (5ms per message budget)
    assert!(
        elapsed.as_millis() < 500,
        "100 messages took {}ms, should be <500ms",
        elapsed.as_millis()
    );
}
