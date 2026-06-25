// Jetporch
// Copyright (C) 2023 - Michael DeHaan <michael@michaeldehaan.net> + contributors
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// long with this program.  If not, see <http://www.gnu.org/licenses/>.

//! The single source of truth for walking a role tree and resolving role /
//! task-file / template-source paths — shared by execution (`process_role`) and
//! static analysis (`ref_collector`) so the two can never drift apart.

use std::path::{Path, PathBuf};

use crate::playbooks::language::{Role, RoleInvocation};
use crate::util::io::jet_file_open;
use crate::util::yaml::show_yaml_error_in_context;

/// Which section of a role is being walked: its tasks or its handlers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoleSection {
    Tasks,
    Handlers,
}

/// The mutable state a role walk needs: per-section "already processed" sets
/// (so a role runs once per section across the whole walk) and a cycle-detection
/// stack. Implemented by `RunState` for execution (backed by its `Arc<RwLock>`
/// fields) and by a small local struct in `ref_collector` for static analysis —
/// both via interior mutability, so every method takes `&self`.
pub trait RoleWalkState {
    fn is_processed(&self, role: &str, section: RoleSection) -> bool;
    fn mark_processed(&self, role: &str, section: RoleSection);
    fn in_stack(&self, role: &str) -> bool;
    fn push_stack(&self, role: &str);
    fn pop_stack(&self);
    fn stack_snapshot(&self) -> Vec<String>;
}

/// Walk a role invocation and its dependencies (dependencies first), calling
/// `on_role` once per resolved role with its invocation, root path, and parsed
/// definition. Cycle detection and per-section dedup are driven by `state`, so
/// the same traversal serves both execution and static analysis.
///
/// `on_role` is called *after* a role's dependencies and *before* it is marked
/// processed — matching `process_role`'s ordering — and receives the invocation
/// (the original, or a synthetic `{ vars: None }` one for a dependency) so the
/// caller can set per-role context.
pub fn walk_role_tree<S, F>(
    state: &S,
    role_paths: &[PathBuf],
    invocation: &RoleInvocation,
    section: RoleSection,
    mut on_role: F,
) -> Result<(), String>
where
    S: RoleWalkState,
    F: FnMut(&RoleInvocation, &Path, &Role) -> Result<(), String>,
{
    // Erase `F` into a trait object for the recursive helper, otherwise the
    // recursion would demand `F = &mut F` (an infinite type).
    walk_role_inner(state, role_paths, invocation, section, &mut on_role)
}

fn walk_role_inner<S: RoleWalkState>(
    state: &S,
    role_paths: &[PathBuf],
    invocation: &RoleInvocation,
    section: RoleSection,
    on_role: &mut dyn FnMut(&RoleInvocation, &Path, &Role) -> Result<(), String>,
) -> Result<(), String> {
    if state.is_processed(&invocation.role, section) {
        return Ok(());
    }
    if state.in_stack(&invocation.role) {
        let mut chain = state.stack_snapshot();
        chain.push(invocation.role.clone());
        return Err(format!(
            "circular role dependency detected: {}",
            chain.join(" -> ")
        ));
    }
    state.push_stack(&invocation.role);

    let (role, role_path) = resolve_role(role_paths, &invocation.role)?;

    // Dependencies first — same ordering as `process_role`.
    if let Some(deps) = role.dependencies.as_ref() {
        for dep in deps {
            let dep_invocation = RoleInvocation {
                role: dep.clone(),
                vars: None,
                tags: invocation.tags.clone(),
            };
            walk_role_inner(state, role_paths, &dep_invocation, section, on_role)?;
        }
    }

    state.pop_stack();
    on_role(invocation, &role_path, &role)?;
    state.mark_processed(&invocation.role, section);
    Ok(())
}

/// Resolve a role name to its parsed definition and root directory by searching
/// the configured role paths. Pure: no `RunState`, no side effects.
pub fn resolve_role(role_paths: &[PathBuf], role_name: &str) -> Result<(Role, PathBuf), String> {
    for path_buf in role_paths.iter() {
        let mut pb = path_buf.clone();
        pb.push(role_name);
        let mut pb2 = pb.clone();
        pb2.push("role.yml");

        // a role.yml file must exist in a directory once we find a directory with a matching name
        if pb2.exists() {
            let path = pb2.as_path();
            let role_file = jet_file_open(path)?;
            let parsed: Result<Role, serde_yaml::Error> = serde_yaml::from_reader(role_file);
            let role = match parsed {
                Ok(role) => role,
                Err(e) => {
                    show_yaml_error_in_context(&e, path);
                    return Err("edit the file and try again?".to_string());
                }
            };
            return Ok((role, pb));
        }
    }
    Err(format!("role not found: {}", role_name))
}

/// Resolve a role's task/handler file path. Absolute paths are used as-is;
/// relative ones live under the role's `tasks/` or `handlers/` subdir.
pub fn resolve_role_file(role_root: &Path, file: &str, section: RoleSection) -> PathBuf {
    let p = Path::new(file);
    if p.is_absolute() {
        return p.to_path_buf();
    }
    let subdir = match section {
        RoleSection::Tasks => "tasks",
        RoleSection::Handlers => "handlers",
    };
    role_root.join(subdir).join(file)
}

/// Resolve a `!template` `src` against a root, trying the root and its
/// `templates/` subdir — the same two candidates `syntax_validate_task` checks.
pub fn resolve_template_src(root: &Path, src: &str) -> Option<PathBuf> {
    let p = Path::new(src);
    if p.is_absolute() {
        return p.is_file().then(|| p.to_path_buf());
    }
    [root.join(src), root.join("templates").join(src)]
        .into_iter()
        .find(|c| c.is_file())
}
