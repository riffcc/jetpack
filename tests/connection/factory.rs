use jetpack::connection::factory::*;
use jetpack::connection::connection::Connection;
use jetpack::connection::command::{CommandResult, Forward};
use jetpack::inventory::hosts::Host;
use jetpack::playbooks::context::PlaybookContext;
use jetpack::tasks::{TaskRequest, TaskResponse};
use jetpack::handle::response::Response;
use jetpack::inventory::inventory::Inventory;
use jetpack::playbooks::traversal::RunState;
use jetpack::playbooks::visitor::{PlaybookVisitor, CheckMode};
use jetpack::cli::parser::CliParser;
use std::sync::{Arc, RwLock, Mutex};
use std::path::Path;

// Mock connection for testing
struct MockConnection {
    host: Arc<RwLock<Host>>,
}

impl Connection for MockConnection {
    fn connect(&mut self) -> Result<(), String> {
        Ok(())
    }

    fn write_data(&self, _response: &Arc<Response>, _request: &Arc<TaskRequest>, _data: &String, _remote_path: &String) -> Result<(), Arc<TaskResponse>> {
        Ok(())
    }

    fn copy_file(&self, _response: &Arc<Response>, _request: &Arc<TaskRequest>, _src: &Path, _dest: &String) -> Result<(), Arc<TaskResponse>> {
        Ok(())
    }

    fn whoami(&self) -> Result<String, String> {
        Ok("mockuser".to_string())
    }

    fn run_command(&self, response: &Arc<Response>, request: &Arc<TaskRequest>, cmd: &String, _forward: Forward) -> Result<Arc<TaskResponse>, Arc<TaskResponse>> {
        Ok(response.command_ok(request, &Arc::new(Some(CommandResult {
            cmd: cmd.clone(),
            out: "Mock output".to_string(),
            rc: 0,
        }))))
    }
}

// Mock factory for testing
struct MockFactory {
    factory_name: String,
}

impl ConnectionFactory for MockFactory {
    fn get_connection(&self, _context: &Arc<RwLock<PlaybookContext>>, host: &Arc<RwLock<Host>>) -> Result<Arc<Mutex<dyn Connection>>, String> {
        let conn = MockConnection {
            host: Arc::clone(host),
        };
        Ok(Arc::new(Mutex::new(conn)))
    }
    
    fn get_local_connection(&self, _context: &Arc<RwLock<PlaybookContext>>) -> Result<Arc<Mutex<dyn Connection>>, String> {
        let hostname = "localhost".to_string();
        let host = Arc::new(RwLock::new(Host::new(&hostname)));
        let conn = MockConnection {
            host: Arc::clone(&host),
        };
        Ok(Arc::new(Mutex::new(conn)))
    }
}

fn create_test_context() -> Arc<RwLock<PlaybookContext>> {
    let parser = CliParser::new();
    Arc::new(RwLock::new(PlaybookContext::new(&parser)))
}

#[test]
fn test_mock_factory_get_connection() {
    let factory = MockFactory {
        factory_name: "test_factory".to_string(),
    };
    
    let context = create_test_context();
    let hostname = "testhost".to_string();
    let host = Arc::new(RwLock::new(Host::new(&hostname)));
    
    let connection = factory.get_connection(&context, &host);
    assert!(connection.is_ok());
}

#[test]
fn test_mock_factory_get_local_connection() {
    let factory = MockFactory {
        factory_name: "test_factory".to_string(),
    };
    
    let context = create_test_context();
    
    let connection = factory.get_local_connection(&context);
    assert!(connection.is_ok());
}

#[test]
fn test_factory_creates_working_connection() {
    let factory = MockFactory {
        factory_name: "test_factory".to_string(),
    };
    
    let context = create_test_context();
    let hostname = "testhost".to_string();
    let host = Arc::new(RwLock::new(Host::new(&hostname)));
    
    let connection = factory.get_connection(&context, &host).unwrap();
    
    // Test that the connection works
    let mut conn = connection.lock().unwrap();
    assert!(conn.connect().is_ok());
    
    // Test whoami
    let username = conn.whoami();
    assert!(username.is_ok());
    assert_eq!(username.unwrap(), "mockuser");
}

#[test]
fn test_factory_local_connection_works() {
    let factory = MockFactory {
        factory_name: "test_factory".to_string(),
    };
    
    let context = create_test_context();
    
    let connection = factory.get_local_connection(&context).unwrap();
    
    // Test that the connection works
    let mut conn = connection.lock().unwrap();
    assert!(conn.connect().is_ok());
    
    // Test whoami
    let username = conn.whoami();
    assert!(username.is_ok());
    assert_eq!(username.unwrap(), "mockuser");
}