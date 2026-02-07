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

use serde::{Deserialize};
use crate::registry::list::Task;
use std::collections::HashMap;

// all the playbook language YAML structures!

#[derive(Debug,Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Play {
    pub name : String,
    pub groups : Vec<String>,
    pub roles : Option<Vec<RoleInvocation>>,
    pub defaults: Option<serde_yaml::Mapping>,
    pub vars : Option<serde_yaml::Mapping>,
    pub vars_files: Option<Vec<String>>,
    pub sudo: Option<String>,
    pub sudo_template: Option<String>,
    pub ssh_user : Option<String>,
    pub ssh_port : Option<i64>,
    pub tasks : Option<Vec<Task>>,
    pub handlers : Option<Vec<Task>>,
    pub batch_size : Option<usize>,
    /// Auto-generate hosts in this group before running
    pub instantiate: Option<InstantiateSpec>,
}

/// Specification for auto-generating hosts in a group
#[derive(Debug, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct InstantiateSpec {
    /// Path to inventory directory (where host_vars/ and groups/ live)
    pub inventory_path: String,
    /// Hostname pattern to expand (e.g., "fleet-{01..10}.lon.riff.cc")
    pub pattern: String,
    /// Nodes to distribute across (round-robin)
    pub nodes: Vec<String>,
    /// Provision configuration template
    pub provision: ProvisionSpec,
    /// Starting VMID (optional - Proxmox auto-assigns if not specified)
    pub vmid_start: Option<u64>,
    /// IP address template with {} placeholder (e.g., "10.7.1.{}/24")
    pub ip_template: Option<String>,
    /// Starting IP number
    pub ip_start: Option<u64>,
    /// Gateway for static IPs
    pub gateway: Option<String>,
}

/// Provision configuration for instantiate
#[derive(Debug, Deserialize, Clone)]
pub struct ProvisionSpec {
    #[serde(rename = "type")]
    pub provision_type: String,
    pub cluster: String,
    pub memory: Option<String>,
    pub cores: Option<String>,
    pub ostemplate: Option<String>,
    pub storage: Option<String>,
    pub rootfs_size: Option<String>,
    pub unprivileged: Option<String>,
    pub start_on_create: Option<String>,
    pub features: Option<String>,
    pub authorized_keys: Option<String>,
    pub ssh_user: Option<String>,
    pub nameserver: Option<String>,
    #[serde(flatten)]
    pub extra: HashMap<String, String>,
}

#[derive(Debug,Deserialize,Clone)]
#[serde(deny_unknown_fields)]
pub struct Role {
    pub name: String,
    pub defaults: Option<serde_yaml::Mapping>,
    pub dependencies: Option<Vec<String>>,
    pub tasks: Option<Vec<String>>,
    pub handlers: Option<Vec<String>>
}

#[derive(Debug,Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoleInvocation {
    pub role: String,
    pub vars: Option<serde_yaml::Mapping>,
    pub tags: Option<Vec<String>>
}

// for Task/module definitions see registry/list.rs
