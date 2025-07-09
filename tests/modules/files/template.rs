use jetpack::modules::files::template::*;
use jetpack::tasks::*;

#[test]
fn test_template_task_basic() {
    let task = TemplateTask {
        name: Some("Deploy config template".to_string()),
        src: "templates/app.conf.j2".to_string(),
        dest: "/etc/app/app.conf".to_string(),
        attributes: None,
        with: None,
        and: None,
    };

    assert_eq!(task.get_module(), "template");
    assert_eq!(task.get_name(), Some("Deploy config template".to_string()));
    assert_eq!(task.src, "templates/app.conf.j2");
    assert_eq!(task.dest, "/etc/app/app.conf");
    assert!(task.get_with().is_none());
}

#[test]
fn test_template_task_with_attributes() {
    let attrs = FileAttributesInput {
        owner: Some("nginx".to_string()),
        group: Some("nginx".to_string()),
        mode: Some("0640".to_string()),
    };

    let task = TemplateTask {
        name: None,
        src: "nginx.conf.j2".to_string(),
        dest: "/etc/nginx/nginx.conf".to_string(),
        attributes: Some(attrs),
        with: None,
        and: None,
    };

    assert_eq!(task.get_module(), "template");
    assert!(task.attributes.is_some());
    let attrs = task.attributes.as_ref().unwrap();
    assert_eq!(attrs.owner, Some("nginx".to_string()));
    assert_eq!(attrs.mode, Some("0640".to_string()));
}

#[test]
fn test_template_task_deserialization() {
    let yaml = r#"
name: Deploy database config
src: templates/database.yml.j2
dest: /opt/app/config/database.yml
attributes:
  owner: app
  group: app
  mode: "0600"
"#;

    let task: Result<TemplateTask, _> = serde_yaml::from_str(yaml);
    assert!(task.is_ok());
    
    let task = task.unwrap();
    assert_eq!(task.name, Some("Deploy database config".to_string()));
    assert_eq!(task.src, "templates/database.yml.j2");
    assert_eq!(task.dest, "/opt/app/config/database.yml");
    assert!(task.attributes.is_some());
}

#[test]
fn test_template_task_deserialization_minimal() {
    let yaml = r#"
src: motd.j2
dest: /etc/motd
"#;

    let task: Result<TemplateTask, _> = serde_yaml::from_str(yaml);
    assert!(task.is_ok());
    
    let task = task.unwrap();
    assert_eq!(task.src, "motd.j2");
    assert_eq!(task.dest, "/etc/motd");
    assert!(task.name.is_none());
    assert!(task.attributes.is_none());
}

#[test]
fn test_template_task_with_logic() {
    let yaml = r#"
src: "templates/{{ env }}.conf.j2"
dest: /etc/app/config.conf
with:
  condition: "{{ deploy_config }}"
and:
  notify: "restart_app"
  retry: "3"
  delay: "5"
"#;

    let task: Result<TemplateTask, _> = serde_yaml::from_str(yaml);
    assert!(task.is_ok());
    
    let task = task.unwrap();
    assert!(task.with.is_some());
    assert!(task.and.is_some());
}

#[test]
fn test_template_task_with_variables() {
    let yaml = r#"
name: Deploy environment-specific config
src: "templates/{{ app_name }}/{{ environment }}.conf.j2"
dest: "/etc/{{ app_name }}/config.conf"
attributes:
  owner: "{{ app_user }}"
  group: "{{ app_group }}"
  mode: "0644"
"#;

    let task: Result<TemplateTask, _> = serde_yaml::from_str(yaml);
    assert!(task.is_ok());
    
    let task = task.unwrap();
    assert_eq!(task.name, Some("Deploy environment-specific config".to_string()));
    assert!(task.src.contains("{{ app_name }}"));
    assert!(task.dest.contains("{{ app_name }}"));
}