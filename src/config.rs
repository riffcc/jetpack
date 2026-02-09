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

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

/// Configuration for running JetPack playbooks programmatically
#[derive(Clone, Debug)]
pub struct JetpackConfig {
    pub playbook_paths: Arc<RwLock<Vec<PathBuf>>>,
    pub inventory_paths: Arc<RwLock<Vec<PathBuf>>>,
    pub role_paths: Arc<RwLock<Vec<PathBuf>>>,
    pub module_paths: Arc<RwLock<Vec<PathBuf>>>,
    pub limit_groups: Vec<String>,
    pub limit_hosts: Vec<String>,
    pub batch_size: Option<usize>,
    pub default_user: String,
    pub sudo: Option<String>,
    pub default_port: i64,
    pub threads: usize,
    pub verbosity: u32,
    pub tags: Option<Vec<String>>,
    pub allow_localhost_delegation: bool,
    pub extra_vars: serde_yaml::Value,
    pub forward_agent: bool,
    pub login_password: Option<String>,
    pub private_key_file: Option<String>,
    pub check_mode: bool,
    pub async_mode: bool,
    pub connection_mode: ConnectionMode,
    pub playbook_contents: Vec<(String, String)>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ConnectionMode {
    Local,
    Ssh,
    Simulate,
}

impl Default for JetpackConfig {
    fn default() -> Self {
        Self {
            playbook_paths: Arc::new(RwLock::new(Vec::new())),
            inventory_paths: Arc::new(RwLock::new(Vec::new())),
            role_paths: Arc::new(RwLock::new(vec![
                PathBuf::from("./roles"),
                PathBuf::from("/usr/share/jetpack/roles"),
            ])),
            module_paths: Arc::new(RwLock::new(vec![
                PathBuf::from("./library"), 
                PathBuf::from("/usr/share/jetpack/library"),
            ])),
            limit_groups: Vec::new(),
            limit_hosts: Vec::new(),
            batch_size: None,
            default_user: String::from("root"),
            sudo: None,
            default_port: 22,
            threads: 1,
            verbosity: 0,
            tags: None,
            allow_localhost_delegation: false,
            extra_vars: serde_yaml::Value::Mapping(Default::default()),
            forward_agent: false,
            login_password: None,
            private_key_file: None,
            check_mode: false,
            async_mode: false,
            connection_mode: ConnectionMode::Ssh,
            playbook_contents: Vec::new(),
        }
    }
}

impl JetpackConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn playbook<P: Into<PathBuf>>(self, path: P) -> Self {
        self.playbook_paths.write().unwrap().push(path.into());
        self
    }

    pub fn inventory<P: Into<PathBuf>>(self, path: P) -> Self {
        self.inventory_paths.write().unwrap().push(path.into());
        self
    }

    pub fn role_path<P: Into<PathBuf>>(self, path: P) -> Self {
        self.role_paths.write().unwrap().push(path.into());
        self
    }

    pub fn module_path<P: Into<PathBuf>>(self, path: P) -> Self {
        self.module_paths.write().unwrap().push(path.into());
        self
    }

    pub fn limit_hosts(mut self, hosts: Vec<String>) -> Self {
        self.limit_hosts = hosts;
        self
    }

    pub fn limit_groups(mut self, groups: Vec<String>) -> Self {
        self.limit_groups = groups;
        self
    }

    pub fn batch_size(mut self, size: usize) -> Self {
        self.batch_size = Some(size);
        self
    }

    pub fn user(mut self, user: String) -> Self {
        self.default_user = user;
        self
    }

    pub fn sudo(mut self, sudo: String) -> Self {
        self.sudo = Some(sudo);
        self
    }

    pub fn port(mut self, port: i64) -> Self {
        self.default_port = port;
        self
    }

    pub fn threads(mut self, threads: usize) -> Self {
        self.threads = threads;
        self
    }

    pub fn verbosity(mut self, verbosity: u32) -> Self {
        self.verbosity = verbosity;
        self
    }

    pub fn tags(mut self, tags: Vec<String>) -> Self {
        self.tags = Some(tags);
        self
    }

    pub fn extra_vars(mut self, vars: serde_yaml::Value) -> Self {
        self.extra_vars = vars;
        self
    }

    pub fn forward_agent(mut self, forward: bool) -> Self {
        self.forward_agent = forward;
        self
    }

    pub fn check_mode(mut self, check: bool) -> Self {
        self.check_mode = check;
        self
    }

    pub fn async_mode(mut self, async_mode: bool) -> Self {
        self.async_mode = async_mode;
        self
    }

    pub fn playbook_content(mut self, name: impl Into<String>, yaml: impl Into<String>) -> Self {
        self.playbook_contents.push((name.into(), yaml.into()));
        self
    }

    pub fn connection_mode(mut self, mode: ConnectionMode) -> Self {
        self.connection_mode = mode;
        self
    }

    pub fn local(mut self) -> Self {
        self.connection_mode = ConnectionMode::Local;
        self
    }

    pub fn ssh(mut self) -> Self {
        self.connection_mode = ConnectionMode::Ssh;
        self
    }

    pub fn simulate(mut self) -> Self {
        self.connection_mode = ConnectionMode::Simulate;
        self
    }
    
    pub fn verbose(mut self) -> Self {
        self.verbosity = 1;
        self
    }

    pub fn login_password(mut self, password: String) -> Self {
        self.login_password = Some(password);
        self
    }

    pub fn private_key_file(mut self, path: String) -> Self {
        self.private_key_file = Some(path);
        self
    }
}