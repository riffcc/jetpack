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

const MODULE: &str = "homebrew";

fn brew_batch_command(arguments: &str) -> String {
    let script = format!(
        "if command -v brew >/dev/null 2>&1; then brew {arguments}; elif [ -x /opt/homebrew/bin/brew ]; then /opt/homebrew/bin/brew {arguments}; else brew {arguments}; fi"
    );
    // Connections prefix commands with LANG=C. Wrap the compound shell
    // discovery so it remains a valid command after that assignment.
    format!("sh -c {}", super::batch::quote(&script))
}

impl super::batch::BatchedPackage for HomebrewAction {
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
            let name = match &self.version {
                Some(version) => format!("{}@{version}", self.package),
                None => self.package.clone(),
            };
            let command = brew_batch_command(&format!(
                "info --json=v2 --formula -- {}",
                super::batch::quote(&name)
            ));
            let response = handle
                .remote
                .run_unsafe(request, &command, CheckRc::Checked)?;
            let parsed: serde_json::Value =
                serde_json::from_str(&cmd_info(&response).1).map_err(|_| {
                    handle
                        .response
                        .is_failed(request, "unable to parse Homebrew repository version")
                })?;
            let candidate = parsed["formulas"][0]["versions"]["stable"]
                .as_str()
                .ok_or_else(|| {
                    handle
                        .response
                        .is_failed(request, "Homebrew query omitted the stable version")
                })?;
            let revision = parsed["formulas"][0]["revision"].as_u64().unwrap_or(0);
            if candidate.is_empty() {
                return Err(handle
                    .response
                    .is_failed(request, "Homebrew query returned an empty stable version"));
            }
            let candidate = if revision == 0 {
                candidate.to_owned()
            } else {
                format!("{candidate}_{revision}")
            };
            return Ok(if installed.version == candidate {
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
        use super::batch::{query_output, quote};
        let names: Vec<_> = packages
            .iter()
            .map(|package| match &package.version {
                Some(version) => format!("{}@{version}", package.package),
                None => package.package.clone(),
            })
            .collect();
        let args = names
            .iter()
            .map(|name| quote(name))
            .collect::<Vec<_>>()
            .join(" ");
        let command = brew_batch_command(&format!("info --json=v2 --formula -- {args}"));
        let (_, out) = query_output(handle, request, &command)?;
        let parsed: serde_json::Value = serde_json::from_str(&out).map_err(|_| {
            handle
                .response
                .is_failed(request, "unable to parse Homebrew batch query output")
        })?;
        let formulas = parsed
            .get("formulas")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| {
                handle.response.is_failed(
                    request,
                    "Homebrew batch query did not return formula information",
                )
            })?;
        names
            .iter()
            .map(|name| {
                let formula = formulas
                    .iter()
                    .find(|formula| {
                        formula["name"].as_str() == Some(name.as_str())
                            || formula["full_name"].as_str() == Some(name.as_str())
                            || formula["aliases"].as_array().is_some_and(|aliases| {
                                aliases
                                    .iter()
                                    .any(|alias| alias.as_str() == Some(name.as_str()))
                            })
                    })
                    .ok_or_else(|| {
                        handle
                            .response
                            .is_failed(request, "Homebrew batch query omitted a requested formula")
                    })?;
                let installed = formula["installed"].as_array().ok_or_else(|| {
                    handle
                        .response
                        .is_failed(request, "Homebrew batch query omitted installed versions")
                })?;
                let version = installed
                    .iter()
                    .find(|entry| entry["version"] == formula["linked_keg"])
                    .or_else(|| installed.last());
                version
                    .map(|entry| {
                        entry["version"]
                            .as_str()
                            .map(|version| PackageDetails {
                                name: name.clone(),
                                version: version.into(),
                            })
                            .ok_or_else(|| {
                                handle.response.is_failed(
                                    request,
                                    "Homebrew batch query returned an invalid installed version",
                                )
                            })
                    })
                    .transpose()
            })
            .collect()
    }
    fn apply_batch(
        &self,
        handle: &Arc<TaskHandle>,
        request: &Arc<TaskRequest>,
        packages: &[&Self],
    ) -> Result<(), Arc<TaskResponse>> {
        use super::batch::quote;
        let targets = packages
            .iter()
            .map(|package| {
                quote(&match &package.version {
                    Some(version) => format!("{}@{version}", package.package),
                    None => package.package.clone(),
                })
            })
            .collect::<Vec<_>>()
            .join(" ");
        let verb = if request.request_type == TaskRequestType::Remove {
            "uninstall"
        } else if request.request_type == TaskRequestType::Modify {
            "upgrade"
        } else {
            "install"
        };
        handle.remote.run_unsafe(
            request,
            &brew_batch_command(&format!("{verb} -- {targets}")),
            CheckRc::Checked,
        )?;
        Ok(())
    }
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
#[derive(Default)]
pub struct HomebrewTask {
    pub name: Option<String>,
    pub package: ResourceInput,
    pub version: Option<String>,
    pub update: Option<String>,
    pub remove: Option<String>,
    pub with: Option<PreLogicInput>,
    pub retry: Option<String>,
    pub delay: Option<String>,
    pub and: Option<PostLogicInput>,
}

#[derive(Clone)]
struct HomebrewAction {
    pub package: String,
    pub version: Option<String>,
    pub update: bool,
    pub remove: bool,
}

impl IsTask for HomebrewTask {
    fn name_variable_writes(&self) -> crate::tasks::naming::NameVariableWrites {
        crate::tasks::naming::NameVariableWrites::None
    }

    fn generated_name(&self, context: &crate::tasks::naming::NamingContext<'_>) -> Option<String> {
        Some(context.packages(&self.package, &self.remove, &self.update))
    }

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
        let prototype = HomebrewAction {
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
                    .map(|package| HomebrewAction {
                        package,
                        ..prototype.clone()
                    })
                    .collect(),
                self.package.is_list(),
            ),
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

impl IsAction for HomebrewAction {
    fn dispatch(
        &self,
        handle: &Arc<TaskHandle>,
        request: &Arc<TaskRequest>,
    ) -> Result<Arc<TaskResponse>, Arc<TaskResponse>> {
        self.common_dispatch(handle, request)
    }
}

impl PackageManagementModule for HomebrewAction {
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
            "{} info {} | head -n 1 | cut -f 4 -d ' '",
            self.brew_cmd(),
            self.package.clone()
        );
        let result = handle.remote.run_unsafe(request, &cmd, CheckRc::Unchecked);
        match result {
            Ok(r) => {
                let (rc, out) = cmd_info(&r);
                if rc == 0 {
                    self.parse_remote_package_details(handle, &out.clone())
                } else {
                    Ok(None)
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
        let cmd = format!("{} info {}", self.brew_cmd(), self.package);
        let result = handle.remote.run(request, &cmd, CheckRc::Unchecked);
        match result {
            Ok(r) => {
                let (rc, out) = cmd_info(&r);
                if rc == 0 {
                    let details = self.parse_local_package_details(handle, &out.clone())?;
                    Ok(details)
                } else {
                    Ok(None)
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
        let cmd = match self.version.is_none() {
            true => format!("{} install '{}'", self.brew_cmd(), self.package),
            false => format!(
                "{} install '{}@{}'",
                self.brew_cmd(),
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
        let cmd = match self.version.is_none() {
            true => format!("{} upgrade '{}'", self.brew_cmd(), self.package),
            false => format!(
                "{} upgrade '{}@{}'",
                self.brew_cmd(),
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
        let cmd = format!("{} uninstall '{}'", self.brew_cmd(), self.package);
        handle.remote.run(request, &cmd, CheckRc::Checked)
    }
}

impl HomebrewAction {
    fn brew_cmd(&self) -> &'static str {
        "if command -v brew >/dev/null 2>&1; then brew; elif [ -x /opt/homebrew/bin/brew ]; then /opt/homebrew/bin/brew; else brew; fi"
    }

    pub fn parse_local_package_details(
        &self,
        _handle: &Arc<TaskHandle>,
        out: &str,
    ) -> Result<Option<PackageDetails>, Arc<TaskResponse>> {
        let mut version: Option<String> = None;

        for line in out.lines() {
            if line.starts_with('/') {
                let path_version = line.split(" ").nth(0).unwrap().trim();
                version = Some(path_version.split("/").last().unwrap().trim().to_string());
            }
            // homebrew puts an asterisk next to the currently installed version
            if line.trim().ends_with('*') {
                break;
            }
            // once homebrew has printed the versions, it starts printing other stuff
            if line.starts_with("From:") {
                break;
            }
        }
        match version {
            Some(v) => Ok(Some(PackageDetails {
                name: self.package.clone(),
                version: v.trim().to_string(),
            })),
            None => Ok(None),
        }
    }

    pub fn parse_remote_package_details(
        &self,
        _handle: &Arc<TaskHandle>,
        out: &str,
    ) -> Result<Option<PackageDetails>, Arc<TaskResponse>> {
        Ok(Some(PackageDetails {
            name: self.package.clone(),
            version: out.trim().to_string(),
        }))
    }
}
