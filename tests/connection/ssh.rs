use jetpack::connection::ssh::*;
use jetpack::connection::factory::ConnectionFactory;
use jetpack::inventory::inventory::Inventory;
use jetpack::inventory::hosts::Host;
use jetpack::playbooks::context::PlaybookContext;
use jetpack::cli::parser::CliParser;
use std::sync::{Arc, RwLock};

fn create_test_inventory_with_localhost() -> Arc<RwLock<Inventory>> {
    let mut inventory = Inventory::new();
    let localhost_name = "localhost".to_string();
    let localhost = Arc::new(RwLock::new(Host::new(&localhost_name)));
    inventory.add_host(localhost);
    Arc::new(RwLock::new(inventory))
}

fn create_test_inventory_with_remote_host() -> Arc<RwLock<Inventory>> {
    let mut inventory = Inventory::new();
    
    // Add localhost as required by SshFactory
    let localhost_name = "localhost".to_string();
    let localhost = Arc::new(RwLock::new(Host::new(&localhost_name)));
    inventory.add_host(localhost);
    
    // Add a remote host
    let remote_name = "remote-host".to_string();
    let remote_host = Arc::new(RwLock::new(Host::new(&remote_name)));
    inventory.add_host(remote_host);
    
    Arc::new(RwLock::new(inventory))
}

#[test]
fn test_ssh_factory_new() {
    let inventory = create_test_inventory_with_localhost();
    let factory = SshFactory::new(&inventory, false, None);
    
    // Verify factory was created successfully by getting local connection
    let parser = CliParser::new();
    let context = Arc::new(RwLock::new(PlaybookContext::new(&parser)));
    
    let conn_result = factory.get_local_connection(&context);
    assert!(conn_result.is_ok());
}

#[test]
fn test_ssh_factory_new_with_forward_agent() {
    let inventory = create_test_inventory_with_localhost();
    let factory = SshFactory::new(&inventory, true, None);
    
    // Verify factory was created with forward agent enabled
    let parser = CliParser::new();
    let context = Arc::new(RwLock::new(PlaybookContext::new(&parser)));
    
    let conn_result = factory.get_local_connection(&context);
    assert!(conn_result.is_ok());
}

#[test]
fn test_ssh_factory_new_with_password() {
    let inventory = create_test_inventory_with_localhost();
    let password = Some("test_password".to_string());
    let factory = SshFactory::new(&inventory, false, password);
    
    // Verify factory was created with password
    let parser = CliParser::new();
    let context = Arc::new(RwLock::new(PlaybookContext::new(&parser)));
    
    let conn_result = factory.get_local_connection(&context);
    assert!(conn_result.is_ok());
}

#[test]
fn test_ssh_factory_get_connection_localhost() {
    let inventory = create_test_inventory_with_localhost();
    let factory = SshFactory::new(&inventory, false, None);
    
    let parser = CliParser::new();
    let context = Arc::new(RwLock::new(PlaybookContext::new(&parser)));
    let localhost_name = "localhost".to_string();
    let localhost = Arc::new(RwLock::new(Host::new(&localhost_name)));
    
    // Getting connection for localhost should return local connection
    let conn_result = factory.get_connection(&context, &localhost);
    assert!(conn_result.is_ok());
}

#[test]
fn test_ssh_connection_new() {
    let hostname = "test-server".to_string();
    let host = Arc::new(RwLock::new(Host::new(&hostname)));
    let username = "testuser".to_string();
    let port = 22;
    let forward_agent = false;
    let login_password = None;
    let key = None;
    let passphrase = None;
    let key_comment = None;
    
    let conn = SshConnection::new(
        host,
        &username,
        port,
        hostname.clone(),
        forward_agent,
        login_password,
        key,
        passphrase,
        key_comment
    );
    
    assert_eq!(conn.username, "testuser");
    assert_eq!(conn.port, 22);
    assert_eq!(conn.hostname, "test-server");
    assert_eq!(conn.forward_agent, false);
    assert!(conn.session.is_none());
}

#[test]
fn test_ssh_connection_new_with_options() {
    let hostname = "test-server".to_string();
    let host = Arc::new(RwLock::new(Host::new(&hostname)));
    let username = "testuser".to_string();
    let port = 2222;
    let forward_agent = true;
    let login_password = Some("password123".to_string());
    let key = Some("/home/user/.ssh/id_rsa".to_string());
    let passphrase = Some("key_passphrase".to_string());
    let key_comment = Some("user@machine".to_string());
    
    let conn = SshConnection::new(
        host,
        &username,
        port,
        hostname.clone(),
        forward_agent,
        login_password.clone(),
        key.clone(),
        passphrase.clone(),
        key_comment.clone()
    );
    
    assert_eq!(conn.username, "testuser");
    assert_eq!(conn.port, 2222);
    assert_eq!(conn.hostname, "test-server");
    assert_eq!(conn.forward_agent, true);
    assert_eq!(conn.login_password, Some("password123".to_string()));
    assert_eq!(conn.key, Some("/home/user/.ssh/id_rsa".to_string()));
    assert_eq!(conn.passphrase, Some("key_passphrase".to_string()));
    assert_eq!(conn.key_comment, Some("user@machine".to_string()));
    assert!(conn.session.is_none());
}

#[test]
fn test_ssh_connection_whoami() {
    let hostname = "test-server".to_string();
    let host = Arc::new(RwLock::new(Host::new(&hostname)));
    let username = "testuser".to_string();
    
    let conn = SshConnection::new(
        host,
        &username,
        22,
        hostname,
        false,
        None,
        None,
        None,
        None
    );
    
    // whoami should return the username
    let result = conn.whoami();
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "testuser");
}

// Note: We can't test actual SSH connections without a real SSH server
// So we focus on testing the struct creation and basic methods