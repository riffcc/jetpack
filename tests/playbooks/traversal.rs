use jetpack::playbooks::traversal::*;
use jetpack::inventory::inventory::Inventory;
use jetpack::inventory::hosts::Host;
use jetpack::cli::parser::CliParser;
use jetpack::playbooks::context::PlaybookContext;
use jetpack::playbooks::visitor::{PlaybookVisitor, CheckMode};
use jetpack::connection::no::NoFactory;
use std::sync::{Arc, RwLock};
use std::path::PathBuf;

#[test]
fn test_run_state_creation() {
    let parser = CliParser::new();
    let inventory = Arc::new(RwLock::new(Inventory::new()));
    let context = Arc::new(RwLock::new(PlaybookContext::new(&parser)));
    
    let run_state = RunState {
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
        processed_role_tasks: Arc::new(RwLock::new(std::collections::HashSet::new())),
        processed_role_handlers: Arc::new(RwLock::new(std::collections::HashSet::new())),
        role_processing_stack: Arc::new(RwLock::new(Vec::new())),
        output_handler: None,
        async_mode: false,
    };

    assert_eq!(run_state.limit_hosts.len(), 0);
    assert_eq!(run_state.limit_groups.len(), 0);
    assert!(run_state.batch_size.is_none());
    assert!(run_state.tags.is_none());
    assert!(!run_state.allow_localhost_delegation);
}

#[test]
fn test_run_state_with_limits() {
    let parser = CliParser::new();
    let inventory = Arc::new(RwLock::new(Inventory::new()));
    let context = Arc::new(RwLock::new(PlaybookContext::new(&parser)));
    
    let run_state = RunState {
        inventory: Arc::clone(&inventory),
        playbook_paths: Arc::new(RwLock::new(vec![PathBuf::from("/path/to/playbook.yml")])),
        role_paths: Arc::new(RwLock::new(Vec::new())),
        module_paths: Arc::new(RwLock::new(Vec::new())),
        limit_hosts: vec!["host1".to_string(), "host2".to_string()],
        limit_groups: vec!["webservers".to_string()],
        batch_size: Some(5),
        context: context,
        visitor: Arc::new(RwLock::new(PlaybookVisitor::new(CheckMode::No))),
        connection_factory: Arc::new(RwLock::new(NoFactory::new())),
        tags: Some(vec!["deploy".to_string(), "configure".to_string()]),
        allow_localhost_delegation: true,
        is_pull_mode: false,
        play_groups: None,
        processed_role_tasks: Arc::new(RwLock::new(std::collections::HashSet::new())),
        processed_role_handlers: Arc::new(RwLock::new(std::collections::HashSet::new())),
        role_processing_stack: Arc::new(RwLock::new(Vec::new())),
        output_handler: None,
        async_mode: false,
    };
    
    assert_eq!(run_state.limit_hosts.len(), 2);
    assert_eq!(run_state.limit_hosts[0], "host1");
    assert_eq!(run_state.limit_groups.len(), 1);
    assert_eq!(run_state.limit_groups[0], "webservers");
    assert_eq!(run_state.batch_size, Some(5));
    assert!(run_state.tags.is_some());
    assert_eq!(run_state.tags.as_ref().unwrap().len(), 2);
    assert!(run_state.allow_localhost_delegation);
}

#[test]
fn test_run_state_paths() {
    let parser = CliParser::new();
    let inventory = Arc::new(RwLock::new(Inventory::new()));
    let context = Arc::new(RwLock::new(PlaybookContext::new(&parser)));
    
    let playbook_paths = vec![
        PathBuf::from("/path/to/playbook1.yml"),
        PathBuf::from("/path/to/playbook2.yml"),
    ];
    
    let role_paths = vec![
        PathBuf::from("/path/to/roles"),
        PathBuf::from("/custom/roles"),
    ];
    
    let module_paths = vec![
        PathBuf::from("/path/to/modules"),
    ];
    
    let run_state = RunState {
        inventory: Arc::clone(&inventory),
        playbook_paths: Arc::new(RwLock::new(playbook_paths)),
        role_paths: Arc::new(RwLock::new(role_paths)),
        module_paths: Arc::new(RwLock::new(module_paths)),
        limit_hosts: Vec::new(),
        limit_groups: Vec::new(),
        batch_size: None,
        context: context,
        visitor: Arc::new(RwLock::new(PlaybookVisitor::new(CheckMode::Yes))),
        connection_factory: Arc::new(RwLock::new(NoFactory::new())),
        tags: None,
        allow_localhost_delegation: false,
        is_pull_mode: false,
        play_groups: None,
        processed_role_tasks: Arc::new(RwLock::new(std::collections::HashSet::new())),
        processed_role_handlers: Arc::new(RwLock::new(std::collections::HashSet::new())),
        role_processing_stack: Arc::new(RwLock::new(Vec::new())),
        output_handler: None,
        async_mode: false,
    };
    
    let playbook_paths = run_state.playbook_paths.read().unwrap();
    assert_eq!(playbook_paths.len(), 2);
    
    let role_paths = run_state.role_paths.read().unwrap();
    assert_eq!(role_paths.len(), 2);
    
    let module_paths = run_state.module_paths.read().unwrap();
    assert_eq!(module_paths.len(), 1);
}