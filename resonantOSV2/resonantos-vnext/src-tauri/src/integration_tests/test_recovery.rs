// Integration test: Workflow Crash Recovery

use super::harness::*;
use super::mock_node::*;
use std::time::Duration;

#[test]
fn test_checkpoint_saves_completed_steps() {
    let mut world = TestWorld::new();
    let _node = world.add_node(desktop_config());

    let steps = vec!["s1".to_string(), "s2".to_string(), "s3".to_string(), "s4".to_string()];
    let workflow_id = world.submit_workflow(steps);

    // Execute 2 steps
    world.advance_time(Duration::from_secs(2));

    // Checkpoint
    world.checkpoint_workflow(&workflow_id);

    // Verify checkpoint saved
    let cp = world.persistence.load_checkpoint(&workflow_id).unwrap();
    assert_eq!(cp.completed_steps.len(), 2);
}

#[test]
fn test_resume_from_checkpoint_skips_completed() {
    let mut world = TestWorld::new();
    let _node = world.add_node(desktop_config());

    let steps = vec!["s1".to_string(), "s2".to_string(), "s3".to_string(), "s4".to_string()];
    let workflow_id = world.submit_workflow(steps);

    // Execute 2 steps and checkpoint
    world.advance_time(Duration::from_secs(2));
    world.checkpoint_workflow(&workflow_id);

    // Simulate crash: remove workflow from active state
    world.workflows.remove(&workflow_id);

    // Resume from checkpoint
    let resumed = world.resume_workflow(&workflow_id, 4);
    assert!(resumed, "Should resume from checkpoint");

    let status = world.get_workflow_status(&workflow_id).unwrap();
    assert_eq!(status.status, WorkflowStatus::Running);
    assert_eq!(status.completed_steps.len(), 2); // Starts from step 3
}

#[test]
fn test_resumed_workflow_completes() {
    let mut world = TestWorld::new();
    let _node = world.add_node(desktop_config());

    let steps = vec!["s1".to_string(), "s2".to_string(), "s3".to_string(), "s4".to_string()];
    let workflow_id = world.submit_workflow(steps);

    // Execute 2 steps and checkpoint
    world.advance_time(Duration::from_secs(2));
    world.checkpoint_workflow(&workflow_id);

    // Crash and resume
    world.workflows.remove(&workflow_id);
    world.resume_workflow(&workflow_id, 4);

    // Advance time to complete remaining steps
    world.advance_time(Duration::from_secs(3));

    let status = world.get_workflow_status(&workflow_id).unwrap();
    assert_eq!(status.status, WorkflowStatus::Completed);
    assert_eq!(status.completed_steps.len(), 4);
}

#[test]
fn test_no_checkpoint_resume_fails() {
    let mut world = TestWorld::new();
    let resumed = world.resume_workflow("nonexistent-workflow", 4);
    assert!(!resumed, "Should fail to resume without checkpoint");
}

#[test]
fn test_checkpoint_preserves_timing() {
    let mut world = TestWorld::new();
    let _node = world.add_node(desktop_config());

    let workflow_id = world.submit_workflow(vec!["s1".to_string(), "s2".to_string()]);
    world.advance_time(Duration::from_secs(5));
    world.checkpoint_workflow(&workflow_id);

    let cp = world.persistence.load_checkpoint(&workflow_id).unwrap();
    assert_eq!(cp.created_at_ms, 5000);
}
