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

const MODULE: &str = "apt";

impl super::batch::BatchedPackage for AptAction {
    fn package_name(&self) -> &str {
        &self.package
    }
    fn query_batch_change(
        &self,
        handle: &Arc<TaskHandle>,
        request: &Arc<TaskRequest>,
        installed: Option<PackageDetails>,
    ) -> Result<Arc<TaskResponse>, Arc<TaskResponse>> {
        if self.upgrade
            && !self.remove
            && let Some(installed) = &installed
        {
            let candidate = if self.is_local_deb() {
                let command = format!("dpkg-deb -f {} Version", super::batch::quote(&self.package));
                cmd_info(&handle.remote.run(request, &command, CheckRc::Checked)?)
                    .1
                    .trim()
                    .to_owned()
            } else {
                let command = format!("apt-cache policy {}", super::batch::quote(&self.package));
                let response = handle.remote.run(request, &command, CheckRc::Checked)?;
                cmd_info(&response)
                    .1
                    .lines()
                    .find_map(|line| {
                        line.trim()
                            .strip_prefix("Candidate:")
                            .map(|version| version.trim().to_owned())
                    })
                    .ok_or_else(|| {
                        handle
                            .response
                            .is_failed(request, "apt query omitted the candidate version")
                    })?
            };
            if candidate.is_empty() || candidate == "(none)" {
                return Err(handle
                    .response
                    .is_failed(request, "apt query returned no candidate version"));
            }
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
        use super::batch::{details, query_output, quote};
        let mut names = Vec::new();
        for package in packages {
            let name = if package.is_local_deb() {
                let command = format!("dpkg-deb -f {} Package", quote(&package.package));
                let response = handle.remote.run(request, &command, CheckRc::Checked)?;
                cmd_info(&response).1.trim().to_owned()
            } else {
                package.package.clone()
            };
            names.push(name);
        }
        let args = names
            .iter()
            .map(|name| quote(name))
            .collect::<Vec<_>>()
            .join(" ");
        let (rc, out) = query_output(
            handle,
            request,
            &format!(
                "dpkg-query -W -f='${{binary:Package}}\\t${{Version}}\\t${{db:Status-Status}}\\n' -- {args}"
            ),
        )?;
        let mut rows = std::collections::HashMap::new();
        let mut seen = std::collections::HashSet::new();
        for line in out.lines().filter(|line| !line.trim().is_empty()) {
            let fields: Vec<_> = line.split('\t').collect();
            if fields.len() == 3 {
                if !matches!(
                    fields[2],
                    "not-installed"
                        | "config-files"
                        | "half-installed"
                        | "unpacked"
                        | "half-configured"
                        | "triggers-awaited"
                        | "triggers-pending"
                        | "installed"
                ) {
                    return Err(handle.response.is_failed(
                        request,
                        "dpkg batch query returned an invalid package status",
                    ));
                }
                for name in &names {
                    if fields[0] == name
                        || (!name.contains(':')
                            && fields[0].split(':').next() == Some(name.as_str()))
                    {
                        seen.insert(name.clone());
                        if fields[2] == "installed" {
                            if fields[1].is_empty() {
                                return Err(handle.response.is_failed(
                                    request,
                                    "dpkg batch query returned an empty installed version",
                                ));
                            }
                            rows.insert(name.clone(), fields[1].into());
                        }
                    }
                }
            } else if !line.contains("no packages found matching") {
                return Err(handle
                    .response
                    .is_failed(request, "unable to parse dpkg batch query output"));
            }
        }
        if rc == 0 && names.iter().any(|name| !seen.contains(name)) {
            return Err(handle
                .response
                .is_failed(request, "dpkg batch query omitted a requested package"));
        }
        Ok(details(&names, rows))
    }

    fn apply_batch(
        &self,
        handle: &Arc<TaskHandle>,
        request: &Arc<TaskRequest>,
        packages: &[&Self],
    ) -> Result<(), Arc<TaskResponse>> {
        use super::batch::quote;
        let mut targets = Vec::new();
        for package in packages {
            let target = if request.request_type == TaskRequestType::Remove
                && package.is_local_deb()
            {
                let command = format!("dpkg-deb -f {} Package", quote(&package.package));
                cmd_info(&handle.remote.run(request, &command, CheckRc::Checked)?)
                    .1
                    .trim()
                    .to_owned()
            } else if package.is_local_deb() || request.request_type == TaskRequestType::Remove {
                package.package.clone()
            } else {
                match &package.version {
                    Some(version) => format!("{}={version}", package.package),
                    None => package.package.clone(),
                }
            };
            targets.push(quote(&target));
        }
        let verb = if request.request_type == TaskRequestType::Remove {
            "remove"
        } else {
            "install"
        };
        let upgrade = if request.request_type == TaskRequestType::Modify {
            "--only-upgrade "
        } else {
            ""
        };
        let command = format!(
            "DEBIAN_FRONTEND=noninteractive apt-get {verb} -y -qq {upgrade}-- {}",
            targets.join(" ")
        );
        handle.remote.run(request, &command, CheckRc::Checked)?;
        Ok(())
    }
}

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
#[derive(Default)]
pub struct AptTask {
    pub name: Option<String>,
    pub package: ResourceInput,
    pub version: Option<String>,
    pub update: Option<String>,
    pub upgrade: Option<String>,
    pub remove: Option<String>,
    pub with: Option<PreLogicInput>,
    pub retry: Option<String>,
    pub delay: Option<String>,
    pub and: Option<PostLogicInput>,
}

#[derive(Clone)]
struct AptAction {
    pub package: String,
    pub version: Option<String>,
    pub update: bool,
    pub upgrade: bool,
    pub remove: bool,
}

impl AptAction {
    /// Check if package is a local .deb file path
    fn is_local_deb(&self) -> bool {
        self.package.ends_with(".deb")
            || self.package.starts_with("/") && self.package.contains(".deb")
            || self.package.starts_with("./")
    }

    /// Extract package name from a .deb file using dpkg-deb
    fn get_deb_package_name(
        &self,
        handle: &Arc<TaskHandle>,
        request: &Arc<TaskRequest>,
    ) -> Result<String, Arc<TaskResponse>> {
        let cmd = format!("dpkg-deb -f '{}' Package", self.package);
        let result = handle.remote.run(request, &cmd, CheckRc::Checked)?;
        let (_, out) = cmd_info(&result);
        Ok(out.trim().to_string())
    }

    /// Extract version from a .deb file using dpkg-deb
    fn get_deb_version(
        &self,
        handle: &Arc<TaskHandle>,
        request: &Arc<TaskRequest>,
    ) -> Result<String, Arc<TaskResponse>> {
        let cmd = format!("dpkg-deb -f '{}' Version", self.package);
        let result = handle.remote.run(request, &cmd, CheckRc::Checked)?;
        let (_, out) = cmd_info(&result);
        Ok(out.trim().to_string())
    }
}

impl IsTask for AptTask {
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
        let prototype = AptAction {
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
            upgrade: handle.template.boolean_option_default_false(
                request,
                tm,
                &String::from("upgrade"),
                &self.upgrade,
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
                    .map(|package| AptAction {
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

impl IsAction for AptAction {
    fn dispatch(
        &self,
        handle: &Arc<TaskHandle>,
        request: &Arc<TaskRequest>,
    ) -> Result<Arc<TaskResponse>, Arc<TaskResponse>> {
        self.common_dispatch(handle, request)
    }
}

impl PackageManagementModule for AptAction {
    fn initial_setup(
        &self,
        handle: &Arc<TaskHandle>,
        request: &Arc<TaskRequest>,
    ) -> Result<(), Arc<TaskResponse>> {
        // Check if we need to refresh apt cache
        // Priority: 1) explicit update: true always updates, 2) check cache age vs lifetime

        if self.update {
            // Explicit update requested
            let cmd = String::from("apt-get update -qq");
            handle.remote.run(request, &cmd, CheckRc::Checked)?;
            return Ok(());
        }

        // Auto-refresh based on cache age
        // Get apt_cache_lifetime from host vars (default 3600 seconds = 1 hour)
        let cache_lifetime: u64 = handle
            .host
            .read()
            .unwrap()
            .get_blended_variables()
            .get(serde_yaml::Value::String("apt_cache_lifetime".to_string()))
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse().ok())
            .unwrap_or(3600);

        // Check if apt cache needs refresh:
        // 1. Directory doesn't exist -> stale
        // 2. Directory is empty (no package lists) -> stale
        // 3. Directory mtime is older than threshold -> stale
        // Note: Using && and || instead of if/then because LANG=C prefix only works with simple commands
        let check_cmd = format!(
            "test -d /var/lib/apt/lists && test -n \"$(ls /var/lib/apt/lists 2>/dev/null | grep -v -E '^(lock|partial)$')\" && test -z \"$(find /var/lib/apt/lists -maxdepth 0 -mmin +{} 2>/dev/null)\" && echo fresh || echo stale",
            cache_lifetime / 60 // convert seconds to minutes for find -mmin
        );

        // Use run_unsafe since the check command contains shell constructs (not user input)
        let result = handle
            .remote
            .run_unsafe(request, &check_cmd, CheckRc::Unchecked);
        if let Ok(response) = result {
            let (_, out) = cmd_info(&response);
            if out.trim() == "stale" {
                let cmd = String::from("apt-get update -qq");
                handle.remote.run(request, &cmd, CheckRc::Checked)?;
            }
        }

        Ok(())
    }

    fn is_update(&self) -> bool {
        self.upgrade
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
        // For local .deb files, get version from the .deb itself
        if self.is_local_deb() {
            let pkg_name = self.get_deb_package_name(handle, request)?;
            let version = self.get_deb_version(handle, request)?;
            return Ok(Some(PackageDetails {
                name: pkg_name,
                version,
            }));
        }

        /* need to implement so update returns the correct modification status */
        let cmd = format!(
            "apt-cache show {} | grep Version | cut -f2 --delimiter=':'",
            self.package.clone()
        );
        let result = handle.remote.run_unsafe(request, &cmd, CheckRc::Unchecked);
        match result {
            Ok(r) => {
                let (rc, out) = cmd_info(&r);
                // (differentiated return code is unavailable for this module only, don't repeat this pattern)
                if out.contains("No packages found") {
                    return Ok(None);
                }
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
        // For local .deb files, check if the package (not the file) is installed
        let pkg_name = if self.is_local_deb() {
            self.get_deb_package_name(handle, request)?
        } else {
            self.package.clone()
        };

        let cmd = format!("dpkg -s '{}' >/dev/null", pkg_name);
        let result = handle.remote.run_unsafe(request, &cmd, CheckRc::Unchecked);
        match result {
            Ok(r) => {
                let (rc, _out) = cmd_info(&r);
                if rc != 0 {
                    return Ok(None);
                }
            }
            Err(e) => return Err(e),
        }

        let cmd = format!("dpkg-query -W '{}'", pkg_name);
        let result = handle.remote.run(request, &cmd, CheckRc::Unchecked);
        match result {
            Ok(r) => {
                let (rc, out) = cmd_info(&r);
                if rc == 0 {
                    let details = self.parse_local_package_details_with_name(
                        handle,
                        &pkg_name,
                        &out.clone(),
                    )?;
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
        // For local .deb files, use apt-get install with the file path
        // apt-get install handles dependencies better than dpkg -i
        if self.is_local_deb() {
            let cmd = format!(
                "DEBIAN_FRONTEND=noninteractive apt-get install -y '{}'",
                self.package
            );
            return handle.remote.run(request, &cmd, CheckRc::Checked);
        }

        let cmd = match self.version.is_none() {
            true => format!(
                "DEBIAN_FRONTEND=noninteractive apt-get install '{}' -qq",
                self.package
            ),
            false => format!(
                "DEBIAN_FRONTEND=noninteractive apt-get install '{}={}' -qq",
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
            true => format!(
                "DEBIAN_FRONTEND=noninteractive apt-get install '{}' --only-upgrade -qq",
                self.package
            ),
            false => format!(
                "DEBIAN_FRONTEND=noninteractive apt-get install '{}={}' --only-upgrade -qq",
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
        let cmd = format!(
            "DEBIAN_FRONTEND=noninteractive apt-get remove '{}' -qq",
            self.package
        );
        handle.remote.run(request, &cmd, CheckRc::Checked)
    }
}

impl AptAction {
    pub fn parse_local_package_details_with_name(
        &self,
        _handle: &Arc<TaskHandle>,
        pkg_name: &str,
        out: &str,
    ) -> Result<Option<PackageDetails>, Arc<TaskResponse>> {
        let mut tokens = out.split("\t");
        let version = tokens.nth(1);
        match version {
            Some(v) => Ok(Some(PackageDetails {
                name: pkg_name.to_string(),
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
