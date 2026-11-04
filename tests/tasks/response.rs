// Tests for TaskResponse
use jetpack::connection::command::CommandResult;
use jetpack::tasks::fields::Field;
use jetpack::tasks::response::{TaskResponse, TaskStatus};
use std::sync::Arc;

#[test]
fn test_task_response_creation() {
    let response = TaskResponse {
        status: TaskStatus::IsCreated,
        deferred: false,
        changes: Vec::new(),
        msg: Some("Test message".to_string()),
        command_result: Arc::new(None),
        with: Arc::new(None),
        and: Arc::new(None),
    };

    assert_eq!(response.status, TaskStatus::IsCreated);
    assert_eq!(response.msg, Some("Test message".to_string()));
    assert_eq!(response.changes.len(), 0);
}

#[test]
fn test_task_response_with_changes() {
    let changes = vec![Field::Content];
    let response = TaskResponse {
        status: TaskStatus::IsModified,
        deferred: false,
        changes,
        msg: None,
        command_result: Arc::new(None),
        with: Arc::new(None),
        and: Arc::new(None),
    };

    assert_eq!(response.status, TaskStatus::IsModified);
    assert_eq!(response.changes.len(), 1);
    assert_eq!(response.changes[0], Field::Content);
}

#[test]
fn test_task_response_failed() {
    let response = TaskResponse {
        status: TaskStatus::Failed,
        deferred: false,
        changes: Vec::new(),
        msg: Some("Error occurred".to_string()),
        command_result: Arc::new(None),
        with: Arc::new(None),
        and: Arc::new(None),
    };

    assert_eq!(response.status, TaskStatus::Failed);
    assert_eq!(response.msg, Some("Error occurred".to_string()));
}

#[test]
fn test_task_status_variants() {
    let statuses = vec![
        TaskStatus::IsCreated,
        TaskStatus::IsRemoved,
        TaskStatus::IsModified,
        TaskStatus::IsExecuted,
        TaskStatus::IsPassive,
        TaskStatus::IsMatched,
        TaskStatus::IsSkipped,
        TaskStatus::NeedsCreation,
        TaskStatus::NeedsRemoval,
        TaskStatus::NeedsModification,
        TaskStatus::NeedsExecution,
        TaskStatus::NeedsPassive,
        TaskStatus::Failed,
    ];

    // Just verify we can create all status variants
    for status in statuses {
        let response = TaskResponse {
            status,
            deferred: false,
            changes: Vec::new(),
            msg: None,
            command_result: Arc::new(None),
            with: Arc::new(None),
            and: Arc::new(None),
        };
        // Basic check that we can create the response
        assert!(response.changes.is_empty());
    }
}

#[test]
fn test_task_response_with_command_result() {
    let command_result = CommandResult {
        cmd: "echo success".to_string(),
        rc: 0,
        out: "success".to_string(),
    };

    let response = TaskResponse {
        status: TaskStatus::IsExecuted,
        deferred: false,
        changes: Vec::new(),
        msg: None,
        command_result: Arc::new(Some(command_result)),
        with: Arc::new(None),
        and: Arc::new(None),
    };

    assert_eq!(response.status, TaskStatus::IsExecuted);
    assert!(response.command_result.is_some());
    if let Some(ref cmd_result) = *response.command_result {
        assert_eq!(cmd_result.rc, 0);
        assert_eq!(cmd_result.out, "success");
    }
}
