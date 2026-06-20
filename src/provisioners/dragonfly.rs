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

#[derive(Serialize)]
struct RegisterRequest<'a> {
    mac_address: &'a str,
    ip_address: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    hostname: Option<&'a str>,
    // Dragonfly's RegisterRequest requires these (non-Option, no default). We
    // pre-register before PXE boot so disk/nameserver info isn't known yet —
    // the agent fills them in on check-in.
    disks: &'a [serde_json::Value],
    nameservers: &'a [String],
}

#[derive(Deserialize, Debug, PartialEq)]
pub struct RegisterResponse {
    pub machine_id: String,
    pub next_step: String,
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

    /// Register (or update) a machine by MAC. Idempotent on the server side.
    pub fn register_machine(
        &self,
        mac: &str,
        ip_address: &str,
        hostname: Option<&str>,
    ) -> Result<RegisterResponse, String> {
        let body = RegisterRequest {
            mac_address: mac,
            ip_address,
            hostname,
            disks: &[],
            nameservers: &[],
        };
        let resp = self.post("/machines", &body)?;
        serde_json::from_str::<RegisterResponse>(&resp.body).map_err(|e| {
            format!(
                "dragonfly register: parse response: {} (body: {})",
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
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};
    use std::thread;

    /// A recorded incoming request, for assertions.
    #[derive(Clone, Debug)]
    struct RecordedReq {
        method: String,
        path: String,
        auth: Option<String>,
        body: String,
    }

    /// Minimal HTTP/1.1 mock: responds per a closure, records every request.
    /// Runs in a plain OS thread (no tokio) so it cannot clash with the
    /// client's current-thread runtime. The listener thread detaches when the
    /// test ends.
    struct MockServer {
        addr: String,
        requests: Arc<Mutex<Vec<RecordedReq>>>,
    }

    impl MockServer {
        fn start<F>(responder: F) -> Self
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

        fn recorded(&self) -> Vec<RecordedReq> {
            self.requests.lock().unwrap().clone()
        }

        fn url(&self) -> String {
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

    fn client(addr: &str) -> DragonflyClient {
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
    fn register_machine_posts_mac_and_hostname_with_bearer_auth() {
        let server = MockServer::start(|_| {
            (
                201,
                r#"{"machine_id":"0192abcd","next_step":"awaiting_os_assignment"}"#.to_string(),
            )
        });
        let c = client(&server.url());
        let resp = c
            .register_machine("BC:24:11:22:33:44", "", Some("k8s01"))
            .unwrap();
        assert_eq!(resp.machine_id, "0192abcd");
        assert_eq!(resp.next_step, "awaiting_os_assignment");

        let recorded = server.recorded();
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].method, "POST");
        assert_eq!(recorded[0].path, "/api/machines");
        assert_eq!(recorded[0].auth.as_deref(), Some("bearer df_test_token"));
        let body: serde_json::Value = serde_json::from_str(&recorded[0].body).unwrap();
        assert_eq!(body["mac_address"], "BC:24:11:22:33:44");
        assert_eq!(body["ip_address"], "");
        assert_eq!(body["hostname"], "k8s01");
        assert_eq!(body["disks"], serde_json::json!([]));
        assert_eq!(body["nameservers"], serde_json::json!([]));
    }

    #[test]
    fn register_machine_accepts_update_status() {
        let server = MockServer::start(|_| {
            (
                200,
                r#"{"machine_id":"0192abcd","next_step":"awaiting_os_assignment"}"#.to_string(),
            )
        });
        let c = client(&server.url());
        assert!(c.register_machine("aa:bb:cc:dd:ee:ff", "", None).is_ok());
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
        assert!(c.register_machine("aa:bb:cc:dd:ee:ff", "", None).is_err());
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
