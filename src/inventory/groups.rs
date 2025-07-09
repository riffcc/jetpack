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
use std::sync::Arc;
use crate::inventory::hosts::Host;
use std::sync::RwLock;
use serde_yaml;

pub struct Group {
    pub name : String,
    pub subgroups : HashMap<String, Arc<RwLock<Self>>>,
    pub parents : HashMap<String, Arc<RwLock<Self>>>,
    pub hosts : HashMap<String, Arc<RwLock<Host>>>,
    pub variables : serde_yaml::Mapping,
}

impl Group {

    pub fn new(name: &String) -> Self {
        Self {
            name : name.clone(),
            subgroups : HashMap::new(),
            parents : HashMap::new(),
            hosts : HashMap::new(),
            variables : serde_yaml::Mapping::new(),
        }
    }

    pub fn add_subgroup(&mut self, name: &String, subgroup: Arc<RwLock<Group>>) {
        assert!(!name.eq(&self.name));
        self.subgroups.insert(
            name.clone(), 
            Arc::clone(&subgroup)
        );
    }

    pub fn add_host(&mut self, name: &String, host: Arc<RwLock<Host>>) {
        self.hosts.insert(
            name.clone(), 
            Arc::clone(&host)
        );
    }

    pub fn add_parent(&mut self, name: &String, parent: Arc<RwLock<Group>>) {
        assert!(!name.eq(&self.name));
        self.parents.insert(
            name.clone(), 
            Arc::clone(&parent)
        );
    }

    pub fn get_ancestor_groups(&self, depth_limit: usize) -> HashMap<String, Arc<RwLock<Group>>> {
        let mut results : HashMap<String, Arc<RwLock<Group>>> = HashMap::new();
        for (k,v) in self.parents.iter() {
            results.insert(k.clone(), Arc::clone(v));
            if depth_limit > 0 {
                for (k2,v2) in v.read().expect("group read").get_ancestor_groups(depth_limit-1) { 
                    results.insert(k2.clone(),Arc::clone(&v2));
                }
            }
        }
        return results
    }

    pub fn get_ancestor_group_names(&self) -> Vec<String> {
        return self.get_ancestor_groups(10usize).iter().map(|(k,_v)| k.clone()).collect();
    }

    pub fn get_descendant_groups(&self, depth_limit: usize) -> HashMap<String, Arc<RwLock<Group>>> {

        let mut results : HashMap<String, Arc<RwLock<Group>>> = HashMap::new();
        for (k,v) in self.subgroups.iter() {
            if results.contains_key(&k.clone()) {
                continue;
            }
            if depth_limit > 0 {
                for (k2,v2) in v.read().expect("group read").get_descendant_groups(depth_limit-1).iter() { 
                    results.insert(
                        k2.clone(), 
                        Arc::clone(&v2)
                    ); 
                }
            }
            results.insert(
                k.clone(), 
                Arc::clone(&v)
            );
        }
        return results
    }

    pub fn get_descendant_group_names(&self) -> Vec<String> {
        return self.get_descendant_groups(10usize).iter().map(|(k,_v)| k.clone()).collect();
    }

    pub fn get_parent_groups(&self) -> HashMap<String, Arc<RwLock<Group>>> {
        let mut results : HashMap<String, Arc<RwLock<Group>>> = HashMap::new();
        for (k,v) in self.parents.iter() {
            results.insert(
                k.clone(), 
                Arc::clone(&v)
            );
        }
        return results
    }

    pub fn get_parent_group_names(&self) -> Vec<String> {
        return self.get_parent_groups().iter().map(|(k,_v)| k.clone()).collect();
    }

    pub fn get_subgroups(&self) -> HashMap<String, Arc<RwLock<Group>>> {
        let mut results : HashMap<String, Arc<RwLock<Group>>> = HashMap::new();
        for (k,v) in self.subgroups.iter() {
            results.insert(
                k.clone(), 
                Arc::clone(&v)
            );
        }
        return results
    }

    pub fn get_subgroup_names(&self) -> Vec<String> {
        return self.get_subgroups().iter().map(|(k,_v)| k.clone()).collect();
    }

    pub fn get_direct_hosts(&self) -> HashMap<String, Arc<RwLock<Host>>> {
        let mut results : HashMap<String, Arc<RwLock<Host>>> = HashMap::new();
        for (k,v) in self.hosts.iter() {
            results.insert(
                k.clone(), 
                Arc::clone(&v)
            );
        }
        return results
    }

    pub fn get_direct_host_names(&self) -> Vec<String> {
        return self.get_direct_hosts().iter().map(|(k,_v)| k.clone()).collect();
    }

    pub fn get_descendant_hosts(&self) -> HashMap<String, Arc<RwLock<Host>>> {
        let mut results : HashMap<String, Arc<RwLock<Host>>> = HashMap::new();
        let children = self.get_direct_hosts();
        for (k,v) in children { results.insert(k.clone(), Arc::clone(&v));  }
        let groups = self.get_descendant_groups(20usize);
        for (_k,v) in groups.iter() {
            let hosts = v.read().unwrap().get_direct_hosts();
            for (k2,v2) in hosts.iter() { results.insert(k2.clone(), Arc::clone(&v2));  }
        }   
        return results
    }

    pub fn get_descendant_host_names(&self) -> Vec<String> {
        return self.get_descendant_hosts().iter().map(|(k,_v)| k.clone()).collect();
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
            let theirs : serde_yaml::Value = serde_yaml::Value::from(v.read().expect("group read").get_variables());
            blend_variables(&mut blended, theirs);
        }
        let mine = serde_yaml::Value::from(self.get_variables());
        blend_variables(&mut blended, mine);
        return match blended {
            serde_yaml::Value::Mapping(x) => x,
            _ => panic!("get_blended_variables produced a non-mapping (1)")
        }
    }

    pub fn get_variables_yaml(&self) -> Result<String,String> {
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


}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_host(name: &str) -> Arc<RwLock<Host>> {
        Arc::new(RwLock::new(Host::new(&name.to_string())))
    }

    fn create_test_group(name: &str) -> Arc<RwLock<Group>> {
        Arc::new(RwLock::new(Group::new(&name.to_string())))
    }

    #[test]
    fn test_group_new() {
        let group = Group::new(&"test-group".to_string());
        assert_eq!(group.name, "test-group");
        assert!(group.subgroups.is_empty());
        assert!(group.parents.is_empty());
        assert!(group.hosts.is_empty());
        assert!(group.variables.is_empty());
    }

    #[test]
    fn test_add_subgroup() {
        let mut group = Group::new(&"parent".to_string());
        let subgroup = create_test_group("child");
        
        group.add_subgroup(&"child".to_string(), subgroup.clone());
        
        assert_eq!(group.subgroups.len(), 1);
        assert!(group.subgroups.contains_key("child"));
    }

    #[test]
    #[should_panic]
    fn test_add_subgroup_self_reference() {
        let mut group = Group::new(&"test".to_string());
        let self_ref = create_test_group("test");
        
        // Should panic - can't add self as subgroup
        group.add_subgroup(&"test".to_string(), self_ref);
    }

    #[test]
    fn test_add_host() {
        let mut group = Group::new(&"web".to_string());
        let host = create_test_host("webserver1");
        
        group.add_host(&"webserver1".to_string(), host.clone());
        
        assert_eq!(group.hosts.len(), 1);
        assert!(group.hosts.contains_key("webserver1"));
    }

    #[test]
    fn test_add_parent() {
        let mut group = Group::new(&"child".to_string());
        let parent = create_test_group("parent");
        
        group.add_parent(&"parent".to_string(), parent.clone());
        
        assert_eq!(group.parents.len(), 1);
        assert!(group.parents.contains_key("parent"));
    }

    #[test]
    #[should_panic]
    fn test_add_parent_self_reference() {
        let mut group = Group::new(&"test".to_string());
        let self_ref = create_test_group("test");
        
        // Should panic - can't add self as parent
        group.add_parent(&"test".to_string(), self_ref);
    }

    #[test]
    fn test_get_ancestor_groups() {
        let mut child = Group::new(&"child".to_string());
        let parent = create_test_group("parent");
        let grandparent = create_test_group("grandparent");
        
        // Set up hierarchy
        child.add_parent(&"parent".to_string(), parent.clone());
        parent.write().unwrap().add_parent(&"grandparent".to_string(), grandparent.clone());
        
        let ancestors = child.get_ancestor_groups(10);
        assert!(ancestors.contains_key("parent"));
        assert!(ancestors.contains_key("grandparent"));
        
        // Test depth limit - depth 0 means only direct parents
        let limited = child.get_ancestor_groups(0);
        assert!(limited.contains_key("parent"));
        assert!(!limited.contains_key("grandparent"));
    }

    #[test]
    fn test_get_ancestor_group_names() {
        let mut child = Group::new(&"child".to_string());
        let parent = create_test_group("parent");
        
        child.add_parent(&"parent".to_string(), parent);
        
        let names = child.get_ancestor_group_names();
        assert!(names.contains(&"parent".to_string()));
    }

    #[test]
    fn test_get_descendant_groups() {
        let mut parent = Group::new(&"parent".to_string());
        let child = create_test_group("child");
        let grandchild = create_test_group("grandchild");
        
        // Set up hierarchy
        parent.add_subgroup(&"child".to_string(), child.clone());
        child.write().unwrap().add_subgroup(&"grandchild".to_string(), grandchild.clone());
        
        let descendants = parent.get_descendant_groups(10);
        assert!(descendants.contains_key("child"));
        assert!(descendants.contains_key("grandchild"));
        
        // Test depth limit - depth 0 means only direct children
        let limited = parent.get_descendant_groups(0);
        assert!(limited.contains_key("child"));
        assert!(!limited.contains_key("grandchild"));
    }

    #[test]
    fn test_get_descendant_group_names() {
        let mut parent = Group::new(&"parent".to_string());
        let child = create_test_group("child");
        
        parent.add_subgroup(&"child".to_string(), child);
        
        let names = parent.get_descendant_group_names();
        assert!(names.contains(&"child".to_string()));
    }

    #[test]
    fn test_get_parent_groups_and_names() {
        let mut child = Group::new(&"child".to_string());
        let parent1 = create_test_group("parent1");
        let parent2 = create_test_group("parent2");
        
        child.add_parent(&"parent1".to_string(), parent1);
        child.add_parent(&"parent2".to_string(), parent2);
        
        let parents = child.get_parent_groups();
        assert_eq!(parents.len(), 2);
        assert!(parents.contains_key("parent1"));
        assert!(parents.contains_key("parent2"));
        
        let names = child.get_parent_group_names();
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"parent1".to_string()));
        assert!(names.contains(&"parent2".to_string()));
    }

    #[test]
    fn test_get_subgroups_and_names() {
        let mut parent = Group::new(&"parent".to_string());
        let child1 = create_test_group("child1");
        let child2 = create_test_group("child2");
        
        parent.add_subgroup(&"child1".to_string(), child1);
        parent.add_subgroup(&"child2".to_string(), child2);
        
        let subgroups = parent.get_subgroups();
        assert_eq!(subgroups.len(), 2);
        assert!(subgroups.contains_key("child1"));
        assert!(subgroups.contains_key("child2"));
        
        let names = parent.get_subgroup_names();
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"child1".to_string()));
        assert!(names.contains(&"child2".to_string()));
    }

    #[test]
    fn test_get_direct_hosts_and_names() {
        let mut group = Group::new(&"web".to_string());
        let host1 = create_test_host("web1");
        let host2 = create_test_host("web2");
        
        group.add_host(&"web1".to_string(), host1);
        group.add_host(&"web2".to_string(), host2);
        
        let hosts = group.get_direct_hosts();
        assert_eq!(hosts.len(), 2);
        assert!(hosts.contains_key("web1"));
        assert!(hosts.contains_key("web2"));
        
        let names = group.get_direct_host_names();
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"web1".to_string()));
        assert!(names.contains(&"web2".to_string()));
    }

    #[test]
    fn test_get_descendant_hosts() {
        let mut parent = Group::new(&"parent".to_string());
        let child = create_test_group("child");
        let host1 = create_test_host("host1");
        let host2 = create_test_host("host2");
        
        // Add host to parent
        parent.add_host(&"host1".to_string(), host1);
        
        // Add host to child
        child.write().unwrap().add_host(&"host2".to_string(), host2);
        
        // Add child to parent
        parent.add_subgroup(&"child".to_string(), child);
        
        let descendants = parent.get_descendant_hosts();
        assert_eq!(descendants.len(), 2);
        assert!(descendants.contains_key("host1"));
        assert!(descendants.contains_key("host2"));
        
        let names = parent.get_descendant_host_names();
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"host1".to_string()));
        assert!(names.contains(&"host2".to_string()));
    }

    #[test]
    fn test_variables() {
        let mut group = Group::new(&"test".to_string());
        
        // Initially empty
        assert!(group.get_variables().is_empty());
        
        // Set variables
        let mut vars = serde_yaml::Mapping::new();
        vars.insert(
            serde_yaml::Value::String("key1".to_string()),
            serde_yaml::Value::String("value1".to_string())
        );
        group.set_variables(vars.clone());
        
        let retrieved = group.get_variables();
        assert_eq!(retrieved["key1"], "value1");
    }

    #[test]
    fn test_update_variables() {
        let mut group = Group::new(&"test".to_string());
        
        // Set initial variables
        let mut vars = serde_yaml::Mapping::new();
        vars.insert(
            serde_yaml::Value::String("key1".to_string()),
            serde_yaml::Value::String("value1".to_string())
        );
        group.set_variables(vars);
        
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
        group.update_variables(update);
        
        let vars = group.get_variables();
        assert_eq!(vars["key1"], "updated1");
        assert_eq!(vars["key2"], "value2");
    }

    #[test]
    fn test_get_blended_variables() {
        let mut child = Group::new(&"child".to_string());
        let parent = create_test_group("parent");
        let grandparent = create_test_group("grandparent");
        
        // Set variables at each level
        {
            let mut gp_mut = grandparent.write().unwrap();
            let mut gp_vars = serde_yaml::Mapping::new();
            gp_vars.insert(
                serde_yaml::Value::String("level".to_string()),
                serde_yaml::Value::String("grandparent".to_string())
            );
            gp_vars.insert(
                serde_yaml::Value::String("gp_only".to_string()),
                serde_yaml::Value::String("gp_value".to_string())
            );
            gp_mut.set_variables(gp_vars);
        }
        
        {
            let mut p_mut = parent.write().unwrap();
            let mut p_vars = serde_yaml::Mapping::new();
            p_vars.insert(
                serde_yaml::Value::String("level".to_string()),
                serde_yaml::Value::String("parent".to_string())
            );
            p_vars.insert(
                serde_yaml::Value::String("p_only".to_string()),
                serde_yaml::Value::String("p_value".to_string())
            );
            p_mut.set_variables(p_vars);
            p_mut.add_parent(&"grandparent".to_string(), grandparent);
        }
        
        let mut c_vars = serde_yaml::Mapping::new();
        c_vars.insert(
            serde_yaml::Value::String("level".to_string()),
            serde_yaml::Value::String("child".to_string())
        );
        c_vars.insert(
            serde_yaml::Value::String("c_only".to_string()),
            serde_yaml::Value::String("c_value".to_string())
        );
        child.set_variables(c_vars);
        child.add_parent(&"parent".to_string(), parent);
        
        // Test blending - child overrides parent overrides grandparent
        let blended = child.get_blended_variables();
        assert_eq!(blended["level"], "child");
        assert_eq!(blended["gp_only"], "gp_value");
        assert_eq!(blended["p_only"], "p_value");
        assert_eq!(blended["c_only"], "c_value");
    }

    #[test]
    fn test_get_variables_yaml() {
        let mut group = Group::new(&"test".to_string());
        
        let mut vars = serde_yaml::Mapping::new();
        vars.insert(
            serde_yaml::Value::String("key".to_string()),
            serde_yaml::Value::String("value".to_string())
        );
        group.set_variables(vars);
        
        let yaml = group.get_variables_yaml().unwrap();
        assert!(yaml.contains("key: value"));
    }

    #[test]
    fn test_get_blended_variables_yaml() {
        let mut group = Group::new(&"test".to_string());
        
        let mut vars = serde_yaml::Mapping::new();
        vars.insert(
            serde_yaml::Value::String("group_var".to_string()),
            serde_yaml::Value::String("group_value".to_string())
        );
        group.set_variables(vars);
        
        let yaml = group.get_blended_variables_yaml().unwrap();
        assert!(yaml.contains("group_var: group_value"));
    }
}




