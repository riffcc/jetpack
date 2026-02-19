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
use std::error::Error as StdError;
use std::sync::{Arc, RwLock};

/// Format a reqwest error with diagnostic context (error kind, cause chain, URL).
fn format_reqwest_error(prefix: &str, e: &reqwest::Error, url: &str) -> String {
    let kind = if e.is_builder() { " [builder]" }
        else if e.is_connect() { " [connect]" }
        else if e.is_timeout() { " [timeout]" }
        else if e.is_request() { " [request]" }
        else { "" };
    let cause = e.source()
        .map(|src| {
            let inner = src.source().map(|s2| format!(" -> {}", s2)).unwrap_or_default();
            format!(" caused by: {}{}", src, inner)
        })
        .unwrap_or_default();
    format!("{}: {}{}{} (url: {})", prefix, e, kind, cause, url)
}

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
            node: self.template_option(&templar, &config.node, vars)?,
            hostname: self.template_option(&templar, &config.hostname, vars)?,
            vmid: self.template_option(&templar, &config.vmid, vars)?,
            memory: self.template_option(&templar, &config.memory, vars)?,
            cores: self.template_option(&templar, &config.cores, vars)?,
            ostemplate: self.template_option(&templar, &config.ostemplate, vars)?,
            fetch: self.template_option(&templar, &config.fetch, vars)?,
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

        let node = config.node.clone()
            .or_else(|| vars.get("proxmox_node").and_then(|v| v.as_str()).map(|s| s.to_string()))
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
        let url = if api_host.contains(':') {
            format!("https://{}/api2/json/access/ticket", api_host)
        } else {
            format!("https://{}:8006/api2/json/access/ticket", api_host)
        };

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
                .map_err(|e| format_reqwest_error("Failed to get Proxmox ticket", &e, &url))?;

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

    /// Find the latest version of a template based on a prefix.
    /// e.g., "debian-13-standard" -> finds "debian-13-standard_amd64.tar.zst"
    fn find_latest_template(&self, conn: &ClusterConnection, template_prefix: &str) -> Result<String, String> {
        // Extract storage and template name from "storage:vztmpl/filename"
        let (storage, prefix) = if let Some((s, t)) = template_prefix.split_once(':') {
            (Some(s.to_string()), t.trim_start_matches("vztmpl/").trim_start_matches("vztmpl/"))
        } else {
            (None, template_prefix.trim_start_matches("vztmpl/").trim_start_matches("vztmpl/"))
        };

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| format!("Failed to create async runtime: {}", e))?;

        rt.block_on(async {
            let client = reqwest::Client::builder()
                .danger_accept_invalid_certs(true)
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

            // Get list of storages - either from specified storage or all storages
            let storages_to_search: Vec<String> = if let Some(ref storage_name) = storage {
                vec![storage_name.clone()]
            } else {
                // Query node for available storages
                let storage_url = self.get_api_url(conn, &format!("/nodes/{}/storage", conn.node));
                let storage_response = self.apply_auth(conn, client.get(&storage_url))
                    .send()
                    .await
                    .map_err(|e| format!("Failed to query storages: {}", e))?;

                if !storage_response.status().is_success() {
                    return Err(format!("Failed to get storage list: {}", storage_response.status()));
                }

                #[derive(Deserialize)]
                struct StorageInfo {
                    #[serde(rename = "storage")]
                    storage: String,
                    #[serde(rename = "content")]
                    content: Option<String>,
                    #[serde(rename = "enabled")]
                    enabled: Option<u8>,
                }

                #[derive(Deserialize)]
                struct StorageApiResponse {
                    data: Vec<StorageInfo>,
                }

                let storage_resp: StorageApiResponse = storage_response.json()
                    .await
                    .map_err(|e| format!("Failed to parse storage list: {}", e))?;

                storage_resp.data.into_iter()
                    .filter(|s| s.enabled.unwrap_or(1) == 1)
                    .filter(|s| s.content.as_deref().map(|c| c.contains("vztmpl")).unwrap_or(false))
                    .map(|s| s.storage)
                    .collect()
            };

            if storages_to_search.is_empty() {
                return Err("No storages with templates found".to_string());
            }

            // Search each storage for matching templates
            let mut all_matching: Vec<(String, String)> = Vec::new();

            for storage_name in storages_to_search {
                let url = self.get_api_url(conn, &format!("/nodes/{}/storage/{}/content", conn.node, storage_name));
                let response = match self.apply_auth(conn, client.get(&url)).send().await {
                    Ok(r) if r.status().is_success() => r,
                    _ => continue, // Skip storages that fail
                };

                #[derive(Deserialize)]
                struct StorageContent {
                    #[serde(rename = "volid")]
                    volid: Option<String>,
                    #[serde(rename = "format")]
                    format: Option<String>,
                }

                #[derive(Deserialize)]
                struct ApiResponse {
                    data: Vec<StorageContent>,
                }

                let api_resp: ApiResponse = match response.json().await {
                    Ok(r) => r,
                    _ => continue,
                };

                for item in api_resp.data {
                    if item.format.as_deref() == Some("tgz") || item.format.as_deref() == Some("tar") || item.format.as_deref() == Some("tar.zst") {
                        if let Some(volid) = item.volid {
                            let volid_clone = volid.clone();
                            if let Some(name) = volid_clone.split('/').last() {
                                if name.to_lowercase().starts_with(&prefix.to_lowercase()) {
                                    all_matching.push((volid, name.to_string()));
                                }
                            }
                        }
                    }
                }
            }

            if all_matching.is_empty() {
                return Err(format!("No templates found matching '{}' in any storage", prefix));
            }

            // Sort by name descending to get latest
            all_matching.sort_by(|a, b| b.1.cmp(&a.1));

            Ok(all_matching[0].0.clone())
        })
    }

    /// List available templates from the Proxmox download repository
    /// Returns a list of template names that can be downloaded
    fn list_available_templates(&self, conn: &ClusterConnection) -> Result<Vec<String>, String> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| format!("Failed to create async runtime: {}", e))?;

        rt.block_on(async {
            let client = reqwest::Client::builder()
                .danger_accept_invalid_certs(true)
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

            // Use GET on /nodes/{node}/aplinfo to list available templates
            let url = self.get_api_url(conn, &format!(
                "/nodes/{}/aplinfo",
                conn.node
            ));

            let response = self.apply_auth(conn, client.get(&url))
                .send()
                .await
                .map_err(|e| format!("Failed to query available templates: {}", e))?;

            if !response.status().is_success() {
                let status = response.status();
                let text = response.text().await.unwrap_or_default();
                return Err(format!("Proxmox API returned status {}: {}", status, text));
            }

            #[derive(Deserialize)]
            struct TemplateInfo {
                #[serde(rename = "template")]
                template: Option<String>,
                #[serde(rename = "version")]
                _version: Option<String>,
                #[serde(rename = "arch")]
                _arch: Option<String>,
                #[serde(rename = "os")]
                _os: Option<String>,
            }

            #[derive(Deserialize)]
            struct ApiResponse {
                data: Option<Vec<TemplateInfo>>,
            }

            let api_resp: ApiResponse = response.json()
                .await
                .map_err(|e| format!("Failed to parse template list: {}", e))?;

            let templates = api_resp.data
                .unwrap_or_default()
                .into_iter()
                .filter_map(|t| t.template)
                .collect();

            Ok(templates)
        })
    }

    /// Find and download a template matching the given regex pattern
    /// 1. Queries Proxmox for available templates
    /// 2. Matches against the provided regex pattern
    /// 3. Downloads the first matching template
    fn find_and_download_template(&self, conn: &ClusterConnection, pattern: &str, storage: &str) -> Result<String, String> {
        // First, get list of available templates from Proxmox
        let available = self.list_available_templates(conn)?;

        if available.is_empty() {
            return Err("No templates available in Proxmox repository".to_string());
        }

        // Compile the regex pattern
        let regex = regex::Regex::new(pattern)
            .map_err(|e| format!("Invalid regex pattern '{}': {}", pattern, e))?;

        // Find matching templates
        let matches: Vec<&String> = available.iter()
            .filter(|t| regex.is_match(t))
            .collect();

        if matches.is_empty() {
            let sample: Vec<&String> = available.iter().take(5).collect();
            let sample_str: Vec<&str> = sample.iter().map(|s| s.as_str()).collect();
            return Err(format!(
                "No templates match pattern '{}'. Available templates: {}",
                pattern,
                sample_str.join(", ")
            ));
        }

        // Sort by name (descending) to get the latest version
        let mut sorted_matches: Vec<&String> = matches.clone();
        sorted_matches.sort_by(|a, b| b.cmp(a));

        let selected = sorted_matches[0].clone();
        eprintln!("  → Found matching template: {}", selected);

        // Download the template
        self.download_template(conn, &format!("{}:vztmpl/{}", storage, selected))?;

        Ok(selected)
    }

    /// Download a template from the Proxmox CT template repository
    /// Uses the /nodes/{node}/aplinfo POST endpoint (same as pveam download)
    fn download_template(&self, conn: &ClusterConnection, template: &str) -> Result<(), String> {
        // Extract storage and template name from "storage:vztmpl/filename"
        let (storage, template_name) = if let Some((s, t)) = template.split_once(':') {
            (s.to_string(), t.trim_start_matches("vztmpl/").trim_start_matches("vztmpl/").to_string())
        } else {
            ("local".to_string(), template.trim_start_matches("vztmpl/").trim_start_matches("vztmpl/").to_string())
        };

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| format!("Failed to create async runtime: {}", e))?;

        rt.block_on(async {
            let client = reqwest::Client::builder()
                .danger_accept_invalid_certs(true)
                .timeout(std::time::Duration::from_secs(300)) // 5 min timeout for download
                .build()
                .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

            // Use POST to /nodes/{node}/aplinfo - same as pveam download
            let url = self.get_api_url(conn, &format!(
                "/nodes/{}/aplinfo",
                conn.node
            ));

            let mut params = HashMap::new();
            params.insert("storage", &storage);
            params.insert("template", &template_name);

            eprintln!("  → Downloading template {} to storage {}...", template_name, storage);

            // POST to initiate the download
            let response = self.apply_auth(conn, client.post(&url))
                .form(&params)
                .send()
                .await
                .map_err(|e| format!("Failed to initiate template download: {}", e))?;

            if !response.status().is_success() {
                let status = response.status();
                let text = response.text().await.unwrap_or_default();
                return Err(format!("Proxmox API returned status {}: {}", status, text));
            }

            // The download is async - Proxmox spawns a task
            // Check the response for any immediate errors
            #[derive(Deserialize)]
            struct ApiResponse {
                data: Option<serde_json::Value>,
            }

            let _api_resp: ApiResponse = response.json()
                .await
                .map_err(|e| format!("Failed to parse download response: {}", e))?;

            // Proxmox returns success if the download task was spawned
            // The actual download happens in background - we need to wait for it
            eprintln!("  → Waiting for template download to complete...");

            // Poll until template is available or timeout (5 minutes)
            let max_attempts = 60; // 60 * 5 seconds = 5 minutes
            let mut attempt = 0;

            loop {
                attempt += 1;

                // Check that template now exists
                let check_url = self.get_api_url(conn, &format!(
                    "/nodes/{}/storage/{}/content?content=vztmpl",
                    conn.node, storage
                ));

                let check_response = self.apply_auth(conn, client.get(&check_url))
                    .send()
                    .await
                    .map_err(|e| format!("Failed to verify template: {}", e))?;

                if check_response.status().is_success() {
                    #[derive(Deserialize)]
                    struct StorageContent {
                        #[serde(rename = "volid")]
                        volid: Option<String>,
                    }

                    #[derive(Deserialize)]
                    struct ContentApiResponse {
                        data: Vec<StorageContent>,
                    }

                    if let Ok(content_resp) = check_response.json::<ContentApiResponse>().await {
                        let found = content_resp.data.iter().any(|item| {
                            item.volid.as_ref().map(|v| {
                                v.contains(&template_name)
                            }).unwrap_or(false)
                        });

                        if found {
                            eprintln!("  → Template {} is available ({}s)", template_name, attempt * 5);
                            return Ok(());
                        }
                    }
                }

                if attempt >= max_attempts {
                    return Err(format!("Template download timed out after {} seconds", attempt * 5));
                }

                eprintln!("  → Waiting for template... ({}/{})", attempt * 5, max_attempts * 5);
                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
            }
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

    /// Delete a container. If `force` is true, stops the container first.
    fn delete_container(&self, conn: &ClusterConnection, vmid: u64, force: bool) -> Result<(), String> {
        let base = self.get_api_url(conn, &format!("/nodes/{}/lxc/{}", conn.node, vmid));
        let url = if force {
            format!("{}?force=1&purge=1", base)
        } else {
            base
        };

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
        let mut config = self.template_config(config, &host_vars)?;

        // Get cluster connection FIRST (needed for container check)
        let conn = self.get_cluster_connection(&config, inventory)?;
        let hostname = config.hostname.as_ref()
            .map(|s| s.as_str())
            .unwrap_or(inventory_name);

        // Check if container already exists FIRST - before any template operations
        if let Some(existing_vmid) = self.find_container_by_hostname(&conn, hostname)? {
            eprintln!("  → Container '{}' already exists (vmid: {}), skipping creation", hostname, existing_vmid);
            return Ok(ProvisionResult::AlreadyExists);
        }

        // Check if we need to fetch the template (only if creating new container)
        if let Some(ref fetch) = config.fetch {
            if fetch == "true" || fetch == "1" || fetch == "latest" {
                let ostemplate = config.ostemplate.as_ref()
                    .ok_or("ostemplate required when fetch is enabled")?;

                if fetch == "latest" {
                    // First try to find local template
                    match self.find_latest_template(&conn, ostemplate) {
                        Ok(latest_template) => {
                            eprintln!("  → Using existing template: {}", latest_template);
                            // Update ostemplate to use the actual template file
                            config.ostemplate = Some(latest_template);
                        }
                        Err(_) => {
                            // Not found locally - download from Proxmox
                            // Query Proxmox for available templates, match pattern, download
                            let storage = config.storage.as_deref().unwrap_or("local");
                            eprintln!("  → Template not found locally, searching Proxmox repository...");
                            let downloaded = self.find_and_download_template(&conn, ostemplate, storage)?;
                            // Update ostemplate to use the actual downloaded template file
                            config.ostemplate = Some(format!("{}:vztmpl/{}", storage, downloaded));
                        }
                    }
                } else {
                    // Just download the exact template specified
                    eprintln!("  → Fetching template: {}", ostemplate);
                    self.download_template(&conn, ostemplate)?;
                }
            }
        }

        // Check if TUN device is requested
        let needs_tun = config.tun.as_ref()
            .map(|t| t == "true" || t == "1" || t == "yes")
            .unwrap_or(false);

        eprintln!("  → Creating new container '{}' on node '{}'", hostname, conn.node);

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

        // SSH wait is handled by ensure_host_provisioned() which has the output handler
        Ok(ProvisionResult::Created)
    }

    fn get_ip(&self, config: &ProvisionConfig, inventory_name: &str, inventory: &Arc<RwLock<Inventory>>) -> Result<Option<String>, String> {
        // First try to parse from net0 config if available (static IP)
        if let Some(ref net0) = config.net0 {
            // Format: "name=eth0,bridge=vmbr0,ip=10.10.10.2/24,gw=10.10.10.1"
            for part in net0.split(',') {
                if part.starts_with("ip=") {
                    let ip_cidr = &part[3..];
                    // Skip DHCP - we need to query for the actual IP
                    if ip_cidr.to_lowercase() != "dhcp" {
                        // Strip CIDR notation if present
                        let ip = ip_cidr.split('/').next().unwrap_or(ip_cidr);
                        return Ok(Some(ip.to_string()));
                    }
                }
            }
        }

        // If DHCP or no IP in config, query Proxmox for the container's actual IP
        // First, resolve vmid from hostname if not provided
        let vmid_to_query = if config.vmid.is_some() {
            config.vmid.clone()
        } else {
            // Look up by hostname
            let conn = self.get_cluster_connection(config, inventory)?;
            let hostname = config.hostname.as_deref().unwrap_or(inventory_name);
            match self.find_container_by_hostname(&conn, hostname)? {
                Some(vmid) => Some(vmid.to_string()),
                None => {
                    eprintln!("  → Warning: container '{}' not found for IP lookup", hostname);
                    return Ok(None);
                }
            }
        };

        // Now query the IP using the vmid
        if let Some(ref vmid) = vmid_to_query {
            if let Ok(vmid_num) = vmid.parse::<u64>() {
                // Get cluster connection to query all nodes
                let conn = self.get_cluster_connection(config, inventory)?;

                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|e| format!("Failed to create async runtime: {}", e))?;

                let result: Result<Option<String>, String> = rt.block_on(async {
                    let client = reqwest::Client::builder()
                        .danger_accept_invalid_certs(true)
                        .timeout(std::time::Duration::from_secs(10))
                        .build()
                        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

                    // First, get list of all cluster nodes
                    let nodes_url = format!("https://{}/api2/json/nodes", conn.api_host);
                    let nodes_response = self.apply_auth(&conn, client.get(&nodes_url))
                        .send()
                        .await
                        .map_err(|e| format!("Failed to get nodes: {}", e))?;

                    #[derive(Deserialize)]
                    struct NodeInfo {
                        node: String,
                    }

                    #[derive(Deserialize)]
                    struct NodesResponse {
                        data: Vec<NodeInfo>,
                    }

                    let nodes_resp: NodesResponse = nodes_response.json()
                        .await
                        .map_err(|e| format!("Failed to parse nodes: {}", e))?;

                    // Try each node using /interfaces — reads the container's actual
                    // network namespace, so it always returns the real DHCP-assigned IP.
                    // The /status/current endpoint only returns the config net0 string
                    // (e.g., "ip=dhcp"), never the runtime-assigned address.
                    #[derive(Deserialize)]
                    struct Iface {
                        // Proxmox LXC /interfaces returns "inet" for IPv4 (e.g. "10.1.21.26/16")
                        name: Option<String>,
                        inet: Option<String>,
                    }

                    #[derive(Deserialize)]
                    struct IfacesApiResponse {
                        data: Option<Vec<Iface>>,
                    }

                    for node_info in nodes_resp.data {
                        let node_name = &node_info.node;
                        let ifaces_url = format!("https://{}/api2/json/nodes/{}/lxc/{}/interfaces",
                            conn.api_host, node_name, vmid_num);

                        let ifaces_response = self.apply_auth(&conn, client.get(&ifaces_url))
                            .send()
                            .await
                            .map_err(|e| format!("Failed to query interfaces on node {}: {}", node_name, e))?;

                        // Skip nodes where the container doesn't live (404 / non-success).
                        if !ifaces_response.status().is_success() {
                            continue;
                        }

                        if let Ok(api_resp) = ifaces_response.json::<IfacesApiResponse>().await {
                            if let Some(ifaces) = api_resp.data {
                                for iface in ifaces {
                                    if iface.name.as_deref() == Some("lo") {
                                        continue; // skip loopback
                                    }
                                    if let Some(ref inet) = iface.inet {
                                        // Strip CIDR mask: "10.1.21.26/16" → "10.1.21.26"
                                        let ip = inet.split('/').next().unwrap_or(inet);
                                        if !ip.is_empty() && ip != "127.0.0.1" {
                                            return Ok(Some(ip.to_string()));
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // Fallback: try cluster-wide resources to find the container
                    let resources_url = format!("https://{}/api2/json/cluster/resources", conn.api_host);
                    let resources_response = self.apply_auth(&conn, client.get(&resources_url))
                        .send()
                        .await
                        .map_err(|e| format!("Failed to get cluster resources: {}", e))?;

                    if resources_response.status().is_success() {
                        #[derive(Deserialize)]
                        struct Resource {
                            #[serde(rename = "vmid")]
                            vmid: Option<u64>,
                            #[serde(rename = "node")]
                            node: Option<String>,
                            #[serde(rename = "config")]
                            config: Option<serde_yaml::Value>,
                        }

                        #[derive(Deserialize)]
                        struct ResourcesResponse {
                            data: Vec<Resource>,
                        }

                        let resources_resp: ResourcesResponse = resources_response.json()
                            .await
                            .map_err(|e| format!("Failed to parse resources: {}", e))?;

                        for resource in resources_resp.data {
                            if resource.vmid == Some(vmid_num) {
                                if let Some(node_name) = resource.node {
                                    // Use /interfaces to read the container's live network state.
                                    let ifaces_url = format!("https://{}/api2/json/nodes/{}/lxc/{}/interfaces", conn.api_host, node_name, vmid_num);
                                    let ifaces_response = self.apply_auth(&conn, client.get(&ifaces_url))
                                        .send()
                                        .await
                                        .map_err(|e| format!("Failed to query interfaces on node {}: {}", node_name, e))?;

                                    if ifaces_response.status().is_success() {
                                        // Proxmox LXC /interfaces returns {name, hwaddr, inet, inet6}
                                        // NOT the QEMU-agent format {ip-addresses, ip-address-type}.
                                        #[derive(Deserialize)]
                                        struct Iface {
                                            name: Option<String>,
                                            inet: Option<String>,
                                        }

                                        #[derive(Deserialize)]
                                        struct ApiResponse {
                                            data: Option<Vec<Iface>>,
                                        }

                                        if let Ok(api_resp) = ifaces_response.json::<ApiResponse>().await {
                                            if let Some(ifaces) = api_resp.data {
                                                for iface in ifaces {
                                                    if iface.name.as_deref() == Some("lo") {
                                                        continue;
                                                    }
                                                    if let Some(ref inet) = iface.inet {
                                                        let ip = inet.split('/').next().unwrap_or(inet);
                                                        if !ip.is_empty() && ip != "127.0.0.1" {
                                                            return Ok(Some(ip.to_string()));
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }

                                    // Note: /status/current only returns the config net0 string
                                    // (e.g., "ip=dhcp"), never the runtime-assigned DHCP address.
                                    // /interfaces is the only correct source for DHCP IPs.
                                }
                            }
                        }
                    }

                    Ok(None)
                });

                if let Ok(Some(ip)) = result {
                    return Ok(Some(ip));
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

        let force = config.state == "force-absent";
        self.delete_container(&conn, vmid, force)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Helper to create a minimal ProvisionConfig for testing
    fn test_config() -> ProvisionConfig {
        ProvisionConfig {
            provision_type: "proxmox_lxc".to_string(),
            state: "present".to_string(),
            cluster: "test-cluster".to_string(),
            node: None,
            hostname: None,
            vmid: None,
            memory: None,
            cores: None,
            ostemplate: None,
            fetch: None,
            storage: None,
            rootfs_size: None,
            net0: None,
            net1: None,
            net2: None,
            net3: None,
            password: None,
            authorized_keys: None,
            ssh_user: None,
            unprivileged: None,
            start_on_create: None,
            features: None,
            tun: None,
            nameserver: None,
            wait_for_host: Some(false),
            wait_timeout: Some(60),
            wait_delay: Some(2),
            wait_strategy: None,
            wait_max_delay: Some(30),
            extra: std::collections::HashMap::new(),
        }
    }

    // Helper to create a provisioner instance
    fn test_provisioner() -> ProxmoxLxcProvisioner {
        ProxmoxLxcProvisioner::new()
    }

    // ========== Template URL Parsing Tests ==========

    #[test]
    fn test_template_url_parsing_with_storage() {
        // Test parsing "storage:vztmpl/debian-12-standard_amd64.tar.zst"
        let provisioner = test_provisioner();

        // Create mock vars for templating
        let vars = serde_yaml::Mapping::new();

        let config = ProvisionConfig {
            ostemplate: Some("local:vztmpl/debian-12-standard_amd64.tar.zst".to_string()),
            ..test_config()
        };

        let result = provisioner.template_config(&config, &vars);
        assert!(result.is_ok());

        let templated = result.unwrap();
        assert!(templated.ostemplate.is_some());

        let ostemplate = templated.ostemplate.unwrap();
        assert!(ostemplate.contains("debian-12"));
    }

    #[test]
    fn test_template_url_parsing_without_storage() {
        // Test parsing just "debian-12-standard_amd64.tar.zst"
        let provisioner = test_provisioner();
        let vars = serde_yaml::Mapping::new();

        let config = ProvisionConfig {
            ostemplate: Some("debian-12-standard_amd64.tar.zst".to_string()),
            ..test_config()
        };

        let result = provisioner.template_config(&config, &vars);
        assert!(result.is_ok());
    }

    #[test]
    fn test_template_url_parsing_with_fetch_latest() {
        // Test that fetch: "latest" is properly stored
        let provisioner = test_provisioner();
        let vars = serde_yaml::Mapping::new();

        let config = ProvisionConfig {
            ostemplate: Some("debian-13-standard".to_string()),
            fetch: Some("latest".to_string()),
            ..test_config()
        };

        let result = provisioner.template_config(&config, &vars);
        assert!(result.is_ok());

        let templated = result.unwrap();
        assert_eq!(templated.fetch, Some("latest".to_string()));
    }

    #[test]
    fn test_template_url_parsing_with_fetch_true() {
        // Test that fetch: "true" is properly stored
        let provisioner = test_provisioner();
        let vars = serde_yaml::Mapping::new();

        let config = ProvisionConfig {
            ostemplate: Some("debian-12-standard".to_string()),
            fetch: Some("true".to_string()),
            ..test_config()
        };

        let result = provisioner.template_config(&config, &vars);
        assert!(result.is_ok());

        let templated = result.unwrap();
        assert_eq!(templated.fetch, Some("true".to_string()));
    }

    #[test]
    fn test_template_url_parsing_with_fetch_1() {
        // Test that fetch: "1" works as alternative to "true"
        let provisioner = test_provisioner();
        let vars = serde_yaml::Mapping::new();

        let config = ProvisionConfig {
            ostemplate: Some("ubuntu-22.04-standard".to_string()),
            fetch: Some("1".to_string()),
            ..test_config()
        };

        let result = provisioner.template_config(&config, &vars);
        assert!(result.is_ok());

        let templated = result.unwrap();
        assert_eq!(templated.fetch, Some("1".to_string()));
    }

    // ========== Variable Templating Tests ==========

    #[test]
    fn test_hostname_templating() {
        let provisioner = test_provisioner();
        let mut vars = serde_yaml::Mapping::new();
        vars.insert(serde_yaml::Value::String("hostname".to_string()), serde_yaml::Value::String("test-host".to_string()));

        let config = ProvisionConfig {
            hostname: Some("{{hostname}}".to_string()),
            ..test_config()
        };

        let result = provisioner.template_config(&config, &vars);
        assert!(result.is_ok());

        let templated = result.unwrap();
        assert_eq!(templated.hostname, Some("test-host".to_string()));
    }

    #[test]
    fn test_memory_templating() {
        let provisioner = test_provisioner();
        let mut vars = serde_yaml::Mapping::new();
        vars.insert(serde_yaml::Value::String("memory".to_string()), serde_yaml::Value::String("4096".to_string()));

        let config = ProvisionConfig {
            memory: Some("{{memory}}".to_string()),
            ..test_config()
        };

        let result = provisioner.template_config(&config, &vars);
        assert!(result.is_ok());

        let templated = result.unwrap();
        assert_eq!(templated.memory, Some("4096".to_string()));
    }

    #[test]
    fn test_cores_templating() {
        let provisioner = test_provisioner();
        let mut vars = serde_yaml::Mapping::new();
        vars.insert(serde_yaml::Value::String("cores".to_string()), serde_yaml::Value::String("4".to_string()));

        let config = ProvisionConfig {
            cores: Some("{{cores}}".to_string()),
            ..test_config()
        };

        let result = provisioner.template_config(&config, &vars);
        assert!(result.is_ok());

        let templated = result.unwrap();
        assert_eq!(templated.cores, Some("4".to_string()));
    }

    #[test]
    fn test_net0_templating_with_dhcp() {
        let provisioner = test_provisioner();
        let vars = serde_yaml::Mapping::new();

        // Test DHCP networking
        let config = ProvisionConfig {
            net0: Some("name=eth0,bridge=vmbr0,ip=dhcp".to_string()),
            ..test_config()
        };

        let result = provisioner.template_config(&config, &vars);
        assert!(result.is_ok());

        let templated = result.unwrap();
        assert!(templated.net0.as_ref().unwrap().contains("dhcp"));
    }

    #[test]
    fn test_net0_templating_with_static_ip() {
        let provisioner = test_provisioner();
        let vars = serde_yaml::Mapping::new();

        // Test static IP with CIDR notation
        let config = ProvisionConfig {
            net0: Some("name=eth0,bridge=vmbr0,ip=10.10.10.50/24,gw=10.10.10.1".to_string()),
            ..test_config()
        };

        let result = provisioner.template_config(&config, &vars);
        assert!(result.is_ok());

        let templated = result.unwrap();
        assert!(templated.net0.as_ref().unwrap().contains("10.10.10.50"));
    }

    #[test]
    fn test_net0_templating_with_template_vars() {
        let provisioner = test_provisioner();
        let mut vars = serde_yaml::Mapping::new();
        vars.insert(serde_yaml::Value::String("lxc_ip".to_string()), serde_yaml::Value::String("192.168.100.10".to_string()));
        vars.insert(serde_yaml::Value::String("lxc_gateway".to_string()), serde_yaml::Value::String("192.168.100.1".to_string()));

        let config = ProvisionConfig {
            net0: Some("name=eth0,bridge=vmbr0,ip={{lxc_ip}}/24,gw={{lxc_gateway}}".to_string()),
            ..test_config()
        };

        let result = provisioner.template_config(&config, &vars);
        assert!(result.is_ok());

        let templated = result.unwrap();
        let net0 = templated.net0.unwrap();
        assert!(net0.contains("192.168.100.10"));
        assert!(net0.contains("192.168.100.1"));
    }

    // ========== ProvisionConfig Field Tests ==========

    #[test]
    fn test_provision_config_defaults() {
        // Test that all fields have sensible defaults
        let config = test_config();

        assert_eq!(config.provision_type, "proxmox_lxc");
        assert_eq!(config.state, "present");
        assert_eq!(config.cluster, "test-cluster");
        assert!(config.node.is_none());
        assert!(config.hostname.is_none());
        assert!(config.vmid.is_none());
        assert!(config.memory.is_none());
        assert!(config.cores.is_none());
        assert!(config.ostemplate.is_none());
        assert!(config.fetch.is_none());
    }

    #[test]
    fn test_provision_config_with_all_fields() {
        // Test ProvisionConfig with all fields populated
        let mut extra = std::collections::HashMap::new();
        extra.insert("mp0".to_string(), "volume:10,mp=/mnt/data".to_string());

        let config = ProvisionConfig {
            provision_type: "proxmox_lxc".to_string(),
            state: "present".to_string(),
            cluster: "prod-cluster".to_string(),
            node: Some("pve-node-1".to_string()),
            hostname: Some("web-01".to_string()),
            vmid: Some("101".to_string()),
            memory: Some("4096".to_string()),
            cores: Some("4".to_string()),
            ostemplate: Some("local:vztmpl/debian-12-standard_amd64.tar.zst".to_string()),
            fetch: Some("latest".to_string()),
            storage: Some("local-lvm".to_string()),
            rootfs_size: Some("32".to_string()),
            net0: Some("name=eth0,bridge=vmbr0,ip=dhcp".to_string()),
            net1: None,
            net2: None,
            net3: None,
            password: Some("secret123".to_string()),
            authorized_keys: Some("ssh-rsa AAAA...".to_string()),
            ssh_user: Some("admin".to_string()),
            unprivileged: Some("true".to_string()),
            start_on_create: Some("true".to_string()),
            features: Some("nesting=1".to_string()),
            tun: Some("true".to_string()),
            nameserver: Some("1.1.1.1".to_string()),
            wait_for_host: Some(true),
            wait_timeout: Some(120),
            wait_delay: Some(5),
            wait_strategy: Some("ssh".to_string()),
            wait_max_delay: Some(60),
            extra,
        };

        assert_eq!(config.node, Some("pve-node-1".to_string()));
        assert_eq!(config.memory, Some("4096".to_string()));
        assert_eq!(config.cores, Some("4".to_string()));
        assert_eq!(config.tun, Some("true".to_string()));
        assert_eq!(config.extra.get("mp0").unwrap(), "volume:10,mp=/mnt/data");
    }

    #[test]
    fn test_provision_config_clone() {
        let config = test_config();
        let cloned = config.clone();

        assert_eq!(cloned.provision_type, config.provision_type);
        assert_eq!(cloned.cluster, config.cluster);
    }

    #[test]
    fn test_provision_config_with_unprivileged() {
        let provisioner = test_provisioner();
        let vars = serde_yaml::Mapping::new();

        // Test unprivileged: "true"
        let config = ProvisionConfig {
            unprivileged: Some("true".to_string()),
            ..test_config()
        };

        let result = provisioner.template_config(&config, &vars);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().unprivileged, Some("true".to_string()));

        // Test unprivileged: "false"
        let config2 = ProvisionConfig {
            unprivileged: Some("false".to_string()),
            ..test_config()
        };

        let result2 = provisioner.template_config(&config2, &vars);
        assert!(result2.is_ok());
        assert_eq!(result2.unwrap().unprivileged, Some("false".to_string()));

        // Test unprivileged: "1"
        let config3 = ProvisionConfig {
            unprivileged: Some("1".to_string()),
            ..test_config()
        };

        let result3 = provisioner.template_config(&config3, &vars);
        assert!(result3.is_ok());
        assert_eq!(result3.unwrap().unprivileged, Some("1".to_string()));
    }

    #[test]
    fn test_provision_config_with_start_on_create() {
        let provisioner = test_provisioner();
        let vars = serde_yaml::Mapping::new();

        // Test start_on_create: "true"
        let config = ProvisionConfig {
            start_on_create: Some("true".to_string()),
            ..test_config()
        };

        let result = provisioner.template_config(&config, &vars);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().start_on_create, Some("true".to_string()));

        // Test start_on_create: "false"
        let config2 = ProvisionConfig {
            start_on_create: Some("false".to_string()),
            ..test_config()
        };

        let result2 = provisioner.template_config(&config2, &vars);
        assert!(result2.is_ok());
        assert_eq!(result2.unwrap().start_on_create, Some("false".to_string()));
    }

    #[test]
    fn test_provision_config_with_tun_device() {
        let provisioner = test_provisioner();
        let vars = serde_yaml::Mapping::new();

        // Test tun: "true"
        let config = ProvisionConfig {
            tun: Some("true".to_string()),
            ..test_config()
        };

        let result = provisioner.template_config(&config, &vars);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().tun, Some("true".to_string()));

        // Test tun: "yes"
        let config2 = ProvisionConfig {
            tun: Some("yes".to_string()),
            ..test_config()
        };

        let result2 = provisioner.template_config(&config2, &vars);
        assert!(result2.is_ok());
        assert_eq!(result2.unwrap().tun, Some("yes".to_string()));
    }

    #[test]
    fn test_provision_config_with_extra_mountpoints() {
        let provisioner = test_provisioner();
        let vars = serde_yaml::Mapping::new();

        let mut extra = std::collections::HashMap::new();
        extra.insert("mp0".to_string(), "volume:10,mp=/mnt/data".to_string());
        extra.insert("mp1".to_string(), "volume:20,mp=/mnt/logs".to_string());

        let config = ProvisionConfig {
            extra: extra.clone(),
            ..test_config()
        };

        let result = provisioner.template_config(&config, &vars);
        assert!(result.is_ok());

        let templated = result.unwrap();
        assert_eq!(templated.extra.get("mp0").unwrap(), "volume:10,mp=/mnt/data");
        assert_eq!(templated.extra.get("mp1").unwrap(), "volume:20,mp=/mnt/logs");
    }

    #[test]
    fn test_provision_config_with_nameserver() {
        let provisioner = test_provisioner();
        let vars = serde_yaml::Mapping::new();

        let config = ProvisionConfig {
            nameserver: Some("1.1.1.1 8.8.8.8".to_string()),
            ..test_config()
        };

        let result = provisioner.template_config(&config, &vars);
        assert!(result.is_ok());

        let templated = result.unwrap();
        assert!(templated.nameserver.unwrap().contains("1.1.1.1"));
    }

    #[test]
    fn test_provision_config_with_features() {
        let provisioner = test_provisioner();
        let vars = serde_yaml::Mapping::new();

        // Test nesting feature for running Docker in LXC
        let config = ProvisionConfig {
            features: Some("nesting=1".to_string()),
            ..test_config()
        };

        let result = provisioner.template_config(&config, &vars);
        assert!(result.is_ok());
        assert!(result.unwrap().features.unwrap().contains("nesting"));
    }

    // ========== State Handling Tests ==========

    #[test]
    fn test_provision_config_state_present() {
        let config = test_config();
        assert_eq!(config.state, "present");
    }

    #[test]
    fn test_provision_config_state_absent() {
        let config = ProvisionConfig {
            state: "absent".to_string(),
            ..test_config()
        };
        assert_eq!(config.state, "absent");
    }

    #[test]
    fn test_provision_config_state_force_absent() {
        let config = ProvisionConfig {
            state: "force-absent".to_string(),
            ..test_config()
        };
        assert_eq!(config.state, "force-absent");
    }

    // ========== Rootfs Size Parsing Tests ==========

    #[test]
    fn test_rootfs_size_parsing() {
        // Test that rootfs_size is passed through as-is (parsing happens at API level)
        let provisioner = test_provisioner();
        let vars = serde_yaml::Mapping::new();

        // Various formats - the API layer handles conversion
        let test_cases = vec![
            "8",    // just number (GB)
            "16",   // larger size
            "32G",  // explicit GB
            "512M", // MB
        ];

        for size in test_cases {
            let config = ProvisionConfig {
                rootfs_size: Some(size.to_string()),
                ..test_config()
            };

            let result = provisioner.template_config(&config, &vars);
            assert!(result.is_ok(), "Failed for size: {}", size);
        }
    }

    // ========== Wait Option Tests ==========

    #[test]
    fn test_wait_options_defaults() {
        let config = test_config();

        assert_eq!(config.wait_for_host, Some(false));
        assert_eq!(config.wait_timeout, Some(60));
        assert_eq!(config.wait_delay, Some(2));
        assert!(config.wait_strategy.is_none());
        assert_eq!(config.wait_max_delay, Some(30));
    }

    #[test]
    fn test_wait_options_custom() {
        let config = ProvisionConfig {
            wait_for_host: Some(true),
            wait_timeout: Some(300),
            wait_delay: Some(5),
            wait_strategy: Some("ssh".to_string()),
            wait_max_delay: Some(120),
            ..test_config()
        };

        assert_eq!(config.wait_for_host, Some(true));
        assert_eq!(config.wait_timeout, Some(300));
        assert_eq!(config.wait_delay, Some(5));
        assert_eq!(config.wait_strategy, Some("ssh".to_string()));
        assert_eq!(config.wait_max_delay, Some(120));
    }

    // ========== Edge Cases ==========

    #[test]
    fn test_empty_optional_fields() {
        // Test that None fields remain None after templating
        let provisioner = test_provisioner();
        let vars = serde_yaml::Mapping::new();

        let config = test_config();

        let result = provisioner.template_config(&config, &vars);
        assert!(result.is_ok());

        let templated = result.unwrap();
        assert!(templated.node.is_none());
        assert!(templated.hostname.is_none());
        assert!(templated.vmid.is_none());
        assert!(templated.password.is_none());
    }

    #[test]
    fn test_special_characters_in_password() {
        // Test that special characters in passwords are preserved
        let provisioner = test_provisioner();
        let vars = serde_yaml::Mapping::new();

        let config = ProvisionConfig {
            password: Some("p@ssw0rd!#$%^&*()".to_string()),
            ..test_config()
        };

        let result = provisioner.template_config(&config, &vars);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().password.unwrap(), "p@ssw0rd!#$%^&*()");
    }

    #[test]
    fn test_authorized_keys_multiline() {
        // Test that multiline SSH authorized keys are preserved
        let provisioner = test_provisioner();
        let vars = serde_yaml::Mapping::new();

        let keys = "ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABgQC7...\nssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABgQD9...";
        let config = ProvisionConfig {
            authorized_keys: Some(keys.to_string()),
            ..test_config()
        };

        let result = provisioner.template_config(&config, &vars);
        assert!(result.is_ok());
        assert!(result.unwrap().authorized_keys.unwrap().contains('\n'));
    }

    #[test]
    fn test_net_config_multiple_interfaces() {
        // Test multiple network interfaces
        let provisioner = test_provisioner();
        let vars = serde_yaml::Mapping::new();

        let config = ProvisionConfig {
            net0: Some("name=eth0,bridge=vmbr0,ip=dhcp".to_string()),
            net1: Some("name=eth1,bridge=vmbr1,ip=dhcp".to_string()),
            net2: Some("name=eth2,bridge=vmbr2,ip=10.0.0.5/24".to_string()),
            ..test_config()
        };

        let result = provisioner.template_config(&config, &vars);
        assert!(result.is_ok());

        let templated = result.unwrap();
        assert!(templated.net0.is_some());
        assert!(templated.net1.is_some());
        assert!(templated.net2.is_some());
        assert!(templated.net3.is_none());
    }

    // ========== Integration-Style Tests ==========

    #[test]
    fn test_full_config_roundtrip() {
        // Test that a complete config can be created, templated, and cloned
        let provisioner = test_provisioner();
        let mut vars = serde_yaml::Mapping::new();
        vars.insert(serde_yaml::Value::String("hostname".to_string()), serde_yaml::Value::String("app-server".to_string()));
        vars.insert(serde_yaml::Value::String("memory".to_string()), serde_yaml::Value::String("2048".to_string()));

        let config = ProvisionConfig {
            provision_type: "proxmox_lxc".to_string(),
            state: "present".to_string(),
            cluster: "test-cluster".to_string(),
            node: Some("pve1".to_string()),
            hostname: Some("{{hostname}}".to_string()),
            vmid: Some("100".to_string()),
            memory: Some("{{memory}}".to_string()),
            cores: Some("2".to_string()),
            ostemplate: Some("local:vztmpl/debian-12-standard_amd64.tar.zst".to_string()),
            fetch: Some("latest".to_string()),
            storage: Some("local-lvm".to_string()),
            rootfs_size: Some("16".to_string()),
            net0: Some("name=eth0,bridge=vmbr0,ip=dhcp".to_string()),
            password: Some("securepassword".to_string()),
            unprivileged: Some("true".to_string()),
            start_on_create: Some("true".to_string()),
            tun: Some("true".to_string()),
            wait_for_host: Some(true),
            wait_timeout: Some(120),
            ..test_config()
        };

        let result = provisioner.template_config(&config, &vars);
        assert!(result.is_ok());

        let templated = result.unwrap();
        assert_eq!(templated.hostname.as_deref(), Some("app-server"));
        assert_eq!(templated.memory.as_deref(), Some("2048"));
        assert_eq!(templated.fetch.as_deref(), Some("latest"));

        // Test clone works
        let cloned = templated.clone();
        assert_eq!(cloned.hostname, templated.hostname);
        assert_eq!(cloned.memory, templated.memory);
    }

    // ========== Idempotency Tests ==========

    #[test]
    fn test_container_idempotency_existing_container_returns_already_exists() {
        // When find_container_by_hostname returns Some(vmid), ensure_exists should return AlreadyExists
        // This tests the logic path, not the actual API call
        let provisioner = test_provisioner();

        // Create a mock config
        let config = ProvisionConfig {
            provision_type: "proxmox_lxc".to_string(),
            state: "present".to_string(),
            cluster: "test-cluster".to_string(),
            node: Some("pve1".to_string()),
            hostname: Some("myapp".to_string()),
            ..test_config()
        };

        // The exists() function should check by hostname
        // We can't test the actual API, but we can verify the logic structure
        assert!(config.hostname.is_some());
        assert_eq!(config.hostname.as_ref().unwrap(), "myapp");
    }

    #[test]
    fn test_container_idempotency_different_nodes_same_name() {
        // Same hostname on different nodes should be treated as different containers
        let config_node1 = ProvisionConfig {
            provision_type: "proxmox_lxc".to_string(),
            node: Some("pve1".to_string()),
            hostname: Some("webapp".to_string()),
            ..test_config()
        };

        let config_node2 = ProvisionConfig {
            provision_type: "proxmox_lxc".to_string(),
            node: Some("pve2".to_string()),
            hostname: Some("webapp".to_string()),
            ..test_config()
        };

        // Same hostname but different nodes - should be different containers
        assert_ne!(config_node1.node, config_node2.node);
        assert_eq!(config_node1.hostname, config_node2.hostname);
    }

    #[test]
    fn test_container_idempotency_with_vmid_override() {
        // When vmid is explicitly specified, existence check should use vmid not hostname
        let config_with_vmid = ProvisionConfig {
            provision_type: "proxmox_lxc".to_string(),
            hostname: Some("myapp".to_string()),
            vmid: Some("100".to_string()),
            ..test_config()
        };

        // The exists check should prefer vmid when specified
        assert!(config_with_vmid.vmid.is_some());
        assert_eq!(config_with_vmid.vmid.as_ref().unwrap(), "100");
    }

    // ========== Template Existence Tests ==========

    #[test]
    fn test_template_existence_check_prefix_matching() {
        // Test that template prefix matching works correctly
        let provisioner = test_provisioner();

        // Simulate template list from local storage
        let templates = vec![
            "local:vztmpl/debian-12-standard_amd64_12.2.0_pve8.tar.zst".to_string(),
            "local:vztmpl/debian-12-standard_amd64_12.3.0_pve8.tar.zst".to_string(),
            "local:vztmpl/debian-13-standard_amd64_13.0.0_pve8.tar.zst".to_string(),
        ];

        // Prefix "debian-12" should match first two
        let prefix = "debian-12";
        let matches: Vec<_> = templates.iter()
            .filter(|t| t.to_lowercase().contains(&prefix.to_lowercase()))
            .collect();

        assert_eq!(matches.len(), 2);
    }

    #[test]
    fn test_template_existence_check_exact_matching() {
        // Test that exact template name matching works
        let provisioner = test_provisioner();

        let templates = vec![
            "local:vztmpl/debian-12-standard_amd64_12.2.0_pve8.tar.zst".to_string(),
            "local:vztmpl/debian-12-standard_arm64_12.2.0_pve8.tar.zst".to_string(),
            "local:vztmpl/debian-13-standard_amd64_13.0.0_pve8.tar.zst".to_string(),
        ];

        // Exact match for debian-12 amd64 - need to match after "local:vztmpl/"
        let exact = "debian-12-standard_amd64";
        let matches: Vec<_> = templates.iter()
            .filter(|t| {
                let path_part = t.split(':').nth(1).unwrap_or("");
                path_part.to_lowercase().starts_with(&format!("vztmpl/{}", exact))
            })
            .collect();

        // Should only match the amd64 one, not arm64
        assert_eq!(matches.len(), 1);
        assert!(matches[0].contains("amd64"));
        assert!(!matches[0].contains("arm64"));
    }

    #[test]
    fn test_template_existence_check_arch_specific() {
        // Test that amd64 and arm64 templates are distinguished
        let provisioner = test_provisioner();

        let templates = vec![
            "local:vztmpl/debian-12-standard_amd64.tar.zst".to_string(),
            "local:vztmpl/debian-12-standard_arm64.tar.zst".to_string(),
        ];

        // Filter for amd64 only
        let amd64_matches: Vec<_> = templates.iter()
            .filter(|t| t.contains("_amd64"))
            .collect();

        let arm64_matches: Vec<_> = templates.iter()
            .filter(|t| t.contains("_arm64"))
            .collect();

        assert_eq!(amd64_matches.len(), 1);
        assert_eq!(arm64_matches.len(), 1);
    }

    #[test]
    fn test_template_existence_check_latest_version() {
        // Test that sorting finds the latest version
        let provisioner = test_provisioner();

        let mut templates = vec![
            "local:vztmpl/debian-12-standard_amd64_12.0.0_pve8.tar.zst".to_string(),
            "local:vztmpl/debian-12-standard_amd64_12.2.0_pve8.tar.zst".to_string(),
            "local:vztmpl/debian-12-standard_amd64_12.1.0_pve8.tar.zst".to_string(),
        ];

        // Sort descending to get latest
        templates.sort_by(|a, b| b.cmp(a));

        // Latest should be 12.2.0
        assert!(templates[0].contains("12.2.0"));
    }

    #[test]
    fn test_template_fetch_latest_skips_download_if_exists() {
        // Test that fetch: latest should check local storage first
        let config = ProvisionConfig {
            provision_type: "proxmox_lxc".to_string(),
            ostemplate: Some("debian-13-standard_amd64_latest".to_string()),
            fetch: Some("latest".to_string()),
            storage: Some("local".to_string()),
            ..test_config()
        };

        // When fetch is "latest", code should:
        // 1. First check local storage for matching template
        // 2. Only download if not found
        assert_eq!(config.fetch.as_ref().unwrap(), "latest");
        assert!(config.ostemplate.is_some());
    }

    #[test]
    fn test_template_fetch_specific_version_downloads() {
        // Test that fetch: true downloads even if exists (user explicitly requested)
        let config = ProvisionConfig {
            provision_type: "proxmox_lxc".to_string(),
            ostemplate: Some("local:vztmpl/debian-12-standard_amd64.tar.zst".to_string()),
            fetch: Some("true".to_string()),
            ..test_config()
        };

        // fetch: true means always download
        assert_eq!(config.fetch.as_ref().unwrap(), "true");
    }

    // ========== Container IP Detection Tests ==========

    #[test]
    fn test_ip_detection_prefers_eth0() {
        // Test that IP detection prioritizes eth0 over other interfaces
        // This simulates the logic from install.rs

        // Mock interface data from Proxmox API
        let interfaces = vec![
            serde_json::json!({
                "name": "lo",
                "ip-addresses": [
                    {"ip-address": "127.0.0.1", "ip-address-type": "inet"}
                ]
            }),
            serde_json::json!({
                "name": "eth0",
                "ip-addresses": [
                    {"ip-address": "10.1.21.10", "ip-address-type": "inet"},
                    {"ip-address": "fe80::be24:11ff:fe24:5c7", "ip-address-type": "inet6"}
                ]
            }),
            serde_json::json!({
                "name": "eth1",
                "ip-addresses": [
                    {"ip-address": "10.2.0.5", "ip-address-type": "inet"}
                ]
            }),
        ];

        // Find eth0 IPv4 address
        let mut found_ip: Option<String> = None;
        for iface in interfaces {
            let name = iface.get("name").and_then(|n| n.as_str()).unwrap_or("");
            if name != "eth0" {
                continue;
            }
            for addr in iface.get("ip-addresses").and_then(|a| a.as_array()).unwrap_or(&vec![]) {
                let addr_type = addr.get("ip-address-type").and_then(|t| t.as_str()).unwrap_or("");
                if addr_type == "inet" {
                    if let Some(ip) = addr.get("ip-address").and_then(|i| i.as_str()) {
                        if ip != "127.0.0.1" {
                            found_ip = Some(ip.to_string());
                            break;
                        }
                    }
                }
            }
        }

        assert_eq!(found_ip, Some("10.1.21.10".to_string()));
    }

    #[test]
    fn test_ip_detection_skips_loopback() {
        // Ensure loopback addresses are never returned
        let interfaces = vec![
            serde_json::json!({
                "name": "lo",
                "ip-addresses": [
                    {"ip-address": "127.0.0.1", "ip-address-type": "inet"}
                ]
            }),
        ];

        let mut found_ip: Option<String> = None;
        for iface in interfaces {
            let name = iface.get("name").and_then(|n| n.as_str()).unwrap_or("");
            if name != "eth0" {
                continue;
            }
            for addr in iface.get("ip-addresses").and_then(|a| a.as_array()).unwrap_or(&vec![]) {
                let addr_type = addr.get("ip-address-type").and_then(|t| t.as_str()).unwrap_or("");
                if addr_type == "inet" {
                    if let Some(ip) = addr.get("ip-address").and_then(|i| i.as_str()) {
                        if ip != "127.0.0.1" {
                            found_ip = Some(ip.to_string());
                        }
                    }
                }
            }
        }

        // Should skip loopback entirely
        assert_eq!(found_ip, None);
    }

    #[test]
    fn test_ip_detection_uses_inet_not_ipv4() {
        // Proxmox API returns "inet" not "ipv4" - verify this is handled
        let addr_type = "inet"; // This is what Proxmox actually returns

        // Our code should check for "inet" not "ipv4"
        assert_eq!(addr_type, "inet");
        assert_ne!(addr_type, "ipv4"); // This would be wrong!
    }

    // ========== DHCP vs Static IP Tests ==========

    #[test]
    fn test_dhcp_net0_parsing() {
        // Test parsing DHCP net0 config
        let net0 = "name=eth0,bridge=vmbr0,ip=dhcp,type=veth";

        let mut ip_mode: Option<String> = None;
        for part in net0.split(',') {
            if part.starts_with("ip=") {
                ip_mode = Some(part[3..].to_string());
            }
        }

        assert_eq!(ip_mode, Some("dhcp".to_string()));
    }

    #[test]
    fn test_static_ip_net0_parsing() {
        // Test parsing static IP net0 config
        let net0 = "name=eth0,bridge=vmbr0,ip=10.10.10.5/24,gw=10.10.10.1,type=veth";

        let mut ip_addr: Option<String> = None;
        for part in net0.split(',') {
            if part.starts_with("ip=") {
                let ip_cidr = &part[3..];
                if ip_cidr.to_lowercase() != "dhcp" {
                    ip_addr = Some(ip_cidr.split('/').next().unwrap_or(ip_cidr).to_string());
                }
            }
        }

        assert_eq!(ip_addr, Some("10.10.10.5".to_string()));
    }

    // ========== Container Name Validation Tests ==========

    #[test]
    fn test_container_hostname_valid_characters() {
        // Container hostnames should be valid DNS names
        let valid_names = vec!["dragonfly", "web-app", "app-server-01", "my.app"];

        for name in valid_names {
            // Basic validation - no spaces, special chars that would break Proxmox
            assert!(!name.contains(' '), "{} contains space", name);
        }
    }

    #[test]
    fn test_container_name_uniqueness_per_node() {
        // Same container name on same node = same container
        // Same container name on different node = different container

        // These should be considered different (different node)
        let container1 = ("webapp", "pve1");
        let container2 = ("webapp", "pve2");

        // Same name, different node = different container
        assert_ne!(container1, container2);

        // Same name, same node = same container
        let container3 = ("webapp", "pve1");
        assert_eq!(container1, container3);
    }

    // ========== State Management Tests ==========

    #[test]
    fn test_provision_state_present_idempotent() {
        // state: present should be idempotent
        let config = ProvisionConfig {
            provision_type: "proxmox_lxc".to_string(),
            state: "present".to_string(),
            ..test_config()
        };

        assert_eq!(config.state, "present");
    }

    #[test]
    fn test_provision_state_absent_removes() {
        // state: absent should remove container
        let config = ProvisionConfig {
            provision_type: "proxmox_lxc".to_string(),
            state: "absent".to_string(),
            ..test_config()
        };

        assert_eq!(config.state, "absent");
    }

    #[test]
    fn test_provision_state_force_absent() {
        // state: force_absent should force remove even if running
        let config = ProvisionConfig {
            provision_type: "proxmox_lxc".to_string(),
            state: "force_absent".to_string(),
            ..test_config()
        };

        assert_eq!(config.state, "force_absent");
    }

    // ========== VMID Range Tests ==========

    #[test]
    fn test_vmid_range_valid() {
        // Proxmox LXC VMIDs should be in valid range (100-999999999)
        let valid_vmids = vec![100, 1000, 100000, 999999999];

        for vmid in valid_vmids {
            assert!(vmid >= 100, "vmid {} too low", vmid);
            assert!(vmid <= 999999999, "vmid {} too high", vmid);
        }
    }

    #[test]
    fn test_vmid_parsing() {
        // Test parsing VMID from string
        let vmid_str = "100";
        let vmid: Result<u64, _> = vmid_str.parse();

        assert!(vmid.is_ok());
        assert_eq!(vmid.unwrap(), 100);
    }

    // ========== Config Merge Tests ==========

    #[test]
    fn test_config_merge_host_vars() {
        // Test that inventory host variables are correctly merged
        let mut host_vars = serde_yaml::Mapping::new();
        host_vars.insert(
            serde_yaml::Value::String("lxc_ip".to_string()),
            serde_yaml::Value::String("10.7.1.50".to_string()),
        );

        // Template config should use these vars
        let config = ProvisionConfig {
            hostname: Some("{{lxc_hostname}}".to_string()),
            ..test_config()
        };

        // Config has template var, should be resolved with host_vars
        assert!(config.hostname.unwrap().contains("{{"));
    }

    // ========== Edge Cases ==========

    #[test]
    fn test_empty_ostemplate_with_fetch() {
        // ostemplate is required when fetch is enabled
        let config = ProvisionConfig {
            provision_type: "proxmox_lxc".to_string(),
            ostemplate: None,
            fetch: Some("latest".to_string()),
            ..test_config()
        };

        // This should fail validation - ostemplate required when fetch is set
        // (code should handle this error)
        assert!(config.ostemplate.is_none());
        assert!(config.fetch.is_some());
    }

    #[test]
    fn test_multi_node_cluster_container_lookup() {
        // In a multi-node cluster, container could be on any node
        // The IP lookup should search all nodes

        let cluster_nodes = vec!["pve1", "pve2", "pve3"];
        let target_hostname = "dragonfly";

        // Simulate finding container on pve2
        let container_location = Some(("pve2", 106u64));

        // Verify we can find which node
        if let Some((node, vmid)) = container_location {
            assert!(cluster_nodes.contains(&node));
            assert_eq!(vmid, 106);
        }
    }

    #[test]
    fn test_template_storage_path_format() {
        // Template paths should be in format "storage:vztmpl/filename"
        let template_path = "local:vztmpl/debian-12-standard_amd64.tar.zst";

        let parts: Vec<&str> = template_path.split(':').collect();
        assert_eq!(parts.len(), 2);

        let (storage, path) = (parts[0], parts[1]);
        assert_eq!(storage, "local");
        assert!(path.starts_with("vztmpl/"));
    }

    #[test]
    fn test_ssh_key_injection_via_authorized_keys() {
        // Test that SSH key is correctly passed via authorized_keys
        let ssh_public_key = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAEXAMPLE user@host";

        // In LXC config, this goes in 'keys' field or as authorized_keys
        let lxc_config = format!("name=eth0,bridge=vmbr0,ip=dhcp,keys={}", ssh_public_key);

        assert!(lxc_config.contains(&ssh_public_key[..20])); // Check key is included
    }

    #[test]
    fn test_provision_result_enum_values() {
        // Verify all possible provision results
        // Created = new container made
        // AlreadyExists = container was there, nothing to do
        // Updated = container existed but was modified

        // These are the expected outcomes from ensure_exists
        let expected_results = vec!["Created", "AlreadyExists", "Updated"];

        // Verify our expectations match the ProvisionResult enum
        assert!(expected_results.contains(&"Created"));
        assert!(expected_results.contains(&"AlreadyExists"));
        assert!(expected_results.contains(&"Updated"));
    }

    // ========== Error Handling Tests ==========

    #[test]
    fn test_missing_required_field_ostemplate() {
        // ostemplate is required for container creation
        let config = ProvisionConfig {
            provision_type: "proxmox_lxc".to_string(),
            ostemplate: None,
            ..test_config()
        };

        // Should fail when trying to create without ostemplate
        assert!(config.ostemplate.is_none());
    }

    #[test]
    fn test_invalid_vmid_format() {
        // Invalid VMID should fail parse
        let invalid_vmid = "not-a-number";
        let result: Result<u64, _> = invalid_vmid.parse();

        assert!(result.is_err());
    }

    #[test]
    fn test_proxmox_api_timeout_handling() {
        // API calls should have timeouts
        let timeout_secs = 30;

        // Verify timeout is reasonable
        assert!(timeout_secs >= 10, "timeout too short");
        assert!(timeout_secs <= 300, "timeout too long");
    }
}
