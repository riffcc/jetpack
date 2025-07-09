use jetpack::modules::packages::yum_dnf::*;
use jetpack::tasks::*;

#[test]
fn test_yum_dnf_task_basic() {
    let task = YumDnfTask {
        name: Some("Install httpd".to_string()),
        package: "httpd".to_string(),
        version: None,
        update: None,
        remove: None,
        with: None,
        and: None,
    };

    assert_eq!(task.get_module(), "yum_dnf");
    assert_eq!(task.get_name(), Some("Install httpd".to_string()));
    assert_eq!(task.package, "httpd");
    assert!(task.version.is_none());
    assert!(task.get_with().is_none());
}

#[test]
fn test_yum_dnf_task_with_version() {
    let task = YumDnfTask {
        name: None,
        package: "mariadb-server".to_string(),
        version: Some("10.5".to_string()),
        update: None,
        remove: None,
        with: None,
        and: None,
    };

    assert_eq!(task.get_module(), "yum_dnf");
    assert_eq!(task.package, "mariadb-server");
    assert_eq!(task.version, Some("10.5".to_string()));
}

#[test]
fn test_yum_dnf_task_remove() {
    let task = YumDnfTask {
        name: Some("Remove old package".to_string()),
        package: "php-5.6".to_string(),
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
fn test_yum_dnf_task_with_update() {
    let task = YumDnfTask {
        name: Some("Update kernel".to_string()),
        package: "kernel".to_string(),
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
fn test_yum_dnf_task_deserialization() {
    let yaml = r#"
name: Install database server
package: postgresql-server
version: "13"
update: "yes"
"#;

    let task: Result<YumDnfTask, _> = serde_yaml::from_str(yaml);
    assert!(task.is_ok());
    
    let task = task.unwrap();
    assert_eq!(task.name, Some("Install database server".to_string()));
    assert_eq!(task.package, "postgresql-server");
    assert_eq!(task.version, Some("13".to_string()));
    assert_eq!(task.update, Some("yes".to_string()));
}

#[test]
fn test_yum_dnf_task_deserialization_minimal() {
    let yaml = r#"
package: vim-enhanced
"#;

    let task: Result<YumDnfTask, _> = serde_yaml::from_str(yaml);
    assert!(task.is_ok());
    
    let task = task.unwrap();
    assert_eq!(task.package, "vim-enhanced");
    assert!(task.name.is_none());
    assert!(task.version.is_none());
}

#[test]
fn test_yum_dnf_task_with_logic() {
    let yaml = r#"
package: "{{ rpm_package }}"
version: "{{ rpm_version | default('latest') }}"
with:
  condition: "{{ is_redhat_based }}"
and:
  notify: "restart_service"
"#;

    let task: Result<YumDnfTask, _> = serde_yaml::from_str(yaml);
    assert!(task.is_ok());
    
    let task = task.unwrap();
    assert!(task.package.contains("{{ rpm_package }}"));
    assert!(task.with.is_some());
    assert!(task.and.is_some());
}