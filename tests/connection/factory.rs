use jetpack::Inventory;
use jetpack::cli::parser::CliParser;
use jetpack::connection::{factory::ConnectionFactory, local::LocalFactory};
use jetpack::playbooks::context::PlaybookContext;
use std::sync::{Arc, RwLock};

#[test]
fn factory_trait_reuses_its_local_connection_for_all_targets() {
    let mut inventory = Inventory::new();
    inventory.create_host("localhost");
    inventory.create_host("logical-target");
    let inventory = Arc::new(RwLock::new(inventory));
    let factory: Box<dyn ConnectionFactory> = Box::new(LocalFactory::new(&inventory));
    let context = Arc::new(RwLock::new(PlaybookContext::new(&CliParser::new())));
    let host = inventory.read().unwrap().get_host("logical-target");
    let local = factory.get_local_connection(&context).unwrap();
    let target = factory.get_connection(&context, &host).unwrap();
    assert!(Arc::ptr_eq(&local, &target));
    assert!(target.lock().unwrap().whoami().is_ok());
    assert!(host.read().unwrap().os_type.is_some());
}
