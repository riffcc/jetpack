use jetpack::modules::packages::pacman::*;
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
        if cmd.contains("pacman -Q --info") {
            let output = if cmd.contains("vim") {
                "Name            : vim\nVersion         : 9.0.1234-1\nDescription     : Vi Improved, a highly configurable text editor\n"
            } else {
                ""
            };
            Ok(response.command_ok(request, &Arc::new(Some(CommandResult {
                cmd: cmd.clone(),
                out: output.to_string(),
                rc: if output.is_empty() { 1 } else { 0 },
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
fn test_pacman_task_deserialize() {
    let yaml = r#"
package: vim
"#;
    let task: Result<PacmanTask, _> = serde_yaml::from_str(yaml);
    assert!(task.is_ok());
    let task = task.unwrap();
    assert_eq!(task.package, "vim");
    assert!(task.version.is_none());
    assert!(task.update.is_none());
    assert!(task.remove.is_none());
}

#[test]
fn test_pacman_task_deserialize_with_version() {
    let yaml = r#"
package: vim
version: "9.0.1234-1"
"#;
    let task: Result<PacmanTask, _> = serde_yaml::from_str(yaml);
    assert!(task.is_ok());
    let task = task.unwrap();
    assert_eq!(task.package, "vim");
    assert_eq!(task.version, Some("9.0.1234-1".to_string()));
}

#[test]
fn test_pacman_task_deserialize_with_flags() {
    let yaml = r#"
package: vim
update: true
remove: false
"#;
    let task: Result<PacmanTask, _> = serde_yaml::from_str(yaml);
    assert!(task.is_ok());
    let task = task.unwrap();
    assert_eq!(task.package, "vim");
    assert_eq!(task.update, Some("true".to_string()));
    assert_eq!(task.remove, Some("false".to_string()));
}

#[test]
fn test_pacman_task_get_module() {
    let task = PacmanTask {
        name: None,
        package: "vim".to_string(),
        version: None,
        update: None,
        remove: None,
        with: None,
        and: None,
    };
    assert_eq!(task.get_module(), "pacman");
}

#[test]
fn test_pacman_task_get_name() {
    let task = PacmanTask {
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
fn test_pacman_action_get_actual_package() {
    let action = PacmanAction {
        package: "vim".to_string(),
        version: None,
        update: false,
        remove: false,
    };
    assert_eq!(action.get_actual_package(), "vim");
}

#[test]
fn test_pacman_action_get_actual_package_with_repo() {
    let action = PacmanAction {
        package: "extra/vim".to_string(),
        version: None,
        update: false,
        remove: false,
    };
    assert_eq!(action.get_actual_package(), "vim");
}

#[test]
fn test_pacman_action_get_actual_package_multiple_slashes() {
    let action = PacmanAction {
        package: "community/extra/vim".to_string(),
        version: None,
        update: false,
        remove: false,
    };
    assert_eq!(action.get_actual_package(), "vim");
}

#[test]
fn test_pacman_action_is_update() {
    let action = PacmanAction {
        package: "vim".to_string(),
        version: None,
        update: true,
        remove: false,
    };
    assert!(action.is_update());
}

#[test]
fn test_pacman_action_is_remove() {
    let action = PacmanAction {
        package: "vim".to_string(),
        version: None,
        update: false,
        remove: true,
    };
    assert!(action.is_remove());
}

#[test]
fn test_pacman_action_get_version() {
    let action = PacmanAction {
        package: "vim".to_string(),
        version: Some("9.0.1234-1".to_string()),
        update: false,
        remove: false,
    };
    assert_eq!(action.get_version(), Some("9.0.1234-1".to_string()));
}

#[test]
fn test_pacman_action_parse_package_details() {
    let action = PacmanAction {
        package: "vim".to_string(),
        version: None,
        update: false,
        remove: false,
    };
    
    let output = "Name            : vim\nVersion         : 9.0.1234-1\nDescription     : Vi Improved, a highly configurable text editor\n";
    let details = action.parse_package_details(&output.to_string());
    
    assert!(details.is_some());
    let details = details.unwrap();
    assert_eq!(details.installed, true);
    assert_eq!(details.version, Some("9.0.1234-1".to_string()));
}

#[test]
fn test_pacman_action_parse_package_details_not_found() {
    let action = PacmanAction {
        package: "notfound".to_string(),
        version: None,
        update: false,
        remove: false,
    };
    
    let output = "";
    let details = action.parse_package_details(&output.to_string());
    
    assert!(details.is_none());
}