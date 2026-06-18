// Test-only scaffolding for exercising module `dispatch()` logic against a
// fake connection, without SSH or a real control plane.
//
// Jetpack runs every command through the Handle/Connection abstraction so the
// same module works locally or over SSH (see CLAUDE.md "Handle System"). These
// helpers let a unit test drive that abstraction with a `RecordingConnection`
// that captures the commands a module issues and returns canned results — which
// is the only way to assert a module talks to the *host* rather than the
// control node's local filesystem.

#![cfg(test)]

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Arc, Mutex, RwLock};

use crate::cli::parser::CliParser;
use crate::connection::command::{CommandResult, Forward};
use crate::connection::connection::Connection;
use crate::connection::factory::ConnectionFactory;
use crate::connection::no::NoFactory;
use crate::handle::handle::TaskHandle;
use crate::handle::response::Response;
use crate::inventory::hosts::Host;
use crate::inventory::inventory::Inventory;
use crate::playbooks::context::PlaybookContext;
use crate::playbooks::traversal::RunState;
use crate::playbooks::visitor::{CheckMode, PlaybookVisitor};
use crate::tasks::request::{SudoDetails, TaskRequest};
use crate::tasks::response::TaskResponse;

/// A fake `Connection` that records every command it is asked to run and
/// returns a canned return code for each (default: 0). Commands are recorded in
/// a shared log so a test can inspect them after the connection has been moved
/// into a `TaskHandle`.
pub struct RecordingConnection {
    log: Arc<Mutex<Vec<String>>>,
    rc_for: Box<dyn Fn(&str) -> i32 + Send + Sync>,
}

impl RecordingConnection {
    /// Every command succeeds (rc 0).
    pub fn new() -> Self {
        Self::with_rc(|_| 0)
    }

    /// Decide the return code per command (e.g. simulate a missing file).
    pub fn with_rc<F: Fn(&str) -> i32 + Send + Sync + 'static>(rc_for: F) -> Self {
        Self {
            log: Arc::new(Mutex::new(Vec::new())),
            rc_for: Box::new(rc_for),
        }
    }

    /// Shared handle to the recorded command log, cloneable before the
    /// connection is wrapped for the handle.
    pub fn command_log(&self) -> Arc<Mutex<Vec<String>>> {
        Arc::clone(&self.log)
    }
}

impl Connection for RecordingConnection {
    fn connect(&mut self) -> Result<(), String> {
        Ok(())
    }

    fn write_data(
        &self,
        _response: &Arc<Response>,
        _request: &Arc<TaskRequest>,
        _data: &String,
        _remote_path: &String,
    ) -> Result<(), Arc<TaskResponse>> {
        Ok(())
    }

    fn copy_file(
        &self,
        _response: &Arc<Response>,
        _request: &Arc<TaskRequest>,
        _src: &Path,
        _dest: &String,
    ) -> Result<(), Arc<TaskResponse>> {
        Ok(())
    }

    fn fetch_file(
        &self,
        _response: &Arc<Response>,
        _request: &Arc<TaskRequest>,
        _remote_path: &String,
    ) -> Result<Vec<u8>, Arc<TaskResponse>> {
        Ok(Vec::new())
    }

    fn whoami(&self) -> Result<String, String> {
        Ok(String::from("root"))
    }

    fn run_command(
        &self,
        response: &Arc<Response>,
        request: &Arc<TaskRequest>,
        cmd: &String,
        _forward: Forward,
    ) -> Result<Arc<TaskResponse>, Arc<TaskResponse>> {
        self.log.lock().unwrap().push(cmd.clone());
        let rc = (self.rc_for)(cmd);
        Ok(response.command_ok(
            request,
            &Arc::new(Some(CommandResult {
                cmd: cmd.clone(),
                out: String::new(),
                rc,
            })),
        ))
    }
}

/// Build a minimal `TaskHandle` wired to the given connection, suitable for
/// driving a module's `dispatch()` in a unit test.
pub fn test_handle(connection: Arc<Mutex<dyn Connection>>) -> Arc<TaskHandle> {
    let inventory = Arc::new(RwLock::new(Inventory::new()));
    let host = Arc::new(RwLock::new(Host::new(&String::from("testhost"))));
    let parser = CliParser::new();
    let context = Arc::new(RwLock::new(PlaybookContext::new(&parser)));
    let visitor = Arc::new(RwLock::new(PlaybookVisitor::new(CheckMode::No)));
    let connection_factory: Arc<RwLock<dyn ConnectionFactory>> =
        Arc::new(RwLock::new(NoFactory::new()));

    let run_state = Arc::new(RunState {
        inventory,
        playbook_paths: Arc::new(RwLock::new(Vec::new())),
        role_paths: Arc::new(RwLock::new(Vec::new())),
        module_paths: Arc::new(RwLock::new(Vec::new())),
        limit_hosts: Vec::new(),
        limit_groups: Vec::new(),
        batch_size: None,
        context,
        visitor,
        connection_factory,
        tags: None,
        allow_localhost_delegation: false,
        is_pull_mode: false,
        syntax_mode: false,
        play_groups: None,
        output_handler: None,
        async_mode: false,
        playbook_contents: Vec::new(),
        processed_role_tasks: Arc::new(RwLock::new(HashSet::new())),
        processed_role_handlers: Arc::new(RwLock::new(HashSet::new())),
        role_processing_stack: Arc::new(RwLock::new(Vec::new())),
        fetched_files: Arc::new(Mutex::new(HashMap::new())),
    });

    Arc::new(TaskHandle::new(run_state, connection, host))
}

/// A no-sudo Query-phase `TaskRequest`.
pub fn query_request() -> Arc<TaskRequest> {
    TaskRequest::query(&SudoDetails {
        user: None,
        template: String::new(),
    })
}
