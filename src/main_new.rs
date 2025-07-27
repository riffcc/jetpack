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

use jetpack::{
    JetpackConfig, ConnectionMode, PlaybookRunner, 
    TerminalOutputHandler, OutputHandler,
    JetpackError, Result
};
use jetpack::inventory::inventory::Inventory;
use jetpack::inventory::loading::load_inventory;
use jetpack::cli::show::{show_inventory_group, show_inventory_host};
use jetpack::cli::parser::{CliParser, CLI_MODE_SSH, CLI_MODE_CHECK_SSH, CLI_MODE_SHOW, 
                            CLI_MODE_SIMULATE, CLI_MODE_LOCAL, CLI_MODE_CHECK_LOCAL};
use jetpack::util::io::quit;
use std::sync::{Arc, RwLock};
use std::process;

fn main() {
    match liftoff() { 
        Err(e) => quit(&e.to_string()), 
        _ => {} 
    }
}

fn liftoff() -> Result<()> {
    let mut cli_parser = CliParser::new();
    cli_parser.parse().map_err(|e| JetpackError::Config(e))?;

    // jetp --help was given, or no arguments
    if cli_parser.needs_help {
        cli_parser.show_help();
        return Ok(());
    }
    if cli_parser.needs_version {
        cli_parser.show_version();
        return Ok(());
    }

    // For show mode, handle it the old way since it's CLI-specific
    if cli_parser.mode == CLI_MODE_SHOW {
        let inventory: Arc<RwLock<Inventory>> = Arc::new(RwLock::new(Inventory::new()));
        load_inventory(&inventory, Arc::clone(&cli_parser.inventory_paths))
            .map_err(|e| JetpackError::Inventory(e))?;
        
        if !cli_parser.inventory_set {
            return Err(JetpackError::Config("--inventory is required".into()));
        }
        if inventory.read().unwrap().hosts.len() == 0 {
            return Err(JetpackError::Inventory("no hosts found in --inventory".into()));
        }
        
        return handle_show(&inventory, &cli_parser);
    }

    // Check playbook is set
    if !cli_parser.playbook_set {
        return Err(JetpackError::Config("--playbook is required".into()));
    }

    // Convert CLI mode to ConnectionMode
    let (connection_mode, check_mode) = match cli_parser.mode {
        CLI_MODE_SSH => (ConnectionMode::Ssh, false),
        CLI_MODE_CHECK_SSH => (ConnectionMode::Ssh, true),
        CLI_MODE_LOCAL => (ConnectionMode::Local, false),
        CLI_MODE_CHECK_LOCAL => (ConnectionMode::Local, true),
        CLI_MODE_SIMULATE => (ConnectionMode::Simulate, false),
        _ => return Err(JetpackError::Config("Invalid CLI mode".into())),
    };

    // Build JetpackConfig from CLI parser
    let mut config = JetpackConfig::new()
        .connection_mode(connection_mode)
        .check_mode(check_mode)
        .user(cli_parser.default_user.clone())
        .port(cli_parser.default_port)
        .threads(cli_parser.threads)
        .verbosity(cli_parser.verbosity)
        .forward_agent(cli_parser.forward_agent)
        .extra_vars(cli_parser.extra_vars.clone());

    // Add playbook paths
    for path in cli_parser.playbook_paths.read().unwrap().iter() {
        config = config.playbook(path.clone());
    }

    // Add inventory paths
    for path in cli_parser.inventory_paths.read().unwrap().iter() {
        config = config.inventory(path.clone());
    }

    // Add role paths
    for path in cli_parser.role_paths.read().unwrap().iter() {
        config = config.role_path(path.clone());
    }

    // Add module paths
    for path in cli_parser.module_paths.read().unwrap().iter() {
        config = config.module_path(path.clone());
    }

    // Set optional fields
    if let Some(sudo) = cli_parser.sudo {
        config = config.sudo(sudo);
    }

    if let Some(batch_size) = cli_parser.batch_size {
        config = config.batch_size(batch_size);
    }

    if let Some(tags) = cli_parser.tags {
        config = config.tags(tags);
    }

    if let Some(password) = cli_parser.login_password {
        config.login_password = Some(password);
    }

    config = config
        .limit_hosts(cli_parser.limit_hosts.clone())
        .limit_groups(cli_parser.limit_groups.clone());

    // Create output handler
    let output_handler = Arc::new(TerminalOutputHandler::new(cli_parser.verbosity));

    // Run playbook using the library API
    let runner = PlaybookRunner::new(config)
        .with_output_handler(output_handler);

    match runner.run() {
        Ok(result) => {
            if !result.success {
                process::exit(1);
            }
            Ok(())
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            process::exit(1);
        }
    }
}

pub fn handle_show(inventory: &Arc<RwLock<Inventory>>, parser: &CliParser) -> Result<()> {
    // jetp show -i inventory
    // jetp show -i inventory --groups g1:g2
    // jetp show -i inventory --hosts h1:h2
    if parser.show_groups.is_empty() && parser.show_hosts.is_empty() {
        show_inventory_group(inventory, &String::from("all"))
            .map_err(|e| JetpackError::Inventory(e))?;
    }
    for group_name in parser.show_groups.iter() {
        show_inventory_group(inventory, &group_name.clone())
            .map_err(|e| JetpackError::Inventory(e))?;
    }
    for host_name in parser.show_hosts.iter() {
        show_inventory_host(inventory, &host_name.clone())
            .map_err(|e| JetpackError::Inventory(e))?;
    }
    Ok(())
}