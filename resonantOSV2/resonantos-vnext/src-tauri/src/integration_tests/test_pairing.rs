// Integration test: Pairing → Assignment → Split Inference flow

use super::harness::*;
use super::mock_node::*;

#[test]
fn test_phone_pairs_and_appears_in_registry() {
    let mut world = TestWorld::new();
    let _desktop = world.add_node(desktop_config());
    let phone_id = world.add_phone(phone_config());

    // Phone should be registered and paired
    let phone = world.get_phone(&phone_id).unwrap();
    assert!(phone.paired);
    assert!(phone.online);
    assert_eq!(phone.config.hostname, "iphone");
}

#[test]
fn test_phone_gets_assignment_after_optimizer() {
    let mut world = TestWorld::new();
    let _desktop = world.add_node(desktop_config());
    let phone_id = world.add_phone(phone_config());

    let plan = world.run_optimizer();

    // Phone should get a placement (battery > 20%)
    let phone_placements: Vec<_> = plan
        .placements
        .iter()
        .filter(|p| p.assigned_nodes.contains(&phone_id))
        .collect();
    assert!(!phone_placements.is_empty(), "Phone should get at least one assignment");

    // Phone placement should be small (< 3GB RAM)
    for p in &phone_placements {
        assert!(p.ram_required_mb <= 3000, "Phone model too large: {}MB", p.ram_required_mb);
    }
}

#[test]
fn test_low_battery_phone_excluded() {
    let mut world = TestWorld::new();
    let _desktop = world.add_node(desktop_config());

    let mut low_battery = phone_config();
    low_battery.battery_percent = 15; // Below 20% threshold
    let phone_id = world.add_phone(low_battery);

    let plan = world.run_optimizer();

    // Phone should NOT get a placement (battery too low)
    let phone_placements: Vec<_> = plan
        .placements
        .iter()
        .filter(|p| p.assigned_nodes.contains(&phone_id))
        .collect();
    assert!(phone_placements.is_empty(), "Low battery phone should not get assignment");
}

#[test]
fn test_pairing_emits_event() {
    let mut world = TestWorld::new();
    let phone_id = world.add_phone(phone_config());

    let events = world.events();
    assert!(events.iter().any(|e| matches!(e, TestEvent::PhonePaired { node_id } if *node_id == phone_id)));
}

#[test]
fn test_split_inference_message_sent() {
    let mut world = TestWorld::new();
    let desktop_id = world.add_node(desktop_config());
    let phone_id = world.add_phone(phone_config());

    // Simulate split inference activation forwarding
    let result = world.send_message(desktop_id, phone_id, b"activate_layer_3".to_vec());
    assert!(result.is_ok());

    let msgs = world.captured_messages();
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].source, desktop_id);
    assert_eq!(msgs[0].target, phone_id);
}

#[test]
fn test_phone_result_collected_at_desktop() {
    let mut world = TestWorld::new();
    let desktop_id = world.add_node(desktop_config());
    let phone_id = world.add_phone(phone_config());

    // Phone sends result back to desktop
    let result = world.send_message(phone_id, desktop_id, b"layer_result_tensor".to_vec());
    assert!(result.is_ok());

    let msgs = world.captured_messages();
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].source, phone_id);
    assert_eq!(msgs[0].target, desktop_id);
}
