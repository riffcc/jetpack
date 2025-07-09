use jetpack::modules::files::copy::*;
use jetpack::tasks::*;

#[test]
fn test_copy_task_basic() {
    let task = CopyTask {
        name: Some("Copy config file".to_string()),
        src: "files/app.conf".to_string(),
        dest: "/etc/app/app.conf".to_string(),
        attributes: None,
        with: None,
        and: None,
    };

    assert_eq!(task.get_module(), "copy");
    assert_eq!(task.get_name(), Some("Copy config file".to_string()));
    assert_eq!(task.src, "files/app.conf");
    assert_eq!(task.dest, "/etc/app/app.conf");
    assert!(task.get_with().is_none());
}

#[test]
fn test_copy_task_with_attributes() {
    let attrs = FileAttributesInput {
        owner: Some("app".to_string()),
        group: Some("app".to_string()),
        mode: Some("0600".to_string()),
    };

    let task = CopyTask {
        name: None,
        src: "secrets/api_key".to_string(),
        dest: "/opt/app/config/api_key".to_string(),
        attributes: Some(attrs),
        with: None,
        and: None,
    };

    assert_eq!(task.get_module(), "copy");
    assert!(task.attributes.is_some());
    let attrs = task.attributes.as_ref().unwrap();
    assert_eq!(attrs.owner, Some("app".to_string()));
    assert_eq!(attrs.mode, Some("0600".to_string()));
}

#[test]
fn test_copy_task_deserialization() {
    let yaml = r#"
name: Deploy application config
src: files/production.conf
dest: /etc/myapp/config.conf
attributes:
  owner: myapp
  group: myapp
  mode: "0644"
"#;

    let task: Result<CopyTask, _> = serde_yaml::from_str(yaml);
    assert!(task.is_ok());
    
    let task = task.unwrap();
    assert_eq!(task.name, Some("Deploy application config".to_string()));
    assert_eq!(task.src, "files/production.conf");
    assert_eq!(task.dest, "/etc/myapp/config.conf");
    assert!(task.attributes.is_some());
}

#[test]
fn test_copy_task_deserialization_minimal() {
    let yaml = r#"
src: README.md
dest: /tmp/README.md
"#;

    let task: Result<CopyTask, _> = serde_yaml::from_str(yaml);
    assert!(task.is_ok());
    
    let task = task.unwrap();
    assert_eq!(task.src, "README.md");
    assert_eq!(task.dest, "/tmp/README.md");
    assert!(task.name.is_none());
    assert!(task.attributes.is_none());
}

#[test]
fn test_copy_task_with_logic() {
    let yaml = r#"
src: "files/{{ env }}.conf"
dest: /etc/app/config.conf
with:
  condition: "{{ deploy_config }}"
and:
  notify: "reload_app"
"#;

    let task: Result<CopyTask, _> = serde_yaml::from_str(yaml);
    assert!(task.is_ok());
    
    let task = task.unwrap();
    assert!(task.with.is_some());
    assert!(task.and.is_some());
}