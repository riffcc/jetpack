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

const MODULE: &str = "zypper";

impl super::batch::BatchedPackage for ZypperAction {
    fn package_name(&self) -> &str {
        &self.package
    }
    fn query_batch_change(
        &self,
        handle: &Arc<TaskHandle>,
        request: &Arc<TaskRequest>,
        installed: Option<PackageDetails>,
    ) -> Result<Arc<TaskResponse>, Arc<TaskResponse>> {
        if self.update
            && !self.remove
            && let Some(installed) = &installed
        {
            let command = format!(
                "zypper --non-interactive --quiet search --match-exact --details {}",
                super::batch::quote(&self.package)
            );
            let response = handle.remote.run(request, &command, CheckRc::Checked)?;
            let candidate = self
                .parse_zypper_search_table(handle, request, &cmd_info(&response).1)?
                .ok_or_else(|| {
                    handle
                        .response
                        .is_failed(request, "zypper query omitted the package version")
                })?;
            if candidate.version.is_empty() {
                return Err(handle
                    .response
                    .is_failed(request, "zypper query returned an empty package version"));
            }
            return Ok(if installed.version == candidate.version {
                handle.response.is_matched(request)
            } else {
                handle
                    .response
                    .needs_modification(request, &[Field::Version])
            });
        }
        self.query_installed_package(handle, request, installed)
    }
    fn query_batch(
        &self,
        handle: &Arc<TaskHandle>,
        request: &Arc<TaskRequest>,
        packages: &[Self],
    ) -> Result<Vec<Option<PackageDetails>>, Arc<TaskResponse>> {
        use super::batch::{query_output, quote, version_rows};
        let names: Vec<_> = packages
            .iter()
            .map(|package| package.package.clone())
            .collect();
        let args = names
            .iter()
            .map(|name| quote(name))
            .collect::<Vec<_>>()
            .join(" ");
        let (rc, out) = query_output(
            handle,
            request,
            &format!("rpm -q --qf '%{{NAME}}\\t%{{VERSION}}-%{{RELEASE}}\\n' -- {args}"),
        )?;
        version_rows(handle, request, &names, &out, "is not installed", rc)
    }
    fn apply_batch(
        &self,
        handle: &Arc<TaskHandle>,
        request: &Arc<TaskRequest>,
        packages: &[&Self],
    ) -> Result<(), Arc<TaskResponse>> {
        use super::batch::quote;
        let removing = request.request_type == TaskRequestType::Remove;
        let targets = packages
            .iter()
            .map(|package| {
                quote(&if removing {
                    package.package.clone()
                } else {
                    match &package.version {
                        Some(version) => format!("{}={version}", package.package),
                        None => package.package.clone(),
                    }
                })
            })
            .collect::<Vec<_>>()
            .join(" ");
        let verb = if removing {
            "remove"
        } else if request.request_type == TaskRequestType::Modify {
            "update"
        } else {
            "install"
        };
        handle.remote.run(
            request,
            &format!("zypper --non-interactive --quiet {verb} -- {targets}"),
            CheckRc::Checked,
        )?;
        Ok(())
    }
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
#[derive(Default)]
pub struct ZypperTask {
    pub name: Option<String>,
    pub package: ResourceInput,
    pub version: Option<String>,
    pub update: Option<String>,
    pub remove: Option<String>,
    pub with: Option<PreLogicInput>,
    pub and: Option<PostLogicInput>,
}

#[derive(Clone)]
struct ZypperAction {
    pub package: String,
    pub version: Option<String>,
    pub update: bool,
    pub remove: bool,
}

impl IsTask for ZypperTask {
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
        let package_names = self
            .package
            .template_no_spaces(handle, request, tm, "package")?;
        let prototype = ZypperAction {
            package: String::new(),
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
        };
        Ok(EvaluatedTask {
            action: super::batch::action(
                package_names
                    .into_iter()
                    .map(|package| ZypperAction {
                        package,
                        ..prototype.clone()
                    })
                    .collect(),
                self.package.is_list(),
            ),
            with: Arc::new(PreLogicInput::template(handle, request, tm, &self.with)?),
            and: Arc::new(PostLogicInput::template(handle, request, tm, &self.and)?),
        })
    }
}

impl IsAction for ZypperAction {
    fn dispatch(
        &self,
        handle: &Arc<TaskHandle>,
        request: &Arc<TaskRequest>,
    ) -> Result<Arc<TaskResponse>, Arc<TaskResponse>> {
        self.common_dispatch(handle, request)
    }
}

impl PackageManagementModule for ZypperAction {
    fn initial_setup(
        &self,
        _handle: &Arc<TaskHandle>,
        _request: &Arc<TaskRequest>,
    ) -> Result<(), Arc<TaskResponse>> {
        // nothing to do here, see how this was used in yum_dnf.rs
        Ok(())
    }

    fn is_update(&self) -> bool {
        self.update
    }

    fn is_remove(&self) -> bool {
        self.remove
    }

    fn get_version(&self) -> Option<String> {
        self.version.clone()
    }

    fn get_remote_version(
        &self,
        handle: &Arc<TaskHandle>,
        request: &Arc<TaskRequest>,
    ) -> Result<Option<PackageDetails>, Arc<TaskResponse>> {
        let cmd = format!(
            "zypper --non-interactive --quiet search --match-exact --details '{}'",
            self.package
        );
        let result = handle.remote.run_unsafe(request, &cmd, CheckRc::Unchecked);
        match result {
            Ok(r) => {
                let (rc, out) = cmd_info(&r);
                if rc == 104 {
                    Ok(None)
                } else if rc != 0 {
                    Err(r)
                } else {
                    self.parse_zypper_search_table(handle, request, &out)
                }
            }
            Err(e) => Err(e),
        }
    }

    fn get_local_version(
        &self,
        handle: &Arc<TaskHandle>,
        request: &Arc<TaskRequest>,
    ) -> Result<Option<PackageDetails>, Arc<TaskResponse>> {
        let cmd = format!(
            "zypper --non-interactive --quiet search --match-exact --details --installed-only '{}'",
            self.package
        );
        let result = handle.remote.run(request, &cmd, CheckRc::Unchecked);
        match result {
            Ok(r) => {
                let (rc, out) = cmd_info(&r);
                if rc == 104 {
                    Ok(None)
                } else if rc != 0 {
                    Err(r)
                } else {
                    self.parse_zypper_search_table(handle, request, &out)
                }
            }
            Err(e) => Err(e),
        }
    }

    fn install_package(
        &self,
        handle: &Arc<TaskHandle>,
        request: &Arc<TaskRequest>,
    ) -> Result<Arc<TaskResponse>, Arc<TaskResponse>> {
        let cmd = match &self.version {
            Some(version) => format!(
                "zypper --non-interactive --quiet  install '{}={}'",
                self.package, version
            ),
            None => format!(
                "zypper --non-interactive --quiet install '{}'",
                self.package
            ),
        };
        handle.remote.run(request, &cmd, CheckRc::Checked)
    }

    fn update_package(
        &self,
        handle: &Arc<TaskHandle>,
        request: &Arc<TaskRequest>,
    ) -> Result<Arc<TaskResponse>, Arc<TaskResponse>> {
        let cmd = match &self.version {
            Some(version) => format!(
                "zypper --non-interactive --quiet update '{}={}'",
                self.package, version
            ),
            None => format!("zypper --non-interactive --quiet update '{}'", self.package),
        };
        handle.remote.run(request, &cmd, CheckRc::Checked)
    }

    fn remove_package(
        &self,
        handle: &Arc<TaskHandle>,
        request: &Arc<TaskRequest>,
    ) -> Result<Arc<TaskResponse>, Arc<TaskResponse>> {
        let cmd = format!("zypper --non-interactive --quiet remove '{}'", self.package);
        handle.remote.run(request, &cmd, CheckRc::Checked)
    }
}

impl ZypperAction {
    // Takes the zypper output table and extract the version out of the table body
    // The tables often looks like this, including the additional empty line.
    //
    // ```text
    //
    // S  | Name | Type    | Version   | Arch   | Repository
    // ---+------+---------+-----------+--------+------------------------
    // i+ | curl | package | 8.3.0-1.1 | x86_64 | openSUSE-Tumbleweed-Oss
    // ```

    pub fn parse_zypper_search_table(
        &self,
        handle: &Arc<TaskHandle>,
        request: &Arc<TaskRequest>,
        out: &str,
    ) -> Result<Option<PackageDetails>, Arc<TaskResponse>> {
        let skip_header = out.trim().lines().nth(2);
        let row = match skip_header {
            Some(r) => r,
            None => {
                return Err(handle.response.is_failed(
                    request,
                    &format!("unable to parse unexpected output from zypper (1): {}", out),
                ));
            }
        };
        match row.split("|").nth(3) {
            Some(version) => Ok(Some(PackageDetails {
                name: self.package.clone(),
                version: version.trim().to_string(),
            })),
            None => Err(handle.response.is_failed(
                request,
                &format!("unable to parse unexpected output from zypper (2): {}", out),
            )),
        }
    }
}
