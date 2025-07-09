use jetpack::modules::control::set::*;
use jetpack::tasks::*;

#[test]
fn test_set_task_basic() {
    let mut vars = serde_yaml::Mapping::new();
    vars.insert(
        serde_yaml::Value::String("new_var".to_string()),
        serde_yaml::Value::String("new_value".to_string()),
    );

    let task = SetTask {
        name: Some("Test Set".to_string()),
        vars: Some(vars),
        with: None,
        and: None,
    };

    assert_eq!(task.get_module(), "Set");
    assert_eq!(task.get_name(), Some("Test Set".to_string()));
    assert!(task.get_with().is_none());
    assert!(task.vars.is_some());
}

#[test]
fn test_set_task_no_vars() {
    let task = SetTask {
        name: None,
        vars: None,
        with: None,
        and: None,
    };

    assert_eq!(task.get_module(), "Set");
    assert_eq!(task.get_name(), None);
    assert!(task.vars.is_none());
}

#[test]
fn test_set_task_deserialization() {
    let yaml = r#"
name: Set Variables
vars:
  var1: value1
  var2: "{{ computed_value }}"
  var3: 123
"#;

    let task: Result<SetTask, _> = serde_yaml::from_str(yaml);
    assert!(task.is_ok());
    
    let task = task.unwrap();
    assert_eq!(task.name, Some("Set Variables".to_string()));
    assert!(task.vars.is_some());
    assert_eq!(task.vars.as_ref().unwrap().len(), 3);
}

#[test]
fn test_set_task_deserialization_empty() {
    let yaml = r#"
vars:
"#;

    let task: Result<SetTask, _> = serde_yaml::from_str(yaml);
    assert!(task.is_ok());
    
    let task = task.unwrap();
    assert!(task.name.is_none());
    assert!(task.vars.is_none());
}

#[test]
fn test_set_task_with_logic() {
    let yaml = r#"
vars:
  conditional_var: "conditional_value"
with:
  condition: "{{ should_set }}"
and:
  notify: "handler_name"
"#;

    let task: Result<SetTask, _> = serde_yaml::from_str(yaml);
    assert!(task.is_ok());
    
    let task = task.unwrap();
    assert!(task.with.is_some());
    assert!(task.and.is_some());
}