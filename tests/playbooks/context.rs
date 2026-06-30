use jetpack::cli::parser::CliParser;
use jetpack::inventory::hosts::Host;
use jetpack::playbooks::context::*;
use jetpack::playbooks::language::Role;
use std::sync::{Arc, RwLock};

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
  - main.yml
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

fn host_named(name: &str) -> Arc<RwLock<Host>> {
    Arc::new(RwLock::new(Host::new(name)))
}

#[test]
fn destroy_host_removes_from_task_pool_without_marking_failed() {
    let parser = CliParser::new();
    let mut context = PlaybookContext::new(&parser);
    let h1 = host_named("node1");
    let h2 = host_named("node2");
    context.set_targetted_hosts(&[Arc::clone(&h1), Arc::clone(&h2)]);
    assert_eq!(context.get_remaining_hosts().len(), 2);

    context.destroy_host(&h1);

    let remaining = context.get_remaining_hosts();
    assert_eq!(remaining.len(), 1);
    assert!(remaining.contains_key("node2"));
    assert!(!remaining.contains_key("node1"));
    // A destroy is an intentional lifecycle outcome, not a task failure.
    assert_eq!(context.failed_tasks, 0);
}

#[test]
fn destroy_host_on_unknown_host_is_a_no_op() {
    let parser = CliParser::new();
    let mut context = PlaybookContext::new(&parser);
    let h1 = host_named("node1");
    context.set_targetted_hosts(&[Arc::clone(&h1)]);

    context.destroy_host(&host_named("not-in-pool"));

    assert_eq!(context.get_remaining_hosts().len(), 1);
}
