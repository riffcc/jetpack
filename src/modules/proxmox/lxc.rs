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

use crate::tasks::*;
use crate::handle::handle::TaskHandle;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::collections::HashMap;

const MODULE: &str = "proxmox_lxc";

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct ProxmoxLxcTask {
    pub name: Option<String>,
    pub api_host: String,
    pub api_token_id: String,
    pub api_token_secret: String,
    pub node: String,
    pub vmid: String,
    pub hostname: Option<String>,
    pub ostemplate: Option<String>,
    pub storage: Option<String>,
    pub memory: Option<String>,
    pub cores: Option<String>,
    pub rootfs_size: Option<String>,
    pub net0: Option<String>,
    pub password: Option<String>,
    pub unprivileged: Option<String>,
    pub start_on_create: Option<String>,
    pub state: Option<String>,
    pub with: Option<PreLogicInput>,
    pub and: Option<PostLogicInput>,
}

struct ProxmoxLxcAction {
    pub api_host: String,
    pub api_token_id: String,
    pub api_token_secret: String,
    pub node: String,
    pub vmid: u64,
    pub hostname: Option<String>,
    pub ostemplate: Option<String>,
    pub storage: String,
    pub memory: u64,
    pub cores: u64,
    pub rootfs_size: String,
    pub net0: Option<String>,
    pub password: Option<String>,
    pub unprivileged: bool,
    pub start_on_create: bool,
    pub state: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct ProxmoxApiResponse<T> {
    data: Option<T>,
}

#[derive(Serialize, Deserialize, Debug)]
struct LxcListItem {
    vmid: u64,
    status: String,
    name: Option<String>,
}

impl IsTask for ProxmoxLxcTask {
    fn get_module(&self) -> String { String::from(MODULE) }
    fn get_name(&self) -> Option<String> { self.name.clone() }
    fn get_with(&self) -> Option<PreLogicInput> { self.with.clone() }

    fn evaluate(&self, handle: &Arc<TaskHandle>, request: &Arc<TaskRequest>, tm: TemplateMode) -> Result<EvaluatedTask, Arc<TaskResponse>> {
        return Ok(
            EvaluatedTask {
                action: Arc::new(ProxmoxLxcAction {
                    api_host: handle.template.string(request, tm, &String::from("api_host"), &self.api_host)?,
                    api_token_id: handle.template.string(request, tm, &String::from("api_token_id"), &self.api_token_id)?,
                    api_token_secret: handle.template.string(request, tm, &String::from("api_token_secret"), &self.api_token_secret)?,
                    node: handle.template.string(request, tm, &String::from("node"), &self.node)?,
                    vmid: handle.template.integer(request, tm, &String::from("vmid"), &self.vmid)?,
                    hostname: handle.template.string_option(request, tm, &String::from("hostname"), &self.hostname)?,
                    ostemplate: handle.template.string_option(request, tm, &String::from("ostemplate"), &self.ostemplate)?,
                    storage: handle.template.string_option(request, tm, &String::from("storage"), &self.storage)?
                        .unwrap_or_else(|| String::from("local-lvm")),
                    memory: handle.template.integer_option(request, tm, &String::from("memory"), &self.memory, Some(512))?
                        .unwrap_or(512),
                    cores: handle.template.integer_option(request, tm, &String::from("cores"), &self.cores, Some(1))?
                        .unwrap_or(1),
                    rootfs_size: handle.template.string_option(request, tm, &String::from("rootfs_size"), &self.rootfs_size)?
                        .unwrap_or_else(|| String::from("8G")),
                    net0: handle.template.string_option(request, tm, &String::from("net0"), &self.net0)?,
                    password: handle.template.string_option(request, tm, &String::from("password"), &self.password)?,
                    unprivileged: handle.template.boolean_option_default_true(request, tm, &String::from("unprivileged"), &self.unprivileged)?,
                    start_on_create: handle.template.boolean_option_default_false(request, tm, &String::from("start_on_create"), &self.start_on_create)?,
                    state: handle.template.string_option(request, tm, &String::from("state"), &self.state)?
                        .unwrap_or_else(|| String::from("present")),
                }),
                with: Arc::new(PreLogicInput::template(handle, request, tm, &self.with)?),
                and: Arc::new(PostLogicInput::template(handle, request, tm, &self.and)?),
            }
        );
    }
}

impl IsAction for ProxmoxLxcAction {
    fn dispatch(&self, handle: &Arc<TaskHandle>, request: &Arc<TaskRequest>) -> Result<Arc<TaskResponse>, Arc<TaskResponse>> {
        match request.request_type {
            TaskRequestType::Query => {
                let exists = self.container_exists(handle, request)?;
                if self.state == "absent" {
                    if exists {
                        return Ok(handle.response.needs_removal(request));
                    } else {
                        return Ok(handle.response.is_matched(request));
                    }
                } else {
                    if exists {
                        return Ok(handle.response.is_matched(request));
                    } else {
                        return Ok(handle.response.needs_creation(request));
                    }
                }
            },

            TaskRequestType::Create => {
                self.create_container(handle, request)?;
                return Ok(handle.response.is_created(request));
            },

            TaskRequestType::Remove => {
                self.delete_container(handle, request)?;
                return Ok(handle.response.is_removed(request));
            },

            _ => { return Err(handle.response.not_supported(request)); }
        }
    }
}

impl ProxmoxLxcAction {
    fn get_auth_header(&self) -> String {
        format!("PVEAPIToken={}={}", self.api_token_id, self.api_token_secret)
    }

    fn get_api_url(&self, path: &str) -> String {
        format!("https://{}:8006/api2/json{}", self.api_host, path)
    }

    fn container_exists(&self, handle: &Arc<TaskHandle>, request: &Arc<TaskRequest>) -> Result<bool, Arc<TaskResponse>> {
        let url = self.get_api_url(&format!("/nodes/{}/lxc", self.node));

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| handle.response.is_failed(request, &format!("Failed to create async runtime: {}", e)))?;

        rt.block_on(async {
            let client = reqwest::Client::builder()
                .danger_accept_invalid_certs(true)
                .build()
                .map_err(|e| handle.response.is_failed(request, &format!("Failed to create HTTP client: {}", e)))?;

            let response = client.get(&url)
                .header("Authorization", self.get_auth_header())
                .send()
                .await
                .map_err(|e| handle.response.is_failed(request, &format!("Failed to query Proxmox API: {}", e)))?;

            if !response.status().is_success() {
                let status = response.status();
                let text = response.text().await.unwrap_or_default();
                return Err(handle.response.is_failed(request, &format!("Proxmox API returned status {}: {}", status, text)));
            }

            let api_response: ProxmoxApiResponse<Vec<LxcListItem>> = response.json()
                .await
                .map_err(|e| handle.response.is_failed(request, &format!("Failed to parse Proxmox response: {}", e)))?;

            if let Some(containers) = api_response.data {
                for container in containers {
                    if container.vmid == self.vmid {
                        return Ok(true);
                    }
                }
            }

            Ok(false)
        })
    }

    fn create_container(&self, handle: &Arc<TaskHandle>, request: &Arc<TaskRequest>) -> Result<(), Arc<TaskResponse>> {
        let url = self.get_api_url(&format!("/nodes/{}/lxc", self.node));

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| handle.response.is_failed(request, &format!("Failed to create async runtime: {}", e)))?;

        rt.block_on(async {
            let client = reqwest::Client::builder()
                .danger_accept_invalid_certs(true)
                .build()
                .map_err(|e| handle.response.is_failed(request, &format!("Failed to create HTTP client: {}", e)))?;

            let mut params: HashMap<String, String> = HashMap::new();
            params.insert("vmid".to_string(), self.vmid.to_string());
            params.insert("memory".to_string(), self.memory.to_string());
            params.insert("cores".to_string(), self.cores.to_string());
            params.insert("rootfs".to_string(), format!("{}:{}", self.storage, self.rootfs_size));
            params.insert("unprivileged".to_string(), if self.unprivileged { "1" } else { "0" }.to_string());
            params.insert("start".to_string(), if self.start_on_create { "1" } else { "0" }.to_string());

            if let Some(ref hostname) = self.hostname {
                params.insert("hostname".to_string(), hostname.clone());
            }

            if let Some(ref ostemplate) = self.ostemplate {
                params.insert("ostemplate".to_string(), ostemplate.clone());
            }

            if let Some(ref net0) = self.net0 {
                params.insert("net0".to_string(), net0.clone());
            }

            if let Some(ref password) = self.password {
                params.insert("password".to_string(), password.clone());
            }

            let response = client.post(&url)
                .header("Authorization", self.get_auth_header())
                .form(&params)
                .send()
                .await
                .map_err(|e| handle.response.is_failed(request, &format!("Failed to create container: {}", e)))?;

            if !response.status().is_success() {
                let status = response.status();
                let text = response.text().await.unwrap_or_default();
                return Err(handle.response.is_failed(request, &format!("Proxmox API returned status {}: {}", status, text)));
            }

            Ok(())
        })
    }

    fn delete_container(&self, handle: &Arc<TaskHandle>, request: &Arc<TaskRequest>) -> Result<(), Arc<TaskResponse>> {
        // First stop the container if running
        let _ = self.stop_container(handle, request);

        let url = self.get_api_url(&format!("/nodes/{}/lxc/{}", self.node, self.vmid));

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| handle.response.is_failed(request, &format!("Failed to create async runtime: {}", e)))?;

        rt.block_on(async {
            let client = reqwest::Client::builder()
                .danger_accept_invalid_certs(true)
                .build()
                .map_err(|e| handle.response.is_failed(request, &format!("Failed to create HTTP client: {}", e)))?;

            let response = client.delete(&url)
                .header("Authorization", self.get_auth_header())
                .send()
                .await
                .map_err(|e| handle.response.is_failed(request, &format!("Failed to delete container: {}", e)))?;

            if !response.status().is_success() {
                let status = response.status();
                let text = response.text().await.unwrap_or_default();
                return Err(handle.response.is_failed(request, &format!("Proxmox API returned status {}: {}", status, text)));
            }

            Ok(())
        })
    }

    fn stop_container(&self, handle: &Arc<TaskHandle>, request: &Arc<TaskRequest>) -> Result<(), Arc<TaskResponse>> {
        let url = self.get_api_url(&format!("/nodes/{}/lxc/{}/status/stop", self.node, self.vmid));

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| handle.response.is_failed(request, &format!("Failed to create async runtime: {}", e)))?;

        rt.block_on(async {
            let client = reqwest::Client::builder()
                .danger_accept_invalid_certs(true)
                .build()
                .map_err(|e| handle.response.is_failed(request, &format!("Failed to create HTTP client: {}", e)))?;

            let response = client.post(&url)
                .header("Authorization", self.get_auth_header())
                .send()
                .await
                .map_err(|e| handle.response.is_failed(request, &format!("Failed to stop container: {}", e)))?;

            // Ignore errors when stopping - container might already be stopped
            let _ = response;

            Ok(())
        })
    }
}
