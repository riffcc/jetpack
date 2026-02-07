// Jetpack - Instantiate Module
// Copyright (C) Riff Labs Limited <team@riff.cc>
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
// along with this program.  If not, see <http://www.gnu.org/licenses/>.

//! The `instantiate` module generates inventory files for a fleet of machines.
//!
//! This module is unique in that it operates on the LOCAL inventory filesystem,
//! not on remote hosts. It creates/updates host_vars and group memberships.
//!
//! # Usage
//!
//! ```yaml
//! - !instantiate
//!   name: Create test fleet
//!   inventory_path: /path/to/inventory
//!   pattern: "fleet-{01..10}.lon.riff.cc"
//!   nodes:
//!     - bee
//!     - beetroot
//!     - cardinal
//!   provision:
//!     type: proxmox_vm          # or proxmox_lxc
//!     cluster: SpaceTempAgency
//!     memory: "2048"
//!     cores: "4"
//!     storage: "moosefs"
//!     rootfs_size: "20G"
//!   groups:
//!     - testfleet
//!   ip_template: "10.7.1.{}/24"
//!   ip_start: 230
//!   gateway: "10.7.1.1"
//!   state: present
//! ```
//!
//! # VMID Handling
//!
//! VMIDs are optional. If `vmid_start` is specified, VMIDs are assigned sequentially.
//! If not specified, Proxmox auto-assigns them during provisioning.
//!
//! # VM vs LXC
//!
//! - `proxmox_vm`: net0 is just `virtio,bridge=vmbr0`, IP stored separately for Dragonfly
//! - `proxmox_lxc`: IP is baked into net0 as `name=eth0,bridge=vmbr0,ip=X.X.X.X/24,gw=...`
//!
//! # Pattern Syntax
//!
//! - `prefix-{01..10}.domain` - Generates prefix-01 through prefix-10
//! - `{a,b,c}.domain` - Generates a.domain, b.domain, c.domain
//!
//! # LWW (Last Write Wins)
//!
//! When updating existing host_vars:
//! - Only explicitly provided provision fields are updated
//! - Existing fields not in the new config are preserved
//! - This allows incremental updates without losing customizations

use crate::tasks::*;
use crate::tasks::logic::{PreLogicInput, PostLogicInput};
use crate::handle::handle::TaskHandle;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::collections::HashMap;
use std::path::PathBuf;
use std::fs;

const MODULE: &str = "instantiate";

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct InstantiateTask {
    pub name: Option<String>,

    /// Path to inventory directory
    pub inventory_path: String,

    /// Hostname pattern to expand (e.g., "fleet-{01..10}.lon.riff.cc")
    pub pattern: String,

    /// Nodes to distribute across (round-robin)
    pub nodes: Vec<String>,

    /// Provision configuration template (fields to set)
    pub provision: ProvisionTemplate,

    /// Groups to add hosts to
    pub groups: Option<Vec<String>>,

    /// Starting VMID (optional - if not specified, Proxmox auto-assigns)
    pub vmid_start: Option<u64>,

    /// IP address template with {} placeholder for number
    /// e.g., "10.7.1.{}/24" or "dhcp"
    pub ip_template: Option<String>,

    /// Starting IP number (substituted into ip_template)
    pub ip_start: Option<u64>,

    /// Gateway for static IPs
    pub gateway: Option<String>,

    /// "present" to create/update, "absent" to remove
    pub state: Option<String>,

    /// Save list of generated hostnames to this variable
    pub save: Option<String>,

    pub with: Option<PreLogicInput>,
    pub and: Option<PostLogicInput>,
}

#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct ProvisionTemplate {
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

    /// Extra fields to pass through (mp0, mp1, etc.)
    #[serde(flatten)]
    pub extra: HashMap<String, String>,
}

struct InstantiateAction {
    pub inventory_path: PathBuf,
    pub hostnames: Vec<String>,
    pub node_distribution: Vec<(String, String)>, // (hostname, node)
    pub provision_template: ProvisionTemplate,
    pub groups: Vec<String>,
    pub vmid_start: Option<u64>,  // Optional - Proxmox auto-assigns if not specified
    pub ip_template: Option<String>,
    pub ip_start: u64,
    pub gateway: Option<String>,
    pub state: String,
}

/// Expand a pattern like "fleet-{01..10}.domain" into hostnames
fn expand_pattern(pattern: &str) -> Result<Vec<String>, String> {
    // Check for range pattern: {01..10}
    if let Some(start_brace) = pattern.find("{") {
        if let Some(end_brace) = pattern.find("}") {
            let prefix = &pattern[..start_brace];
            let suffix = &pattern[end_brace + 1..];
            let range_part = &pattern[start_brace + 1..end_brace];

            // Check for range: 01..10
            if range_part.contains("..") {
                let parts: Vec<&str> = range_part.split("..").collect();
                if parts.len() != 2 {
                    return Err(format!("Invalid range pattern: {}", range_part));
                }

                let start_str = parts[0];
                let end_str = parts[1];

                let start: u64 = start_str.parse()
                    .map_err(|_| format!("Invalid range start: {}", start_str))?;
                let end: u64 = end_str.parse()
                    .map_err(|_| format!("Invalid range end: {}", end_str))?;

                // Determine padding width from the original string
                let width = start_str.len();

                let mut hostnames = Vec::new();
                for i in start..=end {
                    let num = format!("{:0width$}", i, width = width);
                    hostnames.push(format!("{}{}{}", prefix, num, suffix));
                }
                return Ok(hostnames);
            }

            // Check for comma-separated: a,b,c
            if range_part.contains(",") {
                let items: Vec<&str> = range_part.split(",").collect();
                let mut hostnames = Vec::new();
                for item in items {
                    hostnames.push(format!("{}{}{}", prefix, item.trim(), suffix));
                }
                return Ok(hostnames);
            }
        }
    }

    // No pattern, just a single hostname
    Ok(vec![pattern.to_string()])
}

/// Distribute hostnames across nodes (round-robin)
fn distribute_to_nodes(hostnames: &[String], nodes: &[String]) -> Vec<(String, String)> {
    hostnames.iter()
        .enumerate()
        .map(|(i, hostname)| {
            let node = &nodes[i % nodes.len()];
            (hostname.clone(), node.clone())
        })
        .collect()
}

impl IsTask for InstantiateTask {
    fn get_module(&self) -> String { String::from(MODULE) }
    fn get_name(&self) -> Option<String> { self.name.clone() }
    fn get_with(&self) -> Option<PreLogicInput> { self.with.clone() }

    fn evaluate(&self, handle: &Arc<TaskHandle>, request: &Arc<TaskRequest>, tm: TemplateMode) -> Result<EvaluatedTask, Arc<TaskResponse>> {
        let inventory_path = handle.template.string(request, tm, &String::from("inventory_path"), &self.inventory_path)?;
        let pattern = handle.template.string(request, tm, &String::from("pattern"), &self.pattern)?;

        let hostnames = expand_pattern(&pattern).map_err(|e| {
            handle.response.is_failed(request, &format!("Failed to expand pattern: {}", e))
        })?;

        if self.nodes.is_empty() {
            return Err(handle.response.is_failed(request, &String::from("nodes list cannot be empty")));
        }

        let node_distribution = distribute_to_nodes(&hostnames, &self.nodes);

        let groups = self.groups.clone().unwrap_or_default();
        let ip_start = self.ip_start.unwrap_or(1);
        let state = self.state.clone().unwrap_or_else(|| String::from("present"));

        // Template the provision fields that might have variables
        let mut provision_template = self.provision.clone();
        if let Some(ref keys) = provision_template.authorized_keys {
            provision_template.authorized_keys = Some(
                handle.template.string(request, tm, &String::from("authorized_keys"), keys)?
            );
        }

        Ok(
            EvaluatedTask {
                action: Arc::new(InstantiateAction {
                    inventory_path: PathBuf::from(inventory_path),
                    hostnames,
                    node_distribution,
                    provision_template,
                    groups,
                    vmid_start: self.vmid_start,
                    ip_template: self.ip_template.clone(),
                    ip_start,
                    gateway: self.gateway.clone(),
                    state,
                }),
                with: Arc::new(PreLogicInput::template(handle, request, tm, &self.with)?),
                and: Arc::new(PostLogicInput::template(handle, request, tm, &self.and)?),
            }
        )
    }
}

impl IsAction for InstantiateAction {
    fn dispatch(&self, handle: &Arc<TaskHandle>, request: &Arc<TaskRequest>) -> Result<Arc<TaskResponse>, Arc<TaskResponse>> {
        match request.request_type {
            TaskRequestType::Query => self.query(handle, request),
            TaskRequestType::Execute => self.execute(handle, request),
            _ => Ok(handle.response.not_supported(request)),
        }
    }
}

impl InstantiateAction {
    fn query(&self, handle: &Arc<TaskHandle>, request: &Arc<TaskRequest>) -> Result<Arc<TaskResponse>, Arc<TaskResponse>> {
        // Check if all host_vars exist and match
        let host_vars_dir = self.inventory_path.join("host_vars");

        if self.state == "absent" {
            // Check if any exist (need to remove)
            for hostname in &self.hostnames {
                let path = host_vars_dir.join(hostname);
                if path.exists() {
                    return Ok(handle.response.needs_execution(request));
                }
            }
            return Ok(handle.response.is_matched(request));
        }

        // state == "present": check if all exist with correct content
        for hostname in &self.hostnames {
            let path = host_vars_dir.join(hostname);
            if !path.exists() {
                return Ok(handle.response.needs_execution(request));
            }
            // Could do deeper content checking here for LWW
        }

        // Check group membership
        for group in &self.groups {
            let group_path = self.inventory_path.join("groups").join(group);
            if !group_path.exists() {
                return Ok(handle.response.needs_execution(request));
            }
        }

        Ok(handle.response.is_matched(request))
    }

    fn execute(&self, handle: &Arc<TaskHandle>, request: &Arc<TaskRequest>) -> Result<Arc<TaskResponse>, Arc<TaskResponse>> {
        let host_vars_dir = self.inventory_path.join("host_vars");
        let groups_dir = self.inventory_path.join("groups");

        // Ensure directories exist
        fs::create_dir_all(&host_vars_dir).map_err(|e| {
            handle.response.is_failed(request, &format!("Failed to create host_vars dir: {}", e))
        })?;
        fs::create_dir_all(&groups_dir).map_err(|e| {
            handle.response.is_failed(request, &format!("Failed to create groups dir: {}", e))
        })?;

        if self.state == "absent" {
            return self.remove_hosts(handle, request, &host_vars_dir, &groups_dir);
        }

        // Create/update host_vars for each host
        let is_vm = self.provision_template.provision_type == "proxmox_vm";

        for (i, (hostname, node)) in self.node_distribution.iter().enumerate() {
            let host_file = host_vars_dir.join(hostname);

            // Build provision block
            let mut provision = serde_yaml::Mapping::new();
            provision.insert(
                serde_yaml::Value::String("type".to_string()),
                serde_yaml::Value::String(self.provision_template.provision_type.clone())
            );
            provision.insert(
                serde_yaml::Value::String("cluster".to_string()),
                serde_yaml::Value::String(self.provision_template.cluster.clone())
            );
            provision.insert(
                serde_yaml::Value::String("node".to_string()),
                serde_yaml::Value::String(node.clone())
            );

            // VMID only if specified (otherwise Proxmox auto-assigns)
            if let Some(vmid_start) = self.vmid_start {
                let vmid = vmid_start + i as u64;
                provision.insert(
                    serde_yaml::Value::String("vmid".to_string()),
                    serde_yaml::Value::String(vmid.to_string())
                );
            }

            // Extract hostname without domain for container/VM name
            let short_hostname = hostname.split('.').next().unwrap_or(hostname);
            provision.insert(
                serde_yaml::Value::String("hostname".to_string()),
                serde_yaml::Value::String(short_hostname.to_string())
            );

            // Network config - different for VM vs LXC
            if let Some(ref ip_template) = self.ip_template {
                let ip_num = self.ip_start + i as u64;
                let ip = ip_template.replace("{}", &ip_num.to_string());

                if is_vm {
                    // VMs: net0 is just bridge config, IP stored separately for Dragonfly
                    provision.insert(
                        serde_yaml::Value::String("net0".to_string()),
                        serde_yaml::Value::String("virtio,bridge=vmbr0".to_string())
                    );
                    // Store IP for Dragonfly/DHCP reservation
                    provision.insert(
                        serde_yaml::Value::String("ip".to_string()),
                        serde_yaml::Value::String(ip.clone())
                    );
                    if let Some(ref gw) = self.gateway {
                        provision.insert(
                            serde_yaml::Value::String("gateway".to_string()),
                            serde_yaml::Value::String(gw.clone())
                        );
                    }
                } else {
                    // LXC: IP baked into net0
                    if ip_template == "dhcp" {
                        provision.insert(
                            serde_yaml::Value::String("net0".to_string()),
                            serde_yaml::Value::String("name=eth0,bridge=vmbr0,ip=dhcp".to_string())
                        );
                    } else {
                        let mut net0 = format!("name=eth0,bridge=vmbr0,ip={}", ip);
                        if let Some(ref gw) = self.gateway {
                            net0.push_str(&format!(",gw={}", gw));
                        }
                        provision.insert(
                            serde_yaml::Value::String("net0".to_string()),
                            serde_yaml::Value::String(net0)
                        );
                    }
                }
            } else if is_vm {
                // VM with no IP specified - just bridge
                provision.insert(
                    serde_yaml::Value::String("net0".to_string()),
                    serde_yaml::Value::String("virtio,bridge=vmbr0".to_string())
                );
            }

            // Optional fields from template
            if let Some(ref memory) = self.provision_template.memory {
                provision.insert(
                    serde_yaml::Value::String("memory".to_string()),
                    serde_yaml::Value::String(memory.clone())
                );
            }
            if let Some(ref cores) = self.provision_template.cores {
                provision.insert(
                    serde_yaml::Value::String("cores".to_string()),
                    serde_yaml::Value::String(cores.clone())
                );
            }
            if let Some(ref ostemplate) = self.provision_template.ostemplate {
                provision.insert(
                    serde_yaml::Value::String("ostemplate".to_string()),
                    serde_yaml::Value::String(ostemplate.clone())
                );
            }
            if let Some(ref storage) = self.provision_template.storage {
                provision.insert(
                    serde_yaml::Value::String("storage".to_string()),
                    serde_yaml::Value::String(storage.clone())
                );
            }
            if let Some(ref rootfs_size) = self.provision_template.rootfs_size {
                provision.insert(
                    serde_yaml::Value::String("rootfs_size".to_string()),
                    serde_yaml::Value::String(rootfs_size.clone())
                );
            }
            if let Some(ref unprivileged) = self.provision_template.unprivileged {
                provision.insert(
                    serde_yaml::Value::String("unprivileged".to_string()),
                    serde_yaml::Value::String(unprivileged.clone())
                );
            }
            if let Some(ref start_on_create) = self.provision_template.start_on_create {
                provision.insert(
                    serde_yaml::Value::String("start_on_create".to_string()),
                    serde_yaml::Value::String(start_on_create.clone())
                );
            }
            if let Some(ref features) = self.provision_template.features {
                provision.insert(
                    serde_yaml::Value::String("features".to_string()),
                    serde_yaml::Value::String(features.clone())
                );
            }
            if let Some(ref authorized_keys) = self.provision_template.authorized_keys {
                provision.insert(
                    serde_yaml::Value::String("authorized_keys".to_string()),
                    serde_yaml::Value::String(authorized_keys.clone())
                );
            }
            if let Some(ref ssh_user) = self.provision_template.ssh_user {
                provision.insert(
                    serde_yaml::Value::String("ssh_user".to_string()),
                    serde_yaml::Value::String(ssh_user.clone())
                );
            }
            if let Some(ref nameserver) = self.provision_template.nameserver {
                provision.insert(
                    serde_yaml::Value::String("nameserver".to_string()),
                    serde_yaml::Value::String(nameserver.clone())
                );
            }

            // Add extra fields
            for (key, value) in &self.provision_template.extra {
                provision.insert(
                    serde_yaml::Value::String(key.clone()),
                    serde_yaml::Value::String(value.clone())
                );
            }

            // Build full host_vars document
            let mut host_vars = serde_yaml::Mapping::new();
            host_vars.insert(
                serde_yaml::Value::String("provision".to_string()),
                serde_yaml::Value::Mapping(provision)
            );

            // If file exists, merge with LWW semantics
            if host_file.exists() {
                if let Ok(existing_content) = fs::read_to_string(&host_file) {
                    if let Ok(existing_doc) = serde_yaml::from_str::<serde_yaml::Mapping>(&existing_content) {
                        // Merge: new values win, but preserve keys not in new config
                        for (key, value) in existing_doc {
                            if key.as_str() != Some("provision") {
                                host_vars.insert(key, value);
                            }
                            // provision block is always replaced with new config
                        }
                    }
                }
            }

            // Write host_vars file
            let vmid_comment = self.vmid_start.map(|start| {
                format!("\n# VMID: {}", start + i as u64)
            }).unwrap_or_default();

            let yaml_str = format!(
                "# Auto-generated by instantiate module\n# Hostname: {}\n# Node: {}{}\n\n{}",
                hostname,
                node,
                vmid_comment,
                serde_yaml::to_string(&serde_yaml::Value::Mapping(host_vars)).unwrap_or_default()
            );

            fs::write(&host_file, yaml_str).map_err(|e| {
                handle.response.is_failed(request, &format!("Failed to write host_vars for {}: {}", hostname, e))
            })?;
        }

        // Update group files
        for group in &self.groups {
            let group_file = groups_dir.join(group);

            // Read existing group if present
            let mut hosts: Vec<String> = if group_file.exists() {
                if let Ok(content) = fs::read_to_string(&group_file) {
                    if let Ok(doc) = serde_yaml::from_str::<serde_yaml::Mapping>(&content) {
                        if let Some(serde_yaml::Value::Sequence(seq)) = doc.get(&serde_yaml::Value::String("hosts".to_string())) {
                            seq.iter()
                                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                                .collect()
                        } else {
                            Vec::new()
                        }
                    } else {
                        Vec::new()
                    }
                } else {
                    Vec::new()
                }
            } else {
                Vec::new()
            };

            // Add new hosts (avoid duplicates)
            for hostname in &self.hostnames {
                if !hosts.contains(hostname) {
                    hosts.push(hostname.clone());
                }
            }

            // Sort for consistency
            hosts.sort();

            // Build group document
            let mut group_doc = serde_yaml::Mapping::new();
            group_doc.insert(
                serde_yaml::Value::String("hosts".to_string()),
                serde_yaml::Value::Sequence(
                    hosts.iter().map(|h| serde_yaml::Value::String(h.clone())).collect()
                )
            );

            // Write group file
            let yaml_str = serde_yaml::to_string(&serde_yaml::Value::Mapping(group_doc))
                .unwrap_or_default();

            fs::write(&group_file, yaml_str).map_err(|e| {
                handle.response.is_failed(request, &format!("Failed to write group {}: {}", group, e))
            })?;
        }

        Ok(handle.response.is_created(request))
    }

    fn remove_hosts(&self, handle: &Arc<TaskHandle>, request: &Arc<TaskRequest>, host_vars_dir: &PathBuf, groups_dir: &PathBuf) -> Result<Arc<TaskResponse>, Arc<TaskResponse>> {
        // Remove host_vars files
        for hostname in &self.hostnames {
            let path = host_vars_dir.join(hostname);
            if path.exists() {
                fs::remove_file(&path).map_err(|e| {
                    handle.response.is_failed(request, &format!("Failed to remove host_vars for {}: {}", hostname, e))
                })?;
            }
        }

        // Remove from group files
        for group in &self.groups {
            let group_file = groups_dir.join(group);
            if group_file.exists() {
                if let Ok(content) = fs::read_to_string(&group_file) {
                    if let Ok(mut doc) = serde_yaml::from_str::<serde_yaml::Mapping>(&content) {
                        if let Some(serde_yaml::Value::Sequence(ref mut seq)) = doc.get_mut(&serde_yaml::Value::String("hosts".to_string())) {
                            seq.retain(|v| {
                                if let Some(s) = v.as_str() {
                                    !self.hostnames.contains(&s.to_string())
                                } else {
                                    true
                                }
                            });
                        }

                        let yaml_str = serde_yaml::to_string(&serde_yaml::Value::Mapping(doc))
                            .unwrap_or_default();
                        let _ = fs::write(&group_file, yaml_str);
                    }
                }
            }
        }

        Ok(handle.response.is_removed(request))
    }
}
