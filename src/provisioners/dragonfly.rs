// Jetpack - Dragonfly REST API client
// Copyright (C) Riff Labs Limited <team@riff.cc>
//
// Thin client for the Dragonfly bare-metal/VM provisioning server. Used by the
// proxmox_vm provisioner to pre-register PXE-booted VMs (so Dragonfly images
// them on boot) and to resolve the DHCP-assigned IP for a MAC.
//
// All HTTP runs on a current-thread tokio runtime via block_on, mirroring the
// proxmox provisioners. Auth is a bearer token; the API lives under /api.

use serde::{Deserialize, Serialize};
use std::time::Duration;

const TIMEOUT_SECS: u64 = 15;

pub struct DragonflyClient {
    base_url: String,
    token: String,
}

/// Proxmox VM/LXC source info for Dragonfly registration. When included,
/// Dragonfly sets MachineSource::Proxmox so it can reboot-pxe the VM on
/// reimage (vs Manual, which can't trigger a PXE reboot).
#[derive(Clone, Debug)]
pub struct ProxmoxSource {
    pub proxmox_type: String, // "vm" | "lxc" | "node"
    pub cluster: String,
    pub node: String,
    pub vmid: u64,
}

/// Body for `POST /api/machines/admin/create`. Mirrors Dragonfly's
/// `AdminCreateMachineRequest`; only the fields Jetpack sends are modelled.
/// `ip_address` is optional: present → Dragonfly sets `StaticIpv4` and bakes
/// the static config via cloud-init; absent → the machine is left in `Dhcp`.
#[derive(Serialize)]
struct AdminCreateMachineRequest<'a> {
    mac_address: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    ip_address: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    hostname: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    prefix_len: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    gateway: Option<&'a str>,
    nameservers: &'a [String],
    #[serde(skip_serializing_if = "Option::is_none")]
    proxmox_vmid: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    proxmox_node: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    proxmox_cluster: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    proxmox_type: Option<&'a str>,
}

#[derive(Deserialize, Debug, PartialEq)]
pub struct AdminCreateMachineResponse {
    pub machine_id: String,
    pub created: bool,
}

/// Static network details sent in the admin/create body so cloud-init writes a
/// complete config. Read from inventory group_vars (`node_network_*`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NetworkSpec {
    pub prefix_len: Option<u8>,
    pub gateway: Option<String>,
    pub nameservers: Vec<String>,
}

impl NetworkSpec {
    /// Build from blended inventory vars. Missing keys degrade gracefully
    /// (None / empty) — these only matter for the static path; the server
    /// ignores them when no IP is declared.
    pub fn from_vars(vars: &serde_yaml::Mapping) -> Self {
        Self {
            prefix_len: var(vars, "node_network_prefix_len").and_then(|s| s.parse().ok()),
            gateway: var(vars, "node_network_gateway"),
            nameservers: var_list(vars, "node_network_nameservers"),
        }
    }
}

/// A Dragonfly machine's identity + imaging state, as returned by the list API.
#[derive(Deserialize, Debug, PartialEq)]
pub struct MachineRef {
    pub id: String,
    pub status: String,
}

impl MachineRef {
    /// Terminal success state: the OS has been imaged/installed. Anything else
    /// means the machine still needs imaging (or re-imaging).
    pub fn is_installed(&self) -> bool {
        self.status == "Installed"
    }
}

#[derive(Serialize)]
struct OsAssignmentRequest<'a> {
    os_choice: &'a str,
}

#[derive(Deserialize)]
struct LeaseInfo {
    mac: String,
    ip: String,
}

struct HttpResponse {
    #[allow(dead_code)]
    status: u16,
    body: String,
}

/// Read a string-typed inventory var. Returns None when unset (opts the caller
/// out of Dragonfly integration).
pub fn var(vars: &serde_yaml::Mapping, key: &str) -> Option<String> {
    let value = vars.get(serde_yaml::Value::String(key.to_string()))?;
    match value {
        serde_yaml::Value::String(s) => Some(s.clone()),
        other => serde_yaml::to_string(other)
            .ok()
            .map(|s| s.trim().trim_matches('"').to_string()),
    }
}

/// Read a list-of-strings inventory var (a YAML sequence). Empty when unset or
/// not a sequence. Used for `node_network_nameservers`.
fn var_list(vars: &serde_yaml::Mapping, key: &str) -> Vec<String> {
    match vars.get(serde_yaml::Value::String(key.to_string())) {
        Some(serde_yaml::Value::Sequence(items)) => items
            .iter()
            .map(|v| match v {
                serde_yaml::Value::String(s) => s.clone(),
                other => serde_yaml::to_string(other)
                    .ok()
                    .map(|s| s.trim().trim_matches('"').to_string())
                    .unwrap_or_default(),
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Normalize a MAC for comparison: lowercase, colon-separated. Dragonfly stores
/// MACs as lowercase-with-colons.
fn normalize_mac(mac: &str) -> String {
    mac.to_lowercase().replace('-', ":")
}

impl DragonflyClient {
    /// Build a client from blended inventory vars. Returns None unless both
    /// dragonfly_api_url and dragonfly_api_token are set, so the integration is
    /// strictly opt-in.
    pub fn try_from_vars(vars: &serde_yaml::Mapping) -> Option<Self> {
        Some(Self {
            base_url: var(vars, "dragonfly_api_url")?,
            token: var(vars, "dragonfly_api_token")?,
        })
    }

    /// Pre-create (or update) a machine via the admin endpoint. Upsert by MAC:
    /// Dragonfly returns `created: true` for a new machine, `false` for an
    /// update. When `ip` is `Some` the machine is set to `StaticIpv4` with the
    /// full network config; when `None` it's left in `Dhcp` (same outcome the
    /// old open-registration path gave). Re-running is safe — it just
    /// re-applies the config.
    pub fn admin_create_machine(
        &self,
        mac: &str,
        ip: Option<&str>,
        hostname: Option<&str>,
        network: &NetworkSpec,
        proxmox: Option<&ProxmoxSource>,
    ) -> Result<AdminCreateMachineResponse, String> {
        let body = AdminCreateMachineRequest {
            mac_address: mac,
            ip_address: ip,
            hostname,
            prefix_len: network.prefix_len,
            gateway: network.gateway.as_deref(),
            nameservers: &network.nameservers,
            proxmox_type: proxmox.map(|p| p.proxmox_type.as_str()),
            proxmox_cluster: proxmox.map(|p| p.cluster.as_str()),
            proxmox_node: proxmox.map(|p| p.node.as_str()),
            proxmox_vmid: proxmox.map(|p| p.vmid as u32),
        };
        let resp = self.post("/machines/admin/create", &body)?;
        serde_json::from_str::<AdminCreateMachineResponse>(&resp.body).map_err(|e| {
            format!(
                "dragonfly admin/create: parse response: {} (body: {})",
                e, resp.body
            )
        })
    }

    /// Assign an OS template to a machine so Dragonfly images it on PXE boot.
    pub fn assign_os(&self, machine_id: &str, os_choice: &str) -> Result<(), String> {
        let body = OsAssignmentRequest { os_choice };
        let _ = self.post(&format!("/machines/{}/os", machine_id), &body)?;
        Ok(())
    }

    /// Trigger a reimage workflow for a machine. `assign_os` selects the OS
    /// template; this kicks off the actual imaging (POST /machines/{id}/reimage).
    pub fn reimage(&self, machine_id: &str) -> Result<(), String> {
        let _ = self.post(
            &format!("/machines/{}/reimage", machine_id),
            &serde_json::Value::Null,
        )?;
        Ok(())
    }

    /// Set a machine's hostname (PUT /machines/{id}/hostname). The hostname from
    /// admin/create doesn't always stick — the PXE agent may report
    /// "localhost" on first check-in. This sets it explicitly.
    pub fn set_hostname(&self, machine_id: &str, hostname: &str) -> Result<(), String> {
        let _ = self.put(
            &format!("/machines/{}/hostname", machine_id),
            &serde_json::json!({ "hostname": hostname }),
        )?;
        Ok(())
    }

    /// Resolve the current DHCP-assigned IPv4 for a MAC, if a lease exists.
    pub fn lookup_ip(&self, mac: &str) -> Result<Option<String>, String> {
        let resp = self.get("/dhcp/leases")?;
        let leases: Vec<LeaseInfo> = serde_json::from_str(&resp.body)
            .map_err(|e| format!("dragonfly leases: parse: {} (body: {})", e, resp.body))?;
        let target = normalize_mac(mac);
        Ok(leases
            .into_iter()
            .find(|lease| normalize_mac(&lease.mac) == target)
            .map(|lease| lease.ip))
    }

    /// Look up a machine by MAC (read-only). Returns its id + imaging status, or
    /// None if Dragonfly has no machine with that MAC. Used to decide whether a VM
    /// still needs imaging (status != "Installed") or is already done — without
    /// re-registering (which would mutate the machine).
    pub fn find_machine_by_mac(&self, mac: &str) -> Result<Option<MachineRef>, String> {
        let resp = self.get("/machines")?;
        #[derive(Deserialize)]
        struct List {
            data: Vec<Entry>,
        }
        #[derive(Deserialize)]
        struct Entry {
            id: String,
            mac_address: String,
            status: String,
        }
        let list: List = serde_json::from_str(&resp.body)
            .map_err(|e| format!("dragonfly machines: parse: {} (body: {})", e, resp.body))?;
        let target = normalize_mac(mac);
        Ok(list
            .data
            .into_iter()
            .find(|e| normalize_mac(&e.mac_address) == target)
            .map(|e| MachineRef {
                id: e.id,
                status: e.status,
            }))
    }

    fn get(&self, path: &str) -> Result<HttpResponse, String> {
        self.request(reqwest::Method::GET, path, None)
    }

    fn post<T: Serialize>(&self, path: &str, body: &T) -> Result<HttpResponse, String> {
        let json =
            serde_json::to_string(body).map_err(|e| format!("dragonfly: encode body: {}", e))?;
        self.request(reqwest::Method::POST, path, Some(json))
    }

    fn put<T: Serialize>(&self, path: &str, body: &T) -> Result<HttpResponse, String> {
        let json =
            serde_json::to_string(body).map_err(|e| format!("dragonfly: encode body: {}", e))?;
        self.request(reqwest::Method::PUT, path, Some(json))
    }

    fn request(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<String>,
    ) -> Result<HttpResponse, String> {
        let url = format!("{}/api{}", self.base_url.trim_end_matches('/'), path);
        let token = self.token.clone();
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| format!("dragonfly: tokio runtime: {}", e))?;
        rt.block_on(async move {
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(TIMEOUT_SECS))
                .build()
                .map_err(|e| format!("dragonfly: http client: {}", e))?;
            let mut req = client.request(method.clone(), &url).bearer_auth(&token);
            if let Some(b) = body {
                req = req
                    .header(reqwest::header::CONTENT_TYPE, "application/json")
                    .body(b);
            }
            let resp = req
                .send()
                .await
                .map_err(|e| format!("dragonfly: {} {}: {}", method, url, e))?;
            let status = resp.status();
            let text = resp
                .text()
                .await
                .map_err(|e| format!("dragonfly: read body: {}", e))?;
            if !status.is_success() {
                return Err(format!(
                    "dragonfly: {} {} returned {}: {}",
                    method, url, status, text
                ));
            }
            Ok(HttpResponse {
                status: status.as_u16(),
                body: text,
            })
        })
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};
    use std::thread;

    /// A recorded incoming request, for assertions.
    #[derive(Clone, Debug)]
    pub(crate) struct RecordedReq {
        pub(crate) method: String,
        pub(crate) path: String,
        pub(crate) auth: Option<String>,
        pub(crate) body: String,
    }

    /// Minimal HTTP/1.1 mock: responds per a closure, records every request.
    /// Runs in a plain OS thread (no tokio) so it cannot clash with the
    /// client's current-thread runtime. The listener thread detaches when the
    /// test ends.
    pub(crate) struct MockServer {
        addr: String,
        requests: Arc<Mutex<Vec<RecordedReq>>>,
    }

    impl MockServer {
        pub(crate) fn start<F>(responder: F) -> Self
        where
            F: Fn(&RecordedReq) -> (u16, String) + Send + 'static,
        {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let addr = listener.local_addr().unwrap().to_string();
            listener.set_nonblocking(false).ok();
            let requests: Arc<Mutex<Vec<RecordedReq>>> = Arc::new(Mutex::new(Vec::new()));
            let recorded = Arc::clone(&requests);
            thread::spawn(move || {
                for stream in listener.incoming() {
                    let mut stream = match stream {
                        Ok(s) => s,
                        Err(_) => continue,
                    };
                    let req = read_request(&mut stream);
                    {
                        recorded.lock().unwrap().push(req.clone());
                    }
                    let (status, body) = responder(&req);
                    let resp = format!(
                        "HTTP/1.1 {} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        status,
                        body.len(),
                        body
                    );
                    let _ = stream.write_all(resp.as_bytes());
                    let _ = stream.flush();
                }
            });
            MockServer { addr, requests }
        }

        pub(crate) fn recorded(&self) -> Vec<RecordedReq> {
            self.requests.lock().unwrap().clone()
        }

        pub(crate) fn url(&self) -> String {
            format!("http://{}", self.addr)
        }
    }

    fn read_request(stream: &mut std::net::TcpStream) -> RecordedReq {
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut request_line = String::new();
        reader.read_line(&mut request_line).unwrap();
        let mut parts = request_line.split_whitespace();
        let method = parts.next().unwrap_or("").to_string();
        let path = parts.next().unwrap_or("").to_string();

        let mut content_length = 0usize;
        let mut auth: Option<String> = None;
        loop {
            let mut header = String::new();
            if reader.read_line(&mut header).unwrap() == 0 {
                break;
            }
            let trimmed = header.trim_end_matches(|c| c == '\r' || c == '\n');
            if trimmed.is_empty() {
                break;
            }
            let lower = trimmed.to_ascii_lowercase();
            if let Some(v) = lower.strip_prefix("authorization: ") {
                auth = Some(v.to_string());
            }
            if let Some(v) = lower.strip_prefix("content-length: ") {
                content_length = v.parse().unwrap_or(0);
            }
        }
        let mut body = String::new();
        if content_length > 0 {
            reader
                .by_ref()
                .take(content_length as u64)
                .read_to_string(&mut body)
                .unwrap();
        }
        RecordedReq {
            method,
            path,
            auth,
            body,
        }
    }

    pub(crate) fn client(addr: &str) -> DragonflyClient {
        DragonflyClient {
            base_url: addr.to_string(),
            token: "df_test_token".to_string(),
        }
    }

    #[test]
    fn try_from_vars_is_opt_in() {
        let mut vars = serde_yaml::Mapping::new();
        assert!(DragonflyClient::try_from_vars(&vars).is_none());
        vars.insert(
            serde_yaml::Value::String("dragonfly_api_url".into()),
            serde_yaml::Value::String("http://dragonfly".into()),
        );
        assert!(DragonflyClient::try_from_vars(&vars).is_none()); // still no token
        vars.insert(
            serde_yaml::Value::String("dragonfly_api_token".into()),
            serde_yaml::Value::String("df_x".into()),
        );
        assert!(DragonflyClient::try_from_vars(&vars).is_some());
    }

    #[test]
    fn admin_create_machine_posts_full_static_body() {
        let server = MockServer::start(|_| {
            (
                201,
                r#"{"machine_id":"0192abcd","created":true}"#.to_string(),
            )
        });
        let c = client(&server.url());
        let network = NetworkSpec {
            prefix_len: Some(24),
            gateway: Some("10.7.1.1".to_string()),
            nameservers: vec!["10.7.1.11".to_string(), "10.7.1.12".to_string()],
        };
        let proxmox = ProxmoxSource {
            proxmox_type: "vm".to_string(),
            cluster: "SpaceTempAgency".to_string(),
            node: "bee".to_string(),
            vmid: 101,
        };
        let resp = c
            .admin_create_machine(
                "BC:24:11:22:33:44",
                Some("10.7.1.241"),
                Some("k8s01"),
                &network,
                Some(&proxmox),
            )
            .unwrap();
        assert_eq!(resp.machine_id, "0192abcd");
        assert!(resp.created);

        let recorded = server.recorded();
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].method, "POST");
        assert_eq!(recorded[0].path, "/api/machines/admin/create");
        assert_eq!(recorded[0].auth.as_deref(), Some("bearer df_test_token"));
        let body: serde_json::Value = serde_json::from_str(&recorded[0].body).unwrap();
        assert_eq!(body["mac_address"], "BC:24:11:22:33:44");
        assert_eq!(body["ip_address"], "10.7.1.241");
        assert_eq!(body["hostname"], "k8s01");
        assert_eq!(body["prefix_len"], 24);
        assert_eq!(body["gateway"], "10.7.1.1");
        assert_eq!(
            body["nameservers"],
            serde_json::json!(["10.7.1.11", "10.7.1.12"])
        );
        assert_eq!(body["proxmox_type"], "vm");
        assert_eq!(body["proxmox_cluster"], "SpaceTempAgency");
        assert_eq!(body["proxmox_node"], "bee");
        assert_eq!(body["proxmox_vmid"], 101);
    }

    #[test]
    fn admin_create_machine_omits_ip_for_dhcp() {
        let server = MockServer::start(|_| {
            (
                200,
                r#"{"machine_id":"0192abcd","created":false}"#.to_string(),
            )
        });
        let c = client(&server.url());
        // No static IP, no network details known → DHCP precreate.
        let resp = c
            .admin_create_machine(
                "BC:24:11:22:33:44",
                None,
                Some("k8s01"),
                &NetworkSpec::default(),
                None,
            )
            .unwrap();
        assert!(!resp.created);

        let recorded = server.recorded();
        assert_eq!(recorded[0].path, "/api/machines/admin/create");
        let body: serde_json::Value = serde_json::from_str(&recorded[0].body).unwrap();
        // No static IP → ip_address / prefix_len / gateway omitted (server uses Dhcp).
        assert!(body.get("ip_address").is_none());
        assert!(body.get("prefix_len").is_none());
        assert!(body.get("gateway").is_none());
        assert_eq!(body["mac_address"], "BC:24:11:22:33:44");
        assert_eq!(body["hostname"], "k8s01");
    }

    #[test]
    fn network_spec_from_vars_reads_node_network_group_vars() {
        let vars: serde_yaml::Mapping = serde_yaml::from_str(
            "node_network_prefix_len: 24\n\
             node_network_gateway: 10.7.1.1\n\
             node_network_nameservers:\n\
             - 10.7.1.11\n\
             - 10.7.1.12\n",
        )
        .unwrap();
        let spec = NetworkSpec::from_vars(&vars);
        assert_eq!(spec.prefix_len, Some(24));
        assert_eq!(spec.gateway.as_deref(), Some("10.7.1.1"));
        assert_eq!(
            spec.nameservers,
            vec!["10.7.1.11".to_string(), "10.7.1.12".to_string()]
        );
    }

    #[test]
    fn network_spec_from_vars_absent_is_empty() {
        let spec = NetworkSpec::from_vars(&serde_yaml::Mapping::new());
        assert_eq!(spec, NetworkSpec::default());
    }

    #[test]
    fn assign_os_posts_to_machine_os_path() {
        let server = MockServer::start(|_| (200, "{}".to_string()));
        let c = client(&server.url());
        c.assign_os("0192abcd", "debian-13").unwrap();
        let recorded = server.recorded();
        assert_eq!(recorded[0].path, "/api/machines/0192abcd/os");
        let body: serde_json::Value = serde_json::from_str(&recorded[0].body).unwrap();
        assert_eq!(body["os_choice"], "debian-13");
    }

    #[test]
    fn reimage_posts_to_machine_reimage_path() {
        let server = MockServer::start(|_| (200, "{}".to_string()));
        let c = client(&server.url());
        c.reimage("0192abcd").unwrap();
        let recorded = server.recorded();
        assert_eq!(recorded[0].method, "POST");
        assert_eq!(recorded[0].path, "/api/machines/0192abcd/reimage");
    }

    #[test]
    fn set_hostname_puts_to_machine_hostname_path() {
        let server = MockServer::start(|_| (200, "{}".to_string()));
        let c = client(&server.url());
        c.set_hostname("0192abcd", "k8s01").unwrap();
        let recorded = server.recorded();
        assert_eq!(recorded[0].method, "PUT");
        assert_eq!(recorded[0].path, "/api/machines/0192abcd/hostname");
        let body: serde_json::Value = serde_json::from_str(&recorded[0].body).unwrap();
        assert_eq!(body["hostname"], "k8s01");
    }

    #[test]
    fn lookup_ip_matches_mac_case_insensitively() {
        let server = MockServer::start(|_| {
            (
                200,
                r#"[{"mac":"bc:24:11:22:33:44","ip":"10.7.1.99","remaining_secs":600},{"mac":"aa:bb:cc:dd:ee:ff","ip":"10.7.1.50","remaining_secs":600}]"#
                    .to_string(),
            )
        });
        let c = client(&server.url());
        // uppercase requesting MAC must match the lowercase lease
        let ip = c.lookup_ip("BC:24:11:22:33:44").unwrap();
        assert_eq!(ip.as_deref(), Some("10.7.1.99"));
    }

    #[test]
    fn lookup_ip_returns_none_when_no_lease() {
        let server = MockServer::start(|_| (200, "[]".to_string()));
        let c = client(&server.url());
        assert_eq!(c.lookup_ip("00:00:00:00:00:00").unwrap(), None);
    }

    #[test]
    fn surfaces_http_error_as_err() {
        let server = MockServer::start(|_| (500, r#"{"error":"boom"}"#.to_string()));
        let c = client(&server.url());
        assert!(
            c.admin_create_machine(
                "aa:bb:cc:dd:ee:ff",
                None,
                None,
                &NetworkSpec::default(),
                None
            )
            .is_err()
        );
    }

    #[test]
    fn machineref_is_installed_only_for_terminal_state() {
        assert!(
            MachineRef {
                id: "x".into(),
                status: "Installed".into()
            }
            .is_installed()
        );
        for s in [
            "Discovered",
            "ReadyToInstall",
            "Initializing",
            "Installing",
            "Writing",
            "ExistingOs",
            "Failed",
            "Offline",
        ] {
            assert!(
                !MachineRef {
                    id: "x".into(),
                    status: s.into()
                }
                .is_installed(),
                "status {s:?} should NOT count as installed"
            );
        }
    }

    #[test]
    fn find_machine_by_mac_returns_id_and_status() {
        let server = MockServer::start(|_| {
            (
                200,
                r#"{"data":[{"id":"m1","mac_address":"bc:24:11:22:33:44","status":"Installed","hostname":"a"},{"id":"m2","mac_address":"aa:bb:cc:dd:ee:ff","status":"Discovered"}]}"#.to_string(),
            )
        });
        let c = client(&server.url());
        // uppercase requesting MAC must match the lowercase entry
        let m = c
            .find_machine_by_mac("BC:24:11:22:33:44")
            .unwrap()
            .expect("machine should be found");
        assert_eq!(m.id, "m1");
        assert_eq!(m.status, "Installed");
        assert!(m.is_installed());
    }

    #[test]
    fn find_machine_by_mac_returns_none_when_absent() {
        let server = MockServer::start(|_| (200, r#"{"data":[]}"#.to_string()));
        let c = client(&server.url());
        assert_eq!(c.find_machine_by_mac("00:00:00:00:00:00").unwrap(), None);
    }
}
