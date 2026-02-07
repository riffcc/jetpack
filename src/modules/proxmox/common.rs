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

//! Common utilities for Proxmox API modules.
//!
//! Provides shared authentication, HTTP client setup, and response parsing
//! for all Proxmox modules (lxc, migrate, node, etc.)

use crate::tasks::*;
use crate::handle::handle::TaskHandle;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Standard Proxmox API response wrapper.
/// All Proxmox API responses wrap data in a `data` field.
#[derive(Serialize, Deserialize, Debug)]
pub struct ProxmoxApiResponse<T> {
    pub data: Option<T>,
}

/// Proxmox API client configuration.
/// Contains credentials and connection info needed for API calls.
#[derive(Clone)]
pub struct ProxmoxApiConfig {
    pub api_host: String,
    pub api_token_id: String,
    pub api_token_secret: String,
}

impl ProxmoxApiConfig {
    /// Create authorization header value for Proxmox API token auth.
    pub fn get_auth_header(&self) -> String {
        format!("PVEAPIToken={}={}", self.api_token_id, self.api_token_secret)
    }

    /// Build full API URL from path.
    pub fn get_api_url(&self, path: &str) -> String {
        format!("https://{}:8006/api2/json{}", self.api_host, path)
    }

    /// Create a reqwest client configured for Proxmox.
    /// Accepts invalid certs (Proxmox uses self-signed by default).
    pub fn create_client(&self, handle: &Arc<TaskHandle>, request: &Arc<TaskRequest>) -> Result<reqwest::Client, Arc<TaskResponse>> {
        reqwest::Client::builder()
            .danger_accept_invalid_certs(true)
            .build()
            .map_err(|e| handle.response.is_failed(request, &format!("Failed to create HTTP client: {}", e)))
    }

    /// Create a tokio runtime for async operations.
    pub fn create_runtime(&self, handle: &Arc<TaskHandle>, request: &Arc<TaskRequest>) -> Result<tokio::runtime::Runtime, Arc<TaskResponse>> {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| handle.response.is_failed(request, &format!("Failed to create async runtime: {}", e)))
    }
}

/// VM/LXC status as returned by Proxmox list endpoints.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GuestListItem {
    pub vmid: u64,
    pub status: String,
    pub name: Option<String>,
    #[serde(rename = "type")]
    pub guest_type: Option<String>,
}

/// Node status information from /nodes/{node}/status.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct NodeStatus {
    pub uptime: Option<u64>,
    pub loadavg: Option<Vec<String>>,
    pub memory: Option<MemoryInfo>,
    pub cpu: Option<f64>,
    pub cpuinfo: Option<CpuInfo>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MemoryInfo {
    pub total: Option<u64>,
    pub used: Option<u64>,
    pub free: Option<u64>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CpuInfo {
    pub cpus: Option<u32>,
    pub model: Option<String>,
    pub sockets: Option<u32>,
}

/// Cluster node status from /cluster/status.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ClusterNodeStatus {
    pub name: String,
    #[serde(rename = "type")]
    pub node_type: String,
    pub online: Option<u32>,
    pub nodeid: Option<u32>,
    pub quorate: Option<u32>,
    pub local: Option<u32>,
}

/// Migration task status from Proxmox task UPID.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ProxmoxTaskStatus {
    pub status: String,
    pub exitstatus: Option<String>,
}
