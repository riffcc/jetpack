// Jetpack
// Copyright (C) Riff Labs Limited <team@riff.cc>
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

//! Native Gravity DNS provider client.
//!
//! Gravity (https://github.com/beryju/gravity) is the lab's DNS/DHCP server,
//! exposing a REST API at `<api_url>/api/v1/dns/zones/records`. This module
//! talks to it directly — no OctoDNS, no Python, no whole-zone sync — so the
//! DNS layer can upsert and delete individual records as hosts are provisioned
//! and destroyed.
//!
//! The record operations are `async` (HTTP via `reqwest`) and are driven from
//! the engine's synchronous DNS layer through [`crate::runtime`]'s shared
//! runtime, rather than minting a throwaway runtime per call.

use crate::dns::DnsConfig;
use serde::Deserialize;
use std::env;
use std::fs;

/// Native Gravity API configuration. The bearer token is resolved, in priority
/// order, from `api_token`, then `api_token_file`, then `api_token_env` —
/// prefer the file/env forms so the secret never lives in inventory.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GravityConfig {
    pub api_url: String,
    pub api_token: Option<String>,
    pub api_token_file: Option<String>,
    pub api_token_env: Option<String>,
    /// TTL (seconds) applied to records this client creates. Defaults to
    /// [`DEFAULT_TTL`] when unset.
    pub default_ttl: Option<u32>,
}

/// TTL used for created records when `default_ttl` is unset. One hour is a
/// sensible default for infrastructure records: short enough to recover from a
/// renumber without a long stale window, long enough to spare resolvers.
const DEFAULT_TTL: u32 = 3600;

/// Gravity addresses zones by their fully-qualified name with a trailing dot
/// (e.g. `lon.riff.cc.`). Inventory conventionally stores zones without one, so
/// normalize here: a bare `zone=lon.riff.cc` query is rejected with
/// `NOT_FOUND`. Idempotent — an already-qualified zone is returned unchanged.
fn fqdn_zone(zone: &str) -> String {
    if zone.ends_with('.') {
        zone.to_string()
    } else {
        format!("{}.", zone)
    }
}

#[derive(Debug, Deserialize)]
struct RecordsResponse {
    records: Option<Vec<GravityRecord>>,
}

#[derive(Debug, Deserialize)]
struct GravityRecord {
    hostname: String,
    #[serde(rename = "type")]
    record_type: String,
    data: String,
    uid: String,
}

#[derive(Debug, serde::Serialize)]
struct PutRecordInput<'a> {
    data: &'a str,
    #[serde(rename = "type")]
    record_type: &'a str,
    ttl: u32,
}

fn gravity_config(config: &DnsConfig) -> Result<&GravityConfig, String> {
    config
        .gravity
        .as_ref()
        .ok_or_else(|| "Gravity config not present".to_string())
}

/// Resolve the Gravity bearer token. Priority: inline → file → env var → error.
pub(crate) fn resolve_token(gravity: &GravityConfig) -> Result<String, String> {
    if let Some(token) = gravity.api_token.clone() {
        return Ok(token);
    }
    if let Some(path) = gravity.api_token_file.as_ref() {
        return fs::read_to_string(path)
            .map(|s| s.trim().to_string())
            .map_err(|e| format!("Failed to read Gravity API token file '{}': {}", path, e));
    }
    if let Some(env_name) = gravity.api_token_env.as_ref() {
        return env::var(env_name)
            .map_err(|_| format!("Gravity API token env var '{}' is not set", env_name));
    }
    Err("Gravity API token not configured".to_string())
}

fn token_for(config: &DnsConfig) -> Result<String, String> {
    resolve_token(gravity_config(config)?)
}

fn base_url(config: &DnsConfig) -> Result<String, String> {
    Ok(gravity_config(config)?
        .api_url
        .trim_end_matches('/')
        .to_string())
}

async fn client(config: &DnsConfig) -> Result<reqwest::Client, String> {
    let token = token_for(config)?;
    let mut headers = reqwest::header::HeaderMap::new();
    let value = format!("Bearer {}", token)
        .parse()
        .map_err(|e| format!("Invalid Gravity bearer token header: {}", e))?;
    headers.insert(reqwest::header::AUTHORIZATION, value);
    reqwest::Client::builder()
        .default_headers(headers)
        .build()
        .map_err(|e| format!("Failed to build Gravity HTTP client: {}", e))
}

/// Upsert a single-valued record (A, PTR, single-target CNAME, ...).
pub fn upsert_record(
    config: &DnsConfig,
    zone: &str,
    hostname: &str,
    record_type: &str,
    data: &str,
) -> Result<(), String> {
    crate::runtime::block_on(replace_records_async(
        config,
        zone,
        hostname,
        record_type,
        &[data],
    ))
}

/// Sync-blocking facade over [`replace_records_async`].
pub fn replace_records(
    config: &DnsConfig,
    zone: &str,
    hostname: &str,
    record_type: &str,
    values: &[&str],
) -> Result<(), String> {
    crate::runtime::block_on(replace_records_async(
        config,
        zone,
        hostname,
        record_type,
        values,
    ))
}

/// Sync-blocking facade over [`delete_record_async`].
pub fn delete_record(
    config: &DnsConfig,
    zone: &str,
    hostname: &str,
    record_type: &str,
) -> Result<(), String> {
    crate::runtime::block_on(delete_record_async(config, zone, hostname, record_type))
}

/// Sync-blocking facade over [`delete_hostname_records_async`].
pub fn delete_hostname_records(
    config: &DnsConfig,
    zone: &str,
    hostname: &str,
) -> Result<(), String> {
    crate::runtime::block_on(delete_hostname_records_async(config, zone, hostname))
}

async fn list_records(
    config: &DnsConfig,
    zone: &str,
    hostname: Option<&str>,
    record_type: Option<&str>,
) -> Result<Vec<GravityRecord>, String> {
    let client = client(config).await?;
    let base = base_url(config)?;
    let zone = fqdn_zone(zone);
    let mut req = client
        .get(format!("{}/api/v1/dns/zones/records", base))
        .query(&[("zone", zone.as_str())]);

    if let Some(hostname) = hostname {
        req = req.query(&[("hostname", hostname)]);
    }
    if let Some(record_type) = record_type {
        req = req.query(&[("type", record_type)]);
    }

    let response = req
        .send()
        .await
        .map_err(|e| format!("Gravity list-records request failed: {}", e))?;
    let status = response.status();
    if !status.is_success() {
        let body = response
            .text()
            .await
            .unwrap_or_else(|_| "<failed to read body>".to_string());
        return Err(format!(
            "Gravity list-records failed for zone '{}' hostname '{:?}' type '{:?}': {} {}",
            zone, hostname, record_type, status, body
        ));
    }

    let output: RecordsResponse = response
        .json()
        .await
        .map_err(|e| format!("Gravity list-records JSON decode failed: {}", e))?;
    Ok(output.records.unwrap_or_default())
}

/// Make `hostname`/`record_type` resolve to exactly `values` in Gravity. If the
/// existing records already match (order-independent), this is a no-op;
/// otherwise the old records are deleted and the new ones posted.
pub async fn replace_records_async(
    config: &DnsConfig,
    zone: &str,
    hostname: &str,
    record_type: &str,
    values: &[&str],
) -> Result<(), String> {
    let zone = fqdn_zone(zone);
    let ttl = gravity_config(config)?.default_ttl.unwrap_or(DEFAULT_TTL);
    let existing = list_records(config, &zone, Some(hostname), Some(record_type)).await?;
    let mut existing_values: Vec<String> = existing.iter().map(|r| r.data.clone()).collect();
    let mut desired_values: Vec<String> = values.iter().map(|v| v.to_string()).collect();
    existing_values.sort();
    desired_values.sort();

    if existing_values == desired_values {
        return Ok(());
    }

    delete_record_async(config, &zone, hostname, record_type).await?;

    let client = client(config).await?;
    let base = base_url(config)?;
    for value in values {
        let response = client
            .post(format!("{}/api/v1/dns/zones/records", base))
            .query(&[("zone", zone.as_str()), ("hostname", hostname)])
            .json(&PutRecordInput {
                data: value,
                record_type,
                ttl,
            })
            .send()
            .await
            .map_err(|e| format!("Gravity upsert-record request failed: {}", e))?;
        let status = response.status();
        if !status.is_success() {
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "<failed to read body>".to_string());
            return Err(format!(
                "Gravity upsert-record failed for {} {} {} in zone '{}': {} {}",
                hostname, record_type, value, zone, status, body
            ));
        }
    }

    Ok(())
}

/// Delete every record matching `hostname` + `record_type` in Gravity.
pub async fn delete_record_async(
    config: &DnsConfig,
    zone: &str,
    hostname: &str,
    record_type: &str,
) -> Result<(), String> {
    let zone = fqdn_zone(zone);
    let existing = list_records(config, &zone, Some(hostname), Some(record_type)).await?;
    let client = client(config).await?;
    let base = base_url(config)?;

    for record in existing {
        let response = client
            .delete(format!("{}/api/v1/dns/zones/records", base))
            .query(&[
                ("zone", zone.as_str()),
                ("hostname", hostname),
                ("type", record_type),
                ("uid", record.uid.as_str()),
            ])
            .send()
            .await
            .map_err(|e| format!("Gravity delete-record request failed: {}", e))?;
        let status = response.status();
        if !status.is_success() {
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "<failed to read body>".to_string());
            return Err(format!(
                "Gravity delete-record failed for {} {} {} in zone '{}': {} {}",
                record.hostname, record.record_type, record.data, zone, status, body
            ));
        }
    }

    Ok(())
}

/// Delete every record attached to `hostname` (any type) in Gravity.
pub async fn delete_hostname_records_async(
    config: &DnsConfig,
    zone: &str,
    hostname: &str,
) -> Result<(), String> {
    let zone = fqdn_zone(zone);
    let existing = list_records(config, &zone, Some(hostname), None).await?;
    let client = client(config).await?;
    let base = base_url(config)?;

    for record in existing {
        let response = client
            .delete(format!("{}/api/v1/dns/zones/records", base))
            .query(&[
                ("zone", zone.as_str()),
                ("hostname", hostname),
                ("type", record.record_type.as_str()),
                ("uid", record.uid.as_str()),
            ])
            .send()
            .await
            .map_err(|e| format!("Gravity delete-hostname request failed: {}", e))?;
        let status = response.status();
        if !status.is_success() {
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "<failed to read body>".to_string());
            return Err(format!(
                "Gravity delete-hostname failed for {} {} {} in zone '{}': {} {}",
                record.hostname, record.record_type, record.data, zone, status, body
            ));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(token: Option<String>) -> GravityConfig {
        GravityConfig {
            api_url: "http://gravity.example:8008".to_string(),
            api_token: token,
            api_token_file: None,
            api_token_env: None,
            default_ttl: None,
        }
    }

    #[test]
    fn inline_token_wins_over_file_and_env() {
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), "file-secret").unwrap();
        let mut g = cfg(Some("inline-secret".to_string()));
        g.api_token_file = Some(file.path().to_string_lossy().to_string());
        g.api_token_env = Some("GRAVITY_JETPACK_NEVER_SET_X9Q1".to_string());
        assert_eq!(resolve_token(&g).unwrap(), "inline-secret");
    }

    #[test]
    fn token_read_and_trimmed_from_file() {
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), "  file-secret-with-ws  \n").unwrap();
        let mut g = cfg(None);
        g.api_token_file = Some(file.path().to_string_lossy().to_string());
        // file is checked before env, so an unset env var must not matter here
        g.api_token_env = Some("GRAVITY_JETPACK_NEVER_SET_X9Q2".to_string());
        assert_eq!(resolve_token(&g).unwrap(), "file-secret-with-ws");
    }

    #[test]
    fn token_error_when_env_var_unset() {
        let mut g = cfg(None);
        g.api_token_env = Some("GRAVITY_JETPACK_NEVER_SET_X9Q3".to_string());
        assert!(resolve_token(&g).is_err());
    }

    #[test]
    fn token_error_when_no_source_configured() {
        assert!(resolve_token(&cfg(None)).is_err());
    }

    #[test]
    fn gravity_config_parses_minimal() {
        let yaml = "api_url: http://10.7.1.11:8008\napi_token_env: GRAVITY_API_TOKEN\n";
        let g: GravityConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(g.api_url, "http://10.7.1.11:8008");
        assert_eq!(g.api_token_env.as_deref(), Some("GRAVITY_API_TOKEN"));
    }

    #[test]
    fn gravity_config_rejects_unknown_field() {
        let yaml = "api_url: http://x:8008\nbogus: true\n";
        assert!(serde_yaml::from_str::<GravityConfig>(yaml).is_err());
    }
}
