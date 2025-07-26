use jetpack::cli::show::*;
use jetpack::inventory::inventory::Inventory;
use jetpack::inventory::hosts::Host;
use jetpack::inventory::groups::Group;
use std::sync::{Arc, RwLock};

#[test]
fn test_display_mode_enum() {
    assert_ne!(DisplayMode::Groups, DisplayMode::Hosts);
    assert_eq!(DisplayMode::Groups, DisplayMode::Groups);
    assert_eq!(DisplayMode::Hosts, DisplayMode::Hosts);
}

#[test]
fn test_display_mode_debug() {
    let groups_mode = DisplayMode::Groups;
    let hosts_mode = DisplayMode::Hosts;
    
    assert_eq!(format!("{:?}", groups_mode), "Groups");
    assert_eq!(format!("{:?}", hosts_mode), "Hosts");
}

#[test]
fn test_show_inventory_empty() {
    let inventory = Inventory::new();
    
    // This would normally print to stdout, but we can at least verify it doesn't panic
    show_inventory(&inventory, DisplayMode::Hosts);
    show_inventory(&inventory, DisplayMode::Groups);
}

#[test]
fn test_show_inventory_with_hosts() {
    let mut inventory = Inventory::new();
    
    // Add some hosts
    let host1_name = "web1".to_string();
    let host2_name = "web2".to_string();
    let host3_name = "db1".to_string();
    
    let host1 = Arc::new(RwLock::new(Host::new(&host1_name)));
    let host2 = Arc::new(RwLock::new(Host::new(&host2_name)));
    let host3 = Arc::new(RwLock::new(Host::new(&host3_name)));
    
    inventory.add_host(Arc::clone(&host1));
    inventory.add_host(Arc::clone(&host2));
    inventory.add_host(Arc::clone(&host3));
    
    // This would print the hosts - verify it doesn't panic
    show_inventory(&inventory, DisplayMode::Hosts);
}

#[test]
fn test_show_inventory_with_groups() {
    let mut inventory = Inventory::new();
    
    // Add groups
    let webservers_name = "webservers".to_string();
    let databases_name = "databases".to_string();
    
    let webservers = Arc::new(RwLock::new(Group::new(&webservers_name)));
    let databases = Arc::new(RwLock::new(Group::new(&databases_name)));
    
    inventory.add_group(Arc::clone(&webservers));
    inventory.add_group(Arc::clone(&databases));
    
    // Add hosts to groups
    let host1_name = "web1".to_string();
    let host2_name = "web2".to_string();
    let host3_name = "db1".to_string();
    
    let host1 = Arc::new(RwLock::new(Host::new(&host1_name)));
    let host2 = Arc::new(RwLock::new(Host::new(&host2_name)));
    let host3 = Arc::new(RwLock::new(Host::new(&host3_name)));
    
    inventory.add_host(Arc::clone(&host1));
    inventory.add_host(Arc::clone(&host2));
    inventory.add_host(Arc::clone(&host3));
    
    webservers.write().unwrap().add_host(Arc::clone(&host1));
    webservers.write().unwrap().add_host(Arc::clone(&host2));
    databases.write().unwrap().add_host(Arc::clone(&host3));
    
    // This would print the groups - verify it doesn't panic
    show_inventory(&inventory, DisplayMode::Groups);
}