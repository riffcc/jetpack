use jetpack::playbooks::context::*;
use jetpack::cli::parser::CliParser;
use jetpack::playbooks::language::{Play, Role};
use std::sync::{Arc, RwLock};
use serde_yaml;

#[test]
fn test_playbook_context_new() {
    let parser = CliParser::new();
    let context = PlaybookContext::new(&parser);
    
    assert_eq!(context.verbosity, 0);
    assert!(context.playbook_path.is_none());
    assert!(context.playbook_directory.is_none());
    assert!(context.play.is_none());
    assert!(context.role.is_none());
    assert!(context.role_path.is_none());
    assert_eq!(context.play_count, 0);
    assert_eq!(context.role_count, 0);
    assert_eq!(context.task_count, 0);
}

#[test]
fn test_playbook_context_update_playbook_path() {
    let parser = CliParser::new();
    let mut context = PlaybookContext::new(&parser);
    
    context.playbook_path = Some("/path/to/playbook.yml".to_string());
    context.playbook_directory = Some("/path/to".to_string());
    
    assert_eq!(context.playbook_path.unwrap(), "/path/to/playbook.yml");
    assert_eq!(context.playbook_directory.unwrap(), "/path/to");
}

#[test]
fn test_playbook_context_counters() {
    let parser = CliParser::new();
    let mut context = PlaybookContext::new(&parser);
    
    context.play_count = 5;
    context.role_count = 3;
    context.task_count = 10;
    
    assert_eq!(context.play_count, 5);
    assert_eq!(context.role_count, 3);
    assert_eq!(context.task_count, 10);
}

#[test]
fn test_playbook_context_with_play() {
    let parser = CliParser::new();
    let mut context = PlaybookContext::new(&parser);
    
    context.play = Some("Test Play".to_string());
    assert_eq!(context.play.unwrap(), "Test Play");
}

#[test]
fn test_playbook_context_with_role() {
    let parser = CliParser::new();
    let mut context = PlaybookContext::new(&parser);
    
    let role_yaml = r#"
name: test_role
tasks:
  - !echo
    msg: "Hello from role"
"#;
    
    let role: Role = serde_yaml::from_str(role_yaml).unwrap();
    context.role = Some(role);
    context.role_path = Some("/path/to/roles/test_role".to_string());
    
    assert!(context.role.is_some());
    assert_eq!(context.role_path.unwrap(), "/path/to/roles/test_role");
}

#[test]
fn test_playbook_context_verbosity() {
    let parser = CliParser::new();
    let mut context = PlaybookContext::new(&parser);
    
    // Test different verbosity levels
    context.verbosity = 0;
    assert_eq!(context.verbosity, 0);
    
    context.verbosity = 1;
    assert_eq!(context.verbosity, 1);
    
    context.verbosity = 3;
    assert_eq!(context.verbosity, 3);
}