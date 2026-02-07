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

//! Proxmox Node module - Query and manage Proxmox node state.
//!
//! This module provides read-only access to Proxmox node information:
//! - Node status (uptime, load, memory, CPU)
//! - Cluster quorum status
//! - List of VMs/LXCs on the node
//!
//! # Example
//!
//! ```yaml
//! - !proxmox_node
//!   name: Check node bee is healthy
//!   api_host: "{{ proxmox_host }}"
//!   api_token_id: "{{ proxmox_token_id }}"
//!   api_token_secret: "{{ proxmox_token_secret }}"
//!   node: bee
//!   save_to: node_info
//! ```

use crate::tasks::*;
use crate::handle::handle::TaskHandle;
use crate::modules::proxmox::common::{ProxmoxApiConfig, ProxmoxApiResponse, NodeStatus, ClusterNodeStatus, GuestListItem};
use serde::Deserialize;
use std::sync::Arc;
use std::collections::HashMap;

const MODULE: &str = "proxmox_node";

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct ProxmoxNodeTask {
    pub name: Option<String>,
    pub api_host: String,
    pub api_token_id: String,
    pub api_token_secret: String,
    pub node: String,
    /// Variable name to save node info into
    pub save_to: Option<String>,
    /// If true, also fetch cluster quorum status
    pub include_cluster: Option<String>,
    /// If true, also list VMs on this node
    pub include_vms: Option<String>,
    /// If true, also list LXCs on this node
    pub include_lxc: Option<String>,
    pub with: Option<PreLogicInput>,
    pub and: Option<PostLogicInput>,
}

struct ProxmoxNodeAction {
    pub api_config: ProxmoxApiConfig,
    pub node: String,
    pub save_to: Option<String>,
    pub include_cluster: bool,
    pub include_vms: bool,
    pub include_lxc: bool,
}

impl IsTask for ProxmoxNodeTask {
    fn get_module(&self) -> String { String::from(MODULE) }
    fn get_name(&self) -> Option<String> { self.name.clone() }
    fn get_with(&self) -> Option<PreLogicInput> { self.with.clone() }

    fn evaluate(&self, handle: &Arc<TaskHandle>, request: &Arc<TaskRequest>, tm: TemplateMode) -> Result<EvaluatedTask, Arc<TaskResponse>> {
        Ok(
            EvaluatedTask {
                action: Arc::new(ProxmoxNodeAction {
                    api_config: ProxmoxApiConfig {
                        api_host: handle.template.string(request, tm, &String::from("api_host"), &self.api_host)?,
                        api_token_id: handle.template.string(request, tm, &String::from("api_token_id"), &self.api_token_id)?,
                        api_token_secret: handle.template.string(request, tm, &String::from("api_token_secret"), &self.api_token_secret)?,
                    },
                    node: handle.template.string(request, tm, &String::from("node"), &self.node)?,
                    save_to: handle.template.string_option(request, tm, &String::from("save_to"), &self.save_to)?,
                    include_cluster: handle.template.boolean_option_default_false(request, tm, &String::from("include_cluster"), &self.include_cluster)?,
                    include_vms: handle.template.boolean_option_default_false(request, tm, &String::from("include_vms"), &self.include_vms)?,
                    include_lxc: handle.template.boolean_option_default_false(request, tm, &String::from("include_lxc"), &self.include_lxc)?,
                }),
                with: Arc::new(PreLogicInput::template(handle, request, tm, &self.with)?),
                and: Arc::new(PostLogicInput::template(handle, request, tm, &self.and)?),
            }
        )
    }
}

impl IsAction for ProxmoxNodeAction {
    fn dispatch(&self, handle: &Arc<TaskHandle>, request: &Arc<TaskRequest>) -> Result<Arc<TaskResponse>, Arc<TaskResponse>> {
        match request.request_type {
            TaskRequestType::Query => {
                // This is a passive/query module - always needs to run
                Ok(handle.response.needs_passive(request))
            },

            TaskRequestType::Passive => {
                let info = self.get_node_info(handle, request)?;

                // Save to variable if requested
                if let Some(ref var_name) = self.save_to {
                    let mut mapping = serde_yaml::Mapping::new();
                    mapping.insert(
                        serde_yaml::Value::String(var_name.clone()),
                        serde_yaml::Value::String(info.clone())
                    );
                    handle.host.write().unwrap().update_variables(mapping);
                }

                // Log summary
                handle.debug(request, &format!("Node {} info retrieved", self.node));

                Ok(handle.response.is_passive(request))
            },

            _ => Err(handle.response.not_supported(request))
        }
    }
}

impl ProxmoxNodeAction {
    fn get_node_info(&self, handle: &Arc<TaskHandle>, request: &Arc<TaskRequest>) -> Result<String, Arc<TaskResponse>> {
        let rt = self.api_config.create_runtime(handle, request)?;

        rt.block_on(async {
            let client = self.api_config.create_client(handle, request)?;
            let mut result: HashMap<String, serde_json::Value> = HashMap::new();

            // Get node status
            let status = self.fetch_node_status(&client, handle, request).await?;
            result.insert("status".to_string(), serde_json::to_value(&status).unwrap_or_default());
            result.insert("node".to_string(), serde_json::Value::String(self.node.clone()));
            result.insert("online".to_string(), serde_json::Value::Bool(status.is_some()));

            // Get cluster info if requested
            if self.include_cluster {
                let cluster = self.fetch_cluster_status(&client, handle, request).await?;
                result.insert("cluster".to_string(), serde_json::to_value(&cluster).unwrap_or_default());

                // Extract quorum status for easy access
                let is_quorate = cluster.iter()
                    .find(|n| n.node_type == "cluster")
                    .and_then(|c| c.quorate)
                    .map(|q| q == 1)
                    .unwrap_or(false);
                result.insert("quorate".to_string(), serde_json::Value::Bool(is_quorate));
            }

            // Get VMs if requested
            if self.include_vms {
                let vms = self.fetch_vms(&client, handle, request).await?;
                result.insert("vms".to_string(), serde_json::to_value(&vms).unwrap_or_default());
                result.insert("vm_count".to_string(), serde_json::Value::Number(vms.len().into()));
            }

            // Get LXCs if requested
            if self.include_lxc {
                let lxcs = self.fetch_lxcs(&client, handle, request).await?;
                result.insert("lxc".to_string(), serde_json::to_value(&lxcs).unwrap_or_default());
                result.insert("lxc_count".to_string(), serde_json::Value::Number(lxcs.len().into()));
            }

            serde_json::to_string(&result)
                .map_err(|e| handle.response.is_failed(request, &format!("Failed to serialize node info: {}", e)))
        })
    }

    async fn fetch_node_status(&self, client: &reqwest::Client, handle: &Arc<TaskHandle>, request: &Arc<TaskRequest>) -> Result<Option<NodeStatus>, Arc<TaskResponse>> {
        let url = self.api_config.get_api_url(&format!("/nodes/{}/status", self.node));

        let response = client.get(&url)
            .header("Authorization", self.api_config.get_auth_header())
            .send()
            .await
            .map_err(|e| handle.response.is_failed(request, &format!("Failed to query node status: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(handle.response.is_failed(request, &format!("Proxmox API returned status {}: {}", status, text)));
        }

        let api_response: ProxmoxApiResponse<NodeStatus> = response.json()
            .await
            .map_err(|e| handle.response.is_failed(request, &format!("Failed to parse node status: {}", e)))?;

        Ok(api_response.data)
    }

    async fn fetch_cluster_status(&self, client: &reqwest::Client, handle: &Arc<TaskHandle>, request: &Arc<TaskRequest>) -> Result<Vec<ClusterNodeStatus>, Arc<TaskResponse>> {
        let url = self.api_config.get_api_url("/cluster/status");

        let response = client.get(&url)
            .header("Authorization", self.api_config.get_auth_header())
            .send()
            .await
            .map_err(|e| handle.response.is_failed(request, &format!("Failed to query cluster status: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(handle.response.is_failed(request, &format!("Proxmox API returned status {}: {}", status, text)));
        }

        let api_response: ProxmoxApiResponse<Vec<ClusterNodeStatus>> = response.json()
            .await
            .map_err(|e| handle.response.is_failed(request, &format!("Failed to parse cluster status: {}", e)))?;

        Ok(api_response.data.unwrap_or_default())
    }

    async fn fetch_vms(&self, client: &reqwest::Client, handle: &Arc<TaskHandle>, request: &Arc<TaskRequest>) -> Result<Vec<GuestListItem>, Arc<TaskResponse>> {
        let url = self.api_config.get_api_url(&format!("/nodes/{}/qemu", self.node));

        let response = client.get(&url)
            .header("Authorization", self.api_config.get_auth_header())
            .send()
            .await
            .map_err(|e| handle.response.is_failed(request, &format!("Failed to query VMs: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(handle.response.is_failed(request, &format!("Proxmox API returned status {}: {}", status, text)));
        }

        let api_response: ProxmoxApiResponse<Vec<GuestListItem>> = response.json()
            .await
            .map_err(|e| handle.response.is_failed(request, &format!("Failed to parse VM list: {}", e)))?;

        Ok(api_response.data.unwrap_or_default())
    }

    async fn fetch_lxcs(&self, client: &reqwest::Client, handle: &Arc<TaskHandle>, request: &Arc<TaskRequest>) -> Result<Vec<GuestListItem>, Arc<TaskResponse>> {
        let url = self.api_config.get_api_url(&format!("/nodes/{}/lxc", self.node));

        let response = client.get(&url)
            .header("Authorization", self.api_config.get_auth_header())
            .send()
            .await
            .map_err(|e| handle.response.is_failed(request, &format!("Failed to query LXCs: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(handle.response.is_failed(request, &format!("Proxmox API returned status {}: {}", status, text)));
        }

        let api_response: ProxmoxApiResponse<Vec<GuestListItem>> = response.json()
            .await
            .map_err(|e| handle.response.is_failed(request, &format!("Failed to parse LXC list: {}", e)))?;

        Ok(api_response.data.unwrap_or_default())
    }
}
