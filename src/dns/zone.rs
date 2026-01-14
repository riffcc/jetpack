// Jetpack
// Copyright (C) Riff Labs Limited <team@riff.cc>
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// at your option) any later version.

//! OctoDNS zone file management
//!
//! Reads and writes zone files in OctoDNS YAML format.
//!
//! Example zone file (island.lagun.co.yaml):
//! ```yaml
//! ---
//! gravity01:
//!   type: A
//!   value: 10.10.10.2
//!
//! gravity02:
//!   type: A
//!   value: 10.10.10.3
//! ```

use serde::{Deserialize, Serialize};
use serde_yaml::{Mapping, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

/// A DNS record in OctoDNS format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DnsRecord {
    #[serde(rename = "type")]
    pub record_type: String,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub values: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub ttl: Option<u32>,
}

/// Read a zone file and return the records as a map
fn read_zone_file(zones_path: &Path, zone: &str) -> Result<BTreeMap<String, Value>, String> {
    let zone_file = zones_path.join(format!("{}.yaml", zone));

    if !zone_file.exists() {
        // Return empty map if zone file doesn't exist yet
        return Ok(BTreeMap::new());
    }

    let content = fs::read_to_string(&zone_file)
        .map_err(|e| format!("Failed to read zone file {}: {}", zone_file.display(), e))?;

    // Handle empty files
    if content.trim().is_empty() || content.trim() == "---" {
        return Ok(BTreeMap::new());
    }

    let records: BTreeMap<String, Value> = serde_yaml::from_str(&content)
        .map_err(|e| format!("Failed to parse zone file {}: {}", zone_file.display(), e))?;

    Ok(records)
}

/// Write records to a zone file
fn write_zone_file(zones_path: &Path, zone: &str, records: &BTreeMap<String, Value>) -> Result<(), String> {
    let zone_file = zones_path.join(format!("{}.yaml", zone));

    // Ensure zones directory exists
    fs::create_dir_all(zones_path)
        .map_err(|e| format!("Failed to create zones directory: {}", e))?;

    let content = serde_yaml::to_string(&records)
        .map_err(|e| format!("Failed to serialize zone file: {}", e))?;

    // Add YAML document separator
    let content = format!("---\n{}", content);

    fs::write(&zone_file, content)
        .map_err(|e| format!("Failed to write zone file {}: {}", zone_file.display(), e))?;

    Ok(())
}

/// Get an A record's IP address from the zone file
pub fn get_a_record(zones_path: &Path, zone: &str, hostname: &str) -> Result<Option<String>, String> {
    let records = read_zone_file(zones_path, zone)?;

    if let Some(record_value) = records.get(hostname) {
        // Check if it's an A record
        if let Some(record_type) = record_value.get("type") {
            if record_type.as_str() == Some("A") {
                if let Some(value) = record_value.get("value") {
                    return Ok(value.as_str().map(|s| s.to_string()));
                }
            }
        }
    }

    Ok(None)
}

/// Add an A record to the zone file
/// Returns true if the record was added or updated, false if unchanged
/// If the hostname already has A records, the IP is added to the values list
pub fn add_a_record(zones_path: &Path, zone: &str, hostname: &str, ip: &str) -> Result<bool, String> {
    let mut records = read_zone_file(zones_path, zone)?;

    // Check if record already exists
    if let Some(existing) = records.get(hostname) {
        if let Some(record_type) = existing.get("type") {
            if record_type.as_str() == Some("A") {
                // Check single value
                if let Some(value) = existing.get("value") {
                    if value.as_str() == Some(ip) {
                        return Ok(false); // Already exists
                    }
                }
                // Check values array
                if let Some(values) = existing.get("values") {
                    if let Some(arr) = values.as_sequence() {
                        for v in arr {
                            if v.as_str() == Some(ip) {
                                return Ok(false); // Already in values
                            }
                        }
                    }
                }
            }
        }
    }

    // Create or update the A record
    let mut record = Mapping::new();
    record.insert(Value::String("type".to_string()), Value::String("A".to_string()));
    record.insert(Value::String("value".to_string()), Value::String(ip.to_string()));

    records.insert(hostname.to_string(), Value::Mapping(record));

    write_zone_file(zones_path, zone, &records)?;

    Ok(true)
}

/// Set multiple A records for a hostname (used for group service records)
/// Replaces any existing record with a multi-value A record
pub fn set_a_records(zones_path: &Path, zone: &str, hostname: &str, ips: &[String]) -> Result<bool, String> {
    let mut records = read_zone_file(zones_path, zone)?;

    // Check if already matches
    if let Some(existing) = records.get(hostname) {
        if let Some(record_type) = existing.get("type") {
            if record_type.as_str() == Some("A") {
                if let Some(values) = existing.get("values") {
                    if let Some(arr) = values.as_sequence() {
                        let existing_ips: Vec<&str> = arr.iter()
                            .filter_map(|v| v.as_str())
                            .collect();
                        if existing_ips.len() == ips.len() &&
                           ips.iter().all(|ip| existing_ips.contains(&ip.as_str())) {
                            return Ok(false); // Already matches
                        }
                    }
                }
                // Check single value case
                if ips.len() == 1 {
                    if let Some(value) = existing.get("value") {
                        if value.as_str() == Some(&ips[0]) {
                            return Ok(false);
                        }
                    }
                }
            }
        }
    }

    // Create the A record with multiple values
    let mut record = Mapping::new();
    record.insert(Value::String("type".to_string()), Value::String("A".to_string()));

    if ips.len() == 1 {
        record.insert(Value::String("value".to_string()), Value::String(ips[0].clone()));
    } else {
        let values: Vec<Value> = ips.iter()
            .map(|ip| Value::String(ip.clone()))
            .collect();
        record.insert(Value::String("values".to_string()), Value::Sequence(values));
    }

    records.insert(hostname.to_string(), Value::Mapping(record));

    write_zone_file(zones_path, zone, &records)?;

    Ok(true)
}

/// Add a CNAME record
/// Returns true if the record was added or updated, false if unchanged
pub fn add_cname_record(zones_path: &Path, zone: &str, alias: &str, target: &str) -> Result<bool, String> {
    let mut records = read_zone_file(zones_path, zone)?;

    // Ensure target has trailing dot (FQDN)
    let target_fqdn = if target.ends_with('.') {
        target.to_string()
    } else {
        format!("{}.", target)
    };

    // Check if record already exists with same value
    if let Some(existing) = records.get(alias) {
        if let Some(record_type) = existing.get("type") {
            if record_type.as_str() == Some("CNAME") {
                if let Some(value) = existing.get("value") {
                    if value.as_str() == Some(&target_fqdn) {
                        return Ok(false); // Already exists
                    }
                }
            }
        }
    }

    // Create the CNAME record
    let mut record = Mapping::new();
    record.insert(Value::String("type".to_string()), Value::String("CNAME".to_string()));
    record.insert(Value::String("value".to_string()), Value::String(target_fqdn));

    records.insert(alias.to_string(), Value::Mapping(record));

    write_zone_file(zones_path, zone, &records)?;

    Ok(true)
}

/// Add a PTR record for reverse DNS
/// ip should be just the host part (e.g., "2" for 10.10.10.2)
/// target should be the FQDN
pub fn add_ptr_record(zones_path: &Path, zone: &str, ip_host: &str, target: &str) -> Result<bool, String> {
    let mut records = read_zone_file(zones_path, zone)?;

    // Ensure target has trailing dot (FQDN)
    let target_fqdn = if target.ends_with('.') {
        target.to_string()
    } else {
        format!("{}.", target)
    };

    // Check if record already exists with same value
    if let Some(existing) = records.get(ip_host) {
        if let Some(record_type) = existing.get("type") {
            if record_type.as_str() == Some("PTR") {
                if let Some(value) = existing.get("value") {
                    if value.as_str() == Some(&target_fqdn) {
                        return Ok(false); // Already exists
                    }
                }
            }
        }
    }

    // Create the PTR record
    let mut record = Mapping::new();
    record.insert(Value::String("type".to_string()), Value::String("PTR".to_string()));
    record.insert(Value::String("value".to_string()), Value::String(target_fqdn));

    records.insert(ip_host.to_string(), Value::Mapping(record));

    write_zone_file(zones_path, zone, &records)?;

    Ok(true)
}

/// Remove an A record from the zone file
/// Returns true if the record was removed, false if it didn't exist
pub fn remove_a_record(zones_path: &Path, zone: &str, hostname: &str) -> Result<bool, String> {
    let mut records = read_zone_file(zones_path, zone)?;

    if records.remove(hostname).is_some() {
        write_zone_file(zones_path, zone, &records)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_zone_file_alphabetical_order() {
        // OctoDNS requires zone file keys to be in alphabetical order
        let dir = tempdir().unwrap();
        let zones_path = dir.path();

        // Add records in non-alphabetical order
        add_a_record(zones_path, "example.com", "zebra", "10.0.0.3").unwrap();
        add_a_record(zones_path, "example.com", "alpha", "10.0.0.1").unwrap();
        add_a_record(zones_path, "example.com", "middle", "10.0.0.2").unwrap();

        // Read the file and verify keys are alphabetically sorted
        let content = fs::read_to_string(zones_path.join("example.com.yaml")).unwrap();
        let lines: Vec<&str> = content.lines().collect();

        // Find the key lines (those that start without whitespace and contain a colon)
        let keys: Vec<&str> = lines.iter()
            .filter(|l| !l.starts_with(' ') && !l.starts_with('-') && l.contains(':'))
            .map(|l| l.trim_end_matches(':'))
            .collect();

        assert_eq!(keys, vec!["alpha", "middle", "zebra"],
            "Zone file keys must be in alphabetical order for OctoDNS compatibility");
    }

    #[test]
    fn test_add_and_get_a_record() {
        let dir = tempdir().unwrap();
        let zones_path = dir.path();

        // Add a record
        let changed = add_a_record(zones_path, "example.com", "host1", "10.0.0.1").unwrap();
        assert!(changed);

        // Get the record
        let ip = get_a_record(zones_path, "example.com", "host1").unwrap();
        assert_eq!(ip, Some("10.0.0.1".to_string()));

        // Adding same record again should return false
        let changed = add_a_record(zones_path, "example.com", "host1", "10.0.0.1").unwrap();
        assert!(!changed);

        // Updating record should return true
        let changed = add_a_record(zones_path, "example.com", "host1", "10.0.0.2").unwrap();
        assert!(changed);
    }

    #[test]
    fn test_remove_a_record() {
        let dir = tempdir().unwrap();
        let zones_path = dir.path();

        // Add a record
        add_a_record(zones_path, "example.com", "host1", "10.0.0.1").unwrap();

        // Remove it
        let removed = remove_a_record(zones_path, "example.com", "host1").unwrap();
        assert!(removed);

        // Should not exist anymore
        let ip = get_a_record(zones_path, "example.com", "host1").unwrap();
        assert_eq!(ip, None);

        // Removing again should return false
        let removed = remove_a_record(zones_path, "example.com", "host1").unwrap();
        assert!(!removed);
    }
}
