use jetpack::modules::control::assert::*;
use jetpack::tasks::*;

#[test]
fn test_assert_task_basic() {
    let task = AssertTask {
        name: Some("Test Assert".to_string()),
        msg: Some("Test message".to_string()),
        r#true: Some("{{ test_true }}".to_string()),
        r#false: None,
        all_true: None,
        all_false: None,
        some_true: None,
        with: None,
        and: None,
    };

    assert_eq!(task.get_module(), "assert");
    assert_eq!(task.get_name(), Some("Test Assert".to_string()));
    assert!(task.get_with().is_none());
}

#[test]
fn test_assert_task_with_lists() {
    let task = AssertTask {
        name: None,
        msg: None,
        r#true: None,
        r#false: None,
        all_true: Some(vec!["{{ test_true }}".to_string(), "{{ 1 == 1 }}".to_string()]),
        all_false: Some(vec!["{{ test_false }}".to_string(), "{{ 1 == 2 }}".to_string()]),
        some_true: Some(vec!["{{ test_false }}".to_string(), "{{ test_true }}".to_string()]),
        with: None,
        and: None,
    };

    assert_eq!(task.get_module(), "assert");
    assert!(task.all_true.is_some());
    assert!(task.all_false.is_some());
    assert!(task.some_true.is_some());
}

#[test]
fn test_assert_task_deserialization() {
    let yaml = r#"
name: Test Assert
msg: Test assertion
true: "{{ test_var == 42 }}"
false: "{{ test_var == 0 }}"
"#;

    let task: Result<AssertTask, _> = serde_yaml::from_str(yaml);
    assert!(task.is_ok());
    
    let task = task.unwrap();
    assert_eq!(task.name, Some("Test Assert".to_string()));
    assert_eq!(task.msg, Some("Test assertion".to_string()));
    assert!(task.r#true.is_some());
    assert!(task.r#false.is_some());
}

#[test]
fn test_assert_task_deserialization_with_lists() {
    let yaml = r#"
all_true:
  - "{{ condition1 }}"
  - "{{ condition2 }}"
all_false:
  - "{{ bad1 }}"
  - "{{ bad2 }}"
some_true:
  - "{{ maybe1 }}"
  - "{{ maybe2 }}"
"#;

    let task: Result<AssertTask, _> = serde_yaml::from_str(yaml);
    assert!(task.is_ok());
    
    let task = task.unwrap();
    assert_eq!(task.all_true.as_ref().unwrap().len(), 2);
    assert_eq!(task.all_false.as_ref().unwrap().len(), 2);
    assert_eq!(task.some_true.as_ref().unwrap().len(), 2);
}