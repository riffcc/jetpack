// Jetpack - Proxmox VM Provisioner
// Copyright (C) Riff Labs Limited <team@riff.cc>
//
// Creates empty QEMU VMs configured for PXE boot.
// VMs boot from network and get provisioned by Dragonfly.

use crate::provisioners::{ProvisionConfig, ProvisionResult, Provisioner};
use crate::inventory::inventory::Inventory;
use crate::playbooks::templar::{Templar, TemplateMode};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

pub struct ProxmoxVmProvisioner;

#[derive(Serialize, Deserialize, Debug)]
struct ProxmoxApiResponse<T> {
    data: Option<T>,
}

#[derive(Serialize, Deserialize, Debug)]
struct VmListItem {
    vmid: u64,
    status: String,
    name: Option<String>,
}

enum ProxmoxAuth {
    Token { token_id: String, token_secret: String },
    Password { ticket: String, csrf_token: String },
}

struct ClusterConnection {
    api_host: String,
    auth: ProxmoxAuth,
    node: String,
}

impl ProxmoxVmProvisioner {
    pub fn new() -> Self { Self }

    fn template_string(&self, templar: &Templar, value: &str, vars: &serde_yaml::Mapping) -> Result<String, String> {
        if value.contains("{{") {
            templar.render(&value.to_string(), vars.clone(), TemplateMode::Strict)
        } else {
            Ok(value.to_string())
        }
    }

    fn template_option(&self, templar: &Templar, value: &Option<String>, vars: &serde_yaml::Mapping) -> Result<Option<String>, String> {
        match value {
            Some(v) => Ok(Some(self.template_string(templar, v, vars)?)),
            None => Ok(None),
        }
    }

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
            wait_for_host: config.wait_for_host,
            wait_timeout: config.wait_timeout,
            wait_delay: config.wait_delay,
            wait_strategy: config.wait_strategy.clone(),
            wait_max_delay: config.wait_max_delay,
            extra: {
                let mut templated = HashMap::new();
                for (k, v) in &config.extra {
                    templated.insert(k.clone(), self.template_string(&templar, v, vars)?);
                }
                templated
            },
        })
    }

    fn get_cluster_connection(&self, config: &ProvisionConfig, inventory: &Arc<RwLock<Inventory>>) -> Result<ClusterConnection, String> {
        let inv = inventory.read().map_err(|e| format!("Failed to read inventory: {}", e))?;
        if !inv.has_host(&config.cluster) {
            return Err(format!("Cluster '{}' not found in inventory", config.cluster));
        }
        let cluster_host = inv.get_host(&config.cluster);
        let host = cluster_host.read().map_err(|e| format!("Failed to read host: {}", e))?;
        let vars = host.get_blended_variables();

        let api_host = vars.get("proxmox_api_host")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| format!("Cluster '{}' missing proxmox_api_host", config.cluster))?;

        let node = config.node.clone()
            .or_else(|| config.extra.get("node").cloned())
            .or_else(|| vars.get("proxmox_node").and_then(|v| v.as_str()).map(|s| s.to_string()))
            .ok_or_else(|| "No 'node' specified".to_string())?;

        // Token auth
        if let (Some(tid), Some(ts)) = (
            vars.get("proxmox_api_token_id").and_then(|v| v.as_str()),
            vars.get("proxmox_api_token_secret").and_then(|v| v.as_str())
        ) {
            return Ok(ClusterConnection {
                api_host,
                auth: ProxmoxAuth::Token { token_id: tid.to_string(), token_secret: ts.to_string() },
                node,
            });
        }

        // Password auth
        let username = vars.get("proxmox_api_user").and_then(|v| v.as_str())
            .ok_or_else(|| "Missing proxmox_api_user".to_string())?;
        let password = vars.get("proxmox_api_password").and_then(|v| v.as_str())
            .ok_or_else(|| "Missing proxmox_api_password".to_string())?;

        let (ticket, csrf) = self.get_ticket(&api_host, username, password)?;
        Ok(ClusterConnection {
            api_host,
            auth: ProxmoxAuth::Password { ticket, csrf_token: csrf },
            node,
        })
    }

    fn get_ticket(&self, api_host: &str, username: &str, password: &str) -> Result<(String, String), String> {
        let url = if api_host.contains(':') {
            format!("https://{}/api2/json/access/ticket", api_host)
        } else {
            format!("https://{}:8006/api2/json/access/ticket", api_host)
        };
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()
            .map_err(|e| format!("Runtime: {}", e))?;

        rt.block_on(async {
            let client = reqwest::Client::builder().danger_accept_invalid_certs(true).build()
                .map_err(|e| format!("HTTP: {}", e))?;
            let mut params = HashMap::new();
            params.insert("username", username);
            params.insert("password", password);

            let resp = client.post(&url).form(&params).send().await
                .map_err(|e| format!("Auth failed: {}", e))?;

            if !resp.status().is_success() {
                return Err(format!("Auth failed: {}", resp.status()));
            }

            #[derive(Deserialize)]
            struct TicketData { ticket: String, #[serde(rename = "CSRFPreventionToken")] csrf: String }
            #[derive(Deserialize)]
            struct TicketResp { data: TicketData }

            let tr: TicketResp = resp.json().await.map_err(|e| format!("Parse: {}", e))?;
            Ok((tr.data.ticket, tr.data.csrf))
        })
    }

    fn apply_auth(&self, conn: &ClusterConnection, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &conn.auth {
            ProxmoxAuth::Token { token_id, token_secret } =>
                builder.header("Authorization", format!("PVEAPIToken={}={}", token_id, token_secret)),
            ProxmoxAuth::Password { ticket, csrf_token } =>
                builder.header("Cookie", format!("PVEAuthCookie={}", ticket))
                       .header("CSRFPreventionToken", csrf_token),
        }
    }

    fn api_url(&self, conn: &ClusterConnection, path: &str) -> String {
        if conn.api_host.contains(':') {
            format!("https://{}/api2/json{}", conn.api_host, path)
        } else {
            format!("https://{}:8006/api2/json{}", conn.api_host, path)
        }
    }

    fn find_vm(&self, conn: &ClusterConnection, hostname: &str) -> Result<Option<u64>, String> {
        let url = self.api_url(conn, &format!("/nodes/{}/qemu", conn.node));
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()
            .map_err(|e| format!("Runtime: {}", e))?;

        rt.block_on(async {
            let client = reqwest::Client::builder().danger_accept_invalid_certs(true).build()
                .map_err(|e| format!("HTTP: {}", e))?;
            let resp = self.apply_auth(conn, client.get(&url)).send().await
                .map_err(|e| format!("API: {}", e))?;

            if !resp.status().is_success() {
                return Err(format!("API: {}", resp.status()));
            }

            let api_resp: ProxmoxApiResponse<Vec<VmListItem>> = resp.json().await
                .map_err(|e| format!("Parse: {}", e))?;

            if let Some(vms) = api_resp.data {
                for vm in vms {
                    if vm.name.as_deref() == Some(hostname) {
                        return Ok(Some(vm.vmid));
                    }
                }
            }
            Ok(None)
        })
    }

    /// Get next available VMID from Proxmox cluster
    fn get_next_vmid(&self, conn: &ClusterConnection) -> Result<u64, String> {
        let url = self.api_url(conn, "/cluster/nextid");
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()
            .map_err(|e| format!("Runtime: {}", e))?;

        rt.block_on(async {
            let client = reqwest::Client::builder().danger_accept_invalid_certs(true).build()
                .map_err(|e| format!("HTTP: {}", e))?;
            let resp = self.apply_auth(conn, client.get(&url)).send().await
                .map_err(|e| format!("API: {}", e))?;

            if !resp.status().is_success() {
                return Err(format!("Failed to get next VMID: {}", resp.status()));
            }

            let api_resp: ProxmoxApiResponse<String> = resp.json().await
                .map_err(|e| format!("Parse: {}", e))?;

            api_resp.data
                .ok_or_else(|| "No VMID returned".to_string())?
                .parse::<u64>()
                .map_err(|_| "Invalid VMID from API".to_string())
        })
    }

    /// Create an empty VM configured for PXE boot
    fn create_empty_vm(&self, conn: &ClusterConnection, config: &ProvisionConfig, hostname: &str, vmid: u64) -> Result<String, String> {
        let url = self.api_url(conn, &format!("/nodes/{}/qemu", conn.node));
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()
            .map_err(|e| format!("Runtime: {}", e))?;

        rt.block_on(async {
            let client = reqwest::Client::builder().danger_accept_invalid_certs(true).build()
                .map_err(|e| format!("HTTP: {}", e))?;

            let mut params: HashMap<String, String> = HashMap::new();
            params.insert("vmid".to_string(), vmid.to_string());
            params.insert("name".to_string(), hostname.to_string());

            // Memory (default 2048)
            let memory = config.memory.as_ref().map(|s| s.as_str()).unwrap_or("2048");
            params.insert("memory".to_string(), memory.to_string());

            // Cores (default 4)
            let cores = config.cores.as_ref().map(|s| s.as_str()).unwrap_or("4");
            params.insert("cores".to_string(), cores.to_string());

            // CPU type
            params.insert("cpu".to_string(), "host".to_string());

            // SCSI controller
            params.insert("scsihw".to_string(), "virtio-scsi-single".to_string());

            // Disk - empty disk on specified storage
            // Format: storage:size where size is in GB (e.g., "moosefs:20")
            let storage = config.storage.as_ref().map(|s| s.as_str()).unwrap_or("local-lvm");
            let disk_size = config.rootfs_size.as_ref()
                .map(|s| s.trim_end_matches('G').trim_end_matches('g'))
                .unwrap_or("20");
            params.insert("scsi0".to_string(), format!("{}:{},iothread=1", storage, disk_size));

            // Network with optional MAC address
            let net_config = if let Some(ref net0) = config.net0 {
                net0.clone()
            } else if let Some(mac) = config.extra.get("mac") {
                format!("virtio={},bridge=vmbr0", mac)
            } else {
                "virtio,bridge=vmbr0".to_string()
            };
            params.insert("net0".to_string(), net_config.clone());

            // Boot order: network first (PXE), then disk
            params.insert("boot".to_string(), "order=net0;scsi0".to_string());

            // BIOS - SeaBIOS for PXE (OVMF/UEFI can work but SeaBIOS is simpler)
            let bios = config.extra.get("bios").map(|s| s.as_str()).unwrap_or("seabios");
            params.insert("bios".to_string(), bios.to_string());

            // Enable QEMU guest agent (will work after OS install)
            params.insert("agent".to_string(), "1".to_string());

            // OS type hint
            params.insert("ostype".to_string(), "l26".to_string()); // Linux 2.6+ kernel

            // Start on create? Default true for PXE boot
            let start = config.start_on_create.as_ref()
                .map(|s| s == "true" || s == "1" || s == "yes")
                .unwrap_or(true);
            if start {
                params.insert("start".to_string(), "1".to_string());
            }

            // Extra params
            for (k, v) in &config.extra {
                if !["node", "mac", "bios", "ip", "gateway"].contains(&k.as_str()) {
                    params.insert(k.clone(), v.clone());
                }
            }

            let resp = self.apply_auth(conn, client.post(&url)).form(&params).send().await
                .map_err(|e| format!("Create VM failed: {}", e))?;

            if !resp.status().is_success() {
                let text = resp.text().await.unwrap_or_default();
                return Err(format!("Create VM failed: {}", text));
            }

            // Extract MAC address from net config for return
            let mac = if net_config.contains("=") {
                // Format: virtio=XX:XX:XX:XX:XX:XX,bridge=...
                net_config.split(',').next()
                    .and_then(|s| s.split('=').nth(1))
                    .map(|s| s.to_string())
            } else {
                None
            };

            // If we didn't specify MAC, query the VM to get the generated one
            if mac.is_none() {
                if let Ok(Some(generated_mac)) = self.get_vm_mac(conn, vmid).await {
                    return Ok(generated_mac);
                }
            }

            Ok(mac.unwrap_or_default())
        })
    }

    async fn get_vm_mac(&self, conn: &ClusterConnection, vmid: u64) -> Result<Option<String>, String> {
        let url = self.api_url(conn, &format!("/nodes/{}/qemu/{}/config", conn.node, vmid));
        let client = reqwest::Client::builder().danger_accept_invalid_certs(true).build()
            .map_err(|e| format!("HTTP: {}", e))?;

        let resp = self.apply_auth(conn, client.get(&url)).send().await
            .map_err(|e| format!("Get config: {}", e))?;

        if !resp.status().is_success() {
            return Ok(None);
        }

        #[derive(Deserialize)]
        struct VmConfig {
            net0: Option<String>,
        }

        let api_resp: ProxmoxApiResponse<VmConfig> = resp.json().await
            .map_err(|e| format!("Parse: {}", e))?;

        if let Some(cfg) = api_resp.data {
            if let Some(net0) = cfg.net0 {
                // Extract MAC from net0 config
                // Format: virtio=XX:XX:XX:XX:XX:XX,bridge=vmbr0,...
                for part in net0.split(',') {
                    if part.starts_with("virtio=") || part.starts_with("e1000=") {
                        return Ok(Some(part.split('=').nth(1).unwrap_or("").to_string()));
                    }
                }
            }
        }
        Ok(None)
    }

    fn stop_vm(&self, conn: &ClusterConnection, vmid: u64) -> Result<(), String> {
        let url = self.api_url(conn, &format!("/nodes/{}/qemu/{}/status/stop", conn.node, vmid));
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()
            .map_err(|e| format!("Runtime: {}", e))?;

        rt.block_on(async {
            let client = reqwest::Client::builder().danger_accept_invalid_certs(true).build()
                .map_err(|e| format!("HTTP: {}", e))?;
            let _ = self.apply_auth(conn, client.post(&url)).send().await;
            Ok(())
        })
    }

    fn delete_vm(&self, conn: &ClusterConnection, vmid: u64) -> Result<(), String> {
        let _ = self.stop_vm(conn, vmid);
        std::thread::sleep(std::time::Duration::from_secs(3));

        let url = self.api_url(conn, &format!("/nodes/{}/qemu/{}", conn.node, vmid));
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()
            .map_err(|e| format!("Runtime: {}", e))?;

        rt.block_on(async {
            let client = reqwest::Client::builder().danger_accept_invalid_certs(true).build()
                .map_err(|e| format!("HTTP: {}", e))?;

            let resp = self.apply_auth(conn, client.delete(&url)).send().await
                .map_err(|e| format!("Delete: {}", e))?;

            if !resp.status().is_success() {
                let text = resp.text().await.unwrap_or_default();
                return Err(format!("Delete failed: {}", text));
            }
            Ok(())
        })
    }
}

impl Provisioner for ProxmoxVmProvisioner {
    fn exists(&self, config: &ProvisionConfig, inventory_name: &str, inventory: &Arc<RwLock<Inventory>>) -> Result<bool, String> {
        let conn = self.get_cluster_connection(config, inventory)?;
        let hostname = config.hostname.as_deref().unwrap_or(inventory_name);

        // Check by VMID if specified
        if let Some(ref vmid_str) = config.vmid {
            if let Ok(vmid) = vmid_str.parse::<u64>() {
                let url = self.api_url(&conn, &format!("/nodes/{}/qemu/{}/status/current", conn.node, vmid));
                let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()
                    .map_err(|e| format!("Runtime: {}", e))?;

                return rt.block_on(async {
                    let client = reqwest::Client::builder().danger_accept_invalid_certs(true).build()
                        .map_err(|e| format!("HTTP: {}", e))?;
                    let resp = self.apply_auth(&conn, client.get(&url)).send().await
                        .map_err(|e| format!("API: {}", e))?;
                    Ok(resp.status().is_success())
                });
            }
        }

        Ok(self.find_vm(&conn, hostname)?.is_some())
    }

    fn ensure_exists(&self, config: &ProvisionConfig, inventory_name: &str, inventory: &Arc<RwLock<Inventory>>) -> Result<ProvisionResult, String> {
        let inv_name = inventory_name.to_string();
        let host_vars = {
            let inv = inventory.read().map_err(|e| format!("Inventory: {}", e))?;
            if inv.has_host(&inv_name) {
                let host = inv.get_host(&inv_name);
                let h = host.read().map_err(|e| format!("Host: {}", e))?;
                h.get_blended_variables()
            } else {
                serde_yaml::Mapping::new()
            }
        };

        let config = self.template_config(config, &host_vars)?;
        let conn = self.get_cluster_connection(&config, inventory)?;
        let hostname = config.hostname.as_deref().unwrap_or(inventory_name);

        // Check if exists
        if self.find_vm(&conn, hostname)?.is_some() {
            return Ok(ProvisionResult::AlreadyExists);
        }

        // Get VMID - use specified or auto-assign from Proxmox
        let vmid = if let Some(ref vmid_str) = config.vmid {
            vmid_str.parse::<u64>().map_err(|_| "Invalid vmid".to_string())?
        } else {
            self.get_next_vmid(&conn)?
        };

        // Create empty VM for PXE boot
        let mac = self.create_empty_vm(&conn, &config, hostname, vmid)?;

        // Log MAC for Dragonfly registration
        if !mac.is_empty() {
            eprintln!("VM {} created with MAC: {}", hostname, mac);
        }

        Ok(ProvisionResult::Created)
    }

    fn get_ip(&self, _config: &ProvisionConfig, _inventory_name: &str, _inventory: &Arc<RwLock<Inventory>>) -> Result<Option<String>, String> {
        // IP comes from Dragonfly after PXE boot, not from Proxmox
        Ok(None)
    }

    fn destroy(&self, config: &ProvisionConfig, inventory_name: &str, inventory: &Arc<RwLock<Inventory>>) -> Result<(), String> {
        let conn = self.get_cluster_connection(config, inventory)?;
        let hostname = config.hostname.as_deref().unwrap_or(inventory_name);

        if let Some(vmid) = self.find_vm(&conn, hostname)? {
            self.delete_vm(&conn, vmid)?;
        }
        Ok(())
    }
}
