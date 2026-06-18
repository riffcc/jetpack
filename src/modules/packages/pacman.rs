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
use crate::modules::packages::common::{PackageDetails, PackageManagementModule};
use crate::tasks::*;
use serde::Deserialize;
use std::sync::Arc;

const MODULE: &str = "pacman";

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct PacmanTask {
    pub name: Option<String>,
    pub package: String,
    pub version: Option<String>,
    pub update: Option<String>,
    pub remove: Option<String>,
    pub with: Option<PreLogicInput>,
    pub and: Option<PostLogicInput>,
}

struct PacmanAction {
    pub package: String,
    pub version: Option<String>,
    pub update: bool,
    pub remove: bool,
}

impl IsTask for PacmanTask {
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
            action: Arc::new(PacmanAction {
                package: handle.template.string_no_spaces(
                    request,
                    tm,
                    &String::from("package"),
                    &self.package,
                )?,
                version: handle.template.string_option_no_spaces(
                    request,
                    tm,
                    &String::from("version"),
                    &self.version,
                )?,
                update: handle.template.boolean_option_default_false(
                    request,
                    tm,
                    &String::from("update"),
                    &self.update,
                )?,
                remove: handle.template.boolean_option_default_false(
                    request,
                    tm,
                    &String::from("remove"),
                    &self.remove,
                )?,
            }),
            with: Arc::new(PreLogicInput::template(handle, request, tm, &self.with)?),
            and: Arc::new(PostLogicInput::template(handle, request, tm, &self.and)?),
        })
    }
}

impl IsAction for PacmanAction {
    fn dispatch(
        &self,
        handle: &Arc<TaskHandle>,
        request: &Arc<TaskRequest>,
    ) -> Result<Arc<TaskResponse>, Arc<TaskResponse>> {
        self.common_dispatch(handle, request)
    }
}

impl PackageManagementModule for PacmanAction {
    fn is_update(&self) -> bool {
        self.update
    }

    fn is_remove(&self) -> bool {
        self.remove
    }

    fn get_version(&self) -> Option<String> {
        self.version.clone()
    }

    fn initial_setup(
        &self,
        _handle: &Arc<TaskHandle>,
        _request: &Arc<TaskRequest>,
    ) -> Result<(), Arc<TaskResponse>> {
        Ok(())
    }

    fn get_local_version(
        &self,
        handle: &Arc<TaskHandle>,
        request: &Arc<TaskRequest>,
    ) -> Result<Option<PackageDetails>, Arc<TaskResponse>> {
        let actual_package = self.get_actual_package();
        let cmd = match self.version.is_none() {
            true => format!("pacman -Q --info {}", actual_package),
            false => format!(
                "pacman -Q --info {}-{}",
                actual_package,
                self.version.as_ref().unwrap()
            ),
        };
        let result = handle.remote.run(request, &cmd, CheckRc::Unchecked)?;
        let (rc, out) = cmd_info(&result);
        if rc > 1 {
            return Err(handle
                .response
                .is_failed(request, &String::from("pacman query failed")));
        }
        let details = self.parse_package_details(&out.clone());
        Ok(details)
    }

    fn get_remote_version(
        &self,
        _handle: &Arc<TaskHandle>,
        _request: &Arc<TaskRequest>,
    ) -> Result<Option<PackageDetails>, Arc<TaskResponse>> {
        // FIXME: (?) without this implemented this module will always return "Modified" with update: true
        Ok(None)
    }

    fn install_package(
        &self,
        handle: &Arc<TaskHandle>,
        request: &Arc<TaskRequest>,
    ) -> Result<Arc<TaskResponse>, Arc<TaskResponse>> {
        let cmd = match self.version.is_none() {
            true => format!(
                "pacman -S '{}' --noconfirm --noprogressbar --needed",
                self.package
            ),
            false => format!(
                "pacman -S '{}={}' --noconfirm --noprogressbar --needed",
                self.package,
                self.version.as_ref().unwrap()
            ),
        };
        handle.remote.run(request, &cmd, CheckRc::Checked)
    }

    fn update_package(
        &self,
        handle: &Arc<TaskHandle>,
        request: &Arc<TaskRequest>,
    ) -> Result<Arc<TaskResponse>, Arc<TaskResponse>> {
        let actual_package = self.get_actual_package();
        let cmd = match self.version.is_none() {
            true => format!("pacman -Syu '{}' --quiet --noconfirm", actual_package),
            false => format!(
                "pacman -Syu '{}={}' --quiet --noconfirm",
                self.package,
                self.version.as_ref().unwrap()
            ),
        };
        handle.remote.run(request, &cmd, CheckRc::Checked)
    }

    fn remove_package(
        &self,
        handle: &Arc<TaskHandle>,
        request: &Arc<TaskRequest>,
    ) -> Result<Arc<TaskResponse>, Arc<TaskResponse>> {
        let actual_package = self.get_actual_package();
        let cmd = format!("pacman -R '{}' --noconfirm --noprogressbar", actual_package);
        handle.remote.run(request, &cmd, CheckRc::Checked)
    }
}

impl PacmanAction {
    pub fn get_actual_package(&self) -> String {
        if self.package.contains("/") {
            let last = self.package.split("/").last();
            match last {
                Some(x) => x.to_string(),
                None => self.package.clone(), // should be impossible, appease compiler
            }
        } else {
            self.package.clone()
        }
    }

    pub fn parse_package_details(&self, out: &str) -> Option<PackageDetails> {
        let mut name: Option<String> = None;
        let mut version: Option<String> = None;
        for line in out.lines() {
            if line.starts_with("error:") {
                // FIXME: is this possible with rc == 1?
                return None;
            }
            let mut tokens = line.split(":");
            let key = tokens.nth(0);
            let value = tokens.nth(0);
            if let (Some(k), Some(v)) = (key, value) {
                let key2 = k.trim();
                let value2 = v.trim();
                if key2.eq("Name") {
                    name = Some(value2.to_string());
                }
                if key2.eq("Version") {
                    version = Some(value2.to_string());
                    break;
                }
            }
        }
        match (name, version) {
            (Some(name), Some(version)) => Some(PackageDetails { name, version }),
            _ => None,
        }
    }
}
