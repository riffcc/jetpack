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

//! Proxmox LXC Container Provisioner
//!
//! Creates and manages LXC containers on Proxmox VE via the REST API.

use crate::provisioners::{ProvisionConfig, ProvisionResult, Provisioner};
use crate::inventory::inventory::Inventory;
use crate::playbooks::templar::{Templar, TemplateMode};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// Proxmox LXC container provisioner
pub struct ProxmoxLxcProvisioner;

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

/// Authentication method for Proxmox API
enum ProxmoxAuth {
    /// Token-based auth: PVEAPIToken=user@realm!tokenid=secret
    Token {
        token_id: String,
        token_secret: String,
    },
    /// Password-based auth: requires ticket + CSRF token
    Password {
        ticket: String,
        csrf_token: String,
    },
}

struct ClusterConnection {
    api_host: String,
    auth: ProxmoxAuth,
    node: String,
}

impl ProxmoxLxcProvisioner {
    pub fn new() -> Self {
        Self
    }

    /// Template a string value using host variables
    fn template_string(&self, templar: &Templar, value: &str, vars: &serde_yaml::Mapping) -> Result<String, String> {
        // Only template if the value contains {{ - optimization and prevents errors on plain strings
        if value.contains("{{") {
            templar.render(&value.to_string(), vars.clone(), TemplateMode::Strict)
        } else {
            Ok(value.to_string())
        }
    }

    /// Template an optional string value
    fn template_option(&self, templar: &Templar, value: &Option<String>, vars: &serde_yaml::Mapping) -> Result<Option<String>, String> {
        match value {
            Some(v) => Ok(Some(self.template_string(templar, v, vars)?)),
            None => Ok(None),
        }
    }

    /// Template all fields in a ProvisionConfig using host variables
    fn template_config(&self, config: &ProvisionConfig, vars: &serde_yaml::Mapping) -> Result<ProvisionConfig, String> {
        let templar = Templar::new();

        Ok(ProvisionConfig {
            provision_type: self.template_string(&templar, &config.provision_type, vars)?,
            state: config.state.clone(),
            cluster: self.template_string(&templar, &config.cluster, vars)?,
            hostname: self.template_option(&templar, &config.hostname, vars)?,
            vmid: self.template_option(&templar, &config.vmid, vars)?,
            memory: self.template_option(&templar, &config.memory, vars)?,
            cores: self.template_option(&templar, &config.cores, vars)?,
            ostemplate: self.template_option(&templar, &config.ostemplate, vars)?,
            storage: self.template_option(&templar, &config.storage, vars)?,
            rootfs_size: self.template_option(&templar, &config.rootfs_size, vars)?,
            net0: self.template_option(&templar, &config.net0, vars)?,
            net1: self.template_option(&templar, &config.net1, vars)?,
            net2: self.template_option(&templar, &config.net2, vars)?,
            net3: self.template_option(&templar, &config.net3, vars)?,
            password: self.template_option(&templar, &config.password, vars)?,
            authorized_keys: self.template_option(&templar, &config.authorized_keys, vars)?,
            ssh_user: self.template_option(&templar, &config.ssh_user, vars)?,
            unprivileged: self.template_option(&templar, &config.unprivileged, vars)?,
            start_on_create: self.template_option(&templar, &config.start_on_create, vars)?,
            features: self.template_option(&templar, &config.features, vars)?,
            tun: self.template_option(&templar, &config.tun, vars)?,
            nameserver: self.template_option(&templar, &config.nameserver, vars)?,
            // Wait options are not templated (they're booleans/integers)
            wait_for_host: config.wait_for_host,
            wait_timeout: config.wait_timeout,
            wait_delay: config.wait_delay,
            wait_strategy: config.wait_strategy.clone(),
            wait_max_delay: config.wait_max_delay,
            // Template extra fields (mountpoints, etc.)
            extra: {
                let mut templated_extra = std::collections::HashMap::new();
                for (key, value) in &config.extra {
                    templated_extra.insert(
                        key.clone(),
                        self.template_string(&templar, value, vars)?
                    );
                }
                templated_extra
            },
        })
    }

    /// Get connection details from the cluster host in inventory
    fn get_cluster_connection(&self, config: &ProvisionConfig, inventory: &Arc<RwLock<Inventory>>) -> Result<ClusterConnection, String> {
        let inv = inventory.read().map_err(|e| format!("Failed to read inventory: {}", e))?;

        if !inv.has_host(&config.cluster) {
            return Err(format!("Cluster host '{}' not found in inventory", config.cluster));
        }

        let cluster_host = inv.get_host(&config.cluster);
        let host = cluster_host.read().map_err(|e| format!("Failed to read host: {}", e))?;
        let vars = host.get_blended_variables();

        // Get API connection details from cluster host variables
        let api_host = vars.get("proxmox_api_host")
            .or_else(|| vars.get("ansible_host"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| config.cluster.clone());

        let node = vars.get("proxmox_node")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| config.cluster.clone());

        // Try token auth first (preferred), fall back to password auth
        let api_token_id = vars.get("proxmox_api_token_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let api_token_secret = vars.get("proxmox_api_token_secret")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let auth = if let (Some(token_id), Some(token_secret)) = (api_token_id, api_token_secret) {
            // Use token auth
            ProxmoxAuth::Token { token_id, token_secret }
        } else {
            // Fall back to password auth
            let username = vars.get("proxmox_api_user")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .ok_or_else(|| format!(
                    "Cluster host '{}' missing auth credentials. Need either:\n  \
                     - proxmox_api_token_id + proxmox_api_token_secret (recommended), or\n  \
                     - proxmox_api_user + proxmox_api_password",
                    config.cluster
                ))?;

            let password = vars.get("proxmox_api_password")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .ok_or_else(|| format!(
                    "Cluster host '{}' has proxmox_api_user but missing proxmox_api_password",
                    config.cluster
                ))?;

            // Get ticket for password auth
            let (ticket, csrf_token) = self.get_password_ticket(&api_host, &username, &password)?;
            ProxmoxAuth::Password { ticket, csrf_token }
        };

        Ok(ClusterConnection { api_host, auth, node })
    }

    /// Get authentication ticket for password-based auth
    fn get_password_ticket(&self, api_host: &str, username: &str, password: &str) -> Result<(String, String), String> {
        let url = format!("https://{}:8006/api2/json/access/ticket", api_host);

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| format!("Failed to create async runtime: {}", e))?;

        rt.block_on(async {
            let client = reqwest::Client::builder()
                .danger_accept_invalid_certs(true)
                .build()
                .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

            let mut params = HashMap::new();
            params.insert("username", username);
            params.insert("password", password);

            let response = client.post(&url)
                .form(&params)
                .send()
                .await
                .map_err(|e| format!("Failed to get Proxmox ticket: {}", e))?;

            if !response.status().is_success() {
                let status = response.status();
                let text = response.text().await.unwrap_or_default();
                return Err(format!("Proxmox auth failed ({}): {}", status, text));
            }

            #[derive(Deserialize)]
            struct TicketData {
                ticket: String,
                #[serde(rename = "CSRFPreventionToken")]
                csrf_prevention_token: String,
            }

            #[derive(Deserialize)]
            struct TicketResponse {
                data: TicketData,
            }

            let ticket_response: TicketResponse = response.json()
                .await
                .map_err(|e| format!("Failed to parse ticket response: {}", e))?;

            Ok((ticket_response.data.ticket, ticket_response.data.csrf_prevention_token))
        })
    }

    /// Apply authentication headers to a request
    fn apply_auth(&self, conn: &ClusterConnection, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &conn.auth {
            ProxmoxAuth::Token { token_id, token_secret } => {
                builder.header("Authorization", format!("PVEAPIToken={}={}", token_id, token_secret))
            }
            ProxmoxAuth::Password { ticket, csrf_token } => {
                builder
                    .header("Cookie", format!("PVEAuthCookie={}", ticket))
                    .header("CSRFPreventionToken", csrf_token)
            }
        }
    }


    fn get_api_url(&self, conn: &ClusterConnection, path: &str) -> String {
        // api_host may already include a port (e.g. "host:8006"); only append default if missing
        if conn.api_host.contains(':') {
            format!("https://{}/api2/json{}", conn.api_host, path)
        } else {
            format!("https://{}:8006/api2/json{}", conn.api_host, path)
        }
    }

    /// Find container by hostname
    fn find_container_by_hostname(&self, conn: &ClusterConnection, hostname: &str) -> Result<Option<u64>, String> {
        let url = self.get_api_url(conn, &format!("/nodes/{}/lxc", conn.node));

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| format!("Failed to create async runtime: {}", e))?;

        rt.block_on(async {
            let client = reqwest::Client::builder()
                .danger_accept_invalid_certs(true)
                .build()
                .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

            let response = self.apply_auth(conn, client.get(&url))
                .send()
                .await
                .map_err(|e| format!("Failed to query Proxmox API: {}", e))?;

            if !response.status().is_success() {
                let status = response.status();
                let text = response.text().await.unwrap_or_default();
                return Err(format!("Proxmox API returned status {}: {}", status, text));
            }

            let api_response: ProxmoxApiResponse<Vec<LxcListItem>> = response.json()
                .await
                .map_err(|e| format!("Failed to parse Proxmox response: {}", e))?;

            if let Some(containers) = api_response.data {
                for container in containers {
                    if let Some(ref name) = container.name {
                        if name == hostname {
                            return Ok(Some(container.vmid));
                        }
                    }
                }
            }

            Ok(None)
        })
    }

    /// Get next available VMID from Proxmox
    fn get_next_vmid(&self, conn: &ClusterConnection) -> Result<u64, String> {
        let url = self.get_api_url(conn, "/cluster/nextid");

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| format!("Failed to create async runtime: {}", e))?;

        rt.block_on(async {
            let client = reqwest::Client::builder()
                .danger_accept_invalid_certs(true)
                .build()
                .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

            let response = self.apply_auth(conn, client.get(&url))
                .send()
                .await
                .map_err(|e| format!("Failed to query Proxmox API for next VMID: {}", e))?;

            if !response.status().is_success() {
                let status = response.status();
                let text = response.text().await.unwrap_or_default();
                return Err(format!("Proxmox API returned status {}: {}", status, text));
            }

            let api_response: ProxmoxApiResponse<String> = response.json()
                .await
                .map_err(|e| format!("Failed to parse Proxmox response: {}", e))?;

            api_response.data
                .ok_or_else(|| "No VMID returned from Proxmox".to_string())?
                .parse::<u64>()
                .map_err(|e| format!("Invalid VMID returned: {}", e))
        })
    }

    /// Create a new LXC container
    fn create_container(&self, conn: &ClusterConnection, config: &ProvisionConfig, hostname: &str, vmid: u64, host_vars: &serde_yaml::Mapping) -> Result<(), String> {
        let url = self.get_api_url(conn, &format!("/nodes/{}/lxc", conn.node));

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| format!("Failed to create async runtime: {}", e))?;

        rt.block_on(async {
            let client = reqwest::Client::builder()
                .danger_accept_invalid_certs(true)
                .build()
                .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

            let mut params: HashMap<String, String> = HashMap::new();
            params.insert("vmid".to_string(), vmid.to_string());
            params.insert("hostname".to_string(), hostname.to_string());

            // Required: ostemplate
            if let Some(ref ostemplate) = config.ostemplate {
                params.insert("ostemplate".to_string(), ostemplate.clone());
            } else {
                return Err("ostemplate is required for LXC container creation".to_string());
            }

            // Memory (default 512MB)
            let memory = config.memory.as_ref()
                .and_then(|m| m.parse::<u64>().ok())
                .unwrap_or(512);
            params.insert("memory".to_string(), memory.to_string());

            // Cores (default 1)
            let cores = config.cores.as_ref()
                .and_then(|c| c.parse::<u64>().ok())
                .unwrap_or(1);
            params.insert("cores".to_string(), cores.to_string());

            // Storage and rootfs
            let storage = config.storage.as_ref()
                .map(|s| s.as_str())
                .unwrap_or("local-lvm");
            let rootfs_size_raw = config.rootfs_size.as_ref()
                .map(|s| s.as_str())
                .unwrap_or("8");
            // Strip G/M suffix if present - Proxmox API expects just the number
            let rootfs_size = rootfs_size_raw.trim_end_matches(|c| c == 'G' || c == 'M' || c == 'g' || c == 'm');
            params.insert("rootfs".to_string(), format!("{}:{}", storage, rootfs_size));

            // Unprivileged (default true)
            let unprivileged = config.unprivileged.as_ref()
                .map(|u| u == "true" || u == "1" || u == "yes")
                .unwrap_or(true);
            params.insert("unprivileged".to_string(), if unprivileged { "1" } else { "0" }.to_string());

            // Start on create (default true for provisioning)
            let start = config.start_on_create.as_ref()
                .map(|s| s == "true" || s == "1" || s == "yes")
                .unwrap_or(true);
            params.insert("start".to_string(), if start { "1" } else { "0" }.to_string());

            // Network configurations
            if let Some(ref net0) = config.net0 {
                params.insert("net0".to_string(), net0.clone());
            }
            if let Some(ref net1) = config.net1 {
                params.insert("net1".to_string(), net1.clone());
            }
            if let Some(ref net2) = config.net2 {
                params.insert("net2".to_string(), net2.clone());
            }
            if let Some(ref net3) = config.net3 {
                params.insert("net3".to_string(), net3.clone());
            }

            // Password
            if let Some(ref password) = config.password {
                params.insert("password".to_string(), password.clone());
            }

            // SSH authorized keys - check provision config first, then host_vars
            let ssh_keys = config.authorized_keys.clone()
                .or_else(|| {
                    // Fall back to host variable 'provision_ssh_public_keys' for backwards compat
                    host_vars.get(serde_yaml::Value::String("provision_ssh_public_keys".to_string()))
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                });
            if let Some(keys) = ssh_keys {
                params.insert("ssh-public-keys".to_string(), keys);
            }

            // Features (nesting, etc.)
            if let Some(ref features) = config.features {
                params.insert("features".to_string(), features.clone());
            }

            // DNS nameservers
            if let Some(ref nameserver) = config.nameserver {
                params.insert("nameserver".to_string(), nameserver.clone());
            }

            // Add extra fields (mountpoints mp0, mp1, etc.)
            for (key, value) in &config.extra {
                // Pass through any extra fields to the API
                // This handles mp0, mp1, mp2, ... mpN for mountpoints
                // and any other Proxmox LXC parameters
                params.insert(key.clone(), value.clone());
            }

            let response = self.apply_auth(conn, client.post(&url))
                .form(&params)
                .send()
                .await
                .map_err(|e| format!("Failed to create container: {}", e))?;

            if !response.status().is_success() {
                let status = response.status();
                let text = response.text().await.unwrap_or_default();
                return Err(format!("Proxmox API returned status {}: {}", status, text));
            }

            Ok(())
        })
    }

    /// Stop a container
    fn stop_container(&self, conn: &ClusterConnection, vmid: u64) -> Result<(), String> {
        let url = self.get_api_url(conn, &format!("/nodes/{}/lxc/{}/status/stop", conn.node, vmid));

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| format!("Failed to create async runtime: {}", e))?;

        rt.block_on(async {
            let client = reqwest::Client::builder()
                .danger_accept_invalid_certs(true)
                .build()
                .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

            // Ignore errors - container might already be stopped
            let _ = self.apply_auth(conn, client.post(&url))
                .send()
                .await;

            Ok(())
        })
    }

    /// Configure TUN device access for a container via SSH to the Proxmox node
    /// This is needed for VPN software like Tailscale, WireGuard, OpenVPN
    /// Returns true if config was changed, false if already configured
    fn configure_tun(&self, conn: &ClusterConnection, vmid: u64) -> Result<bool, String> {
        let config_path = format!("/etc/pve/lxc/{}.conf", vmid);

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| format!("Failed to create async runtime: {}", e))?;

        rt.block_on(async {
            // Check if TUN config already exists
            let check_cmd = format!("grep -q 'lxc.cgroup2.devices.allow: c 10:200' {}", config_path);
            if self.ssh_command(&conn.api_host, &check_cmd).await.is_ok() {
                // TUN already configured
                return Ok(false);
            }

            // Append TUN configuration
            let tun_lines = vec![
                "lxc.cgroup2.devices.allow: c 10:200 rwm",
                "lxc.mount.entry: /dev/net/tun dev/net/tun none bind,create=file",
            ];

            for line in tun_lines {
                let append_cmd = format!("echo '{}' >> {}", line, config_path);
                self.ssh_command(&conn.api_host, &append_cmd).await
                    .map_err(|e| format!("Failed to append TUN config: {}", e))?;
            }

            Ok(true)
        })
    }

    /// Execute a command on the Proxmox node via SSH
    async fn ssh_command(&self, host: &str, cmd: &str) -> Result<String, String> {
        use tokio::process::Command;

        let mut command = Command::new("ssh");
        command.args(["-o", "StrictHostKeyChecking=no", "-o", "BatchMode=yes", &format!("root@{}", host), cmd]);

        // Inherit SSH_AUTH_SOCK from environment for SSH agent forwarding
        if let Ok(auth_sock) = std::env::var("SSH_AUTH_SOCK") {
            command.env("SSH_AUTH_SOCK", auth_sock);
        }

        let output: std::process::Output = command
            .output()
            .await
            .map_err(|e| format!("SSH command failed: {}", e))?;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).to_string())
        } else {
            Err(String::from_utf8_lossy(&output.stderr).to_string())
        }
    }

    /// Start a container
    fn start_container(&self, conn: &ClusterConnection, vmid: u64) -> Result<(), String> {
        let url = self.get_api_url(conn, &format!("/nodes/{}/lxc/{}/status/start", conn.node, vmid));

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| format!("Failed to create async runtime: {}", e))?;

        rt.block_on(async {
            let client = reqwest::Client::builder()
                .danger_accept_invalid_certs(true)
                .build()
                .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

            let response = self.apply_auth(conn, client.post(&url))
                .send()
                .await
                .map_err(|e| format!("Failed to start container: {}", e))?;

            if !response.status().is_success() {
                let status = response.status();
                let text = response.text().await.unwrap_or_default();
                return Err(format!("Failed to start container: {} - {}", status, text));
            }

            Ok(())
        })
    }

    /// Delete a container
    fn delete_container(&self, conn: &ClusterConnection, vmid: u64) -> Result<(), String> {
        // First stop the container
        let _ = self.stop_container(conn, vmid);

        let url = self.get_api_url(conn, &format!("/nodes/{}/lxc/{}", conn.node, vmid));

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| format!("Failed to create async runtime: {}", e))?;

        rt.block_on(async {
            let client = reqwest::Client::builder()
                .danger_accept_invalid_certs(true)
                .build()
                .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

            let response = self.apply_auth(conn, client.delete(&url))
                .send()
                .await
                .map_err(|e| format!("Failed to delete container: {}", e))?;

            if !response.status().is_success() {
                let status = response.status();
                let text = response.text().await.unwrap_or_default();
                return Err(format!("Proxmox API returned status {}: {}", status, text));
            }

            Ok(())
        })
    }
}

impl Provisioner for ProxmoxLxcProvisioner {
    fn exists(&self, config: &ProvisionConfig, inventory_name: &str, inventory: &Arc<RwLock<Inventory>>) -> Result<bool, String> {
        let conn = self.get_cluster_connection(config, inventory)?;
        let hostname = config.hostname.as_ref()
            .map(|s| s.as_str())
            .unwrap_or(inventory_name);

        // First check by VMID if specified
        if let Some(ref vmid_str) = config.vmid {
            if let Ok(vmid) = vmid_str.parse::<u64>() {
                let url = self.get_api_url(&conn, &format!("/nodes/{}/lxc/{}/status/current", conn.node, vmid));

                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|e| format!("Failed to create async runtime: {}", e))?;

                return rt.block_on(async {
                    let client = reqwest::Client::builder()
                        .danger_accept_invalid_certs(true)
                        .build()
                        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

                    let response = self.apply_auth(&conn, client.get(&url))
                        .send()
                        .await
                        .map_err(|e| format!("Failed to query Proxmox API: {}", e))?;

                    Ok(response.status().is_success())
                });
            }
        }

        // Otherwise check by hostname
        Ok(self.find_container_by_hostname(&conn, hostname)?.is_some())
    }

    fn ensure_exists(&self, config: &ProvisionConfig, inventory_name: &str, inventory: &Arc<RwLock<Inventory>>) -> Result<ProvisionResult, String> {
        // Get host variables first for templating
        let inv_name = inventory_name.to_string();
        let host_vars = {
            let inv = inventory.read().map_err(|e| format!("Failed to read inventory: {}", e))?;
            if inv.has_host(&inv_name) {
                let host = inv.get_host(&inv_name);
                let h = host.read().map_err(|e| format!("Failed to read host: {}", e))?;
                h.get_blended_variables()
            } else {
                serde_yaml::Mapping::new()
            }
        };

        // Template the provision config using host variables
        let config = self.template_config(config, &host_vars)?;

        let conn = self.get_cluster_connection(&config, inventory)?;
        let hostname = config.hostname.as_ref()
            .map(|s| s.as_str())
            .unwrap_or(inventory_name);

        // Check if TUN device is requested
        let needs_tun = config.tun.as_ref()
            .map(|t| t == "true" || t == "1" || t == "yes")
            .unwrap_or(false);

        // Check if already exists
        if let Some(existing_vmid) = self.find_container_by_hostname(&conn, hostname)? {
            // Container exists - check if TUN config needs to be added
            if needs_tun {
                let tun_changed = self.configure_tun(&conn, existing_vmid)?;
                if tun_changed {
                    // Restart container to apply TUN config
                    self.stop_container(&conn, existing_vmid)?;
                    std::thread::sleep(std::time::Duration::from_secs(2));
                    self.start_container(&conn, existing_vmid)?;
                    std::thread::sleep(std::time::Duration::from_secs(3));
                    return Ok(ProvisionResult::Updated);
                }
            }
            return Ok(ProvisionResult::AlreadyExists);
        }

        // Get VMID (specified or auto-assigned)
        let vmid = if let Some(ref vmid_str) = config.vmid {
            vmid_str.parse::<u64>()
                .map_err(|e| format!("Invalid vmid '{}': {}", vmid_str, e))?
        } else {
            self.get_next_vmid(&conn)?
        };

        // Create the container (with start=0 if we need to configure TUN first)
        let mut config_for_create = config.clone();
        if needs_tun {
            config_for_create.start_on_create = Some("false".to_string());
        }
        self.create_container(&conn, &config_for_create, hostname, vmid, &host_vars)?;

        // Configure TUN if needed
        if needs_tun {
            // Wait for container to be fully created
            std::thread::sleep(std::time::Duration::from_secs(2));
            let _ = self.configure_tun(&conn, vmid)?;
            // Now start the container
            self.start_container(&conn, vmid)?;
            std::thread::sleep(std::time::Duration::from_secs(3));
        }

        // Wait for SSH to be ready before returning - "Created means ready"
        if config.wait_for_host != Some(false) {
            if let Some(ip) = self.get_ip(&config, inventory_name, inventory)? {
                let ssh_user = config.ssh_user.as_deref().unwrap_or("root");
                crate::provisioners::wait_for_ssh(&ip, 22, ssh_user, &config, hostname)?;
            }
        }

        Ok(ProvisionResult::Created)
    }

    fn get_ip(&self, config: &ProvisionConfig, _inventory_name: &str, _inventory: &Arc<RwLock<Inventory>>) -> Result<Option<String>, String> {
        // Parse IP from net0 config if available
        if let Some(ref net0) = config.net0 {
            // Format: "name=eth0,bridge=vmbr0,ip=10.10.10.2/24,gw=10.10.10.1"
            for part in net0.split(',') {
                if part.starts_with("ip=") {
                    let ip_cidr = &part[3..];
                    // Strip CIDR notation if present
                    let ip = ip_cidr.split('/').next().unwrap_or(ip_cidr);
                    return Ok(Some(ip.to_string()));
                }
            }
        }
        Ok(None)
    }

    fn destroy(&self, config: &ProvisionConfig, inventory_name: &str, inventory: &Arc<RwLock<Inventory>>) -> Result<(), String> {
        let conn = self.get_cluster_connection(config, inventory)?;
        let hostname = config.hostname.as_ref()
            .map(|s| s.as_str())
            .unwrap_or(inventory_name);

        // Find by VMID first, then hostname
        let vmid = if let Some(ref vmid_str) = config.vmid {
            vmid_str.parse::<u64>()
                .map_err(|e| format!("Invalid vmid '{}': {}", vmid_str, e))?
        } else {
            self.find_container_by_hostname(&conn, hostname)?
                .ok_or_else(|| format!("Container '{}' not found", hostname))?
        };

        self.delete_container(&conn, vmid)
    }
}
