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

//! Per-play variable collection for the missing-secrets diagnostic — what each
//! play references, defines, and targets — using the **same role walk as
//! execution** (`role_tree::walk_role_tree`), so the two can never diverge.
//!
//! Each task file is parsed twice: once as a generic YAML value (so inline
//! templated fields of *any* task type are walked uniformly) and once as typed
//! tasks (to follow `!template` `src` files). Variable extraction itself is the
//! one operation the engine does not do — it discards the raw value and renders
//! per host — so that part stays here.

use std::cell::RefCell;
use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::playbooks::language::Play;
use crate::playbooks::role_tree::{
    RoleSection, RoleWalkState, resolve_role_file, resolve_template_src, walk_role_tree,
};
use crate::playbooks::template_refs::{referenced_variables, referenced_variables_in_value};
use crate::registry::list::Task;

/// Per-play collected variables: what a single play references, what it defines,
/// and the (raw, possibly templated) groups it targets. Kept per-play so the
/// diagnostic can resolve each play's targeted hosts and apply the exact
/// per-play scope formula (see `secrets_diagnostic` + the Lean proof).
#[derive(Debug, Default, PartialEq, Eq)]
pub struct PerPlayVars {
    /// Raw `play.groups` entries (may contain `{{ }}`).
    pub groups: Vec<String>,
    /// Variables the play references in templates.
    pub referenced: BTreeSet<String>,
    /// Variables the play itself defines (play vars/defaults, role defaults,
    /// role-invocation vars, vars_files).
    pub defined: BTreeSet<String>,
}

/// Walk the given playbooks (and the roles they pull in) and collect each play's
/// referenced and defined variables. Pure: reads files, changes nothing. A
/// genuinely broken playbook (missing role, unreadable file, circular role
/// dependency) yields an `Err` matching what a real run would hit.
pub fn collect_per_play(
    playbook_paths: &[PathBuf],
    role_paths: &[PathBuf],
) -> Result<Vec<PerPlayVars>, String> {
    let mut out: Vec<PerPlayVars> = Vec::new();
    for playbook_path in playbook_paths {
        collect_from_playbook(playbook_path, role_paths, &mut out)?;
    }
    Ok(out)
}

fn collect_from_playbook(
    playbook_path: &Path,
    role_paths: &[PathBuf],
    out: &mut Vec<PerPlayVars>,
) -> Result<(), String> {
    let source = fs::read_to_string(playbook_path).map_err(|e| {
        format!(
            "could not read playbook '{}': {}",
            playbook_path.display(),
            e
        )
    })?;

    // Parse twice: as typed plays (structure) and as a generic value (so each
    // play's inline templated fields are walked uniformly). Keep the value as a
    // per-play sequence to attribute inline references to the correct play.
    let whole: serde_yaml::Value = serde_yaml::from_str(&source).unwrap_or(serde_yaml::Value::Null);
    let play_values: Vec<serde_yaml::Value> = match &whole {
        serde_yaml::Value::Sequence(seq) => seq.clone(),
        _ => Vec::new(),
    };

    let plays: Vec<Play> = serde_yaml::from_str(&source).map_err(|e| {
        format!(
            "could not parse playbook '{}': {}",
            playbook_path.display(),
            e
        )
    })?;
    let playbook_dir = playbook_path.parent().unwrap_or_else(|| Path::new("."));

    for (index, play) in plays.iter().enumerate() {
        let mut acc = PerPlayVars {
            groups: play.groups.clone(),
            referenced: BTreeSet::new(),
            defined: BTreeSet::new(),
        };

        // Inline references in this play's own YAML (loose task fields, play var
        // values, names, …) — walked generically so every field type is covered.
        if let Some(play_value) = play_values.get(index) {
            acc.referenced
                .extend(referenced_variables_in_value(play_value));
        }

        // Variables defined at the play level.
        if let Some(defaults) = play.defaults.as_ref() {
            acc.defined.extend(mapping_keys(defaults));
        }
        if let Some(vars) = play.vars.as_ref() {
            acc.defined.extend(mapping_keys(vars));
        }
        if let Some(vars_files) = play.vars_files.as_ref() {
            for file in vars_files.iter() {
                collect_defined_from_vars_file(playbook_dir, file, &mut acc.defined);
            }
        }

        // Loose tasks and handlers: follow any `!template` source files.
        follow_template_tasks(play.tasks.as_ref(), playbook_dir, &mut acc.referenced);
        follow_template_tasks(play.handlers.as_ref(), playbook_dir, &mut acc.referenced);

        // Roles: walk with the SAME traversal the engine uses (deps-first,
        // cycle-detect, per-section dedup), collecting references + role-defined
        // defaults from each task/handler file.
        if let Some(roles) = play.roles.as_ref() {
            let state = CollectorWalkState::default();
            for invocation in roles.iter() {
                if let Some(vars) = invocation.vars.as_ref() {
                    acc.defined.extend(mapping_keys(vars));
                }
                for section in [RoleSection::Tasks, RoleSection::Handlers] {
                    let acc_ref = &mut acc;
                    walk_role_tree(
                        &state,
                        role_paths,
                        invocation,
                        section,
                        |_inv, role_root, role| {
                            if let Some(defaults) = role.defaults.as_ref() {
                                acc_ref.defined.extend(mapping_keys(defaults));
                            }
                            let files = match section {
                                RoleSection::Tasks => role.tasks.as_ref(),
                                RoleSection::Handlers => role.handlers.as_ref(),
                            };
                            if let Some(files) = files {
                                for file in files.iter() {
                                    let path = resolve_role_file(role_root, file, section);
                                    collect_from_task_file(&path, role_root, acc_ref);
                                }
                            }
                            Ok(())
                        },
                    )?;
                }
            }
        }

        out.push(acc);
    }
    Ok(())
}

// Collect references from one task/handler file: inline templated fields (via a
// generic value walk) plus any `!template` source files it references.
fn collect_from_task_file(path: &Path, role_root: &Path, acc: &mut PerPlayVars) {
    let Ok(source) = fs::read_to_string(path) else {
        return; // an absent referenced file is a different error than this diagnostic
    };
    if let Ok(value) = serde_yaml::from_str::<serde_yaml::Value>(&source) {
        acc.referenced.extend(referenced_variables_in_value(&value));
    }
    let Ok(tasks) = serde_yaml::from_str::<Vec<Task>>(&source) else {
        return;
    };
    for task in tasks {
        if let Task::Template(template_task) = task {
            acc.referenced
                .extend(template_file_refs(role_root, &template_task.src));
        }
    }
}

// Follow `!template` source files in a list of typed tasks (loose play tasks /
// handlers), resolving `src` against the playbook directory.
fn follow_template_tasks(
    tasks: Option<&Vec<Task>>,
    root: &Path,
    referenced: &mut BTreeSet<String>,
) {
    if let Some(tasks) = tasks {
        for task in tasks {
            if let Task::Template(template_task) = task {
                referenced.extend(template_file_refs(root, &template_task.src));
            }
        }
    }
}

// Variables referenced inside a `!template` source file. Resolution, read, and
// extraction failures all collapse to an empty set — a missing or malformed
// template is a different error than the one this diagnostic reports.
fn template_file_refs(root: &Path, src: &str) -> BTreeSet<String> {
    let Some(path) = resolve_template_src(root, src) else {
        return BTreeSet::new();
    };
    let Ok(content) = fs::read_to_string(&path) else {
        return BTreeSet::new();
    };
    referenced_variables(&content).unwrap_or_default()
}

// Add a vars_file's top-level keys to the `defined` set. Relative paths resolve
// against the playbook directory (as traversal does); unreadable or unparseable
// files are skipped — not what this diagnostic reports.
fn collect_defined_from_vars_file(playbook_dir: &Path, file: &str, defined: &mut BTreeSet<String>) {
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
        defined.extend(mapping_keys(&map));
    }
}

// The string keys of a YAML mapping — these are the variable names it defines.
fn mapping_keys(mapping: &serde_yaml::Mapping) -> impl Iterator<Item = String> + '_ {
    mapping
        .keys()
        .filter_map(|k| k.as_str().map(|s| s.to_string()))
}

// Local walk state for static analysis — mirrors the per-section dedup and cycle
// stack `RunState` provides to execution, so both drive `walk_role_tree` with
// identical semantics.
#[derive(Default)]
struct CollectorWalkState {
    processed_tasks: RefCell<HashSet<String>>,
    processed_handlers: RefCell<HashSet<String>>,
    stack: RefCell<Vec<String>>,
}

impl RoleWalkState for CollectorWalkState {
    fn is_processed(&self, role: &str, section: RoleSection) -> bool {
        match section {
            RoleSection::Tasks => self.processed_tasks.borrow().contains(role),
            RoleSection::Handlers => self.processed_handlers.borrow().contains(role),
        }
    }

    fn mark_processed(&self, role: &str, section: RoleSection) {
        let set = match section {
            RoleSection::Tasks => &self.processed_tasks,
            RoleSection::Handlers => &self.processed_handlers,
        };
        set.borrow_mut().insert(role.to_string());
    }

    fn in_stack(&self, role: &str) -> bool {
        self.stack.borrow().iter().any(|r| r == role)
    }

    fn push_stack(&self, role: &str) {
        self.stack.borrow_mut().push(role.to_string());
    }

    fn pop_stack(&self) {
        self.stack.borrow_mut().pop();
    }

    fn stack_snapshot(&self) -> Vec<String> {
        self.stack.borrow().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::{PerPlayVars, collect_per_play};
    use std::collections::BTreeSet;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    fn set(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    // Collect and return the single play's variables (all fixtures here have one).
    fn single(playbook: &Path, roles: &[std::path::PathBuf]) -> PerPlayVars {
        let per = collect_per_play(&[playbook.to_path_buf()], roles).unwrap();
        assert_eq!(per.len(), 1, "expected exactly one play, got {}", per.len());
        per.into_iter().next().unwrap()
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
        let collected = single(&playbook, &[]);
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
        let collected = single(&playbook, &[]);
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
        let collected = single(&playbook, &[roles_root]);
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
        let per = collect_per_play(&[playbook], &[roles_root]).unwrap();
        // two plays, each invoking r; both reference x exactly once.
        assert_eq!(per.len(), 2);
        assert!(per.iter().all(|p| p.referenced == set(&["x"])));
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
        let err = collect_per_play(&[playbook], &[roles_root]).unwrap_err();
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
        let collected = single(&playbook, &[]);
        assert_eq!(collected.referenced, set(&["api_token"]));
        assert_eq!(collected.defined, set(&["api_token"]));
    }

    #[test]
    fn empty_playbook_collects_nothing() {
        let (_tmp, playbook) = fixture(|root| {
            fs::write(root.join("site.yml"), "[]\n").unwrap();
        });
        let per = collect_per_play(&[playbook], &[]).unwrap();
        assert!(per.is_empty());
    }
}
