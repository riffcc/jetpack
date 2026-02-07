use jetpack::playbooks::language::*;
use serde_yaml;

#[test]
fn test_play_debug() {
    let play = Play {
        name: "Test Play".to_string(),
        groups: vec!["web".to_string(), "db".to_string()],
        roles: None,
        defaults: None,
        vars: None,
        vars_files: None,
        sudo: None,
        sudo_template: None,
        ssh_user: None,
        ssh_port: None,
        tasks: None,
        handlers: None,
        batch_size: None,
        instantiate: None,
    };
    
    let debug_str = format!("{:?}", play);
    assert!(debug_str.contains("Test Play"));
    assert!(debug_str.contains("web"));
    assert!(debug_str.contains("db"));
}

#[test]
fn test_play_with_options() {
    let mut vars = serde_yaml::Mapping::new();
    vars.insert(
        serde_yaml::Value::String("test_var".to_string()),
        serde_yaml::Value::String("test_value".to_string()),
    );
    
    let play = Play {
        name: "Complex Play".to_string(),
        groups: vec!["all".to_string()],
        roles: Some(vec![
            RoleInvocation {
                role: "common".to_string(),
                vars: None,
                tags: Some(vec!["setup".to_string()]),
            }
        ]),
        defaults: None,
        vars: Some(vars.clone()),
        vars_files: Some(vec!["vars.yml".to_string()]),
        sudo: Some("root".to_string()),
        sudo_template: Some("sudo -u {{user}}".to_string()),
        ssh_user: Some("deploy".to_string()),
        ssh_port: Some(2222),
        tasks: None,
        handlers: None,
        batch_size: Some(10),
        instantiate: None,
    };

    assert_eq!(play.name, "Complex Play");
    assert_eq!(play.groups.len(), 1);
    assert!(play.roles.is_some());
    assert!(play.vars.is_some());
    assert_eq!(play.ssh_port, Some(2222));
    assert_eq!(play.batch_size, Some(10));
}

#[test]
fn test_role_debug() {
    let role = Role {
        name: "common".to_string(),
        defaults: None,
        tasks: Some(vec!["task1.yml".to_string(), "task2.yml".to_string()]),
        handlers: Some(vec!["handler1.yml".to_string()]),
        dependencies: None,
    };
    
    let debug_str = format!("{:?}", role);
    assert!(debug_str.contains("common"));
    assert!(debug_str.contains("task1.yml"));
    assert!(debug_str.contains("handler1.yml"));
}

#[test]
fn test_role_clone() {
    let role = Role {
        name: "test".to_string(),
        defaults: None,
        tasks: Some(vec!["task.yml".to_string()]),
        handlers: None,
        dependencies: None,
    };
    
    let cloned = role.clone();
    assert_eq!(cloned.name, role.name);
    assert_eq!(cloned.tasks, role.tasks);
}

#[test]
fn test_role_invocation_debug() {
    let mut vars = serde_yaml::Mapping::new();
    vars.insert(
        serde_yaml::Value::String("var1".to_string()),
        serde_yaml::Value::String("value1".to_string()),
    );
    
    let invocation = RoleInvocation {
        role: "webserver".to_string(),
        vars: Some(vars),
        tags: Some(vec!["web".to_string(), "nginx".to_string()]),
    };
    
    let debug_str = format!("{:?}", invocation);
    assert!(debug_str.contains("webserver"));
    assert!(debug_str.contains("var1"));
    assert!(debug_str.contains("web"));
    assert!(debug_str.contains("nginx"));
}

#[test]
fn test_role_invocation_minimal() {
    let invocation = RoleInvocation {
        role: "minimal".to_string(),
        vars: None,
        tags: None,
    };
    
    assert_eq!(invocation.role, "minimal");
    assert!(invocation.vars.is_none());
    assert!(invocation.tags.is_none());
}

#[test]
fn test_play_deserialization() {
    let yaml = r#"
name: Test Play
groups:
  - web
  - db
"#;
    
    let play: Result<Play, _> = serde_yaml::from_str(yaml);
    assert!(play.is_ok());
    
    let play = play.unwrap();
    assert_eq!(play.name, "Test Play");
    assert_eq!(play.groups.len(), 2);
    assert!(play.groups.contains(&"web".to_string()));
    assert!(play.groups.contains(&"db".to_string()));
}

#[test]
fn test_role_deserialization() {
    let yaml = r#"
name: common
tasks:
  - install.yml
  - configure.yml
handlers:
  - restart.yml
"#;
    
    let role: Result<Role, _> = serde_yaml::from_str(yaml);
    assert!(role.is_ok());
    
    let role = role.unwrap();
    assert_eq!(role.name, "common");
    assert!(role.tasks.is_some());
    assert_eq!(role.tasks.as_ref().unwrap().len(), 2);
    assert!(role.handlers.is_some());
}

#[test]
fn test_role_invocation_deserialization() {
    let yaml = r#"
role: nginx
tags:
  - web
  - proxy
"#;
    
    let invocation: Result<RoleInvocation, _> = serde_yaml::from_str(yaml);
    assert!(invocation.is_ok());
    
    let invocation = invocation.unwrap();
    assert_eq!(invocation.role, "nginx");
    assert!(invocation.tags.is_some());
    assert_eq!(invocation.tags.as_ref().unwrap().len(), 2);
}