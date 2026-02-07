// Jetpack - Self Location Detection Module
//
// Introspects the local environment to determine:
// - Virtualization type (lxc, qemu, pod, physical)
// - Workload ID (VMID for Proxmox, container ID, etc.)
// - Compute node (if detectable or provided)
// - Compute cluster (if detectable or provided)
//
// Detection methods:
// - systemd-detect-virt
// - /proc/1/cpuset (LXC VMID)
// - /sys/class/dmi/id/* (QEMU/KVM detection)
// - /proc/1/environ (container detection)
// - Proxmox API query (optional, requires credentials)

use crate::tasks::*;
use crate::handle::handle::TaskHandle;
use crate::connection::command::cmd_info;
use crate::inventory::dependencies::{DependencyBuilder, VirtualizationType};
use serde::Deserialize;
use std::sync::Arc;

const MODULE: &str = "self_locate";

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct SelfLocateTask {
    pub name: Option<String>,

    /// Variable name to save results to
    pub save: String,

    /// Optional: Proxmox API host for additional info
    pub api_host: Option<String>,

    /// Optional: Proxmox API token ID
    pub api_token_id: Option<String>,

    /// Optional: Proxmox API token secret
    pub api_token_secret: Option<String>,

    #[serde(default)]
    pub with: Option<PreLogicInput>,

    #[serde(default, rename = "and")]
    pub and: Option<PostLogicInput>,
}

struct SelfLocateAction {
    pub save: String,
    pub api_host: Option<String>,
    pub api_token_id: Option<String>,
    pub api_token_secret: Option<String>,
}

impl IsTask for SelfLocateTask {
    fn get_module(&self) -> String { String::from(MODULE) }
    fn get_name(&self) -> Option<String> { self.name.clone() }
    fn get_with(&self) -> Option<PreLogicInput> { self.with.clone() }

    fn evaluate(&self, handle: &Arc<TaskHandle>, request: &Arc<TaskRequest>, tm: TemplateMode) -> Result<EvaluatedTask, Arc<TaskResponse>> {
        Ok(EvaluatedTask {
            action: Arc::new(SelfLocateAction {
                save: handle.template.string_no_spaces(request, tm.clone(), &String::from("save"), &self.save)?,
                api_host: handle.template.string_option(request, tm.clone(), &String::from("api_host"), &self.api_host)?,
                api_token_id: handle.template.string_option(request, tm.clone(), &String::from("api_token_id"), &self.api_token_id)?,
                api_token_secret: handle.template.string_option(request, tm.clone(), &String::from("api_token_secret"), &self.api_token_secret)?,
            }),
            with: Arc::new(PreLogicInput::template(handle, request, tm.clone(), &self.with)?),
            and: Arc::new(PostLogicInput::template(handle, request, tm, &self.and)?),
        })
    }
}

impl IsAction for SelfLocateAction {
    fn dispatch(&self, handle: &Arc<TaskHandle>, request: &Arc<TaskRequest>) -> Result<Arc<TaskResponse>, Arc<TaskResponse>> {
        match request.request_type {
            TaskRequestType::Query => {
                Ok(handle.response.needs_passive(request))
            },
            TaskRequestType::Passive => {
                self.run_detection(handle, request)
            },
            _ => Err(handle.response.not_supported(request)),
        }
    }
}

impl SelfLocateAction {
    fn run_detection(&self, handle: &Arc<TaskHandle>, request: &Arc<TaskRequest>) -> Result<Arc<TaskResponse>, Arc<TaskResponse>> {
        // Build the detection script
        let detect_script = r#"
# Detect virtualization type
VIRT_TYPE="unknown"
WORKLOAD_ID=""

# Method 1: systemd-detect-virt
if command -v systemd-detect-virt >/dev/null 2>&1; then
    DETECTED=$(systemd-detect-virt 2>/dev/null || echo "none")
    case "$DETECTED" in
        lxc|lxc-libvirt) VIRT_TYPE="lxc" ;;
        qemu|kvm) VIRT_TYPE="qemu" ;;
        docker|podman|containerd) VIRT_TYPE="pod" ;;
        none) VIRT_TYPE="physical" ;;
        *) VIRT_TYPE="$DETECTED" ;;
    esac
fi

# Method 2: Check /proc/1/environ for container hints
if [ "$VIRT_TYPE" = "unknown" ] && [ -f /proc/1/environ ]; then
    if tr '\0' '\n' < /proc/1/environ 2>/dev/null | grep -q "^container=lxc"; then
        VIRT_TYPE="lxc"
    fi
fi

# Method 3: Check DMI/SMBIOS for QEMU
if [ "$VIRT_TYPE" = "unknown" ] && [ -f /sys/class/dmi/id/product_name ]; then
    PRODUCT=$(cat /sys/class/dmi/id/product_name 2>/dev/null)
    case "$PRODUCT" in
        *QEMU*|*KVM*|*Virtual*Machine*) VIRT_TYPE="qemu" ;;
    esac
fi

# Method 4: Check for Proxmox LXC specific paths
if [ -f /dev/lxc/console ] || [ -d /dev/lxc ]; then
    VIRT_TYPE="lxc"
fi

# Get workload ID based on type
case "$VIRT_TYPE" in
    lxc)
        # LXC VMID is in cpuset cgroup path: /lxc/VMID/
        if [ -f /proc/1/cpuset ]; then
            WORKLOAD_ID=$(cat /proc/1/cpuset 2>/dev/null | grep -oP '(?<=/lxc/)\d+' | head -1)
        fi
        # Alternative: cgroup v2
        if [ -z "$WORKLOAD_ID" ] && [ -f /proc/1/cgroup ]; then
            WORKLOAD_ID=$(cat /proc/1/cgroup 2>/dev/null | grep -oP '(?<=/lxc/)\d+' | head -1)
        fi
        ;;
    qemu)
        # QEMU: Check SMBIOS for VM UUID or serial
        if [ -f /sys/class/dmi/id/product_serial ]; then
            WORKLOAD_ID=$(cat /sys/class/dmi/id/product_serial 2>/dev/null)
        fi
        # Proxmox puts VMID in product_uuid sometimes
        if [ -z "$WORKLOAD_ID" ] && [ -f /sys/class/dmi/id/product_uuid ]; then
            WORKLOAD_ID=$(cat /sys/class/dmi/id/product_uuid 2>/dev/null)
        fi
        ;;
esac

# Output as simple key=value pairs for parsing
echo "VIRT_TYPE=$VIRT_TYPE"
echo "WORKLOAD_ID=$WORKLOAD_ID"
"#;

        // Run detection using shell
        let shell_cmd = format!("/bin/sh -c '{}'", detect_script.replace("'", "'\"'\"'"));
        let task_result = handle.remote.run_unsafe(request, &shell_cmd, CheckRc::Unchecked)?;
        let (_rc, output) = cmd_info(&task_result);

        // Parse output
        let mut virt_type = VirtualizationType::Unknown;
        let mut workload_id: Option<String> = None;

        for line in output.lines() {
            if line.starts_with("VIRT_TYPE=") {
                let value = &line[10..];
                virt_type = VirtualizationType::from_str(value.trim());
            } else if line.starts_with("WORKLOAD_ID=") {
                let value = &line[12..];
                let val = value.trim();
                if !val.is_empty() {
                    workload_id = Some(val.to_string());
                }
            }
        }

        // Build result mapping
        let mut result = serde_yaml::Mapping::new();

        DependencyBuilder::set_virtualization(&mut result, virt_type.clone());

        if let Some(ref id) = workload_id {
            DependencyBuilder::set_workload_id(&mut result, id);
        }

        // Add raw detection info
        result.insert(
            serde_yaml::Value::String("detected".to_string()),
            serde_yaml::Value::Bool(true),
        );

        result.insert(
            serde_yaml::Value::String("virtualization_raw".to_string()),
            serde_yaml::Value::String(virt_type.as_str().to_string()),
        );

        if let Some(ref id) = workload_id {
            result.insert(
                serde_yaml::Value::String("workload_id_raw".to_string()),
                serde_yaml::Value::String(id.clone()),
            );
        }

        // Save to host variable
        handle.host.write().unwrap().update_variables(
            serde_yaml::Mapping::from_iter([(
                serde_yaml::Value::String(self.save.clone()),
                serde_yaml::Value::Mapping(result),
            )])
        );

        Ok(handle.response.is_passive(request))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_self_locate_task_basic() {
        let task = SelfLocateTask {
            name: Some("Detect location".to_string()),
            save: "location".to_string(),
            api_host: None,
            api_token_id: None,
            api_token_secret: None,
            with: None,
            and: None,
        };

        assert_eq!(task.get_module(), "self_locate");
        assert_eq!(task.get_name(), Some("Detect location".to_string()));
        assert_eq!(task.save, "location");
    }

    #[test]
    fn test_self_locate_task_with_api() {
        let task = SelfLocateTask {
            name: None,
            save: "loc".to_string(),
            api_host: Some("https://pve.local:8006".to_string()),
            api_token_id: Some("user@pam!token".to_string()),
            api_token_secret: Some("secret".to_string()),
            with: None,
            and: None,
        };

        assert!(task.api_host.is_some());
        assert!(task.api_token_id.is_some());
        assert!(task.api_token_secret.is_some());
    }

    #[test]
    fn test_self_locate_deserialization() {
        let yaml = r#"
name: Detect my location
save: self_location
"#;

        let task: Result<SelfLocateTask, _> = serde_yaml::from_str(yaml);
        assert!(task.is_ok());

        let task = task.unwrap();
        assert_eq!(task.name, Some("Detect my location".to_string()));
        assert_eq!(task.save, "self_location");
    }

    #[test]
    fn test_self_locate_deserialization_with_api() {
        let yaml = r#"
save: location_info
api_host: "{{ proxmox_api_host }}"
api_token_id: "{{ proxmox_token_id }}"
api_token_secret: "{{ proxmox_token_secret }}"
"#;

        let task: Result<SelfLocateTask, _> = serde_yaml::from_str(yaml);
        assert!(task.is_ok());

        let task = task.unwrap();
        assert!(task.api_host.is_some());
    }

    #[test]
    fn test_self_locate_deserialization_minimal() {
        let yaml = r#"
save: loc
"#;

        let task: Result<SelfLocateTask, _> = serde_yaml::from_str(yaml);
        assert!(task.is_ok());

        let task = task.unwrap();
        assert_eq!(task.save, "loc");
        assert!(task.name.is_none());
        assert!(task.api_host.is_none());
    }
}
