use jetpack::connection::cache::*;
use jetpack::inventory::hosts::Host;
use std::sync::{Arc, RwLock};

#[test]
fn test_connection_cache_new() {
    let cache = ConnectionCache::new();
    assert_eq!(cache.size(), 0);
}

#[test]
fn test_connection_cache_has_host() {
    let cache = ConnectionCache::new();
    let hostname = "testhost".to_string();
    let host = Arc::new(RwLock::new(Host::new(&hostname)));
    
    assert!(!cache.has_host(&host));
}

#[test]
fn test_cache_entry_creation() {
    let entry = CacheEntry {
        ssh_process: None,
        connection_type: "ssh".to_string(),
    };
    
    assert!(entry.ssh_process.is_none());
    assert_eq!(entry.connection_type, "ssh");
}

#[test]
fn test_cache_entry_with_process() {
    let entry = CacheEntry {
        ssh_process: Some(42), // Mock process ID
        connection_type: "ssh".to_string(),
    };
    
    assert_eq!(entry.ssh_process, Some(42));
    assert_eq!(entry.connection_type, "ssh");
}

#[test]
fn test_connection_cache_debug() {
    let cache = ConnectionCache::new();
    let debug_str = format!("{:?}", cache);
    assert!(debug_str.contains("ConnectionCache"));
}

#[test]
fn test_cache_entry_debug() {
    let entry = CacheEntry {
        ssh_process: None,
        connection_type: "local".to_string(),
    };
    let debug_str = format!("{:?}", entry);
    assert!(debug_str.contains("CacheEntry"));
    assert!(debug_str.contains("local"));
}

#[test]
fn test_connection_cache_multiple_hosts() {
    let cache = ConnectionCache::new();
    
    let hostname1 = "host1".to_string();
    let host1 = Arc::new(RwLock::new(Host::new(&hostname1)));
    
    let hostname2 = "host2".to_string();
    let host2 = Arc::new(RwLock::new(Host::new(&hostname2)));
    
    // Initially both hosts should not be in cache
    assert!(!cache.has_host(&host1));
    assert!(!cache.has_host(&host2));
}