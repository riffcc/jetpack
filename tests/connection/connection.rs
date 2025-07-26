use jetpack::connection::connection::*;
use jetpack::connection::command::{CommandResult, Forward};
use jetpack::tasks::{TaskRequest, TaskResponse};
use jetpack::handle::response::Response;
use jetpack::inventory::hosts::Host;
use std::sync::{Arc, RwLock};
use std::path::Path;

// Mock connection implementation for testing
struct MockConnection {
    username: String,
    connected: std::cell::RefCell<bool>,
}

impl Connection for MockConnection {
    fn connect(&mut self) -> Result<(), String> {
        *self.connected.borrow_mut() = true;
        Ok(())
    }

    fn write_data(&self, _response: &Arc<Response>, _request: &Arc<TaskRequest>, _data: &String, _remote_path: &String) -> Result<(), Arc<TaskResponse>> {
        Ok(())
    }

    fn copy_file(&self, _response: &Arc<Response>, _request: &Arc<TaskRequest>, _src: &Path, _dest: &String) -> Result<(), Arc<TaskResponse>> {
        Ok(())
    }

    fn whoami(&self) -> Result<String, String> {
        Ok(self.username.clone())
    }

    fn run_command(&self, response: &Arc<Response>, request: &Arc<TaskRequest>, cmd: &String, _forward: Forward) -> Result<Arc<TaskResponse>, Arc<TaskResponse>> {
        Ok(response.command_ok(request, &Arc::new(Some(CommandResult {
            cmd: cmd.clone(),
            out: format!("Mock output for command: {}", cmd),
            rc: 0,
        }))))
    }
}

fn create_test_response() -> Arc<Response> {
    let run_state = crate::common::create_test_run_state();
    let hostname = "testhost".to_string();
    let host = Arc::new(RwLock::new(Host::new(&hostname)));
    
    Arc::new(Response::new(run_state, host))
}

#[test]
fn test_mock_connection_whoami() {
    let conn = MockConnection {
        username: "testuser".to_string(),
        connected: std::cell::RefCell::new(false),
    };
    
    assert_eq!(conn.whoami().unwrap(), "testuser");
}

#[test]
fn test_mock_connection_connect() {
    let mut conn = MockConnection {
        username: "testuser".to_string(),
        connected: std::cell::RefCell::new(false),
    };
    
    assert_eq!(*conn.connected.borrow(), false);
    assert!(conn.connect().is_ok());
    assert_eq!(*conn.connected.borrow(), true);
}

#[test]
fn test_mock_connection_write_data() {
    let conn = MockConnection {
        username: "testuser".to_string(),
        connected: std::cell::RefCell::new(true),
    };
    
    let response = create_test_response();
    let request = TaskRequest::query("test_module".to_string());
    let data = "test data".to_string();
    let remote_path = "/tmp/test.txt".to_string();
    
    assert!(conn.write_data(&response, &request, &data, &remote_path).is_ok());
}

#[test]
fn test_mock_connection_copy_file() {
    let conn = MockConnection {
        username: "testuser".to_string(),
        connected: std::cell::RefCell::new(true),
    };
    
    let response = create_test_response();
    let request = TaskRequest::query("test_module".to_string());
    let src = Path::new("/tmp/source.txt");
    let dest = "/tmp/dest.txt".to_string();
    
    assert!(conn.copy_file(&response, &request, &src, &dest).is_ok());
}

#[test]
fn test_mock_connection_run_command() {
    let conn = MockConnection {
        username: "testuser".to_string(),
        connected: std::cell::RefCell::new(true),
    };
    
    let response = create_test_response();
    let request = TaskRequest::query("test_module".to_string());
    let cmd = "echo hello".to_string();
    
    let result = conn.run_command(&response, &request, &cmd, Forward::No);
    assert!(result.is_ok());
}