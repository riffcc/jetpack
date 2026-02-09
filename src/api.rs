// Jetporch
// Copyright (C) 2023 - Michael DeHaan <michael@michaeldehaan.net> + contributors
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// at your option) any later version.
// 
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
// 
// You should have received a copy of the GNU General Public License
// long with this program.  If not, see <http://www.gnu.org/licenses/>.

use crate::config::{JetpackConfig, ConnectionMode};
use crate::error::{JetpackError, Result};
use crate::output::{OutputHandler, OutputHandlerRef, NullOutputHandler};
use crate::inventory::inventory::Inventory;
use crate::inventory::loading::load_inventory;
use crate::playbooks::context::PlaybookContext;
use crate::playbooks::visitor::{PlaybookVisitor, CheckMode};
use crate::playbooks::traversal::{playbook_traversal, RunState};
use crate::connection::factory::ConnectionFactory;
use crate::connection::ssh::SshFactory as OldSshFactory;
use crate::connection::local::LocalFactory as OldLocalFactory; 
use crate::connection::no::NoFactory as OldNoFactory;
use std::sync::{Arc, RwLock};

/// Main API for running JetPack playbooks
pub struct PlaybookRunner {
    config: JetpackConfig,
    output_handler: OutputHandlerRef,
    provided_inventory: Option<Arc<RwLock<Inventory>>>,
}

impl PlaybookRunner {
    /// Create a new PlaybookRunner with the given configuration
    pub fn new(config: JetpackConfig) -> Self {
        Self {
            config,
            output_handler: Arc::new(NullOutputHandler),
            provided_inventory: None,
        }
    }
    
    /// Set a custom output handler
    pub fn with_output_handler(mut self, handler: Arc<dyn OutputHandler>) -> Self {
        self.output_handler = handler;
        self
    }

    /// Use a pre-built inventory instead of loading from files.
    /// When set, inventory_paths are ignored and this inventory is used directly.
    pub fn with_inventory(mut self, inventory: Arc<RwLock<Inventory>>) -> Self {
        self.provided_inventory = Some(inventory);
        self
    }
    
    /// Run the configured playbooks
    pub fn run(&self) -> Result<PlaybookResult> {
        // Set up thread pool if configured
        if self.config.threads > 1 {
            rayon::ThreadPoolBuilder::new()
                .num_threads(self.config.threads)
                .build_global()
                .map_err(|e| JetpackError::Config(format!("Failed to build thread pool: {}", e)))?;
        }
        
        // Use provided inventory or create and load from files
        let inventory = if let Some(ref inv) = self.provided_inventory {
            Arc::clone(inv)
        } else {
            let inv = Arc::new(RwLock::new(Inventory::new()));
            if !self.config.inventory_paths.read().unwrap().is_empty() {
                load_inventory(&inv, Arc::clone(&self.config.inventory_paths))
                    .map_err(|e| JetpackError::Inventory(e))?;
            }
            inv
        };
        
        match self.config.connection_mode {
            ConnectionMode::Local => {
                // For local mode, add localhost if no inventory was loaded
                if inventory.read().unwrap().hosts.is_empty() {
                    inventory.write().unwrap().store_host(&String::from("all"), &String::from("localhost"));
                }
            }
            _ => {
                // For non-local modes, require inventory (skip check if provided programmatically)
                if self.provided_inventory.is_none() && self.config.inventory_paths.read().unwrap().is_empty() {
                    return Err(JetpackError::Config("No inventory paths specified".into()));
                }
                
                if inventory.read().unwrap().hosts.is_empty() {
                    return Err(JetpackError::Inventory("No hosts found in inventory".into()));
                }
            }
        }
        
        
        // Check playbook paths
        if self.config.playbook_paths.read().unwrap().is_empty() {
            return Err(JetpackError::Config("No playbook paths specified".into()));
        }
        
        // Create connection factory
        let connection_factory: Arc<RwLock<dyn ConnectionFactory>> = match self.config.connection_mode {
            ConnectionMode::Ssh => Arc::new(RwLock::new(OldSshFactory::new(
                &inventory,
                self.config.forward_agent,
                self.config.login_password.clone(),
                self.config.private_key_file.clone(),
            ))),
            ConnectionMode::Local => Arc::new(RwLock::new(OldLocalFactory::new(&inventory))),
            ConnectionMode::Simulate => Arc::new(RwLock::new(OldNoFactory::new())),
        };
        
        // Create a minimal CLI parser to pass to context
        // This is a temporary workaround until we refactor context
        let mut temp_parser = crate::cli::parser::CliParser::new();
        temp_parser.default_user = self.config.default_user.clone();
        temp_parser.default_port = self.config.default_port;
        temp_parser.sudo = self.config.sudo.clone();
        temp_parser.extra_vars = self.config.extra_vars.clone();
        temp_parser.verbosity = self.config.verbosity;
        
        // Create playbook context
        let context = Arc::new(RwLock::new(PlaybookContext::new(&temp_parser)));
        
        // Create visitor with output handler
        let check_mode = if self.config.check_mode {
            CheckMode::Yes
        } else {
            CheckMode::No
        };
        
        let visitor = Arc::new(RwLock::new(PlaybookVisitor::new(check_mode)));
        
        // Create run state
        let run_state = Arc::new(RunState {
            inventory: inventory.clone(),
            playbook_paths: self.config.playbook_paths.clone(),
            role_paths: self.config.role_paths.clone(),
            module_paths: self.config.module_paths.clone(),
            limit_hosts: self.config.limit_hosts.clone(),
            limit_groups: self.config.limit_groups.clone(),
            batch_size: self.config.batch_size,
            context,
            visitor,
            connection_factory,
            tags: self.config.tags.clone(),
            allow_localhost_delegation: self.config.allow_localhost_delegation,
            is_pull_mode: false,
            play_groups: None,
            output_handler: Some(self.output_handler.clone()),
            async_mode: false,
            processed_role_tasks: Arc::new(RwLock::new(std::collections::HashSet::new())),
            processed_role_handlers: Arc::new(RwLock::new(std::collections::HashSet::new())),
            role_processing_stack: Arc::new(RwLock::new(Vec::new())),
        });
        
        // Run the playbooks
        match playbook_traversal(&run_state) {
            Ok(_) => {
                let stats = PlaybookResult {
                    success: true,
                    hosts_processed: inventory.read().unwrap().hosts.len(),
                };
                Ok(stats)
            }
            Err(e) => Err(JetpackError::PlaybookParse(e))
        }
    }
}

/// Result of a playbook run
#[derive(Debug, Clone)]
pub struct PlaybookResult {
    pub success: bool,
    pub hosts_processed: usize,
}

/// Builder-style API for simpler use cases
pub fn run_playbook(playbook_path: &str) -> PlaybookRunnerBuilder {
    PlaybookRunnerBuilder::new(playbook_path)
}

pub struct PlaybookRunnerBuilder {
    config: JetpackConfig,
    provided_inventory: Option<Arc<RwLock<Inventory>>>,
}

impl PlaybookRunnerBuilder {
    fn new(playbook_path: &str) -> Self {
        let config = JetpackConfig::new()
            .playbook(playbook_path);
        Self { config, provided_inventory: None }
    }
    
    pub fn inventory(mut self, path: &str) -> Self {
        self.config = self.config.inventory(path);
        self
    }
    
    pub fn local(mut self) -> Self {
        self.config = self.config.local();
        self
    }
    
    pub fn ssh(mut self) -> Self {
        self.config = self.config.ssh();
        self
    }
    
    pub fn user(mut self, user: &str) -> Self {
        self.config = self.config.user(user.to_string());
        self
    }
    
    pub fn sudo(mut self, sudo: &str) -> Self {
        self.config = self.config.sudo(sudo.to_string());
        self
    }
    
    pub fn limit_hosts(mut self, hosts: Vec<String>) -> Self {
        self.config = self.config.limit_hosts(hosts);
        self
    }
    
    pub fn extra_vars(mut self, vars: serde_yaml::Value) -> Self {
        self.config = self.config.extra_vars(vars);
        self
    }
    
    pub fn threads(mut self, threads: usize) -> Self {
        self.config = self.config.threads(threads);
        self
    }
    
    pub fn verbose(mut self) -> Self {
        self.config = self.config.verbose();
        self
    }
    
    pub fn check_mode(mut self) -> Self {
        self.config = self.config.check_mode(true);
        self
    }

    pub fn login_password(mut self, password: &str) -> Self {
        self.config = self.config.login_password(password.to_string());
        self
    }

    pub fn private_key_file(mut self, path: &str) -> Self {
        self.config = self.config.private_key_file(path.to_string());
        self
    }

    /// Use a pre-built inventory instead of loading from files.
    pub fn with_inventory(mut self, inventory: Arc<RwLock<Inventory>>) -> Self {
        self.provided_inventory = Some(inventory);
        self
    }

    pub fn run(self) -> Result<PlaybookResult> {
        let mut runner = PlaybookRunner::new(self.config);
        if let Some(inv) = self.provided_inventory {
            runner = runner.with_inventory(inv);
        }
        runner.run()
    }

    pub fn run_with_output(self, handler: Arc<dyn OutputHandler>) -> Result<PlaybookResult> {
        let mut runner = PlaybookRunner::new(self.config)
            .with_output_handler(handler);
        if let Some(inv) = self.provided_inventory {
            runner = runner.with_inventory(inv);
        }
        runner.run()
    }
}