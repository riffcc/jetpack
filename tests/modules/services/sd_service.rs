use jetpack::modules::services::sd_service::*;
use jetpack::tasks::*;

#[test]
fn test_systemd_service_task_basic() {
    let task = SystemdServiceTask {
        name: Some("Enable nginx".to_string()),
        service: "nginx".to_string(),
        enabled: None,
        started: None,
        restart: None,
        with: None,
        and: None,
    };

    assert_eq!(task.get_module(), "sd_service");
    assert_eq!(task.get_name(), Some("Enable nginx".to_string()));
    assert_eq!(task.service, "nginx");
    assert!(task.enabled.is_none());
    assert!(task.get_with().is_none());
}

#[test]
fn test_systemd_service_task_enabled_started() {
    let task = SystemdServiceTask {
        name: None,
        service: "postgresql".to_string(),
        enabled: Some("yes".to_string()),
        started: Some("yes".to_string()),
        restart: None,
        with: None,
        and: None,
    };

    assert_eq!(task.get_module(), "sd_service");
    assert_eq!(task.service, "postgresql");
    assert_eq!(task.enabled, Some("yes".to_string()));
    assert_eq!(task.started, Some("yes".to_string()));
}

#[test]
fn test_systemd_service_task_restart() {
    let task = SystemdServiceTask {
        name: Some("Restart web server".to_string()),
        service: "httpd".to_string(),
        enabled: None,
        started: None,
        restart: Some("yes".to_string()),
        with: None,
        and: None,
    };

    assert!(task.restart.is_some());
    assert_eq!(task.restart.unwrap(), "yes");
}

#[test]
fn test_systemd_service_task_disabled_stopped() {
    let task = SystemdServiceTask {
        name: Some("Disable service".to_string()),
        service: "firewalld".to_string(),
        enabled: Some("no".to_string()),
        started: Some("no".to_string()),
        restart: None,
        with: None,
        and: None,
    };

    assert_eq!(task.enabled, Some("no".to_string()));
    assert_eq!(task.started, Some("no".to_string()));
}

#[test]
fn test_systemd_service_task_deserialization() {
    let yaml = r#"
name: Manage database service
service: mariadb
enabled: "yes"
started: "yes"
"#;

    let task: Result<SystemdServiceTask, _> = serde_yaml::from_str(yaml);
    assert!(task.is_ok());
    
    let task = task.unwrap();
    assert_eq!(task.name, Some("Manage database service".to_string()));
    assert_eq!(task.service, "mariadb");
    assert_eq!(task.enabled, Some("yes".to_string()));
    assert_eq!(task.started, Some("yes".to_string()));
}

#[test]
fn test_systemd_service_task_deserialization_minimal() {
    let yaml = r#"
service: sshd
"#;

    let task: Result<SystemdServiceTask, _> = serde_yaml::from_str(yaml);
    assert!(task.is_ok());
    
    let task = task.unwrap();
    assert_eq!(task.service, "sshd");
    assert!(task.name.is_none());
    assert!(task.enabled.is_none());
    assert!(task.started.is_none());
    assert!(task.restart.is_none());
}

#[test]
fn test_systemd_service_task_with_logic() {
    let yaml = r#"
service: "{{ app_service_name }}"
enabled: "{{ enable_service | default('yes') }}"
started: "yes"
with:
  condition: "{{ is_systemd }}"
and:
  notify: "service_configured"
"#;

    let task: Result<SystemdServiceTask, _> = serde_yaml::from_str(yaml);
    assert!(task.is_ok());
    
    let task = task.unwrap();
    assert!(task.service.contains("{{ app_service_name }}"));
    assert!(task.with.is_some());
    assert!(task.and.is_some());
}

#[test]
fn test_systemd_service_task_restart_only() {
    let yaml = r#"
name: Restart application
service: myapp
restart: "yes"
"#;

    let task: Result<SystemdServiceTask, _> = serde_yaml::from_str(yaml);
    assert!(task.is_ok());
    
    let task = task.unwrap();
    assert_eq!(task.name, Some("Restart application".to_string()));
    assert_eq!(task.service, "myapp");
    assert_eq!(task.restart, Some("yes".to_string()));
    assert!(task.enabled.is_none());
    assert!(task.started.is_none());
}