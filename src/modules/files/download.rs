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

use crate::tasks::*;
use crate::handle::handle::{TaskHandle, CheckRc};
use crate::tasks::fields::Field;
use serde::Deserialize;
use std::sync::Arc;

const MODULE: &str = "Download";

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct DownloadTask {
    pub name: Option<String>,
    pub url: String,
    pub dest: String,
    pub mode: Option<String>,
    pub owner: Option<String>,
    pub group: Option<String>,
    pub force: Option<String>,
    pub with: Option<PreLogicInput>,
    pub and: Option<PostLogicInput>,
}

struct DownloadAction {
    pub url: String,
    pub dest: String,
    pub mode: Option<String>,
    pub owner: Option<String>,
    pub group: Option<String>,
    pub force: bool,
}

impl IsTask for DownloadTask {
    fn get_module(&self) -> String { String::from(MODULE) }
    fn get_name(&self) -> Option<String> { self.name.clone() }
    fn get_with(&self) -> Option<PreLogicInput> { self.with.clone() }

    fn evaluate(&self, handle: &Arc<TaskHandle>, request: &Arc<TaskRequest>, tm: TemplateMode) -> Result<EvaluatedTask, Arc<TaskResponse>> {
        let url = handle.template.string(&request, tm, &String::from("url"), &self.url)?;
        let dest = handle.template.path(&request, tm, &String::from("dest"), &self.dest)?;

        let mode = match &self.mode {
            Some(m) => Some(handle.template.string(&request, tm, &String::from("mode"), m)?),
            None => None,
        };

        let owner = match &self.owner {
            Some(o) => Some(handle.template.string(&request, tm, &String::from("owner"), o)?),
            None => None,
        };

        let group = match &self.group {
            Some(g) => Some(handle.template.string(&request, tm, &String::from("group"), g)?),
            None => None,
        };

        let force = match &self.force {
            Some(f) => {
                let force_str = handle.template.string(&request, tm, &String::from("force"), f)?;
                force_str == "true" || force_str == "yes" || force_str == "1"
            },
            None => false,
        };

        return Ok(
            EvaluatedTask {
                action: Arc::new(DownloadAction {
                    url,
                    dest,
                    mode,
                    owner,
                    group,
                    force,
                }),
                with: Arc::new(PreLogicInput::template(&handle, &request, tm, &self.with)?),
                and: Arc::new(PostLogicInput::template(&handle, &request, tm, &self.and)?),
            }
        );
    }
}

impl IsAction for DownloadAction {
    fn dispatch(&self, handle: &Arc<TaskHandle>, request: &Arc<TaskRequest>) -> Result<Arc<TaskResponse>, Arc<TaskResponse>> {
        match request.request_type {
            TaskRequestType::Query => {
                // Check if file exists on the target host
                let check_cmd = format!("test -f '{}'", self.dest);
                let exists = match handle.remote.run(&request, &check_cmd, CheckRc::Unchecked) {
                    Ok(result) => {
                        let (rc, _) = cmd_info(&result);
                        rc == 0
                    },
                    Err(_) => false,
                };

                if exists && !self.force {
                    // File already exists and force is not set
                    return Ok(handle.response.is_matched(&request));
                } else {
                    // File doesn't exist or force is set
                    let mut changes = vec![Field::Content];
                    if self.mode.is_some() { changes.push(Field::Mode); }
                    if self.owner.is_some() { changes.push(Field::Owner); }
                    if self.group.is_some() { changes.push(Field::Group); }
                    return Ok(handle.response.needs_modification(&request, &changes));
                }
            },

            TaskRequestType::Modify => {
                // Create parent directory on target if needed
                if let Some(parent) = std::path::Path::new(&self.dest).parent() {
                    if let Some(parent_str) = parent.to_str() {
                        if !parent_str.is_empty() {
                            let mkdir_cmd = format!("mkdir -p '{}'", parent_str);
                            handle.remote.run(&request, &mkdir_cmd, CheckRc::Checked)?;
                        }
                    }
                }

                // Check if curl is available (required for !download)
                let has_curl = match handle.remote.run(&request, &String::from("command -v curl"), CheckRc::Unchecked) {
                    Ok(result) => {
                        let (rc, _) = cmd_info(&result);
                        rc == 0
                    },
                    Err(_) => false,
                };

                if !has_curl {
                    return Err(handle.response.is_failed(&request,
                        &String::from("curl is required for !download but not found. Install curl first: apt install curl")));
                }

                // Download using curl on the target host
                let download_cmd = format!(
                    "curl -fsSL -o '{}' '{}'",
                    self.dest, self.url
                );
                handle.remote.run(&request, &download_cmd, CheckRc::Checked)?;

                // Apply permissions if specified
                if let Some(ref mode) = self.mode {
                    let chmod_cmd = format!("chmod {} '{}'", mode, self.dest);
                    handle.remote.run(&request, &chmod_cmd, CheckRc::Checked)?;
                }

                // Apply ownership if specified
                if self.owner.is_some() || self.group.is_some() {
                    let owner_str = match (&self.owner, &self.group) {
                        (Some(o), Some(g)) => format!("{}:{}", o, g),
                        (Some(o), None) => o.clone(),
                        (None, Some(g)) => format!(":{}", g),
                        (None, None) => unreachable!(),
                    };
                    let chown_cmd = format!("chown {} '{}'", owner_str, self.dest);
                    handle.remote.run(&request, &chown_cmd, CheckRc::Checked)?;
                }

                let mut changes = vec![Field::Content];
                if self.mode.is_some() { changes.push(Field::Mode); }
                if self.owner.is_some() { changes.push(Field::Owner); }
                if self.group.is_some() { changes.push(Field::Group); }
                return Ok(handle.response.is_modified(&request, changes));
            }

            _ => { return Err(handle.response.not_supported(request)); }
        }
    }
}
