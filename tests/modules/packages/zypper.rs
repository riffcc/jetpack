use jetpack::modules::packages::zypper::*;
use jetpack::tasks::*;
use jetpack::handle::handle::TaskHandle;
use jetpack::inventory::hosts::Host;
use jetpack::connection::connection::Connection;
use jetpack::connection::command::{CommandResult, Forward};
use jetpack::handle::response::Response;
use std::sync::{Arc, RwLock, Mutex};
use std::path::Path;

// Mock connection for testing
struct MockConnection;

impl Connection for MockConnection {
    fn whoami(&self) -> Result<String, String> {
        Ok("mockuser".to_string())
    }
    
    fn connect(&mut self) -> Result<(), String> {
        Ok(())
    }
    
    fn run_command(&self, response: &Arc<Response>, request: &Arc<TaskRequest>, cmd: &String, _forward: Forward) -> Result<Arc<TaskResponse>, Arc<TaskResponse>> {
        // Mock different command outputs based on the command
        if cmd.contains("zypper --non-interactive --quiet search --match-exact --details") {
            let output = if cmd.contains("vim") {
                "S  | Name | Summary                                   | Type    \n---+------+-------------------------------------------+---------\ni+ | vim  | Vi IMproved                               | package \n"
            } else {
                ""
            };
            Ok(response.command_ok(request, &Arc::new(Some(CommandResult {
                cmd: cmd.clone(),
                out: output.to_string(),
                rc: if output.is_empty() { 104 } else { 0 },
            }))))
        } else if cmd.contains("rpm -q") {
            let output = if cmd.contains("vim") && !cmd.contains("grep") {
                "vim-8.2.5226-1.el8.x86_64"
            } else {
                "package vim is not installed"
            };
            Ok(response.command_ok(request, &Arc::new(Some(CommandResult {
                cmd: cmd.clone(),
                out: output.to_string(),
                rc: if output.contains("not installed") { 1 } else { 0 },
            }))))
        } else {
            Ok(response.command_ok(request, &Arc::new(Some(CommandResult {
                cmd: cmd.clone(),
                out: "mock output".to_string(),
                rc: 0,
            }))))
        }
    }
    
    fn copy_file(&self, _response: &Arc<Response>, _request: &Arc<TaskRequest>, _src: &Path, _remote_path: &String) -> Result<(), Arc<TaskResponse>> {
        Ok(())
    }
    
    fn write_data(&self, _response: &Arc<Response>, _request: &Arc<TaskRequest>, _data: &String, _remote_path: &String) -> Result<(), Arc<TaskResponse>> {
        Ok(())
    }
}

fn create_test_handle() -> Arc<TaskHandle> {
    let run_state = crate::common::create_test_run_state();
    let hostname = "testhost".to_string();
    let host = Arc::new(RwLock::new(Host::new(&hostname)));
    let connection: Arc<Mutex<dyn Connection>> = Arc::new(Mutex::new(MockConnection));
    
    Arc::new(TaskHandle::new(run_state, connection, host))
}

#[test]
fn test_zypper_task_deserialize() {
    let yaml = r#"
package: vim
"#;
    let task: Result<ZypperTask, _> = serde_yaml::from_str(yaml);
    assert!(task.is_ok());
    let task = task.unwrap();
    assert_eq!(task.package, "vim");
    assert!(task.version.is_none());
    assert!(task.update.is_none());
    assert!(task.remove.is_none());
}

#[test]
fn test_zypper_task_deserialize_with_version() {
    let yaml = r#"
package: vim
version: "8.2.5226"
"#;
    let task: Result<ZypperTask, _> = serde_yaml::from_str(yaml);
    assert!(task.is_ok());
    let task = task.unwrap();
    assert_eq!(task.package, "vim");
    assert_eq!(task.version, Some("8.2.5226".to_string()));
}

#[test]
fn test_zypper_task_deserialize_with_flags() {
    let yaml = r#"
package: vim
update: true
remove: false
"#;
    let task: Result<ZypperTask, _> = serde_yaml::from_str(yaml);
    assert!(task.is_ok());
    let task = task.unwrap();
    assert_eq!(task.package, "vim");
    assert_eq!(task.update, Some("true".to_string()));
    assert_eq!(task.remove, Some("false".to_string()));
}

#[test]
fn test_zypper_task_deserialize_with_name() {
    let yaml = r#"
name: Install vim editor
package: vim
"#;
    let task: Result<ZypperTask, _> = serde_yaml::from_str(yaml);
    assert!(task.is_ok());
    let task = task.unwrap();
    assert_eq!(task.name, Some("Install vim editor".to_string()));
    assert_eq!(task.package, "vim");
}

#[test]
fn test_zypper_task_get_module() {
    let task = ZypperTask {
        name: None,
        package: "vim".to_string(),
        version: None,
        update: None,
        remove: None,
        with: None,
        and: None,
    };
    assert_eq!(task.get_module(), "zypper");
}

#[test]
fn test_zypper_task_get_name() {
    let task = ZypperTask {
        name: Some("Install vim editor".to_string()),
        package: "vim".to_string(),
        version: None,
        update: None,
        remove: None,
        with: None,
        and: None,
    };
    assert_eq!(task.get_name(), Some("Install vim editor".to_string()));
}

#[test]
fn test_zypper_task_get_with() {
    let task = ZypperTask {
        name: None,
        package: "vim".to_string(),
        version: None,
        update: None,
        remove: None,
        with: None,
        and: None,
    };
    assert!(task.get_with().is_none());
}

#[test]
fn test_zypper_task_evaluate() {
    let task = ZypperTask {
        name: Some("Install vim".to_string()),
        package: "vim".to_string(),
        version: Some("8.2.5226".to_string()),
        update: Some("false".to_string()),
        remove: Some("false".to_string()),
        with: None,
        and: None,
    };
    
    let handle = create_test_handle();
    let request = TaskRequest::query("zypper".to_string());
    
    let result = task.evaluate(&handle, &request, TemplateMode::Strict);
    assert!(result.is_ok());
    
    let evaluated = result.unwrap();
    // We can't easily test the action content without making it public,
    // but we can verify the evaluation succeeded
}