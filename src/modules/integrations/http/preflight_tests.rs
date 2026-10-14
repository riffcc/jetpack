// Copyright (C) Riff Labs <team@riff.cc>
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use crate::playbooks::language::Play;
use crate::playbooks::task_fsm::async_run_single_task;
use crate::playbooks::visitor::CheckMode;
use crate::registry::list::Task;
use crate::test_support::{RecordingConnection, test_handle};
use std::sync::Mutex;
use wiremock::matchers::{body_string, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn handle() -> Arc<TaskHandle> {
    test_handle(Arc::new(Mutex::new(RecordingConnection::new())))
}

async fn run_through_engine(
    task: HttpTask,
    handle: Arc<TaskHandle>,
    plan: bool,
) -> Result<Arc<TaskResponse>, Arc<TaskResponse>> {
    tokio::task::spawn_blocking(move || {
        // This is the same check-mode flag used by both plan and check-local.
        handle.run_state.visitor.write().unwrap().check_mode =
            if plan { CheckMode::Yes } else { CheckMode::No };
        let play: Play = serde_yaml::from_str("name: HTTP preflight\ngroups: [all]\n").unwrap();
        let connection: Arc<Mutex<dyn crate::connection::connection::Connection>> =
            Arc::new(Mutex::new(RecordingConnection::new()));
        async_run_single_task(
            &handle.run_state,
            &connection,
            &handle.host,
            &play,
            &Task::Http(task),
        )
    })
    .await
    .unwrap()
}

fn task(base: &str, matches: &str) -> HttpTask {
    HttpTask {
        url: format!("{base}/resource"),
        method: Some("POST".into()),
        save: Some("saved".into()),
        check: Some(HttpCheck {
            url: format!("{base}/resource"),
            matches: matches.into(),
            ..Default::default()
        }),
        ..Default::default()
    }
}

#[tokio::test]
async fn plan_apply_matrix_runs_mutations_only_when_needed_and_allowed() {
    for (plan, configured, expected, mutation_count) in [
        (true, true, TaskStatus::IsMatched, 0),
        (true, false, TaskStatus::NeedsExecution, 0),
        (false, true, TaskStatus::IsMatched, 0),
        (false, false, TaskStatus::IsExecuted, 1),
    ] {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/resource"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"configured": configured, "changed": false})),
            )
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/resource"))
            .respond_with(
                ResponseTemplate::new(201).set_body_json(serde_json::json!({"changed": true})),
            )
            .expect(mutation_count)
            .mount(&server)
            .await;
        let handle = handle();
        let mut task = task(&server.uri(), "(eq response.json.configured true)");
        task.changed_when = Some("(eq response.json.changed true)".into());
        task.and = Some(serde_yaml::from_str("notify: api_changed\n").unwrap());
        let response = run_through_engine(task, handle.clone(), plan)
            .await
            .unwrap();
        assert_eq!(
            response.status, expected,
            "plan={plan}, configured={configured}"
        );
        let vars = Value::Mapping(handle.host.read().unwrap().get_variables());
        assert_eq!(
            vars.as_mapping()
                .unwrap()
                .contains_key(Value::from("saved")),
            mutation_count != 0
        );
        assert!(
            !vars
                .as_mapping()
                .unwrap()
                .contains_key(Value::from("response"))
        );
        if mutation_count != 0 {
            assert_eq!(vars["saved"]["status"], 201);
        }
        let play_count = handle.run_state.context.read().unwrap().play_count;
        assert_eq!(
            handle
                .host
                .read()
                .unwrap()
                .is_notified(play_count, "api_changed"),
            mutation_count != 0
        );
    }
}

#[tokio::test]
async fn expected_empty_404_establishes_absence_and_never_mutates_in_plan() {
    for plan in [true, false] {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(404))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(204))
            .expect(if plan { 0 } else { 1 })
            .mount(&server)
            .await;
        let mut task = task(
            &server.uri(),
            "(and (eq response.status 200) (eq response.json.authoritative true))",
        );
        task.check.as_mut().unwrap().expected_status = Some(vec!["200".into(), "404".into()]);
        let response = run_through_engine(task, handle(), plan).await.unwrap();
        assert_eq!(
            response.status,
            if plan {
                TaskStatus::NeedsExecution
            } else {
                TaskStatus::IsExecuted
            }
        );
    }
}

#[tokio::test]
async fn mutation_change_condition_can_report_no_change_after_mismatching_check() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_string("{}"))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({"changed": false})),
        )
        .expect(1)
        .mount(&server)
        .await;
    let handle = handle();
    let mut task = task(&server.uri(), "false");
    task.changed_when = Some("(eq response.json.changed true)".into());
    task.and = Some(serde_yaml::from_str("notify: api_changed\n").unwrap());
    let response = run_through_engine(task, handle.clone(), false)
        .await
        .unwrap();
    assert_eq!(response.status, TaskStatus::IsPassive);
    let play_count = handle.run_state.context.read().unwrap().play_count;
    assert!(
        !handle
            .host
            .read()
            .unwrap()
            .is_notified(play_count, "api_changed")
    );
}

#[tokio::test]
async fn check_status_condition_and_json_errors_fail_in_plan_and_apply_without_mutation() {
    for plan in [true, false] {
        for (status, body, matches, expected_status, error) in [
            (
                500,
                "secret-response",
                "(eq response.json.configured true)",
                None,
                "http: check unexpected status 500",
            ),
            (
                200,
                "secret-invalid-json",
                "(eq response.json.configured true)",
                None,
                "http: check.matches response (status 200) is not valid JSON",
            ),
            (
                404,
                "",
                "(eq response.json.configured true)",
                Some(vec!["404".into()]),
                "http: check.matches response (status 404) has no JSON body",
            ),
            (
                200,
                "{}",
                "(unknown_helper \"secret-expression\")",
                None,
                "http: invalid check.matches condition",
            ),
        ] {
            let server = MockServer::start().await;
            Mock::given(method("GET"))
                .respond_with(ResponseTemplate::new(status).set_body_string(body))
                .expect(1)
                .mount(&server)
                .await;
            Mock::given(method("POST"))
                .respond_with(ResponseTemplate::new(200))
                .expect(0)
                .mount(&server)
                .await;
            let mut task = task(&server.uri(), matches);
            task.check.as_mut().unwrap().expected_status = expected_status;
            let response = run_through_engine(task, handle(), plan).await.unwrap_err();
            assert_eq!(response.status, TaskStatus::Failed);
            assert_eq!(response.msg.as_deref(), Some(error));
            assert!(!format!("{response:?}").contains("secret"));
        }
    }
}

#[tokio::test]
async fn unreachable_and_timed_out_checks_fail_without_mutation() {
    for plan in [true, false] {
        let mutation = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&mutation)
            .await;
        let mut task = task(&mutation.uri(), "false");
        task.check.as_mut().unwrap().url = "http://127.0.0.1:1?token=secret".into();
        let response = run_through_engine(task, handle(), plan).await.unwrap_err();
        assert_eq!(
            response.msg.as_deref(),
            Some("http: check: connection failed")
        );
        let check = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(3)))
            .mount(&check)
            .await;
        let mut task = self::task(&mutation.uri(), "false");
        task.check.as_mut().unwrap().url = check.uri();
        task.check.as_mut().unwrap().timeout = Some("1".into());
        let response = run_through_engine(task, handle(), plan).await.unwrap_err();
        assert_eq!(
            response.msg.as_deref(),
            Some("http: check: request timed out")
        );
    }
}

#[tokio::test]
async fn check_settings_are_independent_and_template_with_host_variables() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/check"))
        .and(header("Authorization", "Bearer check-token"))
        .and(body_string("read payload"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(serde_json::json!({"configured": false})),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/mutation"))
        .and(header("Authorization", "Bearer mutation-token"))
        .and(body_string("mutation payload"))
        .respond_with(ResponseTemplate::new(201))
        .expect(1)
        .mount(&server)
        .await;
    let yaml = r#"!http
url: "{{ endpoint }}/mutation"
method: POST
headers:
  Authorization: "Bearer {{ mutation_token }}"
body: mutation payload
check:
  url: "{{ endpoint }}/check"
  method: "{{ read_method }}"
  headers:
    Authorization: "Bearer {{ check_token }}"
  body: read payload
  timeout: 5
  verify_tls: true
  matches: "(eq response.json.configured true)"
"#;
    let Task::Http(task) = serde_yaml::from_str(yaml).unwrap() else {
        panic!("HTTP task expected")
    };
    let handle = handle();
    handle.host.write().unwrap().update_variables(serde_yaml::from_str(&format!(
        "endpoint: {}\nmutation_token: mutation-token\ncheck_token: check-token\nread_method: get\n", server.uri()
    )).unwrap());
    assert_eq!(
        run_through_engine(task, handle, false)
            .await
            .unwrap()
            .status,
        TaskStatus::IsExecuted
    );
}

#[tokio::test]
async fn head_checks_are_supported_and_mutating_check_methods_are_rejected_before_io() {
    let server = MockServer::start().await;
    Mock::given(method("HEAD"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&server)
        .await;
    let mut task = task(&server.uri(), "(eq response.status 204)");
    task.check.as_mut().unwrap().method = Some("HEAD".into());
    assert_eq!(
        run_through_engine(task, handle(), true)
            .await
            .unwrap()
            .status,
        TaskStatus::IsMatched
    );
    for method in ["POST", "PUT", "PATCH", "DELETE", "invalid secret"] {
        let mut task = self::task(&server.uri(), "false");
        task.check.as_mut().unwrap().method = Some(method.into());
        let error = run_through_engine(task, handle(), true).await.unwrap_err();
        assert_eq!(
            error.msg.as_deref(),
            Some("http: check method must be GET or HEAD")
        );
    }
}

#[tokio::test]
async fn item_iterations_check_their_own_resource_and_skip_only_matching_items() {
    let server = MockServer::start().await;
    for (item, configured) in [("one", true), ("two", false)] {
        Mock::given(method("GET"))
            .and(path(format!("/{item}")))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"item": item, "configured": configured})),
            )
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path(format!("/{item}")))
            .and(body_string(item))
            .respond_with(
                ResponseTemplate::new(201)
                    .set_body_json(serde_json::json!({"item": item, "changed": true})),
            )
            .expect(if configured { 0 } else { 1 })
            .mount(&server)
            .await;
    }
    let mut task = task(
        &server.uri(),
        "(and (eq response.json.item item) response.json.configured)",
    );
    task.url = format!("{}/{{{{ item }}}}", server.uri());
    task.body = Some("{{ item }}".into());
    task.check.as_mut().unwrap().url = task.url.clone();
    task.changed_when = Some("(and (eq response.json.item item) response.json.changed)".into());
    task.with = Some(serde_yaml::from_str("items: [one, two]\n").unwrap());
    let handle = handle();
    assert_eq!(
        run_through_engine(task, handle.clone(), false)
            .await
            .unwrap()
            .status,
        TaskStatus::IsExecuted
    );
    assert_eq!(
        Value::Mapping(handle.host.read().unwrap().get_variables())["saved"]["json"]["item"],
        "two"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn parallel_hosts_do_not_share_preflight_or_mutation_responses() {
    let server = MockServer::start().await;
    let mut workers = Vec::new();
    for id in 0..16 {
        let configured = id % 2 == 0;
        let changed = id % 3 == 0;
        Mock::given(method("GET"))
            .and(path(format!("/{id}")))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"id": id, "configured": configured}))
                    .set_delay(Duration::from_millis((15 - id) * 2)),
            )
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path(format!("/{id}")))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"id": id, "changed": changed})),
            )
            .expect(if configured { 0 } else { 1 })
            .mount(&server)
            .await;
        let handle = handle();
        handle
            .host
            .write()
            .unwrap()
            .update_variables(serde_yaml::from_str(&format!("id: {id}\n")).unwrap());
        let mut task = task(
            &server.uri(),
            "(and (eq response.json.id id) response.json.configured)",
        );
        task.url = format!("{}/{{{{ id }}}}", server.uri());
        task.check.as_mut().unwrap().url = task.url.clone();
        task.changed_when = Some("(and (eq response.json.id id) response.json.changed)".into());
        workers.push(tokio::spawn(async move {
            let result = run_through_engine(task, handle, false).await.unwrap();
            assert_eq!(
                result.status,
                if configured {
                    TaskStatus::IsMatched
                } else if changed {
                    TaskStatus::IsExecuted
                } else {
                    TaskStatus::IsPassive
                },
                "host {id}"
            );
        }));
    }
    for worker in workers {
        worker.await.unwrap();
    }
}

#[tokio::test]
async fn skipped_tasks_do_not_evaluate_missing_check_templates_or_send_requests() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&server)
        .await;
    let mut task = task(&server.uri(), "(eq response.json.configured true)");
    task.check.as_mut().unwrap().url = "{{ missing_endpoint }}".into();
    task.with = Some(serde_yaml::from_str("condition: false\n").unwrap());
    assert_eq!(
        run_through_engine(task, handle(), false)
            .await
            .unwrap()
            .status,
        TaskStatus::IsSkipped
    );
}

#[test]
fn check_requires_a_condition_and_rejects_unknown_fields_without_debug_leaks() {
    for yaml in [
        "!http\nurl: http://localhost\ncheck:\n  url: http://localhost\n",
        "!http\nurl: http://localhost\ncheck:\n  url: http://localhost\n  matches: 'true'\n  until: secret\n",
    ] {
        assert!(serde_yaml::from_str::<Task>(yaml).is_err());
    }
    let check: HttpCheck = serde_yaml::from_str("url: http://localhost?token=secret\nheaders:\n  X-Custom: secret\nmatches: '(eq response.body \"secret\")'\n").unwrap();
    assert!(!format!("{check:?}").contains("secret"));
}

#[tokio::test]
async fn indirect_json_access_is_validated_only_when_its_branch_is_reached() {
    for expression in [
        "(eq (lookup response \"json\") true)",
        "(eq (lookup response field_name) true)",
        "(eq (lookup (lookup response \"json\") \"configured\") true)",
    ] {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_string("secret-invalid-json"))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&server)
            .await;
        let handle = handle();
        handle
            .host
            .write()
            .unwrap()
            .update_variables(serde_yaml::from_str("field_name: json\n").unwrap());
        let error = run_through_engine(task(&server.uri(), expression), handle, true)
            .await
            .unwrap_err();
        assert_eq!(
            error.msg.as_deref(),
            Some("http: check.matches response (status 200) is not valid JSON")
        );
    }
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(404))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200))
        .expect(0)
        .mount(&server)
        .await;
    let mut task = task(
        &server.uri(),
        "(and (eq response.status 200) (eq (lookup response \"json\") true))",
    );
    task.check.as_mut().unwrap().expected_status = Some(vec!["404".into()]);
    assert_eq!(
        run_through_engine(task, handle(), true)
            .await
            .unwrap()
            .status,
        TaskStatus::NeedsExecution
    );
}
