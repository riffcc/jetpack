// Jetpack
// Copyright (C) Riff Labs <team@riff.cc>
// SPDX-License-Identifier: GPL-3.0-or-later

//! Native controller-side HTTP requests. Request data never enters a shell or
//! diagnostic; saved results are host variables, like command/shell `save`.

use crate::handle::handle::TaskHandle;
use crate::handle::template::BlendTarget;
use crate::tasks::*;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde::Deserialize;
use serde_yaml::{Mapping, Value};
use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct HttpTask {
    pub name: Option<String>,
    pub url: String,
    pub method: Option<String>,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    pub body: Option<String>,
    pub expected_status: Option<Vec<String>>,
    pub save: Option<String>,
    pub timeout: Option<String>,
    pub verify_tls: Option<String>,
    pub with: Option<PreLogicInput>,
    pub and: Option<PostLogicInput>,
}

// Redact every header, including custom authentication headers. URL and body
// may also carry credentials, so do not expose those through Task's Debug.
impl fmt::Debug for HttpTask {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HttpTask")
            .field("request", &"[REDACTED]")
            .finish_non_exhaustive()
    }
}

struct HttpAction {
    url: String,
    method: String,
    headers: BTreeMap<String, String>,
    body: Option<String>,
    expected_status: Option<Vec<String>>,
    save: Option<String>,
    timeout: String,
    verify_tls: String,
}

impl IsTask for HttpTask {
    fn get_module(&self) -> String {
        "http".into()
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
        // HTTP payloads allow empty strings and shell metacharacters. Suppress
        // template error details: they can quote the original secret-bearing
        // template. Parse/validate rendered values in dispatch, after Off mode.
        let render = |field: &str, input: &str| {
            handle
                .template
                .get_context()
                .read()
                .unwrap()
                .render_template(input, &handle.host, BlendTarget::NotTemplateModule, tm)
                .map_err(|_| {
                    handle
                        .response
                        .is_failed(request, &format!("http: cannot template {field}"))
                })
        };
        let optional = |field: &str, input: &Option<String>| {
            input.as_deref().map(|v| render(field, v)).transpose()
        };
        Ok(EvaluatedTask {
            action: Arc::new(HttpAction {
                url: render("url", &self.url)?,
                method: render("method", self.method.as_deref().unwrap_or("GET"))?,
                headers: self
                    .headers
                    .iter()
                    .map(|(key, value)| {
                        Ok((render("header name", key)?, render("header value", value)?))
                    })
                    .collect::<Result<_, Arc<TaskResponse>>>()?,
                body: optional("body", &self.body)?,
                expected_status: self
                    .expected_status
                    .as_ref()
                    .map(|codes| {
                        codes
                            .iter()
                            .map(|v| render("expected_status", v))
                            .collect::<Result<_, _>>()
                    })
                    .transpose()?,
                save: handle
                    .template
                    .string_option_no_spaces(request, tm, "save", &self.save)?,
                timeout: render("timeout", self.timeout.as_deref().unwrap_or("30"))?,
                verify_tls: render("verify_tls", self.verify_tls.as_deref().unwrap_or("true"))?,
            }),
            with: Arc::new(PreLogicInput::template(handle, request, tm, &self.with)?),
            and: Arc::new(PostLogicInput::template(handle, request, tm, &self.and)?),
        })
    }
}

impl IsAction for HttpAction {
    fn dispatch(
        &self,
        handle: &Arc<TaskHandle>,
        request: &Arc<TaskRequest>,
    ) -> Result<Arc<TaskResponse>, Arc<TaskResponse>> {
        match request.request_type {
            // Check mode stops here, without sending even a GET request.
            TaskRequestType::Query => Ok(handle.response.needs_execution(request)),
            TaskRequestType::Execute => {
                let result = self
                    .send()
                    .map_err(|e| handle.response.is_failed(request, &e))?;
                if let Some(save) = &self.save {
                    let mut vars = Mapping::new();
                    vars.insert(
                        Value::String(save.clone()),
                        Value::Mapping(result.results()),
                    );
                    handle.host.write().unwrap().update_variables(vars);
                }
                if !result.expected {
                    return Err(handle.response.is_failed(
                        request,
                        &format!("http: unexpected status {}", result.status),
                    ));
                }
                Ok(handle.response.is_executed(request))
            }
            _ => Err(handle.response.not_supported(request)),
        }
    }
}

struct HttpResult {
    status: u16,
    body: String,
    headers: BTreeMap<String, Vec<String>>,
    expected: bool,
}

impl HttpResult {
    fn results(&self) -> Mapping {
        let mut result = Mapping::new();
        result.insert("status".into(), Value::Number(self.status.into()));
        result.insert("body".into(), self.body.clone().into());
        // Match the command/shell convention for consumers of rc/out.
        result.insert("rc".into(), Value::Number(i32::from(!self.expected).into()));
        result.insert("out".into(), self.body.clone().into());
        result.insert(
            "headers".into(),
            serde_yaml::to_value(&self.headers).unwrap(),
        );
        result
    }
}

fn sensitive_header(name: &str) -> bool {
    let name = name.to_ascii_lowercase();
    matches!(
        name.as_str(),
        "authorization" | "proxy-authorization" | "cookie" | "set-cookie"
    ) || name.contains("token")
        || name.contains("key")
        || name.contains("secret")
}

impl HttpAction {
    fn send(&self) -> Result<HttpResult, String> {
        // Keep errors independent of request values and reqwest's URL-bearing
        // error strings, including malformed custom headers and method names.
        let url = reqwest::Url::parse(&self.url).map_err(|_| "http: invalid URL")?;
        if !matches!(url.scheme(), "http" | "https") {
            return Err("http: URL must use HTTP or HTTPS".into());
        }
        let method = reqwest::Method::from_bytes(self.method.to_ascii_uppercase().as_bytes())
            .map_err(|_| "http: invalid method")?;
        let timeout = self
            .timeout
            .trim()
            .parse::<u64>()
            .ok()
            .filter(|v| *v > 0)
            .ok_or("http: timeout must be a positive integer in seconds")?;
        let verify_tls = self
            .verify_tls
            .trim()
            .parse::<bool>()
            .map_err(|_| "http: verify_tls must be true or false")?;
        let expected = self
            .expected_status
            .as_ref()
            .map(|codes| {
                if codes.is_empty() {
                    return Err("http: expected_status must not be empty");
                }
                codes
                    .iter()
                    .map(|code| {
                        code.trim()
                            .parse::<u16>()
                            .ok()
                            .filter(|v| (100..=599).contains(v))
                            .ok_or("http: expected_status must contain HTTP status codes")
                    })
                    .collect::<Result<Vec<_>, _>>()
            })
            .transpose()?;
        let mut headers = HeaderMap::new();
        for (key, value) in &self.headers {
            let key =
                HeaderName::from_bytes(key.as_bytes()).map_err(|_| "http: invalid header name")?;
            let mut value =
                HeaderValue::from_str(value).map_err(|_| "http: invalid header value")?;
            // Treat all request headers as sensitive, including custom auth.
            value.set_sensitive(true);
            headers.insert(key, value);
        }
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| "http: cannot create runtime")?;
        rt.block_on(async {
            let client = reqwest::Client::builder()
                .timeout(Duration::from_secs(timeout))
                .danger_accept_invalid_certs(!verify_tls)
                // Custom authentication headers must never follow a redirect
                // to a different endpoint. Capture the 3xx response instead.
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .map_err(|_| "http: cannot create client")?;
            let mut request = client.request(method, url).headers(headers);
            if let Some(body) = &self.body {
                request = request.body(body.clone());
            }
            let response = request.send().await.map_err(|e| {
                if e.is_timeout() {
                    "http: request timed out"
                } else if e.is_connect() {
                    "http: connection failed"
                } else {
                    "http: request failed"
                }
            })?;
            let status = response.status();
            let mut headers = BTreeMap::new();
            for name in response.headers().keys() {
                let values = response
                    .headers()
                    .get_all(name)
                    .iter()
                    .map(|value| {
                        if sensitive_header(name.as_str()) {
                            "[REDACTED]".into()
                        } else {
                            String::from_utf8_lossy(value.as_bytes()).into_owned()
                        }
                    })
                    .collect();
                headers.insert(name.to_string(), values);
            }
            let body = response
                .text()
                .await
                .map_err(|_| "http: cannot read response body")?;
            Ok(HttpResult {
                status: status.as_u16(),
                body,
                headers,
                expected: expected.as_ref().map_or_else(
                    || status.is_success(),
                    |codes| codes.contains(&status.as_u16()),
                ),
            })
        })
    }
}

#[cfg(test)]
mod tests;
