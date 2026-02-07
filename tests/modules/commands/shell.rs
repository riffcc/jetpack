use jetpack::modules::commands::shell::*;
use jetpack::tasks::*;

#[test]
fn test_shell_task_basic() {
    let task = ShellTask {
        name: Some("Run command".to_string()),
        cmd: "echo 'Hello World'".to_string(),
        save: None,
        failed_when: None,
        changed_when: None,
        shell: None,
        with: None,
        and: None,
    };

    assert_eq!(task.get_module(), "Shell");
    assert_eq!(task.get_name(), Some("Run command".to_string()));
    assert_eq!(task.cmd, "echo 'Hello World'");
    assert!(task.get_with().is_none());
}

#[test]
fn test_shell_task_with_save() {
    let task = ShellTask {
        name: None,
        cmd: "hostname -f".to_string(),
        save: Some("hostname_result".to_string()),
        failed_when: None,
        changed_when: None,
        shell: None,
        with: None,
        and: None,
    };

    assert_eq!(task.get_module(), "Shell");
    assert!(task.save.is_some());
    assert_eq!(task.save.unwrap(), "hostname_result");
}

#[test]
fn test_shell_task_with_conditions() {
    let task = ShellTask {
        name: Some("Check service".to_string()),
        cmd: "systemctl status nginx".to_string(),
        save: None,
        failed_when: Some("rc != 0 and rc != 3".to_string()),
        changed_when: Some("false".to_string()),
        shell: None,
        with: None,
        and: None,
    };

    assert!(task.failed_when.is_some());
    assert!(task.changed_when.is_some());
}

#[test]
fn test_shell_task_with_custom_shell() {
    let task = ShellTask {
        name: Some("Run with bash".to_string()),
        cmd: "echo $BASH_VERSION".to_string(),
        save: None,
        failed_when: None,
        changed_when: None,
        shell: Some("bash".to_string()),
        with: None,
        and: None,
    };

    assert!(task.shell.is_some());
    assert_eq!(task.shell.unwrap(), "bash");
}

#[test]
fn test_shell_task_deserialization() {
    let yaml = r#"
name: Create directory
cmd: mkdir -p /opt/myapp/logs
changed_when: "false"
failed_when: "rc != 0"
"#;

    let task: Result<ShellTask, _> = serde_yaml::from_str(yaml);
    assert!(task.is_ok());

    let task = task.unwrap();
    assert_eq!(task.name, Some("Create directory".to_string()));
    assert_eq!(task.cmd, "mkdir -p /opt/myapp/logs");
    assert_eq!(task.changed_when, Some("false".to_string()));
    assert_eq!(task.failed_when, Some("rc != 0".to_string()));
}

#[test]
fn test_shell_task_deserialization_minimal() {
    let yaml = r#"
cmd: date
"#;

    let task: Result<ShellTask, _> = serde_yaml::from_str(yaml);
    assert!(task.is_ok());

    let task = task.unwrap();
    assert_eq!(task.cmd, "date");
    assert!(task.name.is_none());
    assert!(task.save.is_none());
}

#[test]
fn test_shell_task_with_logic() {
    let yaml = r#"
cmd: "{{ install_script }}"
save: install_result
shell: bash
with:
  condition: "{{ needs_installation }}"
and:
  notify: "restart_app"
"#;

    let task: Result<ShellTask, _> = serde_yaml::from_str(yaml);
    assert!(task.is_ok());

    let task = task.unwrap();
    assert!(task.cmd.contains("{{ install_script }}"));
    assert!(task.save.is_some());
    assert!(task.with.is_some());
    assert!(task.and.is_some());
}

#[test]
fn test_shell_task_with_save_and_conditions() {
    let yaml = r#"
name: Check application status
cmd: "/opt/myapp/bin/status.sh"
save: app_status
failed_when: "'ERROR' in stdout"
changed_when: "'RESTARTED' in stdout"
"#;

    let task: Result<ShellTask, _> = serde_yaml::from_str(yaml);
    assert!(task.is_ok());

    let task = task.unwrap();
    assert_eq!(task.save, Some("app_status".to_string()));
    assert!(task.failed_when.unwrap().contains("stdout"));
    assert!(task.changed_when.unwrap().contains("stdout"));
}
