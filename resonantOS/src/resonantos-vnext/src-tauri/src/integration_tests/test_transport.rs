// Integration test: Transport Failover

use super::harness::*;
use super::mock_node::*;

#[test]
fn test_message_delivery_succeeds() {
    let mut world = TestWorld::new();
    let node_a = world.add_node(desktop_config());
    let node_b = world.add_node(laptop_config());

    let result = world.send_message(node_a, node_b, b"hello".to_vec());
    assert!(result.is_ok());
    assert_eq!(world.captured_messages().len(), 1);
}

#[test]
fn test_failover_to_secondary_on_primary_failure() {
    let mut world = TestWorld::new();
    let node_a = world.add_node(desktop_config());
    let node_b = world.add_node(laptop_config());

    // Inject failure on node_b's primary path
    world.inject_transport_failure(node_b);

    // Send should succeed via secondary path
    let result = world.send_message(node_a, node_b, b"hello".to_vec());
    assert!(result.is_ok());

    let msgs = world.captured_messages();
    assert_eq!(msgs.len(), 1);
    assert!(msgs[0].channel.contains("secondary"), "Should use secondary path");
}

#[test]
fn test_recovery_restores_primary_path() {
    let mut world = TestWorld::new();
    let node_a = world.add_node(desktop_config());
    let node_b = world.add_node(laptop_config());

    // Fail and recover
    world.inject_transport_failure(node_b);
    world.recover_transport(node_b);

    // Should use primary path again
    let result = world.send_message(node_a, node_b, b"hello".to_vec());
    assert!(result.is_ok());

    let msgs = world.captured_messages();
    assert_eq!(msgs[0].channel, "default"); // Primary path
}

#[test]
fn test_continued_delivery_during_failure() {
    let mut world = TestWorld::new();
    let node_a = world.add_node(desktop_config());
    let node_b = world.add_node(laptop_config());

    // Send before failure
    world.send_message(node_a, node_b, b"msg1".to_vec()).unwrap();

    // Inject failure
    world.inject_transport_failure(node_b);

    // Send during failure (via secondary)
    world.send_message(node_a, node_b, b"msg2".to_vec()).unwrap();

    // Recover
    world.recover_transport(node_b);

    // Send after recovery
    world.send_message(node_a, node_b, b"msg3".to_vec()).unwrap();

    let msgs = world.captured_messages();
    assert_eq!(msgs.len(), 3, "All 3 messages should be delivered");
}

#[test]
fn test_failure_emits_event() {
    let mut world = TestWorld::new();
    let node_b = world.add_node(laptop_config());

    world.inject_transport_failure(node_b);

    let events = world.events();
    assert!(events.iter().any(|e| matches!(
        e,
        TestEvent::TransportFailure { node_id } if *node_id == node_b
    )));
}

#[test]
fn test_recovery_emits_event() {
    let mut world = TestWorld::new();
    let node_b = world.add_node(laptop_config());

    world.inject_transport_failure(node_b);
    world.recover_transport(node_b);

    let events = world.events();
    assert!(events.iter().any(|e| matches!(
        e,
        TestEvent::TransportRecovered { node_id } if *node_id == node_b
    )));
}

#[test]
fn test_multiple_nodes_independent_failure() {
    let mut world = TestWorld::new();
    let node_a = world.add_node(desktop_config());
    let node_b = world.add_node(laptop_config());
    let node_c = world.add_node(MockNodeConfig {
        hostname: "server".to_string(),
        ..desktop_config()
    });

    // Fail only node_b
    world.inject_transport_failure(node_b);

    // node_a → node_c should still work on primary
    let result = world.send_message(node_a, node_c, b"hello".to_vec());
    assert!(result.is_ok());

    let msgs = world.captured_messages();
    assert_eq!(msgs[0].channel, "default"); // Primary, not secondary
}
