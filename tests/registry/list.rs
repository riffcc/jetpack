use jetpack::registry::list::*;
use jetpack::tasks::*;

#[test]
fn test_task_enum_variant_access_modules() {
    // Test Group task variant
    let group_yaml = r#"
user: testgroup
gid: "1001"
"#;
    let group_task: Result<Task, _> = serde_yaml::from_str(group_yaml);
    assert\!(group_task.is_err()); // Should fail without module field
    
    // Test with module field
    let group_yaml_with_module = r#"
\!group
group: testgroup
gid: "1001"
"#;
    let group_task: Result<Task, _> = serde_yaml::from_str(group_yaml_with_module);
    assert\!(group_task.is_ok());
    match group_task.unwrap() {
        Task::Group(_) => assert\!(true),
        _ => panic\!("Expected Group task"),
    }
}

#[test]
fn test_task_enum_variant_command_modules() {
    let shell_yaml = r#"
\!shell
cmd: echo "Hello World"
"#;
    let shell_task: Result<Task, _> = serde_yaml::from_str(shell_yaml);
    assert\!(shell_task.is_ok());
    match shell_task.unwrap() {
        Task::Shell(_) => assert\!(true),
        _ => panic\!("Expected Shell task"),
    }
}

#[test]
fn test_task_enum_variant_control_modules() {
    // Test Echo task
    let echo_yaml = r#"
\!echo
msg: Test message
"#;
    let echo_task: Result<Task, _> = serde_yaml::from_str(echo_yaml);
    assert\!(echo_task.is_ok());
    match echo_task.unwrap() {
        Task::Echo(_) => assert\!(true),
        _ => panic\!("Expected Echo task"),
    }
    
    // Test Debug task
    let debug_yaml = r#"
\!debug
vars: some_var
"#;
    let debug_task: Result<Task, _> = serde_yaml::from_str(debug_yaml);
    assert\!(debug_task.is_ok());
    match debug_task.unwrap() {
        Task::Debug(_) => assert\!(true),
        _ => panic\!("Expected Debug task"),
    }
}

#[test]
fn test_task_enum_variant_file_modules() {
    // Test File task
    let file_yaml = r#"
\!file
path: /tmp/test.txt
"#;
    let file_task: Result<Task, _> = serde_yaml::from_str(file_yaml);
    assert\!(file_task.is_ok());
    match file_task.unwrap() {
        Task::File(_) => assert\!(true),
        _ => panic\!("Expected File task"),
    }
    
    // Test Copy task
    let copy_yaml = r#"
\!copy
src: source.txt
dest: /tmp/dest.txt
"#;
    let copy_task: Result<Task, _> = serde_yaml::from_str(copy_yaml);
    assert\!(copy_task.is_ok());
    match copy_task.unwrap() {
        Task::Copy(_) => assert\!(true),
        _ => panic\!("Expected Copy task"),
    }
}

#[test]
fn test_task_enum_variant_package_modules() {
    // Test Apt task
    let apt_yaml = r#"
\!apt
package: nginx
"#;
    let apt_task: Result<Task, _> = serde_yaml::from_str(apt_yaml);
    assert\!(apt_task.is_ok());
    match apt_task.unwrap() {
        Task::Apt(_) => assert\!(true),
        _ => panic\!("Expected Apt task"),
    }
    
    // Test Homebrew task
    let brew_yaml = r#"
\!homebrew
package: wget
"#;
    let brew_task: Result<Task, _> = serde_yaml::from_str(brew_yaml);
    assert\!(brew_task.is_ok());
    match brew_task.unwrap() {
        Task::Homebrew(_) => assert\!(true),
        _ => panic\!("Expected Homebrew task"),
    }
}

#[test]
fn test_task_enum_variant_service_modules() {
    let service_yaml = r#"
\!sd_service
service: nginx
enabled: "yes"
"#;
    let service_task: Result<Task, _> = serde_yaml::from_str(service_yaml);
    assert\!(service_task.is_ok());
    match service_task.unwrap() {
        Task::SystemdService(_) => assert\!(true),
        _ => panic\!("Expected SystemdService task"),
    }
}

#[test]
fn test_task_enum_get_module() {
    let echo_yaml = r#"
\!echo
msg: Test
"#;
    let task: Task = serde_yaml::from_str(echo_yaml).unwrap();
    assert_eq\!(task.get_module(), "echo");
    
    let file_yaml = r#"
\!file
path: /tmp/test
"#;
    let task: Task = serde_yaml::from_str(file_yaml).unwrap();
    assert_eq\!(task.get_module(), "file");
}

#[test]
fn test_task_enum_get_name() {
    let echo_yaml = r#"
\!echo
name: Test Echo Task
msg: Hello
"#;
    let task: Task = serde_yaml::from_str(echo_yaml).unwrap();
    assert_eq\!(task.get_name(), Some("Test Echo Task".to_string()));
    
    let file_yaml = r#"
\!file
path: /tmp/test
"#;
    let task: Task = serde_yaml::from_str(file_yaml).unwrap();
    assert_eq\!(task.get_name(), None);
}

#[test]
fn test_task_list_deserialization() {
    let tasks_yaml = r#"
- \!echo
  msg: First message
- \!file
  path: /tmp/test.txt
- \!shell
  cmd: ls -la
"#;
    
    let tasks: Result<Vec<Task>, _> = serde_yaml::from_str(tasks_yaml);
    assert\!(tasks.is_ok());
    
    let tasks = tasks.unwrap();
    assert_eq\!(tasks.len(), 3);
    
    // Verify task types
    match &tasks[0] {
        Task::Echo(_) => assert\!(true),
        _ => panic\!("Expected Echo task"),
    }
    match &tasks[1] {
        Task::File(_) => assert\!(true),
        _ => panic\!("Expected File task"),
    }
    match &tasks[2] {
        Task::Shell(_) => assert\!(true),
        _ => panic\!("Expected Shell task"),
    }
}
EOF < /dev/null