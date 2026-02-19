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
use crate::handle::handle::TaskHandle;
use crate::tasks::fields::Field;
use std::path::{Path, PathBuf};
use serde::{Deserialize};
use std::sync::Arc;
use std::vec::Vec;
use crate::tasks::files::Recurse;

const MODULE: &str = "copy";

#[derive(Deserialize,Debug)]
#[serde(deny_unknown_fields)]
pub struct CopyTask {
    pub name: Option<String>,
    pub src: String,
    pub dest: String,
    pub recursive: Option<bool>,
    pub attributes: Option<FileAttributesInput>,
    pub with: Option<PreLogicInput>,
    pub and: Option<PostLogicInput>
}
struct CopyAction {
    pub src: PathBuf,
    pub dest: String,
    pub recursive: bool,
    pub attributes: Option<FileAttributesEvaluated>,
}

impl IsTask for CopyTask {

    fn get_module(&self) -> String { String::from(MODULE) }
    fn get_name(&self) -> Option<String> { self.name.clone() }
    fn get_with(&self) -> Option<PreLogicInput> { self.with.clone() }

    fn evaluate(&self, handle: &Arc<TaskHandle>, request: &Arc<TaskRequest>, tm: TemplateMode) -> Result<EvaluatedTask, Arc<TaskResponse>> {
        let src_str = handle.template.string(&request, tm, &String::from("src"), &self.src)?;
        let recursive = self.recursive.unwrap_or(false);

        let src_path = if recursive {
            if tm == TemplateMode::Off {
                // During pre-evaluation traversal, src_str is a sentinel ("empty") —
                // skip the filesystem check and return a placeholder, consistent with
                // how find_sub_path and every other typed helper handles Off mode.
                PathBuf::new()
            } else {
                // Recursive mode: src must be a directory; resolve it directly without
                // the files/ prefix that find_file_path would add for relative paths.
                let p = PathBuf::from(&src_str);
                if !p.is_dir() {
                    return Err(handle.response.is_failed(
                        request,
                        &format!("src '{}': not a directory; recursive: true requires a directory source", src_str),
                    ));
                }
                p
            }
        } else {
            handle.template.find_file_path(request, tm, &String::from("src"), &src_str)?
        };

        return Ok(
            EvaluatedTask {
                action: Arc::new(CopyAction {
                    src: src_path,
                    dest:       handle.template.path(&request, tm, &String::from("dest"), &self.dest)?,
                    recursive,
                    attributes: FileAttributesInput::template(&handle, &request, tm, &self.attributes)?
                }),
                with: Arc::new(PreLogicInput::template(&handle, &request, tm, &self.with)?),
                and: Arc::new(PostLogicInput::template(&handle, &request, tm, &self.and)?),
            }
        );
    }

}

impl IsAction for CopyAction {

    fn dispatch(&self, handle: &Arc<TaskHandle>, request: &Arc<TaskRequest>) -> Result<Arc<TaskResponse>, Arc<TaskResponse>> {

        match request.request_type {

            TaskRequestType::Query => {
                if self.recursive {
                    // For recursive directory copy: check whether the remote directory
                    // already exists.  If it does we treat it as matched (idempotent);
                    // if not, we need to create the whole tree.
                    match handle.remote.get_is_directory(request, &self.dest) {
                        Ok(true)  => return Ok(handle.response.is_matched(request)),
                        Ok(false) => return Ok(handle.response.needs_creation(request)),
                        Err(e)    => return Err(e),
                    }
                }

                let mut changes : Vec<Field> = Vec::new();
                let remote_mode = handle.remote.query_common_file_attributes(request, &self.dest, &self.attributes, &mut changes, Recurse::No)?;
                if remote_mode.is_none() {
                    return Ok(handle.response.needs_creation(request));
                }
                // this query leg is (at least originally) the same as the template module query except these two lines
                // to calculate the checksum differently
                let src_path = self.src.as_path();
                let local_512 = handle.local.get_sha512(request, &src_path, true)?;
                let remote_512 = handle.remote.get_sha512(request, &self.dest)?;
                if ! remote_512.eq(&local_512) {
                    changes.push(Field::Content);
                }
                if ! changes.is_empty() {
                    return Ok(handle.response.needs_modification(request, &changes));
                }
                return Ok(handle.response.is_matched(request));
            },

            TaskRequestType::Create => {
                self.do_copy(handle, request, None)?;
                return Ok(handle.response.is_created(request));
            },

            TaskRequestType::Modify => {
                if self.recursive {
                    // Re-copy the whole tree on modify (dest dir existed but was flagged).
                    self.do_copy(handle, request, None)?;
                    return Ok(handle.response.is_modified(request, request.changes.clone()));
                }
                if request.changes.contains(&Field::Content) {
                    self.do_copy(handle, request, Some(request.changes.clone()))?;
                }
                else {
                    handle.remote.process_common_file_attributes(request, &self.dest, &self.attributes, &request.changes, Recurse::No)?;
                }
                return Ok(handle.response.is_modified(request, request.changes.clone()));
            },

            _ => { return Err(handle.response.not_supported(request)); }

        }
    }

}

impl CopyAction {

    pub fn do_copy(&self, handle: &Arc<TaskHandle>, request: &Arc<TaskRequest>, _changes: Option<Vec<Field>>) -> Result<(), Arc<TaskResponse>> {
        if self.recursive {
            return self.copy_dir_recursive(handle, request, &self.src, &self.dest);
        }
        handle.remote.copy_file(request, &self.src, &self.dest, |f| { /* after save */
            match handle.remote.process_all_common_file_attributes(request, &f, &self.attributes, Recurse::No) {
                Ok(_x) => Ok(()), Err(y) => Err(y)
            }
        })?;
        return Ok(());
    }

    /// Recursively copy a local directory tree to the remote.
    ///
    /// For each entry in `local_dir`:
    /// - subdirectory → `mkdir -p` on remote, then recurse
    /// - file         → `copy_file` to the matching remote path
    fn copy_dir_recursive(
        &self,
        handle: &Arc<TaskHandle>,
        request: &Arc<TaskRequest>,
        local_dir: &Path,
        remote_dir: &str,
    ) -> Result<(), Arc<TaskResponse>> {
        // Ensure the remote directory exists.
        handle.remote.create_directory(request, &remote_dir.to_string())?;

        let entries = std::fs::read_dir(local_dir).map_err(|e| {
            handle.response.is_failed(
                request,
                &format!("failed to read local directory '{}': {}", local_dir.display(), e),
            )
        })?;

        for entry in entries {
            let entry = entry.map_err(|e| {
                handle.response.is_failed(request, &format!("directory entry error: {}", e))
            })?;
            let local_path = entry.path();
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            let remote_path = format!("{}/{}", remote_dir, name_str);

            if local_path.is_dir() {
                self.copy_dir_recursive(handle, request, &local_path, &remote_path)?;
            } else if local_path.is_file() {
                handle.remote.copy_file(request, &local_path, &remote_path, |_f| Ok(()))?;
            }
            // Symlinks and other special files are intentionally skipped.
        }

        Ok(())
    }

}
