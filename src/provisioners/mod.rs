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

//! Provisioners module - infrastructure as code for Jetpack
//!
//! This module enables declarative infrastructure provisioning. Hosts defined
//! in inventory can include a `provision:` block that specifies how to create
//! the underlying infrastructure (VMs, containers, etc.) before configuration.
//!
//! Example host_vars:
//! ```yaml
//! provision:
//!   type: proxmox_lxc
//!   cluster: jasmine          # Reference to Proxmox host in inventory
//!   hostname: gravity1        # Container hostname (optional, defaults to inventory name)
//!   memory: 1536
//!   cores: 1
//!   ostemplate: "local:vztmpl/debian-12-standard.tar.zst"
//!   net0: "name=eth0,bridge=vmbr0,ip=10.10.10.2/24,gw=10.10.10.1"
//! ```

pub mod proxmox_lxc;
pub mod proxmox_vm;

use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::RwLock;
use crate::dns::DnsConfig;
use crate::inventory::inventory::Inventory;

/// Default state for provisioning (present)
fn default_state() -> String {
    "present".to_string()
}

/// Configuration for provisioning a host's underlying infrastructure
#[derive(Debug, Clone, Deserialize)]
pub struct ProvisionConfig {
    /// The type of provisioner (e.g., "proxmox_lxc", "proxmox_vm", "docker")
    #[serde(rename = "type")]
    pub provision_type: String,

    /// Desired state: "present" (default) or "absent" (destroy)
    #[serde(default = "default_state")]
    pub state: String,

    /// Reference to the cluster/hypervisor host in inventory
    pub cluster: String,

    /// Hostname for the VM/container (defaults to inventory name if not specified)
    pub hostname: Option<String>,

    /// VM ID (optional - auto-assigned if not specified)
    pub vmid: Option<String>,

    /// Memory in MB
    pub memory: Option<String>,

    /// Number of CPU cores
    pub cores: Option<String>,

    /// OS template path (for containers)
    pub ostemplate: Option<String>,

    /// Storage location
    pub storage: Option<String>,

    /// Root filesystem size
    pub rootfs_size: Option<String>,

    /// Network configuration (net0, net1, etc.)
    pub net0: Option<String>,
    pub net1: Option<String>,
    pub net2: Option<String>,
    pub net3: Option<String>,

    /// Root password
    pub password: Option<String>,

    /// SSH authorized keys (public keys to add to the container/VM)
    pub authorized_keys: Option<String>,

    /// SSH user for the provisioned host
    pub ssh_user: Option<String>,

    /// Run as unprivileged container
    pub unprivileged: Option<String>,

    /// Start container after creation
    pub start_on_create: Option<String>,

    /// Additional features (nesting, etc.)
    pub features: Option<String>,

    /// Enable TUN device (for VPN software like Tailscale, WireGuard)
    pub tun: Option<String>,

    /// DNS nameservers (space-separated, e.g. "1.1.1.1 8.8.8.8")
    pub nameserver: Option<String>,

    /// Wait for host SSH to become available after provisioning (default: true)
    pub wait_for_host: Option<bool>,

    /// Timeout in seconds for wait_for_host (default: 300)
    pub wait_timeout: Option<u64>,

    /// Initial delay between SSH connection attempts in seconds (default: 2)
    pub wait_delay: Option<u64>,

    /// Wait strategy: "simple" (fixed delay) or "backoff" (exponential backoff, default)
    pub wait_strategy: Option<String>,

    /// Maximum delay for backoff strategy in seconds (default: 30)
    pub wait_max_delay: Option<u64>,

    /// Additional fields (mountpoints mp0, mp1, etc. and other dynamic options)
    /// Mountpoint format: "storage:size,mp=/mount/path" e.g. "local-lvm:75,mp=/mnt/data"
    #[serde(flatten)]
    pub extra: HashMap<String, String>,
}

/// Result of a provisioning operation
#[derive(Debug)]
pub enum ProvisionResult {
    /// Host already exists, no action needed
    AlreadyExists,
    /// Host was created
    Created,
    /// Host was updated (config changed)
    Updated,
    /// Host was destroyed (state: absent)
    Destroyed,
}

/// Wait strategy for SSH connection attempts
#[derive(Debug, Clone, PartialEq)]
pub enum WaitStrategy {
    /// Fixed delay between attempts
    Simple,
    /// Exponential backoff with configurable max delay
    Backoff,
}

impl WaitStrategy {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "simple" => WaitStrategy::Simple,
            _ => WaitStrategy::Backoff,
        }
    }
}

/// Wait for SSH to become available on a host
/// Returns Ok(()) if SSH becomes available, Err with message if timeout
pub fn wait_for_ssh(
    ip: &str,
    port: u16,
    _user: &str,  // Reserved for future SSH authentication check
    config: &ProvisionConfig,
    host_name: &str,
) -> Result<(), String> {
    use std::net::TcpStream;
    use std::time::{Duration, Instant};
    use std::thread;

    // Check if wait is disabled
    if config.wait_for_host == Some(false) {
        return Ok(());
    }

    let timeout = config.wait_timeout.unwrap_or(300);
    let initial_delay = config.wait_delay.unwrap_or(2);
    let max_delay = config.wait_max_delay.unwrap_or(30);
    let strategy = config.wait_strategy.as_ref()
        .map(|s| WaitStrategy::from_str(s))
        .unwrap_or(WaitStrategy::Backoff);

    let start = Instant::now();
    let timeout_duration = Duration::from_secs(timeout);
    let mut current_delay = initial_delay;
    let mut attempt = 0;

    eprintln!("  → waiting for SSH on {}:{} (timeout: {}s)", ip, port, timeout);

    loop {
        attempt += 1;

        // Try TCP connection first (quick check if port is open)
        let addr = format!("{}:{}", ip, port);
        match TcpStream::connect_timeout(
            &addr.parse().map_err(|e| format!("Invalid address: {}", e))?,
            Duration::from_secs(5),
        ) {
            Ok(_) => {
                // Port is open, try SSH connection
                // We use a simple TCP check for now - if port 22 responds, SSH is likely ready
                let elapsed = start.elapsed().as_secs();
                eprintln!("  → SSH available on {} after {}s ({} attempts)", host_name, elapsed, attempt);
                return Ok(());
            }
            Err(_) => {
                // Connection failed, check timeout
                if start.elapsed() >= timeout_duration {
                    return Err(format!(
                        "Timeout waiting for SSH on {} after {}s ({} attempts)",
                        host_name, timeout, attempt
                    ));
                }

                // Calculate next delay based on strategy
                let sleep_duration = Duration::from_secs(current_delay);
                thread::sleep(sleep_duration);

                if strategy == WaitStrategy::Backoff {
                    // Exponential backoff: double the delay up to max
                    current_delay = (current_delay * 2).min(max_delay);
                }
            }
        }
    }
}

/// Trait for provisioner implementations
pub trait Provisioner: Send + Sync {
    /// Check if the host infrastructure exists
    fn exists(&self, config: &ProvisionConfig, inventory_name: &str, inventory: &Arc<RwLock<Inventory>>) -> Result<bool, String>;

    /// Ensure the host infrastructure exists, creating if necessary
    fn ensure_exists(&self, config: &ProvisionConfig, inventory_name: &str, inventory: &Arc<RwLock<Inventory>>) -> Result<ProvisionResult, String>;

    /// Get the IP address of the provisioned host (for connection)
    fn get_ip(&self, config: &ProvisionConfig, inventory_name: &str, inventory: &Arc<RwLock<Inventory>>) -> Result<Option<String>, String>;

    /// Destroy the host infrastructure
    fn destroy(&self, config: &ProvisionConfig, inventory_name: &str, inventory: &Arc<RwLock<Inventory>>) -> Result<(), String>;
}

/// Get the appropriate provisioner for a provision type
pub fn get_provisioner(provision_type: &str) -> Result<Box<dyn Provisioner>, String> {
    match provision_type {
        "proxmox_lxc" => Ok(Box::new(proxmox_lxc::ProxmoxLxcProvisioner::new())),
        "proxmox_vm" => Ok(Box::new(proxmox_vm::ProxmoxVmProvisioner::new())),
        _ => Err(format!("Unknown provisioner type: {}", provision_type))
    }
}

/// Ensure a host is provisioned before attempting to connect
/// Also creates DNS records if dns config is present in vars
/// If state is "absent", destroys the host instead
pub fn ensure_host_provisioned(
    provision_config: &ProvisionConfig,
    inventory_name: &str,
    inventory: &Arc<RwLock<Inventory>>,
    dns_config: Option<&DnsConfig>,
) -> Result<ProvisionResult, String> {
    // Handle state: absent - destroy instead of provision
    if provision_config.state == "absent" {
        destroy_host(provision_config, inventory_name, inventory, dns_config)?;
        return Ok(ProvisionResult::Destroyed);
    }

    let provisioner = get_provisioner(&provision_config.provision_type)?;
    let result = provisioner.ensure_exists(provision_config, inventory_name, inventory)?;

    // Create DNS record if dns config is present and we have an IP
    if let Some(dns_config) = dns_config {
        if let Ok(Some(ip)) = provisioner.get_ip(provision_config, inventory_name, inventory) {
            match crate::dns::add_host_record(dns_config, inventory_name, &ip) {
                Ok(true) => {
                    let hostname = crate::dns::extract_hostname(inventory_name);
                    let zone = dns_config.zone.clone()
                        .or_else(|| crate::dns::infer_zone(inventory_name))
                        .unwrap_or_else(|| "?".to_string());
                    eprintln!("  → DNS: added {} -> {} to {}", hostname, ip, zone);
                }
                Ok(false) => {
                    // Record already exists with same value, no action needed
                }
                Err(e) => {
                    eprintln!("  → DNS: warning: failed to add record: {}", e);
                }
            }
        }
    }

    Ok(result)
}

/// Destroy a host's infrastructure
/// Also removes DNS records if dns config is present in vars
pub fn destroy_host(
    provision_config: &ProvisionConfig,
    inventory_name: &str,
    inventory: &Arc<RwLock<Inventory>>,
    dns_config: Option<&DnsConfig>,
) -> Result<(), String> {
    // Remove DNS record first if dns config is present
    if let Some(dns_config) = dns_config {
        match crate::dns::remove_host_record(dns_config, inventory_name) {
            Ok(true) => {
                let hostname = crate::dns::extract_hostname(inventory_name);
                let zone = dns_config.zone.clone()
                    .or_else(|| crate::dns::infer_zone(inventory_name))
                    .unwrap_or_else(|| "?".to_string());
                eprintln!("  → DNS: removed {} from {}", hostname, zone);
            }
            Ok(false) => {
                // Record didn't exist, no action needed
            }
            Err(e) => {
                eprintln!("  → DNS: warning: failed to remove record: {}", e);
            }
        }
    }

    let provisioner = get_provisioner(&provision_config.provision_type)?;
    provisioner.destroy(provision_config, inventory_name, inventory)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_provision_config_basic_deserialization() {
        let yaml = r#"
type: proxmox_lxc
cluster: hypervisor1
hostname: testhost
memory: "1024"
cores: "2"
"#;
        let config: ProvisionConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.provision_type, "proxmox_lxc");
        assert_eq!(config.cluster, "hypervisor1");
        assert_eq!(config.hostname, Some("testhost".to_string()));
        assert_eq!(config.memory, Some("1024".to_string()));
        assert_eq!(config.cores, Some("2".to_string()));
        assert!(config.extra.is_empty());
    }

    #[test]
    fn test_provision_config_with_mountpoints() {
        let yaml = r#"
type: proxmox_lxc
cluster: hypervisor1
hostname: chunkserver1
memory: "1024"
mp0: "local-lvm:75,mp=/mnt/chunk0"
mp1: "local-lvm:75,mp=/mnt/chunk1"
"#;
        let config: ProvisionConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.provision_type, "proxmox_lxc");
        assert_eq!(config.cluster, "hypervisor1");
        assert_eq!(config.extra.len(), 2);
        assert_eq!(config.extra.get("mp0"), Some(&"local-lvm:75,mp=/mnt/chunk0".to_string()));
        assert_eq!(config.extra.get("mp1"), Some(&"local-lvm:75,mp=/mnt/chunk1".to_string()));
    }

    #[test]
    fn test_provision_config_with_many_mountpoints() {
        let yaml = r#"
type: proxmox_lxc
cluster: hypervisor1
mp0: "local-lvm:100,mp=/mnt/data0"
mp5: "local-lvm:100,mp=/mnt/data5"
mp10: "local-lvm:100,mp=/mnt/data10"
mp50: "local-lvm:100,mp=/mnt/data50"
"#;
        let config: ProvisionConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.extra.len(), 4);
        assert_eq!(config.extra.get("mp0"), Some(&"local-lvm:100,mp=/mnt/data0".to_string()));
        assert_eq!(config.extra.get("mp5"), Some(&"local-lvm:100,mp=/mnt/data5".to_string()));
        assert_eq!(config.extra.get("mp10"), Some(&"local-lvm:100,mp=/mnt/data10".to_string()));
        assert_eq!(config.extra.get("mp50"), Some(&"local-lvm:100,mp=/mnt/data50".to_string()));
    }

    #[test]
    fn test_provision_config_extra_fields_passthrough() {
        // Test that any extra fields are captured, not just mp*
        let yaml = r#"
type: proxmox_lxc
cluster: hypervisor1
mp0: "local-lvm:75,mp=/mnt/data"
hookscript: "local:snippets/hookscript.pl"
tags: "production;database"
"#;
        let config: ProvisionConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.extra.len(), 3);
        assert_eq!(config.extra.get("mp0"), Some(&"local-lvm:75,mp=/mnt/data".to_string()));
        assert_eq!(config.extra.get("hookscript"), Some(&"local:snippets/hookscript.pl".to_string()));
        assert_eq!(config.extra.get("tags"), Some(&"production;database".to_string()));
    }

    #[test]
    fn test_provision_config_with_all_standard_fields() {
        let yaml = r#"
type: proxmox_lxc
cluster: hypervisor1
hostname: fulltest
vmid: "100"
memory: "2048"
cores: "4"
ostemplate: "local:vztmpl/debian-13-standard.tar.zst"
storage: "local-lvm"
rootfs_size: "16G"
net0: "name=eth0,bridge=vmbr0,ip=10.0.0.100/24,gw=10.0.0.1"
password: "secret"
unprivileged: "true"
start_on_create: "true"
features: "nesting=1"
nameserver: "1.1.1.1 8.8.8.8"
mp0: "local-lvm:50,mp=/mnt/extra"
"#;
        let config: ProvisionConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.provision_type, "proxmox_lxc");
        assert_eq!(config.cluster, "hypervisor1");
        assert_eq!(config.hostname, Some("fulltest".to_string()));
        assert_eq!(config.vmid, Some("100".to_string()));
        assert_eq!(config.memory, Some("2048".to_string()));
        assert_eq!(config.cores, Some("4".to_string()));
        assert_eq!(config.ostemplate, Some("local:vztmpl/debian-13-standard.tar.zst".to_string()));
        assert_eq!(config.storage, Some("local-lvm".to_string()));
        assert_eq!(config.rootfs_size, Some("16G".to_string()));
        assert_eq!(config.net0, Some("name=eth0,bridge=vmbr0,ip=10.0.0.100/24,gw=10.0.0.1".to_string()));
        assert_eq!(config.password, Some("secret".to_string()));
        assert_eq!(config.unprivileged, Some("true".to_string()));
        assert_eq!(config.start_on_create, Some("true".to_string()));
        assert_eq!(config.features, Some("nesting=1".to_string()));
        assert_eq!(config.nameserver, Some("1.1.1.1 8.8.8.8".to_string()));
        assert_eq!(config.extra.len(), 1);
        assert_eq!(config.extra.get("mp0"), Some(&"local-lvm:50,mp=/mnt/extra".to_string()));
    }

    #[test]
    fn test_wait_strategy_from_str() {
        assert_eq!(WaitStrategy::from_str("simple"), WaitStrategy::Simple);
        assert_eq!(WaitStrategy::from_str("Simple"), WaitStrategy::Simple);
        assert_eq!(WaitStrategy::from_str("SIMPLE"), WaitStrategy::Simple);
        assert_eq!(WaitStrategy::from_str("backoff"), WaitStrategy::Backoff);
        assert_eq!(WaitStrategy::from_str("anything_else"), WaitStrategy::Backoff);
    }
}

