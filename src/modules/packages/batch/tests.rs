// Copyright (C) Riff Labs <team@riff.cc>
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use crate::connection::connection::Connection;
use crate::inventory::hosts::PackagePreference;
use crate::playbooks::language::Play;
use crate::playbooks::task_fsm::async_run_single_task;
use crate::playbooks::visitor::CheckMode;
use crate::registry::list::Task;
use crate::test_support::{RecordingConnection, query_request, test_handle};

const BACKENDS: &[&str] = &["apt", "homebrew", "pacman", "yum", "dnf", "zypper"];

fn is_query(command: &str) -> bool {
    command.starts_with("dpkg-query ")
        || command.starts_with("rpm -q ")
        || command.starts_with("pacman -Q ")
        || command.contains("info --json=v2")
}

fn is_mutation(command: &str) -> bool {
    command.contains("apt-get install")
        || command.contains("apt-get remove")
        || command.starts_with("pacman -S ")
        || command.starts_with("pacman -R ")
        || command.starts_with("dnf install ")
        || command.starts_with("dnf remove ")
        || command.starts_with("dnf update ")
        || command.contains("zypper --non-interactive --quiet install")
        || command.contains("zypper --non-interactive --quiet remove")
        || command.contains("then brew install ")
        || command.contains("then brew uninstall ")
        || command.contains("then brew upgrade ")
        || command.contains("zypper --non-interactive --quiet update")
}

type Fixture = (
    Arc<TaskHandle>,
    Arc<Mutex<dyn Connection>>,
    Arc<Mutex<Vec<String>>>,
);

fn fixture<F: Fn(&str) -> (i32, String) + Send + Sync + 'static>(result: F) -> Fixture {
    let recording = RecordingConnection::with_result(move |command| {
        if command.contains("/var/lib/apt/lists") {
            (0, "fresh".into())
        } else {
            result(command)
        }
    });
    let log = recording.command_log();
    let connection: Arc<Mutex<dyn Connection>> = Arc::new(Mutex::new(recording));
    let handle = test_handle(connection.clone());
    handle.host.write().unwrap().package_preference = Some(PackagePreference::Dnf);
    (handle, connection, log)
}

fn installed(backend: &str, packages: &[(&str, &str)], requested: &[&str]) -> (i32, String) {
    if backend == "homebrew" {
        return (
            0,
            serde_json::json!({"formulas": requested.iter().map(|name| {
            let versions: Vec<_> = packages.iter().filter(|(package, _)| package == name)
                .map(|(_, version)| serde_json::json!({"version":version})).collect();
            serde_json::json!({"name":name, "full_name":name, "installed":versions})
        }).collect::<Vec<_>>()})
            .to_string(),
        );
    }
    let mut out = packages
        .iter()
        .map(|(name, version)| match backend {
            "apt" => format!("{name}\t{version}\tinstalled\n"),
            "pacman" => format!("{name} {version}\n"),
            _ => format!("{name}\t{version}\n"),
        })
        .collect::<String>();
    for name in requested
        .iter()
        .filter(|name| !packages.iter().any(|(package, _)| package == *name))
    {
        out.push_str(&match backend {
            "apt" => format!("dpkg-query: no packages found matching {name}\n"),
            "pacman" => format!("error: package '{name}' was not found\n"),
            _ => format!("package {name} is not installed\n"),
        });
    }
    // The real package tools return 1 when any queried names are absent.
    (
        if packages.len() == requested.len() {
            0
        } else {
            1
        },
        out,
    )
}

fn run(
    yaml: &str,
    handle: &Arc<TaskHandle>,
    connection: &Arc<Mutex<dyn Connection>>,
) -> Result<Arc<TaskResponse>, Arc<TaskResponse>> {
    let task: Task = serde_yaml::from_str(yaml).unwrap();
    let play: Play = serde_yaml::from_str("name: package batches\ngroups: [all]\n").unwrap();
    async_run_single_task(&handle.run_state, connection, &handle.host, &play, &task)
}

#[test]
fn scalar_and_list_parse_on_every_package_backend() {
    for backend in BACKENDS {
        for package in ["vim", "[vim, curl]", "['{{ tool }}', curl]"] {
            assert!(
                serde_yaml::from_str::<Task>(&format!("!{backend}\npackage: {package}\n")).is_ok()
            );
        }
    }
    for yaml in [
        "!apt\npackage: {name: vim}\n",
        "!apt\npackage: [vim, {name: curl}]\n",
        "!file\npath: [/tmp/one, /tmp/two]\n",
    ] {
        assert!(serde_yaml::from_str::<Task>(yaml).is_err());
    }
}

#[test]
fn mixed_presence_and_version_drift_select_only_changes_in_one_transaction() {
    for backend in BACKENDS {
        let backend = *backend;
        let (handle, connection, log) = fixture(move |command| {
            if is_query(command) {
                if backend == "homebrew" {
                    installed(
                        backend,
                        &[("ready@2", "2"), ("old@2", "1")],
                        &["ready@2", "old@2", "new@2"],
                    )
                } else {
                    installed(
                        backend,
                        &[("ready", "2"), ("old", "1")],
                        &["ready", "old", "new"],
                    )
                }
            } else {
                (0, String::new())
            }
        });
        let result = run(
            &format!("!{backend}\npackage: [ready, old, new]\nversion: '2'\n"),
            &handle,
            &connection,
        )
        .unwrap();
        assert_eq!(result.status, TaskStatus::IsCreated, "{backend}");
        let commands = log.lock().unwrap();
        assert_eq!(
            commands.iter().filter(|cmd| is_query(cmd)).count(),
            1,
            "{backend}"
        );
        let mutations: Vec<_> = commands.iter().filter(|cmd| is_mutation(cmd)).collect();
        assert_eq!(mutations.len(), 1, "{backend}: {commands:?}");
        assert!(!mutations[0].contains("ready"));
        assert!(mutations[0].contains("old") && mutations[0].contains("new"));
    }
}

#[test]
fn removal_selects_only_installed_packages_and_converges() {
    for backend in BACKENDS {
        let backend = *backend;
        let removed = Arc::new(Mutex::new(false));
        let state = removed.clone();
        let (handle, connection, log) = fixture(move |command| {
            if is_query(command) {
                installed(
                    backend,
                    &if *state.lock().unwrap() {
                        vec![]
                    } else {
                        vec![("old", "1")]
                    },
                    &["old", "absent"],
                )
            } else {
                if is_mutation(command) {
                    *state.lock().unwrap() = true;
                }
                (0, String::new())
            }
        });
        let yaml = format!("!{backend}\npackage: [old, absent]\nremove: 'true'\n");
        assert_eq!(
            run(&yaml, &handle, &connection).unwrap().status,
            TaskStatus::IsRemoved
        );
        assert_eq!(
            run(&yaml, &handle, &connection).unwrap().status,
            TaskStatus::IsMatched
        );
        let commands = log.lock().unwrap();
        let mutations: Vec<_> = commands.iter().filter(|cmd| is_mutation(cmd)).collect();
        assert_eq!(mutations.len(), 1, "{backend}: {commands:?}");
        assert!(mutations[0].contains("old") && !mutations[0].contains("absent"));
    }
}

#[test]
fn install_reapply_is_matched_and_duplicates_do_not_repeat_resource_work() {
    for backend in BACKENDS {
        let backend = *backend;
        let applied = Arc::new(Mutex::new(false));
        let state = applied.clone();
        let (handle, connection, log) = fixture(move |command| {
            if is_query(command) {
                installed(
                    backend,
                    &if *state.lock().unwrap() {
                        vec![("one", "1"), ("two", "1")]
                    } else {
                        vec![]
                    },
                    &["one", "two"],
                )
            } else {
                if is_mutation(command) {
                    *state.lock().unwrap() = true;
                }
                (0, String::new())
            }
        });
        let yaml = format!("!{backend}\npackage: [one, two, one]\n");
        assert_eq!(
            run(&yaml, &handle, &connection).unwrap().status,
            TaskStatus::IsCreated
        );
        assert_eq!(
            run(&yaml, &handle, &connection).unwrap().status,
            TaskStatus::IsMatched
        );
        let commands = log.lock().unwrap();
        assert_eq!(commands.iter().filter(|cmd| is_query(cmd)).count(), 2);
        assert_eq!(commands.iter().filter(|cmd| is_mutation(cmd)).count(), 1);
        let query = commands.iter().find(|cmd| is_query(cmd)).unwrap();
        // Homebrew's shell discovery repeats argv in its three branches.
        assert_eq!(
            query.matches("one").count(),
            if backend == "homebrew" { 3 } else { 1 }
        );
    }
}

#[test]
fn plan_reports_changes_without_running_mutations_or_apt_cache_refresh() {
    let (handle, connection, log) = fixture(|command| {
        if is_query(command) {
            installed("apt", &[], &["one", "two"])
        } else {
            panic!("unexpected command in plan: {command}")
        }
    });
    handle.run_state.visitor.write().unwrap().check_mode = CheckMode::Yes;
    let response = run(
        "!apt\npackage: [one, two]\nupdate: 'true'\n",
        &handle,
        &connection,
    )
    .unwrap();
    assert_eq!(response.status, TaskStatus::NeedsCreation);
    assert_eq!(log.lock().unwrap().len(), 1);
}

#[test]
fn failed_query_or_transaction_never_reports_success() {
    for query_failure in [true, false] {
        let (handle, connection, log) = fixture(move |command| {
            if is_query(command) {
                if query_failure {
                    (2, "database unavailable".into())
                } else {
                    installed("apt", &[], &["one", "two"])
                }
            } else if is_mutation(command) {
                (100, "installation failed".into())
            } else {
                (0, String::new())
            }
        });
        assert_eq!(
            run("!apt\npackage: [one, two]\n", &handle, &connection)
                .unwrap_err()
                .status,
            TaskStatus::Failed
        );
        assert_eq!(
            log.lock()
                .unwrap()
                .iter()
                .filter(|cmd| is_mutation(cmd))
                .count(),
            usize::from(!query_failure)
        );
    }
}

#[test]
fn malformed_query_output_fails_before_apply_for_every_backend() {
    for backend in BACKENDS {
        let (handle, connection, log) = fixture(|command| {
            if is_query(command) {
                (1, "database is broken".into())
            } else {
                (0, String::new())
            }
        });
        assert!(
            run(
                &format!("!{backend}\npackage: [one, two]\n"),
                &handle,
                &connection
            )
            .is_err()
        );
        assert!(!log.lock().unwrap().iter().any(|cmd| is_mutation(cmd)));
    }
}

#[test]
fn validation_is_indexed_and_skipped_tasks_defer_resource_templates() {
    for input in [
        "[]",
        "[one, '']",
        "[one, 'two words']",
        "[one, '-y']",
        "[one, 'two;evil']",
        "[one, \"two\\nwords\"]",
    ] {
        let (handle, connection, log) = fixture(|_| panic!("invalid batch must not execute"));
        assert!(run(&format!("!apt\npackage: {input}\n"), &handle, &connection).is_err());
        assert!(log.lock().unwrap().is_empty());
    }
    let (handle, connection, log) = fixture(|_| panic!("skipped batch must not execute"));
    let response = run(
        "!apt\npackage: ['{{ missing }}', two]\nwith:\n  condition: 'false'\n",
        &handle,
        &connection,
    )
    .unwrap();
    assert_eq!(response.status, TaskStatus::IsSkipped);
    assert!(log.lock().unwrap().is_empty());
    let task: Task = serde_yaml::from_str("!apt\npackage: [one, '-y']\n").unwrap();
    let error = match task.evaluate(&handle, &query_request(), TemplateMode::Strict) {
        Err(error) => error,
        Ok(_) => panic!("invalid input passed"),
    };
    assert!(error.msg.as_ref().unwrap().contains("package[1]"));
}

#[test]
fn templating_and_batch_selection_are_scoped_to_hosts_and_items() {
    let workers: Vec<_> = (0..8).map(|id| std::thread::spawn(move || {
        let (handle, connection, log) = fixture(|command| if is_query(command) { (1, "dpkg-query: no packages found matching requested names".into()) } else { (0, String::new()) });
        handle.host.write().unwrap().update_variables(serde_yaml::from_str(&format!("prefix: host{id}\n")).unwrap());
        let response = run("!apt\npackage: ['{{ prefix }}-{{ item }}-one', '{{ prefix }}-{{ item }}-two']\nwith:\n  items: [a, b]\n", &handle, &connection).unwrap();
        assert_eq!(response.status, TaskStatus::IsCreated);
        let commands = log.lock().unwrap();
        let mutations: Vec<_> = commands.iter().filter(|cmd| is_mutation(cmd)).collect();
        assert_eq!(mutations.len(), 2);
        assert!(mutations[0].contains(&format!("host{id}-a-one")) && !mutations[0].contains(&format!("host{id}-b-one")));
        assert!(mutations[1].contains(&format!("host{id}-b-two")) && !mutations[1].contains(&format!("host{id}-a-two")));
    })).collect();
    for worker in workers {
        worker.join().unwrap();
    }
}

#[test]
fn scalar_apt_keeps_existing_query_and_install_path() {
    let (handle, connection, log) = fixture(|command| {
        if command.starts_with("dpkg -s ") {
            (1, String::new())
        } else {
            (0, String::new())
        }
    });
    assert_eq!(
        run("!apt\npackage: one\n", &handle, &connection)
            .unwrap()
            .status,
        TaskStatus::IsCreated
    );
    let commands = log.lock().unwrap();
    assert!(commands.iter().any(|cmd| cmd == "dpkg -s 'one' >/dev/null"));
    assert!(
        commands
            .iter()
            .any(|cmd| cmd == "DEBIAN_FRONTEND=noninteractive apt-get install 'one' -qq")
    );
}

#[test]
fn apply_requires_a_successful_query_for_the_same_evaluated_batch() {
    let (handle, _, log) = fixture(|_| panic!("unqueried batch must not execute"));
    let task: Task = serde_yaml::from_str("!apt\npackage: [one, two]\n").unwrap();
    let action = task
        .evaluate(&handle, &query_request(), TemplateMode::Strict)
        .unwrap()
        .action;
    let request = TaskRequest::create(&query_request().sudo_details.clone().unwrap());
    assert!(action.dispatch(&handle, &request).is_err());
    assert!(log.lock().unwrap().is_empty());
}

#[test]
fn all_installed_version_drift_reports_modified_and_notifies_once() {
    let (handle, connection, log) = fixture(|command| {
        if is_query(command) {
            installed("apt", &[("ready", "2"), ("old", "1")], &["ready", "old"])
        } else {
            (0, String::new())
        }
    });
    let response = run(
        "!apt\npackage: [ready, old]\nversion: '2'\nand:\n  notify: packages_changed\n",
        &handle,
        &connection,
    )
    .unwrap();
    assert_eq!(response.status, TaskStatus::IsModified);
    assert_eq!(response.changes, vec![Field::Version]);
    let play_count = handle.run_state.context.read().unwrap().play_count;
    assert!(
        handle
            .host
            .read()
            .unwrap()
            .is_notified(play_count, "packages_changed")
    );
    let commands = log.lock().unwrap();
    let mutation = commands
        .iter()
        .find(|command| is_mutation(command))
        .unwrap();
    assert!(mutation.contains("'old=2'") && !mutation.contains("ready"));
}

#[test]
fn apt_batch_accepts_multiarch_names_and_treats_config_files_as_absent() {
    let (handle, connection, log) = fixture(|command| {
        if is_query(command) {
            (
                0,
                "ready:amd64\t2\tinstalled\nremoved\t1\tconfig-files\n".into(),
            )
        } else {
            (0, String::new())
        }
    });
    assert_eq!(
        run("!apt\npackage: [ready, removed]\n", &handle, &connection)
            .unwrap()
            .status,
        TaskStatus::IsCreated
    );
    let commands = log.lock().unwrap();
    let mutation = commands
        .iter()
        .find(|command| is_mutation(command))
        .unwrap();
    assert!(mutation.contains("'removed'") && !mutation.contains("ready"));
}

#[test]
fn apt_local_deb_metadata_feeds_the_shared_query_and_one_install_transaction() {
    let (handle, connection, log) = fixture(|command| {
        if command.starts_with("dpkg-deb ") {
            (0, "example-agent".into())
        } else if is_query(command) {
            installed("apt", &[("ready", "1")], &["ready", "example-agent", "new"])
        } else {
            (0, String::new())
        }
    });
    let response = run(
        "!apt\npackage: [ready, /tmp/example-agent.deb, new]\n",
        &handle,
        &connection,
    )
    .unwrap();
    assert_eq!(response.status, TaskStatus::IsCreated);
    let commands = log.lock().unwrap();
    let query = commands.iter().find(|command| is_query(command)).unwrap();
    assert!(query.contains("'example-agent'") && !query.contains(".deb"));
    let mutations: Vec<_> = commands
        .iter()
        .filter(|command| is_mutation(command))
        .collect();
    assert_eq!(mutations.len(), 1);
    assert!(
        mutations[0].contains("'/tmp/example-agent.deb'")
            && mutations[0].contains("'new'")
            && !mutations[0].contains("ready")
    );
}

#[test]
fn empty_or_incomplete_success_output_fails_before_mutation() {
    for backend in BACKENDS {
        for empty in [true, false] {
            let backend = *backend;
            let (handle, connection, log) = fixture(move |command| {
                if is_query(command) {
                    if empty {
                        (0, String::new())
                    } else {
                        installed(backend, &[("one", "1")], &["one"])
                    }
                } else {
                    (0, String::new())
                }
            });
            assert!(
                run(
                    &format!("!{backend}\npackage: [one, two]\n"),
                    &handle,
                    &connection
                )
                .is_err(),
                "{backend}, empty={empty}"
            );
            assert!(
                !log.lock()
                    .unwrap()
                    .iter()
                    .any(|command| is_mutation(command))
            );
        }
    }
}

#[test]
fn batch_arguments_with_quotes_remain_literal_shell_words() {
    let values = ["tool'quoted", "tool\"double"];
    let arguments = values
        .iter()
        .map(|value| quote(value))
        .collect::<Vec<_>>()
        .join(" ");
    let output = std::process::Command::new("sh")
        .arg("-c")
        .arg(format!("printf '%s\\n' {arguments}"))
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!("{}\n{}\n", values[0], values[1])
    );
}

#[cfg(unix)]
#[test]
fn homebrew_transaction_is_valid_under_connection_locale_prefix_and_preserves_argv() {
    use std::os::unix::fs::PermissionsExt;
    let _lock = crate::test_support::ENV_LOCK.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let brew = root.path().join("brew");
    std::fs::write(&brew, "#!/bin/sh\nprintf '%s\\n' \"$@\"\n").unwrap();
    std::fs::set_permissions(&brew, std::fs::Permissions::from_mode(0o755)).unwrap();
    let (handle, connection, log) = fixture(|command| {
        if is_query(command) {
            installed("homebrew", &[], &["tool'quoted", "two"])
        } else {
            (0, String::new())
        }
    });
    run(
        "!homebrew\npackage: [\"tool'quoted\", two]\n",
        &handle,
        &connection,
    )
    .unwrap();
    let commands = log.lock().unwrap();
    let transaction = commands
        .iter()
        .find(|command| is_mutation(command))
        .unwrap();
    let output = std::process::Command::new("sh")
        .arg("-c")
        .arg(format!("LANG=C {transaction}"))
        .env("PATH", format!("{}:/usr/bin:/bin", root.path().display()))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "install\n--\ntool'quoted\ntwo\n"
    );
}

#[test]
fn upgrades_compare_candidates_and_skip_packages_already_at_the_desired_version() {
    for backend in ["apt", "pacman", "homebrew", "dnf", "yum", "zypper"] {
        for changed in [true, false] {
            let version = if backend == "zypper" { "2-1" } else { "2" };
            let previous = if backend == "zypper" { "1-1" } else { "1" };
            let (handle, connection, log) = fixture(move |command| {
                if command.starts_with("apt-cache policy ") {
                    (0, "  Candidate: 2\n".into())
                } else if command.starts_with("pacman -Si ") {
                    (0, "Name : old\nVersion : 2\n".into())
                } else if command.starts_with("zypper --non-interactive --quiet search ") {
                    (0, "S | Name | Type | Version | Arch | Repository\n--+------+-----+---------+------+-----------\n  | old | package | 2-1 | x86_64 | repo\n".into())
                } else if command.starts_with("repoquery ") {
                    (0, "2".into())
                } else if backend == "homebrew"
                    && command.contains("info --json=v2")
                    && !(command.contains("ready") && command.contains("old"))
                {
                    (
                        0,
                        serde_json::json!({"formulas":[{"versions":{"stable":"2"},"revision":0}]})
                            .to_string(),
                    )
                } else if is_query(command) {
                    installed(
                        backend,
                        &[
                            ("ready", version),
                            ("old", if changed { previous } else { version }),
                        ],
                        &["ready", "old"],
                    )
                } else {
                    (0, String::new())
                }
            });
            let field = if backend == "apt" {
                "upgrade"
            } else {
                "update"
            };
            let response = run(
                &format!("!{backend}\npackage: [ready, old]\n{field}: 'true'\n"),
                &handle,
                &connection,
            )
            .unwrap();
            assert_eq!(
                response.status,
                if changed {
                    TaskStatus::IsModified
                } else {
                    TaskStatus::IsMatched
                },
                "{backend}: changed={changed}"
            );
            let commands = log.lock().unwrap();
            let transactions: Vec<_> = commands
                .iter()
                .filter(|command| is_mutation(command))
                .collect();
            assert_eq!(
                transactions.len(),
                usize::from(changed),
                "{backend}: {commands:?}"
            );
            if changed {
                assert!(transactions[0].contains("old") && !transactions[0].contains("ready"));
                assert!(!transactions[0].contains("-Syu"));
            }
        }
    }
}

#[test]
fn unknown_upgrade_candidates_fail_before_any_transaction() {
    let (handle, connection, log) = fixture(|command| {
        if is_query(command) {
            installed("apt", &[("one", "1")], &["one"])
        } else if command.starts_with("apt-cache policy ") {
            (0, "  Candidate: (none)\n".into())
        } else {
            (0, String::new())
        }
    });
    assert!(
        run(
            "!apt\npackage: [one]\nupgrade: 'true'\n",
            &handle,
            &connection
        )
        .is_err()
    );
    assert!(
        !log.lock()
            .unwrap()
            .iter()
            .any(|command| is_mutation(command))
    );
}

#[test]
fn failed_repository_query_prevents_upgrade_on_every_backend() {
    for backend in BACKENDS {
        let backend = *backend;
        let queried = Arc::new(Mutex::new(false));
        let (handle, connection, log) = fixture(move |command| {
            if is_query(command) && !*queried.lock().unwrap() {
                *queried.lock().unwrap() = true;
                installed(backend, &[("one", "1")], &["one"])
            } else if is_query(command)
                || command.starts_with("apt-cache policy ")
                || command.starts_with("pacman -Si ")
                || command.starts_with("repoquery ")
                || command.starts_with("zypper --non-interactive --quiet search ")
            {
                (2, "repository unavailable".into())
            } else {
                (0, String::new())
            }
        });
        let field = if backend == "apt" {
            "upgrade"
        } else {
            "update"
        };
        assert!(
            run(
                &format!("!{backend}\npackage: [one]\n{field}: 'true'\n"),
                &handle,
                &connection
            )
            .is_err()
        );
        assert!(
            !log.lock()
                .unwrap()
                .iter()
                .any(|command| is_mutation(command))
        );
    }
}

#[test]
fn homebrew_revision_is_part_of_upgrade_comparison() {
    let (handle, connection, log) = fixture(|command| {
        if is_query(command) {
            (0, serde_json::json!({"formulas":[{"name":"one", "full_name":"one", "installed":[{"version":"2_1"}], "linked_keg":"2_1", "versions":{"stable":"2"}, "revision":1}]}).to_string())
        } else {
            (0, String::new())
        }
    });
    assert_eq!(
        run(
            "!homebrew\npackage: [one]\nupdate: 'true'\n",
            &handle,
            &connection
        )
        .unwrap()
        .status,
        TaskStatus::IsMatched
    );
    assert!(
        !log.lock()
            .unwrap()
            .iter()
            .any(|command| is_mutation(command))
    );
}

#[test]
fn diagnostic_lines_never_turn_missing_packages_into_installed_records() {
    let (handle, connection, log) = fixture(|command| {
        if is_query(command) {
            (1, "package package is not installed\n".into())
        } else {
            (0, String::new())
        }
    });
    assert_eq!(
        run("!dnf\npackage: [package]\n", &handle, &connection)
            .unwrap()
            .status,
        TaskStatus::IsCreated
    );
    assert_eq!(
        log.lock()
            .unwrap()
            .iter()
            .filter(|command| is_mutation(command))
            .count(),
        1
    );
    let (handle, connection, log) = fixture(|command| {
        if is_query(command) {
            (0, "one\t1\tdatabase-error\n".into())
        } else {
            (0, String::new())
        }
    });
    assert!(run("!apt\npackage: [one]\n", &handle, &connection).is_err());
    assert!(
        !log.lock()
            .unwrap()
            .iter()
            .any(|command| is_mutation(command))
    );
}
