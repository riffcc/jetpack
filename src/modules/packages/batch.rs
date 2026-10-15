// Copyright (C) Riff Labs <team@riff.cc>
// SPDX-License-Identifier: GPL-3.0-or-later

use super::common::{PackageDetails, PackageManagementModule};
use crate::tasks::*;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub trait BatchedPackage: PackageManagementModule + IsAction + Sized + 'static {
    fn package_name(&self) -> &str;
    fn query_batch_change(
        &self,
        handle: &Arc<TaskHandle>,
        request: &Arc<TaskRequest>,
        installed: Option<PackageDetails>,
    ) -> Result<Arc<TaskResponse>, Arc<TaskResponse>> {
        self.query_installed_package(handle, request, installed)
    }
    fn query_batch(
        &self,
        handle: &Arc<TaskHandle>,
        request: &Arc<TaskRequest>,
        packages: &[Self],
    ) -> Result<Vec<Option<PackageDetails>>, Arc<TaskResponse>>;
    fn apply_batch(
        &self,
        handle: &Arc<TaskHandle>,
        request: &Arc<TaskRequest>,
        packages: &[&Self],
    ) -> Result<(), Arc<TaskResponse>>;
}

struct PackageBatch<M> {
    packages: Vec<M>,
    selection: Mutex<Option<Vec<usize>>>,
}

pub fn action<M: BatchedPackage>(mut packages: Vec<M>, list: bool) -> Arc<dyn IsAction> {
    if !list {
        return Arc::new(packages.remove(0));
    }
    crate::tasks::resources::retain_unique_resources(&mut packages, |package| {
        package.package_name().to_owned()
    });
    Arc::new(PackageBatch {
        packages,
        selection: Mutex::new(None),
    })
}

impl<M: BatchedPackage> IsAction for PackageBatch<M> {
    fn dispatch(
        &self,
        handle: &Arc<TaskHandle>,
        request: &Arc<TaskRequest>,
    ) -> Result<Arc<TaskResponse>, Arc<TaskResponse>> {
        let first = &self.packages[0];
        match request.request_type {
            TaskRequestType::Query => {
                *self.selection.lock().unwrap() = None;
                // Package queries are read-only in plan/check mode. In
                // particular, apt cache refresh belongs to apply execution.
                if !handle.run_state.visitor.read().unwrap().is_check_mode() {
                    first.initial_setup(handle, request)?;
                }
                let local = first.query_batch(handle, request, &self.packages)?;
                if local.len() != self.packages.len() {
                    return Err(handle
                        .response
                        .is_failed(request, "package batch query returned an incomplete result"));
                }
                let mut selected = Vec::new();
                let mut creates = false;
                for (index, (package, installed)) in self.packages.iter().zip(local).enumerate() {
                    let result = package.query_batch_change(handle, request, installed)?;
                    if result.status != TaskStatus::IsMatched {
                        creates |= result.status == TaskStatus::NeedsCreation;
                        selected.push(index);
                    }
                }
                if selected.is_empty() {
                    return Ok(handle.response.is_matched(request));
                }
                *self.selection.lock().unwrap() = Some(selected);
                Ok(if first.is_remove() {
                    handle.response.needs_removal(request)
                } else if creates {
                    handle.response.needs_creation(request)
                } else {
                    handle
                        .response
                        .needs_modification(request, &[Field::Version])
                })
            }
            TaskRequestType::Create | TaskRequestType::Modify | TaskRequestType::Remove => {
                let selected = self.selection.lock().unwrap().clone().ok_or_else(|| {
                    handle.response.is_failed(
                        request,
                        "package batch requires a successful change query before apply",
                    )
                })?;
                let packages: Vec<&M> = selected
                    .iter()
                    .map(|index| &self.packages[*index])
                    .collect();
                first.apply_batch(handle, request, &packages)?;
                Ok(match request.request_type {
                    TaskRequestType::Create => handle.response.is_created(request),
                    TaskRequestType::Remove => handle.response.is_removed(request),
                    _ => handle
                        .response
                        .is_modified(request, request.changes.clone()),
                })
            }
            _ => Err(handle.response.not_supported(request)),
        }
    }
}

pub fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

/// Map line-oriented package-manager output to every requested name. Missing
/// packages remain None; callers validate exit status and record shape first.
pub fn details(names: &[String], rows: HashMap<String, String>) -> Vec<Option<PackageDetails>> {
    names
        .iter()
        .map(|name| {
            rows.get(name).map(|version| PackageDetails {
                name: name.clone(),
                version: version.clone(),
            })
        })
        .collect()
}

pub fn query_output(
    handle: &Arc<TaskHandle>,
    request: &Arc<TaskRequest>,
    command: &str,
) -> Result<(i32, String), Arc<TaskResponse>> {
    let response = handle
        .remote
        .run_unsafe(request, command, CheckRc::Unchecked)?;
    let (rc, out) = cmd_info(&response);
    if rc != 0 && rc != 1 {
        return Err(handle
            .response
            .command_failed(request, &response.command_result));
    }
    if out.trim().is_empty() {
        return Err(handle
            .response
            .is_failed(request, "package batch query returned empty output"));
    }
    Ok((rc, out))
}

pub fn version_rows(
    handle: &Arc<TaskHandle>,
    request: &Arc<TaskRequest>,
    names: &[String],
    out: &str,
    missing: &str,
    rc: i32,
) -> Result<Vec<Option<PackageDetails>>, Arc<TaskResponse>> {
    let mut rows = HashMap::new();
    for line in out.lines().filter(|line| !line.trim().is_empty()) {
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.len() == 2 && names.iter().any(|name| name == fields[0]) {
            rows.insert(fields[0].into(), fields[1].into());
        } else if !line.contains(missing) {
            return Err(handle
                .response
                .is_failed(request, "unable to parse package batch query output"));
        }
    }
    if rc == 0 && names.iter().any(|name| !rows.contains_key(name)) {
        return Err(handle
            .response
            .is_failed(request, "package batch query omitted a requested package"));
    }
    Ok(details(names, rows))
}

#[cfg(test)]
mod tests;
