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

pub mod zone;

use serde::Deserialize;
use std::path::PathBuf;
use std::process::Command;

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
    inventory_name.split('.').next().unwrap_or(inventory_name).to_string()
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
        let zone = self.zone.clone()
            .or_else(|| infer_zone(inventory_name))
            .ok_or_else(|| format!("Cannot infer zone from '{}'", inventory_name))?;

        let hostname = extract_hostname(inventory_name);
        zone::get_a_record(&self.zones_path(), &zone, &hostname)
    }

    /// Check if DNS is the source of truth
    pub fn is_dns_authoritative(&self) -> bool {
        self.source_of_truth == DnsSourceOfTruth::Dns
    }
}

/// Add a DNS A record for a host, plus PTR if configured, and optionally sync
/// inventory_name is the FQDN from inventory (e.g., "gravity01.island.lagun.co")
pub fn add_host_record(
    config: &DnsConfig,
    inventory_name: &str,
    ip: &str,
) -> Result<bool, String> {
    let zone = config.zone.clone()
        .or_else(|| infer_zone(inventory_name))
        .ok_or_else(|| format!("Cannot infer zone from '{}' - use FQDN or set zone explicitly", inventory_name))?;

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
        sync(config, &zone)?;
        if ptr_changed {
            if let Some(ref reverse_zone) = config.reverse_zone {
                sync(config, reverse_zone)?;
            }
        }
    }

    Ok(changed)
}

/// Remove a DNS A record for a host and optionally sync
/// inventory_name is the FQDN from inventory (e.g., "gravity01.island.lagun.co")
pub fn remove_host_record(
    config: &DnsConfig,
    inventory_name: &str,
) -> Result<bool, String> {
    let zone = config.zone.clone()
        .or_else(|| infer_zone(inventory_name))
        .ok_or_else(|| format!("Cannot infer zone from '{}' - use FQDN or set zone explicitly", inventory_name))?;

    let hostname = extract_hostname(inventory_name);

    // Remove record from zone file
    let changed = zone::remove_a_record(&config.zones_path(), &zone, &hostname)?;

    if changed && config.auto_sync {
        sync(config, &zone)?;
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
    zone::set_a_records(&config.zones_path(), zone, service_name, ips)
}

/// Add a CNAME alias pointing to a target
pub fn add_cname_alias(
    config: &DnsConfig,
    zone: &str,
    alias: &str,
    target: &str,
) -> Result<bool, String> {
    zone::add_cname_record(&config.zones_path(), zone, alias, target)
}

/// Add a PTR record for reverse DNS
/// Extracts the host part from the IP and creates record in reverse zone
pub fn add_ptr_record(
    config: &DnsConfig,
    ip: &str,
    fqdn: &str,
) -> Result<bool, String> {
    let reverse_zone = config.reverse_zone.as_ref()
        .ok_or_else(|| "No reverse_zone configured".to_string())?;

    // Extract host part from IP (last octet for /24)
    let host_part = ip.rsplit('.').next()
        .ok_or_else(|| format!("Invalid IP: {}", ip))?;

    zone::add_ptr_record(&config.zones_path(), reverse_zone, host_part, fqdn)
}

/// Remove a record (A, CNAME, or PTR)
pub fn remove_record(
    config: &DnsConfig,
    zone: &str,
    name: &str,
) -> Result<bool, String> {
    zone::remove_a_record(&config.zones_path(), zone, name)
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
