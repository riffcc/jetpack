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

//! The missing-secrets variable diagnostic — *exact* per-play scope.
//!
//! When `secrets_inventory` is declared but absent (a non-mutating run skips the
//! overlay rather than failing — see issue #55), this names the variables that
//! would be undefined, using the proven-exact per-play formula:
//!
//! ```text
//! Missing_p = R(p) \ ( D(p) ∪ G ∪ B ∪ ⋂_{h ∈ H(p)} I(h) )
//! ```
//!
//! where `R(p)` is what play `p` references, `D(p)` what it defines, `G` the
//! `extra_vars` (which already carry the `JET_*` builtins), `B` the render-time
//! builtins, `I(h)` host `h`'s blended inventory scope, and `H(p)` the hosts `p`
//! targets. The Lean theorem `missing_per_play_exact` proves this equals the
//! per-(play, host) semantics `⋃_{h ∈ H(p)} ( R(p) \ (D(p) ∪ G ∪ B ∪ I(h)) )` —
//! so it has no false positives and no false negatives.
//!
//! Earlier this used a run-wide *union* approximation (`⋂` → `⋃` over all hosts),
//! which under-reports (a var defined only in a non-targeted group's hosts) and,
//! for empty-target plays, over-reports. The intersection form fixes both.

use std::collections::{BTreeSet, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use crate::inventory::hosts::Host;
use crate::inventory::inventory::Inventory;
use crate::playbooks::ref_collector::collect_per_play;

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

/// Variables the run references but cannot resolve without the secrets overlay.
///
/// Best-effort: if the playbook cannot be walked (genuinely broken — missing
/// role, circular dependency), this returns an empty set so the caller's basic
/// "skipping secrets overlay" notice still prints undisturbed.
pub fn missing_secret_variables(
    playbook_paths: &[PathBuf],
    role_paths: &[PathBuf],
    inventory: &Arc<RwLock<Inventory>>,
    extra_vars: &serde_yaml::Value,
) -> BTreeSet<String> {
    let per_play = match collect_per_play(playbook_paths, role_paths) {
        Ok(p) => p,
        Err(_) => return BTreeSet::new(),
    };
    let global = global_keys(extra_vars);
    let inv = inventory.read().expect("inventory read");
    let all_host_keys = union_all_host_scope_keys(&inv);

    let mut out = BTreeSet::new();
    for play in &per_play {
        let miss = match resolve_target_hosts(&play.groups, &inv) {
            // Exact per-play formula: intersect the targeted hosts' scopes.
            Some(hosts) if !hosts.is_empty() => {
                let avail = available_exact(&play.defined, &global, &intersect_host_scopes(&hosts));
                diff(&play.referenced, &avail)
            }
            // A play that targets no host renders nowhere -> contributes nothing.
            Some(_) => BTreeSet::new(),
            // Templated or unknown groups: targets can't be resolved statically,
            // so fall back to the run-wide-union view for this play (sound; may
            // under-report) rather than guess at host membership.
            None => {
                let avail = available_exact(&play.defined, &global, &all_host_keys);
                diff(&play.referenced, &avail)
            }
        };
        out.extend(miss);
    }
    out
}

// Resolve a play's groups to the hosts it targets. Returns `None` when a group
// is templated (`{{ }}`) or absent from inventory (targets unknowable without a
// full run); otherwise the union of each group's descendant hosts (possibly
// empty, which is exact: the play runs on no host).
fn resolve_target_hosts(groups: &[String], inv: &Inventory) -> Option<Vec<Arc<RwLock<Host>>>> {
    let mut hosts: Vec<Arc<RwLock<Host>>> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for group in groups {
        if group.contains("{{") {
            return None;
        }
        if !inv.has_group(group) {
            return None;
        }
        for (name, host) in inv
            .get_group(group)
            .read()
            .expect("group read")
            .get_descendant_hosts()
        {
            if seen.insert(name) {
                hosts.push(host);
            }
        }
    }
    Some(hosts)
}

fn intersect_host_scopes(hosts: &[Arc<RwLock<Host>>]) -> BTreeSet<String> {
    let mut iter = hosts.iter();
    let Some(first) = iter.next() else {
        return BTreeSet::new();
    };
    let mut acc = host_scope_keys(&first.read().expect("host read"));
    for host in iter {
        let theirs = host_scope_keys(&host.read().expect("host read"));
        acc.retain(|k| theirs.contains(k));
    }
    acc
}

fn union_all_host_scope_keys(inv: &Inventory) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for host in inv.hosts.values() {
        for key in host_scope_keys(&host.read().expect("host read")) {
            out.insert(key);
        }
    }
    out
}

fn host_scope_keys(host: &Host) -> BTreeSet<String> {
    host.get_blended_variables()
        .keys()
        .filter_map(|k| k.as_str().map(|s| s.to_string()))
        .collect()
}

fn global_keys(extra_vars: &serde_yaml::Value) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
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
    out
}

fn available_exact(
    defined: &BTreeSet<String>,
    global: &BTreeSet<String>,
    host_keys: &BTreeSet<String>,
) -> BTreeSet<String> {
    defined
        .iter()
        .chain(global.iter())
        .chain(host_keys.iter())
        .cloned()
        .collect()
}

fn diff(referenced: &BTreeSet<String>, available: &BTreeSet<String>) -> BTreeSet<String> {
    referenced
        .iter()
        .filter(|v| !available.contains(*v))
        .cloned()
        .collect()
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

    fn group_vars_inventory(group: &str, vars: &[&str]) -> Arc<RwLock<Inventory>> {
        let mut inv = Inventory::new();
        inv.store_host(group, "h1");
        if let serde_yaml::Value::Mapping(m) = mapping(vars) {
            inv.store_group_variables(group, m);
        }
        Arc::new(RwLock::new(inv))
    }

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
        let fx = playbook(
            "- name: site\n  groups: [all]\n  vars:\n    defaulted: d\n  tasks:\n    \
             - !echo\n      msg: \"a={{ secret_only }} b={{ public_var }} \
             c={{ defaulted }} d={{ jet_hostname }}\"\n",
        );
        let inventory = group_vars_inventory("all", &["public_var"]);
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
        let inventory = group_vars_inventory("all", &[]);
        let extra = mapping(&["from_e"]);
        let missing = missing_secret_variables(&[fx.playbook.clone()], &[], &inventory, &extra);
        assert!(missing.is_empty(), "got: {missing:?}");
    }

    #[test]
    fn empty_when_playbook_has_no_references() {
        let fx = playbook(
            "- name: site\n  groups: [all]\n  tasks:\n    - !echo\n      msg: \"plain text\"\n",
        );
        let inventory = group_vars_inventory("all", &[]);
        let extra = serde_yaml::Value::Mapping(serde_yaml::Mapping::new());
        let missing = missing_secret_variables(&[fx.playbook.clone()], &[], &inventory, &extra);
        assert!(missing.is_empty());
    }

    #[test]
    fn returns_empty_for_an_unparseable_playbook() {
        let fx = playbook("this: : is not\nvalid playbook: yaml:\n  - [");
        let inventory = group_vars_inventory("all", &[]);
        let extra = serde_yaml::Value::Mapping(serde_yaml::Mapping::new());
        let missing = missing_secret_variables(&[fx.playbook.clone()], &[], &inventory, &extra);
        assert!(missing.is_empty());
    }

    // --- exactness properties the old union approximation got wrong ----------

    #[test]
    fn flags_a_var_defined_only_in_a_non_targeted_group() {
        // secret_in_b lives in gB's host (h2); the play targets gA (h1), which
        // does NOT descend from gB. The exact per-host scope catches this; the
        // old run-wide-union approximation would have silently missed it.
        let mut inv = Inventory::new();
        inv.store_host("gA", "h1");
        inv.store_host("gB", "h2");
        if let serde_yaml::Value::Mapping(m) = mapping(&["secret_in_b"]) {
            inv.store_group_variables("gB", m);
        }
        let inventory = Arc::new(RwLock::new(inv));
        let fx = playbook(
            "- name: site\n  groups: [gA]\n  tasks:\n    - !echo\n      msg: \"{{ secret_in_b }}\"\n",
        );
        let extra = serde_yaml::Value::Mapping(serde_yaml::Mapping::new());
        let missing = missing_secret_variables(&[fx.playbook.clone()], &[], &inventory, &extra);
        let expected: BTreeSet<String> = ["secret_in_b"].iter().map(|s| s.to_string()).collect();
        assert_eq!(missing, expected);
    }

    #[test]
    fn empty_target_play_contributes_nothing() {
        // The play targets a group with no member hosts, so it renders nowhere:
        // a referenced-but-undefined var is NOT reported (nothing can fail).
        let mut inv = Inventory::new();
        inv.store_host("all", "h1"); // a real host exists, just not in 'ghost'
        inv.store_group("ghost");
        let inventory = Arc::new(RwLock::new(inv));
        let fx = playbook(
            "- name: site\n  groups: [ghost]\n  tasks:\n    - !echo\n      msg: \"{{ nowhere }}\"\n",
        );
        let extra = serde_yaml::Value::Mapping(serde_yaml::Mapping::new());
        let missing = missing_secret_variables(&[fx.playbook.clone()], &[], &inventory, &extra);
        assert!(missing.is_empty(), "got: {missing:?}");
    }
}
