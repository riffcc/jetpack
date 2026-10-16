// Copyright (C) Riff Labs <team@riff.cc>
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use crate::connection::command::cmd_info;
use crate::connection::connection::Connection;
use crate::connection::local::LocalConnection;
use crate::handle::handle::TaskHandle;
use crate::inventory::hosts::Host;
use crate::playbooks::language::Play;
use crate::playbooks::task_fsm::async_run_single_task;
use crate::playbooks::visitor::CheckMode;
use crate::registry::list::Task;
use crate::tasks::*;
use crate::test_support::{RecordingConnection, query_request, test_handle};
use serde_yaml::Value;
use std::sync::{Arc, Mutex, RwLock};

fn local() -> (Arc<TaskHandle>, Arc<Mutex<dyn Connection>>) {
    let host = Arc::new(RwLock::new(Host::new("localhost")));
    let connection: Arc<Mutex<dyn Connection>> = Arc::new(Mutex::new(LocalConnection::new(&host)));
    (test_handle(connection.clone()), connection)
}

fn run(
    task: &Task,
    handle: &Arc<TaskHandle>,
    connection: &Arc<Mutex<dyn Connection>>,
) -> Result<Arc<TaskResponse>, Arc<TaskResponse>> {
    let play: Play = serde_yaml::from_str("name: working directory\ngroups: [all]\n").unwrap();
    async_run_single_task(&handle.run_state, connection, &handle.host, &play, task)
}

fn command(tag: &str, cmd: &str, dir: Option<&str>) -> Task {
    let mut input = serde_yaml::Mapping::new();
    input.insert("cmd".into(), cmd.into());
    input.insert("save".into(), "saved".into());
    if let Some(dir) = dir {
        input.insert("chdir".into(), dir.into());
    }
    let body = serde_yaml::to_string(&input).unwrap();
    serde_yaml::from_str(&format!("!{tag}\n{body}")).unwrap()
}

#[test]
fn rejects_nonabsolute_empty_tilde_and_nul_paths() {
    for path in [
        "",
        " ",
        ".",
        "..",
        "project",
        "~/project",
        "~",
        "/tmp/\0invalid",
    ] {
        assert!(validate(path).is_err(), "{path:?}");
    }
    for path in [
        "/",
        "/app",
        "/a path/with'quotes",
        "/target/../project",
        "/dir/$literal",
    ] {
        assert!(validate(path).is_ok(), "{path}");
    }
}

#[test]
fn omitted_directory_preserves_original_command_exactly() {
    let prepared = prepare("printf 'hello'", None).unwrap();
    assert_eq!(prepared.command, "printf 'hello'");
    assert!(prepared.directory_failure.is_none());
}

#[test]
fn command_and_shell_enter_the_directory_and_save_output_without_changing_controller_cwd() {
    let _lock = crate::test_support::ENV_LOCK.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().canonicalize().unwrap();
    let original = std::env::current_dir().unwrap();
    for tag in ["command", "shell"] {
        let (handle, connection) = local();
        let result = run(&command(tag, "pwd", dir.to_str()), &handle, &connection).unwrap();
        assert_eq!(result.status, TaskStatus::IsExecuted);
        assert_eq!(cmd_info(&result), (0, dir.display().to_string()));
        let vars = Value::Mapping(handle.host.read().unwrap().get_variables());
        assert_eq!(vars["saved"]["out"], dir.display().to_string());
        assert_eq!(std::env::current_dir().unwrap(), original);
    }
}

#[test]
fn path_metacharacters_and_quotes_are_literal_arguments() {
    let _lock = crate::test_support::ENV_LOCK.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let root = root.path().canonicalize().unwrap();
    let dir = root.join("work space ' $literal `echo injected` ; &");
    std::fs::create_dir(&dir).unwrap();
    for tag in ["command", "shell"] {
        let (handle, connection) = local();
        let result = run(&command(tag, "pwd", dir.to_str()), &handle, &connection).unwrap();
        assert_eq!(cmd_info(&result).1, dir.display().to_string());
    }
}

#[test]
fn invalid_paths_fail_at_evaluation_and_missing_or_nondirectory_paths_never_run_the_command() {
    let _lock = crate::test_support::ENV_LOCK.lock().unwrap();
    let (handle, connection) = local();
    for dir in ["", "relative", "~", "~/project"] {
        let error = run(
            &command("command", "echo should-not-run", Some(dir)),
            &handle,
            &connection,
        )
        .unwrap_err();
        assert!(error.msg.as_ref().unwrap().contains("absolute target path"));
    }
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("file");
    std::fs::write(&file, "not a directory").unwrap();
    let marker = root.path().join("should-not-exist");
    for dir in [root.path().join("missing"), file] {
        let mut task = command(
            "shell",
            &format!("touch '{}'", marker.display()),
            dir.to_str(),
        );
        if let Task::Shell(task) = &mut task {
            task.failed_when = Some("false".into());
        }
        let error = run(&task, &handle, &connection).unwrap_err();
        assert!(error.msg.as_ref().unwrap().contains("cannot enter chdir"));
        assert!(!marker.exists());
    }
}

#[test]
fn a_normal_command_exit_125_is_not_a_directory_entry_error() {
    let _lock = crate::test_support::ENV_LOCK.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let (handle, connection) = local();
    let mut task = command("command", "exit 125", root.path().to_str());
    if let Task::Command(task) = &mut task {
        task.failed_when = Some("false".into());
    }
    let result = run(&task, &handle, &connection).unwrap();
    assert_eq!(cmd_info(&result).0, 125);
}

#[test]
fn template_resolution_is_per_item_and_skipped_tasks_defer_directory_validation() {
    let _lock = crate::test_support::ENV_LOCK.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let a = root.path().join("a");
    let b = root.path().join("b");
    std::fs::create_dir(&a).unwrap();
    std::fs::create_dir(&b).unwrap();
    let (handle, connection) = local();
    let mut task = command("command", "pwd", Some("{{ item }}"));
    if let Task::Command(task) = &mut task {
        task.with = Some(
            serde_yaml::from_str(&format!("items: ['{}', '{}']", a.display(), b.display()))
                .unwrap(),
        );
    }
    run(&task, &handle, &connection).unwrap();
    assert_eq!(
        Value::Mapping(handle.host.read().unwrap().get_variables())["saved"]["out"],
        b.display().to_string()
    );
    let mut task = command(
        "command",
        "echo should-not-run",
        Some("{{ missing_directory }}"),
    );
    if let Task::Command(task) = &mut task {
        task.with = Some(serde_yaml::from_str("condition: false").unwrap());
    }
    assert_eq!(
        run(&task, &handle, &connection).unwrap().status,
        TaskStatus::IsSkipped
    );
}

#[test]
fn plan_validates_absolute_syntax_but_does_not_execute_or_enter_the_directory() {
    let recording = RecordingConnection::new();
    let log = recording.command_log();
    let connection: Arc<Mutex<dyn Connection>> = Arc::new(Mutex::new(recording));
    let handle = test_handle(connection.clone());
    handle.run_state.visitor.write().unwrap().check_mode = CheckMode::Yes;
    assert_eq!(
        run(
            &command("command", "pwd", Some("/target-only/path")),
            &handle,
            &connection
        )
        .unwrap()
        .status,
        TaskStatus::NeedsExecution
    );
    assert!(log.lock().unwrap().is_empty());
    assert!(
        run(
            &command("command", "pwd", Some("relative")),
            &handle,
            &connection
        )
        .is_err()
    );
}

#[test]
fn target_namespace_is_preserved_in_shared_execution_and_sudo_wraps_directory_entry() {
    let recording = RecordingConnection::new();
    let log = recording.command_log();
    let connection: Arc<Mutex<dyn Connection>> = Arc::new(Mutex::new(recording));
    let handle = test_handle(connection.clone());
    let mut task = command("command", "pwd", Some("/target-only/path"));
    if let Task::Command(task) = &mut task {
        task.with = Some(serde_yaml::from_str("sudo: root").unwrap());
    }
    run(&task, &handle, &connection).unwrap();
    let commands = log.lock().unwrap();
    assert_eq!(commands.len(), 1);
    assert!(commands[0].starts_with("/usr/bin/sudo -u 'root' sh -c "));
    assert!(commands[0].contains("/target-only/path"));
    assert!(!commands[0].contains(&std::env::current_dir().unwrap().display().to_string()));
}

#[test]
fn unsupported_modules_and_with_chdir_are_rejected() {
    for yaml in [
        "!http\nurl: https://api.example.com\nchdir: /app\n",
        "!file\npath: /tmp/example\nchdir: /app\n",
        "!command\ncmd: pwd\nwith:\n  chdir: /app\n",
    ] {
        assert!(serde_yaml::from_str::<Task>(yaml).is_err());
    }
}

#[test]
fn request_context_changes_do_not_mutate_shared_requests() {
    let original = query_request();
    let one = original.clone().with_chdir(Some("/one"));
    let two = original.clone().with_chdir(Some("/two"));
    assert!(original.chdir.is_none());
    assert_eq!(one.chdir.as_deref(), Some("/one"));
    assert_eq!(two.chdir.as_deref(), Some("/two"));
    assert!(one.clone().with_chdir(None).chdir.is_none());
    assert_eq!(one.chdir.as_deref(), Some("/one"));
}

#[test]
fn directory_wrapper_preserves_command_screening_and_detects_failure_after_target_notices() {
    let recording = RecordingConnection::with_result(|cmd| {
        let start = cmd.find("jetpack-chdir-failed-").unwrap();
        let marker: String = cmd[start..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '-')
            .collect();
        (125, format!("target login notice\n{marker}\n"))
    });
    let log = recording.command_log();
    let connection: Arc<Mutex<dyn Connection>> = Arc::new(Mutex::new(recording));
    let handle = test_handle(connection);
    let request = query_request().with_chdir(Some("/app"));
    assert!(
        handle
            .remote
            .run(&request, "echo x; echo injected", CheckRc::Unchecked)
            .is_err()
    );
    assert!(log.lock().unwrap().is_empty());
    let error = handle
        .remote
        .run(&request, "pwd", CheckRc::Unchecked)
        .unwrap_err();
    assert!(error.msg.as_ref().unwrap().contains("cannot enter chdir"));
}

#[test]
fn parallel_hosts_and_following_tasks_keep_their_own_directories() {
    let _lock = crate::test_support::ENV_LOCK.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let original = std::env::current_dir().unwrap();
    let workers: Vec<_> = (0..16)
        .map(|id| {
            let dir = root.path().join(id.to_string());
            std::fs::create_dir(&dir).unwrap();
            std::thread::spawn(move || {
                let (handle, connection) = local();
                handle.host.write().unwrap().update_variables(
                    serde_yaml::from_str(&format!("project_directory: {}", dir.display())).unwrap(),
                );
                let result = run(
                    &command("command", "pwd", Some("{{ project_directory }}")),
                    &handle,
                    &connection,
                )
                .unwrap();
                assert_eq!(cmd_info(&result).1, dir.display().to_string());
                let default = run(&command("command", "pwd", None), &handle, &connection).unwrap();
                assert_eq!(
                    cmd_info(&default).1,
                    std::env::current_dir().unwrap().display().to_string()
                );
            })
        })
        .collect();
    for worker in workers {
        worker.join().unwrap();
    }
    assert_eq!(std::env::current_dir().unwrap(), original);
}

#[test]
fn external_module_directory_applies_to_invocation_while_staging_and_cleanup_keep_default_context()
{
    let root = tempfile::tempdir().unwrap();
    let module = root.path().join("probe");
    std::fs::write(&module, "#!/bin/sh\necho '{\"rc\":0}'\n").unwrap();
    let recording = RecordingConnection::with_result(|cmd| {
        if cmd.contains("cd --") {
            (0, "{\"rc\":0,\"cwd\":\"/target-only/path\"}".into())
        } else {
            (0, String::new())
        }
    });
    let log = recording.command_log();
    let connection: Arc<Mutex<dyn Connection>> = Arc::new(Mutex::new(recording));
    let handle = test_handle(connection.clone());
    handle.host.write().unwrap().os_type = Some(crate::inventory::hosts::HostOSType::Linux);
    handle
        .run_state
        .module_paths
        .write()
        .unwrap()
        .push(root.path().into());
    let task: Task = serde_yaml::from_str(
        "!external\nuse: probe\nparams: {}\nchdir: /target-only/path\nsave: saved\n",
    )
    .unwrap();
    run(&task, &handle, &connection).unwrap();
    let commands = log.lock().unwrap();
    assert_eq!(
        commands.iter().filter(|cmd| cmd.contains("cd --")).count(),
        1
    );
    assert!(
        commands
            .iter()
            .find(|cmd| cmd.contains("cd --"))
            .unwrap()
            .contains(" < ")
    );
    assert!(commands.last().unwrap().starts_with("rm "));
    assert_eq!(
        Value::Mapping(handle.host.read().unwrap().get_variables())["saved"]["cwd"],
        "/target-only/path"
    );
}

#[cfg(unix)]
#[test]
fn inaccessible_directory_fails_before_command_execution() {
    let _lock = crate::test_support::ENV_LOCK.lock().unwrap();
    use std::os::unix::fs::PermissionsExt;
    let uid = std::process::Command::new("id").arg("-u").output().unwrap();
    if uid.stdout == b"0\n" {
        return;
    } // root can enter a mode-000 directory
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("private");
    std::fs::create_dir(&dir).unwrap();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o000)).unwrap();
    let (handle, connection) = local();
    let result = run(
        &command("command", "echo should-not-run", dir.to_str()),
        &handle,
        &connection,
    );
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert!(
        result
            .unwrap_err()
            .msg
            .as_ref()
            .unwrap()
            .contains("cannot enter chdir")
    );
}
