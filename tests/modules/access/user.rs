use jetpack::modules::access::user::*;
use jetpack::tasks::*;
use std::collections::HashSet;

#[test]
fn test_user_task_basic() {
    let task = UserTask {
        name: Some("Create deploy user".to_string()),
        user: "deploy".to_string(),
        uid: None,
        system: None,
        gid: None,
        groups: None,
        append: None,
        create_home: None,
        create_user_group: None,
        gecos: None,
        shell: None,
        remove: None,
        cleanup: None,
        with: None,
        and: None,
    };

    assert_eq!(task.get_module(), "user");
    assert_eq!(task.get_name(), Some("Create deploy user".to_string()));
    assert_eq!(task.user, "deploy");
    assert!(task.get_with().is_none());
}

#[test]
fn test_user_task_with_details() {
    let mut groups = HashSet::new();
    groups.insert("wheel".to_string());
    groups.insert("docker".to_string());

    let task = UserTask {
        name: None,
        user: "webuser".to_string(),
        uid: Some("1001".to_string()),
        system: Some("no".to_string()),
        gid: Some("1001".to_string()),
        groups: Some(groups),
        append: Some("yes".to_string()),
        create_home: Some("yes".to_string()),
        create_user_group: Some("yes".to_string()),
        gecos: Some("Web Application User".to_string()),
        shell: Some("/bin/bash".to_string()),
        remove: None,
        cleanup: None,
        with: None,
        and: None,
    };

    assert_eq!(task.get_module(), "user");
    assert_eq!(task.uid, Some("1001".to_string()));
    assert!(task.groups.is_some());
    let groups = task.groups.unwrap();
    assert!(groups.contains("wheel"));
    assert!(groups.contains("docker"));
}

#[test]
fn test_user_task_system_user() {
    let task = UserTask {
        name: Some("Create system user".to_string()),
        user: "prometheus".to_string(),
        uid: None,
        system: Some("yes".to_string()),
        gid: None,
        groups: None,
        append: None,
        create_home: Some("no".to_string()),
        create_user_group: Some("yes".to_string()),
        gecos: None,
        shell: Some("/usr/sbin/nologin".to_string()),
        remove: None,
        cleanup: None,
        with: None,
        and: None,
    };

    assert_eq!(task.system, Some("yes".to_string()));
    assert_eq!(task.create_home, Some("no".to_string()));
    assert_eq!(task.shell, Some("/usr/sbin/nologin".to_string()));
}

#[test]
fn test_user_task_remove() {
    let task = UserTask {
        name: Some("Remove user".to_string()),
        user: "tempuser".to_string(),
        uid: None,
        system: None,
        gid: None,
        groups: None,
        append: None,
        create_home: None,
        create_user_group: None,
        gecos: None,
        shell: None,
        remove: Some("yes".to_string()),
        cleanup: None,
        with: None,
        and: None,
    };

    assert!(task.remove.is_some());
    assert_eq!(task.remove.unwrap(), "yes");
}

#[test]
fn test_user_task_deserialization() {
    let yaml = r#"
name: Create application user
user: appuser
uid: "2001"
groups: ["app", "logs"]
append: "yes"
shell: /bin/bash
gecos: "Application User"
"#;

    let task: Result<UserTask, _> = serde_yaml::from_str(yaml);
    assert!(task.is_ok());
    
    let task = task.unwrap();
    assert_eq!(task.name, Some("Create application user".to_string()));
    assert_eq!(task.user, "appuser");
    assert_eq!(task.uid, Some("2001".to_string()));
    assert!(task.groups.is_some());
    assert_eq!(task.shell, Some("/bin/bash".to_string()));
}

#[test]
fn test_user_task_deserialization_minimal() {
    let yaml = r#"
user: testuser
"#;

    let task: Result<UserTask, _> = serde_yaml::from_str(yaml);
    assert!(task.is_ok());
    
    let task = task.unwrap();
    assert_eq!(task.user, "testuser");
    assert!(task.name.is_none());
    assert!(task.uid.is_none());
}

#[test]
fn test_user_task_with_logic() {
    let yaml = r#"
user: "{{ username }}"
groups: ["sudo", "{{ app_group }}"]
with:
  condition: "{{ create_users }}"
and:
  notify: "user_created"
"#;

    let task: Result<UserTask, _> = serde_yaml::from_str(yaml);
    assert!(task.is_ok());
    
    let task = task.unwrap();
    assert!(task.user.contains("{{ username }}"));
    assert!(task.with.is_some());
    assert!(task.and.is_some());
}