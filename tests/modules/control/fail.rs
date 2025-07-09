use jetpack::modules::control::fail::*;
use jetpack::tasks::*;

#[test]
fn test_fail_task_basic() {
    let task = FailTask {
        name: Some("Test Fail".to_string()),
        msg: Some("Custom failure message".to_string()),
        with: None,
        and: None,
    };

    assert_eq!(task.get_module(), "fail");
    assert_eq!(task.get_name(), Some("Test Fail".to_string()));
    assert!(task.get_with().is_none());
}

#[test]
fn test_fail_task_no_message() {
    let task = FailTask {
        name: None,
        msg: None,
        with: None,
        and: None,
    };

    assert_eq!(task.get_module(), "fail");
    assert_eq!(task.get_name(), None);
    assert!(task.msg.is_none());
}

#[test]
fn test_fail_task_deserialization() {
    let yaml = r#"
name: Test Fail
msg: "Something went wrong"
"#;

    let task: Result<FailTask, _> = serde_yaml::from_str(yaml);
    assert!(task.is_ok());
    
    let task = task.unwrap();
    assert_eq!(task.name, Some("Test Fail".to_string()));
    assert_eq!(task.msg, Some("Something went wrong".to_string()));
}

#[test]
fn test_fail_task_deserialization_minimal() {
    let yaml = r#"
msg:
"#;

    let task: Result<FailTask, _> = serde_yaml::from_str(yaml);
    assert!(task.is_ok());
    
    let task = task.unwrap();
    assert!(task.name.is_none());
    assert!(task.msg.is_none());
}

#[test]
fn test_fail_task_with_logic() {
    let yaml = r#"
msg: "Failed when condition met"
with:
  condition: "{{ error_occurred }}"
and:
  ignore_errors: "no"
"#;

    let task: Result<FailTask, _> = serde_yaml::from_str(yaml);
    assert!(task.is_ok());
    
    let task = task.unwrap();
    assert!(task.with.is_some());
    assert!(task.and.is_some());
}