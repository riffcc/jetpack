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

use crate::inventory::hosts::Host;
use crate::tasks::request::TaskRequest;
use crate::tasks::response::TaskResponse;
use std::sync::Arc;
use inline_colorization::{color_red, color_green, color_yellow, color_reset};

/// Trait for handling output from JetPack operations
pub trait OutputHandler: Send + Sync {
    fn on_playbook_start(&self, playbook_path: &str);
    fn on_playbook_end(&self, playbook_path: &str, success: bool);
    
    fn on_play_start(&self, play_name: &str, hosts: Vec<String>);
    fn on_play_end(&self, play_name: &str);
    
    fn on_task_start(&self, task_name: &str, host_count: usize);
    fn on_task_host_result(&self, host: &Host, task: &TaskRequest, response: &TaskResponse);
    fn on_task_end(&self, task_name: &str);
    
    fn on_handler_start(&self, handler_name: &str);
    fn on_handler_end(&self, handler_name: &str);
    
    fn on_recap(&self, recap_data: RecapData);

    // Provisioning lifecycle events
    fn on_provision_created(&self, _host: &str) {}
    fn on_provision_exists(&self, _host: &str) {}
    fn on_provision_ssh_wait(&self, _host: &str, _ip: &str, _timeout_secs: u64) {}
    fn on_provision_ssh_ready(&self, _host: &str, _elapsed_secs: u64, _attempts: u32) {}
    fn on_provision_destroyed(&self, _host: &str) {}

    fn log(&self, level: LogLevel, message: &str);
    fn debug(&self, message: &str) {
        self.log(LogLevel::Debug, message);
    }
    fn info(&self, message: &str) {
        self.log(LogLevel::Info, message);
    }
    fn warning(&self, message: &str) {
        self.log(LogLevel::Warning, message);
    }
    fn error(&self, message: &str) {
        self.log(LogLevel::Error, message);
    }
}

#[derive(Debug, Clone)]
pub enum LogLevel {
    Debug,
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone)]
pub struct RecapData {
    pub host: String,
    pub ok: usize,
    pub changed: usize,
    pub unreachable: usize,
    pub failed: usize,
    pub skipped: usize,
}

/// A no-op output handler for when output is not needed
pub struct NullOutputHandler;

impl OutputHandler for NullOutputHandler {
    fn on_playbook_start(&self, _playbook_path: &str) {}
    fn on_playbook_end(&self, _playbook_path: &str, _success: bool) {}
    fn on_play_start(&self, _play_name: &str, _hosts: Vec<String>) {}
    fn on_play_end(&self, _play_name: &str) {}
    fn on_task_start(&self, _task_name: &str, _host_count: usize) {}
    fn on_task_host_result(&self, _host: &Host, _task: &TaskRequest, _response: &TaskResponse) {}
    fn on_task_end(&self, _task_name: &str) {}
    fn on_handler_start(&self, _handler_name: &str) {}
    fn on_handler_end(&self, _handler_name: &str) {}
    fn on_recap(&self, _recap_data: RecapData) {}
    fn log(&self, _level: LogLevel, _message: &str) {}
}

/// Standard terminal output handler that mimics the original CLI output
pub struct TerminalOutputHandler {
    pub verbosity: u32,
}

impl TerminalOutputHandler {
    pub fn new(verbosity: u32) -> Self {
        Self { verbosity }
    }
}

impl OutputHandler for TerminalOutputHandler {
    fn on_playbook_start(&self, playbook_path: &str) {
        crate::util::terminal::banner(&format!("PLAYBOOK: {}", playbook_path));
        println!();
    }
    
    fn on_playbook_end(&self, _playbook_path: &str, success: bool) {
        if !success {
            println!("\n{color_red}PLAYBOOK FAILED{color_reset}");
        }
    }
    
    fn on_play_start(&self, play_name: &str, hosts: Vec<String>) {
        println!();
        crate::util::terminal::banner(&format!("PLAY: {} => {}", play_name, hosts.join(", ")));
    }
    
    fn on_play_end(&self, _play_name: &str) {}
    
    fn on_task_start(&self, task_name: &str, _host_count: usize) {
        println!();
        crate::util::terminal::banner(&format!("TASK: {}", task_name));
    }
    
    fn on_task_host_result(&self, host: &Host, _task: &TaskRequest, response: &TaskResponse) {
        use crate::tasks::response::TaskStatus;
        let status = match &response.status {
            TaskStatus::IsModified | TaskStatus::IsCreated | TaskStatus::IsRemoved | TaskStatus::IsExecuted => {
                format!("{color_yellow}CHANGED{color_reset}")
            },
            TaskStatus::IsPassive | TaskStatus::IsMatched => {
                format!("{color_green}OK{color_reset}")
            },
            TaskStatus::Failed => {
                format!("{color_red}FAILED{color_reset}")
            },
            TaskStatus::IsSkipped => {
                format!("{color_yellow}SKIPPED{color_reset}")
            },
            _ => format!("{color_green}OK{color_reset}")
        };
        
        println!("{} => {}", status, host.name);
        
        if self.verbosity > 0 || response.status == TaskStatus::Failed {
            if let Some(msg) = &response.msg {
                println!("  {}", msg);
            }
        }
    }
    
    fn on_task_end(&self, _task_name: &str) {}
    
    fn on_handler_start(&self, handler_name: &str) {
        println!();
        crate::util::terminal::banner(&format!("HANDLER: {}", handler_name));
    }
    
    fn on_handler_end(&self, _handler_name: &str) {}
    
    fn on_recap(&self, recap_data: RecapData) {
        println!();
        crate::util::terminal::banner(&String::from("RECAP"));
        println!("{} : ok={} changed={} unreachable={} failed={} skipped={}", 
            recap_data.host,
            recap_data.ok,
            recap_data.changed,
            recap_data.unreachable,
            recap_data.failed,
            recap_data.skipped
        );
    }
    
    fn log(&self, level: LogLevel, message: &str) {
        match level {
            LogLevel::Debug if self.verbosity >= 3 => println!("DEBUG: {}", message),
            LogLevel::Info if self.verbosity >= 1 => println!("INFO: {}", message),
            LogLevel::Warning => println!("WARNING: {}", message),
            LogLevel::Error => eprintln!("ERROR: {}", message),
            _ => {}
        }
    }
}

/// Thread-safe wrapper for output handlers
pub type OutputHandlerRef = Arc<dyn OutputHandler>;