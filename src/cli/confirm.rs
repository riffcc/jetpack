//! Destroy-action confirmation (PR2 of #47 / confirm-ux-destroy-only).
//!
//! Provisioning is driven by each inventory host's `provision:` block, and a
//! host is destroyed when `provision.state` is `absent` or `destroyed` — both
//! route to `destroy_host` (`provisioners/mod.rs`). This gate prompts for
//! interactive confirmation — TTY-gated, mutating modes only — before dispatch
//! when such a host is in scope. A normal `apply` with no destroy hosts never
//! prompts. There is no global `--yes`: in a non-TTY (CI/piped) run the prompt
//! is skipped and the run proceeds as before.

use crate::cli::parser::CliParser;
use crate::connection::no::NoFactory;
use crate::inventory::hosts::Host;
use crate::inventory::inventory::Inventory;
use crate::playbooks::context::PlaybookContext;
use crate::playbooks::traversal::{RunState, resolve_playbook_targets};
use crate::playbooks::visitor::{CheckMode, PlaybookVisitor};
use crate::provisioners::ProvisionState;
use std::collections::{HashMap, HashSet};
use std::io::{IsTerminal, Write};
use std::sync::{Arc, Mutex, RwLock};

/// Mutating execution modes that can actually destroy resources. The dry-run /
/// validation modes (plan/check-*/syntax/simulate/show/pull) never mutate, so
/// they never prompt.
fn is_mutating_mode(mode: u32) -> bool {
    use crate::cli::parser::*;
    matches!(
        mode,
        CLI_MODE_APPLY | CLI_MODE_RUN | CLI_MODE_SSH | CLI_MODE_LOCAL
    )
}

/// True when `host` would be destroyed by a run that touches it — i.e. its
/// `provision.state` parses to `Absent` or `Destroyed` (both call
/// `destroy_host`). Hosts with no provision block, or `present`/`stopped`,
/// are not destroys.
fn host_is_destroy_bearing(host: &Host) -> bool {
    let Some(cfg) = host.get_provision() else {
        return false;
    };
    matches!(
        ProvisionState::parse(&cfg.state),
        Ok(ProvisionState::Absent) | Ok(ProvisionState::Destroyed)
    )
}

/// Whether `host` passes the CLI `--limit-hosts` / `--limit-groups` filters,
/// mirroring `get_play_hosts` so the gate only considers hosts the run could
/// touch. Both filters, when active, must pass (AND).
fn host_in_limit_scope(host: &Host, parser: &CliParser) -> bool {
    if !parser.limit_hosts.is_empty() && !parser.limit_hosts.contains(&host.name) {
        return false;
    }
    if !parser.limit_groups.is_empty() {
        let in_a_limited_group = parser
            .limit_groups
            .iter()
            .any(|g| host.has_ancestor_group(g));
        if !in_a_limited_group {
            return false;
        }
    }
    true
}

/// Sorted list of in-scope hosts a run would destroy — those whose
/// `provision.state` is `absent`/`destroyed`, passing the `--limit` filters, and
/// (when known) actually targeted by the specified playbook's plays.
///
/// `playbook_targets` scopes the list to the hosts the `-p` playbook resolves to,
/// so `apply -p repro.yml` only prompts for what repro.yml touches — not every
/// destroy-declared host in the inventory. It is `None` when no playbook was
/// given or its groups couldn't be resolved; in that case every destroy-bearing
/// host in `--limit` scope is listed, preserving the safe-direction
/// over-approximation (it never misses a real destroy).
pub fn destroy_bearing_hosts(
    inventory: &Arc<RwLock<Inventory>>,
    parser: &CliParser,
    playbook_targets: Option<&std::collections::HashSet<String>>,
) -> Vec<String> {
    let inv = inventory.read().unwrap();
    let mut names: Vec<String> = inv
        .hosts
        .values()
        .filter_map(|h| {
            let h = h.read().unwrap();
            if !host_is_destroy_bearing(&h) || !host_in_limit_scope(&h, parser) {
                return None;
            }
            // Scope to the playbook's resolved targets when known; None = unknown,
            // so keep the host (over-approximation — never under-prompt).
            match playbook_targets {
                Some(targets) if !targets.contains(&h.name) => None,
                _ => Some(h.name.clone()),
            }
        })
        .collect();
    names.sort();
    names
}

/// Prompt for interactive confirmation (TTY-gated) when a mutating run would
/// destroy ≥1 in-scope host. No-op for non-mutating modes, non-TTY runs
/// (CI/pipes — proceeds as before), or runs with no destroy hosts. Returns
/// `Err` (abort) unless the operator types `destroy`.
pub fn confirm_destroy_if_tty(
    inventory: &Arc<RwLock<Inventory>>,
    parser: &CliParser,
) -> Result<(), String> {
    if !is_mutating_mode(parser.mode) {
        return Ok(());
    }
    if !std::io::stdin().is_terminal() {
        return Ok(());
    }
    // Scope the prompt to what the -p playbook actually targets (resolved play
    // groups), not the whole inventory. None when there's no playbook or its
    // groups can't be resolved → destroy_bearing_hosts falls back to inventory-wide.
    let playbook_targets = resolve_playbook_targets_for_confirm(inventory, parser);
    let targets = destroy_bearing_hosts(inventory, parser, playbook_targets.as_ref());
    if targets.is_empty() {
        return Ok(());
    }
    println!(
        "!! this run will DESTROY {} host(s): {}",
        targets.len(),
        targets.join(", ")
    );
    println!("   this deletes the underlying VM/container and is irreversible.");
    print!("   type 'destroy' to proceed, anything else to abort: ");
    let _ = std::io::stdout().flush();
    let mut answer = String::new();
    std::io::stdin()
        .read_line(&mut answer)
        .map_err(|e| format!("could not read confirmation: {}", e))?;
    if answer.trim() == "destroy" {
        Ok(())
    } else {
        Err(format!(
            "aborted: declined destroy of {} host(s) ({})",
            targets.len(),
            targets.join(", ")
        ))
    }
}

/// Resolve the `-p` playbook's target host set for the destroy prompt. Returns
/// `None` (→ inventory-wide over-approximation) when there's no playbook or
/// `resolve_playbook_targets` couldn't resolve every play's groups — so the
/// prompt never silently under-counts.
fn resolve_playbook_targets_for_confirm(
    inventory: &Arc<RwLock<Inventory>>,
    parser: &CliParser,
) -> Option<HashSet<String>> {
    if parser.playbook_paths.read().unwrap().is_empty() {
        return None;
    }
    let run_state = resolution_run_state(inventory, parser);
    match resolve_playbook_targets(&run_state) {
        Ok(Some(set)) => Some(set),
        Ok(None) | Err(_) => None,
    }
}

/// A `RunState` just capable of group resolution (no real connection factory) —
/// mirrors `playbook_syntax_check`'s build. Used only to drive
/// `resolve_target_groups` / `get_play_hosts` for the destroy prompt.
fn resolution_run_state(inventory: &Arc<RwLock<Inventory>>, parser: &CliParser) -> Arc<RunState> {
    Arc::new(RunState {
        inventory: Arc::clone(inventory),
        playbook_paths: Arc::clone(&parser.playbook_paths),
        role_paths: Arc::clone(&parser.role_paths),
        module_paths: Arc::clone(&parser.module_paths),
        limit_hosts: parser.limit_hosts.clone(),
        limit_groups: parser.limit_groups.clone(),
        batch_size: parser.batch_size,
        context: Arc::new(RwLock::new(PlaybookContext::new(parser))),
        visitor: Arc::new(RwLock::new(PlaybookVisitor::new(CheckMode::No))),
        connection_factory: Arc::new(RwLock::new(NoFactory::new())),
        tags: parser.tags.clone(),
        allow_localhost_delegation: parser.allow_localhost_delegation,
        is_pull_mode: false,
        syntax_mode: false,
        play_groups: parser.play_groups.clone(),
        output_handler: None,
        async_mode: parser.async_mode,
        playbook_contents: Vec::new(),
        processed_role_tasks: Arc::new(RwLock::new(std::collections::HashSet::new())),
        processed_role_handlers: Arc::new(RwLock::new(std::collections::HashSet::new())),
        role_processing_stack: Arc::new(RwLock::new(Vec::new())),
        fetched_files: Arc::new(Mutex::new(HashMap::new())),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provisioners::ProvisionConfig;

    /// Build a `ProvisionConfig` with only `state` set (via serde, so new Option
    /// fields stay defaulted). `type`/`cluster` are the required non-Option fields.
    fn provision(state: &str) -> ProvisionConfig {
        serde_yaml::from_str(&format!(
            "type: proxmox_lxc\nstate: {}\ncluster: testcluster\n",
            state
        ))
        .expect("test ProvisionConfig deserializes")
    }

    /// Build an inventory with hosts `(name, Option<state>, groups)`.
    fn inventory_with(hosts: &[(&str, Option<&str>, &[&str])]) -> Arc<RwLock<Inventory>> {
        let mut inv = Inventory::new();
        inv.store_group("all");
        for (name, state, groups) in hosts {
            if groups.is_empty() {
                inv.create_host(name);
            }
            for g in groups.iter() {
                inv.store_host(g, name);
            }
            if let Some(s) = state {
                inv.get_host(name)
                    .write()
                    .unwrap()
                    .set_provision(provision(s));
            }
        }
        Arc::new(RwLock::new(inv))
    }

    fn parser_with(limit_hosts: &[&str], limit_groups: &[&str]) -> CliParser {
        let mut p = CliParser::new();
        p.limit_hosts = limit_hosts.iter().map(|s| s.to_string()).collect();
        p.limit_groups = limit_groups.iter().map(|s| s.to_string()).collect();
        p.mode = crate::cli::parser::CLI_MODE_APPLY;
        p
    }

    #[test]
    fn detects_absent_and_destroyed_excludes_present_stopped_and_unprovisioned() {
        let inv = inventory_with(&[
            ("web-present", Some("present"), &["webservers"]),
            ("web-destroyed", Some("destroyed"), &["webservers"]),
            ("web-absent", Some("absent"), &["webservers"]),
            ("web-stopped", Some("stopped"), &["webservers"]),
            ("db-bare", None, &["dbservers"]),
        ]);
        let parser = parser_with(&[], &[]);
        let got = destroy_bearing_hosts(&inv, &parser, None);
        assert_eq!(
            got,
            vec!["web-absent".to_string(), "web-destroyed".to_string()]
        );
    }

    #[test]
    fn empty_when_no_destroy_hosts() {
        let inv = inventory_with(&[
            ("web", Some("present"), &["webservers"]),
            ("db", None, &["dbservers"]),
        ]);
        let parser = parser_with(&[], &[]);
        assert!(destroy_bearing_hosts(&inv, &parser, None).is_empty());
    }

    #[test]
    fn respects_limit_hosts() {
        let inv = inventory_with(&[
            ("web-destroyed", Some("destroyed"), &["webservers"]),
            ("db-absent", Some("absent"), &["dbservers"]),
        ]);
        // --limit web-destroyed only
        let parser = parser_with(&["web-destroyed"], &[]);
        assert_eq!(
            destroy_bearing_hosts(&inv, &parser, None),
            vec!["web-destroyed".to_string()]
        );
    }

    #[test]
    fn respects_limit_groups() {
        let inv = inventory_with(&[
            ("web-destroyed", Some("destroyed"), &["webservers"]),
            ("db-absent", Some("absent"), &["dbservers"]),
        ]);
        // --limit webservers only
        let parser = parser_with(&[], &["webservers"]);
        assert_eq!(
            destroy_bearing_hosts(&inv, &parser, None),
            vec!["web-destroyed".to_string()]
        );
    }

    #[test]
    fn playbook_targets_scope_the_destroy_prompt() {
        let inv = inventory_with(&[
            ("web-destroyed", Some("destroyed"), &["webservers"]),
            ("db-destroyed", Some("destroyed"), &["dbservers"]),
            ("web-present", Some("present"), &["webservers"]),
        ]);
        let parser = parser_with(&[], &[]);
        // A -p playbook resolving to only the webservers' hosts.
        let targets: HashSet<String> = ["web-destroyed", "web-present"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        // Only the destroy-bearing host the playbook targets is prompted; the
        // db-destroyed host (destroy-bearing, but not targeted) is excluded.
        assert_eq!(
            destroy_bearing_hosts(&inv, &parser, Some(&targets)),
            vec!["web-destroyed".to_string()]
        );
        // None (no playbook, or unresolvable groups) → inventory-wide
        // over-approximation: every destroy-bearing host in scope is listed.
        let mut all = destroy_bearing_hosts(&inv, &parser, None);
        all.sort();
        assert_eq!(
            all,
            vec!["db-destroyed".to_string(), "web-destroyed".to_string()]
        );
    }
}
