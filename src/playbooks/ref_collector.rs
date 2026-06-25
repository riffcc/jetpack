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

//! Read-only walk of a playbook (and the roles it pulls in) that collects, for
//! the missing-secrets diagnostic:
//!
//! - **referenced** — every template variable referenced anywhere, inline in
//!   task fields or inside `!template` source files ("Set A": what the run uses);
//! - **defined** — every variable *defined* in the playbook itself (play
//!   `vars`/`defaults`, role `defaults`, role-invocation `vars`, `vars_files`),
//!   so the diagnostic does not flag a variable that has a default here.
//!
//! The diagnostic subtracts `referenced` from the full available set (inventory
//! variables + `extra_vars` + these `defined` names + builtins) to name the
//! variables that only the secrets overlay would supply.
//!
//! The walk mirrors `traversal`'s role/task/template resolution (reusing
//! [`resolve_role`]) with no `chdir` and no execution — paths are joined
//! explicitly. Each task file is parsed twice: once as a generic YAML value (so
//! inline templated fields of *any* task type are walked uniformly), and once as
//! typed tasks (to follow `!template` `src` files at their known field).

use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::playbooks::language::Play;
use crate::playbooks::template_refs::{referenced_variables, referenced_variables_in_value};
use crate::playbooks::traversal::resolve_role;
use crate::registry::list::Task;

/// Variables collected from a playbook tree.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct CollectedVariables {
    /// Variables the playbook references in templates (Set A).
    pub referenced: BTreeSet<String>,
    /// Variables the playbook itself defines (play/role/invocation/vars_files).
    pub defined: BTreeSet<String>,
}

/// Walk the given playbooks (and the roles they pull in, dependencies resolved
/// recursively) and collect their referenced and defined variables.
///
/// Pure: reads files, changes nothing. A genuinely broken playbook (missing
/// role, unreadable file, circular role dependency) yields an `Err` matching
/// what a real run would hit; the caller decides whether to tolerate it for a
/// best-effort diagnostic.
pub fn collect_variables(
    playbook_paths: &[PathBuf],
    role_paths: &[PathBuf],
) -> Result<CollectedVariables, String> {
    let mut acc = CollectedVariables::default();
    for playbook_path in playbook_paths {
        collect_from_playbook(playbook_path, role_paths, &mut acc)?;
    }
    Ok(acc)
}

fn collect_from_playbook(
    playbook_path: &Path,
    role_paths: &[PathBuf],
    acc: &mut CollectedVariables,
) -> Result<(), String> {
    let source = fs::read_to_string(playbook_path).map_err(|e| {
        format!(
            "could not read playbook '{}': {}",
            playbook_path.display(),
            e
        )
    })?;

    // Inline references anywhere in the playbook document (loose task fields,
    // play var values, names, …) — a single generic walk covers every field.
    if let Ok(value) = serde_yaml::from_str::<serde_yaml::Value>(&source) {
        acc.referenced.extend(referenced_variables_in_value(&value));
    }

    let plays: Vec<Play> = serde_yaml::from_str(&source).map_err(|e| {
        format!(
            "could not parse playbook '{}': {}",
            playbook_path.display(),
            e
        )
    })?;
    let playbook_dir = playbook_path.parent().unwrap_or_else(|| Path::new("."));

    for play in plays.iter() {
        // Variables defined at the play level.
        if let Some(defaults) = play.defaults.as_ref() {
            acc.defined.extend(mapping_keys(defaults));
        }
        if let Some(vars) = play.vars.as_ref() {
            acc.defined.extend(mapping_keys(vars));
        }
        if let Some(vars_files) = play.vars_files.as_ref() {
            for file in vars_files.iter() {
                collect_defined_from_vars_file(playbook_dir, file, acc);
            }
        }

        // Loose tasks and handlers: follow any `!template` source files.
        follow_template_tasks(play.tasks.as_ref(), playbook_dir, acc);
        follow_template_tasks(play.handlers.as_ref(), playbook_dir, acc);

        // Roles: walk each one's task/handler files (and their templates),
        // resolving dependencies recursively with cycle detection.
        if let Some(roles) = play.roles.as_ref() {
            let mut seen: HashSet<String> = HashSet::new();
            let mut stack: Vec<String> = Vec::new();
            for invocation in roles.iter() {
                if let Some(vars) = invocation.vars.as_ref() {
                    acc.defined.extend(mapping_keys(vars));
                }
                collect_from_role(&invocation.role, role_paths, &mut seen, &mut stack, acc)?;
            }
        }
    }
    Ok(())
}

fn collect_from_role(
    role_name: &str,
    role_paths: &[PathBuf],
    seen: &mut HashSet<String>,
    stack: &mut Vec<String>,
    acc: &mut CollectedVariables,
) -> Result<(), String> {
    // A role already on the current chain is a cycle (check before the dedup, so
    // a cycle is never masked by a partial visit — mirroring `process_role`).
    if stack.iter().any(|r| r == role_name) {
        let cycle: Vec<String> = stack
            .iter()
            .cloned()
            .chain([role_name.to_string()])
            .collect();
        return Err(format!(
            "circular role dependency detected: {}",
            cycle.join(" -> ")
        ));
    }
    // A role already fully processed can be skipped.
    if seen.contains(role_name) {
        return Ok(());
    }
    stack.push(role_name.to_string());

    let (role, role_path) = resolve_role(role_paths, role_name)?;
    if let Some(defaults) = role.defaults.as_ref() {
        acc.defined.extend(mapping_keys(defaults));
    }

    // Dependencies first, mirroring traversal.
    if let Some(deps) = role.dependencies.as_ref() {
        for dep in deps.iter() {
            collect_from_role(dep, role_paths, seen, stack, acc)?;
        }
    }

    // Walk both task and handler files; templates under either can reference
    // variables the secrets overlay would supply.
    if let Some(files) = role.tasks.as_ref() {
        for file in files.iter() {
            let path = resolve_role_file(&role_path, file, "tasks");
            collect_from_task_file(&path, &role_path, acc)?;
        }
    }
    if let Some(files) = role.handlers.as_ref() {
        for file in files.iter() {
            let path = resolve_role_file(&role_path, file, "handlers");
            collect_from_task_file(&path, &role_path, acc)?;
        }
    }

    stack.pop();
    seen.insert(role_name.to_string());
    Ok(())
}

fn collect_from_task_file(
    path: &Path,
    role_root: &Path,
    acc: &mut CollectedVariables,
) -> Result<(), String> {
    let source = match fs::read_to_string(path) {
        Ok(s) => s,
        // A referenced task file that is absent is a real playbook error, but it
        // is not what this diagnostic is about — skip it and keep collecting.
        Err(_) => return Ok(()),
    };

    // Inline references across the whole task file (any task type's fields).
    if let Ok(value) = serde_yaml::from_str::<serde_yaml::Value>(&source) {
        acc.referenced.extend(referenced_variables_in_value(&value));
    }

    // `!template` source files: read and extract their references too.
    let tasks: Vec<Task> = match serde_yaml::from_str(&source) {
        Ok(tasks) => tasks,
        Err(_) => return Ok(()),
    };
    for task in tasks {
        if let Task::Template(template_task) = task {
            acc.referenced
                .extend(template_file_refs(role_root, &template_task.src));
        }
    }
    Ok(())
}

// Add a vars_file's top-level keys to the `defined` set. Relative paths resolve
// against the playbook directory (as traversal does); unreadable or unparseable
// files are skipped — not what this diagnostic reports.
fn collect_defined_from_vars_file(playbook_dir: &Path, file: &str, acc: &mut CollectedVariables) {
    let path = Path::new(file);
    let resolved = if path.is_absolute() {
        path.to_path_buf()
    } else {
        playbook_dir.join(file)
    };
    let Ok(source) = fs::read_to_string(&resolved) else {
        return;
    };
    let Ok(value) = serde_yaml::from_str::<serde_yaml::Value>(&source) else {
        return;
    };
    if let serde_yaml::Value::Mapping(map) = value {
        acc.defined.extend(mapping_keys(&map));
    }
}

// Absolute task/handler filenames are used as-is; relative ones live under the
// role's `tasks/` or `handlers/` subdir (exactly as `process_role` resolves).
fn resolve_role_file(role_root: &Path, file: &str, subdir: &str) -> PathBuf {
    let p = Path::new(file);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        role_root.join(subdir).join(file)
    }
}

// `!template` `src` resolves against the role/playbook root, or its `templates/`
// subdir — the same two candidates `syntax_validate_task` checks.
fn resolve_template_src(root: &Path, src: &str) -> Option<PathBuf> {
    let p = Path::new(src);
    if p.is_absolute() {
        return p.is_file().then(|| p.to_path_buf());
    }
    [root.join(src), root.join("templates").join(src)]
        .into_iter()
        .find(|c| c.is_file())
}

// Variables referenced inside a `!template` source file. Resolution, read, and
// extraction failures all collapse to an empty set — a missing or malformed
// template is a different error than the one this diagnostic reports.
fn template_file_refs(root: &Path, src: &str) -> BTreeSet<String> {
    let path = match resolve_template_src(root, src) {
        Some(p) => p,
        None => return BTreeSet::new(),
    };
    let content = match fs::read_to_string(&path) {
        Ok(c) => c,
        Err(_) => return BTreeSet::new(),
    };
    referenced_variables(&content).unwrap_or_default()
}

// Follow `!template` source files in a list of typed tasks (loose play tasks /
// handlers), resolving `src` against the playbook directory.
fn follow_template_tasks(tasks: Option<&Vec<Task>>, root: &Path, acc: &mut CollectedVariables) {
    if let Some(tasks) = tasks {
        for task in tasks {
            if let Task::Template(template_task) = task {
                acc.referenced
                    .extend(template_file_refs(root, &template_task.src));
            }
        }
    }
}

// The string keys of a YAML mapping — these are the variable names it defines.
fn mapping_keys(mapping: &serde_yaml::Mapping) -> impl Iterator<Item = String> + '_ {
    mapping
        .keys()
        .filter_map(|k| k.as_str().map(|s| s.to_string()))
}

#[cfg(test)]
mod tests {
    use super::{CollectedVariables, collect_variables};
    use std::collections::BTreeSet;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn set(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    // Build a temp tree and return (playbook_path, roles_dir).
    fn fixture<F: FnOnce(&Path)>(build: F) -> (TempDir, std::path::PathBuf) {
        let dir = TempDir::new().unwrap();
        build(dir.path());
        let playbook = dir.path().join("site.yml");
        (dir, playbook)
    }

    #[test]
    fn collects_inline_references_from_loose_tasks() {
        let (_tmp, playbook) = fixture(|root| {
            fs::write(
                root.join("site.yml"),
                "- name: loose\n  groups: [all]\n  tasks:\n    - !echo\n      msg: \"hello {{ who }}\"\n",
            )
            .unwrap();
        });
        let collected = collect_variables(&[playbook], &[]).unwrap();
        assert_eq!(collected.referenced, set(&["who"]));
        assert!(collected.defined.is_empty());
    }

    #[test]
    fn collects_from_a_template_source_file() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("templates")).unwrap();
        fs::write(
            dir.path().join("templates").join("redis.conf.hb"),
            "port {{ redis_port }}\nbind {{ bind_addr }}\n",
        )
        .unwrap();
        fs::write(
            dir.path().join("site.yml"),
            "- name: t\n  groups: [all]\n  tasks:\n    - !template\n      src: redis.conf.hb\n      dest: /etc/redis/redis.conf\n",
        )
        .unwrap();
        let playbook = dir.path().join("site.yml");
        let collected = collect_variables(&[playbook], &[]).unwrap();
        assert_eq!(collected.referenced, set(&["bind_addr", "redis_port"]));
    }

    #[test]
    fn collects_from_role_tasks_and_handlers_and_dependencies() {
        let dir = tempfile::tempdir().unwrap();
        let roles_root = dir.path().join("roles");
        fs::create_dir_all(roles_root.join("base/tasks")).unwrap();
        fs::create_dir_all(roles_root.join("base/handlers")).unwrap();
        fs::create_dir_all(roles_root.join("base/templates")).unwrap();
        fs::write(
            roles_root.join("base/role.yml"),
            "name: base\ndefaults:\n  base_token: default\ndependencies: [shared]\ntasks: [main.yml]\nhandlers: [handlers.yml]\n",
        )
        .unwrap();
        fs::write(
            roles_root.join("base/tasks/main.yml"),
            "- !echo\n  msg: \"cfg={{ base_token }}\"\n- !template\n  src: app.conf.hb\n  dest: /etc/app.conf\n",
        )
        .unwrap();
        fs::write(
            roles_root.join("base/handlers/handlers.yml"),
            "- !command\n  argv: [systemctl, restart, \"{{ svc }}\"]\n",
        )
        .unwrap();
        fs::write(
            roles_root.join("base/templates/app.conf.hb"),
            "secret = {{ app_secret }}\n",
        )
        .unwrap();
        fs::create_dir_all(roles_root.join("shared/tasks")).unwrap();
        fs::write(
            roles_root.join("shared/role.yml"),
            "name: shared\ntasks: [main.yml]\n",
        )
        .unwrap();
        fs::write(
            roles_root.join("shared/tasks/main.yml"),
            "- !echo\n  msg: \"shared={{ shared_key }}\"\n",
        )
        .unwrap();
        fs::write(
            dir.path().join("site.yml"),
            "- name: site\n  groups: [all]\n  vars:\n    svc: sshd\n  roles:\n    - role: base\n      vars:\n        app_secret: inline\n",
        )
        .unwrap();

        let playbook = dir.path().join("site.yml");
        let collected = collect_variables(&[playbook], &[roles_root]).unwrap();
        assert_eq!(
            collected.referenced,
            set(&["app_secret", "base_token", "shared_key", "svc"])
        );
        // base_token has a role default; svc is a play var; app_secret is an
        // invocation var — all defined here, so a missing-vars diagnostic would
        // correctly report only shared_key.
        assert_eq!(collected.defined, set(&["app_secret", "base_token", "svc"]));
    }

    #[test]
    fn dedupes_a_role_used_twice() {
        let dir = tempfile::tempdir().unwrap();
        let roles_root = dir.path().join("roles");
        fs::create_dir_all(roles_root.join("r/tasks")).unwrap();
        fs::write(
            roles_root.join("r/role.yml"),
            "name: r\ntasks: [main.yml]\n",
        )
        .unwrap();
        fs::write(
            roles_root.join("r/tasks/main.yml"),
            "- !echo\n  msg: \"{{ x }}\"\n",
        )
        .unwrap();
        fs::write(
            dir.path().join("site.yml"),
            "- name: a\n  groups: [all]\n  roles:\n    - role: r\n- name: b\n  groups: [all]\n  roles:\n    - role: r\n",
        )
        .unwrap();
        let playbook = dir.path().join("site.yml");
        let collected = collect_variables(&[playbook], &[roles_root]).unwrap();
        assert_eq!(collected.referenced, set(&["x"]));
    }

    #[test]
    fn detects_circular_role_dependency() {
        let dir = tempfile::tempdir().unwrap();
        let roles_root = dir.path().join("roles");
        fs::create_dir_all(roles_root.join("a/tasks")).unwrap();
        fs::create_dir_all(roles_root.join("b/tasks")).unwrap();
        fs::write(
            roles_root.join("a/role.yml"),
            "name: a\ndependencies: [b]\ntasks: [main.yml]\n",
        )
        .unwrap();
        fs::write(
            roles_root.join("b/role.yml"),
            "name: b\ndependencies: [a]\ntasks: [main.yml]\n",
        )
        .unwrap();
        fs::write(roles_root.join("a/tasks/main.yml"), "- !echo\n  msg: ok\n").unwrap();
        fs::write(roles_root.join("b/tasks/main.yml"), "- !echo\n  msg: ok\n").unwrap();
        fs::write(
            dir.path().join("site.yml"),
            "- name: site\n  groups: [all]\n  roles:\n    - role: a\n",
        )
        .unwrap();
        let playbook = dir.path().join("site.yml");
        let err = collect_variables(&[playbook], &[roles_root]).unwrap_err();
        assert!(err.contains("circular"), "got: {err}");
    }

    #[test]
    fn collects_defined_names_from_vars_files() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("secrets.yml"), "api_token: from-file\n").unwrap();
        fs::write(
            dir.path().join("site.yml"),
            "- name: site\n  groups: [all]\n  vars_files:\n    - secrets.yml\n  tasks:\n    - !echo\n      msg: \"t={{ api_token }}\"\n",
        )
        .unwrap();
        let playbook = dir.path().join("site.yml");
        let collected = collect_variables(&[playbook], &[]).unwrap();
        assert_eq!(collected.referenced, set(&["api_token"]));
        assert_eq!(collected.defined, set(&["api_token"]));
    }

    #[test]
    fn empty_playbook_collects_nothing() {
        let (_tmp, playbook) = fixture(|root| {
            fs::write(root.join("site.yml"), "[]\n").unwrap();
        });
        let collected = collect_variables(&[playbook], &[]).unwrap();
        assert_eq!(collected, CollectedVariables::default());
    }
}
