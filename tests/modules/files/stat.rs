use jetpack::modules::files::stat::*;
use jetpack::tasks::*;

#[test]
fn test_stat_task_basic() {
    let task = StatTask {
        name: Some("Check file status".to_string()),
        path: "/etc/passwd".to_string(),
        save: "passwd_stat".to_string(),
        with: None,
        and: None,
    };

    assert_eq!(task.get_module(), "stat");
    assert_eq!(task.get_name(), Some("Check file status".to_string()));
    assert_eq!(task.path, "/etc/passwd");
    assert_eq!(task.save, "passwd_stat");
    assert!(task.get_with().is_none());
}

#[test]
fn test_stat_task_with_logic() {
    let pre_logic = PreLogicInput {
        condition: Some("{{ check_files }}".to_string()),
        subscribe: None,
        sudo: None,
        items: None,
        tags: None,
        delegate_to: None,
    };

    let post_logic = PostLogicInput {
        notify: Some("file_checked".to_string()),
        ignore_errors: None,
        retry: None,
        delay: None,
    };

    let task = StatTask {
        name: None,
        path: "/tmp/important.txt".to_string(),
        save: "important_file_stat".to_string(),
        with: Some(pre_logic),
        and: Some(post_logic),
    };

    assert_eq!(task.get_module(), "stat");
    assert!(task.with.is_some());
    assert!(task.and.is_some());
}

#[test]
fn test_stat_task_deserialization() {
    let yaml = r#"
name: Check application directory
path: /opt/myapp
save: app_dir_stat
"#;

    let task: Result<StatTask, _> = serde_yaml::from_str(yaml);
    assert!(task.is_ok());
    
    let task = task.unwrap();
    assert_eq!(task.name, Some("Check application directory".to_string()));
    assert_eq!(task.path, "/opt/myapp");
    assert_eq!(task.save, "app_dir_stat");
}

#[test]
fn test_stat_task_deserialization_minimal() {
    let yaml = r#"
path: /var/log/syslog
save: syslog_stat
"#;

    let task: Result<StatTask, _> = serde_yaml::from_str(yaml);
    assert!(task.is_ok());
    
    let task = task.unwrap();
    assert_eq!(task.path, "/var/log/syslog");
    assert_eq!(task.save, "syslog_stat");
    assert!(task.name.is_none());
}

#[test]
fn test_stat_task_with_template_variables() {
    let yaml = r#"
path: "{{ config_dir }}/{{ app_name }}.conf"
save: "{{ app_name }}_config_stat"
with:
  condition: "{{ validate_config }}"
"#;

    let task: Result<StatTask, _> = serde_yaml::from_str(yaml);
    assert!(task.is_ok());
    
    let task = task.unwrap();
    assert!(task.path.contains("{{ config_dir }}"));
    assert!(task.save.contains("{{ app_name }}"));
    assert!(task.with.is_some());
}

#[test]
fn test_stat_task_with_items() {
    let yaml = r#"
name: Check multiple files
path: "{{ item }}"
save: "file_{{ item | basename }}_stat"
with:
  items: "{{ files_to_check }}"
"#;

    let task: Result<StatTask, _> = serde_yaml::from_str(yaml);
    assert!(task.is_ok());
    
    let task = task.unwrap();
    assert_eq!(task.name, Some("Check multiple files".to_string()));
    assert!(task.path.contains("{{ item }}"));
    assert!(task.save.contains("{{ item | basename }}"));
    assert!(task.with.is_some());
    assert!(task.with.as_ref().unwrap().items.is_some());
}