// Copyright (C) Riff Labs <team@riff.cc>
// SPDX-License-Identifier: GPL-3.0-or-later

use super::xml_edit::{self, Namespaces};
use crate::handle::{
    file_transaction::{self, Snapshot},
    handle::TaskHandle,
    template::BlendTarget,
};
use crate::tasks::fields::Field;
use crate::tasks::*;
use serde::Deserialize;
use serde_json::Value;
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

#[derive(Deserialize, Debug, Default)]
#[serde(deny_unknown_fields)]
pub struct XmlTask {
    pub name: Option<String>,
    pub path: String,
    #[serde(default)]
    pub set: BTreeMap<String, Value>,
    #[serde(default)]
    pub remove: Vec<String>,
    #[serde(default)]
    pub create: bool,
    #[serde(default)]
    pub namespaces: Namespaces,
    pub attributes: Option<FileAttributesInput>,
    pub with: Option<PreLogicInput>,
    pub retry: Option<String>,
    pub delay: Option<String>,
    pub and: Option<PostLogicInput>,
}
struct XmlAction {
    path: String,
    set: BTreeMap<String, Value>,
    remove: Vec<String>,
    create: bool,
    namespaces: Namespaces,
    attributes: Option<FileAttributesEvaluated>,
    prepared: Mutex<Option<(Option<Snapshot>, String)>>,
}
impl IsTask for XmlTask {
    fn get_module(&self) -> String {
        "xml".into()
    }
    fn get_name(&self) -> Option<String> {
        self.name.clone()
    }
    fn get_with(&self) -> Option<PreLogicInput> {
        self.with.clone()
    }
    fn name_variable_writes(&self) -> crate::tasks::naming::NameVariableWrites {
        crate::tasks::naming::NameVariableWrites::None
    }
    fn generated_name(&self, ctx: &crate::tasks::naming::NamingContext<'_>) -> Option<String> {
        Some(ctx.resource("Edit", &self.path, "Edit XML settings"))
    }
    fn evaluate(
        &self,
        handle: &Arc<TaskHandle>,
        request: &Arc<TaskRequest>,
        tm: TemplateMode,
    ) -> Result<EvaluatedTask, Arc<TaskResponse>> {
        let path = handle.template.path(request, tm, "path", &self.path)?;
        let mut set = self.set.clone();
        if tm != TemplateMode::Off {
            file_transaction::absolute_path(&path)
                .map_err(|e| handle.response.is_failed(request, &e))?;
            if set.is_empty() && self.remove.is_empty() {
                return Err(handle
                    .response
                    .is_failed(request, "xml requires set or remove operations"));
            }
            let variables = handle
                .template
                .get_context()
                .read()
                .unwrap()
                .get_complete_blended_variables(&handle.host, BlendTarget::TemplateModule);
            let templar = crate::playbooks::templar::Templar::new();
            for value in set.values_mut() {
                *value = templar.render_value(value, &variables, tm).map_err(|_| {
                    handle
                        .response
                        .is_failed(request, "XML value template failed (value withheld)")
                })?;
            }
        }
        Ok(EvaluatedTask {
            action: Arc::new(XmlAction {
                path,
                set,
                remove: self.remove.clone(),
                create: self.create,
                namespaces: self.namespaces.clone(),
                attributes: FileAttributesInput::template(handle, request, tm, &self.attributes)?,
                prepared: Mutex::new(None),
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
impl IsAction for XmlAction {
    fn dispatch(
        &self,
        handle: &Arc<TaskHandle>,
        request: &Arc<TaskRequest>,
    ) -> Result<Arc<TaskResponse>, Arc<TaskResponse>> {
        match request.request_type {
            TaskRequestType::Query => {
                let old = file_transaction::snapshot(handle, request, &self.path)?;
                let output = xml_edit::edit(
                    old.as_ref().map(|s| s.content.as_str()),
                    &self.set,
                    &self.remove,
                    self.create,
                    &self.namespaces,
                )
                .map_err(|e| handle.response.is_failed(request, &e))?;
                let mut changes = Vec::new();
                let absent = old.is_none();
                if let Some(old) = &old {
                    if old.content != output {
                        changes.push(Field::Content);
                    }
                    old.attribute_changes(&self.attributes, &mut changes);
                }
                *self.prepared.lock().unwrap() = Some((old, output));
                if absent {
                    Ok(handle.response.needs_creation(request))
                } else if changes.is_empty() {
                    Ok(handle.response.is_matched(request))
                } else {
                    Ok(handle.response.needs_modification(request, &changes))
                }
            }
            TaskRequestType::Create | TaskRequestType::Modify => {
                let prepared = self.prepared.lock().unwrap();
                let (old, output) = prepared.as_ref().ok_or_else(|| {
                    handle
                        .response
                        .is_failed(request, "XML task must be queried before installing")
                })?;
                file_transaction::install(
                    handle,
                    request,
                    &self.path,
                    output,
                    old,
                    &self.attributes,
                    None,
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
mod tests {
    use super::*;
    use crate::handle::file_transaction::tests::local_handle;
    use crate::tasks::request::SudoDetails;
    use crate::test_support::query_request;
    use serde_json::json;
    use std::os::unix::fs::{MetadataExt, PermissionsExt};

    fn task(path: &std::path::Path, values: Value, create: bool) -> XmlTask {
        XmlTask {
            path: path.to_str().unwrap().into(),
            set: serde_json::from_value(values).unwrap(),
            create,
            ..Default::default()
        }
    }
    fn modify(
        handle: &Arc<TaskHandle>,
        action: &Arc<dyn IsAction>,
        changes: Vec<Field>,
    ) -> Arc<TaskResponse> {
        action
            .dispatch(
                handle,
                &TaskRequest::modify(
                    &SudoDetails {
                        user: None,
                        template: String::new(),
                    },
                    changes,
                ),
            )
            .unwrap()
    }
    #[test]
    fn typed_values_and_parallel_hosts_preserve_their_own_expected_lists() {
        let _lock = crate::test_support::ENV_LOCK.lock().unwrap();
        let workers:Vec<_>=(0..8).map(|id|std::thread::spawn(move ||{
            let root=tempfile::tempdir().unwrap();let path=root.path().join("network.xml");
            std::fs::write(&path,"<Network><Port>0</Port><Enabled>false</Enabled><KnownProxies/></Network>").unwrap();
            let task=task(&path,json!({"/Network/Port":"{{ port }}","/Network/Enabled":"{{ enabled }}","/Network/KnownProxies":{"children":{"string":"{{ proxies }}"}}}),true);
            let handle=local_handle();
            let data=json!({"port":8000+id,"enabled":true,"proxies":[format!("proxy-{id}-a"),format!("proxy-{id}-b")]});
            handle.host.write().unwrap().update_variables(serde_yaml::from_value(serde_yaml::to_value(data).unwrap()).unwrap());
            let query=query_request();let evaluated=task.evaluate(&handle,&query,TemplateMode::Strict).unwrap();
            let result=evaluated.action.dispatch(&handle,&query).unwrap();assert_eq!(result.status,TaskStatus::NeedsModification);
            let before=std::fs::read_to_string(&path).unwrap();assert!(before.contains("<Port>0</Port>"));
            assert_eq!(modify(&handle,&evaluated.action,result.changes.clone()).status,TaskStatus::IsModified);
            let content=std::fs::read_to_string(&path).unwrap();
            assert!(content.contains(&format!("<Port>{}</Port>",8000+id)));assert!(content.contains("<Enabled>true</Enabled>"));
            assert!(content.contains(&format!("<string>proxy-{id}-a</string><string>proxy-{id}-b</string>")));
            let evaluated=task.evaluate(&handle,&query,TemplateMode::Strict).unwrap();
            assert_eq!(evaluated.action.dispatch(&handle,&query).unwrap().status,TaskStatus::IsMatched);
        })).collect();
        for worker in workers {
            worker.join().unwrap();
        }
    }
    #[test]
    fn plan_apply_reapply_preserves_application_changes_and_explicit_metadata() {
        let _lock = crate::test_support::ENV_LOCK.lock().unwrap();
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("network.xml");
        let source = "<?xml version='1.0'?>\n<Network><!-- keep --><Port>0</Port><Application>old</Application></Network>\n";
        std::fs::write(&path, source).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();
        let mut task = task(&path, json!({"/Network/Port":8096}), false);
        task.attributes = Some(FileAttributesInput {
            mode: Some("0600".into()),
            owner: None,
            group: None,
        });
        let handle = local_handle();
        let query = query_request();
        let evaluated = task
            .evaluate(&handle, &query, TemplateMode::Strict)
            .unwrap();
        let result = evaluated.action.dispatch(&handle, &query).unwrap();
        assert_eq!(result.status, TaskStatus::NeedsModification);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), source);
        assert_eq!(std::fs::metadata(&path).unwrap().mode() & 0o777, 0o640);
        modify(&handle, &evaluated.action, result.changes.clone());
        assert_eq!(std::fs::metadata(&path).unwrap().mode() & 0o777, 0o600);
        let content = std::fs::read_to_string(&path).unwrap().replace(
            "<Application>old</Application>",
            "<Application>app-edited</Application>",
        );
        std::fs::write(&path, &content).unwrap();
        let inode = std::fs::metadata(&path).unwrap().ino();
        let evaluated = task
            .evaluate(&handle, &query, TemplateMode::Strict)
            .unwrap();
        assert_eq!(
            evaluated.action.dispatch(&handle, &query).unwrap().status,
            TaskStatus::IsMatched
        );
        assert_eq!(std::fs::metadata(&path).unwrap().ino(), inode);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), content);
        // Another application change survives a later selected-field mutation too.
        task.set.insert("/Network/Port".into(), json!(9000));
        let evaluated = task
            .evaluate(&handle, &query, TemplateMode::Strict)
            .unwrap();
        let result = evaluated.action.dispatch(&handle, &query).unwrap();
        modify(&handle, &evaluated.action, result.changes.clone());
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            content.replace("8096", "9000")
        );
    }
    #[test]
    fn failures_are_transactional_and_concurrent_application_writes_are_not_overwritten() {
        let _lock = crate::test_support::ENV_LOCK.lock().unwrap();
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("network.xml");
        std::fs::write(&path, "<Root><A>old</A><Other>app</Other></Root>").unwrap();
        let handle = local_handle();
        let query = query_request();
        let before = std::fs::read_to_string(&path).unwrap();
        let inode = std::fs::metadata(&path).unwrap().ino();
        for values in [
            json!({"/Root/A":"new","/Root/Z/Missing":true}),
            json!({"/Root/A":"new","/Root/Other":null}),
        ] {
            let evaluated = task(&path, values, false)
                .evaluate(&handle, &query, TemplateMode::Strict)
                .unwrap();
            assert!(evaluated.action.dispatch(&handle, &query).is_err());
            assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
        }
        assert_eq!(inode, std::fs::metadata(&path).unwrap().ino());
        let evaluated = task(&path, json!({"/Root/A":"new"}), false)
            .evaluate(&handle, &query, TemplateMode::Strict)
            .unwrap();
        let result = evaluated.action.dispatch(&handle, &query).unwrap();
        let concurrent = before.replace("app", "concurrent");
        std::fs::write(&path, &concurrent).unwrap();
        assert!(
            evaluated
                .action
                .dispatch(
                    &handle,
                    &TaskRequest::modify(
                        &SudoDetails {
                            user: None,
                            template: String::new()
                        },
                        result.changes.clone()
                    )
                )
                .is_err()
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), concurrent);
        assert!(!std::fs::read_dir(root.path()).unwrap().any(|e| {
            e.unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".jetpack-candidate")
        }));
    }
    #[test]
    fn missing_file_creation_is_explicit_and_skipped_evaluation_defers_templates() {
        let _lock = crate::test_support::ENV_LOCK.lock().unwrap();
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("new.xml");
        let handle = local_handle();
        let query = query_request();
        let mut task = task(&path, json!({"/Root/Enabled":true}), false);
        let evaluated = task
            .evaluate(&handle, &query, TemplateMode::Strict)
            .unwrap();
        assert!(evaluated.action.dispatch(&handle, &query).is_err());
        task.create = true;
        let evaluated = task
            .evaluate(&handle, &query, TemplateMode::Strict)
            .unwrap();
        assert_eq!(
            evaluated.action.dispatch(&handle, &query).unwrap().status,
            TaskStatus::NeedsCreation
        );
        assert!(!path.exists());
        let create = TaskRequest::create(&SudoDetails {
            user: None,
            template: String::new(),
        });
        evaluated.action.dispatch(&handle, &create).unwrap();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "<Root><Enabled>true</Enabled></Root>"
        );
        task.set
            .insert("/Root/Enabled".into(), json!("{{ undefined_secret }}"));
        assert!(task.evaluate(&handle, &query, TemplateMode::Off).is_ok());
        let error = match task.evaluate(&handle, &query, TemplateMode::Strict) {
            Err(error) => error,
            _ => panic!("missing variable should fail"),
        };
        assert!(!format!("{error:?}").contains("undefined_secret"));
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
    fn delegated_xml_plan_apply_reapply_notifies_only_for_installed_changes() {
        let _lock = crate::test_support::ENV_LOCK.lock().unwrap();
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("network.xml");
        std::fs::write(
            &path,
            "<Network><Enabled>false</Enabled><Application>keep</Application></Network>",
        )
        .unwrap();
        let origin: Arc<Mutex<dyn Connection>> =
            Arc::new(Mutex::new(RecordingConnection::with_rc(|_| {
                panic!("XML must use delegate")
            })));
        let delegate_host = Arc::new(RwLock::new(Host::new("delegate")));
        let delegate: Arc<Mutex<dyn Connection>> =
            Arc::new(Mutex::new(LocalConnection::new(&delegate_host)));
        let handle =
            test_handle_with_factory(origin.clone(), Arc::new(RwLock::new(Delegate(delegate))));
        let os = if cfg!(target_os = "macos") {
            HostOSType::MacOS
        } else {
            HostOSType::Linux
        };
        handle.host.write().unwrap().os_type = Some(os);
        {
            let mut inventory = handle.run_state.inventory.write().unwrap();
            inventory.create_host("delegate");
            inventory.get_host("delegate").write().unwrap().os_type = Some(os);
        }
        let task:crate::registry::list::Task=serde_yaml::from_str(&format!("!xml\npath: {}\nset:\n  /Network/Enabled: true\nwith:\n  delegate_to: delegate\nand:\n  notify: restart",path.display())).unwrap();
        let play = serde_yaml::from_str("name: XML lifecycle\ngroups: [all]").unwrap();
        let run = |plan, number| {
            handle.run_state.visitor.write().unwrap().check_mode =
                if plan { CheckMode::Yes } else { CheckMode::No };
            handle.run_state.context.write().unwrap().play_count = number;
            async_run_single_task(&handle.run_state, &origin, &handle.host, &play, &task)
        };
        assert_eq!(run(true, 1).unwrap().status, TaskStatus::NeedsModification);
        assert!(std::fs::read_to_string(&path).unwrap().contains("false"));
        assert!(!handle.host.read().unwrap().is_notified(1, "restart"));
        assert_eq!(run(false, 2).unwrap().status, TaskStatus::IsModified);
        assert!(handle.host.read().unwrap().is_notified(2, "restart"));
        assert_eq!(run(false, 3).unwrap().status, TaskStatus::IsMatched);
        assert!(!handle.host.read().unwrap().is_notified(3, "restart"));
        std::fs::write(
            &path,
            "<Network><Enabled>false</Enabled><Enabled>true</Enabled></Network>",
        )
        .unwrap();
        assert!(run(false, 4).is_err());
        assert!(!handle.host.read().unwrap().is_notified(4, "restart"));
    }
}
