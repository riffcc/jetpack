// Copyright (C) Riff Labs <team@riff.cc>
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use crate::registry::list::Task;
use crate::test_support::{RecordingConnection, query_request, test_handle};
use std::sync::Mutex;
use wiremock::matchers::{body_string, header, method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn handle() -> Arc<TaskHandle> {
    test_handle(Arc::new(Mutex::new(RecordingConnection::new())))
}

async fn execute(
    task: HttpTask,
    handle: Arc<TaskHandle>,
) -> Result<Arc<TaskResponse>, Arc<TaskResponse>> {
    // The real task engine dispatches on synchronous worker threads.
    tokio::task::spawn_blocking(move || {
        let query = query_request();
        let evaluated = task.evaluate(&handle, &query, TemplateMode::Strict)?;
        evaluated.action.dispatch(
            &handle,
            &TaskRequest::execute(query.sudo_details.as_ref().unwrap()),
        )
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn templates_request_and_captures_response_for_later_tasks() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/v1/dhcp/scopes"))
        .and(query_param("scope", "example.com"))
        .and(header("authorization", "Bearer secret-token"))
        .and(header("content-type", "application/json"))
        .and(body_string(r#"{"name":"example.com","enabled":true}"#))
        .respond_with(
            ResponseTemplate::new(201)
                .set_body_string(r#"{"created":true}"#)
                .insert_header("X-Request-Id", "request-1")
                .append_header("Set-Cookie", "session=secret-cookie")
                .append_header("Set-Cookie", "other=secret-cookie")
                .insert_header("X-Api-Key", "secret-key"),
        )
        .expect(1)
        .mount(&server)
        .await;
    let task: Task = serde_yaml::from_str(
        r#"!http
url: "{{ endpoint }}/api/v1/dhcp/scopes?scope=example.com"
method: "{{ verb }}"
headers:
  Authorization: "Bearer {{ token }}"
  Content-Type: application/json
body: "{{ payload }}"
expected_status: [200, "{{ status }}"]
save: "{{ result_name }}"
timeout: 5
verify_tls: true
"#,
    )
    .unwrap();
    assert_eq!(task.get_module(), "http");
    assert_eq!(task.category(), "integrations");
    let Task::Http(task) = task else {
        panic!("expected HTTP task")
    };
    let handle = handle();
    handle.host.write().unwrap().update_variables(serde_yaml::from_str(&format!(
        "endpoint: {}\nverb: POST\ntoken: secret-token\nstatus: 201\nresult_name: response\npayload: '{{\"name\":\"example.com\",\"enabled\":true}}'",
        server.uri()
    )).unwrap());
    let response = execute(task, handle.clone()).await.unwrap();
    assert_eq!(response.status, TaskStatus::IsExecuted);
    assert!(response.command_result.is_none());
    let vars = Value::Mapping(handle.host.read().unwrap().get_variables());
    assert_eq!(vars["response"]["status"], 201);
    assert_eq!(vars["response"]["rc"], 0);
    assert_eq!(vars["response"]["body"], r#"{"created":true}"#);
    assert_eq!(vars["response"]["out"], vars["response"]["body"]);
    assert_eq!(vars["response"]["headers"]["x-request-id"][0], "request-1");
    assert_eq!(vars["response"]["headers"]["set-cookie"][0], "[REDACTED]");
    assert_eq!(vars["response"]["headers"]["set-cookie"][1], "[REDACTED]");
    assert_eq!(vars["response"]["headers"]["x-api-key"][0], "[REDACTED]");
    let rendered = handle
        .template
        .get_context()
        .read()
        .unwrap()
        .render_template(
            "{{ response.status }}",
            &handle.host,
            BlendTarget::NotTemplateModule,
            TemplateMode::Strict,
        )
        .unwrap();
    assert_eq!(rendered, "201");
}

#[tokio::test]
async fn defaults_to_get_and_accepts_any_success_status() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;
    let handle = handle();
    execute(
        HttpTask {
            url: server.uri(),
            save: Some("response".into()),
            ..Default::default()
        },
        handle.clone(),
    )
    .await
    .unwrap();
    let vars = Value::Mapping(handle.host.read().unwrap().get_variables());
    assert_eq!(vars["response"]["status"], 204);
    assert_eq!(vars["response"]["body"], "");
}

#[tokio::test]
async fn captures_unexpected_status_without_logging_body_or_headers() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(401)
                .set_body_string("secret-response")
                .insert_header("Authorization", "secret-header"),
        )
        .expect(1)
        .mount(&server)
        .await;
    let handle = handle();
    let error = execute(
        HttpTask {
            url: server.uri(),
            save: Some("response".into()),
            ..Default::default()
        },
        handle.clone(),
    )
    .await
    .unwrap_err();
    assert_eq!(error.msg.as_deref(), Some("http: unexpected status 401"));
    assert!(!format!("{error:?}").contains("secret"));
    let vars = Value::Mapping(handle.host.read().unwrap().get_variables());
    assert_eq!(vars["response"]["rc"], 1);
    assert_eq!(vars["response"]["body"], "secret-response");
    assert_eq!(
        vars["response"]["headers"]["authorization"][0],
        "[REDACTED]"
    );
}

#[tokio::test]
async fn explicitly_expected_error_status_succeeds() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(404))
        .expect(1)
        .mount(&server)
        .await;
    execute(
        HttpTask {
            url: server.uri(),
            expected_status: Some(vec!["404".into()]),
            ..Default::default()
        },
        handle(),
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn query_sends_no_request_and_does_not_save_results() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&server)
        .await;
    let handle = handle();
    let query = query_request();
    let task = HttpTask {
        url: server.uri(),
        method: Some("POST".into()),
        save: Some("response".into()),
        ..Default::default()
    };
    let evaluated = task
        .evaluate(&handle, &query, TemplateMode::Strict)
        .unwrap();
    let response = evaluated.action.dispatch(&handle, &query).unwrap();
    assert_eq!(response.status, TaskStatus::NeedsExecution);
    assert!(handle.host.read().unwrap().get_variables().is_empty());
}

#[tokio::test]
async fn redirects_are_captured_without_forwarding_custom_authentication() {
    let destination = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&destination)
        .await;
    let source = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(302).insert_header("Location", destination.uri()))
        .expect(1)
        .mount(&source)
        .await;
    execute(
        HttpTask {
            url: source.uri(),
            headers: BTreeMap::from([("X-Custom-Auth".into(), "secret".into())]),
            expected_status: Some(vec!["302".into()]),
            ..Default::default()
        },
        handle(),
    )
    .await
    .unwrap();
}

#[tokio::test]
async fn times_out_without_leaking_url_credentials() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(3)))
        .mount(&server)
        .await;
    let error = execute(
        HttpTask {
            url: format!("{}?token=secret", server.uri()),
            timeout: Some("1".into()),
            ..Default::default()
        },
        handle(),
    )
    .await
    .unwrap_err();
    assert_eq!(error.msg.as_deref(), Some("http: request timed out"));
}

#[test]
fn off_evaluation_does_not_validate_placeholder_values() {
    let task: HttpTask = serde_yaml::from_str(
        "url: '{{ missing }}'\nexpected_status: ['{{ missing }}']\ntimeout: '{{ missing }}'\n",
    )
    .unwrap();
    assert!(
        task.evaluate(&handle(), &TaskRequest::validate(), TemplateMode::Off)
            .is_ok()
    );
}

#[test]
fn debug_and_template_errors_redact_custom_header_secrets() {
    let task: Task = serde_yaml::from_str(
        "!http\nurl: http://localhost\nheaders:\n  X-Custom-Auth: 'secret-{{ missing }}'\n",
    )
    .unwrap();
    assert!(!format!("{task:?}").contains("secret"));
    let error = task
        .evaluate(&handle(), &TaskRequest::validate(), TemplateMode::Strict)
        .err()
        .unwrap();
    assert_eq!(
        error.msg.as_deref(),
        Some("http: cannot template header value")
    );
    assert!(!format!("{error:?}").contains("secret"));
}

#[test]
fn invalid_inputs_fail_without_exposing_request_values() {
    let cases = [
        "url: 'http://[secret'",
        "url: 'file:///secret'",
        "url: http://localhost\nmethod: 'secret invalid'",
        "url: http://localhost\nheaders:\n  'secret invalid': secret",
        "url: http://localhost\nheaders:\n  Authorization: \"secret\\ninvalid\"",
        "url: http://localhost\nexpected_status: []",
        "url: http://localhost\nexpected_status: [99]",
        "url: http://localhost\nexpected_status: [600]",
        "url: http://localhost\ntimeout: 0",
        "url: http://localhost\nverify_tls: secret",
    ];
    for yaml in cases {
        let task: HttpTask = serde_yaml::from_str(yaml).unwrap();
        let handle = handle();
        let query = query_request();
        let evaluated = task
            .evaluate(&handle, &query, TemplateMode::Strict)
            .unwrap();
        let error = evaluated
            .action
            .dispatch(
                &handle,
                &TaskRequest::execute(query.sudo_details.as_ref().unwrap()),
            )
            .unwrap_err();
        assert!(!format!("{error:?}").contains("secret"));
        assert_eq!(error.status, TaskStatus::Failed);
    }
}

#[tokio::test]
async fn empty_body_and_header_values_are_valid() {
    let server = MockServer::start().await;
    Mock::given(method("PUT"))
        .and(body_string(""))
        .respond_with(ResponseTemplate::new(200))
        .expect(1)
        .mount(&server)
        .await;
    execute(
        HttpTask {
            url: server.uri(),
            method: Some("PUT".into()),
            body: Some(String::new()),
            headers: BTreeMap::from([("X-Empty".into(), String::new())]),
            ..Default::default()
        },
        handle(),
    )
    .await
    .unwrap();
}

#[test]
fn unknown_fields_are_rejected() {
    assert!(
        serde_yaml::from_str::<Task>("!http\nurl: http://localhost\npassword: secret\n").is_err()
    );
}

#[tokio::test]
async fn change_conditions_use_the_actual_response_after_success() {
    for (status, body, condition, expected) in [
        (200, "", "true", TaskStatus::IsExecuted),
        (
            200,
            "{{ missing_secret }}",
            "(contains response.body \"missing_secret\")",
            TaskStatus::IsExecuted,
        ),
        (204, "", "false", TaskStatus::IsPassive),
        (
            201,
            "not json",
            "(eq response.status 201)",
            TaskStatus::IsExecuted,
        ),
        (
            200,
            "not json",
            "(eq response.status 201)",
            TaskStatus::IsPassive,
        ),
        (
            200,
            r#"{"changed":true}"#,
            "(eq response.json.changed true)",
            TaskStatus::IsExecuted,
        ),
        (
            200,
            r#"{"changed":false}"#,
            "(eq response.json.changed true)",
            TaskStatus::IsPassive,
        ),
        (
            200,
            "updated secret-data",
            "(contains response.body \"updated\")",
            TaskStatus::IsExecuted,
        ),
        (
            200,
            "response.json is literal text",
            "(contains response.body \"response.json\")",
            TaskStatus::IsExecuted,
        ),
        (
            200,
            "",
            "(eq response.headers.[x-version].[0] \"v1\")",
            TaskStatus::IsExecuted,
        ),
    ] {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(
                ResponseTemplate::new(status)
                    .set_body_string(body)
                    .insert_header("X-Version", "v1"),
            )
            .expect(1)
            .mount(&server)
            .await;
        let result = execute(
            HttpTask {
                url: server.uri(),
                changed_when: Some(condition.into()),
                ..Default::default()
            },
            handle(),
        )
        .await
        .unwrap();
        assert_eq!(result.status, expected, "{condition}");
        assert!(result.msg.is_none());
        assert!(result.command_result.is_none());
    }
}

#[tokio::test]
async fn unexpected_status_wins_over_invalid_change_condition() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(500).set_body_string("secret-error"))
        .expect(1)
        .mount(&server)
        .await;
    let handle = handle();
    let error = execute(
        HttpTask {
            url: server.uri(),
            save: Some("saved".into()),
            changed_when: Some("(eq response.json.changed true)".into()),
            ..Default::default()
        },
        handle.clone(),
    )
    .await
    .unwrap_err();
    assert_eq!(error.msg.as_deref(), Some("http: unexpected status 500"));
    assert_eq!(
        Value::Mapping(handle.host.read().unwrap().get_variables())["saved"]["body"],
        "secret-error"
    );
}

#[tokio::test]
async fn json_access_distinguishes_absent_invalid_and_valid_null() {
    for (body, message) in [
        (
            "",
            "http: changed_when response (status 200) has no JSON body",
        ),
        (
            " \n\t",
            "http: changed_when response (status 200) has no JSON body",
        ),
        (
            "secret invalid json",
            "http: changed_when response (status 200) is not valid JSON",
        ),
    ] {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_string(body))
            .expect(1)
            .mount(&server)
            .await;
        let error = execute(
            HttpTask {
                url: server.uri(),
                changed_when: Some("response.json".into()),
                ..Default::default()
            },
            handle(),
        )
        .await
        .unwrap_err();
        assert_eq!(error.msg.as_deref(), Some(message));
        assert!(!format!("{error:?}").contains("secret"));
    }
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_string("null"))
        .expect(1)
        .mount(&server)
        .await;
    let handle = handle();
    let response = execute(
        HttpTask {
            url: server.uri(),
            changed_when: Some("response.json".into()),
            save: Some("saved".into()),
            ..Default::default()
        },
        handle.clone(),
    )
    .await
    .unwrap();
    assert_eq!(response.status, TaskStatus::IsPassive);
    assert!(
        Value::Mapping(handle.host.read().unwrap().get_variables())["saved"]
            .as_mapping()
            .unwrap()
            .contains_key(Value::from("json"))
    );
}

#[tokio::test]
async fn logical_guards_skip_json_validation() {
    for condition in [
        "(and (eq response.status 200) (eq response.json.changed true))",
        "(and false (eq response.json.changed true))",
        "(eq (or true response.json) true)",
    ] {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(404))
            .expect(1)
            .mount(&server)
            .await;
        execute(
            HttpTask {
                url: server.uri(),
                expected_status: Some(vec!["404".into()]),
                changed_when: Some(condition.into()),
                ..Default::default()
            },
            handle(),
        )
        .await
        .unwrap();
    }
}

#[tokio::test]
async fn transient_response_replaces_stale_bindings_without_publishing() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(204))
        .expect(2)
        .mount(&server)
        .await;
    let handle = handle();
    let original: Mapping =
        serde_yaml::from_str("response:\n  status: 201\n  json:\n    changed: true\n").unwrap();
    handle
        .host
        .write()
        .unwrap()
        .update_variables(original.clone());
    let response = execute(
        HttpTask {
            url: server.uri(),
            changed_when: Some("(eq response.status 201)".into()),
            ..Default::default()
        },
        handle.clone(),
    )
    .await
    .unwrap();
    assert_eq!(response.status, TaskStatus::IsPassive);
    let error = execute(
        HttpTask {
            url: server.uri(),
            changed_when: Some("(eq response.json.changed true)".into()),
            ..Default::default()
        },
        handle.clone(),
    )
    .await
    .unwrap_err();
    assert_eq!(
        error.msg.as_deref(),
        Some("http: changed_when response (status 204) has no JSON body")
    );
    assert_eq!(handle.host.read().unwrap().get_variables(), original);
}

#[tokio::test]
async fn json_is_saved_when_valid_without_requiring_json_for_success() {
    for (body, has_json) in [
        (r#"{"changed":true}"#, true),
        ("", false),
        ("plain text", false),
    ] {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_string(body))
            .expect(1)
            .mount(&server)
            .await;
        let handle = handle();
        execute(
            HttpTask {
                url: server.uri(),
                save: Some("saved".into()),
                changed_when: Some("false".into()),
                ..Default::default()
            },
            handle.clone(),
        )
        .await
        .unwrap();
        let vars = Value::Mapping(handle.host.read().unwrap().get_variables());
        assert_eq!(
            vars["saved"]
                .as_mapping()
                .unwrap()
                .contains_key(Value::from("json")),
            has_json
        );
        assert_eq!(vars["saved"]["body"], body);
    }
}

#[tokio::test]
async fn condition_errors_do_not_print_payload_or_expression_secrets() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_string("secret-body"))
        .expect(2)
        .mount(&server)
        .await;
    for expression in [
        "(gt response.body 0)",
        "(unknown_helper \"secret-literal\")",
    ] {
        let error = execute(
            HttpTask {
                url: server.uri(),
                changed_when: Some(expression.into()),
                ..Default::default()
            },
            handle(),
        )
        .await
        .unwrap_err();
        assert_eq!(
            error.msg.as_deref(),
            Some("http: invalid changed_when condition")
        );
        assert!(!format!("{error:?}").contains("secret"));
    }
}

#[test]
fn json_path_validation_uses_parsed_paths_and_checks_bracket_and_root_forms() {
    for field in [
        "response.json.changed",
        "response.[json].changed",
        "response/json/changed",
        "this.response.json.changed",
        "@root.response.json.changed",
    ] {
        assert!(
            response_json_path(&handlebars::Path::parse(field).unwrap()),
            "{field}"
        );
    }
    for field in ["response.body", "response.headers", "other.response.json"] {
        assert!(
            !response_json_path(&handlebars::Path::parse(field).unwrap()),
            "{field}"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn parallel_hosts_and_overlapping_items_have_invocation_local_response_and_variables() {
    let server = MockServer::start().await;
    for id in 0..32 {
        Mock::given(method("GET"))
            .and(path(format!("/{id}")))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"id": id, "changed": id % 2 == 0}))
                    .set_delay(Duration::from_millis((31 - id) * 2)),
            )
            .expect(1)
            .mount(&server)
            .await;
    }
    let shared_host = handle();
    let mut jobs = Vec::new();
    // Half of the invocations overlap on one host (simulated item parallelism);
    // the other half use independent hosts. Evaluate while each item is active,
    // then dispatch after all mutable host item variables have moved on.
    for id in 0..32 {
        let host = if id < 16 {
            shared_host.clone()
        } else {
            handle()
        };
        host.host
            .write()
            .unwrap()
            .update_variables(serde_yaml::from_str(&format!("item: {id}\n")).unwrap());
        let task = HttpTask {
            url: format!("{}/{{{{ item }}}}", server.uri()),
            changed_when: Some("(and (eq response.json.id item) response.json.changed)".into()),
            ..Default::default()
        };
        let query = query_request();
        let evaluated = task.evaluate(&host, &query, TemplateMode::Strict).unwrap();
        jobs.push((id, host, query, evaluated));
    }
    let workers: Vec<_> = jobs
        .into_iter()
        .map(|(id, host, query, evaluated)| {
            tokio::task::spawn_blocking(move || {
                let result = evaluated
                    .action
                    .dispatch(
                        &host,
                        &TaskRequest::execute(query.sudo_details.as_ref().unwrap()),
                    )
                    .unwrap();
                assert_eq!(
                    result.status,
                    if id % 2 == 0 {
                        TaskStatus::IsExecuted
                    } else {
                        TaskStatus::IsPassive
                    },
                    "item {id}"
                );
                assert!(
                    !host
                        .host
                        .read()
                        .unwrap()
                        .get_variables()
                        .contains_key(Value::from("response"))
                );
            })
        })
        .collect();
    for worker in workers {
        worker.await.unwrap();
    }
}
