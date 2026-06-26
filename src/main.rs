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

use jetpack::cli::confirm::confirm_destroy_if_tty;
use jetpack::cli::docs::docs;
use jetpack::cli::gen_reference::gen_reference;
use jetpack::cli::install::install;
use jetpack::cli::parser::{CliParser, is_execution_mode};
use jetpack::cli::playbooks::{
    full_check, inventory_check, playbook_check_local, playbook_check_ssh, playbook_local,
    playbook_pull, playbook_simulate, playbook_ssh, playbook_syntax_check,
};
use jetpack::cli::secrets_diagnostic::missing_secret_variables;
use jetpack::cli::show::{show_inventory_group, show_inventory_host};
use jetpack::inventory::inventory::Inventory;
use jetpack::inventory::loading::load_inventory;
use jetpack::util::io::quit;
use jetpack::util::terminal::two_column_table;
use std::process;
use std::sync::{Arc, RwLock};

fn main() {
    if let Err(e) = liftoff() {
        quit(&e)
    }
}

fn liftoff() -> Result<(), String> {
    let mut cli_parser = CliParser::new();
    cli_parser.parse()?;

    // jetp --help was given, or no arguments
    if cli_parser.needs_help {
        cli_parser.show_help();
        return Ok(());
    }
    if cli_parser.needs_version {
        cli_parser.show_version();
        return Ok(());
    }

    // The resolution summary is a verbose-only diagnostic: automation root,
    // playbook, inventory, roles, and mode after CLI flags + the .jetpack
    // contract + conventions have merged. It is informational, so it goes to
    // stdout (never stderr) and only when the user asks for verbosity in a mode
    // that actually resolves a playbook.
    if cli_parser.verbosity > 0 && is_execution_mode(cli_parser.mode) {
        two_column_table("field", "value", &cli_parser.resolution_summary());
    }

    let inventory: Arc<RwLock<Inventory>> = Arc::new(RwLock::new(Inventory::new()));

    match cli_parser.mode {
        jetpack::cli::parser::CLI_MODE_SSH
        | jetpack::cli::parser::CLI_MODE_CHECK_SSH
        | jetpack::cli::parser::CLI_MODE_APPLY
        | jetpack::cli::parser::CLI_MODE_RUN
        | jetpack::cli::parser::CLI_MODE_PLAN
        | jetpack::cli::parser::CLI_MODE_SHOW
        | jetpack::cli::parser::CLI_MODE_SIMULATE => {
            load_inventory(
                &inventory,
                Arc::new(RwLock::new(cli_parser.inventory_load_paths())),
            )?;
            if !cli_parser.inventory_set {
                return Err(String::from(
                    "--inventory is required (pass -i PATH; for zero-arg runs, declare \
                     `inventory:` in a .jetpack.yml)",
                ));
            }
            if inventory.read().expect("inventory read").hosts.is_empty() {
                return Err(String::from("no hosts found in --inventory"));
            }
        }
        jetpack::cli::parser::CLI_MODE_PULL
        | jetpack::cli::parser::CLI_MODE_LOCAL
        | jetpack::cli::parser::CLI_MODE_CHECK_LOCAL => {
            // In pull/local modes, inventory is optional. If provided, it's used for variables/groups
            if cli_parser.inventory_set {
                load_inventory(
                    &inventory,
                    Arc::new(RwLock::new(cli_parser.inventory_load_paths())),
                )?;
            }
            // Ensure localhost is in the inventory for local execution
            inventory
                .write()
                .expect("inventory write")
                .store_host(&String::from("all"), &String::from("localhost"));
        }
        jetpack::cli::parser::CLI_MODE_SYNTAX
        | jetpack::cli::parser::CLI_MODE_INVENTORY_CHECK
        | jetpack::cli::parser::CLI_MODE_FULL_CHECK
        | jetpack::cli::parser::CLI_MODE_DOCS
        | jetpack::cli::parser::CLI_MODE_GEN_REFERENCE
        | jetpack::cli::parser::CLI_MODE_INSTALL => {
            // validation modes load inventory on demand inside the check
            // functions; do not seed localhost so inventory-check inspects the
            // on-disk tree exactly as declared. install is a local self-setup
            // utility and needs no inventory.
        }
        _ => {
            inventory
                .write()
                .expect("inventory write")
                .store_host(&String::from("all"), &String::from("localhost"));
        }
    };

    match cli_parser.mode {
        jetpack::cli::parser::CLI_MODE_SHOW
        | jetpack::cli::parser::CLI_MODE_INVENTORY_CHECK
        | jetpack::cli::parser::CLI_MODE_DOCS
        | jetpack::cli::parser::CLI_MODE_GEN_REFERENCE
        | jetpack::cli::parser::CLI_MODE_INSTALL => {}
        jetpack::cli::parser::CLI_MODE_PULL => {
            if !cli_parser.playbook_set && cli_parser.pull_url.is_none() {
                return Err(String::from(
                    "--playbook or --url is required for pull mode",
                ));
            }
        }
        _ => {
            if !cli_parser.playbook_set {
                return Err(String::from(
                    "--playbook is required (pass -p PATH; for zero-arg runs, declare \
                     `playbook:` in a .jetpack.yml)",
                ));
            }
        }
    };

    if cli_parser.threads > 1 {
        rayon::ThreadPoolBuilder::new()
            .num_threads(cli_parser.threads)
            .build_global()
            .expect("build global");
    };

    // Destroy-action confirm (PR2 of #47): a TTY-gated prompt only when a
    // mutating run targets hosts whose `provision.state` is absent/destroyed.
    // No-op for non-mutating modes, non-TTY runs (CI/pipes), and runs with no
    // destroy hosts — so a normal `apply` never prompts, and there is no
    // `--yes`.
    confirm_destroy_if_tty(&inventory, &cli_parser)?;

    // #55: a declared-but-missing secrets_inventory in a non-mutating mode
    // (converging modes already hard-errored in parse) is skipped, not fatal — so
    // a fresh clone / CI / contributor without the secrets sibling can still
    // validate. Informational notice → stdout, never stderr. When inventory is
    // loaded we also name the variables that would be undefined without the
    // overlay, so the operator sees the blast radius rather than a bare "skipped".
    {
        let missing = cli_parser.missing_secrets.read().unwrap();
        if !missing.is_empty() {
            let paths: Vec<String> = missing.iter().map(|p| p.display().to_string()).collect();
            let inventory_loaded = !inventory.read().expect("inventory read").hosts.is_empty();
            let undefined = if inventory_loaded {
                let playbook_paths = cli_parser.playbook_paths.read().unwrap().clone();
                let role_paths = cli_parser.role_paths.read().unwrap().clone();
                missing_secret_variables(
                    &playbook_paths,
                    &role_paths,
                    &inventory,
                    &cli_parser.extra_vars,
                )
            } else {
                std::collections::BTreeSet::new()
            };
            if undefined.is_empty() {
                println!(
                    "note: secrets_inventory not found on this machine — skipping the secrets \
                     overlay for this non-mutating run (define it, or pass --no-secrets to \
                     silence): {}",
                    paths.join(", ")
                );
            } else {
                let vars: Vec<String> = undefined.iter().cloned().collect();
                println!(
                    "note: secrets_inventory not found on this machine — skipping the secrets \
                     overlay for this non-mutating run. Without it these referenced variables \
                     would be undefined (define the overlay, or pass --no-secrets to silence): \
                     {}",
                    vars.join(", ")
                );
            }
        }
    }

    let exit_status = match cli_parser.mode {
        jetpack::cli::parser::CLI_MODE_SHOW => match handle_show(&inventory, &cli_parser) {
            Ok(_) => 0,
            Err(s) => {
                println!("{}", s);
                1
            }
        },
        // #49: `apply`/`run` converge over SSH (run is apply under a less-loaded
        // term), grouped with the legacy `ssh` alias; `plan` is the dry-run,
        // grouped with the legacy `check-ssh` alias.
        jetpack::cli::parser::CLI_MODE_APPLY
        | jetpack::cli::parser::CLI_MODE_RUN
        | jetpack::cli::parser::CLI_MODE_SSH => playbook_ssh(&inventory, &cli_parser),
        jetpack::cli::parser::CLI_MODE_PLAN | jetpack::cli::parser::CLI_MODE_CHECK_SSH => {
            playbook_check_ssh(&inventory, &cli_parser)
        }
        jetpack::cli::parser::CLI_MODE_LOCAL => playbook_local(&inventory, &cli_parser),
        jetpack::cli::parser::CLI_MODE_CHECK_LOCAL => playbook_check_local(&inventory, &cli_parser),
        jetpack::cli::parser::CLI_MODE_SIMULATE => playbook_simulate(&inventory, &cli_parser),
        jetpack::cli::parser::CLI_MODE_PULL => playbook_pull(&inventory, &cli_parser),
        jetpack::cli::parser::CLI_MODE_SYNTAX => playbook_syntax_check(&inventory, &cli_parser),
        jetpack::cli::parser::CLI_MODE_INVENTORY_CHECK => inventory_check(&inventory, &cli_parser),
        jetpack::cli::parser::CLI_MODE_FULL_CHECK => full_check(&inventory, &cli_parser),
        jetpack::cli::parser::CLI_MODE_DOCS => docs(&cli_parser),
        jetpack::cli::parser::CLI_MODE_GEN_REFERENCE => gen_reference(&cli_parser),
        jetpack::cli::parser::CLI_MODE_INSTALL => install(&cli_parser),

        _ => {
            println!("invalid CLI mode");
            1
        }
    };
    if exit_status != 0 {
        process::exit(exit_status);
    }
    Ok(())
}

pub fn handle_show(inventory: &Arc<RwLock<Inventory>>, parser: &CliParser) -> Result<(), String> {
    // jetp show -i inventory
    // jetp show -i inventory --groups g1:g2
    // jetp show -i inventory --hosts h1:h2
    if parser.show_groups.is_empty() && parser.show_hosts.is_empty() {
        show_inventory_group(inventory, &String::from("all"))?;
    }
    for group_name in parser.show_groups.iter() {
        show_inventory_group(inventory, &group_name.clone())?;
    }
    for host_name in parser.show_hosts.iter() {
        show_inventory_host(inventory, &host_name.clone())?;
    }
    Ok(())
}
