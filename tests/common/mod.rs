// Common test utilities

use jetpack::playbooks::traversal::RunState;
use jetpack::inventory::inventory::Inventory;
use jetpack::playbooks::context::PlaybookContext;
use jetpack::playbooks::visitor::{PlaybookVisitor, CheckMode};
use jetpack::cli::parser::CliParser;
use jetpack::connection::factory::ConnectionFactory;
use jetpack::connection::local::LocalFactory;
use std::sync::{Arc, RwLock};
use std::collections::HashSet;

pub fn create_test_run_state() -> Arc<RunState> {
    let parser = CliParser::new();
    let mut inventory = Inventory::new();
    
    // Add localhost as required by LocalFactory
    let localhost_name = "localhost".to_string();
    inventory.create_host(&localhost_name);
    
    let inventory = Arc::new(RwLock::new(inventory));
    let context = Arc::new(RwLock::new(PlaybookContext::new(&parser)));
    let visitor = Arc::new(RwLock::new(PlaybookVisitor::new(CheckMode::No)));
    let connection_factory: Arc<RwLock<dyn ConnectionFactory>> = Arc::new(RwLock::new(LocalFactory::new(&inventory)));
    
    Arc::new(RunState {
        inventory: Arc::clone(&inventory),
        playbook_paths: Arc::new(RwLock::new(Vec::new())),
        role_paths: Arc::new(RwLock::new(Vec::new())),
        module_paths: Arc::new(RwLock::new(Vec::new())),
        limit_hosts: Vec::new(),
        limit_groups: Vec::new(),
        batch_size: None,
        context: Arc::clone(&context),
        visitor,
        connection_factory,
        tags: None,
        allow_localhost_delegation: false,
        is_pull_mode: false,
        play_groups: None,
        processed_role_tasks: Arc::new(RwLock::new(HashSet::new())),
        processed_role_handlers: Arc::new(RwLock::new(HashSet::new())),
        role_processing_stack: Arc::new(RwLock::new(Vec::new())),
        output_handler: None,
        async_mode: false,
    })
}