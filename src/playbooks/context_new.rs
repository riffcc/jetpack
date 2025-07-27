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

use crate::util::io::{path_as_string,directory_as_string};
use crate::playbooks::language::{Play,Role,RoleInvocation};
use std::path::PathBuf;
use std::collections::HashMap;
use crate::inventory::hosts::Host;
use std::sync::{Arc,RwLock};
use crate::connection::cache::ConnectionCache;
use crate::registry::list::Task;
use crate::util::yaml::blend_variables;
use crate::playbooks::templar::{Templar,TemplateMode};
use crate::handle::template::BlendTarget;
use std::ops::Deref;
use std::env;
use guid_create::GUID;
use expanduser::expanduser;

pub struct PlaybookContext {

    pub verbosity: u32,

    pub playbook_path: Option<String>,
    pub playbook_directory: Option<String>,
    pub play: Option<String>,
    
    pub role: Option<Role>,
    pub role_path: Option<String>,
    pub play_count: usize,
    pub role_count: usize,

    pub task_count: usize,
    pub task: Option<String>,
    
    seen_hosts:               HashMap<String, Arc<RwLock<Host>>>,
    targetted_hosts:          HashMap<String, Arc<RwLock<Host>>>,
    failed_hosts:             HashMap<String, Arc<RwLock<Host>>>,

    attempted_count_for_host: HashMap<String, usize>,
    adjusted_count_for_host:  HashMap<String, usize>,
    created_count_for_host:   HashMap<String, usize>,
    removed_count_for_host:   HashMap<String, usize>,
    modified_count_for_host:  HashMap<String, usize>,
    executed_count_for_host:  HashMap<String, usize>,
    passive_count_for_host:   HashMap<String, usize>,
    matched_count_for_host:   HashMap<String, usize>,
    skipped_count_for_host:   HashMap<String, usize>,
    failed_count_for_host:    HashMap<String, usize>,
    
    pub failed_tasks:           usize,
    pub defaults_storage:       RwLock<serde_yaml::Mapping>,
    pub vars_storage:           RwLock<serde_yaml::Mapping>,
    pub role_defaults_storage:  RwLock<serde_yaml::Mapping>,
    pub role_vars_storage:      RwLock<serde_yaml::Mapping>,
    pub env_storage:            RwLock<serde_yaml::Mapping>,
    
    pub connection_cache:     RwLock<ConnectionCache>,
    pub templar:              RwLock<Templar>,

    pub ssh_user:             String,
    pub ssh_port:             i64,
    pub sudo:                 Option<String>,
    pub sudo_template:        Option<String>,
    pub configured_sudo:      Option<String>,
    pub configured_sudo_template: Option<String>,
    pub configured_ssh_user:  Option<String>,
    pub configured_ssh_port:  Option<i64>,
    pub run_id:               String,
    pub fact_storage:         RwLock<HashMap<String,serde_yaml::Mapping>>
    
}

impl PlaybookContext {
    
    pub fn new(default_user: String, default_port: i64, sudo: Option<String>, extra_vars: serde_yaml::Value) -> Self {
        let mut s = Self {
            verbosity: 0,
            playbook_path: None,
            playbook_directory: None,
            failed_tasks: 0,
            play: None,
            role: None,
            task: None,
            play_count : 0,
            role_count : 0,
            task_count : 0,
            seen_hosts: HashMap::new(),
            targetted_hosts: HashMap::new(),
            failed_hosts: HashMap::new(),
            role_path: None,
            adjusted_count_for_host:  HashMap::new(),
            attempted_count_for_host: HashMap::new(),
            created_count_for_host:   HashMap::new(),
            removed_count_for_host:   HashMap::new(),
            modified_count_for_host:  HashMap::new(),
            executed_count_for_host:  HashMap::new(),
            passive_count_for_host:   HashMap::new(),
            matched_count_for_host:   HashMap::new(),
            failed_count_for_host:    HashMap::new(),
            skipped_count_for_host:   HashMap::new(),
            connection_cache:         RwLock::new(ConnectionCache::new()),
            templar:                  RwLock::new(Templar::new()),
            defaults_storage:         RwLock::new(serde_yaml::Mapping::new()),
            vars_storage:             RwLock::new(serde_yaml::Mapping::new()),
            role_vars_storage:        RwLock::new(serde_yaml::Mapping::new()),
            role_defaults_storage:    RwLock::new(serde_yaml::Mapping::new()),
            env_storage:              RwLock::new(serde_yaml::Mapping::new()),
            configured_ssh_user:      Some(default_user.clone()),
            configured_ssh_port:      Some(default_port),
            configured_sudo:          sudo.clone(),
            configured_sudo_template: None,
            ssh_user:                 default_user,
            ssh_port:                 default_port,
            sudo:                     sudo,
            sudo_template:            None,
            run_id:                   GUID::rand().to_string(),
            fact_storage:             RwLock::new(HashMap::new())
        };
        s.push_env_variables();
        
        // Apply extra vars if provided
        if let serde_yaml::Value::Mapping(extra_mapping) = extra_vars {
            s.push_extra_vars(extra_mapping);
        }
        
        s
    }

    // Copy all existing methods from the original context.rs below...
    // (These would be copied from the original file)
    
    pub fn set_playbook_path(&mut self, path: &PathBuf) {
        self.playbook_path = Some(path_as_string(path));
        self.playbook_directory = directory_as_string(path);
    }

    pub fn push_env_variables(&mut self) {
        let mut mapping = serde_yaml::Mapping::new();
        for (k,v) in env::vars() {
            mapping.insert(
                serde_yaml::Value::from(k.clone()),
                serde_yaml::Value::from(v.clone())
            );
        }
        self.env_storage = RwLock::new(mapping);
    }

    pub fn push_extra_vars(&mut self, vars: serde_yaml::Mapping) {
        let mut vs = self.vars_storage.write().unwrap();
        blend_variables(&mut vs, vars, BlendTarget::Variables);
    }

    pub fn is_host_failed(&self, host: &Arc<RwLock<Host>>) -> bool {
        let hg = host.read().unwrap();
        return self.failed_hosts.contains_key(&hg.name);
    }

    pub fn add_seen_host(&mut self, host: &Arc<RwLock<Host>>) {
        let hg = host.read().unwrap();
        self.seen_hosts.insert(hg.name.clone(), Arc::clone(&host));
    }

    pub fn add_targetted_host(&mut self, host: &Arc<RwLock<Host>>) {
        let hg = host.read().unwrap();
        self.targetted_hosts.insert(hg.name.clone(), Arc::clone(&host));
    }

    pub fn add_failed_host(&mut self, host: &Arc<RwLock<Host>>) {
        let hg = host.read().unwrap();
        self.failed_hosts.insert(hg.name.clone(), Arc::clone(&host));
    }

    pub fn get_failed_host_count(&self) -> usize {
        return self.failed_hosts.len();
    }

    pub fn increment_attempted_for_host(&mut self, host: &Arc<RwLock<Host>>) {
        let hg = host.read().unwrap();
        let count = self.attempted_count_for_host.entry(hg.name.clone()).or_insert(0);
        *count += 1;
    }

    pub fn increment_adjusted_for_host(&mut self, host: &Arc<RwLock<Host>>) {
        let hg = host.read().unwrap();
        let count = self.adjusted_count_for_host.entry(hg.name.clone()).or_insert(0);
        *count += 1;
    }

    pub fn increment_created_for_host(&mut self, host: &Arc<RwLock<Host>>) {
        let hg = host.read().unwrap();
        let count = self.created_count_for_host.entry(hg.name.clone()).or_insert(0);
        *count += 1;
    }

    pub fn increment_removed_for_host(&mut self, host: &Arc<RwLock<Host>>) {
        let hg = host.read().unwrap();
        let count = self.removed_count_for_host.entry(hg.name.clone()).or_insert(0);
        *count += 1;
    }

    pub fn increment_modified_for_host(&mut self, host: &Arc<RwLock<Host>>) {
        let hg = host.read().unwrap();
        let count = self.modified_count_for_host.entry(hg.name.clone()).or_insert(0);
        *count += 1;
    }

    pub fn increment_executed_for_host(&mut self, host: &Arc<RwLock<Host>>) {
        let hg = host.read().unwrap();
        let count = self.executed_count_for_host.entry(hg.name.clone()).or_insert(0);
        *count += 1;
    }

    pub fn increment_passive_for_host(&mut self, host: &Arc<RwLock<Host>>) {
        let hg = host.read().unwrap();
        let count = self.passive_count_for_host.entry(hg.name.clone()).or_insert(0);
        *count += 1;
    }

    pub fn increment_matched_for_host(&mut self, host: &Arc<RwLock<Host>>) {
        let hg = host.read().unwrap();
        let count = self.matched_count_for_host.entry(hg.name.clone()).or_insert(0);
        *count += 1;
    }

    pub fn increment_failed_for_host(&mut self, host: &Arc<RwLock<Host>>) {
        let hg = host.read().unwrap();
        let count = self.failed_count_for_host.entry(hg.name.clone()).or_insert(0);
        *count += 1;
    }

    pub fn increment_skipped_for_host(&mut self, host: &Arc<RwLock<Host>>) {
        let hg = host.read().unwrap();
        let count = self.skipped_count_for_host.entry(hg.name.clone()).or_insert(0);
        *count += 1;
    }

    pub fn get_attempted_count_for_host(&self, host: &Arc<RwLock<Host>>) -> usize {
        let hg = host.read().unwrap();
        match self.attempted_count_for_host.get(&hg.name) {
            Some(x) => x.clone(),
            None => 0
        }
    }

    pub fn set_sudo(&mut self, sudo: &Option<String>, sudo_template: &Option<String>) {
        self.sudo = match sudo {
            Some(x) => Some(x.clone()),
            None => self.configured_sudo.clone()
        };
        self.sudo_template = match sudo_template {
            Some(x) => Some(x.clone()),
            None => self.configured_sudo_template.clone()
        };
    }

    pub fn set_ssh_port(&mut self, port: &Option<i64>) {
        self.ssh_port = match port {
            Some(x) => x.clone(),
            None => match self.configured_ssh_port {
                Some(x) => x.clone(),
                None => self.ssh_port
            }
        };
    }

    pub fn set_ssh_user(&mut self, user: &Option<String>) {
        self.ssh_user = match user {
            Some(x) => x.clone(),
            None => match &self.configured_ssh_user {
                Some(x) => x.clone(),
                None => self.ssh_user.clone()
            }
        };
    }

    pub fn store_host_facts(&mut self, host: &Arc<RwLock<Host>>, facts: serde_yaml::Mapping) {
        let hg = host.read().unwrap();
        let mut storage = self.fact_storage.write().unwrap();
        storage.insert(hg.name.clone(), facts);
    }

    pub fn get_host_facts(&self, host: &Arc<RwLock<Host>>) -> Option<serde_yaml::Mapping> {
        let hg = host.read().unwrap();
        let storage = self.fact_storage.read().unwrap();
        match storage.get(&hg.name) {
            Some(x) => Some(x.clone()),
            None => None
        }
    }

    pub fn host_context(&self) -> String {
        match self.play.as_ref() {
            Some(p) => format!("{:?}", p),
            None => format!("")
        }
    }

    pub fn get_hosts_failed_count(&self) -> usize {
        return self.failed_count_for_host.keys().len();
    }

    pub fn get_hosts_adjusted_count(&self) -> usize {
        return self.adjusted_count_for_host.keys().len();
    }
}