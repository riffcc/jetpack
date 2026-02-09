use jetpack::handle::response::*;
use jetpack::tasks::request::{TaskRequest, SudoDetails};
use jetpack::tasks::response::TaskStatus;
use jetpack::tasks::fields::Field;
use jetpack::playbooks::traversal::RunState;
use jetpack::playbooks::context::PlaybookContext;
use jetpack::inventory::hosts::Host;
use jetpack::inventory::inventory::Inventory;
use jetpack::cli::parser::CliParser;
use jetpack::playbooks::visitor::{PlaybookVisitor, CheckMode};
use jetpack::connection::no::NoFactory;
use std::sync::{Arc, RwLock};
use std::collections::HashSet;

fn create_test_sudo_details() -> SudoDetails {
    SudoDetails {
        user: None,
        template: "test".to_string()
    }
}

fn create_test_response() -> Response {
    let parser = CliParser::new();
    let inventory = Arc::new(RwLock::new(Inventory::new()));
    let context = Arc::new(RwLock::new(PlaybookContext::new(&parser)));
    
    let run_state = Arc::new(RunState {
        inventory: Arc::clone(&inventory),
        playbook_paths: Arc::new(RwLock::new(Vec::new())),
        role_paths: Arc::new(RwLock::new(Vec::new())),
        module_paths: Arc::new(RwLock::new(Vec::new())),
        limit_hosts: Vec::new(),
        limit_groups: Vec::new(),
        batch_size: None,
        context: context,
        visitor: Arc::new(RwLock::new(PlaybookVisitor::new(CheckMode::No))),
        connection_factory: Arc::new(RwLock::new(NoFactory::new())),
        tags: None,
        allow_localhost_delegation: false,
        is_pull_mode: false,
        play_groups: None,
        processed_role_tasks: Arc::new(RwLock::new(HashSet::new())),
        processed_role_handlers: Arc::new(RwLock::new(HashSet::new())),
        role_processing_stack: Arc::new(RwLock::new(Vec::new())),
        output_handler: None,
    });
    
    let hostname = "testhost".to_string();
    let host = Arc::new(RwLock::new(Host::new(&hostname)));
    
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
    let request = TaskRequest::validate();
    
    let result = response.is_matched(&request);
    assert!(result.status == TaskStatus::IsMatched);
}

#[test]
fn test_response_needs_creation() {
    let response = create_test_response();
    let sudo_details = create_test_sudo_details();
    let request = TaskRequest::query(&sudo_details);
    
    let result = response.needs_creation(&request);
    assert!(result.status == TaskStatus::NeedsCreation);
}

#[test]
fn test_response_needs_modification() {
    let response = create_test_response();
    let sudo_details = create_test_sudo_details();
    let request = TaskRequest::query(&sudo_details);
    let changes = vec![Field::Content, Field::Mode];
    
    let result = response.needs_modification(&request, &changes);
    assert!(result.status == TaskStatus::NeedsModification);
    assert_eq!(result.changes.len(), 2);
}

#[test]
fn test_response_needs_removal() {
    let response = create_test_response();
    let sudo_details = create_test_sudo_details();
    let request = TaskRequest::query(&sudo_details);
    
    let result = response.needs_removal(&request);
    assert!(result.status == TaskStatus::NeedsRemoval);
}

#[test]
fn test_response_is_created() {
    let response = create_test_response();
    let sudo_details = create_test_sudo_details();
    let request = TaskRequest::create(&sudo_details);
    
    let result = response.is_created(&request);
    assert!(result.status == TaskStatus::IsCreated);
}

#[test]
fn test_response_is_modified() {
    let response = create_test_response();
    let sudo_details = create_test_sudo_details();
    let changes = vec![Field::Owner];
    let request = TaskRequest::modify(&sudo_details, changes.clone());
    
    let result = response.is_modified(&request, changes);
    assert!(result.status == TaskStatus::IsModified);
    assert_eq!(result.changes.len(), 1);
}

#[test]
fn test_response_is_removed() {
    let response = create_test_response();
    let sudo_details = create_test_sudo_details();
    let request = TaskRequest::remove(&sudo_details);
    
    let result = response.is_removed(&request);
    assert!(result.status == TaskStatus::IsRemoved);
}

#[test]
fn test_response_is_failed() {
    let response = create_test_response();
    let request = TaskRequest::validate();
    
    let result = response.is_failed(&request, &"Test error message".to_string());
    assert!(result.status == TaskStatus::Failed);
    assert_eq!(result.msg, Some("Test error message".to_string()));
}

