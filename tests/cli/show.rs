use jetpack::Inventory;
use jetpack::cli::show::{show_inventory_group, show_inventory_host};
use std::sync::{Arc, RwLock};

#[test]
fn missing_host_and_group_report_the_requested_name() {
    let inventory = Arc::new(RwLock::new(Inventory::new()));
    assert_eq!(
        show_inventory_host(&inventory, &"missing-host".into()).unwrap_err(),
        "no such host: missing-host"
    );
    assert_eq!(
        show_inventory_group(&inventory, &"missing-group".into()).unwrap_err(),
        "no such group: missing-group"
    );
}

#[test]
fn existing_host_and_group_can_be_displayed() {
    let mut inventory = Inventory::new();
    inventory.store_host("webservers", "web1");
    let inventory = Arc::new(RwLock::new(inventory));
    show_inventory_host(&inventory, &"web1".into()).unwrap();
    show_inventory_group(&inventory, &"webservers".into()).unwrap();
}
