// Jetpack
// Copyright (C) 2026 - Riff.CC <code@riff.cc>
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

//! `!fetch` module — transfers a file from the remote host to the control machine.
//!
//! The inverse of `!copy`. Fetched content is stored in `PlaybookResult.fetched_files`
//! (keyed by remote path) for programmatic access by callers using Jetpack as a crate.
//! When `dest` is set the bytes are also written to that local path.
//!
//! # Example
//! ```yaml
//! - !fetch
//!   name: Retrieve generated password
//!   src: /var/lib/dragonfly/initial_password.txt
//!   dest: /tmp/dragonfly-password.txt   # optional
//! ```

use crate::tasks::*;
use crate::handle::handle::TaskHandle;
use crate::tasks::fields::Field;
use std::path::Path;
use serde::Deserialize;
use std::sync::Arc;
use std::vec::Vec;

const MODULE: &str = "fetch";

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct FetchTask {
    pub name: Option<String>,
    /// Remote path to fetch.
    pub src: String,
    /// Local destination path. When set, fetched bytes are written here.
    /// Leave empty to capture content only via `PlaybookResult.fetched_files`.
    pub dest: Option<String>,
    pub with: Option<PreLogicInput>,
    pub and: Option<PostLogicInput>,
}

struct FetchAction {
    pub src: String,
    pub dest: Option<String>,
}

impl IsTask for FetchTask {
    fn get_module(&self) -> String { String::from(MODULE) }
    fn get_name(&self) -> Option<String> { self.name.clone() }
    fn get_with(&self) -> Option<PreLogicInput> { self.with.clone() }

    fn evaluate(&self, handle: &Arc<TaskHandle>, request: &Arc<TaskRequest>, tm: TemplateMode) -> Result<EvaluatedTask, Arc<TaskResponse>> {
        let dest = match &self.dest {
            Some(d) => Some(handle.template.path(request, tm, &String::from("dest"), d)?),
            None => None,
        };
        return Ok(EvaluatedTask {
            action: Arc::new(FetchAction {
                src: handle.template.path(request, tm, &String::from("src"), &self.src)?,
                dest,
            }),
            with: Arc::new(PreLogicInput::template(handle, request, tm, &self.with)?),
            and: Arc::new(PostLogicInput::template(handle, request, tm, &self.and)?),
        });
    }
}

impl IsAction for FetchAction {
    fn dispatch(&self, handle: &Arc<TaskHandle>, request: &Arc<TaskRequest>) -> Result<Arc<TaskResponse>, Arc<TaskResponse>> {
        match request.request_type {

            TaskRequestType::Query => {
                // Verify the remote file exists.
                let remote_512 = handle.remote.get_sha512(request, &self.src)?;
                if remote_512.is_empty() {
                    return Err(handle.response.is_failed(request, &format!("remote file does not exist: {}", self.src)));
                }

                // If no local dest, always fetch (no idempotency check possible).
                let dest = match &self.dest {
                    None => return Ok(handle.response.needs_creation(request)),
                    Some(d) => d,
                };

                // Check whether local dest exists and matches the remote checksum.
                let local_dest = Path::new(dest);
                let local_512 = handle.local.get_sha512(request, local_dest, false)?;
                if local_512.is_empty() {
                    return Ok(handle.response.needs_creation(request));
                }

                let mut changes: Vec<Field> = Vec::new();
                if !remote_512.eq(&local_512) {
                    changes.push(Field::Content);
                }
                if !changes.is_empty() {
                    return Ok(handle.response.needs_modification(request, &changes));
                }
                Ok(handle.response.is_matched(request))
            }

            TaskRequestType::Create | TaskRequestType::Modify => {
                let content = handle.remote.fetch_file(request, &self.src)?;
                if let Some(dest) = &self.dest {
                    // Ensure parent directory exists.
                    let dest_path = Path::new(dest);
                    if let Some(parent) = dest_path.parent() {
                        if !parent.as_os_str().is_empty() && !parent.exists() {
                            std::fs::create_dir_all(parent).map_err(|e| {
                                handle.response.is_failed(request, &format!("mkdir failed for {}: {}", parent.display(), e))
                            })?;
                        }
                    }
                    std::fs::write(dest_path, &content).map_err(|e| {
                        handle.response.is_failed(request, &format!("write to '{}' failed: {}", dest, e))
                    })?;
                }
                match request.request_type {
                    TaskRequestType::Create => Ok(handle.response.is_created(request)),
                    _                       => Ok(handle.response.is_modified(request, request.changes.clone())),
                }
            }

            _ => Err(handle.response.not_supported(request)),
        }
    }
}
