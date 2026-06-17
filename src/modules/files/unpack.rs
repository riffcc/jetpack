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

use crate::handle::handle::{CheckRc, TaskHandle};
use crate::tasks::fields::Field;
use crate::tasks::*;
use serde::Deserialize;
use std::path::Path;
use std::sync::Arc;

const MODULE: &str = "Unpack";

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct UnpackTask {
    pub name: Option<String>,
    pub src: String,
    pub dest: String,
    pub mode: Option<String>,
    pub owner: Option<String>,
    pub group: Option<String>,
    pub with: Option<PreLogicInput>,
    pub and: Option<PostLogicInput>,
}

struct UnpackAction {
    pub src: String,
    pub dest: String,
    pub mode: Option<String>,
    pub owner: Option<String>,
    pub group: Option<String>,
}

impl IsTask for UnpackTask {
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
            .path(&request, tm, &String::from("src"), &self.src)?;
        let dest = handle
            .template
            .path(&request, tm, &String::from("dest"), &self.dest)?;

        let mode = match &self.mode {
            Some(m) => Some(
                handle
                    .template
                    .string(&request, tm, &String::from("mode"), m)?,
            ),
            None => None,
        };

        let owner = match &self.owner {
            Some(o) => Some(
                handle
                    .template
                    .string(&request, tm, &String::from("owner"), o)?,
            ),
            None => None,
        };

        let group = match &self.group {
            Some(g) => Some(
                handle
                    .template
                    .string(&request, tm, &String::from("group"), g)?,
            ),
            None => None,
        };

        return Ok(EvaluatedTask {
            action: Arc::new(UnpackAction {
                src,
                dest,
                mode,
                owner,
                group,
            }),
            with: Arc::new(PreLogicInput::template(&handle, &request, tm, &self.with)?),
            and: Arc::new(PostLogicInput::template(&handle, &request, tm, &self.and)?),
        });
    }
}

impl IsAction for UnpackAction {
    fn dispatch(
        &self,
        handle: &Arc<TaskHandle>,
        request: &Arc<TaskRequest>,
    ) -> Result<Arc<TaskResponse>, Arc<TaskResponse>> {
        match request.request_type {
            TaskRequestType::Query => {
                // Check the source archive exists ON THE TARGET HOST, not the
                // control node. The archive is typically produced by a prior
                // remote task (e.g. !download), so a local std::fs check here
                // wrongly fails over SSH even when the file is really present.
                // Route the check through the connection so it works for both
                // local and remote execution.
                let check_cmd = format!("test -f '{}'", self.src);
                let exists = match handle.remote.run(&request, &check_cmd, CheckRc::Unchecked) {
                    Ok(result) => {
                        let (rc, _) = cmd_info(&result);
                        rc == 0
                    }
                    Err(_) => false,
                };
                if !exists {
                    return Err(handle.response.is_failed(
                        &request,
                        &format!("Source archive does not exist: {}", self.src),
                    ));
                }

                let mut changes = vec![Field::Content];
                if self.mode.is_some() {
                    changes.push(Field::Mode);
                }
                if self.owner.is_some() {
                    changes.push(Field::Owner);
                }
                if self.group.is_some() {
                    changes.push(Field::Group);
                }
                return Ok(handle.response.needs_modification(&request, &changes));
            }

            TaskRequestType::Modify => {
                // Create destination directory if it doesn't exist
                handle.remote.run(
                    &request,
                    &format!("mkdir -p \"{}\"", self.dest),
                    CheckRc::Checked,
                )?;

                // Determine archive type and extract
                let extract_cmd = if self.src.ends_with(".tar.gz") || self.src.ends_with(".tgz") {
                    format!("tar -xzf \"{}\" -C \"{}\"", self.src, self.dest)
                } else if self.src.ends_with(".tar.bz2") || self.src.ends_with(".tbz2") {
                    format!("tar -xjf \"{}\" -C \"{}\"", self.src, self.dest)
                } else if self.src.ends_with(".tar.xz") || self.src.ends_with(".txz") {
                    format!("tar -xJf \"{}\" -C \"{}\"", self.src, self.dest)
                } else if self.src.ends_with(".tar") {
                    format!("tar -xf \"{}\" -C \"{}\"", self.src, self.dest)
                } else if self.src.ends_with(".zip") {
                    format!("unzip -o \"{}\" -d \"{}\"", self.src, self.dest)
                } else if self.src.ends_with(".gz") && !self.src.ends_with(".tar.gz") {
                    // Single file gzip
                    let filename = Path::new(&self.src)
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .ok_or_else(|| {
                            handle
                                .response
                                .is_failed(&request, &String::from("Invalid source filename"))
                        })?;
                    format!(
                        "gunzip -c \"{}\" > \"{}/{}\"",
                        self.src, self.dest, filename
                    )
                } else if self.src.ends_with(".bz2") && !self.src.ends_with(".tar.bz2") {
                    // Single file bzip2
                    let filename = Path::new(&self.src)
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .ok_or_else(|| {
                            handle
                                .response
                                .is_failed(&request, &String::from("Invalid source filename"))
                        })?;
                    format!(
                        "bunzip2 -c \"{}\" > \"{}/{}\"",
                        self.src, self.dest, filename
                    )
                } else if self.src.ends_with(".xz") && !self.src.ends_with(".tar.xz") {
                    // Single file xz
                    let filename = Path::new(&self.src)
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .ok_or_else(|| {
                            handle
                                .response
                                .is_failed(&request, &String::from("Invalid source filename"))
                        })?;
                    format!("unxz -c \"{}\" > \"{}/{}\"", self.src, self.dest, filename)
                } else {
                    return Err(handle.response.is_failed(
                        &request,
                        &format!("Unsupported archive format: {}", self.src),
                    ));
                };

                handle
                    .remote
                    .run(&request, &extract_cmd, CheckRc::Checked)?;

                // Apply permissions if specified
                if let Some(ref mode) = self.mode {
                    handle.remote.run(
                        &request,
                        &format!("chmod -R {} \"{}\"", mode, self.dest),
                        CheckRc::Checked,
                    )?;
                }

                // Apply ownership if specified
                if self.owner.is_some() || self.group.is_some() {
                    let owner_str = match (&self.owner, &self.group) {
                        (Some(o), Some(g)) => format!("{}:{}", o, g),
                        (Some(o), None) => o.clone(),
                        (None, Some(g)) => format!(":{}", g),
                        (None, None) => unreachable!(),
                    };

                    handle.remote.run(
                        &request,
                        &format!("chown -R {} \"{}\"", owner_str, self.dest),
                        CheckRc::Checked,
                    )?;
                }

                let mut changes = vec![Field::Content];
                if self.mode.is_some() {
                    changes.push(Field::Mode);
                }
                if self.owner.is_some() {
                    changes.push(Field::Owner);
                }
                if self.group.is_some() {
                    changes.push(Field::Group);
                }
                return Ok(handle.response.is_modified(&request, changes));
            }

            _ => {
                return Err(handle.response.not_supported(request));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{RecordingConnection, query_request, test_handle};
    use std::sync::{Arc, Mutex};

    // Regression: the Query phase must check whether the source archive exists on
    // the TARGET HOST (through the connection), not on the control node's local
    // filesystem. The archive is normally produced by a prior remote task (e.g.
    // !download), so a local std::fs check fails over SSH even when the file is
    // really present. Here the path does not exist locally, but the recording
    // connection reports every command (incl. `test -f`) as succeeding — so a
    // host-aware Query succeeds and a control-node-local Query fails.
    #[test]
    fn query_checks_source_on_target_host_not_control_node() {
        let conn = RecordingConnection::new(); // every command returns rc 0
        let log = conn.command_log();
        let handle = test_handle(Arc::new(Mutex::new(conn)));
        let request = query_request();

        let action = UnpackAction {
            src: String::from("/nonexistent/on/control/node/archive.tar.gz"),
            dest: String::from("/opt/app"),
            mode: None,
            owner: None,
            group: None,
        };

        let result = action.dispatch(&handle, &request);
        assert!(
            result.is_ok(),
            "Query must consult the target host, not the control node's local filesystem",
        );

        let commands = log.lock().unwrap();
        assert!(
            commands
                .iter()
                .any(|c| c.contains("test -f") && c.contains("archive.tar.gz")),
            "expected a remote existence check (`test -f ...`); recorded commands: {:?}",
            *commands,
        );
    }
}
