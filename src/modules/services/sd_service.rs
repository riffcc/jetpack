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
use std::sync::Arc;
use std::vec::Vec;

const MODULE: &str = "sd_service";

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
#[derive(Default)]
pub struct SystemdServiceTask {
    pub name: Option<String>,
    pub service: String,
    pub enabled: Option<String>,
    pub started: Option<String>,
    pub reload: Option<String>,
    pub restart: Option<String>,
    pub with: Option<PreLogicInput>,
    pub and: Option<PostLogicInput>,
}

struct SystemdServiceAction {
    pub service: String,
    pub enabled: Option<bool>,
    pub started: Option<bool>,
    pub reload: bool,
    pub restart: bool,
}

/// How `systemctl is-enabled <unit>` classifies a unit's boot-time state.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Enablement {
    /// enabled / alias / linked — will start at boot.
    Enabled,
    /// disabled — won't start at boot.
    Disabled,
    /// static / indirect / generated / transient / masked — the unit has no
    /// `[Install]` section, so it cannot be enabled or disabled. Typically
    /// activated by other means (socket activation, udev, dependencies).
    /// qemu-guest-agent, dbus and getty are common examples.
    Static,
}

#[derive(Clone, PartialEq, Debug)]
struct ServiceDetails {
    enablement: Enablement,
    started: bool,
}

impl IsTask for SystemdServiceTask {
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
            action: Arc::new(SystemdServiceAction {
                service: handle.template.string_no_spaces(
                    request,
                    tm,
                    &String::from("service"),
                    &self.service,
                )?,
                enabled: handle.template.boolean_option_default_none(
                    request,
                    tm,
                    &String::from("enabled"),
                    &self.enabled,
                )?,
                started: handle.template.boolean_option_default_none(
                    request,
                    tm,
                    &String::from("started"),
                    &self.started,
                )?,
                reload: handle.template.boolean_option_default_false(
                    request,
                    tm,
                    &String::from("reload"),
                    &self.reload,
                )?,
                restart: handle.template.boolean_option_default_false(
                    request,
                    tm,
                    &String::from("restart"),
                    &self.restart,
                )?,
            }),
            with: Arc::new(PreLogicInput::template(handle, request, tm, &self.with)?),
            and: Arc::new(PostLogicInput::template(handle, request, tm, &self.and)?),
        })
    }
}

impl IsAction for SystemdServiceAction {
    fn dispatch(
        &self,
        handle: &Arc<TaskHandle>,
        request: &Arc<TaskRequest>,
    ) -> Result<Arc<TaskResponse>, Arc<TaskResponse>> {
        match request.request_type {
            TaskRequestType::Query => {
                let mut changes: Vec<Field> = Vec::new();
                let actual = self.get_service_details(handle, request)?;

                match (actual.enablement, self.enabled) {
                    (Enablement::Enabled, Some(false)) => {
                        changes.push(Field::Disable);
                    }
                    (Enablement::Disabled, Some(true)) => {
                        changes.push(Field::Enable);
                    }
                    (Enablement::Static, _) => {
                        // Static units have no [Install] section: `systemctl
                        // enable`/`disable` are no-ops (systemd itself reports
                        // "no installation config (static)"). Skip enablement
                        // regardless of request so units started by other means
                        // — qemu-guest-agent, dbus, getty — converge instead of
                        // failing the play.
                    }
                    _ => {}
                };

                match (actual.started, self.started, self.reload, self.restart) {
                    (_, Some(false), true, _) => {
                        return Err(handle.response.is_failed(
                            request,
                            &String::from("started:false and reload:true conflict"),
                        ));
                    }
                    (_, Some(false), _, true) => {
                        return Err(handle.response.is_failed(
                            request,
                            &String::from("started:false and restart:true conflict"),
                        ));
                    }
                    (_, _, true, true) => {
                        return Err(handle.response.is_failed(
                            request,
                            &String::from("reload:true and restart:true conflict"),
                        ));
                    }
                    (true, Some(true), true, false) => {
                        changes.push(Field::Reload);
                    }
                    (true, None, true, false) => {
                        changes.push(Field::Reload); /* a little weird, but we know what you mean */
                    }
                    (false, None, true, false) => {
                        return Err(handle.response.is_failed(
                            request,
                            &String::from(
                                "reload:true requires the service to already be started or started:true",
                            ),
                        ));
                    }
                    (true, Some(true), false, true) => {
                        changes.push(Field::Restart);
                    }
                    (true, None, false, true) => {
                        changes.push(Field::Restart); /* a little weird, but we know what you mean */
                    }
                    (false, None, false, true) => {
                        changes.push(Field::Start); /* a little weird, but we know what you mean */
                    }
                    (false, Some(true), _, _) => {
                        changes.push(Field::Start);
                    }
                    (true, Some(false), false, false) => {
                        changes.push(Field::Stop);
                    }
                    _ => {}
                };

                if !changes.is_empty() {
                    Ok(handle.response.needs_modification(request, &changes))
                } else {
                    Ok(handle.response.is_matched(request))
                }
            }

            TaskRequestType::Modify => {
                if request.changes.contains(&Field::Start) {
                    self.do_start(handle, request)?;
                } else if request.changes.contains(&Field::Stop) {
                    self.do_stop(handle, request)?;
                } else if request.changes.contains(&Field::Reload) {
                    self.do_reload(handle, request)?;
                } else if request.changes.contains(&Field::Restart) {
                    self.do_restart(handle, request)?;
                }

                if request.changes.contains(&Field::Enable) {
                    self.do_enable(handle, request)?;
                } else if request.changes.contains(&Field::Disable) {
                    self.do_disable(handle, request)?;
                }

                Ok(handle
                    .response
                    .is_modified(request, request.changes.clone()))
            }

            _ => Err(handle.response.not_supported(request)),
        }
    }
}

impl SystemdServiceAction {
    pub fn get_service_details(
        &self,
        handle: &Arc<TaskHandle>,
        request: &Arc<TaskRequest>,
    ) -> Result<ServiceDetails, Arc<TaskResponse>> {
        let is_active: bool;
        let is_enabled_cmd = format!("systemctl is-enabled '{}'", self.service);
        let is_active_cmd = format!("systemctl is-active '{}'", self.service);

        let result = handle
            .remote
            .run(request, &is_enabled_cmd, CheckRc::Unchecked)?;
        let (_rc, out) = cmd_info(&result);
        let enablement = match classify_enablement(&out) {
            Ok(e) => e,
            Err(reason) => {
                return Err(handle.response.is_failed(
                    request,
                    &format!(
                        "systemctl enablement status unexpected for service({}): {}",
                        self.service, reason
                    ),
                ));
            }
        };

        let result2 = handle
            .remote
            .run(request, &is_active_cmd, CheckRc::Unchecked)?;
        let (_rc2, out2) = cmd_info(&result2);
        is_active = match classify_activity(&out2) {
            Ok(active) => active,
            Err(reason) => {
                return Err(handle.response.is_failed(
                    request,
                    &format!(
                        "systemctl activity status unexpected for service({}): {}",
                        self.service, reason
                    ),
                ));
            }
        };

        Ok(ServiceDetails {
            enablement,
            started: is_active,
        })
    }

    pub fn do_start(
        &self,
        handle: &Arc<TaskHandle>,
        request: &Arc<TaskRequest>,
    ) -> Result<Arc<TaskResponse>, Arc<TaskResponse>> {
        let cmd = format!("systemctl start '{}'", self.service);
        handle.remote.run(request, &cmd, CheckRc::Checked)
    }

    pub fn do_stop(
        &self,
        handle: &Arc<TaskHandle>,
        request: &Arc<TaskRequest>,
    ) -> Result<Arc<TaskResponse>, Arc<TaskResponse>> {
        let cmd = format!("systemctl stop '{}'", self.service);
        handle.remote.run(request, &cmd, CheckRc::Checked)
    }

    pub fn do_enable(
        &self,
        handle: &Arc<TaskHandle>,
        request: &Arc<TaskRequest>,
    ) -> Result<Arc<TaskResponse>, Arc<TaskResponse>> {
        let cmd = format!("systemctl enable '{}'", self.service);
        handle.remote.run(request, &cmd, CheckRc::Checked)
    }

    pub fn do_disable(
        &self,
        handle: &Arc<TaskHandle>,
        request: &Arc<TaskRequest>,
    ) -> Result<Arc<TaskResponse>, Arc<TaskResponse>> {
        let cmd = format!("systemctl disable '{}'", self.service);
        handle.remote.run(request, &cmd, CheckRc::Checked)
    }

    pub fn do_restart(
        &self,
        handle: &Arc<TaskHandle>,
        request: &Arc<TaskRequest>,
    ) -> Result<Arc<TaskResponse>, Arc<TaskResponse>> {
        let cmd = format!("systemctl restart '{}'", self.service);
        handle.remote.run(request, &cmd, CheckRc::Checked)
    }

    pub fn do_reload(
        &self,
        handle: &Arc<TaskHandle>,
        request: &Arc<TaskRequest>,
    ) -> Result<Arc<TaskResponse>, Arc<TaskResponse>> {
        let cmd = format!("systemctl reload '{}'", self.service);
        handle.remote.run(request, &cmd, CheckRc::Checked)
    }
}

/// Classify the stdout of `systemctl is-enabled <unit>` into an [`Enablement`]
/// state. Kept as a free function so the parsing logic is unit-testable without
/// a live host. `systemctl is-enabled` emits exactly one token per unit, so we
/// match the trimmed token in full rather than as a substring — a state like
/// `static` must never be misread as containing another state. Unrecognized
/// output is an error: it usually means the unit isn't installed (`not-found`)
/// or is broken (`bad`), which the caller should surface rather than silently
/// treat as enabled or disabled.
fn classify_enablement(out: &str) -> Result<Enablement, String> {
    match out.trim() {
        "enabled" | "enabled-runtime" | "alias" | "linked" | "linked-runtime" => {
            Ok(Enablement::Enabled)
        }
        "disabled" => Ok(Enablement::Disabled),
        "static" | "indirect" | "generated" | "transient" | "masked" | "masked-runtime" => {
            Ok(Enablement::Static)
        }
        other => Err(format!(
            "{:?} — expected one of: enabled, enabled-runtime, alias, linked, \
             linked-runtime, disabled, static, indirect, generated, transient, \
             masked, masked-runtime",
            other
        )),
    }
}

/// Classify the stdout of `systemctl is-active <unit>` into a running bool, kept
/// as a free function so the parsing logic is unit-testable without a live host.
///
/// `activating` is treated as running: the unit has been told to start and is
/// mid-startup (e.g. k3s-agent registering with the API server), which is not a
/// failure for a `started: true` assertion. `reloading` is likewise running. We
/// match the trimmed token in full rather than as a substring so that, e.g.,
/// `inactive` is never misread via the `active` it happens to contain.
/// Unrecognized output is an error so the caller surfaces it rather than
/// silently coercing it to running or stopped.
fn classify_activity(out: &str) -> Result<bool, String> {
    match out.trim() {
        "active" | "reloading" | "activating" => Ok(true),
        "inactive" | "failed" | "deactivating" => Ok(false),
        other => Err(format!(
            "{:?} — expected one of: active, reloading, activating, inactive, \
             failed, deactivating",
            other
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_enabled_states() {
        for s in [
            "enabled",
            "enabled-runtime",
            "alias",
            "linked",
            "linked-runtime",
        ] {
            assert_eq!(
                classify_enablement(s).unwrap(),
                Enablement::Enabled,
                "{}",
                s
            );
            // real systemctl output carries a trailing newline
            assert_eq!(
                classify_enablement(&format!("{}\n", s)).unwrap(),
                Enablement::Enabled,
                "{}\\n",
                s
            );
        }
    }

    #[test]
    fn classifies_disabled() {
        assert_eq!(
            classify_enablement("disabled\n").unwrap(),
            Enablement::Disabled
        );
    }

    #[test]
    fn classifies_static_units_as_not_enableable() {
        // qemu-guest-agent, dbus, getty, ... ship without an [Install] section.
        for s in [
            "static",
            "indirect",
            "generated",
            "transient",
            "masked",
            "masked-runtime",
        ] {
            assert_eq!(classify_enablement(s).unwrap(), Enablement::Static, "{}", s);
        }
    }

    #[test]
    fn rejects_unrecognized_states() {
        // "not-found" / "bad" — the unit is absent or broken. Must surface as an
        // error rather than be silently coerced to enabled/disabled.
        assert!(classify_enablement("not-found\n").is_err());
        assert!(classify_enablement("bad").is_err());
        assert!(classify_enablement("something-unexpected").is_err());
    }

    #[test]
    fn classifies_running_activity_states() {
        for s in ["active", "reloading", "activating"] {
            assert!(classify_activity(s).unwrap(), "{}", s);
            // real systemctl output carries a trailing newline
            assert!(classify_activity(&format!("{}\n", s)).unwrap(), "{}\\n", s);
        }
    }

    #[test]
    fn classifies_stopped_activity_states() {
        for s in ["inactive", "failed", "deactivating"] {
            assert!(!classify_activity(s).unwrap(), "{}", s);
        }
    }

    #[test]
    fn activity_treats_activating_as_running_not_unknown() {
        // Regression: k3s-agent is mid-startup (registering with the API server)
        // when jetpack inspects it, so `systemctl is-active` reports "activating".
        // This must read as running, not blow up as an unknown state.
        assert!(classify_activity("activating\n").unwrap());
    }

    #[test]
    fn rejects_unrecognized_activity_states() {
        assert!(classify_activity("something-unexpected\n").is_err());
        assert!(classify_activity("").is_err());
    }
}
