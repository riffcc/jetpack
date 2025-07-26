use jetpack::connection::local::LocalConnection;
use jetpack::connection::connection::Connection;
use jetpack::inventory::hosts::Host;
use jetpack::tasks::request::{TaskRequest, SudoDetails};
use jetpack::handle::response::Response;
use jetpack::connection::command::Forward;
use std::sync::{Arc, RwLock};
use std::fs;
use std::env;

fn create_test_host() -> Arc<RwLock<Host>> {
    let hostname = "localhost".to_string();
    Arc::new(RwLock::new(Host::new(&hostname)))
}

#[test]
fn test_local_connection_real_whoami() {
    let host = create_test_host();
    let conn = LocalConnection::new(&host);
    
    let result = conn.whoami();
    assert!(result.is_ok());
    
    let username = result.unwrap();
    // Should match the actual system username
    let expected_username = env::var("USER")
        .or_else(|_| env::var("USERNAME"))
        .unwrap_or_else(|_| "unknown".to_string());
    assert_eq!(username, expected_username);
}

#[test]
fn test_local_connection_real_run_command() {
    let host = create_test_host();
    let conn = LocalConnection::new(&host);
    
    let run_state = crate::common::create_test_run_state();
    let response = Arc::new(Response::new(run_state, host.clone()));
    let sudo_details = jetpack::tasks::request::SudoDetails {
        user: None,
        template: "test".to_string(),
    };
    let request = TaskRequest::query(&sudo_details);
    
    // Test a simple echo command
    let cmd = "echo 'test output'".to_string();
    let result = conn.run_command(&response, &request, &cmd, Forward::No);
    
    assert!(result.is_ok());
    let task_response = result.unwrap();
    
    // Check that it executed successfully
    match task_response.status {
        jetpack::tasks::response::TaskStatus::IsExecuted => {},
        _ => panic!("Expected IsExecuted status"),
    }
    
    let cmd_result = task_response.command_result.as_ref().as_ref().unwrap();
    assert_eq!(cmd_result.rc, 0);
    assert!(cmd_result.out.contains("test output"));
}

#[test]
fn test_local_connection_real_run_command_error() {
    let host = create_test_host();
    let conn = LocalConnection::new(&host);
    
    let run_state = crate::common::create_test_run_state();
    let response = Arc::new(Response::new(run_state, host.clone()));
    let sudo_details = jetpack::tasks::request::SudoDetails {
        user: None,
        template: "test".to_string(),
    };
    let request = TaskRequest::query(&sudo_details);
    
    // Test a command that fails
    let cmd = "ls /nonexistent/directory/that/does/not/exist".to_string();
    let result = conn.run_command(&response, &request, &cmd, Forward::No);
    
    assert!(result.is_err());
}

#[test]
fn test_local_connection_real_write_data() {
    let host = create_test_host();
    let conn = LocalConnection::new(&host);
    
    let run_state = crate::common::create_test_run_state();
    let response = Arc::new(Response::new(run_state, host.clone()));
    let sudo_details = jetpack::tasks::request::SudoDetails {
        user: None,
        template: "test".to_string(),
    };
    let request = TaskRequest::query(&sudo_details);
    
    // Create a temporary file path
    let temp_dir = env::temp_dir();
    let temp_file = temp_dir.join("jetpack_test_write.txt");
    let remote_path = temp_file.to_str().unwrap().to_string();
    
    // Write some data
    let data = "Test data for write_data method".to_string();
    let result = conn.write_data(&response, &request, &data, &remote_path);
    
    assert!(result.is_ok());
    
    // Verify the file was created and contains the data
    assert!(temp_file.exists());
    let content = fs::read_to_string(&temp_file).unwrap();
    assert_eq!(content, data);
    
    // Clean up
    fs::remove_file(&temp_file).unwrap();
}

#[test]
fn test_local_connection_real_copy_file() {
    let host = create_test_host();
    let conn = LocalConnection::new(&host);
    
    let run_state = crate::common::create_test_run_state();
    let response = Arc::new(Response::new(run_state, host.clone()));
    let sudo_details = jetpack::tasks::request::SudoDetails {
        user: None,
        template: "test".to_string(),
    };
    let request = TaskRequest::query(&sudo_details);
    
    // Create a source file
    let temp_dir = env::temp_dir();
    let src_file = temp_dir.join("jetpack_test_src.txt");
    let dst_file = temp_dir.join("jetpack_test_dst.txt");
    
    // Write some data to source file
    let data = "Test data for copy_file method";
    fs::write(&src_file, data).unwrap();
    
    // Copy the file
    let remote_path = dst_file.to_str().unwrap().to_string();
    let result = conn.copy_file(&response, &request, &src_file, &remote_path);
    
    assert!(result.is_ok());
    
    // Verify the file was copied
    assert!(dst_file.exists());
    let content = fs::read_to_string(&dst_file).unwrap();
    assert_eq!(content, data);
    
    // Clean up
    fs::remove_file(&src_file).unwrap();
    fs::remove_file(&dst_file).unwrap();
}

#[test]
fn test_local_connection_real_command_with_env_vars() {
    let host = create_test_host();
    let conn = LocalConnection::new(&host);
    
    let run_state = crate::common::create_test_run_state();
    let response = Arc::new(Response::new(run_state, host.clone()));
    let sudo_details = jetpack::tasks::request::SudoDetails {
        user: None,
        template: "test".to_string(),
    };
    let request = TaskRequest::query(&sudo_details);
    
    // Test a command that uses environment variables
    let cmd = "echo $HOME".to_string();
    let result = conn.run_command(&response, &request, &cmd, Forward::No);
    
    assert!(result.is_ok());
    let task_response = result.unwrap();
    let cmd_result = task_response.command_result.as_ref().as_ref().unwrap();
    assert_eq!(cmd_result.rc, 0);
    assert!(!cmd_result.out.trim().is_empty());
}

#[test]
fn test_local_connection_real_command_with_pipes() {
    let host = create_test_host();
    let conn = LocalConnection::new(&host);
    
    let run_state = crate::common::create_test_run_state();
    let response = Arc::new(Response::new(run_state, host.clone()));
    let sudo_details = jetpack::tasks::request::SudoDetails {
        user: None,
        template: "test".to_string(),
    };
    let request = TaskRequest::query(&sudo_details);
    
    // Test a command with pipes
    let cmd = "echo 'line1\nline2\nline3' | grep line2".to_string();
    let result = conn.run_command(&response, &request, &cmd, Forward::No);
    
    assert!(result.is_ok());
    let task_response = result.unwrap();
    let cmd_result = task_response.command_result.as_ref().as_ref().unwrap();
    assert_eq!(cmd_result.rc, 0);
    assert!(cmd_result.out.contains("line2"));
    assert!(!cmd_result.out.contains("line1"));
    assert!(!cmd_result.out.contains("line3"));
}

#[test]
#[cfg(unix)]
fn test_local_connection_real_permissions() {
    use std::os::unix::fs::PermissionsExt;
    
    let host = create_test_host();
    let conn = LocalConnection::new(&host);
    
    let run_state = crate::common::create_test_run_state();
    let response = Arc::new(Response::new(run_state, host.clone()));
    let sudo_details = jetpack::tasks::request::SudoDetails {
        user: None,
        template: "test".to_string(),
    };
    let request = TaskRequest::query(&sudo_details);
    
    // Create a file with specific permissions
    let temp_dir = env::temp_dir();
    let temp_file = temp_dir.join("jetpack_test_perms.txt");
    let remote_path = temp_file.to_str().unwrap().to_string();
    
    // Write data
    let data = "Test permissions".to_string();
    conn.write_data(&response, &request, &data, &remote_path).unwrap();
    
    // Change permissions using command
    let chmod_cmd = format!("chmod 644 {}", remote_path);
    let result = conn.run_command(&response, &request, &chmod_cmd, Forward::No);
    assert!(result.is_ok());
    
    // Verify permissions
    let metadata = fs::metadata(&temp_file).unwrap();
    let permissions = metadata.permissions();
    let mode = permissions.mode();
    
    // Check that the file has 644 permissions (owner: rw-, group: r--, others: r--)
    assert_eq!(mode & 0o777, 0o644);
    
    // Clean up
    fs::remove_file(&temp_file).unwrap();
}