// Jetpack
// Copyright (C) Riff Labs Limited <team@riff.cc>
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// at your option) any later version.

//! DNS module - automatic DNS management for provisioned hosts
//!
//! Integrates with OctoDNS for zone file management and provider sync.
//! When hosts are provisioned or destroyed, DNS records are automatically
//! updated in the zone files and synced via OctoDNS to any supported provider.
//!
//! Supports all OctoDNS providers: Gravity, Route53, Cloudflare, PowerDNS, etc.

pub mod gravity;
pub mod zone;

use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{LazyLock, Mutex};

/// Serializes DNS zone mutations across threads.
///
/// Parallel host provisioning calls `add_host_record` / `remove_host_record`
/// concurrently; without this guard the zone-file read-modify-write in
/// `zone.rs` (and the octodns/gravity sync that follows it) would race and lose
/// updates. The record-mutating entry points below hold this lock for their
/// entire body — zone edit plus provider sync — so a host's DNS change is
/// atomic. Internal helpers (`add_ptr_record`, `sync`) deliberately take no
/// lock: they are only reached through these entry points, and
/// `std::sync::Mutex` is not reentrant.
static DNS_WRITE_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

/// Acquire the process-wide DNS write lock, propagating poison as a `String`.
fn dns_write_lock() -> Result<std::sync::MutexGuard<'static, ()>, String> {
    DNS_WRITE_LOCK
        .lock()
        .map_err(|e| format!("DNS write lock poisoned: {}", e))
}

/// Source of truth for IP addresses
#[derive(Debug, Clone, Deserialize, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
pub enum DnsSourceOfTruth {
    /// Zone file is source of truth - IPs read from zone, inventory/Proxmox updated to match
    Dns,
    /// Inventory is source of truth - IPs from host_vars, zone file updated to match
    #[default]
    Inventory,
}

/// DNS configuration
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DnsConfig {
    /// Path to DNS directory containing config.yaml and zones/
    /// Relative to working directory (e.g., "dns/lagun.co")
    pub path: String,

    /// Zone name - optional, inferred from inventory FQDN if not specified
    /// e.g., "gravity01.island.lagun.co" → zone "island.lagun.co"
    pub zone: Option<String>,

    /// Source of truth for IP addresses
    /// - "dns": Zone file is authoritative, inventory/Proxmox updated to match
    /// - "inventory": Inventory is authoritative (default), zone file updated to match
    #[serde(default)]
    pub source_of_truth: DnsSourceOfTruth,

    /// Whether to sync to DNS provider (default: true)
    /// Set to false to only modify zone files without pushing to provider
    #[serde(default = "default_true")]
    pub auto_sync: bool,

    /// DNS aliases - map of alias name to target (group or host)
    /// Creates CNAME records pointing to the target
    /// If target is a group → CNAME points to group service record (which has multiple A records)
    /// If target is a host → CNAME points to host's A record
    #[serde(default)]
    pub aliases: std::collections::HashMap<String, String>,

    /// Reverse DNS zone (e.g., "10.10.10.in-addr.arpa")
    /// If set, PTR records are automatically created for provisioned hosts
    pub reverse_zone: Option<String>,

    /// Native Gravity provider configuration. When present, record mutations
    /// are pushed directly to the Gravity API instead of via OctoDNS sync.
    pub gravity: Option<gravity::GravityConfig>,
}

fn default_true() -> bool {
    true
}

/// Extract zone from inventory FQDN
/// e.g., "gravity01.island.lagun.co" → "island.lagun.co"
pub fn infer_zone(inventory_name: &str) -> Option<String> {
    let parts: Vec<&str> = inventory_name.splitn(2, '.').collect();
    if parts.len() == 2 {
        Some(parts[1].to_string())
    } else {
        None
    }
}

/// Extract short hostname from inventory FQDN
/// e.g., "gravity01.island.lagun.co" → "gravity01"
pub fn extract_hostname(inventory_name: &str) -> String {
    inventory_name
        .split('.')
        .next()
        .unwrap_or(inventory_name)
        .to_string()
}

impl DnsConfig {
    /// Get the zones directory path
    pub fn zones_path(&self) -> PathBuf {
        PathBuf::from(&self.path).join("zones")
    }

    /// Get the OctoDNS config file path
    pub fn config_path(&self) -> PathBuf {
        PathBuf::from(&self.path).join("config.yaml")
    }

    /// Look up IP address for a host from the zone file
    /// inventory_name is the FQDN from inventory (e.g., "gravity01.island.lagun.co")
    pub fn lookup_ip(&self, inventory_name: &str) -> Result<Option<String>, String> {
        let zone = self
            .zone
            .clone()
            .or_else(|| infer_zone(inventory_name))
            .ok_or_else(|| format!("Cannot infer zone from '{}'", inventory_name))?;

        let hostname = extract_hostname(inventory_name);
        zone::get_a_record(&self.zones_path(), &zone, &hostname)
    }

    /// Check if DNS is the source of truth
    pub fn is_dns_authoritative(&self) -> bool {
        self.source_of_truth == DnsSourceOfTruth::Dns
    }

    /// Whether a native Gravity provider is configured for direct record sync.
    pub fn has_native_gravity(&self) -> bool {
        self.gravity.is_some()
    }

    /// Anchor a relative `path` to `automation_root`, making it absolute. Absolute
    /// paths are left untouched. This makes the README's "resolved relative to
    /// the current repository root" promise hold, and it defeats a race where a
    /// process `chdir` mid-batch would otherwise re-target
    /// `current_dir(&config.path)` in [`sync`]. Uses [`Path::join`], never
    /// `canonicalize`, so a not-yet-existing `dns/` tree still resolves.
    pub fn resolve_path_against(&mut self, automation_root: &Path) {
        let candidate = PathBuf::from(&self.path);
        if !candidate.is_absolute() {
            self.path = automation_root.join(&candidate).display().to_string();
        }
    }
}

/// Deserialize a [`DnsConfig`] from host variables and immediately anchor its
/// `path` to `automation_root`. Returns `None` when the `dns:` block is absent or
/// fails to deserialize. Centralizes resolution so every call site in
/// `playbooks::traversal` produces paths anchored to the repo root.
pub fn dns_config_from_vars(
    value: &serde_yaml::Value,
    automation_root: &Path,
) -> Option<DnsConfig> {
    let mut config: DnsConfig = serde_yaml::from_value(value.clone()).ok()?;
    config.resolve_path_against(automation_root);
    Some(config)
}

/// Add a DNS A record for a host, plus PTR if configured, and optionally sync
/// inventory_name is the FQDN from inventory (e.g., "gravity01.island.lagun.co")
pub fn add_host_record(config: &DnsConfig, inventory_name: &str, ip: &str) -> Result<bool, String> {
    let _guard = dns_write_lock()?;
    let zone = config
        .zone
        .clone()
        .or_else(|| infer_zone(inventory_name))
        .ok_or_else(|| {
            format!(
                "Cannot infer zone from '{}' - use FQDN or set zone explicitly",
                inventory_name
            )
        })?;

    let hostname = extract_hostname(inventory_name);
    let fqdn = format!("{}.{}.", hostname, zone);

    // Add A record to zone file
    let a_changed = zone::add_a_record(&config.zones_path(), &zone, &hostname, ip)?;

    // Add PTR record if reverse zone is configured
    let ptr_changed = if config.reverse_zone.is_some() {
        add_ptr_record(config, ip, &fqdn).unwrap_or(false)
    } else {
        false
    };

    let changed = a_changed || ptr_changed;

    // Sync to provider if auto_sync enabled
    if changed && config.auto_sync {
        if config.has_native_gravity() {
            gravity::upsert_record(config, &zone, &hostname, "A", ip)?;
            if ptr_changed && let Some(ref reverse_zone) = config.reverse_zone {
                let host_part = ip
                    .rsplit('.')
                    .next()
                    .ok_or_else(|| format!("Invalid IP: {}", ip))?;
                gravity::upsert_record(config, reverse_zone, host_part, "PTR", &fqdn)?;
            }
        } else {
            sync(config, &zone)?;
            if ptr_changed && let Some(ref reverse_zone) = config.reverse_zone {
                sync(config, reverse_zone)?;
            }
        }
    }

    Ok(changed)
}

/// Remove a DNS A record for a host and optionally sync
/// inventory_name is the FQDN from inventory (e.g., "gravity01.island.lagun.co")
pub fn remove_host_record(config: &DnsConfig, inventory_name: &str) -> Result<bool, String> {
    let _guard = dns_write_lock()?;
    let zone = config
        .zone
        .clone()
        .or_else(|| infer_zone(inventory_name))
        .ok_or_else(|| {
            format!(
                "Cannot infer zone from '{}' - use FQDN or set zone explicitly",
                inventory_name
            )
        })?;

    let hostname = extract_hostname(inventory_name);

    let previous_ip = zone::get_a_record(&config.zones_path(), &zone, &hostname)?;

    // Remove record from zone file
    let changed = zone::remove_a_record(&config.zones_path(), &zone, &hostname)?;

    if changed && config.auto_sync {
        if config.has_native_gravity() {
            gravity::delete_record(config, &zone, &hostname, "A")?;
            if let (Some(ip), Some(reverse_zone)) = (previous_ip, config.reverse_zone.as_ref()) {
                if let Some(host_part) = ip.rsplit('.').next() {
                    gravity::delete_record(config, reverse_zone, host_part, "PTR")?;
                }
            }
        } else {
            sync(config, &zone)?;
        }
    }

    Ok(changed)
}

/// Set service A records for a group (multiple IPs)
pub fn set_service_records(
    config: &DnsConfig,
    zone: &str,
    service_name: &str,
    ips: &[String],
) -> Result<bool, String> {
    let _guard = dns_write_lock()?;
    let changed = zone::set_a_records(&config.zones_path(), zone, service_name, ips)?;

    if changed && config.auto_sync {
        if config.has_native_gravity() {
            gravity::replace_records(
                config,
                zone,
                service_name,
                "A",
                &ips.iter().map(|ip| ip.as_str()).collect::<Vec<_>>(),
            )?;
        } else {
            sync(config, zone)?;
        }
    }

    Ok(changed)
}

/// Add a CNAME alias pointing to a target
pub fn add_cname_alias(
    config: &DnsConfig,
    zone: &str,
    alias: &str,
    target: &str,
) -> Result<bool, String> {
    let _guard = dns_write_lock()?;
    let changed = zone::add_cname_record(&config.zones_path(), zone, alias, target)?;

    if changed && config.auto_sync {
        if config.has_native_gravity() {
            let target_fqdn = if target.ends_with('.') {
                target.to_string()
            } else {
                format!("{}.", target)
            };
            gravity::replace_records(config, zone, alias, "CNAME", &[target_fqdn.as_str()])?;
        } else {
            sync(config, zone)?;
        }
    }

    Ok(changed)
}

/// Add a PTR record for reverse DNS
/// Extracts the host part from the IP and creates record in reverse zone
pub fn add_ptr_record(config: &DnsConfig, ip: &str, fqdn: &str) -> Result<bool, String> {
    let reverse_zone = config
        .reverse_zone
        .as_ref()
        .ok_or_else(|| "No reverse_zone configured".to_string())?;

    // Extract host part from IP (last octet for /24)
    let host_part = ip
        .rsplit('.')
        .next()
        .ok_or_else(|| format!("Invalid IP: {}", ip))?;

    zone::add_ptr_record(&config.zones_path(), reverse_zone, host_part, fqdn)
}

/// Remove a record (A, CNAME, or PTR)
pub fn remove_record(config: &DnsConfig, zone: &str, name: &str) -> Result<bool, String> {
    let _guard = dns_write_lock()?;
    let changed = zone::remove_a_record(&config.zones_path(), zone, name)?;

    if changed && config.auto_sync {
        if config.has_native_gravity() {
            gravity::delete_hostname_records(config, zone, name)?;
        } else {
            sync(config, zone)?;
        }
    }

    Ok(changed)
}

/// Sync zone to providers via OctoDNS
pub fn sync(config: &DnsConfig, zone: &str) -> Result<(), String> {
    let config_path = config.config_path();

    if !config_path.exists() {
        return Err(format!(
            "OctoDNS config not found: {}",
            config_path.display()
        ));
    }

    // Zone name needs trailing dot for OctoDNS
    let zone_with_dot = if zone.ends_with('.') {
        zone.to_string()
    } else {
        format!("{}.", zone)
    };

    let output = Command::new("octodns-sync")
        .arg("--config-file")
        .arg(&config_path)
        .arg("--doit")
        .arg(&zone_with_dot)
        .current_dir(&config.path)
        .output()
        .map_err(|e| format!("Failed to run octodns-sync: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        return Err(format!(
            "octodns-sync failed:\nstdout: {}\nstderr: {}",
            stdout, stderr
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_config(path: &str) -> DnsConfig {
        DnsConfig {
            path: path.to_string(),
            zone: None,
            source_of_truth: DnsSourceOfTruth::default(),
            auto_sync: true,
            aliases: Default::default(),
            reverse_zone: None,
            gravity: None,
        }
    }

    #[test]
    fn resolve_path_joins_relative_path_to_automation_root() {
        let mut config = make_config("dns/riff.cc");
        config.resolve_path_against(std::path::Path::new("/repo"));
        assert_eq!(config.path, "/repo/dns/riff.cc");
    }

    #[test]
    fn resolve_path_leaves_absolute_path_untouched() {
        let mut config = make_config("/srv/dns/riff.cc");
        config.resolve_path_against(std::path::Path::new("/repo"));
        assert_eq!(config.path, "/srv/dns/riff.cc");
    }

    #[test]
    fn resolve_path_with_empty_root_stays_relative() {
        // an empty repo root (no detection) must not corrupt the path
        let mut config = make_config("dns/riff.cc");
        config.resolve_path_against(std::path::Path::new(""));
        assert_eq!(config.path, "dns/riff.cc");
    }

    #[test]
    fn from_vars_anchors_relative_path_to_automation_root() {
        let mut mapping = serde_yaml::Mapping::new();
        mapping.insert(
            serde_yaml::Value::String("path".to_string()),
            serde_yaml::Value::String("dns/riff.cc".to_string()),
        );
        let config = dns_config_from_vars(
            &serde_yaml::Value::Mapping(mapping),
            std::path::Path::new("/repo"),
        )
        .expect("dns block deserializes");
        assert_eq!(config.path, "/repo/dns/riff.cc");
        // downstream paths inherit the anchored root — this is the bug fix
        assert_eq!(
            config.zones_path(),
            PathBuf::from("/repo/dns/riff.cc/zones")
        );
    }

    #[test]
    fn from_vars_keeps_absolute_path_from_host_vars() {
        let mut mapping = serde_yaml::Mapping::new();
        mapping.insert(
            serde_yaml::Value::String("path".to_string()),
            serde_yaml::Value::String("/srv/dns/riff.cc".to_string()),
        );
        let config = dns_config_from_vars(
            &serde_yaml::Value::Mapping(mapping),
            std::path::Path::new("/repo"),
        )
        .expect("dns block deserializes");
        assert_eq!(config.path, "/srv/dns/riff.cc");
    }

    #[test]
    fn from_vars_returns_none_for_non_mapping() {
        let none = dns_config_from_vars(&serde_yaml::Value::Null, std::path::Path::new("/repo"));
        assert!(none.is_none());
    }
}
