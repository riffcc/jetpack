use jetpack::modules::packages::pacman::*;
use jetpack::tasks::*;

#[test]
fn test_pacman_task_deserialize() {
    let yaml = r#"
package: vim
"#;
    let task: Result<PacmanTask, _> = serde_yaml::from_str(yaml);
    assert!(task.is_ok());
    let task = task.unwrap();
    assert_eq!(task.package, "vim");
    assert!(task.version.is_none());
    assert!(task.update.is_none());
    assert!(task.remove.is_none());
}

#[test]
fn test_pacman_task_deserialize_with_version() {
    let yaml = r#"
package: vim
version: "9.0.1234-1"
"#;
    let task: Result<PacmanTask, _> = serde_yaml::from_str(yaml);
    assert!(task.is_ok());
    let task = task.unwrap();
    assert_eq!(task.package, "vim");
    assert_eq!(task.version, Some("9.0.1234-1".to_string()));
}

#[test]
fn test_pacman_task_deserialize_with_flags() {
    let yaml = r#"
package: vim
update: true
remove: false
"#;
    let task: Result<PacmanTask, _> = serde_yaml::from_str(yaml);
    assert!(task.is_ok());
    let task = task.unwrap();
    assert_eq!(task.package, "vim");
    assert_eq!(task.update, Some("true".to_string()));
    assert_eq!(task.remove, Some("false".to_string()));
}

#[test]
fn test_pacman_task_get_module() {
    let task = PacmanTask {
        retry: None,
        delay: None,
        name: None,
        package: "vim".to_string().into(),
        version: None,
        update: None,
        remove: None,
        with: None,
        and: None,
    };
    assert_eq!(task.get_module(), "pacman");
}

#[test]
fn test_pacman_task_get_name() {
    let task = PacmanTask {
        retry: None,
        delay: None,
        name: Some("Install vim editor".to_string()),
        package: "vim".to_string().into(),
        version: None,
        update: None,
        remove: None,
        with: None,
        and: None,
    };
    assert_eq!(task.get_name(), Some("Install vim editor".to_string()));
}
