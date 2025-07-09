use jetpack::modules::files::directory::*;
use jetpack::tasks::*;

#[test]
fn test_directory_task_basic() {
    let task = DirectoryTask {
        name: Some("Test Directory".to_string()),
        path: "/tmp/testdir".to_string(),
        remove: None,
        recurse: None,
        attributes: None,
        with: None,
        and: None,
    };

    assert_eq!(task.get_module(), "directory");
    assert_eq!(task.get_name(), Some("Test Directory".to_string()));
    assert_eq!(task.path, "/tmp/testdir");
    assert!(task.get_with().is_none());
}

#[test]
fn test_directory_task_with_recurse() {
    let task = DirectoryTask {
        name: None,
        path: "/var/www".to_string(),
        remove: None,
        recurse: Some("yes".to_string()),
        attributes: Some(FileAttributesInput {
            owner: Some("www-data".to_string()),
            group: Some("www-data".to_string()),
            mode: Some("0755".to_string()),
        }),
        with: None,
        and: None,
    };

    assert_eq!(task.get_module(), "directory");
    assert!(task.recurse.is_some());
    assert!(task.attributes.is_some());
}

#[test]
fn test_directory_task_remove() {
    let task = DirectoryTask {
        name: Some("Remove directory".to_string()),
        path: "/tmp/old_dir".to_string(),
        remove: Some("yes".to_string()),
        recurse: Some("yes".to_string()),
        attributes: None,
        with: None,
        and: None,
    };

    assert!(task.remove.is_some());
    assert!(task.recurse.is_some());
}

#[test]
fn test_directory_task_deserialization() {
    let yaml = r#"
name: Create web directory
path: /var/www/html
recurse: "yes"
attributes:
  owner: www-data
  group: www-data
  mode: "0755"
"#;

    let task: Result<DirectoryTask, _> = serde_yaml::from_str(yaml);
    assert!(task.is_ok());
    
    let task = task.unwrap();
    assert_eq!(task.name, Some("Create web directory".to_string()));
    assert_eq!(task.path, "/var/www/html");
    assert_eq!(task.recurse, Some("yes".to_string()));
    assert!(task.attributes.is_some());
}

#[test]
fn test_directory_task_deserialization_minimal() {
    let yaml = r#"
path: /tmp/simple_dir
"#;

    let task: Result<DirectoryTask, _> = serde_yaml::from_str(yaml);
    assert!(task.is_ok());
    
    let task = task.unwrap();
    assert_eq!(task.path, "/tmp/simple_dir");
    assert!(task.remove.is_none());
    assert!(task.recurse.is_none());
}

#[test]
fn test_directory_task_with_logic() {
    let yaml = r#"
path: /opt/app/logs
recurse: "yes"
with:
  condition: "{{ app_installed }}"
and:
  notify: "restart_app"
"#;

    let task: Result<DirectoryTask, _> = serde_yaml::from_str(yaml);
    assert!(task.is_ok());
    
    let task = task.unwrap();
    assert!(task.with.is_some());
    assert!(task.and.is_some());
}