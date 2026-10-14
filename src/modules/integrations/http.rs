// Jetpack
// Copyright (C) Riff Labs <team@riff.cc>
// SPDX-License-Identifier: GPL-3.0-or-later

//! Native controller-side HTTP requests. Request data never enters a shell or
//! diagnostic; saved results are host variables, like command/shell `save`.

use crate::handle::handle::TaskHandle;
use crate::handle::template::BlendTarget;
use crate::playbooks::structural::StructuralMatch;
use crate::playbooks::templar::Templar;
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
    pub changed_when: Option<String>,
    pub check: Option<HttpCheck>,
    pub timeout: Option<String>,
    pub verify_tls: Option<String>,
    pub with: Option<PreLogicInput>,
    pub and: Option<PostLogicInput>,
}

/// A read-only request whose response establishes whether mutation is needed.
/// Request settings are independent of the mutation, including authentication.
#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct HttpCheck {
    pub url: String,
    pub method: Option<String>,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    pub body: Option<String>,
    pub expected_status: Option<Vec<String>>,
    pub timeout: Option<String>,
    pub verify_tls: Option<String>,
    pub matches: HttpMatch,
}

/// Keep expression matching and explicit structural matching complementary.
#[derive(Deserialize, Clone)]
#[serde(untagged)]
pub enum HttpMatch {
    Expression(String),
    Structural(StructuralMatch),
}

impl Default for HttpMatch {
    fn default() -> Self {
        Self::Expression(String::new())
    }
}

impl From<&str> for HttpMatch {
    fn from(value: &str) -> Self {
        Self::Expression(value.into())
    }
}

impl From<String> for HttpMatch {
    fn from(value: String) -> Self {
        Self::Expression(value)
    }
}

impl fmt::Debug for HttpCheck {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HttpCheck")
            .field("request", &"[REDACTED]")
            .finish_non_exhaustive()
    }
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
    request: HttpRequest,
    check: Option<HttpCheckAction>,
    save: Option<String>,
    changed_when: Option<String>,
    condition_vars: Mapping,
}

struct HttpCheckAction {
    request: HttpRequest,
    matches: HttpMatch,
}

struct HttpRequest {
    url: String,
    method: String,
    headers: BTreeMap<String, String>,
    body: Option<String>,
    expected_status: Option<Vec<String>>,
    timeout: String,
    verify_tls: String,
}

struct HttpRequestInput<'a> {
    url: &'a str,
    method: &'a Option<String>,
    headers: &'a BTreeMap<String, String>,
    body: &'a Option<String>,
    expected_status: &'a Option<Vec<String>>,
    timeout: &'a Option<String>,
    verify_tls: &'a Option<String>,
}

impl HttpRequestInput<'_> {
    fn template(
        &self,
        render: &impl Fn(&str, &str) -> Result<String, Arc<TaskResponse>>,
    ) -> Result<HttpRequest, Arc<TaskResponse>> {
        Ok(HttpRequest {
            url: render("url", self.url)?,
            method: render("method", self.method.as_deref().unwrap_or("GET"))?,
            headers: self
                .headers
                .iter()
                .map(|(key, value)| {
                    Ok((render("header name", key)?, render("header value", value)?))
                })
                .collect::<Result<_, Arc<TaskResponse>>>()?,
            body: self
                .body
                .as_deref()
                .map(|v| render("body", v))
                .transpose()?,
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
            timeout: render("timeout", self.timeout.as_deref().unwrap_or("30"))?,
            verify_tls: render("verify_tls", self.verify_tls.as_deref().unwrap_or("true"))?,
        })
    }
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
        Ok(EvaluatedTask {
            action: Arc::new(HttpAction {
                request: HttpRequestInput {
                    url: &self.url,
                    method: &self.method,
                    headers: &self.headers,
                    body: &self.body,
                    expected_status: &self.expected_status,
                    timeout: &self.timeout,
                    verify_tls: &self.verify_tls,
                }
                .template(&render)?,
                check: self
                    .check
                    .as_ref()
                    .map(|check| {
                        Ok::<_, Arc<TaskResponse>>(HttpCheckAction {
                            request: HttpRequestInput {
                                url: &check.url,
                                method: &check.method,
                                headers: &check.headers,
                                body: &check.body,
                                expected_status: &check.expected_status,
                                timeout: &check.timeout,
                                verify_tls: &check.verify_tls,
                            }
                            .template(&|field, input| render(&format!("check.{field}"), input))?,
                            matches: check.matches.clone(),
                        })
                    })
                    .transpose()?,
                save: handle
                    .template
                    .string_option_no_spaces(request, tm, "save", &self.save)?,
                // Preserve the condition until the actual response exists.
                changed_when: self.changed_when.clone(),
                // Capture variables while this invocation's item/host context
                // is active. Neither dispatch nor parallel invocations mutate
                // this snapshot or publish the temporary response binding.
                condition_vars: handle
                    .template
                    .get_context()
                    .read()
                    .unwrap()
                    .get_complete_blended_variables(&handle.host, BlendTarget::NotTemplateModule),
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
            TaskRequestType::Query => {
                if let Some(check) = &self.check {
                    // Enforce read-only preflight methods before any I/O.
                    // The engine never enters Execute in plan/check mode.
                    if !matches!(
                        check.request.method.to_ascii_uppercase().as_str(),
                        "GET" | "HEAD"
                    ) {
                        return Err(handle
                            .response
                            .is_failed(request, "http: check method must be GET or HEAD"));
                    }
                    let result = check.request.send().map_err(|e| {
                        handle.response.is_failed(
                            request,
                            &format!("http: check: {}", e.strip_prefix("http: ").unwrap_or(&e)),
                        )
                    })?;
                    if !result.expected {
                        return Err(handle.response.is_failed(
                            request,
                            &format!("http: check unexpected status {}", result.status),
                        ));
                    }
                    if result
                        .test_match(&check.matches, &self.condition_vars)
                        .map_err(|e| handle.response.is_failed(request, &e))?
                    {
                        return Ok(handle.response.is_matched(request));
                    }
                }
                Ok(handle.response.needs_execution(request))
            }
            TaskRequestType::Execute => {
                let result = self
                    .request
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
                let changed = match &self.changed_when {
                    Some(condition) => result
                        .test_condition(condition, &self.condition_vars, "changed_when")
                        .map_err(|e| handle.response.is_failed(request, &e))?,
                    None => true,
                };
                if changed {
                    Ok(handle.response.is_executed(request))
                } else {
                    Ok(handle.response.is_passive(request))
                }
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
        // Saving a non-JSON/empty response remains valid. Parsing errors are
        // surfaced only if a condition reaches response.json.
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&self.body) {
            result.insert("json".into(), serde_yaml::to_value(json).unwrap());
        }
        result
    }

    fn test_match(&self, matcher: &HttpMatch, variables: &Mapping) -> Result<bool, String> {
        match matcher {
            HttpMatch::Expression(expression) => {
                self.test_condition(expression, variables, "check.matches")
            }
            HttpMatch::Structural(matcher) => {
                let path = Templar::new()
                    .parse_value_path(&matcher.path)
                    .map_err(|_| "http: invalid check.matches path")?;
                if response_json_path(&path) {
                    // An explicitly accepted 404 establishes resource absence.
                    // Unexpected statuses have already failed before this call.
                    if self.status == 404 {
                        return Ok(false);
                    }
                    self.require_json("check.matches")?;
                }
                let mut data = variables.clone();
                data.insert("response".into(), Value::Mapping(self.results()));
                matcher
                    .test(&data)
                    .map_err(|_| "http: invalid check.matches structural matcher".into())
            }
        }
    }

    fn require_json(&self, field: &str) -> Result<(), String> {
        if self.body.trim().is_empty() {
            Err(format!(
                "http: {field} response (status {}) has no JSON body",
                self.status
            ))
        } else if serde_json::from_str::<serde_json::Value>(&self.body).is_err() {
            Err(format!(
                "http: {field} response (status {}) is not valid JSON",
                self.status
            ))
        } else {
            Ok(())
        }
    }

    fn test_condition(
        &self,
        expression: &str,
        variables: &Mapping,
        field: &str,
    ) -> Result<bool, String> {
        let mut data = variables.clone();
        // Replace the whole binding, rather than merging in stale response
        // fields from inventory, previous saves, or another item iteration.
        data.insert("response".into(), Value::Mapping(self.results()));
        let mut json_error = None;
        let mut path_check = |path: &handlebars::Path| {
            if response_json_path(path)
                && let Err(error) = self.require_json(field)
            {
                json_error = Some(error.clone());
                return Err(error);
            }
            Ok(())
        };
        Templar::new()
            .test_condition_with_path_check(expression, data, &mut path_check)
            .map_err(|_| json_error.unwrap_or_else(|| format!("http: invalid {field} condition")))
    }
}

fn response_json_path(path: &handlebars::Path) -> bool {
    use handlebars::{Path, PathSeg};
    match path {
        Path::Relative((segments, _)) => {
            let mut names = segments.iter().filter_map(|segment| match segment {
                PathSeg::Named(name) => Some(name.as_str()),
                _ => None,
            });
            names.next() == Some("response") && names.next() == Some("json")
        }
        Path::Local((0, name, raw)) if name == "root" => raw
            .strip_prefix("@root.")
            .or_else(|| raw.strip_prefix("@root/"))
            .and_then(|rest| Path::parse(rest).ok())
            .is_some_and(|path| response_json_path(&path)),
        _ => false,
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

impl HttpRequest {
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

#[cfg(test)]
mod preflight_tests;

#[cfg(test)]
mod structural_tests;
