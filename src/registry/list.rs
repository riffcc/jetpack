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
use serde::Deserialize;
use std::sync::Arc;

// note: there is some repetition in this module that we would rather not have
// however, it comes from a conflict between polymorphic dispatch macros + traits
// and a lack of data-inheritance in structs. please ignore it the best you can
// and this may be improved later. If there was no Enum, we could have
// polymorphic dispatch, but traversal would lose a lot of serde benefits.

// ADD NEW MODULES HERE, KEEP ALPHABETIZED BY SECTION

// accessctl
use crate::modules::access::group::GroupTask;
use crate::modules::access::user::UserTask;

// commands
use crate::modules::commands::command::CommandTask;
use crate::modules::commands::external::ExternalTask;
use crate::modules::commands::shell::ShellTask;

// control
use crate::modules::control::assert::AssertTask;
use crate::modules::control::debug::DebugTask;
use crate::modules::control::echo::EchoTask;
use crate::modules::control::facts::FactsTask;
use crate::modules::control::fail::FailTask;
use crate::modules::control::self_locate::SelfLocateTask;
use crate::modules::control::set::SetTask;
use crate::modules::control::wait_for_host::WaitForHostTask;
use crate::modules::control::wait_for_http::WaitForHttpTask;
use crate::modules::control::wait_for_others::WaitForOthersTask;

// files
use crate::modules::files::copy::CopyTask;
use crate::modules::files::directory::DirectoryTask;
use crate::modules::files::download::DownloadTask;
use crate::modules::files::fetch::FetchTask;
use crate::modules::files::file::FileTask;
use crate::modules::files::git::GitTask;
use crate::modules::files::r#move::MoveTask;
use crate::modules::files::stat::StatTask;
use crate::modules::files::template::TemplateTask;
use crate::modules::files::unpack::UnpackTask;

// integrations
use crate::modules::integrations::github_release::GithubReleaseTask;

// inventory
use crate::modules::inventory::instantiate::InstantiateTask;

// proxmox
use crate::modules::proxmox::lxc::ProxmoxLxcTask;
use crate::modules::proxmox::migrate::ProxmoxMigrateTask;
use crate::modules::proxmox::node::ProxmoxNodeTask;

// packages
use crate::modules::packages::apt::AptTask;
use crate::modules::packages::homebrew::HomebrewTask;
use crate::modules::packages::pacman::PacmanTask;
use crate::modules::packages::yum_dnf::YumDnfTask;
use crate::modules::packages::zypper::ZypperTask;

// services
use crate::modules::services::sd_service::SystemdServiceTask;

#[allow(non_camel_case_types)]
#[derive(Deserialize, Debug, strum::EnumIter, strum::AsRefStr)]
#[serde(rename_all = "lowercase")]
#[strum(serialize_all = "lowercase")]
pub enum Task {
    // ADD NEW MODULES HERE, KEEP ALPHABETIZED BY NAME
    Apt(AptTask),
    Assert(AssertTask),
    Command(CommandTask),
    Copy(CopyTask),
    Debug(DebugTask),
    Fetch(FetchTask),
    Directory(DirectoryTask),
    Dnf(YumDnfTask),
    Echo(EchoTask),
    External(ExternalTask),
    Facts(FactsTask),
    Fail(FailTask),
    Download(DownloadTask),
    File(FileTask),
    Git(GitTask),
    Github_Release(GithubReleaseTask),
    Group(GroupTask),
    Homebrew(HomebrewTask),
    Instantiate(InstantiateTask),
    Move(MoveTask),
    Pacman(PacmanTask),
    Proxmox_Lxc(ProxmoxLxcTask),
    Proxmox_Migrate(ProxmoxMigrateTask),
    Proxmox_Node(ProxmoxNodeTask),
    Sd_Service(SystemdServiceTask),
    Self_Locate(SelfLocateTask),
    Set(SetTask),
    Shell(ShellTask),
    Stat(StatTask),
    Template(TemplateTask),
    Unpack(UnpackTask),
    User(UserTask),
    Wait_For_Host(WaitForHostTask),
    Wait_For_Http(WaitForHttpTask),
    Wait_For_Others(WaitForOthersTask),
    Yum(YumDnfTask),
    Zypper(ZypperTask),
}

impl Task {
    pub fn get_module(&self) -> String {
        // ADD NEW MODULES HERE, KEEP ALPHABETIZED BY NAME
        match self {
            Task::Apt(x) => x.get_module(),
            Task::Assert(x) => x.get_module(),
            Task::Copy(x) => x.get_module(),
            Task::Debug(x) => x.get_module(),
            Task::Fetch(x) => x.get_module(),
            Task::Directory(x) => x.get_module(),
            Task::Dnf(x) => x.get_module(),
            Task::Echo(x) => x.get_module(),
            Task::External(x) => x.get_module(),
            Task::Facts(x) => x.get_module(),
            Task::Fail(x) => x.get_module(),
            Task::Download(x) => x.get_module(),
            Task::File(x) => x.get_module(),
            Task::Git(x) => x.get_module(),
            Task::Github_Release(x) => x.get_module(),
            Task::Group(x) => x.get_module(),
            Task::Homebrew(x) => x.get_module(),
            Task::Instantiate(x) => x.get_module(),
            Task::Move(x) => x.get_module(),
            Task::Pacman(x) => x.get_module(),
            Task::Proxmox_Lxc(x) => x.get_module(),
            Task::Proxmox_Migrate(x) => x.get_module(),
            Task::Proxmox_Node(x) => x.get_module(),
            Task::Sd_Service(x) => x.get_module(),
            Task::Self_Locate(x) => x.get_module(),
            Task::Set(x) => x.get_module(),
            Task::Command(x) => x.get_module(),
            Task::Shell(x) => x.get_module(),
            Task::Stat(x) => x.get_module(),
            Task::Template(x) => x.get_module(),
            Task::Unpack(x) => x.get_module(),
            Task::User(x) => x.get_module(),
            Task::Wait_For_Host(x) => x.get_module(),
            Task::Wait_For_Http(x) => x.get_module(),
            Task::Wait_For_Others(x) => x.get_module(),
            Task::Yum(x) => x.get_module(),
            Task::Zypper(x) => x.get_module(),
        }
    }

    pub fn get_name(&self) -> Option<String> {
        // ADD NEW MODULES HERE, KEEP ALPHABETIZED BY NAME
        match self {
            Task::Apt(x) => x.get_name(),
            Task::Assert(x) => x.get_name(),
            Task::Copy(x) => x.get_name(),
            Task::Debug(x) => x.get_name(),
            Task::Fetch(x) => x.get_name(),
            Task::Directory(x) => x.get_name(),
            Task::Dnf(x) => x.get_name(),
            Task::Echo(x) => x.get_name(),
            Task::External(x) => x.get_name(),
            Task::Facts(x) => x.get_name(),
            Task::Fail(x) => x.get_name(),
            Task::Download(x) => x.get_name(),
            Task::File(x) => x.get_name(),
            Task::Git(x) => x.get_name(),
            Task::Github_Release(x) => x.get_name(),
            Task::Group(x) => x.get_name(),
            Task::Homebrew(x) => x.get_name(),
            Task::Instantiate(x) => x.get_name(),
            Task::Move(x) => x.get_name(),
            Task::Pacman(x) => x.get_name(),
            Task::Proxmox_Lxc(x) => x.get_name(),
            Task::Proxmox_Migrate(x) => x.get_name(),
            Task::Proxmox_Node(x) => x.get_name(),
            Task::Sd_Service(x) => x.get_name(),
            Task::Self_Locate(x) => x.get_name(),
            Task::Set(x) => x.get_name(),
            Task::Command(x) => x.get_name(),
            Task::Shell(x) => x.get_name(),
            Task::Stat(x) => x.get_name(),
            Task::Template(x) => x.get_name(),
            Task::Unpack(x) => x.get_name(),
            Task::User(x) => x.get_name(),
            Task::Wait_For_Host(x) => x.get_name(),
            Task::Wait_For_Http(x) => x.get_name(),
            Task::Wait_For_Others(x) => x.get_name(),
            Task::Yum(x) => x.get_name(),
            Task::Zypper(x) => x.get_name(),
        }
    }

    pub fn get_with(&self) -> Option<PreLogicInput> {
        // ADD NEW MODULES HERE, KEEP ALPHABETIZED BY NAME
        match self {
            Task::Apt(x) => x.get_with(),
            Task::Assert(x) => x.get_with(),
            Task::Copy(x) => x.get_with(),
            Task::Debug(x) => x.get_with(),
            Task::Fetch(x) => x.get_with(),
            Task::Directory(x) => x.get_with(),
            Task::Dnf(x) => x.get_with(),
            Task::Echo(x) => x.get_with(),
            Task::External(x) => x.get_with(),
            Task::Facts(x) => x.get_with(),
            Task::Fail(x) => x.get_with(),
            Task::Download(x) => x.get_with(),
            Task::File(x) => x.get_with(),
            Task::Git(x) => x.get_with(),
            Task::Github_Release(x) => x.get_with(),
            Task::Group(x) => x.get_with(),
            Task::Homebrew(x) => x.get_with(),
            Task::Instantiate(x) => x.get_with(),
            Task::Move(x) => x.get_with(),
            Task::Pacman(x) => x.get_with(),
            Task::Proxmox_Lxc(x) => x.get_with(),
            Task::Proxmox_Migrate(x) => x.get_with(),
            Task::Proxmox_Node(x) => x.get_with(),
            Task::Sd_Service(x) => x.get_with(),
            Task::Self_Locate(x) => x.get_with(),
            Task::Set(x) => x.get_with(),
            Task::Command(x) => x.get_with(),
            Task::Shell(x) => x.get_with(),
            Task::Stat(x) => x.get_with(),
            Task::Template(x) => x.get_with(),
            Task::Unpack(x) => x.get_with(),
            Task::User(x) => x.get_with(),
            Task::Wait_For_Host(x) => x.get_with(),
            Task::Wait_For_Http(x) => x.get_with(),
            Task::Wait_For_Others(x) => x.get_with(),
            Task::Yum(x) => x.get_with(),
            Task::Zypper(x) => x.get_with(),
        }
    }

    pub fn evaluate(
        &self,
        handle: &Arc<TaskHandle>,
        request: &Arc<TaskRequest>,
        tm: TemplateMode,
    ) -> Result<EvaluatedTask, Arc<TaskResponse>> {
        // ADD NEW MODULES HERE, KEEP ALPHABETIZED BY NAME
        match self {
            Task::Apt(x) => x.evaluate(handle, request, tm),
            Task::Assert(x) => x.evaluate(handle, request, tm),
            Task::Copy(x) => x.evaluate(handle, request, tm),
            Task::Debug(x) => x.evaluate(handle, request, tm),
            Task::Fetch(x) => x.evaluate(handle, request, tm),
            Task::Directory(x) => x.evaluate(handle, request, tm),
            Task::Dnf(x) => x.evaluate(handle, request, tm),
            Task::Echo(x) => x.evaluate(handle, request, tm),
            Task::External(x) => x.evaluate(handle, request, tm),
            Task::Facts(x) => x.evaluate(handle, request, tm),
            Task::Fail(x) => x.evaluate(handle, request, tm),
            Task::Download(x) => x.evaluate(handle, request, tm),
            Task::File(x) => x.evaluate(handle, request, tm),
            Task::Git(x) => x.evaluate(handle, request, tm),
            Task::Github_Release(x) => x.evaluate(handle, request, tm),
            Task::Group(x) => x.evaluate(handle, request, tm),
            Task::Homebrew(x) => x.evaluate(handle, request, tm),
            Task::Instantiate(x) => x.evaluate(handle, request, tm),
            Task::Move(x) => x.evaluate(handle, request, tm),
            Task::Pacman(x) => x.evaluate(handle, request, tm),
            Task::Proxmox_Lxc(x) => x.evaluate(handle, request, tm),
            Task::Proxmox_Migrate(x) => x.evaluate(handle, request, tm),
            Task::Proxmox_Node(x) => x.evaluate(handle, request, tm),
            Task::Sd_Service(x) => x.evaluate(handle, request, tm),
            Task::Self_Locate(x) => x.evaluate(handle, request, tm),
            Task::Set(x) => x.evaluate(handle, request, tm),
            Task::Command(x) => x.evaluate(handle, request, tm),
            Task::Shell(x) => x.evaluate(handle, request, tm),
            Task::Stat(x) => x.evaluate(handle, request, tm),
            Task::Template(x) => x.evaluate(handle, request, tm),
            Task::Unpack(x) => x.evaluate(handle, request, tm),
            Task::User(x) => x.evaluate(handle, request, tm),
            Task::Wait_For_Host(x) => x.evaluate(handle, request, tm),
            Task::Wait_For_Http(x) => x.evaluate(handle, request, tm),
            Task::Wait_For_Others(x) => x.evaluate(handle, request, tm),
            Task::Yum(x) => x.evaluate(handle, request, tm),
            Task::Zypper(x) => x.evaluate(handle, request, tm),
        }
    }

    // ==== END MODULE REGISTRY CONFIG ====

    pub fn get_display_name(&self) -> String {
        match self.get_name() {
            Some(x) => x,
            _ => self.get_module(),
        }
    }

    /// Returns true if this task is a `wait_for_others` barrier task.
    pub fn is_wait_for_others(&self) -> bool {
        matches!(self, Task::Wait_For_Others(_))
    }

    /// If this is a `wait_for_others` task, returns whether it's in strict mode.
    pub fn is_wait_for_others_strict(&self) -> bool {
        match self {
            Task::Wait_For_Others(t) => t
                .mode
                .as_ref()
                .map(|m| m.eq_ignore_ascii_case("strict"))
                .unwrap_or(false),
            _ => false,
        }
    }

    /// Documentation category for this module (used by `jetpack gen-reference`).
    /// The exhaustive match means a new module variant won't compile until it has
    /// a category here — so the docs reference can't silently drop a module.
    pub fn category(&self) -> &'static str {
        match self {
            Task::Group(_) | Task::User(_) => "access",
            Task::Command(_) | Task::External(_) | Task::Shell(_) => "commands",
            Task::Assert(_)
            | Task::Debug(_)
            | Task::Echo(_)
            | Task::Facts(_)
            | Task::Fail(_)
            | Task::Self_Locate(_)
            | Task::Set(_)
            | Task::Wait_For_Host(_)
            | Task::Wait_For_Http(_)
            | Task::Wait_For_Others(_) => "control",
            Task::Copy(_)
            | Task::Directory(_)
            | Task::Download(_)
            | Task::Fetch(_)
            | Task::File(_)
            | Task::Git(_)
            | Task::Move(_)
            | Task::Stat(_)
            | Task::Template(_)
            | Task::Unpack(_) => "files",
            Task::Github_Release(_) => "integrations",
            Task::Instantiate(_) => "inventory",
            Task::Proxmox_Lxc(_) | Task::Proxmox_Migrate(_) | Task::Proxmox_Node(_) => "proxmox",
            Task::Apt(_)
            | Task::Dnf(_)
            | Task::Homebrew(_)
            | Task::Pacman(_)
            | Task::Yum(_)
            | Task::Zypper(_) => "packages",
            Task::Sd_Service(_) => "services",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use strum::IntoEnumIterator;

    const KNOWN_CATEGORIES: [&str; 9] = [
        "access",
        "commands",
        "control",
        "files",
        "integrations",
        "inventory",
        "packages",
        "proxmox",
        "services",
    ];

    #[test]
    fn every_module_tag_is_lowercase_snake() {
        // the canonical YAML tag is the lowercased enum variant; derive it from
        // the variant (via AsRefStr), never from get_module() — some modules'
        // MODULE consts drift to "Download"/"Command" casing.
        for variant in Task::iter() {
            let tag = variant.as_ref();
            assert!(
                !tag.is_empty() && tag.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
                "module tag {:?} must be lowercase snake_case",
                tag
            );
        }
    }

    #[test]
    fn every_module_has_a_known_category() {
        for variant in Task::iter() {
            assert!(
                KNOWN_CATEGORIES.contains(&variant.category()),
                "module {} has unknown category {:?}",
                variant.as_ref(),
                variant.category()
            );
        }
    }

    #[test]
    fn yum_and_dnf_are_both_registered() {
        // two tags, one struct (YumDnfTask) — both must appear in the reference.
        let tags: Vec<String> = Task::iter().map(|v| v.as_ref().to_string()).collect();
        assert!(tags.iter().any(|t| t == "yum"), "yum tag missing");
        assert!(tags.iter().any(|t| t == "dnf"), "dnf tag missing");
    }
}
