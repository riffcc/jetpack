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

use crate::playbooks::context::PlaybookContext;
use crate::output::{OutputHandlerRef, RecapData};
use std::sync::Arc;
use crate::tasks::*;
use std::sync::RwLock;
use crate::inventory::hosts::Host;
use crate::connection::command::CommandResult;
use crate::playbooks::traversal::HandlerMode;
use std::fs::OpenOptions;
use std::io::prelude::*;
use std::fs::File;
use serde_json::json;
use guid_create::GUID;
use chrono::prelude::*;
use std::env;
use std::collections::HashMap;

#[derive(PartialEq)]
pub enum CheckMode {
    Yes,
    No
}

pub struct PlaybookVisitor {
    pub check_mode: CheckMode,
    pub logfile: Option<Arc<RwLock<File>>>,
    pub run_id: String,
    pub utc_start: DateTime<Utc>,
    pub output_handler: OutputHandlerRef,
    pub verbosity: u32,
    pub host_stats: Arc<RwLock<HashMap<String, HostStats>>>,
}

#[derive(Default, Clone)]
pub struct HostStats {
    pub ok: usize,
    pub changed: usize,
    pub unreachable: usize,
    pub failed: usize,
    pub skipped: usize,
}

pub struct LogData {
    pub event: String,
    pub play: Option<String>,
    pub playbook_path: Option<String>,
    pub role: Option<String>,
    pub task: Option<String>,
    pub task_ct: Option<usize>,
    pub cmd: Option<String>,
    pub cmd_rc: Option<i32>,
    pub cmd_out: Option<String>,
    pub task_status: Option<String>,
    pub host: Option<String>,
    pub summary: Option<serde_json::map::Map<String,serde_json::Value>>
}

impl PlaybookVisitor {

    pub fn new(verbosity: u32, check_mode: CheckMode, output_handler: OutputHandlerRef) -> Self {

        let logpath : String = match env::var("JET_LOG") {
            Ok(x) => x,
            Err(_) => String::from("/var/log/jetp/jetp.log")
        };

        let logfile : Option<Arc<RwLock<File>>> = match OpenOptions::new().write(true).create(true).append(true).open(logpath) {
            Ok(x) => Some(Arc::new(RwLock::new(x))),
            Err(_) => None
        };

        Self {
            check_mode,
            logfile,
            utc_start: Utc::now(),
            run_id: GUID::rand().to_string(),
            output_handler,
            verbosity,
            host_stats: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn log_entry(&self, event: &String, context: Arc<RwLock<PlaybookContext>>) -> LogData {
        let ctx = context.read().unwrap();
        LogData {
            event: event.clone(),
            play: ctx.play.clone(),
            playbook_path: ctx.playbook_path.clone(),
            role: match &ctx.role {
                Some(x) => Some(x.name.clone()),
                None => None
            },
            task: ctx.task.clone(),
            task_ct: None,
            cmd: None,
            cmd_rc: None,
            cmd_out: None,
            task_status: None,
            host: None,
            summary: None
        }
    }

    pub fn log(&self, _context: &Arc<RwLock<PlaybookContext>>, data: LogData) {
        match &self.logfile {
            Some(logfile) => {
                let record = json!({
                    "utc": Utc::now().to_rfc3339(),
                    "run_id": self.run_id,
                    "event": data.event,
                    "playbook_path": data.playbook_path,
                    "play": data.play,
                    "role": data.role,
                    "task": data.task,
                    "task_ct": data.task_ct,
                    "host": data.host,
                    "cmd": data.cmd,
                    "cmd_rc": data.cmd_rc,
                    "cmd_out": data.cmd_out,
                    "task_status": data.task_status,
                    "summary": data.summary
                });
                writeln!(logfile.write().unwrap(), "{}", &record.to_string()).unwrap();
            },
            None => {}
        };
    }

    pub fn on_playbook_start(&self, context: &Arc<RwLock<PlaybookContext>>) {
        let ctx = context.read().unwrap();
        if let Some(path) = &ctx.playbook_path {
            self.output_handler.on_playbook_start(path);
        }
        
        let entry = self.log_entry(&String::from("playbook/start"), Arc::clone(context));
        self.log(context, entry);
    }

    pub fn on_playbook_stop(&self, context: &Arc<RwLock<PlaybookContext>>, success: bool) {
        let ctx = context.read().unwrap();
        if let Some(path) = &ctx.playbook_path {
            self.output_handler.on_playbook_end(path, success);
        }
        
        let entry = self.log_entry(&String::from("playbook/stop"), Arc::clone(context));
        self.log(context, entry);
        
        // Show recap
        let stats = self.host_stats.read().unwrap();
        for (host, stat) in stats.iter() {
            let recap = RecapData {
                host: host.clone(),
                ok: stat.ok,
                changed: stat.changed,
                unreachable: stat.unreachable,
                failed: stat.failed,
                skipped: stat.skipped,
            };
            self.output_handler.on_recap(recap);
        }
    }

    pub fn on_play_start(&self, context: &Arc<RwLock<PlaybookContext>>, matched_hosts: Vec<Host>) {
        let ctx = context.read().unwrap();
        let host_names: Vec<String> = matched_hosts.iter().map(|h| h.name.clone()).collect();
        
        if let Some(play_name) = &ctx.play {
            self.output_handler.on_play_start(play_name, host_names);
        }
        
        let entry = self.log_entry(&String::from("play/start"), Arc::clone(context));
        self.log(context, entry);
    }

    pub fn on_play_stop(&self, context: &Arc<RwLock<PlaybookContext>>) {
        let ctx = context.read().unwrap();
        if let Some(play_name) = &ctx.play {
            self.output_handler.on_play_end(play_name);
        }
        
        let entry = self.log_entry(&String::from("play/stop"), Arc::clone(context));
        self.log(context, entry);
    }

    pub fn on_role_start(&self, context: &Arc<RwLock<PlaybookContext>>) {
        let entry = self.log_entry(&String::from("role/start"), Arc::clone(context));
        self.log(context, entry);
    }

    pub fn on_role_stop(&self, context: &Arc<RwLock<PlaybookContext>>) {
        let entry = self.log_entry(&String::from("role/stop"), Arc::clone(context));
        self.log(context, entry);
    }

    pub fn on_task_start(&self, context: &Arc<RwLock<PlaybookContext>>, mode: HandlerMode) {
        let ctx = context.read().unwrap();
        let default_name = String::from("unknown");
        let task_name = ctx.task.as_ref().unwrap_or(&default_name);
        let name = match mode {
            HandlerMode::Handlers => format!("HANDLER: {}", task_name),
            _ => task_name.clone()
        };
        
        self.output_handler.on_task_start(&name, 0);
        
        let mut entry = self.log_entry(&String::from("task/start"), Arc::clone(context));
        entry.task_ct = Some(0);
        self.log(context, entry);
    }

    pub fn on_task_stop(&self, context: &Arc<RwLock<PlaybookContext>>, mode: HandlerMode) {
        let ctx = context.read().unwrap();
        if let Some(task_name) = &ctx.task {
            match mode {
                HandlerMode::Handlers => self.output_handler.on_handler_end(task_name),
                _ => self.output_handler.on_task_end(task_name),
            }
        }
        
        let entry = self.log_entry(&String::from("task/stop"), Arc::clone(context));
        self.log(context, entry);
    }

    pub fn on_host_task_start(&self, context: &Arc<RwLock<PlaybookContext>>, _request: &request::TaskRequest, host: &Host) {
        let mut entry = self.log_entry(&String::from("host/task/start"), Arc::clone(context));
        entry.host = Some(host.name.clone());
        self.log(context, entry);
    }

    pub fn on_host_task_ok(&self, context: &Arc<RwLock<PlaybookContext>>, request: &request::TaskRequest, response: &response::TaskResponse, host: &Host) {
        self.output_handler.on_task_host_result(host, request, response);
        
        // Update stats
        let mut stats = self.host_stats.write().unwrap();
        let host_stat = stats.entry(host.name.clone()).or_insert_with(HostStats::default);
        
        use crate::tasks::response::TaskStatus;
        match &response.status {
            TaskStatus::IsSkipped => {
                host_stat.skipped += 1;
            },
            TaskStatus::IsModified | TaskStatus::IsCreated | TaskStatus::IsRemoved | TaskStatus::IsExecuted => {
                host_stat.changed += 1;
                host_stat.ok += 1;
            },
            TaskStatus::IsPassive | TaskStatus::IsMatched => {
                host_stat.ok += 1;
            },
            _ => {
                host_stat.ok += 1;
            }
        }
        
        let mut entry = self.log_entry(&String::from("host/task/ok"), Arc::clone(context));
        entry.task_status = Some(String::from("ok"));
        entry.host = Some(host.name.clone());
        self.log(context, entry);
    }

    pub fn on_host_task_failed(&self, context: &Arc<RwLock<PlaybookContext>>, request: &request::TaskRequest, response: &response::TaskResponse, host: &Host) {
        self.output_handler.on_task_host_result(host, request, response);
        
        // Update stats
        let mut stats = self.host_stats.write().unwrap();
        let host_stat = stats.entry(host.name.clone()).or_insert_with(HostStats::default);
        host_stat.failed += 1;
        
        let mut entry = self.log_entry(&String::from("host/task/failed"), Arc::clone(context));
        entry.task_status = Some(String::from("failed"));
        entry.host = Some(host.name.clone());
        self.log(context, entry);
    }

    pub fn on_command_run(&self, context: &Arc<RwLock<PlaybookContext>>, host: &Host, cmd: &String, result: &CommandResult) {
        let mut entry = self.log_entry(&String::from("cmd/run"), Arc::clone(context));
        entry.host = Some(host.name.clone());
        entry.cmd = Some(cmd.clone());
        entry.cmd_rc = Some(result.rc);
        
        if self.verbosity >= 2 {
            self.output_handler.debug(&format!("Command: {} (rc={})", cmd, result.rc));
            if !result.out.is_empty() {
                self.output_handler.debug(&format!("Output: {}", result.out));
            }
        }
        
        entry.cmd_out = Some(result.out.clone());
        self.log(context, entry);
    }

    pub fn on_unreachable_host(&self, context: &Arc<RwLock<PlaybookContext>>, host: &Host) {
        // Update stats
        let mut stats = self.host_stats.write().unwrap();
        let host_stat = stats.entry(host.name.clone()).or_insert_with(HostStats::default);
        host_stat.unreachable += 1;
        
        self.output_handler.error(&format!("Host {} is unreachable", host.name));
        
        let mut entry = self.log_entry(&String::from("host/unreachable"), Arc::clone(context));
        entry.host = Some(host.name.clone());
        self.log(context, entry);
    }
}