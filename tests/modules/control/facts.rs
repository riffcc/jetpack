use jetpack::modules::control::facts::*;
use jetpack::tasks::*;

#[test]
fn test_facts_task_basic() {
    let task = FactsTask {
        name: Some("Test Facts".to_string()),
        facter: Some("true".to_string()),
        ohai: Some("false".to_string()),
        with: None,
        and: None,
    };

    assert_eq!(task.get_module(), "facts");
    assert_eq!(task.get_name(), Some("Test Facts".to_string()));
    assert!(task.get_with().is_none());
}

#[test]
fn test_facts_task_defaults() {
    let task = FactsTask {
        name: None,
        facter: None,
        ohai: None,
        with: None,
        and: None,
    };

    assert_eq!(task.get_module(), "facts");
    assert_eq!(task.get_name(), None);
    assert!(task.facter.is_none());
    assert!(task.ohai.is_none());
}

#[test]
fn test_facts_task_deserialization() {
    let yaml = r#"
name: Gather Facts
facter: "yes"
ohai: "no"
"#;

    let task: Result<FactsTask, _> = serde_yaml::from_str(yaml);
    assert!(task.is_ok());
    
    let task = task.unwrap();
    assert_eq!(task.name, Some("Gather Facts".to_string()));
    assert_eq!(task.facter, Some("yes".to_string()));
    assert_eq!(task.ohai, Some("no".to_string()));
}

#[test]
fn test_facts_task_deserialization_minimal() {
    let yaml = r#"
facter:
"#;

    let task: Result<FactsTask, _> = serde_yaml::from_str(yaml);
    assert!(task.is_ok());
    
    let task = task.unwrap();
    assert!(task.name.is_none());
    assert!(task.facter.is_none());
    assert!(task.ohai.is_none());
}

#[test]
fn test_facts_task_with_logic() {
    let yaml = r#"
facter: "true"
with:
  condition: "{{ gather_external_facts }}"
and:
  delay: "2"
"#;

    let task: Result<FactsTask, _> = serde_yaml::from_str(yaml);
    assert!(task.is_ok());
    
    let task = task.unwrap();
    assert!(task.with.is_some());
    assert!(task.and.is_some());
}