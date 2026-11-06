use jetpack::connection::{cache::ConnectionCache, connection::Connection, local::LocalConnection};
use jetpack::inventory::hosts::Host;
use std::sync::{Arc, Mutex, RwLock};

#[test]
fn cache_reuses_connections_by_host_name_and_clear_removes_them() {
    let host = Arc::new(RwLock::new(Host::new("one")));
    let same_name = Arc::new(RwLock::new(Host::new("one")));
    let other = Arc::new(RwLock::new(Host::new("two")));
    let connection: Arc<Mutex<dyn Connection>> = Arc::new(Mutex::new(LocalConnection::new(&host)));
    let mut cache = ConnectionCache::new();
    assert!(!cache.has_connection(&host));
    cache.add_connection(&host, &connection);
    assert!(cache.has_connection(&same_name));
    assert!(!cache.has_connection(&other));
    assert!(Arc::ptr_eq(&cache.get_connection(&same_name), &connection));
    cache.clear();
    assert!(!cache.has_connection(&host));
}

#[test]
fn cache_replaces_only_the_requested_hosts_connection() {
    let host = Arc::new(RwLock::new(Host::new("one")));
    let other = Arc::new(RwLock::new(Host::new("two")));
    let first: Arc<Mutex<dyn Connection>> = Arc::new(Mutex::new(LocalConnection::new(&host)));
    let second: Arc<Mutex<dyn Connection>> = Arc::new(Mutex::new(LocalConnection::new(&host)));
    let mut cache = ConnectionCache::new();
    cache.add_connection(&host, &first);
    cache.add_connection(&other, &first);
    cache.add_connection(&host, &second);
    assert!(Arc::ptr_eq(&cache.get_connection(&host), &second));
    assert!(Arc::ptr_eq(&cache.get_connection(&other), &first));
}
