use jetpack::connection::local::*;
use jetpack::connection::connection::Connection;
use jetpack::connection::factory::ConnectionFactory;
use jetpack::inventory::inventory::Inventory;
use jetpack::inventory::hosts::Host;
use jetpack::playbooks::context::PlaybookContext;
use jetpack::cli::parser::CliParser;
use std::sync::{Arc, RwLock, Mutex};

fn create_test_inventory() -> Arc<RwLock<Inventory>> {
    let mut inventory = Inventory::new();
    let localhost_name = "localhost".to_string();
    let localhost = Arc::new(RwLock::new(Host::new(&localhost_name)));
    inventory.add_host(localhost);
    Arc::new(RwLock::new(inventory))
}

#[test]
fn test_local_factory_new() {
    let inventory = create_test_inventory();
    let factory = LocalFactory::new(&inventory);
    
    // Verify factory was created successfully
    let parser = CliParser::new();
    let context = Arc::new(RwLock::new(PlaybookContext::new(&parser)));
    let localhost_name = "localhost".to_string();
    let localhost = Arc::new(RwLock::new(Host::new(&localhost_name)));
    
    let conn_result = factory.get_connection(&context, &localhost);
    assert!(conn_result.is_ok());
}

#[test]
fn test_local_factory_get_local_connection() {
    let inventory = create_test_inventory();
    let factory = LocalFactory::new(&inventory);
    
    let parser = CliParser::new();
    let context = Arc::new(RwLock::new(PlaybookContext::new(&parser)));
    
    let conn_result = factory.get_local_connection(&context);
    assert!(conn_result.is_ok());
}

#[test]
fn test_local_connection_new() {
    let hostname = "testhost".to_string();
    let host = Arc::new(RwLock::new(Host::new(&hostname)));
    let conn = LocalConnection::new(&host);
    
    // Verify connection was created (we can't do much more without trait access)
    // Just ensure it doesn't panic
}

#[test]
fn test_convert_out() {
    let stdout = b"Hello World";
    let stderr = b"Error message";
    
    let result = convert_out(&stdout.to_vec(), &stderr.to_vec());
    assert_eq!(result, "Hello World\nError message");
}

#[test]
fn test_convert_out_empty() {
    let stdout = b"";
    let stderr = b"";
    
    let result = convert_out(&stdout.to_vec(), &stderr.to_vec());
    assert_eq!(result, "");
}

#[test]
fn test_convert_out_invalid_utf8() {
    let stdout = vec![0xFF, 0xFE]; // Invalid UTF-8
    let stderr = b"Valid error";
    
    let result = convert_out(&stdout, &stderr.to_vec());
    assert!(result.contains("invalid UTF-8"));
}

#[test]
fn test_convert_out_whitespace_trimming() {
    let stdout = b"  Hello World  \n";
    let stderr = b"  Error  ";
    
    let result = convert_out(&stdout.to_vec(), &stderr.to_vec());
    assert_eq!(result, "Hello World  \n\n  Error");
}

#[test]
fn test_local_connection_whoami() {
    let hostname = "localhost".to_string();
    let host = Arc::new(RwLock::new(Host::new(&hostname)));
    let conn = LocalConnection::new(&host);
    
    // This should get the current user from $USER env var
    let result = conn.whoami();
    assert!(result.is_ok());
    
    // The result should be a non-empty string
    let username = result.unwrap();
    assert!(!username.is_empty());
}

#[test]
fn test_local_connection_connect() {
    let hostname = "localhost".to_string();
    let host = Arc::new(RwLock::new(Host::new(&hostname)));
    let mut conn = LocalConnection::new(&host);
    
    // Connect should succeed for localhost
    let result = conn.connect();
    assert!(result.is_ok());
}