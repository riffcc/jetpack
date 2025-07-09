use jetpack::connection::command::*;
use jetpack::tasks::response::{TaskResponse, TaskStatus};
use jetpack::tasks::fields::Field;
use std::sync::Arc;

#[test]
fn test_command_result_creation() {
    let cmd_result = CommandResult {
        cmd: "ls -la".to_string(),
        out: "file1\nfile2\n".to_string(),
        rc: 0,
    };
    
    assert_eq!(cmd_result.cmd, "ls -la");
    assert_eq!(cmd_result.out, "file1\nfile2\n");
    assert_eq!(cmd_result.rc, 0);
}

#[test]
fn test_command_result_clone() {
    let cmd_result = CommandResult {
        cmd: "echo test".to_string(),
        out: "test\n".to_string(),
        rc: 0,
    };
    
    let cloned = cmd_result.clone();
    assert_eq!(cloned.cmd, cmd_result.cmd);
    assert_eq!(cloned.out, cmd_result.out);
    assert_eq!(cloned.rc, cmd_result.rc);
}

#[test]
fn test_forward_enum() {
    assert_eq!(Forward::Yes, Forward::Yes);
    assert_eq!(Forward::No, Forward::No);
    assert_ne!(Forward::Yes, Forward::No);
}

#[test]
fn test_cmd_info() {
    let response = TaskResponse {
        status: TaskStatus::IsExecuted,
        changes: vec![],
        msg: None,
        command_result: Arc::new(Some(CommandResult {
            cmd: "test command".to_string(),
            out: "output text".to_string(),
            rc: 0,
        })),
        with: Arc::new(None),
        and: Arc::new(None),
    };
    
    let arc_response = Arc::new(response);
    let (rc, out) = cmd_info(&arc_response);
    
    assert_eq!(rc, 0);
    assert_eq!(out, "output text");
}

#[test]
fn test_cmd_info_with_error() {
    let response = TaskResponse {
        status: TaskStatus::Failed,
        changes: vec![],
        msg: Some("Command failed".to_string()),
        command_result: Arc::new(Some(CommandResult {
            cmd: "failing command".to_string(),
            out: "error message".to_string(),
            rc: 1,
        })),
        with: Arc::new(None),
        and: Arc::new(None),
    };
    
    let arc_response = Arc::new(response);
    let (rc, out) = cmd_info(&arc_response);
    
    assert_eq!(rc, 1);
    assert_eq!(out, "error message");
}

#[test]
#[should_panic(expected = "called cmd_info on a response that is not a command result")]
fn test_cmd_info_without_command_result() {
    let response = TaskResponse {
        status: TaskStatus::IsPassive,
        changes: vec![],
        msg: None,
        command_result: Arc::new(None),
        with: Arc::new(None),
        and: Arc::new(None),
    };
    
    let arc_response = Arc::new(response);
    cmd_info(&arc_response);
}