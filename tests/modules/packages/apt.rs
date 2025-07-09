use jetpack::modules::packages::apt::*;
use jetpack::tasks::*;

#[test]
fn test_apt_task_basic() {
    let task = AptTask {
        name: Some("Install nginx".to_string()),
        package: "nginx".to_string(),
        version: None,
        update: None,
        remove: None,
        with: None,
        and: None,
    };

    assert_eq!(task.get_module(), "apt");
    assert_eq!(task.get_name(), Some("Install nginx".to_string()));
    assert_eq!(task.package, "nginx");
    assert!(task.version.is_none());
    assert!(task.get_with().is_none());
}

#[test]
fn test_apt_task_with_version() {
    let task = AptTask {
        name: None,
        package: "postgresql".to_string(),
        version: Some("14".to_string()),
        update: None,
        remove: None,
        with: None,
        and: None,
    };

    assert_eq!(task.get_module(), "apt");
    assert_eq!(task.package, "postgresql");
    assert_eq!(task.version, Some("14".to_string()));
}

#[test]
fn test_apt_task_remove() {
    let task = AptTask {
        name: Some("Remove old package".to_string()),
        package: "apache2".to_string(),
        version: None,
        update: None,
        remove: Some("yes".to_string()),
        with: None,
        and: None,
    };

    assert!(task.remove.is_some());
    assert_eq!(task.remove.unwrap(), "yes");
}

#[test]
fn test_apt_task_with_update() {
    let task = AptTask {
        name: Some("Install latest vim".to_string()),
        package: "vim".to_string(),
        version: None,
        update: Some("yes".to_string()),
        remove: None,
        with: None,
        and: None,
    };

    assert!(task.update.is_some());
    assert_eq!(task.update.unwrap(), "yes");
}

#[test]
fn test_apt_task_deserialization() {
    let yaml = r#"
name: Install web server
package: nginx
version: "1.22"
update: "yes"
"#;

    let task: Result<AptTask, _> = serde_yaml::from_str(yaml);
    assert!(task.is_ok());
    
    let task = task.unwrap();
    assert_eq!(task.name, Some("Install web server".to_string()));
    assert_eq!(task.package, "nginx");
    assert_eq!(task.version, Some("1.22".to_string()));
    assert_eq!(task.update, Some("yes".to_string()));
}

#[test]
fn test_apt_task_deserialization_minimal() {
    let yaml = r#"
package: git
"#;

    let task: Result<AptTask, _> = serde_yaml::from_str(yaml);
    assert!(task.is_ok());
    
    let task = task.unwrap();
    assert_eq!(task.package, "git");
    assert!(task.name.is_none());
    assert!(task.version.is_none());
    assert!(task.update.is_none());
    assert!(task.remove.is_none());
}

#[test]
fn test_apt_task_with_logic() {
    let yaml = r#"
package: "{{ package_name }}"
version: "{{ package_version | default('latest') }}"
with:
  condition: "{{ install_packages }}"
and:
  notify: "restart_service"
"#;

    let task: Result<AptTask, _> = serde_yaml::from_str(yaml);
    assert!(task.is_ok());
    
    let task = task.unwrap();
    assert!(task.package.contains("{{ package_name }}"));
    assert!(task.with.is_some());
    assert!(task.and.is_some());
}