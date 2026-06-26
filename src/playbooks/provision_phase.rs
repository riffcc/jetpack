// Jetpack
// Copyright (C) Riff Labs Limited <team@riff.cc>
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

//! Per-host provision phase.
//!
//! [`provision_host_with`] provisions a single host and bakes its SSH
//! connection variables (`jet_ssh_hostname` / `jet_ssh_user`) onto it, then
//! returns a [`ProvisionOutcome`]. It performs no task execution and touches no
//! barriers — control flow (abort-vs-continue, destroy handling) is owned by
//! the caller. It is parameterized over the provision function so it can be
//! exercised in tests without real infrastructure; production passes
//! [`ensure_host_provisioned`](crate::provisioners::ensure_host_provisioned).

use crate::dns::DnsConfig;
use crate::inventory::hosts::Host;
use crate::inventory::inventory::Inventory;
use crate::playbooks::context::PlaybookContext;
use crate::playbooks::traversal::RunState;
use crate::provisioners::{ProvisionConfig, ProvisionResult, get_provisioner};
use std::sync::{Arc, RwLock};

/// The result of attempting to provision a single host.
///
/// Callers apply their own policy: `Ready` hosts run tasks; `Destroyed` hosts
/// are excluded from the task pool (the provisioner removed them intentionally);
/// `Failed` aborts the play (sequential) or is recorded as a per-host failure
/// (async).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProvisionOutcome {
    /// Provisioned (or needed no provisioning); ready for tasks.
    Ready,
    /// The provisioner destroyed this host; do not run tasks against it.
    Destroyed,
    /// Provisioning failed. Holds the raw error from the provision function.
    Failed(String),
}

/// Provision one host and set its connection variables.
///
/// `provision` is [`ensure_host_provisioned`] in production and a fake in tests.
pub fn provision_host_with<F>(
    run_state: &Arc<RunState>,
    host: &Arc<RwLock<Host>>,
    provision: F,
) -> ProvisionOutcome
where
    F: Fn(
        &ProvisionConfig,
        &str,
        &Arc<RwLock<Inventory>>,
        Option<&DnsConfig>,
        Option<&crate::output::OutputHandlerRef>,
    ) -> Result<ProvisionResult, String>,
{
    let (host_name, needs_provision, provision_config, host_vars) = {
        let h = host.read().unwrap();
        (
            h.name.clone(),
            h.needs_provisioning(),
            h.get_provision().cloned(),
            h.get_variables(),
        )
    };

    // No provision declared → nothing to do; the host is ready as-is.
    if !needs_provision {
        return ProvisionOutcome::Ready;
    }

    let Some(config) = provision_config else {
        return ProvisionOutcome::Ready;
    };

    // Dry-run: never create/update/destroy infrastructure in check mode.
    if run_state.visitor.read().unwrap().is_check_mode() {
        eprintln!(
            "  ~ {} => would provision ({}) — skipped (check mode)",
            host_name, config.provision_type
        );
        return ProvisionOutcome::Ready;
    }

    let dns_key = serde_yaml::Value::String("dns".to_string());
    let automation_root = run_state.context.read().unwrap().automation_root.clone();
    let dns_config = host_vars
        .get(&dns_key)
        .and_then(|v| crate::dns::dns_config_from_vars(v, &automation_root));

    match provision(
        &config,
        &host_name,
        &run_state.inventory,
        dns_config.as_ref(),
        run_state.output_handler.as_ref(),
    ) {
        Ok(result) => {
            run_state.visitor.read().unwrap().on_host_provisioned(
                &run_state.context,
                &host_name,
                &result,
            );

            if matches!(result, ProvisionResult::Destroyed) {
                return ProvisionOutcome::Destroyed;
            }

            // Resolve the connection IP and bake it (plus the ssh user) onto the
            // host so subsequent tasks connect to the right place.
            let ip = get_provisioner(&config.provision_type).ok().and_then(|p| {
                p.get_ip(&config, &host_name, &run_state.inventory)
                    .ok()
                    .flatten()
            });

            let mut h = host.write().unwrap();
            let mut vars = h.get_variables();
            let mut changed = false;

            if let Some(ip_addr) = ip {
                let key = serde_yaml::Value::String("jet_ssh_hostname".to_string());
                if !vars.contains_key(&key) {
                    vars.insert(key, serde_yaml::Value::String(ip_addr));
                    changed = true;
                }
            }

            if let Some(ssh_user) = &config.ssh_user {
                let key = serde_yaml::Value::String("jet_ssh_user".to_string());
                if !vars.contains_key(&key) {
                    vars.insert(key, serde_yaml::Value::String(ssh_user.clone()));
                    changed = true;
                }
            }

            if changed {
                h.set_variables(vars);
            }

            ProvisionOutcome::Ready
        }
        Err(e) => ProvisionOutcome::Failed(e),
    }
}

/// Apply per-host provision outcomes to the task pool and decide whether the
/// play proceeds.
///
/// - `Ready` → the host stays in the task pool.
/// - `Destroyed` → the host is removed from the pool (an intentional destroy,
///   not a failure).
/// - `Failed` → abort the play immediately (the sequential
///   abort-on-any-provision-failure policy).
///
/// Returns the number of destroyed hosts so the caller can recognize an
/// all-destroyed play — a success with nothing left to configure — rather than
/// erroring "no hosts remaining".
pub fn apply_provision_outcomes(
    context: &Arc<RwLock<PlaybookContext>>,
    hosts: &[Arc<RwLock<Host>>],
    outcomes: &[ProvisionOutcome],
) -> Result<usize, String> {
    let mut destroyed = 0usize;
    let mut ctx = context.write().unwrap();
    for (host, outcome) in hosts.iter().zip(outcomes) {
        match outcome {
            ProvisionOutcome::Ready => {}
            ProvisionOutcome::Destroyed => {
                ctx.destroy_host(host);
                destroyed += 1;
            }
            ProvisionOutcome::Failed(error) => {
                let name = host.read().unwrap().name.clone();
                return Err(format!("Failed to provision host '{}': {}", name, error));
            }
        }
    }
    Ok(destroyed)
}

/// Format a one-line-per-host summary of the parallel provision phase, flagging
/// the slowest host as the straggler. Pure (no I/O) so it can be unit-tested.
///
/// `timed[i]` corresponds to `hosts[i]` — rayon's `par_iter().collect()` into a
/// `Vec` preserves the original order.
pub fn format_provision_summary(
    hosts: &[Arc<RwLock<Host>>],
    timed: &[(ProvisionOutcome, std::time::Duration)],
    total: std::time::Duration,
) -> String {
    use std::fmt::Write as _;
    let (mut ready, mut destroyed, mut failed) = (0usize, 0usize, 0usize);
    let mut slowest = std::time::Duration::ZERO;
    let mut straggler_idx = None;
    for (i, (outcome, dur)) in timed.iter().enumerate() {
        match outcome {
            ProvisionOutcome::Ready => ready += 1,
            ProvisionOutcome::Destroyed => destroyed += 1,
            ProvisionOutcome::Failed(_) => failed += 1,
        }
        if *dur > slowest {
            slowest = *dur;
            straggler_idx = Some(i);
        }
    }
    let mut out = String::new();
    let _ = writeln!(
        out,
        "> provision phase: {} host(s) in {:.1}s (parallel) — ready:{} destroyed:{} failed:{}",
        hosts.len(),
        total.as_secs_f64(),
        ready,
        destroyed,
        failed
    );
    for (i, (host, (outcome, dur))) in hosts.iter().zip(timed.iter()).enumerate() {
        let name = host.read().unwrap().name.clone();
        let label = match outcome {
            ProvisionOutcome::Ready => "ready",
            ProvisionOutcome::Destroyed => "destroyed",
            ProvisionOutcome::Failed(_) => "failed",
        };
        let flag = if straggler_idx == Some(i) && timed.len() > 1 {
            "  <- straggler"
        } else {
            ""
        };
        let _ = writeln!(
            out,
            "    {name:<28} {label:<10} {:>6.1}s{flag}",
            dur.as_secs_f64()
        );
    }
    out
}
