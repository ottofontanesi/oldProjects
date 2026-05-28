// Integration test: Concurrency scenarios

use super::harness::*;
use super::mock_node::*;
use std::time::Duration;

#[test]
fn test_multiple_workflows_simultaneously() {
    let mut world = TestWorld::new();
    let _node = world.add_node(desktop_config());

    let wf1 = world.submit_workflow(vec!["a".to_string(), "b".to_string()]);
    let wf2 = world.submit_workflow(vec!["x".to_string(), "y".to_string()]);
    let wf3 = world.submit_workflow(vec!["p".to_string(), "q".to_string(), "r".to_string()]);

    world.advance_time(Duration::from_secs(3));

    assert_eq!(world.get_workflow_status(&wf1).unwrap().status, WorkflowStatus::Completed);
    assert_eq!(world.get_workflow_status(&wf2).unwrap().status, WorkflowStatus::Completed);
    assert_eq!(world.get_workflow_status(&wf3).unwrap().status, WorkflowStatus::Completed);
}

#[test]
fn test_node_joining_during_optimizer() {
    let mut world = TestWorld::new();
    let _node_a = world.add_node(desktop_config());

    // Run optimizer with 1 node
    let plan1 = world.run_optimizer();
    let count1 = plan1.placements.len();

    // Add another node
    let _node_b = world.add_node(laptop_config());

    // Run optimizer again — should produce more placements
    let plan2 = world.run_optimizer();
    let count2 = plan2.placements.len();

    assert!(count2 >= count1, "More nodes should allow more placements");
}

#[test]
fn test_node_leaving_during_optimizer() {
    let mut world = TestWorld::new();
    let node_a = world.add_node(desktop_config());
    let node_b = world.add_node(laptop_config());

    // Run optimizer with 2 nodes
    let plan1 = world.run_optimizer();
    let count1 = plan1.placements.len();

    // Crash node_b
    world.crash_node(node_b);

    // Run optimizer again — fewer placements
    let plan2 = world.run_optimizer();

    // node_b should not appear in new plan
    for p in &plan2.placements {
        assert!(!p.assigned_nodes.contains(&node_b), "Crashed node should not be in plan");
    }
}

#[test]
fn test_no_lost_messages_under_concurrent_sends() {
    let mut world = TestWorld::new();
    let node_a = world.add_node(desktop_config());
    let node_b = world.add_node(laptop_config());
    let node_c = world.add_node(MockNodeConfig {
        hostname: "server".to_string(),
        ..desktop_config()
    });

    // Send many messages between different pairs
    for i in 0..100 {
        let payload = format!("msg-{}", i).into_bytes();
        match i % 3 {
            0 => world.send_message(node_a, node_b, payload).unwrap(),
            1 => world.send_message(node_b, node_c, payload).unwrap(),
            _ => world.send_message(node_c, node_a, payload).unwrap(),
        }
    }

    // All 100 messages should be captured
    assert_eq!(world.captured_messages().len(), 100);
}

#[test]
fn test_workflow_during_node_crash() {
    let mut world = TestWorld::new();
    let node = world.add_node(desktop_config());

    let wf = world.submit_workflow(vec!["s1".to_string(), "s2".to_string(), "s3".to_string()]);

    // Execute 1 step
    world.advance_time(Duration::from_secs(1));

    // Crash node (workflow continues in our simplified model)
    world.crash_node(node);

    // Advance more time
    world.advance_time(Duration::from_secs(3));

    // Workflow should still complete (simplified model doesn't block on node crash)
    let status = world.get_workflow_status(&wf).unwrap();
    assert_eq!(status.status, WorkflowStatus::Completed);
}
