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

use crate::handle::handle::TaskHandle;
use crate::handle::template::BlendTarget;
use crate::playbooks::templar::TemplateMode;
use crate::tasks::request::TaskRequest;
use crate::tasks::response::TaskResponse;
use serde::Deserialize;
use std::sync::Arc;

// this is storage behind all 'and' and 'with' statements in the program, which
// are mostly implemented in task_fsm

#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct PreLogicInput {
    pub condition: Option<String>,
    pub subscribe: Option<String>,
    pub sudo: Option<String>,
    pub items: Option<ItemsInput>,
    pub tags: Option<Vec<String>>,
    pub delegate_to: Option<String>,
    pub skip_if_exists: Option<String>,
}

#[derive(Deserialize, Debug, Clone)]
#[serde(untagged)]
pub enum ItemsInput {
    ItemsString(String),
    ItemsList(Vec<String>),
}

#[derive(Debug)]
pub struct PreLogicEvaluated {
    pub condition: Option<String>, // this is not evaluated here
    pub subscribe: Option<String>,
    pub sudo: Option<String>,
    pub items: Option<ItemsInput>,
    pub tags: Option<Vec<String>>,
    pub skip_if_exists: Option<String>,
}

#[derive(Deserialize, Debug, Default)]
#[serde(deny_unknown_fields)]
pub struct PostLogicInput {
    pub notify: Option<String>,
    pub ignore_errors: Option<String>,
}

#[derive(Debug)]
pub struct PostLogicEvaluated {
    pub notify: Option<String>,
    pub ignore_errors: bool,
    pub retry: u64,
    pub delay: u64,
}

impl PreLogicInput {
    pub fn template(
        handle: &TaskHandle,
        request: &Arc<TaskRequest>,
        tm: TemplateMode,
        input: &Option<Self>,
    ) -> Result<Option<PreLogicEvaluated>, Arc<TaskResponse>> {
        if input.is_none() {
            return Ok(None);
        }
        let input2 = input.as_ref().unwrap();
        Ok(Some(PreLogicEvaluated {
            condition: input2.condition.clone(),
            sudo: handle.template.string_option_no_spaces(
                request,
                tm,
                &String::from("sudo"),
                &input2.sudo,
            )?,
            subscribe: handle
                .template
                .no_template_string_option_trim(&input2.subscribe),
            items: input2.items.clone(),
            tags: input2.tags.clone(),
            skip_if_exists: input2.skip_if_exists.clone(), // Don't template here - do it in FSM
        }))
    }
}

impl PostLogicInput {
    pub fn template(
        handle: &TaskHandle,
        request: &Arc<TaskRequest>,
        tm: TemplateMode,
        input: &Option<Self>,
        retry: &Option<String>,
        delay: &Option<String>,
    ) -> Result<Option<PostLogicEvaluated>, Arc<TaskResponse>> {
        if input.is_none() && retry.is_none() && delay.is_none() {
            return Ok(None);
        }
        let defaults = Self::default();
        let input2 = input.as_ref().unwrap_or(&defaults);
        Ok(Some(PostLogicEvaluated {
            notify: handle.template.string_option_trim(
                request,
                tm,
                &String::from("notify"),
                &input2.notify,
            )?,
            // unsafe here means the options cannot be sent to the shell, which they are not.
            delay: handle.template.integer_option_to_integer(
                request,
                tm,
                &String::from("delay"),
                delay,
                1,
            )?,
            ignore_errors: handle.template.boolean_option_default_false(
                request,
                tm,
                &String::from("ignore_errors"),
                &input2.ignore_errors,
            )?,
            retry: handle.template.integer_option_to_integer(
                request,
                tm,
                &String::from("retry"),
                retry,
                0,
            )?,
        }))
    }
}

/* this is called from the task_fsm, not above */
pub fn template_items(
    handle: &Arc<TaskHandle>,
    request: &Arc<TaskRequest>,
    tm: TemplateMode,
    items_input: &Option<ItemsInput>,
) -> Result<Vec<serde_yaml::Value>, Arc<TaskResponse>> {
    match items_input {
        None => Ok(empty_items_vector()),

        // with/items: varname
        Some(ItemsInput::ItemsString(x)) => {
            let blended = handle
                .run_state
                .context
                .read()
                .unwrap()
                .get_complete_blended_variables(&handle.host, BlendTarget::NotTemplateModule);
            match blended.contains_key(x) {
                true => {
                    let value: serde_yaml::Value = blended.get(x).unwrap().clone();
                    match value {
                        serde_yaml::Value::Sequence(vs) => {
                            template_serde_sequence(handle, request, tm, vs)
                        }
                        _ => Err(handle
                            .response
                            .is_failed(request, "with/items variable did not resolve to a list")),
                    }
                }
                false => Err(handle
                    .response
                    .is_failed(request, &format!("variable not found for items: {}", x))),
            }
        }
        Some(ItemsInput::ItemsList(x)) => {
            let mut output: Vec<serde_yaml::Value> = Vec::new();
            for item in x.iter() {
                output.push(serde_yaml::Value::String(handle.template.string(
                    request,
                    tm,
                    &String::from("items"),
                    item,
                )?));
            }
            Ok(output)
        }
    }
}

pub fn empty_items_vector() -> Vec<serde_yaml::Value> {
    vec![serde_yaml::Value::Bool(true)]
}

pub fn template_serde_sequence(
    handle: &TaskHandle,
    request: &Arc<TaskRequest>,
    tm: TemplateMode,
    vs: serde_yaml::Sequence,
) -> Result<Vec<serde_yaml::Value>, Arc<TaskResponse>> {
    let mut output: Vec<serde_yaml::Value> = Vec::new();

    for seq_item in vs.iter() {
        match seq_item {
            serde_yaml::Value::String(x) => output.push(serde_yaml::Value::String(
                handle
                    .template
                    .string(request, tm, &String::from("items"), x)?,
            )),
            x => output.push(x.clone()),
        }
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_skip_if_exists_not_templated_in_evaluation() {
        // PreLogicInput should store skip_if_exists as-is, without templating
        let input = PreLogicInput {
            condition: None,
            subscribe: None,
            sudo: None,
            items: None,
            tags: None,
            delegate_to: None,
            skip_if_exists: Some("/home/{{ user }}/.config".to_string()),
        };

        // The evaluated version should have the same raw string
        // (templating happens later in the FSM)
        assert_eq!(
            input.skip_if_exists,
            Some("/home/{{ user }}/.config".to_string())
        );
    }
}

#[cfg(test)]
mod retry_tests {
    use super::*;
    use crate::registry::list::Task;
    use crate::test_support::{RecordingConnection, query_request, test_handle};
    use std::sync::Mutex;

    fn evaluate(yaml: &str) -> Option<PostLogicEvaluated> {
        let task: Task = serde_yaml::from_str(yaml).unwrap();
        let handle = test_handle(Arc::new(Mutex::new(RecordingConnection::new())));
        let evaluated = task
            .evaluate(&handle, &query_request(), TemplateMode::Strict)
            .unwrap();
        Arc::try_unwrap(evaluated.and).unwrap()
    }

    #[test]
    fn retry_controls_work_without_and_on_different_module_categories() {
        for yaml in [
            "!http\nurl: https://example.com\nretry: 3\ndelay: 0",
            "!command\ncmd: true\nretry: 3\ndelay: 0",
            "!apt\npackage: example\nretry: 3\ndelay: 0",
            "!file\npath: /tmp/example\nretry: 3\ndelay: 0",
            "!self_locate\nsave: location\nretry: 3\ndelay: 0",
        ] {
            let result = evaluate(yaml).unwrap();
            assert_eq!((result.retry, result.delay), (3, 0), "{yaml}");
            assert!(!result.ignore_errors);
            assert!(result.notify.is_none());
        }
    }

    #[test]
    fn retry_controls_preserve_defaults_and_other_post_logic() {
        assert!(evaluate("!command\ncmd: true").is_none());
        let result = evaluate(
            "!command\ncmd: true\nretry: 2\nand:\n  notify: restart\n  ignore_errors: true",
        )
        .unwrap();
        assert_eq!((result.retry, result.delay), (2, 1));
        assert!(result.ignore_errors);
        assert_eq!(result.notify.as_deref(), Some("restart"));
        let result = evaluate("!command\ncmd: true\ndelay: 7").unwrap();
        assert_eq!((result.retry, result.delay), (0, 7));
    }

    #[test]
    fn old_nested_retry_controls_are_rejected() {
        for field in ["retry", "delay"] {
            let yaml = format!("!command\ncmd: true\nand:\n  {field}: 2");
            let error = serde_yaml::from_str::<Task>(&yaml).unwrap_err().to_string();
            assert!(error.contains("unknown field"), "{error}");
        }
    }

    #[test]
    fn retry_controls_defer_validation_and_render_per_host_and_item() {
        let task: Task =
            serde_yaml::from_str("!command\ncmd: true\nretry: '{{ item }}'\ndelay: '{{ pause }}'")
                .unwrap();
        let threads: Vec<_> = (0..8)
            .map(|n| {
                std::thread::spawn(move || {
                    let task: Task = serde_yaml::from_str(
                        "!command\ncmd: true\nretry: '{{ item }}'\ndelay: '{{ pause }}'",
                    )
                    .unwrap();
                    let handle = test_handle(Arc::new(Mutex::new(RecordingConnection::new())));
                    assert!(
                        task.evaluate(&handle, &query_request(), TemplateMode::Off)
                            .is_ok()
                    );
                    for item in [n, n + 8] {
                        handle.host.write().unwrap().update_facts2(
                            serde_yaml::from_str(&format!("item: {item}\npause: {n}")).unwrap(),
                        );
                        let evaluated = task
                            .evaluate(&handle, &query_request(), TemplateMode::Strict)
                            .unwrap();
                        let result = evaluated.and.as_ref().as_ref().unwrap();
                        assert_eq!((result.retry, result.delay), (item, n));
                    }
                })
            })
            .collect();
        let handle = test_handle(Arc::new(Mutex::new(RecordingConnection::new())));
        assert!(
            task.evaluate(&handle, &query_request(), TemplateMode::Strict)
                .is_err()
        );
        for thread in threads {
            thread.join().unwrap();
        }
        for field in ["retry", "delay"] {
            for value in ["-1", "1.5", "invalid"] {
                let task: Task =
                    serde_yaml::from_str(&format!("!command\ncmd: true\n{field}: '{value}'"))
                        .unwrap();
                assert!(
                    task.evaluate(&handle, &query_request(), TemplateMode::Off)
                        .is_ok()
                );
                assert!(
                    task.evaluate(&handle, &query_request(), TemplateMode::Strict)
                        .is_err()
                );
            }
        }
    }
}
