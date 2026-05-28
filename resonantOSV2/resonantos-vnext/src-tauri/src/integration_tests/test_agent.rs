// Integration test: Agent Workflow Execution

use super::harness::*;
use super::mock_node::*;
use std::time::Duration;

#[test]
fn test_workflow_completes_all_steps() {
    let mut world = TestWorld::new();
    let _node_a = world.add_node(MockNodeConfig {
        hostname: "node-a".to_string(),
        tools: vec!["browser".to_string(), "filesystem".to_string()],
        ..desktop_config()
    });
    let _node_b = world.add_node(MockNodeConfig {
        hostname: "node-b".to_string(),
        tools: vec!["code_exec".to_string(), "filesystem".to_string()],
        ..laptop_config()
    });

    // Submit 3-step workflow
    let steps = vec!["search".to_string(), "code".to_string(), "synthesize".to_string()];
    let workflow_id = world.submit_workflow(steps);

    // Advance time to let steps complete
    world.advance_time(Duration::from_secs(5));

    let status = world.get_workflow_status(&workflow_id).unwrap();
    assert_eq!(status.status, WorkflowStatus::Completed);
    assert_eq!(status.completed_steps.len(), 3);
}

#[test]
fn test_workflow_starts_in_running_state() {
    let mut world = TestWorld::new();
    let _node = world.add_node(desktop_config());

    let steps = vec!["step-1".to_string(), "step-2".to_string()];
    let workflow_id = world.submit_workflow(steps);

    let status = world.get_workflow_status(&workflow_id).unwrap();
    assert_eq!(status.status, WorkflowStatus::Running);
    assert_eq!(status.completed_steps.len(), 0);
}

#[test]
fn test_workflow_emits_start_event() {
    let mut world = TestWorld::new();
    let _node = world.add_node(desktop_config());

    let workflow_id = world.submit_workflow(vec!["step-1".to_string()]);

    let events = world.events();
    assert!(events.iter().any(|e| matches!(
        e,
        TestEvent::WorkflowStarted { workflow_id: wid } if *wid == workflow_id
    )));
}

#[test]
fn test_multiple_workflows_independent() {
    let mut world = TestWorld::new();
    let _node = world.add_node(desktop_config());

    let wf1 = world.submit_workflow(vec!["a".to_string(), "b".to_string()]);
    let wf2 = world.submit_workflow(vec!["x".to_string(), "y".to_string(), "z".to_string()]);

    world.advance_time(Duration::from_secs(2));

    let s1 = world.get_workflow_status(&wf1).unwrap();
    let s2 = world.get_workflow_status(&wf2).unwrap();

    assert_eq!(s1.status, WorkflowStatus::Completed);
    assert_eq!(s2.completed_steps.len(), 2); // 2 seconds = 2 steps
}

#[test]
fn test_workflow_with_many_steps() {
    let mut world = TestWorld::new();
    let _node = world.add_node(desktop_config());

    let steps: Vec<String> = (0..10).map(|i| format!("step-{}", i)).collect();
    let workflow_id = world.submit_workflow(steps);

    world.advance_time(Duration::from_secs(10));

    let status = world.get_workflow_status(&workflow_id).unwrap();
    assert_eq!(status.status, WorkflowStatus::Completed);
    assert_eq!(status.completed_steps.len(), 10);
}
