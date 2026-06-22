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

use crate::connection::local::convert_out;
use crate::inventory::inventory::Inventory;
use crate::provisioners::ProvisionConfig;
use crate::util::io::directory_as_string;
use crate::util::io::{is_executable, jet_file_open, path_basename_as_string, path_walk};
use crate::util::yaml::show_yaml_error_in_context;
use Vec;
use serde::Deserialize;
use serde_json;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::RwLock;

// ==============================================================================================================
// YAML SPEC
// ==============================================================================================================
// for groups/<groupname> inventory files

//#[derive(Debug, PartialEq, Deserialize)]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct YamlGroup {
    hosts: Option<Vec<String>>,
    subgroups: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum DynamicInventoryJson {
    Entry(HashMap<String, DynamicInventoryJsonEntry>),
}

/* groups named _meta are not real groups */
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DynamicInventoryJsonEntry {
    hostvars: Option<serde_json::Map<String, serde_json::Value>>, /* if supplied, hosts is not supplied */
    vars: Option<serde_json::Map<String, serde_json::Value>>,
    children: Option<Vec<String>>,
    hosts: Option<Vec<String>>,
}

// ==============================================================================================================
// PUBLIC API
// ==============================================================================================================

pub fn load_inventory(
    inventory: &Arc<RwLock<Inventory>>,
    inventory_paths: Arc<RwLock<Vec<PathBuf>>>,
) -> Result<(), String> {
    {
        let mut inv_obj = inventory.write().unwrap();
        inv_obj.store_group(&String::from("all"));
    }

    for inventory_path_buf in inventory_paths.read().unwrap().iter() {
        let inventory_path = inventory_path_buf.as_path();
        if inventory_path.is_dir() {
            let groups_pathbuf = inventory_path_buf.join("groups");
            let groups_path = groups_pathbuf.as_path();

            if groups_path.exists() && groups_path.is_dir() {
                load_on_disk_inventory_tree(inventory, true, inventory_path)?;
            } else {
                return Err(format!(
                    "missing groups/ in --inventory path parameter ({})",
                    inventory_path.display()
                ));
            }
        } else {
            if is_executable(inventory_path) {
                load_dynamic_inventory(inventory, inventory_path)?;
                let dirname = directory_as_string(inventory_path);
                let dir = Path::new(&dirname);
                load_on_disk_inventory_tree(inventory, false, dir)?;
            } else {
                return Err(format!(
                    "non-directory path to --inventory ({}) is not executable",
                    inventory_path.display()
                ));
            }
        }
    }

    // Now that every --inventory path has contributed its group_vars/host_vars,
    // fold each host's (now path-complete) ancestor-group variables into its own
    // stored variables. See `propagate_group_vars_to_hosts` for why this is needed.
    propagate_group_vars_to_hosts(&inventory);

    Ok(())
}

// ==============================================================================================================
// PRIVATE INTERNALS
// ==============================================================================================================

/// After every `--inventory` path is loaded, fold each host's ancestor-group
/// variables into its own stored variables so `get_variables()` is fully
/// resolved — consistent with what templates already see via
/// `Host::get_blended_variables`.
///
/// Without this pass, group vars only reach a host's variables at the moment
/// that host's `host_vars` file is loaded (see `load_vars_directory`). When a
/// later inventory path — the canonical case is a secrets overlay — contributes
/// `group_vars` for a group but carries no `host_vars` for the member, those
/// vars land on the group object yet never propagate to the host. Templates
/// (which resolve via blended vars) are unaffected; anything reading raw host
/// variables — notably the DNS reconciliation — silently misses them, which is
/// exactly how a `dns:` block defined in a secrets overlay never fires.
///
/// The group objects already carry their correctly path-merged values (each
/// successive path's group_vars merges onto the group with later-wins, #18), so
/// re-blending the ancestor-group vars underneath the host's own vars yields the
/// complete, correctly-precedenced view: host-specific vars win over group vars
/// (host vars are layered on top last), group vars fill the gaps.
fn propagate_group_vars_to_hosts(inventory: &Arc<RwLock<Inventory>>) {
    let host_names: Vec<String> = {
        let inv = inventory.read().unwrap();
        inv.hosts.keys().cloned().collect()
    };

    for host_name in &host_names {
        let host_arc = {
            let inv = inventory.read().unwrap();
            inv.get_host(host_name)
        };
        let mut host = host_arc.write().unwrap();

        // Base layer: ancestor group vars, deep-blended. Each group object
        // already holds its path-merged values, so blending them gives the
        // complete group-var view for this host.
        let mut blended = serde_yaml::Value::Mapping(serde_yaml::Mapping::new());
        for (_name, group_arc) in host.get_ancestor_groups(20) {
            let group_vars = serde_yaml::Value::from(group_arc.read().unwrap().get_variables());
            crate::util::yaml::blend_variables(&mut blended, group_vars);
        }

        // Top layer: the host's own variables win over group vars.
        let mine = serde_yaml::Value::from(host.get_variables());
        crate::util::yaml::blend_variables(&mut blended, mine);

        if let serde_yaml::Value::Mapping(resolved) = blended {
            host.set_variables(resolved);
        }
    }
}

// loads an entire on-disk inventory tree structure (groups/, group_vars/, host_vars/)
fn load_on_disk_inventory_tree(
    inventory: &Arc<RwLock<Inventory>>,
    include_groups: bool,
    path: &Path,
) -> Result<(), String> {
    let path_buf = PathBuf::from(path);
    let group_vars_pathbuf = path_buf.join("group_vars");
    let host_vars_pathbuf = path_buf.join("host_vars");
    let groups_path = path_buf.join("groups");
    let group_vars_path = group_vars_pathbuf.as_path();
    let host_vars_path = host_vars_pathbuf.as_path();

    if include_groups {
        load_groups_directory(inventory, &groups_path)?;
    }
    if group_vars_path.exists() {
        load_vars_directory(inventory, group_vars_path, true)?;
    }
    if host_vars_path.exists() {
        load_vars_directory(inventory, host_vars_path, false)?;
    }
    Ok(())
}

// for inventory/groups/* files
fn load_groups_directory(inventory: &Arc<RwLock<Inventory>>, path: &Path) -> Result<(), String> {
    path_walk(path, |groups_file_path| {
        let mut group_name = path_basename_as_string(groups_file_path).clone();

        // skip dot files and backup files
        if group_name.ends_with("~") || group_name.starts_with(".") {
            return Ok(());
        }

        // ignore yaml extensions
        if group_name.ends_with(".yml") {
            group_name = group_name[0..group_name.len() - 4].to_string();
        }

        let groups_file = jet_file_open(groups_file_path)?;
        let groups_file_parse_result: Result<YamlGroup, serde_yaml::Error> =
            serde_yaml::from_reader(groups_file);
        let yaml_result = match groups_file_parse_result {
            Ok(y) => y,
            Err(e) => {
                show_yaml_error_in_context(&e, groups_file_path);
                return Err("edit the file and try again?".to_string());
            }
        };
        add_group_file_contents_to_inventory(inventory, group_name.clone(), &yaml_result);
        Ok(())
    })?;
    Ok(())
}

// for inventory/groups/* files
fn add_group_file_contents_to_inventory(
    inventory: &Arc<RwLock<Inventory>>,
    group_name: String,
    yaml_group: &YamlGroup,
) {
    let mut inventory = inventory.write().unwrap();
    let hosts = &yaml_group.hosts;
    if hosts.is_some() {
        let hosts = hosts.as_ref().unwrap();
        for hostname in hosts {
            inventory.store_host(&group_name.clone(), &hostname.clone());
        }
    }
    let subgroups = &yaml_group.subgroups;
    if subgroups.is_some() {
        let subgroups = subgroups.as_ref().unwrap();
        for subgroupname in subgroups {
            // FIXME: we should not panic here, but do something better
            if !group_name.eq(subgroupname) {
                inventory.store_subgroup(&group_name.clone(), &subgroupname.clone());
            }
        }
    }
}

// this is used by both on-disk and dynamic inventory sources to load group_vars/ and host_vars/ directories
fn load_vars_directory(
    inventory: &Arc<RwLock<Inventory>>,
    path: &Path,
    is_group: bool,
) -> Result<(), String> {
    let inv = inventory.write().unwrap();

    path_walk(path, |vars_path| {
        let mut effective_name = path_basename_as_string(vars_path).clone();
        // skip dot files and backup files
        if effective_name.ends_with("~") || effective_name.starts_with(".") {
            return Ok(());
        }
        // ignore yaml extensions
        if effective_name.ends_with(".yml") {
            effective_name = effective_name[0..effective_name.len() - 4].to_string();
        }

        // FIXME: warning and continue instead?
        match is_group {
            true => {
                if !inv.has_group(&effective_name.clone()) {
                    return Ok(());
                }
            }
            false => {
                if !inv.has_host(&effective_name.clone()) {
                    return Ok(());
                }
            }
        }

        let file = jet_file_open(vars_path)?;
        let file_parse_result: Result<serde_yaml::Mapping, serde_yaml::Error> =
            serde_yaml::from_reader(file);
        let yaml_result = match file_parse_result {
            Ok(y) => y,
            Err(e) => {
                show_yaml_error_in_context(&e, vars_path);
                return Err("edit the file and try again?".to_string());
            }
        };

        // serialize the vars again just to make them easier to store/output elsewhere
        // this will also remove any comments and shorten things up
        //let yaml_string = &serde_yaml::to_string(&yaml_result).unwrap();
        match is_group {
            true => {
                let group = inv.get_group(&effective_name.clone());
                // Merge onto existing vars (don't replace) so group_vars from
                // successive --inventory paths overlay earlier ones, with the later
                // path winning on key conflicts. Matches the host_vars branch below
                // and the dynamic-inventory group-vars path. (#18)
                group.write().unwrap().update_variables(yaml_result);
            }
            false => {
                let host = inv.get_host(&effective_name);

                // Start with existing host variables (from previous inventory paths)
                let mut merged_vars = {
                    let h = host.read().unwrap();
                    h.get_variables()
                };

                // Get all groups this host belongs to
                let host_groups = {
                    let h = host.read().unwrap();
                    h.get_group_names()
                };

                // Merge group_vars in order (all group first, then more specific groups)
                for group_name in &host_groups {
                    if let Some(group_arc) = inv.groups.get(group_name) {
                        let group = group_arc.read().unwrap();
                        let group_vars = group.get_variables();

                        // Merge group vars into merged_vars
                        for (k, v) in group_vars.iter() {
                            merged_vars.insert(k.clone(), v.clone());
                        }
                    }
                }

                // Check for provision block and extract it specially
                let provision_key = serde_yaml::Value::String("provision".to_string());
                if let Some(provision_value) = yaml_result.get(&provision_key) {
                    // Parse the provision block as ProvisionConfig
                    match serde_yaml::from_value::<ProvisionConfig>(provision_value.clone()) {
                        Ok(provision_config) => {
                            host.write().unwrap().set_provision(provision_config);
                        }
                        Err(e) => {
                            return Err(format!(
                                "Failed to parse provision block for host '{}': {}",
                                effective_name, e
                            ));
                        }
                    }
                }

                // Merge host-specific vars, excluding the provision block
                for (k, v) in yaml_result.iter() {
                    if k != &provision_key {
                        merged_vars.insert(k.clone(), v.clone());
                    }
                }

                // Set the merged variables on the host
                host.write().unwrap().set_variables(merged_vars);
            }
        }
        Ok(())
    })?;
    Ok(())
}

// TODO: implement
fn load_dynamic_inventory(inv: &Arc<RwLock<Inventory>>, path: &Path) -> Result<(), String> {
    let mut inventory = inv.write().unwrap();

    let mut command = Command::new(format!("{}", path.display()));
    let output = match command.output() {
        Ok(x) => match x.status.code() {
            Some(_rc) => convert_out(&x.stdout, &x.stderr),
            None => {
                return Err(format!(
                    "unable to get status code from process: {}",
                    path.display()
                ));
            }
        },
        Err(y) => {
            return Err(format!(
                "inventory script failed: {}, {}",
                path.display(),
                y
            ));
        }
    };

    let file_parse_result: Result<HashMap<String, DynamicInventoryJsonEntry>, serde_json::Error> =
        serde_json::from_str(&output);
    let json_result = match file_parse_result {
        Ok(j) => j,
        Err(e) => {
            return Err(format!(
                "error parsing dynamic inventory source: {:?}: {:?}",
                path.display(),
                e
            ));
        }
    };

    for (possible_group_name, entry) in json_result.iter() {
        let group_name = match possible_group_name.eq("_meta") {
            true => String::from("all"),
            false => possible_group_name.clone(),
        };
        if group_name.starts_with("_") {
            continue;
        }

        inventory.store_group(&group_name);
        let group = inventory.get_group(&group_name);

        if let Some(hostvars) = &entry.hostvars {
            for (host_name, values) in hostvars.iter() {
                inventory.store_host(&group_name, host_name);
                let host = inventory.get_host(host_name);
                let vars = convert_json_vars(values);
                let mut hst = host.write().unwrap();
                hst.update_variables(vars);
            }
        }
        if let Some(hosts) = &entry.hosts {
            for host_name in hosts.iter() {
                inventory.store_host(&group_name, host_name);
            }
        }
        if entry.children.as_ref().is_some() {
            let subgroups = entry.children.as_ref().unwrap();
            for subgroup_name in subgroups.iter() {
                inventory.store_subgroup(&group_name, subgroup_name);
            }
        }
        if entry.vars.as_ref().is_some() {
            let mut grp = group.write().unwrap();
            let vars = convert_json_vars(&serde_json::Value::Object(entry.vars.clone().unwrap()));
            grp.update_variables(vars);
        }
    }

    Ok(())
}

pub fn convert_json_vars(input: &serde_json::Value) -> serde_yaml::Mapping {
    let json = input.to_string();
    let parse_result: Result<serde_yaml::Mapping, serde_yaml::Error> = serde_yaml::from_str(&json);
    match parse_result {
        Ok(parsed) => parsed.clone(),
        Err(y) => panic!(
            "unable to load JSON back to YAML (1), this shouldn't happen: {}",
            y
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;
    use tempfile::TempDir;

    // Build a minimal on-disk inventory tree:
    //   <root>/groups/            (required by load_inventory; may be empty)
    //   <root>/group_vars/all.yml (the given YAML body)
    //
    // Returns the TempDir (caller must keep it alive for the duration of the load)
    // and the path to pass as a --inventory argument.
    fn inventory_tree_with_group_vars(all_vars_yaml: &str) -> (TempDir, PathBuf) {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join("groups")).unwrap();
        fs::create_dir_all(dir.path().join("group_vars")).unwrap();
        let mut file = fs::File::create(dir.path().join("group_vars").join("all.yml")).unwrap();
        file.write_all(all_vars_yaml.as_bytes()).unwrap();
        let path = dir.path().to_path_buf();
        (dir, path)
    }

    // Load two inventory trees (in order) and return the merged `all` group vars.
    fn load_two_inventories(first_yaml: &str, second_yaml: &str) -> serde_yaml::Mapping {
        let inventory = Arc::new(RwLock::new(Inventory::new()));
        let (_keep_a, path_a) = inventory_tree_with_group_vars(first_yaml);
        let (_keep_b, path_b) = inventory_tree_with_group_vars(second_yaml);
        let paths = Arc::new(RwLock::new(vec![path_a, path_b]));
        load_inventory(&inventory, paths).expect("load_inventory should succeed");
        inventory
            .read()
            .unwrap()
            .get_group("all")
            .read()
            .unwrap()
            .get_variables()
    }

    // Regression for #18: group_vars from a later --inventory path must merge onto
    // (not replace) an earlier path's group_vars for the same group, with the later
    // path winning on key conflicts.
    #[test]
    fn group_vars_merge_across_multiple_inventory_paths() {
        let vars = load_two_inventories(
            "url: https://example.com\nshared: from-first\n",
            "token: s3cr3t\nshared: from-second\n",
        );

        // Key only present in the first path must survive loading the second path.
        assert_eq!(vars["url"], "https://example.com");
        // New key contributed by the second path is present.
        assert_eq!(vars["token"], "s3cr3t");
        // On conflict, the later path wins.
        assert_eq!(vars["shared"], "from-second");
    }

    // Build an inventory tree with a named group (+ optional members), optional
    // group_vars for that group, and optional host_vars. The groups/ dir is
    // always created (load_inventory requires it); the group-members file is
    // written only when `members` is non-empty so a path can carry group_vars
    // with an empty groups/ (the secrets-overlay shape).
    fn inventory_tree_with_host(
        group: &str,
        members: &[&str],
        group_vars_yaml: Option<&str>,
        host_vars: &[(&str, &str)],
    ) -> (TempDir, PathBuf) {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        let groups_dir = root.join("groups");
        fs::create_dir_all(&groups_dir).unwrap();
        if !members.is_empty() {
            let mut body = String::from("hosts:\n");
            for m in members {
                body.push_str(&format!("  - {}\n", m));
            }
            fs::write(groups_dir.join(group), body).unwrap();
        }
        if let Some(yaml) = group_vars_yaml {
            let gv = root.join("group_vars");
            fs::create_dir_all(&gv).unwrap();
            fs::write(gv.join(group), yaml).unwrap();
        }
        if !host_vars.is_empty() {
            let hv = root.join("host_vars");
            fs::create_dir_all(&hv).unwrap();
            for (host, yaml) in host_vars {
                fs::write(hv.join(host), yaml).unwrap();
            }
        }
        let path = root.to_path_buf();
        (dir, path)
    }

    // Regression for the london k3s DNS no-op: group_vars contributed by a later
    // --inventory path (a secrets overlay) must reach a member host's resolved
    // variables — not just sit on the group object. The host's host_vars live in
    // the first (public) path; a `dns:` block lives only in the second (secrets)
    // path's group_vars (and that path has no host_vars to trigger propagation).
    // After loading both, the host's variables must contain the dns block, or the
    // DNS reconciliation (which reads host variables) silently never fires.
    #[test]
    fn group_vars_from_later_inventory_path_reach_host_variables() {
        let (_keep_pub, pub_path) = inventory_tree_with_host(
            "webservers",
            &["web1"],
            Some("public_key: from-public\n"),
            &[("web1", "host_key: from-host\n")],
        );
        let (_keep_sec, sec_path) = inventory_tree_with_host(
            "webservers",
            &[],
            Some("dns:\n  path: dns/riff.cc\n  zone: lon.riff.cc\n  source_of_truth: inventory\n"),
            &[],
        );

        let inventory = Arc::new(RwLock::new(Inventory::new()));
        let paths = Arc::new(RwLock::new(vec![pub_path, sec_path]));
        load_inventory(&inventory, paths).expect("load_inventory should succeed");

        let host_vars = inventory
            .read()
            .unwrap()
            .get_host("web1")
            .read()
            .unwrap()
            .get_variables();

        // Public group var propagated via the path-1 host_vars load.
        assert_eq!(host_vars["public_key"], "from-public");
        // Host-specific var present.
        assert_eq!(host_vars["host_key"], "from-host");
        // THE REGRESSION: the secrets-overlay group var must reach the host.
        let dns_key = serde_yaml::Value::String("dns".to_string());
        assert!(
            host_vars.get(&dns_key).is_some(),
            "dns block from secrets group_vars must propagate to host variables; got: {:?}",
            host_vars
        );
    }
}
