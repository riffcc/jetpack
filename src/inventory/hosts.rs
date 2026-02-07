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

use std::collections::HashMap;
use crate::util::yaml::blend_variables;
use crate::provisioners::ProvisionConfig;
use crate::inventory::dependencies::{DependencyReader, DependencyBuilder, VirtualizationType};
use std::sync::Arc;
use crate::inventory::groups::Group;
use std::sync::RwLock;
use std::collections::HashSet;
use serde_yaml;

#[derive(Clone,Copy,Debug,PartialEq)]
pub enum HostOSType {
    Linux,
    MacOS,
}

#[derive(Clone,Copy,Debug)]
pub enum PackagePreference {
    // other package systems are supported but no other OSes are 'fuzzy' between distro families (yet)
    // so we don't need to specify them here (yet)
    Dnf,
    Yum,
}

pub struct Host {
    pub name               : String,
    pub groups             : HashMap<String, Arc<RwLock<Group>>>,
    pub variables          : serde_yaml::Mapping,
    pub os_type            : Option<HostOSType>,
    checksum_cache         : HashMap<String,String>,
    checksum_cache_task_id : usize,
    facts                  : serde_yaml::Value,
    pub package_preference : Option<PackagePreference>,
    notified_handlers      : HashMap<usize, HashSet<String>>,
    pub provision          : Option<ProvisionConfig>,
}

impl Host {

    pub fn new(name: &String) -> Self {
        Self {
            name: name.clone(),
            variables : serde_yaml::Mapping::new(),
            groups: HashMap::new(),
            os_type: None,
            checksum_cache: HashMap::new(),
            checksum_cache_task_id: 0,
            facts: serde_yaml::Value::from(serde_yaml::Mapping::new()),
            notified_handlers: HashMap::new(),
            package_preference: None,
            provision: None,
        }
    }

    pub fn set_provision(&mut self, config: ProvisionConfig) {
        self.provision = Some(config);
    }

    pub fn get_provision(&self) -> Option<&ProvisionConfig> {
        self.provision.as_ref()
    }

    pub fn needs_provisioning(&self) -> bool {
        self.provision.is_some()
    }

    pub fn notify(&mut self, play_number: usize, signal: &String) {
        if ! self.notified_handlers.contains_key(&play_number) {
            self.notified_handlers.insert(play_number, HashSet::new());
        }
        let entry = self.notified_handlers.get_mut(&play_number).unwrap();
        entry.insert(signal.clone());
    }

    pub fn is_notified(&self, play_number: usize, signal: &String) -> bool {
        let entry = self.notified_handlers.get(&play_number);
        if entry.is_none() {
            return false;
        } else {
            return entry.unwrap().contains(&signal.clone());
        }
    }

    pub fn set_checksum_cache(&mut self, path: &String, checksum: &String) {
        self.checksum_cache.insert(path.clone(), checksum.clone());
    }

    pub fn get_checksum_cache(&mut self, task_id: usize, path: &String) -> Option<String> {
        if task_id > self.checksum_cache_task_id {
            self.checksum_cache_task_id = task_id;
            self.checksum_cache.clear();
        }
        if self.checksum_cache.contains_key(path) {
            let result = self.checksum_cache.get(path).unwrap();
            return Some(result.clone());
        }
        else {
            return None;
        }
    }

    // used by connection class on initial connect
    pub fn set_os_info(&mut self, uname_output: &String) -> Result<(),String> {
        if uname_output.starts_with("Linux")   { self.os_type = Some(HostOSType::Linux)   }
        else if uname_output.starts_with("Darwin")  { self.os_type = Some(HostOSType::MacOS)   }
        else {
            return Err(format!("OS Type could not be detected from uname -a: {}", uname_output));
        }
        return Ok(());
    }

    // ==============================================================================================================
    // PUBLIC API - most code can use this
    // ==============================================================================================================
  
    pub fn get_groups(&self) -> HashMap<String, Arc<RwLock<Group>>> {
        let mut results : HashMap<String, Arc<RwLock<Group>>> = HashMap::new();
        for (k,v) in self.groups.iter() {
            results.insert(k.clone(), Arc::clone(&v));
        }
        return results;
    }

    pub fn has_group(&self, group_name: &String) -> bool {
        for (k,_v) in self.groups.iter() {
            if k == group_name {
                return true;
            }
        }
        return false;
    }

    // get_ancestor_groups(&self, depth_limit: usize) -> HashMap<String, Arc<RwLock<Group>>>

    pub fn has_ancestor_group(&self, group_name: &String) -> bool {
        for (k,v) in self.groups.iter() {
            if k == group_name {
                return true;
            }
            for (k2,_v2) in v.read().unwrap().get_ancestor_groups(10) {
                if k2 == group_name.clone() {
                    return true;
                }
            }
        }
        return false;
    }

    pub fn get_group_names(&self) -> Vec<String> {
        return self.get_groups().iter().map(|(k,_v)| k.clone()).collect();
    }

    pub fn add_group(&mut self, name: &String, group: Arc<RwLock<Group>>) {
        self.groups.insert(name.clone(), Arc::clone(&group));
    }

    pub fn get_ancestor_groups(&self, depth_limit: usize) -> HashMap<String, Arc<RwLock<Group>>> {

        let mut results : HashMap<String, Arc<RwLock<Group>>> = HashMap::new();
        for (k,v) in self.get_groups().into_iter() {
            results.insert(k, Arc::clone(&v));
            for (k2,v2) in v.read().expect("group read").get_ancestor_groups(depth_limit).into_iter() { 
                results.insert(k2, Arc::clone(&v2)); 
            }
        }
        return results;
    }

    pub fn get_ancestor_group_names(&self) -> Vec<String> {
        return self.get_ancestor_groups(20usize).iter().map(|(k,_v)| k.clone()).collect();
    }

    pub fn get_variables(&self) -> serde_yaml::Mapping {
        return self.variables.clone();
    }

    pub fn set_variables(&mut self, variables: serde_yaml::Mapping) {
        self.variables = variables.clone();
    }

    pub fn update_variables(&mut self, mapping: serde_yaml::Mapping) {
        for (k,v) in mapping.iter() {
            self.variables.insert(k.clone(),v.clone());
        }
    }

    pub fn get_blended_variables(&self) -> serde_yaml::Mapping {
        let mut blended : serde_yaml::Value = serde_yaml::Value::from(serde_yaml::Mapping::new());
        let ancestors = self.get_ancestor_groups(20);
        for (_k,v) in ancestors.iter() {
            let theirs : serde_yaml::Value = serde_yaml::Value::from(v.read().unwrap().get_variables());
            blend_variables(&mut blended, theirs);
        }
        let mine = serde_yaml::Value::from(self.get_variables());
        blend_variables(&mut blended, mine);
        blend_variables(&mut blended, self.facts.clone());

        // Add magic variables
        let mut result = match blended {
            serde_yaml::Value::Mapping(x) => x,
            _ => panic!("get_blended_variables produced a non-mapping (1)")
        };
        // Full inventory hostname
        result.insert(
            serde_yaml::Value::String("jet_hostname".to_string()),
            serde_yaml::Value::String(self.name.clone())
        );
        // Short hostname (first part before any dot)
        let short_name = self.name.split('.').next().unwrap_or(&self.name).to_string();
        result.insert(
            serde_yaml::Value::String("jet_hostname_short".to_string()),
            serde_yaml::Value::String(short_name)
        );
        return result;
    }

    pub fn update_facts(&mut self, mapping: &Arc<RwLock<serde_yaml::Mapping>>) {
        let map = mapping.read().unwrap().clone();
        blend_variables(&mut self.facts, serde_yaml::Value::Mapping(map));
    }

    pub fn update_facts2(&mut self, mapping: serde_yaml::Mapping) {
        blend_variables(&mut self.facts, serde_yaml::Value::Mapping(mapping));
    }

    pub fn get_variables_yaml(&self) -> Result<String, String> {
        let result = serde_yaml::to_string(&self.get_variables());
        return match result {
            Ok(x) => Ok(x),
            Err(_y) => Err(String::from("error loading variables"))
        }
    }

    pub fn get_blended_variables_yaml(&self) -> Result<String,String> {
        let result = serde_yaml::to_string(&self.get_blended_variables());
        return match result {
            Ok(x) => Ok(x),
            Err(_y) => Err(String::from("error loading blended variables"))
        }
    }

    // ==============================================================================================================
    // DEPENDENCY METADATA API
    // ==============================================================================================================

    /// Get the compute node this workload runs on
    pub fn get_runs_on(&self) -> Option<String> {
        DependencyReader::get_runs_on(&self.get_blended_variables())
    }

    /// Get the workload ID (VMID for Proxmox, pod name for k8s, etc.)
    pub fn get_workload_id(&self) -> Option<String> {
        DependencyReader::get_workload_id(&self.get_blended_variables())
    }

    /// Get the compute cluster this host belongs to
    pub fn get_compute_cluster(&self) -> Option<String> {
        DependencyReader::get_compute_cluster(&self.get_blended_variables())
    }

    /// Get the virtualization type (lxc, qemu, pod, physical)
    pub fn get_virtualization(&self) -> VirtualizationType {
        DependencyReader::get_virtualization(&self.get_blended_variables())
    }

    /// Get the list of hosts/services this depends on
    pub fn get_depends_on(&self) -> Vec<String> {
        DependencyReader::get_depends_on(&self.get_blended_variables())
    }

    /// Get the list of services this host provides
    pub fn get_provides(&self) -> Vec<String> {
        DependencyReader::get_provides(&self.get_blended_variables())
    }

    /// Check if this is a critical infrastructure component
    pub fn is_critical(&self) -> bool {
        DependencyReader::is_critical(&self.get_blended_variables())
    }

    /// Get the primary storage backend
    pub fn get_storage(&self) -> Option<String> {
        DependencyReader::get_storage(&self.get_blended_variables())
    }

    /// Set infrastructure location metadata
    pub fn set_location(&mut self, runs_on: &str, workload_id: &str, cluster: &str, vtype: VirtualizationType) {
        DependencyBuilder::set_runs_on(&mut self.variables, runs_on);
        DependencyBuilder::set_workload_id(&mut self.variables, workload_id);
        DependencyBuilder::set_compute_cluster(&mut self.variables, cluster);
        DependencyBuilder::set_virtualization(&mut self.variables, vtype);
    }

    /// Set service dependencies
    pub fn set_depends_on(&mut self, deps: &[String]) {
        DependencyBuilder::set_depends_on(&mut self.variables, deps);
    }

    /// Set provided services
    pub fn set_provides(&mut self, services: &[String]) {
        DependencyBuilder::set_provides(&mut self.variables, services);
    }

    /// Set critical infrastructure flag
    pub fn set_critical(&mut self, critical: bool) {
        DependencyBuilder::set_critical(&mut self.variables, critical);
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_group(name: &str) -> Arc<RwLock<Group>> {
        Arc::new(RwLock::new(Group::new(&name.to_string())))
    }

    #[test]
    fn test_host_new() {
        let host = Host::new(&"test-host".to_string());
        assert_eq!(host.name, "test-host");
        assert!(host.groups.is_empty());
        assert!(host.variables.is_empty());
        assert!(host.os_type.is_none());
        assert!(host.package_preference.is_none());
    }

    #[test]
    fn test_set_os_info_linux() {
        let mut host = Host::new(&"test-host".to_string());
        let result = host.set_os_info(&"Linux 5.15.0-58-generic".to_string());
        assert!(result.is_ok());
        assert_eq!(host.os_type, Some(HostOSType::Linux));
    }

    #[test]
    fn test_set_os_info_macos() {
        let mut host = Host::new(&"test-host".to_string());
        let result = host.set_os_info(&"Darwin 21.6.0".to_string());
        assert!(result.is_ok());
        assert_eq!(host.os_type, Some(HostOSType::MacOS));
    }

    #[test]
    fn test_set_os_info_unknown() {
        let mut host = Host::new(&"test-host".to_string());
        let result = host.set_os_info(&"UnknownOS 1.0".to_string());
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("OS Type could not be detected"));
    }

    #[test]
    fn test_notify_and_is_notified() {
        let mut host = Host::new(&"test-host".to_string());
        
        // Initially not notified
        assert!(!host.is_notified(1, &"handler1".to_string()));
        
        // Notify handler
        host.notify(1, &"handler1".to_string());
        assert!(host.is_notified(1, &"handler1".to_string()));
        
        // Different play number
        assert!(!host.is_notified(2, &"handler1".to_string()));
        
        // Multiple handlers in same play
        host.notify(1, &"handler2".to_string());
        assert!(host.is_notified(1, &"handler1".to_string()));
        assert!(host.is_notified(1, &"handler2".to_string()));
    }

    #[test]
    fn test_checksum_cache() {
        let mut host = Host::new(&"test-host".to_string());
        
        // Initially empty
        assert!(host.get_checksum_cache(1, &"/path/file".to_string()).is_none());
        
        // Set checksum
        host.set_checksum_cache(&"/path/file".to_string(), &"abc123".to_string());
        assert_eq!(host.get_checksum_cache(1, &"/path/file".to_string()), Some("abc123".to_string()));
        
        // Different task ID clears cache
        assert!(host.get_checksum_cache(2, &"/path/file".to_string()).is_none());
        
        // Set and get multiple checksums
        host.set_checksum_cache(&"/path/file1".to_string(), &"checksum1".to_string());
        host.set_checksum_cache(&"/path/file2".to_string(), &"checksum2".to_string());
        assert_eq!(host.get_checksum_cache(2, &"/path/file1".to_string()), Some("checksum1".to_string()));
        assert_eq!(host.get_checksum_cache(2, &"/path/file2".to_string()), Some("checksum2".to_string()));
    }

    #[test]
    fn test_add_group_and_has_group() {
        let mut host = Host::new(&"test-host".to_string());
        let group = create_test_group("web");
        
        assert!(!host.has_group(&"web".to_string()));
        
        host.add_group(&"web".to_string(), group);
        assert!(host.has_group(&"web".to_string()));
        assert!(!host.has_group(&"db".to_string()));
    }

    #[test]
    fn test_get_groups_and_group_names() {
        let mut host = Host::new(&"test-host".to_string());
        let group1 = create_test_group("web");
        let group2 = create_test_group("prod");
        
        host.add_group(&"web".to_string(), group1);
        host.add_group(&"prod".to_string(), group2);
        
        let groups = host.get_groups();
        assert_eq!(groups.len(), 2);
        assert!(groups.contains_key("web"));
        assert!(groups.contains_key("prod"));
        
        let names = host.get_group_names();
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"web".to_string()));
        assert!(names.contains(&"prod".to_string()));
    }

    #[test]
    fn test_has_ancestor_group() {
        let mut host = Host::new(&"test-host".to_string());
        let group = create_test_group("web");
        
        host.add_group(&"web".to_string(), group.clone());
        
        // Direct group membership
        assert!(host.has_ancestor_group(&"web".to_string()));
        
        // Non-existent group
        assert!(!host.has_ancestor_group(&"nonexistent".to_string()));
    }

    #[test]
    fn test_variables() {
        let mut host = Host::new(&"test-host".to_string());
        
        // Initially empty
        assert!(host.get_variables().is_empty());
        
        // Set variables
        let mut vars = serde_yaml::Mapping::new();
        vars.insert(
            serde_yaml::Value::String("key1".to_string()),
            serde_yaml::Value::String("value1".to_string())
        );
        host.set_variables(vars.clone());
        
        let retrieved = host.get_variables();
        assert_eq!(retrieved["key1"], "value1");
    }

    #[test]
    fn test_update_variables() {
        let mut host = Host::new(&"test-host".to_string());
        
        // Set initial variables
        let mut vars = serde_yaml::Mapping::new();
        vars.insert(
            serde_yaml::Value::String("key1".to_string()),
            serde_yaml::Value::String("value1".to_string())
        );
        host.set_variables(vars);
        
        // Update with new variables
        let mut update = serde_yaml::Mapping::new();
        update.insert(
            serde_yaml::Value::String("key2".to_string()),
            serde_yaml::Value::String("value2".to_string())
        );
        update.insert(
            serde_yaml::Value::String("key1".to_string()),
            serde_yaml::Value::String("updated1".to_string())
        );
        host.update_variables(update);
        
        let vars = host.get_variables();
        assert_eq!(vars["key1"], "updated1");
        assert_eq!(vars["key2"], "value2");
    }

    #[test]
    fn test_update_facts() {
        let mut host = Host::new(&"test-host".to_string());
        
        let mut facts = serde_yaml::Mapping::new();
        facts.insert(
            serde_yaml::Value::String("os_family".to_string()),
            serde_yaml::Value::String("debian".to_string())
        );
        facts.insert(
            serde_yaml::Value::String("kernel".to_string()),
            serde_yaml::Value::String("linux".to_string())
        );
        
        host.update_facts2(facts);
        
        // Facts should be included in blended variables
        let blended = host.get_blended_variables();
        assert_eq!(blended["os_family"], "debian");
        assert_eq!(blended["kernel"], "linux");
    }

    #[test]
    fn test_get_variables_yaml() {
        let mut host = Host::new(&"test-host".to_string());
        
        let mut vars = serde_yaml::Mapping::new();
        vars.insert(
            serde_yaml::Value::String("key".to_string()),
            serde_yaml::Value::String("value".to_string())
        );
        host.set_variables(vars);
        
        let yaml = host.get_variables_yaml().unwrap();
        assert!(yaml.contains("key: value"));
    }

    #[test]
    fn test_get_blended_variables_yaml() {
        let mut host = Host::new(&"test-host".to_string());
        
        let mut vars = serde_yaml::Mapping::new();
        vars.insert(
            serde_yaml::Value::String("host_var".to_string()),
            serde_yaml::Value::String("host_value".to_string())
        );
        host.set_variables(vars);
        
        let yaml = host.get_blended_variables_yaml().unwrap();
        assert!(yaml.contains("host_var: host_value"));
    }

    #[test]
    fn test_package_preference() {
        let mut host = Host::new(&"test-host".to_string());
        
        assert!(host.package_preference.is_none());
        
        host.package_preference = Some(PackagePreference::Dnf);
        assert!(matches!(host.package_preference, Some(PackagePreference::Dnf)));
        
        host.package_preference = Some(PackagePreference::Yum);
        assert!(matches!(host.package_preference, Some(PackagePreference::Yum)));
    }

    #[test]
    fn test_get_ancestor_groups_and_names() {
        let mut host = Host::new(&"test-host".to_string());
        let group1 = create_test_group("web");
        let group2 = create_test_group("prod");
        
        host.add_group(&"web".to_string(), group1);
        host.add_group(&"prod".to_string(), group2);
        
        let ancestors = host.get_ancestor_groups(10);
        assert!(ancestors.len() >= 2);
        
        let names = host.get_ancestor_group_names();
        assert!(names.contains(&"web".to_string()));
        assert!(names.contains(&"prod".to_string()));
    }

    #[test]
    fn test_blended_variables_with_groups() {
        let mut host = Host::new(&"test-host".to_string());
        
        // Create group with variables
        let group = create_test_group("web");
        {
            let mut group_mut = group.write().unwrap();
            let mut group_vars = serde_yaml::Mapping::new();
            group_vars.insert(
                serde_yaml::Value::String("port".to_string()),
                serde_yaml::Value::Number(serde_yaml::Number::from(80))
            );
            group_vars.insert(
                serde_yaml::Value::String("service".to_string()),
                serde_yaml::Value::String("nginx".to_string())
            );
            group_mut.set_variables(group_vars);
        }
        
        host.add_group(&"web".to_string(), group);
        
        // Set host variables
        let mut host_vars = serde_yaml::Mapping::new();
        host_vars.insert(
            serde_yaml::Value::String("port".to_string()),
            serde_yaml::Value::Number(serde_yaml::Number::from(8080))
        );
        host_vars.insert(
            serde_yaml::Value::String("hostname".to_string()),
            serde_yaml::Value::String("webserver1".to_string())
        );
        host.set_variables(host_vars);
        
        // Test blending - host vars should override group vars
        let blended = host.get_blended_variables();
        assert_eq!(blended["port"], 8080); // Host overrides group
        assert_eq!(blended["service"], "nginx"); // From group
        assert_eq!(blended["hostname"], "webserver1"); // From host
    }
}
