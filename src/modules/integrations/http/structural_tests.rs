// Copyright (C) Riff Labs <team@riff.cc>
// SPDX-License-Identifier: GPL-3.0-or-later

use super::preflight_tests::{handle, run_through_engine};
use super::*;
use crate::registry::list::Task;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn task(base: &str, matcher: serde_json::Value) -> HttpTask {
    HttpTask {
        url: format!("{base}/resource"),
        method: Some("POST".into()),
        check: Some(HttpCheck {
            url: format!("{base}/resource"),
            matches: serde_json::from_value(matcher).unwrap(),
            ..Default::default()
        }),
        ..Default::default()
    }
}

#[tokio::test]
async fn typed_dns_shape_matches_metadata_without_mutating_in_plan_or_apply() {
    for plan in [true, false] {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "zones": [{"name":"example.com", "authoritative":true, "defaultTTL":3600,
                    "handlerConfigs":[{"type":"memory", "id":1},{"type":"etcd", "id":2}],
                    "hook":"", "id":"server-generated"}], "metadata":{"page":1}
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(201))
            .expect(0)
            .mount(&server)
            .await;
        let mut task = task(
            &server.uri(),
            json!({"path":"response.json.zones", "contains_exactly":[{
                "name":"{{ item }}", "authoritative":true, "defaultTTL":"{{ gravity_dns_ttl }}",
                "handlerConfigs":"{{ gravity_authoritative_handlers }}", "hook":"{{ gravity_zone_hook }}"
            }]}),
        );
        task.with = Some(serde_yaml::from_str("items: [example.com]").unwrap());
        let handle = handle();
        handle.host.write().unwrap().update_variables(serde_yaml::from_str("item: example.com\ngravity_dns_ttl: 3600\ngravity_authoritative_handlers: [{type: memory}, {type: etcd}]\ngravity_zone_hook: ''\nresponse: {json: {zones: []}}\n").unwrap());
        assert_eq!(
            run_through_engine(task, handle.clone(), plan)
                .await
                .unwrap()
                .status,
            TaskStatus::IsMatched
        );
        // Temporary response must not replace the user's existing host variable.
        assert_eq!(
            Value::Mapping(handle.host.read().unwrap().get_variables())["response"]["json"]["zones"],
            Value::Sequence(vec![])
        );
    }
}

#[tokio::test]
async fn contains_accepts_extra_unordered_resources_while_exact_detects_duplicates() {
    for operator in ["contains", "contains_exactly"] {
        for actual in [
            json!([{"name":"other.example"},{"name":"example.com", "id":1}]),
            json!([{"name":"example.com"},{"name":"example.com"}]),
        ] {
            for plan in [true, false] {
                let matches = operator == "contains";
                let server = MockServer::start().await;
                Mock::given(method("GET"))
                    .respond_with(ResponseTemplate::new(200).set_body_json(json!({"zones":actual})))
                    .expect(1)
                    .mount(&server)
                    .await;
                Mock::given(method("POST"))
                    .respond_with(ResponseTemplate::new(201))
                    .expect(if !plan && !matches { 1 } else { 0 })
                    .mount(&server)
                    .await;
                let task = task(
                    &server.uri(),
                    json!({"path":"response.json.zones", operator:[{"name":"example.com"}]}),
                );
                let expected = if matches {
                    TaskStatus::IsMatched
                } else if plan {
                    TaskStatus::NeedsExecution
                } else {
                    TaskStatus::IsExecuted
                };
                assert_eq!(
                    run_through_engine(task, handle(), plan)
                        .await
                        .unwrap()
                        .status,
                    expected
                );
            }
        }
    }
}

#[tokio::test]
async fn structural_json_checks_handle_absence_and_validate_status_before_matching() {
    for plan in [true, false] {
        for (status, body, accepted, expected_error) in [
            (404, "", true, None),
            (404, "secret-invalid-json", true, None),
            (404, "", false, Some("http: check unexpected status 404")),
            (
                204,
                "",
                true,
                Some("http: check.matches response (status 204) has no JSON body"),
            ),
            (
                200,
                "secret-invalid-json",
                true,
                Some("http: check.matches response (status 200) is not valid JSON"),
            ),
            (
                500,
                "secret-invalid-json",
                false,
                Some("http: check unexpected status 500"),
            ),
        ] {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .respond_with(ResponseTemplate::new(status).set_body_string(body))
                .expect(1)
                .mount(&server)
                .await;
            let mutation_count = if expected_error.is_none() && !plan {
                1
            } else {
                0
            };
            Mock::given(method("POST"))
                .respond_with(ResponseTemplate::new(201))
                .expect(mutation_count)
                .mount(&server)
                .await;
            let mut task = task(
                &server.uri(),
                json!({"path":"response.json.zones", "contains_exactly":[{"name":"example.com"}]}),
            );
            if accepted {
                task.check.as_mut().unwrap().expected_status = Some(vec![status.to_string()]);
            }
            let result = run_through_engine(task, handle(), plan).await;
            if let Some(error) = expected_error {
                let response = result.unwrap_err();
                assert_eq!(response.msg.as_deref(), Some(error));
                assert!(!format!("{response:?}").contains("secret"));
            } else {
                assert_eq!(
                    result.unwrap().status,
                    if plan {
                        TaskStatus::NeedsExecution
                    } else {
                        TaskStatus::IsExecuted
                    }
                );
            }
        }
    }
}

#[tokio::test]
async fn missing_is_a_mismatch_and_explicit_null_is_a_match() {
    for (body, selector, expected, matches) in [
        (
            json!({"hook":null}),
            "response.json.hook",
            json!(null),
            true,
        ),
        (json!({}), "response.json.hook", json!(null), false),
        (json!(null), "response.json", json!(null), true),
        (
            json!({"ttl":"3600"}),
            "response.json.ttl",
            json!(3600),
            false,
        ),
        (
            json!({"authoritative":true}),
            "response.json.authoritative",
            json!(true),
            true,
        ),
    ] {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(201))
            .expect(0)
            .mount(&server)
            .await;
        let task = task(
            &server.uri(),
            json!({"path":selector, "contains_exactly":expected}),
        );
        assert_eq!(
            run_through_engine(task, handle(), true)
                .await
                .unwrap()
                .status,
            if matches {
                TaskStatus::IsMatched
            } else {
                TaskStatus::NeedsExecution
            }
        );
    }
}

#[test]
fn yaml_and_json_inputs_share_explicit_structural_syntax_and_reject_ambiguous_maps() {
    for document in [
        "!http\nurl: https://example.com\ncheck:\n  url: https://example.com\n  matches:\n    path: response.json.zones\n    contains_exactly: [{name: '{{ item }}'}]\n",
        "!http\nurl: https://example.com\ncheck:\n  url: https://example.com\n  matches: {\"path\":\"response.json.zones\",\"contains_exactly\":[{\"name\":\"{{ item }}\"}]}\n",
    ] {
        let Task::Http(task) = serde_yaml::from_str(document).unwrap() else {
            panic!("HTTP expected");
        };
        assert!(matches!(
            task.check.unwrap().matches,
            HttpMatch::Structural(_)
        ));
    }
    for matcher in [
        json!({"path":"response.json"}),
        json!({"contains":{}}),
        json!({"path":"response.json", "contains":{},"contains_exactly":{}}),
        json!({"path":"response.json","equals":{}}),
        json!(true),
        json!([]),
        json!(null),
    ] {
        assert!(serde_json::from_value::<HttpMatch>(matcher).is_err());
    }
    assert!(serde_yaml::from_str::<Task>("!http\nurl: https://example.com\nchanged_when: {path: response.status, contains: 201}\n").is_err());
}

#[tokio::test]
async fn bad_paths_and_expected_templates_fail_redacted_but_skipped_tasks_defer_them() {
    for matcher in [
        json!({"path":"response.json secret", "contains":{}}),
        json!({"path":"response.json", "contains":{"token":"secret-{{ missing }}"}}),
    ] {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(201))
            .expect(0)
            .mount(&server)
            .await;
        let task = task(&server.uri(), matcher.clone());
        assert!(!format!("{task:?}").contains("secret"));
        let response = run_through_engine(task, handle(), true).await.unwrap_err();
        assert!(
            response
                .msg
                .as_deref()
                .unwrap()
                .starts_with("http: invalid check.matches")
        );
        assert!(!format!("{response:?}").contains("secret"));
        let mut task = self::task(&server.uri(), matcher);
        task.with = Some(serde_yaml::from_str("condition: false").unwrap());
        assert_eq!(
            run_through_engine(task, handle(), false)
                .await
                .unwrap()
                .status,
            TaskStatus::IsSkipped
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn parallel_hosts_and_item_loops_keep_typed_expectations_invocation_scoped() {
    let server = MockServer::start().await;
    let mut workers = Vec::new();
    for id in 0..12 {
        for item in ["one", "two"] {
            let matched = id % 2 == 0 || item == "one";
            Mock::given(method("GET")).and(path(format!("/{id}/{item}"))).respond_with(
                ResponseTemplate::new(200).set_body_json(json!({"zones":[{"name":item, "id":if matched { id } else { 999 }, "handlers":[{"type":"memory", "metadata":id}]}]})).set_delay(Duration::from_millis((12-id)*2))
            ).expect(1).mount(&server).await;
            Mock::given(method("POST"))
                .and(path(format!("/{id}/{item}")))
                .respond_with(ResponseTemplate::new(201))
                .expect(if matched { 0 } else { 1 })
                .mount(&server)
                .await;
        }
        let mut task = task(
            &server.uri(),
            json!({"path":"response.json.zones", "contains_exactly":[{"name":"{{ item }}", "id":"{{ id }}", "handlers":"{{ handlers }}"}]}),
        );
        task.url = format!("{}/{{{{ id }}}}/{{{{ item }}}}", server.uri());
        task.check.as_mut().unwrap().url = task.url.clone();
        task.with = Some(serde_yaml::from_str("items: [one, two]").unwrap());
        let handle = handle();
        handle.host.write().unwrap().update_variables(
            serde_yaml::from_str(&format!("id: {id}\nhandlers: [{{type: memory}}]\n")).unwrap(),
        );
        workers.push(tokio::spawn(async move {
            let result = run_through_engine(task, handle, false).await.unwrap();
            assert_eq!(
                result.status,
                if id % 2 == 0 {
                    TaskStatus::IsMatched
                } else {
                    TaskStatus::IsExecuted
                }
            );
        }));
    }
    for worker in workers {
        worker.await.unwrap();
    }
}

#[tokio::test]
async fn json_path_aliases_keep_validation_and_non_json_paths_need_no_body() {
    for selector in [
        "response/json/configured",
        "this.response.json.configured",
        "@root.response.json.configured",
        "response.[json].[configured]",
    ] {
        for valid in [true, false] {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .respond_with(ResponseTemplate::new(200).set_body_string(if valid {
                    r#"{"configured":true}"#
                } else {
                    "secret-invalid-json"
                }))
                .expect(1)
                .mount(&server)
                .await;
            let task = task(
                &server.uri(),
                json!({"path":selector,"contains_exactly":true}),
            );
            let result = run_through_engine(task, handle(), true).await;
            if valid {
                assert_eq!(result.unwrap().status, TaskStatus::IsMatched);
            } else {
                assert_eq!(
                    result.unwrap_err().msg.as_deref(),
                    Some("http: check.matches response (status 200) is not valid JSON")
                );
            }
        }
    }
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;
    let task = task(
        &server.uri(),
        json!({"path":"response.status", "contains_exactly":204}),
    );
    assert_eq!(
        run_through_engine(task, handle(), true)
            .await
            .unwrap()
            .status,
        TaskStatus::IsMatched
    );
}
