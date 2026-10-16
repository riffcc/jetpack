// Copyright (C) Riff Labs <team@riff.cc>
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use crate::connection::factory::ConnectionFactory;
use crate::connection::local::LocalConnection;
use crate::playbooks::context::PlaybookContext;
use crate::playbooks::visitor::CheckMode;
use crate::test_support::{RecordingConnection, test_handle, test_handle_with_factory};

fn task(path: &str) -> Task {
    let mut input = serde_yaml::Mapping::new();
    input.insert("cmd".into(), "echo mutation".into());
    input.insert(
        "with".into(),
        serde_yaml::to_value(serde_json::json!({"skip_if_exists":path})).unwrap(),
    );
    serde_yaml::from_str(&format!(
        "!command\n{}",
        serde_yaml::to_string(&input).unwrap()
    ))
    .unwrap()
}

fn run(
    handle: &Arc<TaskHandle>,
    connection: &Arc<Mutex<dyn Connection>>,
    task: &Task,
    plan: bool,
) -> Result<Arc<TaskResponse>, Arc<TaskResponse>> {
    handle.run_state.visitor.write().unwrap().check_mode =
        if plan { CheckMode::Yes } else { CheckMode::No };
    let play = serde_yaml::from_str("name: target guard\ngroups: [all]").unwrap();
    async_run_single_task(&handle.run_state, connection, &handle.host, &play, task)
}

#[test]
fn target_existence_overrides_controller_existence_in_plan_and_apply() {
    let root = tempfile::tempdir().unwrap();
    let controller_present = root.path().join("controller-only");
    std::fs::write(&controller_present, "present").unwrap();
    let controller_absent = root.path().join("target-only");
    for plan in [true, false] {
        for (path, remote_present) in [(&controller_present, false), (&controller_absent, true)] {
            let recording = RecordingConnection::with_rc(move |cmd| {
                if cmd.starts_with("test -e ") && !remote_present {
                    1
                } else {
                    0
                }
            });
            let log = recording.command_log();
            let connection: Arc<Mutex<dyn Connection>> = Arc::new(Mutex::new(recording));
            let handle = test_handle(connection.clone());
            let result = run(&handle, &connection, &task(path.to_str().unwrap()), plan).unwrap();
            assert_eq!(
                result.status,
                if remote_present {
                    TaskStatus::IsSkipped
                } else if plan {
                    TaskStatus::NeedsExecution
                } else {
                    TaskStatus::IsExecuted
                }
            );
            let commands = log.lock().unwrap();
            assert_eq!(commands[0], format!("test -e '{}'", path.display()));
            assert_eq!(commands.len(), if !plan && !remote_present { 2 } else { 1 });
        }
    }
}

struct DelegateFactory(Arc<Mutex<dyn Connection>>);
impl ConnectionFactory for DelegateFactory {
    fn get_connection(
        &self,
        _: &Arc<RwLock<PlaybookContext>>,
        host: &Arc<RwLock<Host>>,
    ) -> Result<Arc<Mutex<dyn Connection>>, String> {
        assert_eq!(host.read().unwrap().name, "delegate");
        Ok(self.0.clone())
    }
    fn get_local_connection(
        &self,
        _: &Arc<RwLock<PlaybookContext>>,
    ) -> Result<Arc<Mutex<dyn Connection>>, String> {
        Ok(self.0.clone())
    }
}

#[test]
fn delegation_uses_the_delegate_connection_for_the_guard_and_mutation() {
    for present in [true, false] {
        let source = RecordingConnection::with_rc(|_| panic!("origin connection must not be used"));
        let connection: Arc<Mutex<dyn Connection>> = Arc::new(Mutex::new(source));
        let delegate = RecordingConnection::with_rc(move |cmd| {
            if cmd.starts_with("test -e ") && !present {
                1
            } else {
                0
            }
        });
        let log = delegate.command_log();
        let delegate: Arc<Mutex<dyn Connection>> = Arc::new(Mutex::new(delegate));
        let handle = test_handle_with_factory(
            connection.clone(),
            Arc::new(RwLock::new(DelegateFactory(delegate))),
        );
        handle
            .run_state
            .inventory
            .write()
            .unwrap()
            .create_host("delegate");
        let mut task = task("/target/marker");
        if let Task::Command(task) = &mut task {
            task.with.as_mut().unwrap().delegate_to = Some("delegate".into());
        }
        assert_eq!(
            run(&handle, &connection, &task, false).unwrap().status,
            if present {
                TaskStatus::IsSkipped
            } else {
                TaskStatus::IsExecuted
            }
        );
        assert_eq!(log.lock().unwrap().len(), if present { 1 } else { 2 });
    }
}

#[test]
fn item_guards_do_not_stop_later_items_and_hosts_keep_their_own_paths() {
    let workers: Vec<_> = (0..8).map(|id| std::thread::spawn(move || {
        let recording = RecordingConnection::with_rc(|cmd| if cmd.starts_with("test -e ") && cmd.contains("/two'") {1} else {0});
        let log = recording.command_log();
        let connection: Arc<Mutex<dyn Connection>> = Arc::new(Mutex::new(recording));
        let handle = test_handle(connection.clone());
        handle.host.write().unwrap().update_variables(serde_yaml::from_str(&format!("id: {id}")).unwrap());
        let task: Task = serde_yaml::from_str("!command\ncmd: echo mutation\nwith:\n  items: [one, two]\n  skip_if_exists: '/target/{{ id }}/{{ item }}'").unwrap();
        assert_eq!(run(&handle, &connection, &task, false).unwrap().status, TaskStatus::IsExecuted);
        assert_eq!(*log.lock().unwrap(), vec![format!("test -e '/target/{id}/one'"), format!("test -e '/target/{id}/two'"), "echo mutation".into()]);
    })).collect();
    for worker in workers {
        worker.join().unwrap();
    }
}

#[test]
fn probe_errors_fail_and_invalid_or_condition_skipped_paths_do_not_probe() {
    for rc in [2, 126, 255] {
        let recording = RecordingConnection::with_rc(move |_| rc);
        let log = recording.command_log();
        let connection: Arc<Mutex<dyn Connection>> = Arc::new(Mutex::new(recording));
        let handle = test_handle(connection.clone());
        assert!(run(&handle, &connection, &task("/target/marker"), false).is_err());
        assert_eq!(log.lock().unwrap().len(), 1);
    }
    let recording = RecordingConnection::with_rc(|_| panic!("must not probe"));
    let connection: Arc<Mutex<dyn Connection>> = Arc::new(Mutex::new(recording));
    let handle = test_handle(connection.clone());
    for path in ["{{ missing }}", "/tmp/a;bad", "/tmp/\0bad"] {
        assert!(run(&handle, &connection, &task(path), false).is_err());
    }
    let mut skipped = task("{{ missing }}");
    if let Task::Command(task) = &mut skipped {
        task.with.as_mut().unwrap().condition = Some("false".into());
    }
    assert_eq!(
        run(&handle, &connection, &skipped, false).unwrap().status,
        TaskStatus::IsSkipped
    );
}

#[test]
fn probe_honors_task_sudo_over_play_and_cli_sudo() {
    let recording = RecordingConnection::new();
    let log = recording.command_log();
    let connection: Arc<Mutex<dyn Connection>> = Arc::new(Mutex::new(recording));
    let handle = test_handle(connection.clone());
    handle.run_state.context.write().unwrap().sudo = Some("cli-user".into());
    let play: Play = serde_yaml::from_str("name: guard\ngroups: [all]\nsudo: play-user\nsudo_template: \"sudo -u '{{jet_sudo_user}}' {{jet_command}}\"").unwrap();
    let mut task = task("/target/marker");
    if let Task::Command(task) = &mut task {
        task.with.as_mut().unwrap().sudo = Some("task-user".into());
    }
    assert_eq!(
        async_run_single_task(&handle.run_state, &connection, &handle.host, &play, &task)
            .unwrap()
            .status,
        TaskStatus::IsSkipped
    );
    assert_eq!(
        *log.lock().unwrap(),
        vec!["sudo -u 'task-user' test -e '/target/marker'"]
    );
}

#[test]
fn local_guards_support_literal_quotes_and_relative_paths_in_task_chdir() {
    let _lock = crate::test_support::ENV_LOCK.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let marker = "marker with 'quotes'";
    std::fs::write(root.path().join(marker), "present").unwrap();
    let host = Arc::new(RwLock::new(Host::new("localhost")));
    let connection: Arc<Mutex<dyn Connection>> = Arc::new(Mutex::new(LocalConnection::new(&host)));
    let handle = test_handle(connection.clone());
    let mut task = task(marker);
    if let Task::Command(task) = &mut task {
        task.chdir = Some(root.path().to_str().unwrap().into());
    }
    for plan in [true, false] {
        assert_eq!(
            run(&handle, &connection, &task, plan).unwrap().status,
            TaskStatus::IsSkipped
        );
    }
    std::fs::remove_file(root.path().join(marker)).unwrap();
    assert_eq!(
        run(&handle, &connection, &task, false).unwrap().status,
        TaskStatus::IsExecuted
    );
}

#[test]
fn local_unpack_reapplication_skips_extraction_and_plan_does_not_create_files() {
    let _lock = crate::test_support::ENV_LOCK.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let src = root.path().join("source");
    std::fs::create_dir(&src).unwrap();
    std::fs::write(src.join("tool"), "original").unwrap();
    let archive = root.path().join("tool.tar.gz");
    assert!(
        std::process::Command::new("tar")
            .arg("-czf")
            .arg(&archive)
            .arg("-C")
            .arg(&src)
            .arg("tool")
            .status()
            .unwrap()
            .success()
    );
    let dest = root.path().join("dest");
    let task: Task = serde_yaml::from_str(&format!(
        "!unpack\nsrc: '{}'\ndest: '{}'\nwith:\n  skip_if_exists: '{}/tool'",
        archive.display(),
        dest.display(),
        dest.display()
    ))
    .unwrap();
    let host = Arc::new(RwLock::new(Host::new("localhost")));
    let connection: Arc<Mutex<dyn Connection>> = Arc::new(Mutex::new(LocalConnection::new(&host)));
    let handle = test_handle(connection.clone());
    assert_eq!(
        run(&handle, &connection, &task, true).unwrap().status,
        TaskStatus::NeedsModification
    );
    assert!(!dest.exists());
    assert_eq!(
        run(&handle, &connection, &task, false).unwrap().status,
        TaskStatus::IsModified
    );
    std::fs::write(dest.join("tool"), "preserve").unwrap();
    for plan in [true, false] {
        assert_eq!(
            run(&handle, &connection, &task, plan).unwrap().status,
            TaskStatus::IsSkipped
        );
    }
    assert_eq!(
        std::fs::read_to_string(dest.join("tool")).unwrap(),
        "preserve"
    );
}
