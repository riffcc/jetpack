use jetpack::handle::response::*;
use jetpack::tasks::request::{TaskRequest, TaskRequestType};
use jetpack::tasks::response::{TaskStatus, TaskResponse};
use jetpack::tasks::fields::Field;
use jetpack::playbooks::traversal::RunState;
use jetpack::playbooks::context::PlaybookContext;
use jetpack::inventory::hosts::Host;
use jetpack::cli::parser::CliParser;
use std::sync::{Arc, RwLock};

fn create_test_response() -> Response {
    let cli_args: Vec<String> = vec!["jetpack".to_string()];
    let parser = Arc::new(CliParser::new(&cli_args, false));
    let context = Arc::new(RwLock::new(PlaybookContext::new(
        Arc::clone(&parser), 
        None, 
        None,
        0
    )));
    let run_state = Arc::new(RunState::new(context));
    let host = Arc::new(RwLock::new(Host::new("testhost", "test_connection")));
    
    Response::new(run_state, host)
}

#[test]
fn test_response_new() {
    let response = create_test_response();
    assert!(response.get_context().read().is_ok());
}

#[test]
fn test_response_is_matched() {
    let response = create_test_response();
    let request = Arc::new(TaskRequest::new(
        TaskRequestType::Query,
        "test_module".to_string(),
        "test_task".to_string(),
    ));
    
    let result = response.is_matched(&request);
    assert!(result.status == TaskStatus::IsMatched);
    assert_eq!(result.module, "test_module");
    assert_eq!(result.task_name, "test_task");
}

#[test]
fn test_response_needs_creation() {
    let response = create_test_response();
    let request = Arc::new(TaskRequest::new(
        TaskRequestType::Query,
        "test_module".to_string(),
        "test_task".to_string(),
    ));
    
    let result = response.needs_creation(&request);
    assert!(result.status == TaskStatus::NeedsCreation);
}

#[test]
fn test_response_needs_modification() {
    let response = create_test_response();
    let request = Arc::new(TaskRequest::new(
        TaskRequestType::Query,
        "test_module".to_string(),
        "test_task".to_string(),
    ));
    let changes = vec![Field::Content, Field::Mode];
    
    let result = response.needs_modification(&request, &changes);
    assert!(result.status == TaskStatus::NeedsModification);
    assert_eq!(result.changes.len(), 2);
}

#[test]
fn test_response_needs_removal() {
    let response = create_test_response();
    let request = Arc::new(TaskRequest::new(
        TaskRequestType::Query,
        "test_module".to_string(),
        "test_task".to_string(),
    ));
    
    let result = response.needs_removal(&request);
    assert!(result.status == TaskStatus::NeedsRemoval);
}

#[test]
fn test_response_is_created() {
    let response = create_test_response();
    let request = Arc::new(TaskRequest::new(
        TaskRequestType::Create,
        "test_module".to_string(),
        "test_task".to_string(),
    ));
    
    let result = response.is_created(&request);
    assert!(result.status == TaskStatus::IsCreated);
}

#[test]
fn test_response_is_modified() {
    let response = create_test_response();
    let request = Arc::new(TaskRequest::new(
        TaskRequestType::Modify,
        "test_module".to_string(),
        "test_task".to_string(),
    ));
    let changes = vec![Field::Owner];
    
    let result = response.is_modified(&request, changes);
    assert!(result.status == TaskStatus::IsModified);
    assert_eq!(result.changes.len(), 1);
}

#[test]
fn test_response_is_removed() {
    let response = create_test_response();
    let request = Arc::new(TaskRequest::new(
        TaskRequestType::Remove,
        "test_module".to_string(),
        "test_task".to_string(),
    ));
    
    let result = response.is_removed(&request);
    assert!(result.status == TaskStatus::IsRemoved);
}

#[test]
fn test_response_is_failed() {
    let response = create_test_response();
    let request = Arc::new(TaskRequest::new(
        TaskRequestType::Execute,
        "test_module".to_string(),
        "test_task".to_string(),
    ));
    
    let result = response.is_failed(&request, "Test error message");
    assert!(result.status == TaskStatus::IsFailed);
    assert_eq!(result.message, Some("Test error message".to_string()));
}

#[test]
fn test_response_not_supported() {
    let response = create_test_response();
    let request = Arc::new(TaskRequest::new(
        TaskRequestType::Validate,
        "test_module".to_string(),
        "test_task".to_string(),
    ));
    
    let result = response.not_supported(&request);
    assert!(result.status == TaskStatus::NotSupported);
}