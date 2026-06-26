// Jetpack
// Copyright (C) Riff Labs Limited <team@riff.cc>
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

//! Tests for the per-host provision seam (`provision_host_with`).
//!
//! These drive the helper with a fake provision function so the outcome
//! mapping, SSH-var setting, and check-mode / no-provision short-circuits can
//! be verified without any real infrastructure.

use jetpack::cli::parser::CliParser;
use jetpack::connection::no::NoFactory;
use jetpack::dns::DnsConfig;
use jetpack::inventory::hosts::Host;
use jetpack::inventory::inventory::Inventory;
use jetpack::output::OutputHandlerRef;
use jetpack::playbooks::context::PlaybookContext;
use jetpack::playbooks::provision_phase::{
    ProvisionOutcome, apply_provision_outcomes, provision_host_with,
};
use jetpack::playbooks::traversal::RunState;
use jetpack::playbooks::visitor::{CheckMode, PlaybookVisitor};
use jetpack::provisioners::{ProvisionConfig, ProvisionResult};
use serde_yaml::Value;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, RwLock};

type ProvisionFn = fn(
    &ProvisionConfig,
    &str,
    &Arc<RwLock<Inventory>>,
    Option<&DnsConfig>,
    Option<&OutputHandlerRef>,
) -> Result<ProvisionResult, String>;

fn make_run_state(check_mode: bool) -> Arc<RunState> {
    let parser = CliParser::new();
    let mode = if check_mode {
        CheckMode::Yes
    } else {
        CheckMode::No
    };
    Arc::new(RunState {
        inventory: Arc::new(RwLock::new(Inventory::new())),
        playbook_paths: Arc::new(RwLock::new(Vec::new())),
        role_paths: Arc::new(RwLock::new(Vec::new())),
        module_paths: Arc::new(RwLock::new(Vec::new())),
        limit_hosts: Vec::new(),
        limit_groups: Vec::new(),
        batch_size: None,
        context: Arc::new(RwLock::new(PlaybookContext::new(&parser))),
        visitor: Arc::new(RwLock::new(PlaybookVisitor::new(mode))),
        connection_factory: Arc::new(RwLock::new(NoFactory::new())),
        tags: None,
        allow_localhost_delegation: false,
        is_pull_mode: false,
        syntax_mode: false,
        play_groups: None,
        processed_role_tasks: Arc::new(RwLock::new(HashSet::new())),
        processed_role_handlers: Arc::new(RwLock::new(HashSet::new())),
        role_processing_stack: Arc::new(RwLock::new(Vec::new())),
        output_handler: None,
        async_mode: false,
        playbook_contents: Vec::new(),
        fetched_files: Arc::new(Mutex::new(HashMap::new())),
    })
}

fn provision_config(ip: &str) -> ProvisionConfig {
    let yaml =
        format!("type: proxmox_vm\ncluster: test\nstate: present\nssh_user: root\nip: {ip}\n");
    serde_yaml::from_str(&yaml).unwrap()
}

fn host_with_provision(name: &str, ip: &str) -> Arc<RwLock<Host>> {
    let mut h = Host::new(name);
    h.set_provision(provision_config(ip));
    Arc::new(RwLock::new(h))
}

fn host_without_provision(name: &str) -> Arc<RwLock<Host>> {
    Arc::new(RwLock::new(Host::new(name)))
}

fn ssh_var(host: &Arc<RwLock<Host>>, key: &str) -> Option<String> {
    let vars = host.read().unwrap().get_variables();
    vars.get(Value::String(key.to_string()))
        .and_then(|v| v.as_str().map(|s| s.to_string()))
}

fn fake_created(
    _: &ProvisionConfig,
    _: &str,
    _: &Arc<RwLock<Inventory>>,
    _: Option<&DnsConfig>,
    _: Option<&OutputHandlerRef>,
) -> Result<ProvisionResult, String> {
    Ok(ProvisionResult::Created)
}

fn fake_destroyed(
    _: &ProvisionConfig,
    _: &str,
    _: &Arc<RwLock<Inventory>>,
    _: Option<&DnsConfig>,
    _: Option<&OutputHandlerRef>,
) -> Result<ProvisionResult, String> {
    Ok(ProvisionResult::Destroyed)
}

fn fake_fails(
    _: &ProvisionConfig,
    _: &str,
    _: &Arc<RwLock<Inventory>>,
    _: Option<&DnsConfig>,
    _: Option<&OutputHandlerRef>,
) -> Result<ProvisionResult, String> {
    Err("provision blew up".to_string())
}

#[test]
fn ready_outcome_records_ssh_hostname_and_user() {
    let rs = make_run_state(false);
    let host = host_with_provision("node1", "10.0.0.5");

    let outcome = provision_host_with(&rs, &host, fake_created as ProvisionFn);

    assert!(matches!(outcome, ProvisionOutcome::Ready));
    assert_eq!(
        ssh_var(&host, "jet_ssh_hostname").as_deref(),
        Some("10.0.0.5")
    );
    assert_eq!(ssh_var(&host, "jet_ssh_user").as_deref(), Some("root"));
}

#[test]
fn destroyed_outcome_does_not_set_ssh_vars() {
    let rs = make_run_state(false);
    let host = host_with_provision("node1", "10.0.0.5");

    let outcome = provision_host_with(&rs, &host, fake_destroyed as ProvisionFn);

    assert!(matches!(outcome, ProvisionOutcome::Destroyed));
    assert!(ssh_var(&host, "jet_ssh_hostname").is_none());
    assert!(ssh_var(&host, "jet_ssh_user").is_none());
}

#[test]
fn failed_outcome_preserves_error_message() {
    let rs = make_run_state(false);
    let host = host_with_provision("node1", "10.0.0.5");

    let outcome = provision_host_with(&rs, &host, fake_fails as ProvisionFn);

    match outcome {
        ProvisionOutcome::Failed(msg) => assert_eq!(msg, "provision blew up"),
        other => panic!("expected Failed, got {other:?}"),
    }
}

#[test]
fn host_without_provision_is_ready_without_calling_provision() {
    let rs = make_run_state(false);
    let host = host_without_provision("node1");

    let calls = Arc::new(AtomicU32::new(0));
    let calls_clone = Arc::clone(&calls);
    let fake = move |_: &ProvisionConfig,
                     _: &str,
                     _: &Arc<RwLock<Inventory>>,
                     _: Option<&DnsConfig>,
                     _: Option<&OutputHandlerRef>|
          -> Result<ProvisionResult, String> {
        calls_clone.fetch_add(1, Ordering::SeqCst);
        Ok(ProvisionResult::Created)
    };

    let outcome = provision_host_with(&rs, &host, fake);

    assert!(matches!(outcome, ProvisionOutcome::Ready));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn check_mode_is_ready_without_calling_provision() {
    let rs = make_run_state(true);
    let host = host_with_provision("node1", "10.0.0.5");

    let calls = Arc::new(AtomicU32::new(0));
    let calls_clone = Arc::clone(&calls);
    let fake = move |_: &ProvisionConfig,
                     _: &str,
                     _: &Arc<RwLock<Inventory>>,
                     _: Option<&DnsConfig>,
                     _: Option<&OutputHandlerRef>|
          -> Result<ProvisionResult, String> {
        calls_clone.fetch_add(1, Ordering::SeqCst);
        Ok(ProvisionResult::Created)
    };

    let outcome = provision_host_with(&rs, &host, fake);

    assert!(matches!(outcome, ProvisionOutcome::Ready));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn apply_outcomes_excludes_destroyed_keeps_ready() {
    let rs = make_run_state(false);
    let h1 = host_without_provision("node1");
    let h2 = host_without_provision("node2");
    let hosts = vec![Arc::clone(&h1), Arc::clone(&h2)];
    rs.context.write().unwrap().set_targetted_hosts(&hosts);
    let outcomes = vec![ProvisionOutcome::Ready, ProvisionOutcome::Destroyed];

    let destroyed = apply_provision_outcomes(&rs.context, &hosts, &outcomes).unwrap();

    assert_eq!(destroyed, 1);
    let remaining = rs.context.read().unwrap().get_remaining_hosts();
    assert_eq!(remaining.len(), 1);
    assert!(remaining.contains_key("node1"));
    assert!(!remaining.contains_key("node2"));
}

#[test]
fn apply_outcomes_all_destroyed_empties_pool_and_counts() {
    let rs = make_run_state(false);
    let h1 = host_without_provision("node1");
    let h2 = host_without_provision("node2");
    let hosts = vec![Arc::clone(&h1), Arc::clone(&h2)];
    rs.context.write().unwrap().set_targetted_hosts(&hosts);
    let outcomes = vec![ProvisionOutcome::Destroyed, ProvisionOutcome::Destroyed];

    let destroyed = apply_provision_outcomes(&rs.context, &hosts, &outcomes).unwrap();

    assert_eq!(destroyed, 2);
    assert!(rs.context.read().unwrap().get_remaining_hosts().is_empty());
}

#[test]
fn apply_outcomes_aborts_on_first_failure() {
    let rs = make_run_state(false);
    let h1 = host_without_provision("node1");
    let hosts = vec![Arc::clone(&h1)];
    rs.context.write().unwrap().set_targetted_hosts(&hosts);
    let outcomes = vec![ProvisionOutcome::Failed("provision blew up".to_string())];

    let result = apply_provision_outcomes(&rs.context, &hosts, &outcomes);

    let err = result.unwrap_err();
    assert!(err.contains("node1"), "got: {err}");
    assert!(err.contains("provision blew up"), "got: {err}");
}
