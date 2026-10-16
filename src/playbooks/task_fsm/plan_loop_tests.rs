// Copyright (C) Riff Labs <team@riff.cc>
// SPDX-License-Identifier: GPL-3.0-or-later
use super::*;
use crate::connection::connection::Connection;
use crate::playbooks::visitor::CheckMode;
use crate::test_support::{RecordingConnection, test_handle};

type Fixture = (
    Arc<TaskHandle>,
    Arc<Mutex<dyn Connection>>,
    Arc<Mutex<Vec<String>>>,
);
fn fixture() -> Fixture {
    let recording = RecordingConnection::with_result(|command| {
        (
            if command.contains("fail-now") { 1 } else { 0 },
            if command.ends_with(" changed") || command == "changed" {
                "changed"
            } else {
                "enabled"
            }
            .into(),
        )
    });
    let log = recording.command_log();
    let connection: Arc<Mutex<dyn Connection>> = Arc::new(Mutex::new(recording));
    (test_handle(connection.clone()), connection, log)
}
fn run(
    handle: &Arc<TaskHandle>,
    connection: &Arc<Mutex<dyn Connection>>,
    yaml: &str,
    plan: bool,
) -> Result<Arc<TaskResponse>, Arc<TaskResponse>> {
    handle.run_state.visitor.write().unwrap().check_mode =
        if plan { CheckMode::Yes } else { CheckMode::No };
    let play = serde_yaml::from_str("name: plan and loop regression\ngroups: [all]").unwrap();
    let task = serde_yaml::from_str(yaml).unwrap();
    async_run_single_task(&handle.run_state, connection, &handle.host, &play, &task)
}
const PRODUCER: &str =
    "!command\ncmd: /usr/bin/printf enabled\nsave: current_setting\nchanged_when: false";
const DEPENDENT: &str = "!command\ncmd: /usr/bin/true\nwith:\n  condition: '(not (contains current_setting.out \"enabled\"))'";

#[test]
fn skipped_saved_command_defers_dependent_plan_without_io_or_fabricated_output() {
    let (handle, connection, log) = fixture();
    assert_eq!(
        run(&handle, &connection, PRODUCER, true).unwrap().status,
        TaskStatus::NeedsExecution
    );
    let response = run(&handle, &connection, DEPENDENT, true).unwrap();
    assert_eq!(response.status, TaskStatus::IsDeferred);
    assert!(response.deferred);
    assert!(log.lock().unwrap().is_empty());
    assert!(
        !handle
            .host
            .read()
            .unwrap()
            .variables
            .contains_key(serde_yaml::Value::String("current_setting".into()))
    );
    let context = handle.run_state.context.read().unwrap();
    assert_eq!(context.get_total_deferred_count(), 1);
    assert_eq!(context.get_total_matched_count(), 0);
    assert_eq!(context.get_total_failed_count(), 0);
}
#[test]
fn unavailable_is_not_empty_or_stale_and_real_plan_values_clear_the_dependency() {
    let (handle, connection, log) = fixture();
    handle
        .host
        .write()
        .unwrap()
        .update_variables(serde_yaml::from_str("current_setting: {out: enabled}").unwrap());
    run(&handle, &connection, PRODUCER, true).unwrap();
    assert_eq!(
        run(&handle, &connection, DEPENDENT, true).unwrap().status,
        TaskStatus::IsDeferred
    );
    assert_eq!(
        run(
            &handle,
            &connection,
            "!set\nvars:\n  current_setting: {out: ''}",
            true
        )
        .unwrap()
        .status,
        TaskStatus::IsPassive
    );
    assert_eq!(
        run(&handle, &connection, DEPENDENT, true).unwrap().status,
        TaskStatus::NeedsExecution
    );
    assert!(log.lock().unwrap().is_empty());
    run(&handle, &connection, PRODUCER, false).unwrap();
    assert_eq!(
        run(&handle, &connection, DEPENDENT, false).unwrap().status,
        TaskStatus::IsSkipped
    );
    assert_eq!(log.lock().unwrap().len(), 1);
}
#[test]
fn lazy_conditions_skip_unknown_operands_but_reached_unknown_paths_defer() {
    let (handle, connection, _) = fixture();
    run(&handle, &connection, PRODUCER, true).unwrap();
    for (condition, status) in [
        (
            "(and false (eq current_setting.out \"\"))",
            TaskStatus::IsSkipped,
        ),
        (
            "(or true (eq current_setting.out \"\"))",
            TaskStatus::NeedsExecution,
        ),
        ("(isdefined current_setting)", TaskStatus::IsDeferred),
        (
            "(eq @root.current_setting.out \"\")",
            TaskStatus::IsDeferred,
        ),
        (
            "(eq (lookup current_setting \"out\") \"\")",
            TaskStatus::IsDeferred,
        ),
    ] {
        let task = format!("!command\ncmd: /usr/bin/true\nwith:\n  condition: '{condition}'");
        assert_eq!(
            run(&handle, &connection, &task, true).unwrap().status,
            status,
            "{condition}"
        );
    }
    assert_eq!(
        run(
            &handle,
            &connection,
            "!command\ncmd: true\nwith:\n  condition: '(eq genuine_typo.out 1)'",
            true
        )
        .unwrap_err()
        .status,
        TaskStatus::Failed
    );
    assert_eq!(
        run(
            &handle,
            &connection,
            "!command\ncmd: true\nwith:\n  condition: '(and current_setting.out'",
            true
        )
        .unwrap_err()
        .status,
        TaskStatus::Failed
    );
}
#[test]
fn unknown_values_propagate_through_parameters_aliases_sets_and_item_sources() {
    let (handle, connection, log) = fixture();
    run(&handle, &connection, PRODUCER, true).unwrap();
    for task in [
        "!command\ncmd: '{{ current_setting.out }}'\nsave: indirect",
        "!command\ncmd: true\nwith:\n  condition: '(eq indirect.out \"x\")'",
        "!set\nvars:\n  derived: '{{ current_setting.out }}'",
        "!command\ncmd: true\nwith:\n  condition: '(eq derived \"x\")'",
        "!command\ncmd: true\nwith:\n  items: current_setting",
    ] {
        assert_eq!(
            run(&handle, &connection, task, true).unwrap().status,
            TaskStatus::IsDeferred,
            "{task}"
        );
    }
    handle
        .host
        .write()
        .unwrap()
        .update_variables(serde_yaml::from_str("alias: '{{ current_setting.out }}'").unwrap());
    assert_eq!(
        run(&handle, &connection, "!command\ncmd: '{{ alias }}'", true)
            .unwrap()
            .status,
        TaskStatus::IsDeferred
    );
    assert!(log.lock().unwrap().is_empty());
}
#[test]
fn dynamic_save_names_and_parallel_hosts_keep_unknown_outputs_scoped() {
    let workers: Vec<_> = (0..8)
        .map(|id| {
            std::thread::spawn(move || {
                let (handle, connection, log) = fixture();
                handle.host.write().unwrap().update_variables(
                    serde_yaml::from_str(&format!("key: result_{id}\nforeign: {{out: enabled}}"))
                        .unwrap(),
                );
                run(
                    &handle,
                    &connection,
                    "!shell\ncmd: printf enabled\nsave: '{{ key }}'\nchanged_when: false",
                    true,
                )
                .unwrap();
                let condition = format!(
                    "!command\ncmd: true\nwith:\n  condition: '(eq result_{id}.out \"enabled\")'"
                );
                assert_eq!(
                    run(&handle, &connection, &condition, true).unwrap().status,
                    TaskStatus::IsDeferred
                );
                assert_eq!(
                    run(
                        &handle,
                        &connection,
                        "!command\ncmd: true\nwith:\n  condition: '(eq foreign.out \"disabled\")'",
                        true
                    )
                    .unwrap()
                    .status,
                    TaskStatus::IsSkipped
                );
                assert!(log.lock().unwrap().is_empty());
                assert_eq!(handle.host.read().unwrap().plan_unavailable.len(), 1);
            })
        })
        .collect();
    for worker in workers {
        worker.join().unwrap();
    }
}
#[test]
fn apply_loops_keep_earlier_changes_and_notify_independently_of_order() {
    for items in ["[changed, unchanged]", "[unchanged, changed]"] {
        let (handle, connection, log) = fixture();
        let task = format!(
            "!command\ncmd: echo {{{{ item }}}}\nchanged_when: '(eq out \"changed\")'\nwith:\n  items: {items}\nand:\n  notify: reload"
        );
        assert_eq!(
            run(&handle, &connection, &task, false).unwrap().status,
            TaskStatus::IsExecuted
        );
        assert_eq!(log.lock().unwrap().len(), 2);
        assert!(handle.host.read().unwrap().is_notified(0, "reload"));
        let context = handle.run_state.context.read().unwrap();
        assert_eq!(context.get_total_executions_count(), 1);
        assert_eq!(context.get_total_passive_count(), 0);
    }
}
#[test]
fn changed_then_skipped_is_changed_all_skipped_is_skipped_and_failures_stop() {
    for (items, expected) in [
        ("[changed, skipped]", TaskStatus::IsExecuted),
        ("[skipped, changed]", TaskStatus::IsExecuted),
        ("[skipped, skipped]", TaskStatus::IsSkipped),
    ] {
        let recording = RecordingConnection::with_rc(|cmd| {
            if cmd.starts_with("test -e") && !cmd.contains("skipped") {
                1
            } else {
                0
            }
        });
        let connection: Arc<Mutex<dyn Connection>> = Arc::new(Mutex::new(recording));
        let handle = test_handle(connection.clone());
        let task = format!(
            "!command\ncmd: echo {{{{ item }}}}\nwith:\n  items: {items}\n  skip_if_exists: '/target/{{{{ item }}}}'"
        );
        assert_eq!(
            run(&handle, &connection, &task, false).unwrap().status,
            expected
        );
    }
    let (handle, connection, log) = fixture();
    assert!(
        run(
            &handle,
            &connection,
            "!command\ncmd: '{{ item }}'\nwith:\n  items: [fail-now, later]",
            false
        )
        .is_err()
    );
    assert_eq!(*log.lock().unwrap(), vec!["fail-now"]);
}

#[test]
fn http_loop_plan_apply_reapply_aggregates_missing_and_existing_in_both_orders() {
    use std::collections::HashSet;
    use wiremock::matchers::method;
    use wiremock::{Mock, MockServer, ResponseTemplate};
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    for items in ["[missing, existing]", "[existing, missing]"] {
        let server = runtime.block_on(MockServer::start());
        let state = Arc::new(Mutex::new(HashSet::from([
            "/resources/existing".to_string()
        ])));
        let observed = state.clone();
        runtime.block_on(
            Mock::given(method("GET"))
                .respond_with(move |request: &wiremock::Request| {
                    ResponseTemplate::new(
                        if observed.lock().unwrap().contains(request.url.path()) {
                            200
                        } else {
                            404
                        },
                    )
                })
                .mount(&server),
        );
        let written = state.clone();
        runtime.block_on(
            Mock::given(method("PUT"))
                .respond_with(move |request: &wiremock::Request| {
                    written.lock().unwrap().insert(request.url.path().into());
                    ResponseTemplate::new(204)
                })
                .mount(&server),
        );
        let (handle, connection, log) = fixture();
        let task = format!(
            "!http\nurl: '{}/resources/{{{{ item }}}}'\nmethod: PUT\nbody: desired\nexpected_status: [204]\ncheck:\n  url: '{}/resources/{{{{ item }}}}'\n  expected_status: [200, 404]\n  matches: '(eq response.status 200)'\nwith:\n  items: {items}\nand:\n  notify: reload",
            server.uri(),
            server.uri()
        );
        assert_eq!(
            run(&handle, &connection, &task, true).unwrap().status,
            TaskStatus::NeedsExecution
        );
        assert_eq!(
            handle
                .run_state
                .context
                .read()
                .unwrap()
                .get_total_executions_count(),
            1
        );
        let requests = runtime.block_on(server.received_requests()).unwrap();
        assert_eq!(requests.len(), 2);
        assert!(requests.iter().all(|r| r.method == "GET"));
        assert!(!handle.host.read().unwrap().is_notified(0, "reload"));
        assert_eq!(
            run(&handle, &connection, &task, false).unwrap().status,
            TaskStatus::IsExecuted
        );
        assert!(handle.host.read().unwrap().is_notified(0, "reload"));
        handle.run_state.context.write().unwrap().play_count = 1;
        assert_eq!(
            run(&handle, &connection, &task, false).unwrap().status,
            TaskStatus::IsMatched
        );
        assert!(!handle.host.read().unwrap().is_notified(1, "reload"));
        assert_eq!(
            run(&handle, &connection, &task, true).unwrap().status,
            TaskStatus::IsMatched
        );
        let requests = runtime.block_on(server.received_requests()).unwrap();
        assert_eq!(requests.iter().filter(|r| r.method == "PUT").count(), 1);
        assert_eq!(requests.iter().filter(|r| r.method == "GET").count(), 8);
        assert!(log.lock().unwrap().is_empty());
    }
}
#[test]
fn item_specific_unknown_outputs_do_not_hide_known_changes_or_skip_later_items() {
    let (handle, connection, log) = fixture();
    run(
        &handle,
        &connection,
        "!command\ncmd: true\nsave: missing",
        true,
    )
    .unwrap();
    handle
        .host
        .write()
        .unwrap()
        .update_variables(serde_yaml::from_str("known: echo-known").unwrap());
    let task = "!command\ncmd: '{{ lookup this item }}'\nwith:\n  items: [missing, known]";
    let response = run(&handle, &connection, task, true).unwrap();
    assert_eq!(response.status, TaskStatus::NeedsExecution);
    assert!(response.deferred);
    assert!(log.lock().unwrap().is_empty());
    assert_eq!(
        handle
            .run_state
            .context
            .read()
            .unwrap()
            .get_total_deferred_count(),
        1
    );
}
