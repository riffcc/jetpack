use jetpack::modules::files::file::*;
use jetpack::tasks::*;

#[test]
fn test_file_task_basic() {
    let task = FileTask {
        name: Some("Test File".to_string()),
        path: "/tmp/test.txt".to_string(),
        remove: None,
        attributes: None,
        with: None,
        and: None,
    };

    assert_eq!(task.get_module(), "file");
    assert_eq!(task.get_name(), Some("Test File".to_string()));
    assert_eq!(task.path, "/tmp/test.txt");
    assert!(task.get_with().is_none());
}

#[test]
fn test_file_task_with_remove() {
    let task = FileTask {
        name: None,
        path: "/tmp/remove_me.txt".to_string(),
        remove: Some("yes".to_string()),
        attributes: None,
        with: None,
        and: None,
    };

    assert_eq!(task.get_module(), "file");
    assert!(task.remove.is_some());
}

#[test]
fn test_file_task_with_attributes() {
    let attrs = FileAttributesInput {
        owner: Some("user".to_string()),
        group: Some("group".to_string()),
        mode: Some("0644".to_string()),
    };

    let task = FileTask {
        name: Some("File with attrs".to_string()),
        path: "/etc/config.conf".to_string(),
        remove: None,
        attributes: Some(attrs),
        with: None,
        and: None,
    };

    assert!(task.attributes.is_some());
    assert_eq!(task.attributes.as_ref().unwrap().owner, Some("user".to_string()));
}

#[test]
fn test_file_task_deserialization() {
    let yaml = r#"
name: Create test file
path: /tmp/test.txt
attributes:
  owner: root
  group: root
  mode: "0644"
"#;

    let task: Result<FileTask, _> = serde_yaml::from_str(yaml);
    assert!(task.is_ok());
    
    let task = task.unwrap();
    assert_eq!(task.name, Some("Create test file".to_string()));
    assert_eq!(task.path, "/tmp/test.txt");
    assert!(task.attributes.is_some());
}

#[test]
fn test_file_task_deserialization_remove() {
    let yaml = r#"
path: /tmp/delete_me.txt
remove: "yes"
"#;

    let task: Result<FileTask, _> = serde_yaml::from_str(yaml);
    assert!(task.is_ok());
    
    let task = task.unwrap();
    assert_eq!(task.path, "/tmp/delete_me.txt");
    assert_eq!(task.remove, Some("yes".to_string()));
}

#[test]
fn test_file_task_with_logic() {
    let yaml = r#"
path: /tmp/conditional.txt
with:
  condition: "{{ create_file }}"
and:
  notify: "file_created"
"#;

    let task: Result<FileTask, _> = serde_yaml::from_str(yaml);
    assert!(task.is_ok());
    
    let task = task.unwrap();
    assert!(task.with.is_some());
    assert!(task.and.is_some());
}