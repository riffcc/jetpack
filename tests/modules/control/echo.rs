use jetpack::modules::control::echo::*;
use jetpack::tasks::*;

#[test]
fn test_echo_task_basic() {
    let task = EchoTask {
        name: Some("Test Echo".to_string()),
        msg: "Hello, World!".to_string(),
        with: None,
        and: None,
    };

    assert_eq!(task.get_module(), "echo");
    assert_eq!(task.get_name(), Some("Test Echo".to_string()));
    assert!(task.get_with().is_none());
}

#[test]
fn test_echo_task_with_template() {
    let task = EchoTask {
        name: None,
        msg: "Value is: {{ test_var }}".to_string(),
        with: None,
        and: None,
    };

    assert_eq!(task.get_module(), "echo");
    assert_eq!(task.get_name(), None);
}

#[test]
fn test_echo_task_deserialization() {
    let yaml = r#"
name: Test Echo
msg: Hello from YAML
"#;

    let task: Result<EchoTask, _> = serde_yaml::from_str(yaml);
    assert!(task.is_ok());
    
    let task = task.unwrap();
    assert_eq!(task.name, Some("Test Echo".to_string()));
    assert_eq!(task.msg, "Hello from YAML");
}

#[test]
fn test_echo_task_with_logic() {
    let yaml = r#"
msg: Conditional echo
with:
  condition: "{{ some_condition }}"
and:
  ignore_errors: "yes"
"#;

    let task: Result<EchoTask, _> = serde_yaml::from_str(yaml);
    assert!(task.is_ok());
    
    let task = task.unwrap();
    assert!(task.with.is_some());
    assert!(task.and.is_some());
}