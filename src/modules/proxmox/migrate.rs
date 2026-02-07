// Jetpack
// Copyright (C) Riff Labs Limited <team@riff.cc>
// Based on Jetporch by Michael DeHaan <michael@michaeldehaan.net> + contributors
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

//! Proxmox Migrate module - Live migrate VMs and LXCs between nodes.
//!
//! This module provides migration capabilities for Proxmox VMs and LXCs:
//! - Live (online) migration
//! - Offline migration
//! - Target node selection
//! - Migration verification
//!
//! # Example
//!
//! ```yaml
//! - !proxmox_migrate
//!   name: Migrate jetpack01 to moth
//!   api_host: "{{ proxmox_host }}"
//!   api_token_id: "{{ proxmox_token_id }}"
//!   api_token_secret: "{{ proxmox_token_secret }}"
//!   source_node: bee
//!   target_node: moth
//!   vmid: "100"
//!   guest_type: lxc
//!   online: true
//! ```

use crate::tasks::*;
use crate::tasks::fields::Field;
use crate::handle::handle::TaskHandle;
use crate::modules::proxmox::common::{ProxmoxApiConfig, ProxmoxApiResponse, GuestListItem, ProxmoxTaskStatus};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::collections::HashMap;

const MODULE: &str = "proxmox_migrate";

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct ProxmoxMigrateTask {
    pub name: Option<String>,
    pub api_host: String,
    pub api_token_id: String,
    pub api_token_secret: String,
    /// Source node where the guest currently resides
    pub source_node: String,
    /// Target node to migrate to
    pub target_node: String,
    /// VM/LXC ID
    pub vmid: String,
    /// Guest type: "vm" (qemu) or "lxc"
    pub guest_type: String,
    /// Live migration (true) or offline (false). Default: true
    pub online: Option<String>,
    /// Restart guest after migration if it was running. Default: true
    pub restart: Option<String>,
    /// Timeout in seconds to wait for migration. Default: 300
    pub timeout: Option<String>,
    pub with: Option<PreLogicInput>,
    pub and: Option<PostLogicInput>,
}

struct ProxmoxMigrateAction {
    pub api_config: ProxmoxApiConfig,
    pub source_node: String,
    pub target_node: String,
    pub vmid: u64,
    pub guest_type: GuestType,
    pub online: bool,
    pub restart: bool,
    pub timeout: u64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum GuestType {
    Vm,
    Lxc,
}

impl GuestType {
    fn from_str(s: &str) -> Option<GuestType> {
        match s.to_lowercase().as_str() {
            "vm" | "qemu" => Some(GuestType::Vm),
            "lxc" | "container" => Some(GuestType::Lxc),
            _ => None,
        }
    }

    fn api_path(&self) -> &'static str {
        match self {
            GuestType::Vm => "qemu",
            GuestType::Lxc => "lxc",
        }
    }
}

#[derive(Serialize, Deserialize, Debug)]
struct MigrateResponse {
    data: Option<String>, // UPID of the migration task
}

impl IsTask for ProxmoxMigrateTask {
    fn get_module(&self) -> String { String::from(MODULE) }
    fn get_name(&self) -> Option<String> { self.name.clone() }
    fn get_with(&self) -> Option<PreLogicInput> { self.with.clone() }

    fn evaluate(&self, handle: &Arc<TaskHandle>, request: &Arc<TaskRequest>, tm: TemplateMode) -> Result<EvaluatedTask, Arc<TaskResponse>> {
        let guest_type_str = handle.template.string(request, tm, &String::from("guest_type"), &self.guest_type)?;
        let guest_type = GuestType::from_str(&guest_type_str)
            .ok_or_else(|| handle.response.is_failed(request, &format!("Invalid guest_type '{}'. Must be 'vm' or 'lxc'", guest_type_str)))?;

        Ok(
            EvaluatedTask {
                action: Arc::new(ProxmoxMigrateAction {
                    api_config: ProxmoxApiConfig {
                        api_host: handle.template.string(request, tm, &String::from("api_host"), &self.api_host)?,
                        api_token_id: handle.template.string(request, tm, &String::from("api_token_id"), &self.api_token_id)?,
                        api_token_secret: handle.template.string(request, tm, &String::from("api_token_secret"), &self.api_token_secret)?,
                    },
                    source_node: handle.template.string(request, tm, &String::from("source_node"), &self.source_node)?,
                    target_node: handle.template.string(request, tm, &String::from("target_node"), &self.target_node)?,
                    vmid: handle.template.integer(request, tm, &String::from("vmid"), &self.vmid)?,
                    guest_type,
                    online: handle.template.boolean_option_default_true(request, tm, &String::from("online"), &self.online)?,
                    restart: handle.template.boolean_option_default_true(request, tm, &String::from("restart"), &self.restart)?,
                    timeout: handle.template.integer_option(request, tm, &String::from("timeout"), &self.timeout, Some(300))?
                        .unwrap_or(300) as u64,
                }),
                with: Arc::new(PreLogicInput::template(handle, request, tm, &self.with)?),
                and: Arc::new(PostLogicInput::template(handle, request, tm, &self.and)?),
            }
        )
    }
}

impl IsAction for ProxmoxMigrateAction {
    fn dispatch(&self, handle: &Arc<TaskHandle>, request: &Arc<TaskRequest>) -> Result<Arc<TaskResponse>, Arc<TaskResponse>> {
        match request.request_type {
            TaskRequestType::Query => {
                // Check if guest is already on target node
                let current_node = self.find_guest_node(handle, request)?;

                if let Some(node) = current_node {
                    if node == self.target_node {
                        // Already on target - no migration needed
                        return Ok(handle.response.is_matched(request));
                    } else if node == self.source_node {
                        // On source node - needs migration
                        let changes = vec![Field::Location];
                        return Ok(handle.response.needs_modification(request, &changes));
                    } else {
                        // On unexpected node
                        return Err(handle.response.is_failed(request,
                            &format!("Guest {} found on unexpected node '{}', expected source '{}'",
                                self.vmid, node, self.source_node)));
                    }
                } else {
                    return Err(handle.response.is_failed(request,
                        &format!("Guest {} not found on any node", self.vmid)));
                }
            },

            TaskRequestType::Modify => {
                self.migrate_guest(handle, request)?;
                let changes = vec![Field::Location];
                Ok(handle.response.is_modified(request, changes))
            },

            _ => Err(handle.response.not_supported(request))
        }
    }
}

impl ProxmoxMigrateAction {
    /// Find which node a guest is currently on.
    fn find_guest_node(&self, handle: &Arc<TaskHandle>, request: &Arc<TaskRequest>) -> Result<Option<String>, Arc<TaskResponse>> {
        let rt = self.api_config.create_runtime(handle, request)?;

        rt.block_on(async {
            let client = self.api_config.create_client(handle, request)?;

            // Check source node first
            if self.guest_exists_on_node(&client, &self.source_node, handle, request).await? {
                return Ok(Some(self.source_node.clone()));
            }

            // Check target node
            if self.guest_exists_on_node(&client, &self.target_node, handle, request).await? {
                return Ok(Some(self.target_node.clone()));
            }

            Ok(None)
        })
    }

    async fn guest_exists_on_node(&self, client: &reqwest::Client, node: &str, handle: &Arc<TaskHandle>, request: &Arc<TaskRequest>) -> Result<bool, Arc<TaskResponse>> {
        let url = self.api_config.get_api_url(&format!("/nodes/{}/{}", node, self.guest_type.api_path()));

        let response = client.get(&url)
            .header("Authorization", self.api_config.get_auth_header())
            .send()
            .await
            .map_err(|e| handle.response.is_failed(request, &format!("Failed to query {}: {}", node, e)))?;

        if !response.status().is_success() {
            // Node might be offline - don't fail, just return false
            return Ok(false);
        }

        let api_response: ProxmoxApiResponse<Vec<GuestListItem>> = response.json()
            .await
            .map_err(|e| handle.response.is_failed(request, &format!("Failed to parse response: {}", e)))?;

        if let Some(guests) = api_response.data {
            for guest in guests {
                if guest.vmid == self.vmid {
                    return Ok(true);
                }
            }
        }

        Ok(false)
    }

    fn migrate_guest(&self, handle: &Arc<TaskHandle>, request: &Arc<TaskRequest>) -> Result<(), Arc<TaskResponse>> {
        let rt = self.api_config.create_runtime(handle, request)?;

        rt.block_on(async {
            let client = self.api_config.create_client(handle, request)?;

            // Initiate migration
            let upid = self.start_migration(&client, handle, request).await?;

            handle.debug(request, &format!("Migration started, task UPID: {}", upid));

            // Wait for migration to complete
            self.wait_for_task(&client, &upid, handle, request).await?;

            // Verify guest is now on target
            if !self.guest_exists_on_node(&client, &self.target_node, handle, request).await? {
                return Err(handle.response.is_failed(request,
                    &format!("Migration completed but guest {} not found on target node {}", self.vmid, self.target_node)));
            }

            handle.debug(request, &format!("Guest {} successfully migrated to {}", self.vmid, self.target_node));

            Ok(())
        })
    }

    async fn start_migration(&self, client: &reqwest::Client, handle: &Arc<TaskHandle>, request: &Arc<TaskRequest>) -> Result<String, Arc<TaskResponse>> {
        let url = self.api_config.get_api_url(&format!(
            "/nodes/{}/{}/{}/migrate",
            self.source_node,
            self.guest_type.api_path(),
            self.vmid
        ));

        let mut params: HashMap<String, String> = HashMap::new();
        params.insert("target".to_string(), self.target_node.clone());

        // For VMs, 'online' controls live migration
        // For LXCs, 'restart' controls whether to restart after migration
        match self.guest_type {
            GuestType::Vm => {
                params.insert("online".to_string(), if self.online { "1" } else { "0" }.to_string());
            },
            GuestType::Lxc => {
                params.insert("restart".to_string(), if self.restart { "1" } else { "0" }.to_string());
            },
        }

        let response = client.post(&url)
            .header("Authorization", self.api_config.get_auth_header())
            .form(&params)
            .send()
            .await
            .map_err(|e| handle.response.is_failed(request, &format!("Failed to start migration: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(handle.response.is_failed(request, &format!("Migration API returned status {}: {}", status, text)));
        }

        let migrate_response: MigrateResponse = response.json()
            .await
            .map_err(|e| handle.response.is_failed(request, &format!("Failed to parse migration response: {}", e)))?;

        migrate_response.data
            .ok_or_else(|| handle.response.is_failed(request, &String::from("Migration started but no task UPID returned")))
    }

    async fn wait_for_task(&self, client: &reqwest::Client, upid: &str, handle: &Arc<TaskHandle>, request: &Arc<TaskRequest>) -> Result<(), Arc<TaskResponse>> {
        let url = self.api_config.get_api_url(&format!("/nodes/{}/tasks/{}/status", self.source_node, upid));

        let start = std::time::Instant::now();
        let timeout_duration = std::time::Duration::from_secs(self.timeout);

        loop {
            if start.elapsed() > timeout_duration {
                return Err(handle.response.is_failed(request,
                    &format!("Migration timed out after {} seconds", self.timeout)));
            }

            let response = client.get(&url)
                .header("Authorization", self.api_config.get_auth_header())
                .send()
                .await
                .map_err(|e| handle.response.is_failed(request, &format!("Failed to check task status: {}", e)))?;

            if !response.status().is_success() {
                let status = response.status();
                let text = response.text().await.unwrap_or_default();
                return Err(handle.response.is_failed(request, &format!("Task status API returned {}: {}", status, text)));
            }

            let api_response: ProxmoxApiResponse<ProxmoxTaskStatus> = response.json()
                .await
                .map_err(|e| handle.response.is_failed(request, &format!("Failed to parse task status: {}", e)))?;

            if let Some(task_status) = api_response.data {
                match task_status.status.as_str() {
                    "stopped" => {
                        // Task completed - check exit status
                        if let Some(exit) = task_status.exitstatus {
                            if exit == "OK" {
                                return Ok(());
                            } else {
                                return Err(handle.response.is_failed(request,
                                    &format!("Migration task failed with exit status: {}", exit)));
                            }
                        }
                        return Ok(());
                    },
                    "running" => {
                        // Still running - continue waiting
                        // Use tokio sleep to avoid blocking
                        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    },
                    other => {
                        return Err(handle.response.is_failed(request,
                            &format!("Unexpected task status: {}", other)));
                    }
                }
            } else {
                return Err(handle.response.is_failed(request, &String::from("No task status data returned")));
            }
        }
    }
}
