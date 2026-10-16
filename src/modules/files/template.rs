// Jetporch
// Copyright (C) 2023 - Michael DeHaan <michael@michaeldehaan.net> + contributors
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// long with this program.  If not, see <http://www.gnu.org/licenses/>.

use crate::handle::file_transaction::{self, Snapshot, Validator};
use crate::handle::handle::TaskHandle;
use crate::handle::template::BlendTarget;
use crate::tasks::fields::Field;
use crate::tasks::*;
use serde::Deserialize;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

const MODULE: &str = "template";

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
#[derive(Default)]
pub struct TemplateTask {
    pub name: Option<String>,
    pub src: String,
    pub dest: String,
    pub validate: Option<String>,
    pub chdir: Option<String>,
    pub attributes: Option<FileAttributesInput>,
    pub with: Option<PreLogicInput>,
    pub retry: Option<String>,
    pub delay: Option<String>,
    pub and: Option<PostLogicInput>,
}

struct TemplateAction {
    pub src: PathBuf,
    pub dest: String,
    pub attributes: Option<FileAttributesEvaluated>,
    pub validator: Option<Validator>,
    pub chdir: Option<String>,
    prepared: Mutex<Option<(String, Option<Snapshot>, String)>>,
}

impl IsTask for TemplateTask {
    fn name_variable_writes(&self) -> crate::tasks::naming::NameVariableWrites {
        crate::tasks::naming::NameVariableWrites::None
    }

    fn generated_name(&self, context: &crate::tasks::naming::NamingContext<'_>) -> Option<String> {
        Some(context.resource("Render", &self.dest, "Render files"))
    }

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
        let src = handle
            .template
            .string(request, tm, &String::from("src"), &self.src)?;
        Ok(EvaluatedTask {
            action: Arc::new(TemplateAction {
                src: handle
                    .template
                    .find_template_path(request, tm, &String::from("src"), &src)?,
                dest: handle
                    .template
                    .path(request, tm, &String::from("dest"), &self.dest)?,
                prepared: Mutex::new(None),
                chdir: handle.template.chdir(request, tm, &self.chdir)?,
                validator: if tm == TemplateMode::Off {
                    None
                } else {
                    self.validate
                        .as_ref()
                        .map(|input| {
                            let rendered = handle
                                .template
                                .string_unsafe_for_shell(request, tm, "validate", input)?;
                            Validator::parse(&rendered)
                                .map_err(|e| handle.response.is_failed(request, &e))
                        })
                        .transpose()?
                },
                attributes: FileAttributesInput::template(handle, request, tm, &self.attributes)?,
            }),
            with: Arc::new(PreLogicInput::template(handle, request, tm, &self.with)?),
            and: Arc::new(PostLogicInput::template(
                handle,
                request,
                tm,
                &self.and,
                &self.retry,
                &self.delay,
            )?),
        })
    }
}

impl IsAction for TemplateAction {
    fn chdir(&self) -> Option<&str> {
        self.chdir.as_deref()
    }

    fn dispatch(
        &self,
        handle: &Arc<TaskHandle>,
        request: &Arc<TaskRequest>,
    ) -> Result<Arc<TaskResponse>, Arc<TaskResponse>> {
        match request.request_type {
            TaskRequestType::Query => {
                let dest = file_transaction::resolve_path(handle, request, &self.dest)?;
                let snapshot = file_transaction::snapshot(handle, request, &dest)?;
                let variables = handle
                    .template
                    .get_context()
                    .read()
                    .unwrap()
                    .get_complete_blended_variables(&handle.host, BlendTarget::TemplateModule);
                let root = std::env::current_dir().map_err(|_| {
                    handle
                        .response
                        .is_failed(request, "cannot resolve template root")
                })?;
                let data = crate::playbooks::composition::render(&self.src, &root, &variables)
                    .map_err(|e| handle.response.is_failed(request, &e))?;
                let mut changes = Vec::new();
                if let Some(old) = &snapshot {
                    if old.content != data {
                        changes.push(Field::Content);
                    }
                    old.attribute_changes(&self.attributes, &mut changes);
                }
                let absent = snapshot.is_none();
                *self.prepared.lock().unwrap() = Some((dest, snapshot, data));
                if absent {
                    Ok(handle.response.needs_creation(request))
                } else if !changes.is_empty() {
                    Ok(handle.response.needs_modification(request, &changes))
                } else {
                    Ok(handle.response.is_matched(request))
                }
            }
            TaskRequestType::Create | TaskRequestType::Modify => {
                let prepared = self.prepared.lock().unwrap();
                let (dest, snapshot, data) = prepared.as_ref().ok_or_else(|| {
                    handle
                        .response
                        .is_failed(request, "template was not queried before installation")
                })?;
                file_transaction::install(
                    handle,
                    request,
                    dest,
                    data,
                    snapshot,
                    &self.attributes,
                    self.validator.as_ref(),
                )?;
                if request.request_type == TaskRequestType::Create {
                    Ok(handle.response.is_created(request))
                } else {
                    Ok(handle
                        .response
                        .is_modified(request, request.changes.clone()))
                }
            }
            _ => Err(handle.response.not_supported(request)),
        }
    }
}

#[cfg(test)]
mod transaction_tests {
    use super::*;
    use crate::handle::file_transaction::tests::local_handle;
    use crate::tasks::request::SudoDetails;
    use crate::test_support::query_request;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    #[test]
    fn query_is_readonly_apply_then_reapply_is_matched_and_validation_failure_is_unchanged() {
        let _lock = crate::test_support::ENV_LOCK.lock().unwrap();
        let root = tempfile::tempdir().unwrap();
        let src = root.path().join("source");
        let dest = root.path().join("live file's.conf");
        std::fs::write(&src, "desired\n").unwrap();
        let marker = root.path().join("validated");
        let validator = root.path().join("validator");
        std::fs::write(
            &validator,
            format!(
                "#!/bin/sh\nprintf called >> {}\ngrep -q desired \"$1\"\n",
                file_transaction::quote(marker.to_str().unwrap())
            ),
        )
        .unwrap();
        std::fs::set_permissions(&validator, std::fs::Permissions::from_mode(0o700)).unwrap();
        let task = TemplateTask {
            src: src.to_str().unwrap().into(),
            dest: dest.to_str().unwrap().into(),
            validate: Some(format!(
                "{} {{}}",
                file_transaction::quote(validator.to_str().unwrap())
            )),
            ..Default::default()
        };
        let handle = local_handle();
        let query = query_request();
        let evaluated = task
            .evaluate(&handle, &query, TemplateMode::Strict)
            .unwrap();
        assert_eq!(
            evaluated.action.dispatch(&handle, &query).unwrap().status,
            TaskStatus::NeedsCreation
        );
        assert!(!dest.exists());
        assert!(!marker.exists());
        let create = TaskRequest::create(&SudoDetails {
            user: None,
            template: String::new(),
        });
        assert_eq!(
            evaluated.action.dispatch(&handle, &create).unwrap().status,
            TaskStatus::IsCreated
        );
        assert_eq!(std::fs::read_to_string(&marker).unwrap(), "called");
        let before = std::fs::metadata(&dest).unwrap();
        let evaluated = task
            .evaluate(&handle, &query, TemplateMode::Strict)
            .unwrap();
        assert_eq!(
            evaluated.action.dispatch(&handle, &query).unwrap().status,
            TaskStatus::IsMatched
        );
        assert_eq!(before.ino(), std::fs::metadata(&dest).unwrap().ino());
        assert_eq!(std::fs::read_to_string(marker).unwrap(), "called");
        std::fs::write(&src, "invalid\n").unwrap();
        let evaluated = task
            .evaluate(&handle, &query, TemplateMode::Strict)
            .unwrap();
        let response = evaluated.action.dispatch(&handle, &query).unwrap();
        let modify = TaskRequest::modify(
            &SudoDetails {
                user: None,
                template: String::new(),
            },
            response.changes.clone(),
        );
        assert!(evaluated.action.dispatch(&handle, &modify).is_err());
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "desired\n");
        assert_eq!(before.ino(), std::fs::metadata(&dest).unwrap().ino());
    }
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;
    use crate::connection::{
        connection::Connection, factory::ConnectionFactory, local::LocalConnection,
    };
    use crate::inventory::hosts::{Host, HostOSType};
    use crate::playbooks::{
        context::PlaybookContext, task_fsm::async_run_single_task, visitor::CheckMode,
    };
    use crate::test_support::{RecordingConnection, test_handle_with_factory};
    use std::os::unix::fs::MetadataExt;
    use std::sync::RwLock;

    struct Delegate(Arc<Mutex<dyn Connection>>);
    impl ConnectionFactory for Delegate {
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
    fn delegated_plan_apply_reapply_and_failed_validation_notify_only_on_installation() {
        let _lock = crate::test_support::ENV_LOCK.lock().unwrap();
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        let live = root.path().join("live");
        std::fs::write(&source, "desired").unwrap();
        let origin: Arc<Mutex<dyn Connection>> =
            Arc::new(Mutex::new(RecordingConnection::with_rc(|_| {
                panic!("origin must not run file commands")
            })));
        let delegate_host = Arc::new(RwLock::new(Host::new("delegate")));
        let os = if cfg!(target_os = "macos") {
            HostOSType::MacOS
        } else {
            HostOSType::Linux
        };
        let delegate: Arc<Mutex<dyn Connection>> =
            Arc::new(Mutex::new(LocalConnection::new(&delegate_host)));
        let handle =
            test_handle_with_factory(origin.clone(), Arc::new(RwLock::new(Delegate(delegate))));
        handle.host.write().unwrap().os_type = Some(os);
        {
            let mut inventory = handle.run_state.inventory.write().unwrap();
            inventory.create_host("delegate");
            inventory.get_host("delegate").write().unwrap().os_type = Some(os);
        }
        let task:crate::registry::list::Task=serde_yaml::from_str(&format!("!template\nsrc: {}\ndest: {}\nvalidate: 'grep desired {{}}'\nwith:\n  delegate_to: delegate\nand:\n  notify: restart",source.display(),live.display())).unwrap();
        let play = serde_yaml::from_str("name: file lifecycle\ngroups: [all]").unwrap();
        let run = |plan, number| {
            handle.run_state.visitor.write().unwrap().check_mode =
                if plan { CheckMode::Yes } else { CheckMode::No };
            handle.run_state.context.write().unwrap().play_count = number;
            async_run_single_task(&handle.run_state, &origin, &handle.host, &play, &task)
        };
        assert_eq!(run(true, 1).unwrap().status, TaskStatus::NeedsCreation);
        assert!(!live.exists());
        assert!(!handle.host.read().unwrap().is_notified(1, "restart"));
        assert_eq!(run(false, 2).unwrap().status, TaskStatus::IsCreated);
        assert!(handle.host.read().unwrap().is_notified(2, "restart"));
        let inode = std::fs::metadata(&live).unwrap().ino();
        assert_eq!(run(false, 3).unwrap().status, TaskStatus::IsMatched);
        assert!(!handle.host.read().unwrap().is_notified(3, "restart"));
        assert_eq!(inode, std::fs::metadata(&live).unwrap().ino());
        std::fs::write(&source, "invalid").unwrap();
        assert!(run(false, 4).is_err());
        assert!(!handle.host.read().unwrap().is_notified(4, "restart"));
        assert_eq!(std::fs::read_to_string(live).unwrap(), "desired");
    }
}
