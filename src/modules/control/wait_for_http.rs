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
// along with this program.  If not, see <http://www.gnu.org/licenses/>.

//! Wait for an HTTP(S) endpoint to become healthy — e.g. an API readiness
//! probe. Polls the URL from the controller (reqwest) until it returns a
//! success status, or a configured expected status. No curl, no shell sleep
//! loop. `verify_tls: false` skips TLS verification for self-signed endpoints
//! (a freshly-bootstrapped k3s API, for example).

use crate::handle::handle::TaskHandle;
use crate::tasks::*;
use serde::Deserialize;
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

const MODULE: &str = "wait_for_http";

#[derive(Deserialize, Debug, Default)]
#[serde(deny_unknown_fields)]
pub struct WaitForHttpTask {
    pub name: Option<String>,
    /// HTTP(S) URL to poll until healthy.
    pub url: String,
    /// Timeout in seconds (default 300).
    pub timeout: Option<String>,
    /// Delay between attempts in seconds (default 5).
    pub delay: Option<String>,
    /// Expected status code (e.g. "200"). Default: any 2xx.
    pub expected: Option<String>,
    /// Verify the TLS certificate (default true; set false for self-signed).
    #[serde(default = "default_verify_tls")]
    pub verify_tls: bool,
    pub with: Option<PreLogicInput>,
    pub and: Option<PostLogicInput>,
}

fn default_verify_tls() -> bool {
    true
}

struct WaitForHttpAction {
    url: String,
    // Numeric fields are kept as their templated *strings* and parsed in
    // dispatch(), NOT in evaluate(). evaluate() runs once in TemplateMode::Off
    // before the task's skip-condition is considered; in Off mode the templar
    // substitutes the literal "empty" for every value (see templar.rs), so
    // parsing here would abort the task before its `with.condition` could skip
    // it. Parsing under Strict in dispatch() sees the real rendered values.
    timeout: Option<String>,
    delay: Option<String>,
    expected: Option<String>,
    verify_tls: bool,
}

impl IsTask for WaitForHttpTask {
    fn get_module(&self) -> String {
        String::from(MODULE)
    }
    fn get_name(&self) -> Option<String> {
        self.name.clone()
    }
    fn get_with(&self) -> Option<PreLogicInput> {
        self.with.clone()
    }

    fn evaluate(
        &self,
        handle: &Arc<TaskHandle>,
        request: &Arc<TaskRequest>,
        tm: TemplateMode,
    ) -> Result<EvaluatedTask, Arc<TaskResponse>> {
        let url = handle
            .template
            .string(request, tm, &String::from("url"), &self.url)?;
        Ok(EvaluatedTask {
            action: Arc::new(WaitForHttpAction {
                url,
                timeout: handle.template.string_option(
                    request,
                    tm,
                    &String::from("timeout"),
                    &self.timeout,
                )?,
                delay: handle.template.string_option(
                    request,
                    tm,
                    &String::from("delay"),
                    &self.delay,
                )?,
                expected: handle.template.string_option(
                    request,
                    tm,
                    &String::from("expected"),
                    &self.expected,
                )?,
                verify_tls: self.verify_tls,
            }),
            with: Arc::new(PreLogicInput::template(handle, request, tm, &self.with)?),
            and: Arc::new(PostLogicInput::template(handle, request, tm, &self.and)?),
        })
    }
}

/// Parse an already-templated duration field into seconds, defaulting to
/// `default` when unset. Free function (no host/handle) so it is unit-testable.
fn parse_secs(field: &str, value: &Option<String>, default: u64) -> Result<u64, String> {
    match value {
        Some(v) => v
            .trim()
            .parse::<u64>()
            .map_err(|e| format!("{}: {}", field, e)),
        None => Ok(default),
    }
}

impl IsAction for WaitForHttpAction {
    fn dispatch(
        &self,
        handle: &Arc<TaskHandle>,
        request: &Arc<TaskRequest>,
    ) -> Result<Arc<TaskResponse>, Arc<TaskResponse>> {
        match request.request_type {
            TaskRequestType::Query => Ok(handle.response.needs_passive(request)),
            TaskRequestType::Passive => {
                // The action was templated under Strict in evaluate(); these are
                // the real rendered values. Parse now, not earlier.
                let timeout = match parse_secs("timeout", &self.timeout, 300) {
                    Ok(v) => v,
                    Err(e) => return Err(handle.response.is_failed(request, &e)),
                };
                let delay = match parse_secs("delay", &self.delay, 5) {
                    Ok(v) => v,
                    Err(e) => return Err(handle.response.is_failed(request, &e)),
                };
                let expected = match &self.expected {
                    Some(code) => match code.trim().parse::<u16>() {
                        Ok(v) => Some(v),
                        Err(e) => {
                            return Err(handle
                                .response
                                .is_failed(request, &format!("expected: {}", e)));
                        }
                    },
                    None => None,
                };
                match poll_until_ready(&self.url, self.verify_tls, expected, timeout, delay) {
                    Ok(()) => Ok(handle.response.is_passive(request)),
                    Err(e) => Err(handle.response.is_failed(request, &e)),
                }
            }
            _ => Err(handle.response.not_supported(request)),
        }
    }
}

/// Poll `url` until it answers success (or `expected`), or `timeout_secs` elapses.
/// Controller-side, via reqwest. Exposed for unit tests.
fn poll_until_ready(
    url: &str,
    verify_tls: bool,
    expected: Option<u16>,
    timeout_secs: u64,
    delay_secs: u64,
) -> Result<(), String> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("wait_for_http: runtime: {}", e))?;
    let client = reqwest::Client::builder()
        .danger_accept_invalid_certs(!verify_tls)
        .build()
        .map_err(|e| format!("wait_for_http: http client: {}", e))?;
    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    let delay = Duration::from_secs(delay_secs);
    loop {
        let ready = rt.block_on(async {
            match client.get(url).send().await {
                Ok(resp) => match expected {
                    Some(code) => resp.status().as_u16() == code,
                    None => resp.status().is_success(),
                },
                Err(_) => false,
            }
        });
        if ready {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(format!(
                "wait_for_http: timed out waiting for {} after {}s",
                url, timeout_secs
            ));
        }
        thread::sleep(delay);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::net::TcpListener;

    /// Minimal HTTP/1.1 mock: answers every request with `status`. Runs in a
    /// plain OS thread (no tokio) so it can't clash with the client runtime.
    fn mock_http(status: u16) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let mut s = match stream {
                    Ok(s) => s,
                    Err(_) => continue,
                };
                let body = format!(
                    "HTTP/1.1 {} OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    status
                );
                let _ = s.write_all(body.as_bytes());
            }
        });
        format!("http://{}", addr)
    }

    #[test]
    fn returns_ok_when_endpoint_is_2xx() {
        let url = mock_http(200);
        assert!(poll_until_ready(&url, true, None, 5, 0).is_ok());
    }

    #[test]
    fn times_out_when_unreachable() {
        // port 1 is never open on loopback -> connection refused every poll
        let err = poll_until_ready("http://127.0.0.1:1", true, None, 1, 0).unwrap_err();
        assert!(err.contains("timed out"), "{}", err);
    }

    #[test]
    fn honors_expected_status() {
        let url = mock_http(204);
        // expects 204 -> ok immediately
        assert!(poll_until_ready(&url, true, Some(204), 5, 0).is_ok());
        // expects 200 but endpoint serves 204 -> never ready -> timeout
        assert!(poll_until_ready(&url, true, Some(200), 1, 0).is_err());
    }

    #[test]
    fn parse_secs_uses_default_when_unset() {
        assert_eq!(parse_secs("timeout", &None, 300).unwrap(), 300);
    }

    #[test]
    fn parse_secs_parses_rendered_number() {
        // role tasks write `timeout: 240` (a YAML integer); the struct field is
        // Option<String> and serde_yaml renders the scalar as "240".
        assert_eq!(
            parse_secs("timeout", &Some("240".to_string()), 300).unwrap(),
            240
        );
        // tolerate surrounding whitespace from templating
        assert_eq!(
            parse_secs("delay", &Some(" 2 \n".to_string()), 5).unwrap(),
            2
        );
    }

    #[test]
    fn parse_secs_rejects_garbage() {
        // e.g. what TemplateMode::Off would have produced before parsing moved
        // out of evaluate() — must surface as a clear error, never panic.
        let err = parse_secs("timeout", &Some("empty".to_string()), 300).unwrap_err();
        assert!(err.contains("timeout"), "{}", err);
    }

    #[test]
    fn deserializes_unquoted_numeric_fields() {
        // role tasks write bare integers for timeout/delay; the text load path
        // (serde_yaml::from_str) must render them as parseable strings.
        let yaml = "url: https://x\ntimeout: 240\ndelay: 2\nverify_tls: false\n";
        let task: WaitForHttpTask = serde_yaml::from_str(yaml).expect("deserialize");
        assert_eq!(task.timeout.as_deref(), Some("240"));
        assert_eq!(task.delay.as_deref(), Some("2"));
    }
}
