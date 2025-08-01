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

const MODULE: &str = "Move";

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct MoveTask {
    pub name: Option<String>,
    pub src: String,
    pub dest: String,
    pub mode: Option<String>,
    pub owner: Option<String>,
    pub group: Option<String>,
    pub backup: Option<String>,
    pub force: Option<String>,
    pub with: Option<PreLogicInput>,
    pub and: Option<PostLogicInput>,
}

struct MoveAction {
    pub src: String,
    pub dest: String,
    pub mode: Option<String>,
    pub owner: Option<String>,
    pub group: Option<String>,
    pub backup: bool,
    pub force: bool,
}

impl IsTask for MoveTask {
    fn get_module(&self) -> String { String::from(MODULE) }
    fn get_name(&self) -> Option<String> { self.name.clone() }
    fn get_with(&self) -> Option<PreLogicInput> { self.with.clone() }

    fn evaluate(&self, handle: &Arc<TaskHandle>, request: &Arc<TaskRequest>, tm: TemplateMode) -> Result<EvaluatedTask, Arc<TaskResponse>> {
        let src = handle.template.path(&request, tm, &String::from("src"), &self.src)?;
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
        
        let backup = match &self.backup {
            Some(b) => {
                let backup_str = handle.template.string(&request, tm, &String::from("backup"), b)?;
                backup_str == "true" || backup_str == "yes" || backup_str == "1"
            },
            None => false,
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
                action: Arc::new(MoveAction {
                    src,
                    dest,
                    mode,
                    owner,
                    group,
                    backup,
                    force,
                }),
                with: Arc::new(PreLogicInput::template(&handle, &request, tm, &self.with)?),
                and: Arc::new(PostLogicInput::template(&handle, &request, tm, &self.and)?),
            }
        );
    }
}

impl MoveAction {
    fn check_file_exists(&self, handle: &Arc<TaskHandle>, request: &Arc<TaskRequest>, path: &str) -> Result<bool, Arc<TaskResponse>> {
        handle.remote.file_exists(&request, &path.to_string())
    }
    
    fn check_parent_dir_exists(&self, handle: &Arc<TaskHandle>, request: &Arc<TaskRequest>) -> Result<(), Arc<TaskResponse>> {
        // Extract parent directory path
        if let Some(slash_pos) = self.dest.rfind('/') {
            if slash_pos > 0 {  // Not root directory
                let parent_dir = &self.dest[..slash_pos];
                if !handle.remote.get_is_directory(&request, &parent_dir.to_string())? {
                    return Err(handle.response.is_failed(&request, 
                        &format!("Parent directory '{}' does not exist", parent_dir)));
                }
            }
        }
        Ok(())
    }
    
    fn create_backup(&self, handle: &Arc<TaskHandle>, request: &Arc<TaskRequest>) -> Result<(), Arc<TaskResponse>> {
        let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S");
        handle.remote.run(&request, 
            &format!("cp -a \"{}\" \"{}.backup.{}\"", self.dest, self.dest, timestamp),
            CheckRc::Checked)?;
        Ok(())
    }
    
    fn perform_move(&self, handle: &Arc<TaskHandle>, request: &Arc<TaskRequest>) -> Result<(), Arc<TaskResponse>> {
        handle.remote.rename(&request, &self.src, &self.dest, self.force)?;
        Ok(())
    }
    
    fn apply_permissions(&self, handle: &Arc<TaskHandle>, request: &Arc<TaskRequest>) -> Result<(), Arc<TaskResponse>> {
        if let Some(ref mode) = self.mode {
            handle.remote.run(&request, 
                &format!("chmod {} \"{}\"", mode, self.dest),
                CheckRc::Checked)?;
        }
        
        if self.owner.is_some() || self.group.is_some() {
            let owner_str = match (&self.owner, &self.group) {
                (Some(o), Some(g)) => format!("{}:{}", o, g),
                (Some(o), None) => o.clone(),
                (None, Some(g)) => format!(":{}", g),
                (None, None) => unreachable!(),
            };
            
            handle.remote.run(&request, 
                &format!("chown {} \"{}\"", owner_str, self.dest),
                CheckRc::Checked)?;
        }
        Ok(())
    }
}

impl IsAction for MoveAction {
    fn dispatch(&self, handle: &Arc<TaskHandle>, request: &Arc<TaskRequest>) -> Result<Arc<TaskResponse>, Arc<TaskResponse>> {
        match request.request_type {
            TaskRequestType::Query => {
                // Check if source exists
                if !self.check_file_exists(handle, request, &self.src)? {
                    return Err(handle.response.is_failed(&request, 
                        &format!("Source '{}' does not exist", self.src)));
                }
                
                // Check if parent directory exists
                self.check_parent_dir_exists(handle, request)?;
                
                // Check if destination exists
                if self.check_file_exists(handle, request, &self.dest)? && !self.force {
                    return Ok(handle.response.is_matched(&request));
                }
                
                let mut changes = vec![Field::Content];
                if self.mode.is_some() { changes.push(Field::Mode); }
                if self.owner.is_some() { changes.push(Field::Owner); }
                if self.group.is_some() { changes.push(Field::Group); }
                
                return Ok(handle.response.needs_modification(&request, &changes));
            },

            TaskRequestType::Modify => {
                // Create backup if requested and destination exists
                if self.backup && self.check_file_exists(handle, request, &self.dest)? {
                    self.create_backup(handle, request)?;
                }
                
                // Perform the move
                self.perform_move(handle, request)?;
                
                // Apply permissions and ownership
                self.apply_permissions(handle, request)?;
                
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