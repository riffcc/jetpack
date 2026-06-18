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

use crate::connection::command::cmd_info;
use crate::handle::handle::TaskHandle;
use crate::inventory::hosts::Host;
use crate::tasks::*;
use serde::Deserialize;
use std::sync::{Arc, RwLock};

const MODULE: &str = "Shell";

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct ShellTask {
    pub name: Option<String>,
    pub cmd: String,
    pub save: Option<String>,
    pub failed_when: Option<String>,
    pub changed_when: Option<String>,
    pub shell: Option<String>, // optional shell to use (bash, zsh, sh)
    pub with: Option<PreLogicInput>,
    pub and: Option<PostLogicInput>,
}
struct ShellAction {
    pub cmd: String,
    pub save: Option<String>,
    pub failed_when: Option<String>,
    pub changed_when: Option<String>,
    pub shell: String,
}

impl IsTask for ShellTask {
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
        Ok(EvaluatedTask {
            action: Arc::new(ShellAction {
                cmd: handle.template.string_unsafe_for_shell(
                    request,
                    tm,
                    &String::from("cmd"),
                    &self.cmd,
                )?,
                save: handle.template.string_option_no_spaces(
                    request,
                    tm,
                    &String::from("save"),
                    &self.save,
                )?,
                failed_when: handle.template.string_option_unsafe_for_shell(
                    request,
                    tm,
                    &String::from("failed_when"),
                    &self.failed_when,
                )?,
                changed_when: handle.template.string_option_unsafe_for_shell(
                    request,
                    tm,
                    &String::from("changed_when"),
                    &self.changed_when,
                )?,
                shell: match &self.shell {
                    Some(s) => handle.template.string_unsafe_for_shell(
                        request,
                        tm,
                        &String::from("shell"),
                        s,
                    )?,
                    None => String::from("/bin/bash"),
                },
            }),
            with: Arc::new(PreLogicInput::template(handle, request, tm, &self.with)?),
            and: Arc::new(PostLogicInput::template(handle, request, tm, &self.and)?),
        })
    }
}

impl IsAction for ShellAction {
    fn dispatch(
        &self,
        handle: &Arc<TaskHandle>,
        request: &Arc<TaskRequest>,
    ) -> Result<Arc<TaskResponse>, Arc<TaskResponse>> {
        match request.request_type {
            TaskRequestType::Query => Ok(handle.response.needs_execution(request)),

            TaskRequestType::Execute => {
                // Unlike the command module, shell module runs commands through an actual shell
                // This allows for shell features like pipes, redirects, and shell built-ins
                let shell_cmd =
                    format!("{} -c '{}'", &self.shell, &self.cmd.replace("'", "'\"'\"'"));

                // Always use run_unsafe since we're explicitly using a shell
                let task_result =
                    handle
                        .remote
                        .run_unsafe(request, &shell_cmd, CheckRc::Unchecked)?;
                let (rc, out) = cmd_info(&task_result);
                let map_data = build_results_map(rc, &out);

                let should_fail = if let Some(condition) = &self.failed_when {
                    handle.template.test_condition_with_extra_data(
                        request,
                        TemplateMode::Strict,
                        condition,
                        &handle.host,
                        map_data.clone(),
                    )?
                } else {
                    rc != 0
                };

                let should_mark_changed = if let Some(condition) = &self.changed_when {
                    handle.template.test_condition_with_extra_data(
                        request,
                        TemplateMode::Strict,
                        condition,
                        &handle.host,
                        map_data.clone(),
                    )?
                } else {
                    true
                };

                if let Some(save) = &self.save {
                    save_results(&handle.host, save, map_data);
                }

                match should_fail {
                    true => Err(handle
                        .response
                        .command_failed(request, &Arc::clone(&task_result.command_result))),
                    false => match should_mark_changed {
                        true => Ok(task_result),
                        false => Ok(handle.response.is_passive(request)),
                    },
                }
            }

            _ => Err(handle.response.not_supported(request)),
        }
    }
}

fn build_results_map(rc: i32, out: &str) -> serde_yaml::Mapping {
    let mut result = serde_yaml::Mapping::new();
    let num: serde_yaml::Value = serde_yaml::from_str(&format!("{}", rc)).unwrap();
    result.insert(serde_yaml::Value::String(String::from("rc")), num);
    result.insert(
        serde_yaml::Value::String(String::from("out")),
        serde_yaml::Value::String(out.to_string()),
    );
    result.insert(
        serde_yaml::Value::String(String::from("stdout")),
        serde_yaml::Value::String(out.to_string()),
    );
    result
}

fn save_results(host: &Arc<RwLock<Host>>, key: &str, map_data: serde_yaml::Mapping) {
    let mut result = serde_yaml::Mapping::new();
    result.insert(
        serde_yaml::Value::String(key.to_string()),
        serde_yaml::Value::Mapping(map_data.clone()),
    );
    host.write().unwrap().update_variables(result);
}
