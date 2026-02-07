// Jetpack - Dependency Metadata Support
//
// This module defines conventions for declaring infrastructure dependencies as metadata.
// Dependencies are DATA, not logic. Modules query them, playbooks/inventory declare them.
//
// Magic variable names that Jetpack understands:
//
// Infrastructure location:
//   jet_runs_on        - Compute node name this workload runs on
//   jet_workload_id    - Workload ID (VMID for Proxmox, pod name for k8s, etc.)
//   jet_compute_cluster - Compute cluster name (Proxmox cluster, k8s cluster, etc.)
//   jet_virtualization - Type: "lxc", "qemu", "pod", "physical"
//
// Service dependencies:
//   jet_depends_on   - List of hosts/services this depends on
//   jet_provides     - List of services this host provides
//   jet_critical     - Boolean: is this a critical infrastructure component?
//
// Storage dependencies:
//   jet_storage      - Primary storage backend (e.g., "moosefs", "local", "ceph")
//   jet_storage_deps - List of storage services this depends on

use serde_yaml;

/// Well-known variable names for dependency metadata
pub mod vars {
    pub const RUNS_ON: &str = "jet_runs_on";
    pub const WORKLOAD_ID: &str = "jet_workload_id";
    pub const COMPUTE_CLUSTER: &str = "jet_compute_cluster";
    pub const VIRTUALIZATION: &str = "jet_virtualization";
    pub const DEPENDS_ON: &str = "jet_depends_on";
    pub const PROVIDES: &str = "jet_provides";
    pub const CRITICAL: &str = "jet_critical";
    pub const STORAGE: &str = "jet_storage";
    pub const STORAGE_DEPS: &str = "jet_storage_deps";
}

/// Virtualization/workload types
#[derive(Debug, Clone, PartialEq)]
pub enum VirtualizationType {
    Lxc,      // Proxmox LXC container
    Qemu,     // Proxmox/KVM virtual machine
    Pod,      // Kubernetes pod
    Physical, // Bare metal
    Unknown,
}

impl VirtualizationType {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "lxc" | "container" => VirtualizationType::Lxc,
            "qemu" | "vm" | "kvm" => VirtualizationType::Qemu,
            "pod" | "k8s" | "kubernetes" => VirtualizationType::Pod,
            "physical" | "bare" | "baremetal" => VirtualizationType::Physical,
            _ => VirtualizationType::Unknown,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            VirtualizationType::Lxc => "lxc",
            VirtualizationType::Qemu => "qemu",
            VirtualizationType::Pod => "pod",
            VirtualizationType::Physical => "physical",
            VirtualizationType::Unknown => "unknown",
        }
    }
}

/// Helper functions for extracting dependency metadata from variables
pub struct DependencyReader;

impl DependencyReader {
    /// Get the compute node this workload runs on (if any)
    pub fn get_runs_on(vars: &serde_yaml::Mapping) -> Option<String> {
        Self::get_string(vars, vars::RUNS_ON)
    }

    /// Get the workload ID (VMID for Proxmox, pod name for k8s, etc.)
    pub fn get_workload_id(vars: &serde_yaml::Mapping) -> Option<String> {
        let key = serde_yaml::Value::String(vars::WORKLOAD_ID.to_string());
        vars.get(&key).and_then(|v| match v {
            serde_yaml::Value::Number(n) => n.as_i64().map(|i| i.to_string()),
            serde_yaml::Value::String(s) => Some(s.clone()),
            _ => None,
        })
    }

    /// Get the compute cluster this host belongs to (Proxmox cluster, k8s cluster, etc.)
    pub fn get_compute_cluster(vars: &serde_yaml::Mapping) -> Option<String> {
        Self::get_string(vars, vars::COMPUTE_CLUSTER)
    }

    /// Get the virtualization type
    pub fn get_virtualization(vars: &serde_yaml::Mapping) -> VirtualizationType {
        Self::get_string(vars, vars::VIRTUALIZATION)
            .map(|s| VirtualizationType::from_str(&s))
            .unwrap_or(VirtualizationType::Unknown)
    }

    /// Get the list of hosts/services this depends on
    pub fn get_depends_on(vars: &serde_yaml::Mapping) -> Vec<String> {
        Self::get_string_list(vars, vars::DEPENDS_ON)
    }

    /// Get the list of services this host provides
    pub fn get_provides(vars: &serde_yaml::Mapping) -> Vec<String> {
        Self::get_string_list(vars, vars::PROVIDES)
    }

    /// Check if this is a critical infrastructure component
    pub fn is_critical(vars: &serde_yaml::Mapping) -> bool {
        let key = serde_yaml::Value::String(vars::CRITICAL.to_string());
        vars.get(&key).and_then(|v| match v {
            serde_yaml::Value::Bool(b) => Some(*b),
            serde_yaml::Value::String(s) => match s.to_lowercase().as_str() {
                "true" | "yes" | "1" => Some(true),
                "false" | "no" | "0" => Some(false),
                _ => None,
            },
            _ => None,
        }).unwrap_or(false)
    }

    /// Get the primary storage backend
    pub fn get_storage(vars: &serde_yaml::Mapping) -> Option<String> {
        Self::get_string(vars, vars::STORAGE)
    }

    /// Get storage dependencies
    pub fn get_storage_deps(vars: &serde_yaml::Mapping) -> Vec<String> {
        Self::get_string_list(vars, vars::STORAGE_DEPS)
    }

    // Helper: get a string value
    fn get_string(vars: &serde_yaml::Mapping, key_name: &str) -> Option<String> {
        let key = serde_yaml::Value::String(key_name.to_string());
        vars.get(&key).and_then(|v| match v {
            serde_yaml::Value::String(s) => Some(s.clone()),
            _ => None,
        })
    }

    // Helper: get a list of strings
    fn get_string_list(vars: &serde_yaml::Mapping, key_name: &str) -> Vec<String> {
        let key = serde_yaml::Value::String(key_name.to_string());
        vars.get(&key).and_then(|v| match v {
            serde_yaml::Value::Sequence(seq) => {
                Some(seq.iter().filter_map(|item| match item {
                    serde_yaml::Value::String(s) => Some(s.clone()),
                    _ => None,
                }).collect())
            },
            serde_yaml::Value::String(s) => Some(vec![s.clone()]),
            _ => None,
        }).unwrap_or_default()
    }
}

/// Helper functions for building dependency metadata
pub struct DependencyBuilder;

impl DependencyBuilder {
    /// Set the compute node this runs on
    pub fn set_runs_on(vars: &mut serde_yaml::Mapping, node: &str) {
        Self::set_string(vars, vars::RUNS_ON, node);
    }

    /// Set the workload ID (VMID for Proxmox, pod name for k8s, etc.)
    pub fn set_workload_id(vars: &mut serde_yaml::Mapping, id: &str) {
        Self::set_string(vars, vars::WORKLOAD_ID, id);
    }

    /// Set the compute cluster
    pub fn set_compute_cluster(vars: &mut serde_yaml::Mapping, cluster: &str) {
        Self::set_string(vars, vars::COMPUTE_CLUSTER, cluster);
    }

    /// Set the virtualization type
    pub fn set_virtualization(vars: &mut serde_yaml::Mapping, vtype: VirtualizationType) {
        Self::set_string(vars, vars::VIRTUALIZATION, vtype.as_str());
    }

    /// Set dependencies
    pub fn set_depends_on(vars: &mut serde_yaml::Mapping, deps: &[String]) {
        Self::set_string_list(vars, vars::DEPENDS_ON, deps);
    }

    /// Set provided services
    pub fn set_provides(vars: &mut serde_yaml::Mapping, services: &[String]) {
        Self::set_string_list(vars, vars::PROVIDES, services);
    }

    /// Set critical flag
    pub fn set_critical(vars: &mut serde_yaml::Mapping, critical: bool) {
        let key = serde_yaml::Value::String(vars::CRITICAL.to_string());
        vars.insert(key, serde_yaml::Value::Bool(critical));
    }

    /// Set storage backend
    pub fn set_storage(vars: &mut serde_yaml::Mapping, storage: &str) {
        Self::set_string(vars, vars::STORAGE, storage);
    }

    /// Set storage dependencies
    pub fn set_storage_deps(vars: &mut serde_yaml::Mapping, deps: &[String]) {
        Self::set_string_list(vars, vars::STORAGE_DEPS, deps);
    }

    // Helper: set a string value
    fn set_string(vars: &mut serde_yaml::Mapping, key_name: &str, value: &str) {
        let key = serde_yaml::Value::String(key_name.to_string());
        vars.insert(key, serde_yaml::Value::String(value.to_string()));
    }

    // Helper: set a list of strings
    fn set_string_list(vars: &mut serde_yaml::Mapping, key_name: &str, values: &[String]) {
        let key = serde_yaml::Value::String(key_name.to_string());
        let seq: Vec<serde_yaml::Value> = values.iter()
            .map(|s| serde_yaml::Value::String(s.clone()))
            .collect();
        vars.insert(key, serde_yaml::Value::Sequence(seq));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_vars() -> serde_yaml::Mapping {
        let mut vars = serde_yaml::Mapping::new();
        DependencyBuilder::set_runs_on(&mut vars, "pve01");
        DependencyBuilder::set_workload_id(&mut vars, "100");
        DependencyBuilder::set_compute_cluster(&mut vars, "homelab");
        DependencyBuilder::set_virtualization(&mut vars, VirtualizationType::Lxc);
        DependencyBuilder::set_depends_on(&mut vars, &["moosefs".to_string(), "dns".to_string()]);
        DependencyBuilder::set_provides(&mut vars, &["web".to_string()]);
        DependencyBuilder::set_critical(&mut vars, true);
        DependencyBuilder::set_storage(&mut vars, "moosefs");
        vars
    }

    #[test]
    fn test_runs_on() {
        let vars = create_test_vars();
        assert_eq!(DependencyReader::get_runs_on(&vars), Some("pve01".to_string()));
    }

    #[test]
    fn test_workload_id() {
        let vars = create_test_vars();
        assert_eq!(DependencyReader::get_workload_id(&vars), Some("100".to_string()));
    }

    #[test]
    fn test_compute_cluster() {
        let vars = create_test_vars();
        assert_eq!(DependencyReader::get_compute_cluster(&vars), Some("homelab".to_string()));
    }

    #[test]
    fn test_virtualization() {
        let vars = create_test_vars();
        assert_eq!(DependencyReader::get_virtualization(&vars), VirtualizationType::Lxc);
    }

    #[test]
    fn test_depends_on() {
        let vars = create_test_vars();
        let deps = DependencyReader::get_depends_on(&vars);
        assert_eq!(deps.len(), 2);
        assert!(deps.contains(&"moosefs".to_string()));
        assert!(deps.contains(&"dns".to_string()));
    }

    #[test]
    fn test_provides() {
        let vars = create_test_vars();
        let services = DependencyReader::get_provides(&vars);
        assert_eq!(services, vec!["web".to_string()]);
    }

    #[test]
    fn test_critical() {
        let vars = create_test_vars();
        assert!(DependencyReader::is_critical(&vars));
    }

    #[test]
    fn test_storage() {
        let vars = create_test_vars();
        assert_eq!(DependencyReader::get_storage(&vars), Some("moosefs".to_string()));
    }

    #[test]
    fn test_virtualization_from_str() {
        assert_eq!(VirtualizationType::from_str("lxc"), VirtualizationType::Lxc);
        assert_eq!(VirtualizationType::from_str("LXC"), VirtualizationType::Lxc);
        assert_eq!(VirtualizationType::from_str("container"), VirtualizationType::Lxc);
        assert_eq!(VirtualizationType::from_str("qemu"), VirtualizationType::Qemu);
        assert_eq!(VirtualizationType::from_str("vm"), VirtualizationType::Qemu);
        assert_eq!(VirtualizationType::from_str("kvm"), VirtualizationType::Qemu);
        assert_eq!(VirtualizationType::from_str("pod"), VirtualizationType::Pod);
        assert_eq!(VirtualizationType::from_str("k8s"), VirtualizationType::Pod);
        assert_eq!(VirtualizationType::from_str("kubernetes"), VirtualizationType::Pod);
        assert_eq!(VirtualizationType::from_str("physical"), VirtualizationType::Physical);
        assert_eq!(VirtualizationType::from_str("baremetal"), VirtualizationType::Physical);
        assert_eq!(VirtualizationType::from_str("unknown_type"), VirtualizationType::Unknown);
    }

    #[test]
    fn test_empty_vars() {
        let vars = serde_yaml::Mapping::new();
        assert_eq!(DependencyReader::get_runs_on(&vars), None);
        assert_eq!(DependencyReader::get_workload_id(&vars), None);
        assert_eq!(DependencyReader::get_compute_cluster(&vars), None);
        assert_eq!(DependencyReader::get_virtualization(&vars), VirtualizationType::Unknown);
        assert!(DependencyReader::get_depends_on(&vars).is_empty());
        assert!(DependencyReader::get_provides(&vars).is_empty());
        assert!(!DependencyReader::is_critical(&vars));
    }

    #[test]
    fn test_workload_id_as_number() {
        // When a number is provided for workload_id, it should be converted to string
        let mut vars = serde_yaml::Mapping::new();
        let key = serde_yaml::Value::String(vars::WORKLOAD_ID.to_string());
        vars.insert(key, serde_yaml::Value::Number(serde_yaml::Number::from(200)));
        assert_eq!(DependencyReader::get_workload_id(&vars), Some("200".to_string()));
    }

    #[test]
    fn test_single_string_as_list() {
        // When a single string is provided instead of a list, treat it as a single-item list
        let mut vars = serde_yaml::Mapping::new();
        let key = serde_yaml::Value::String(vars::DEPENDS_ON.to_string());
        vars.insert(key, serde_yaml::Value::String("moosefs".to_string()));
        assert_eq!(DependencyReader::get_depends_on(&vars), vec!["moosefs".to_string()]);
    }

    #[test]
    fn test_critical_string_values() {
        let mut vars = serde_yaml::Mapping::new();
        let key = serde_yaml::Value::String(vars::CRITICAL.to_string());

        vars.insert(key.clone(), serde_yaml::Value::String("yes".to_string()));
        assert!(DependencyReader::is_critical(&vars));

        vars.insert(key.clone(), serde_yaml::Value::String("no".to_string()));
        assert!(!DependencyReader::is_critical(&vars));

        vars.insert(key.clone(), serde_yaml::Value::String("true".to_string()));
        assert!(DependencyReader::is_critical(&vars));
    }
}
