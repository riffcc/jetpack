use jetpack::modules::control::debug::*;
use jetpack::tasks::*;

#[test]
fn test_debug_task_basic() {
    let task = DebugTask {
        name: Some("Test Debug".to_string()),
        vars: Some(vec!["test_var".to_string()]),
        with: None,
        and: None,
    };

    assert_eq!(task.get_module(), "debug");
    assert_eq!(task.get_name(), Some("Test Debug".to_string()));
    assert!(task.get_with().is_none());
}

#[test]
fn test_debug_task_no_vars() {
    let task = DebugTask {
        name: None,
        vars: None,
        with: None,
        and: None,
    };

    assert_eq!(task.get_module(), "debug");
    assert_eq!(task.get_name(), None);
    assert!(task.vars.is_none());
}

#[test]
fn test_debug_task_deserialization() {
    let yaml = r#"
name: Test Debug
vars:
  - var1
  - var2
"#;

    let task: Result<DebugTask, _> = serde_yaml::from_str(yaml);
    assert!(task.is_ok());
    
    let task = task.unwrap();
    assert_eq!(task.name, Some("Test Debug".to_string()));
    assert_eq!(task.vars, Some(vec!["var1".to_string(), "var2".to_string()]));
}

#[test]
fn test_debug_task_deserialization_minimal() {
    let yaml = r#"
vars:
"#;

    let task: Result<DebugTask, _> = serde_yaml::from_str(yaml);
    assert!(task.is_ok());
    
    let task = task.unwrap();
    assert!(task.name.is_none());
    assert!(task.vars.is_none());
}

#[test]
fn test_debug_task_with_logic() {
    let yaml = r#"
vars:
  - debug_var
with:
  condition: "{{ debug_enabled }}"
and:
  retry: "3"
"#;

    let task: Result<DebugTask, _> = serde_yaml::from_str(yaml);
    assert!(task.is_ok());
    
    let task = task.unwrap();
    assert!(task.with.is_some());
    assert!(task.and.is_some());
}