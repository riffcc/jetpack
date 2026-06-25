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

//! The missing-secrets variable diagnostic.
//!
//! When `secrets_inventory` is declared but absent (a non-mutating run, which
//! skips the overlay rather than failing — see issue #55), the operator wants to
//! know the blast radius: which variables would be undefined. This names them by
//! subtracting every variable the run can resolve *without* the overlay
//! (inventory variables, `extra_vars`, variables the playbook itself defines, and
//! engine builtins) from every variable the playbook *references*.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use crate::inventory::inventory::Inventory;
use crate::playbooks::ref_collector::collect_variables;

// Builtins injected at render time (per host / per sudo template), which are NOT
// in `extra_vars` (that only carries the control-node `JET_*` vars). Always
// available, so never reported as missing.
const RENDER_BUILTINS: &[&str] = &[
    "jet_hostname",
    "jet_hostname_short",
    "jet_play_hosts",
    "jet_sudo_user",
    "jet_command",
];

/// Variables the run references but cannot resolve without the secrets overlay —
/// i.e. the names an operator should expect to be undefined.
///
/// Best-effort: if the playbook cannot be walked (genuinely broken — missing
/// role, circular dependency), this returns an empty set so the caller's basic
/// "skipping secrets overlay" notice still prints undisturbed; the broken
/// playbook will surface its real error when the run proceeds.
pub fn missing_secret_variables(
    playbook_paths: &[PathBuf],
    role_paths: &[PathBuf],
    inventory: &Arc<RwLock<Inventory>>,
    extra_vars: &serde_yaml::Value,
) -> BTreeSet<String> {
    let collected = match collect_variables(playbook_paths, role_paths) {
        Ok(c) => c,
        Err(_) => return BTreeSet::new(),
    };
    let available = available_variable_names(inventory, extra_vars, &collected.defined);
    collected
        .referenced
        .into_iter()
        .filter(|name| !available.contains(name))
        .collect()
}

/// Every variable name resolvable without the secrets overlay: what the playbook
/// defines, plus inventory group/host variables, plus `extra_vars` (which already
/// includes the `JET_*` control-node builtins), plus the render-time builtins.
fn available_variable_names(
    inventory: &Arc<RwLock<Inventory>>,
    extra_vars: &serde_yaml::Value,
    playbook_defined: &BTreeSet<String>,
) -> BTreeSet<String> {
    let mut out: BTreeSet<String> = playbook_defined.iter().cloned().collect();

    if let Some(map) = extra_vars.as_mapping() {
        for key in map.keys() {
            if let Some(name) = key.as_str() {
                out.insert(name.to_string());
            }
        }
    }
    for builtin in RENDER_BUILTINS {
        out.insert((*builtin).to_string());
    }

    let inv = inventory.read().expect("inventory read");
    for group in inv.groups.values() {
        let group = group.read().expect("group read");
        for key in group.get_variables().keys() {
            if let Some(name) = key.as_str() {
                out.insert(name.to_string());
            }
        }
    }
    for host in inv.hosts.values() {
        let host = host.read().expect("host read");
        for key in host.get_variables().keys() {
            if let Some(name) = key.as_str() {
                out.insert(name.to_string());
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::missing_secret_variables;
    use crate::inventory::inventory::Inventory;
    use std::collections::BTreeSet;
    use std::fs;
    use std::sync::{Arc, RwLock};
    use tempfile::TempDir;

    fn mapping(items: &[&str]) -> serde_yaml::Value {
        let mut map = serde_yaml::Mapping::new();
        for item in items {
            map.insert(
                serde_yaml::Value::String(item.to_string()),
                serde_yaml::Value::String("value".to_string()),
            );
        }
        serde_yaml::Value::Mapping(map)
    }

    fn inventory_with_group_vars(group: &str, vars: &[&str]) -> Arc<RwLock<Inventory>> {
        let mut inv = Inventory::new();
        inv.store_host(group, "h1");
        if let serde_yaml::Value::Mapping(m) = mapping(vars) {
            inv.store_group_variables(group, m);
        }
        Arc::new(RwLock::new(inv))
    }

    // Playbook + role fixtures return the temp dir (kept alive) and playbook path.
    struct Fixture {
        _dir: TempDir,
        playbook: std::path::PathBuf,
    }

    fn playbook(content: &str) -> Fixture {
        let dir = TempDir::new().unwrap();
        let playbook = dir.path().join("site.yml");
        fs::write(&playbook, content).unwrap();
        Fixture {
            _dir: dir,
            playbook,
        }
    }

    #[test]
    fn names_only_the_secret_only_variable() {
        // References: secret_only (no source), public_var (in inventory),
        // defaulted (play var), and jet_hostname (render builtin).
        let fx = playbook(
            "- name: site\n  groups: [all]\n  vars:\n    defaulted: d\n  tasks:\n    \
             - !echo\n      msg: \"a={{ secret_only }} b={{ public_var }} \
             c={{ defaulted }} d={{ jet_hostname }}\"\n",
        );
        let inventory = inventory_with_group_vars("all", &["public_var"]);
        let extra = serde_yaml::Value::Mapping(serde_yaml::Mapping::new());
        let missing = missing_secret_variables(&[fx.playbook.clone()], &[], &inventory, &extra);
        let expected: BTreeSet<String> = ["secret_only"].iter().map(|s| s.to_string()).collect();
        assert_eq!(missing, expected);
    }

    #[test]
    fn extra_vars_cover_their_references() {
        let fx = playbook(
            "- name: site\n  groups: [all]\n  tasks:\n    - !echo\n      msg: \"{{ from_e }}\"\n",
        );
        let inventory = inventory_with_group_vars("all", &[]);
        let extra = mapping(&["from_e"]);
        let missing = missing_secret_variables(&[fx.playbook.clone()], &[], &inventory, &extra);
        assert!(missing.is_empty(), "got: {missing:?}");
    }

    #[test]
    fn empty_when_playbook_has_no_references() {
        let fx = playbook(
            "- name: site\n  groups: [all]\n  tasks:\n    - !echo\n      msg: \"plain text\"\n",
        );
        let inventory = inventory_with_group_vars("all", &[]);
        let extra = serde_yaml::Value::Mapping(serde_yaml::Mapping::new());
        let missing = missing_secret_variables(&[fx.playbook.clone()], &[], &inventory, &extra);
        assert!(missing.is_empty());
    }

    #[test]
    fn returns_empty_for_an_unparseable_playbook() {
        // Best-effort: a broken playbook yields no names, not an abort.
        let fx = playbook("this: : is not\nvalid playbook: yaml:\n  - [");
        let inventory = inventory_with_group_vars("all", &[]);
        let extra = serde_yaml::Value::Mapping(serde_yaml::Mapping::new());
        let missing = missing_secret_variables(&[fx.playbook.clone()], &[], &inventory, &extra);
        assert!(missing.is_empty());
    }
}
