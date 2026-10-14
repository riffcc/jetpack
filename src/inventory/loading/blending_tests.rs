// Copyright (C) Riff Labs <team@riff.cc>
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use serde_yaml::{Mapping, Value};
use std::fs;
use tempfile::TempDir;

fn write(root: &Path, file: &str, body: &str) {
    let file = root.join(file);
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(file, body).unwrap();
}

fn load(paths: Vec<PathBuf>, extra: Value) -> Arc<RwLock<Inventory>> {
    let inventory = Arc::new(RwLock::new(Inventory::new()));
    load_inventory(&inventory, Arc::new(RwLock::new(paths)), extra).unwrap();
    inventory
}

#[test]
fn every_source_can_contribute_topology_and_variables_for_later_sources() {
    let first = TempDir::new().unwrap();
    let second = TempDir::new().unwrap();
    write(first.path(), "groups/service", "hosts: [web1]\n");
    write(first.path(), "group_vars/service", "priority: first\n");
    write(
        first.path(),
        "group_vars/database",
        "database_value: from-first\n",
    );
    write(first.path(), "host_vars/db1", "host_value: from-first\n");
    write(second.path(), "groups/service", "hosts: [web2]\n");
    write(second.path(), "groups/database", "hosts: [db1]\n");
    write(second.path(), "group_vars/service", "priority: second\n");
    let inventory = load(vec![first.path().into(), second.path().into()], Value::Null);
    let inv = inventory.read().unwrap();
    for name in ["web1", "web2"] {
        assert_eq!(
            Value::Mapping(inv.get_host(name).read().unwrap().get_inventory_variables())["priority"],
            "second"
        );
    }
    let db = Value::Mapping(
        inv.get_host("db1")
            .read()
            .unwrap()
            .get_inventory_variables(),
    );
    assert_eq!(db["database_value"], "from-first");
    assert_eq!(db["host_value"], "from-first");
}

#[test]
fn variables_only_source_can_precede_the_source_declaring_its_targets() {
    let variables = TempDir::new().unwrap();
    let topology = TempDir::new().unwrap();
    write(variables.path(), "group_vars/service", "value: first\n");
    write(variables.path(), "host_vars/web1", "host_value: retained\n");
    write(topology.path(), "groups/service", "hosts: [web1]\n");
    write(topology.path(), "group_vars/service", "value: later\n");
    let inventory = load(
        vec![variables.path().into(), topology.path().into()],
        Value::Null,
    );
    let host = inventory.read().unwrap().get_host("web1");
    let vars = Value::Mapping(host.read().unwrap().get_inventory_variables());
    assert_eq!(vars["value"], "later");
    assert_eq!(vars["host_value"], "retained");
}

#[cfg(unix)]
#[test]
fn dynamic_source_executes_once_and_preserves_variable_source_order() {
    use std::os::unix::fs::PermissionsExt;
    let first = TempDir::new().unwrap();
    let dynamic = TempDir::new().unwrap();
    let last = TempDir::new().unwrap();
    write(
        first.path(),
        "group_vars/service",
        "first: retained\npriority: first\n",
    );
    write(
        first.path(),
        "host_vars/web1",
        "host_first: retained\npriority: first-host\n",
    );
    let script = dynamic.path().join("inventory.sh");
    let count = dynamic.path().join("count");
    fs::write(&script, format!("#!/bin/sh\nprintf x >> '{}'\ncat <<'INVENTORY'\n{{\"service\":{{\"hosts\":[\"web1\"],\"vars\":{{\"priority\":\"dynamic-group\",\"values\":[\"dynamic\"]}}}},\"_meta\":{{\"hostvars\":{{\"web1\":{{\"dynamic\":\"retained\",\"priority\":\"dynamic-host\"}}}}}}}}\nINVENTORY\n", count.display())).unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    write(last.path(), "group_vars/service", "values: [last]\n");
    write(last.path(), "host_vars/web1", "priority: last-host\n");
    let inventory = load(
        vec![first.path().into(), script, last.path().into()],
        Value::Null,
    );
    assert_eq!(fs::read_to_string(count).unwrap(), "x");
    let host = inventory.read().unwrap().get_host("web1");
    let vars = Value::Mapping(host.read().unwrap().get_inventory_variables());
    assert_eq!(vars["first"], "retained");
    assert_eq!(vars["host_first"], "retained");
    assert_eq!(vars["dynamic"], "retained");
    assert_eq!(vars["priority"], "last-host");
    assert_eq!(
        vars["values"],
        serde_yaml::from_str::<Value>("[last]").unwrap()
    );
}

#[test]
fn inherited_lists_contribute_once_including_nested_mappings() {
    let root = TempDir::new().unwrap();
    write(root.path(), "groups/service", "hosts: [web1]\n");
    write(
        root.path(),
        "group_vars/service",
        "ports: [80, 443]\nservice_expected:\n  handlerConfigs:\n    - type: memory\n    - type: etcd\n",
    );
    let inventory = load(vec![root.path().into()], Value::Null);
    let host = inventory.read().unwrap().get_host("web1");
    let vars = Value::Mapping(host.read().unwrap().get_blended_variables());
    assert_eq!(
        vars["ports"],
        serde_yaml::from_str::<Value>("[80, 443]").unwrap()
    );
    assert_eq!(
        vars["service_expected"]["handlerConfigs"],
        serde_yaml::from_str::<Value>("[{type: memory}, {type: etcd}]").unwrap()
    );
    assert!(host.read().unwrap().get_variables().is_empty());
    for _ in 0..20 {
        assert_eq!(
            Value::Mapping(host.read().unwrap().get_blended_variables()),
            vars
        );
    }
}

#[test]
fn parent_child_and_diamond_inheritance_order_declarations_once() {
    let root = TempDir::new().unwrap();
    write(
        root.path(),
        "groups/zparent",
        "subgroups: [left, right]\nhosts: [web1]\n",
    );
    write(root.path(), "groups/left", "subgroups: [aleaf]\n");
    write(root.path(), "groups/right", "subgroups: [aleaf]\n");
    write(root.path(), "groups/aleaf", "hosts: [web1]\n");
    for (group, values) in [
        ("zparent", "[parent, parent]"),
        ("left", "[left]"),
        ("right", "[right]"),
        ("aleaf", "[leaf]"),
    ] {
        write(
            root.path(),
            &format!("group_vars/{group}"),
            &format!("contributions: {values}\nlevel: {group}\n"),
        );
    }
    write(
        root.path(),
        "host_vars/web1",
        "contributions: [host]\nlevel: host\n",
    );
    let inventory = load(vec![root.path().into()], Value::Null);
    let host = inventory.read().unwrap().get_host("web1");
    let expected: Value =
        serde_yaml::from_str("[parent, parent, left, right, leaf, host]").unwrap();
    for _ in 0..20 {
        let vars = Value::Mapping(host.read().unwrap().get_blended_variables());
        assert_eq!(vars["contributions"], expected);
        assert_eq!(vars["level"], "host");
    }
    let leaf = inventory.read().unwrap().get_group("aleaf");
    let vars = Value::Mapping(leaf.read().unwrap().get_blended_variables());
    assert_eq!(
        vars["contributions"],
        serde_yaml::from_str::<Value>("[parent, parent, left, right, leaf]").unwrap()
    );
    assert_eq!(vars["level"], "aleaf");
    assert_eq!(
        Value::Mapping(host.read().unwrap().get_variables())["contributions"],
        serde_yaml::from_str::<Value>("[host]").unwrap()
    );
}

#[test]
fn variables_only_overlay_preserves_source_and_host_precedence() {
    let main = TempDir::new().unwrap();
    let overlay = TempDir::new().unwrap();
    write(main.path(), "groups/service", "hosts: [web1]\n");
    write(
        main.path(),
        "group_vars/service",
        "contributions: [old-group]\nshared: old-group\npublic: retained\n",
    );
    write(
        main.path(),
        "host_vars/web1",
        "contributions: [old-host]\nshared: host\n",
    );
    write(
        overlay.path(),
        "group_vars/service",
        "contributions: [group-overlay, group-overlay]\nshared: group-overlay\nsecret: overlay\n",
    );
    write(
        overlay.path(),
        "host_vars/web1",
        "contributions: [host-overlay]\n",
    );
    let inventory = load(vec![main.path().into(), overlay.path().into()], Value::Null);
    let host = inventory.read().unwrap().get_host("web1");
    let vars = Value::Mapping(host.read().unwrap().get_inventory_variables());
    assert_eq!(
        vars["contributions"],
        serde_yaml::from_str::<Value>("[group-overlay, group-overlay, host-overlay]").unwrap()
    );
    assert_eq!(vars["shared"], "host");
    assert_eq!(vars["public"], "retained");
    assert_eq!(vars["secret"], "overlay");
    assert!(
        host.read()
            .unwrap()
            .get_variables()
            .get(Value::from("secret"))
            .is_none()
    );
    let declarations = host.read().unwrap().get_variables();
    let paths = vec![main.path().into(), overlay.path().into()];
    load_inventory(&inventory, Arc::new(RwLock::new(paths)), Value::Null).unwrap();
    assert_eq!(host.read().unwrap().get_variables(), declarations);
    assert_eq!(
        Value::Mapping(host.read().unwrap().get_inventory_variables()),
        vars
    );
}

#[test]
fn host_updates_and_facts_remain_separate_from_inherited_lists() {
    let root = TempDir::new().unwrap();
    write(root.path(), "groups/service", "hosts: [web1]\n");
    write(root.path(), "group_vars/service", "values: [same, same]\n");
    let inventory = load(vec![root.path().into()], Value::Null);
    let host = inventory.read().unwrap().get_host("web1");
    let mut host = host.write().unwrap();
    host.update_variables(serde_yaml::from_str::<Mapping>("values: [host]\n").unwrap());
    host.update_facts2(serde_yaml::from_str("values: [fact]\n").unwrap());
    assert_eq!(
        Value::Mapping(host.get_blended_variables())["values"],
        serde_yaml::from_str::<Value>("[same, same, host, fact]").unwrap()
    );
    assert_eq!(
        Value::Mapping(host.get_inventory_variables())["values"],
        serde_yaml::from_str::<Value>("[same, same, host]").unwrap()
    );
    assert_eq!(
        Value::Mapping(host.get_variables())["values"],
        serde_yaml::from_str::<Value>("[host]").unwrap()
    );
}

#[test]
fn extra_vars_contribute_once_to_inventory_and_template_views() {
    use crate::cli::parser::CliParser;
    use crate::handle::template::BlendTarget;
    use crate::playbooks::context::PlaybookContext;
    let _lock = crate::test_support::ENV_LOCK.lock().unwrap();
    let root = TempDir::new().unwrap();
    write(root.path(), "groups/service", "hosts: [web1]\n");
    write(
        root.path(),
        "group_vars/service",
        "values: [group]\nlevel: group\n",
    );
    write(
        root.path(),
        "host_vars/web1",
        "values: [host]\nlevel: host\n",
    );
    let extra: Value = serde_yaml::from_str("values: [extra]\nlevel: extra\n").unwrap();
    let inventory = load(vec![root.path().into()], extra.clone());
    let host = inventory.read().unwrap().get_host("web1");
    let expected: Value = serde_yaml::from_str("[group, host, extra]").unwrap();
    let vars = Value::Mapping(host.read().unwrap().get_inventory_variables());
    assert_eq!(vars["values"], expected);
    assert_eq!(vars["level"], "extra");
    let standalone = Value::Mapping(host.read().unwrap().get_blended_variables());
    assert_eq!(standalone["values"], expected);
    assert_eq!(standalone["level"], "extra");
    let mut parser = CliParser::new();
    parser.extra_vars = extra;
    let context = PlaybookContext::new(&parser);
    let vars = Value::Mapping(
        context.get_complete_blended_variables(&host, BlendTarget::NotTemplateModule),
    );
    assert_eq!(vars["values"], expected);
    assert_eq!(vars["level"], "extra");
    *context.vars_storage.write().unwrap() =
        serde_yaml::from_str("values: [play]\nlevel: play\n").unwrap();
    *context.role_vars_storage.write().unwrap() =
        serde_yaml::from_str("values: [role]\nlevel: role\n").unwrap();
    let vars = Value::Mapping(
        context.get_complete_blended_variables(&host, BlendTarget::NotTemplateModule),
    );
    assert_eq!(
        vars["values"],
        serde_yaml::from_str::<Value>("[group, host, play, role, extra]").unwrap()
    );
    assert_eq!(vars["level"], "extra");
    assert_eq!(
        Value::Mapping(host.read().unwrap().get_variables())["values"],
        serde_yaml::from_str::<Value>("[host]").unwrap()
    );
}

#[tokio::test]
async fn http_preflight_matches_inherited_lists_and_item_loops_do_not_repeat_mutations() {
    use crate::connection::connection::Connection;
    use crate::handle::handle::TaskHandle;
    use crate::playbooks::language::Play;
    use crate::playbooks::task_fsm::async_run_single_task;
    use crate::playbooks::visitor::CheckMode;
    use crate::registry::list::Task;
    use crate::tasks::TaskStatus;
    use crate::test_support::{RecordingConnection, test_handle};
    use std::sync::Mutex;
    use wiremock::matchers::method;
    use wiremock::{Mock, MockServer, ResponseTemplate};

    let root = TempDir::new().unwrap();
    write(root.path(), "groups/service", "hosts: [web1]\n");
    write(
        root.path(),
        "group_vars/service",
        "resources: [one, two]\nservice_expected:\n  handlerConfigs:\n    - type: memory\n    - type: etcd\n",
    );
    let overlay = TempDir::new().unwrap();
    write(overlay.path(), "group_vars/service", "overlay: present\n");
    let inventory = load(vec![root.path().into(), overlay.path().into()], Value::Null);
    let host = inventory.read().unwrap().get_host("web1");
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "handlerConfigs": [{"type": "memory"}, {"type": "etcd"}]
        })))
        .expect(6)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(204))
        .expect(0)
        .mount(&server)
        .await;
    let url = server.uri();
    tokio::task::spawn_blocking(move || {
        let connection: Arc<Mutex<dyn Connection>> = Arc::new(Mutex::new(RecordingConnection::new()));
        let base = test_handle(connection.clone());
        let handle = TaskHandle::new(base.run_state.clone(), connection.clone(), host.clone());
        let play: Play = serde_yaml::from_str("name: inherited HTTP convergence\ngroups: [all]\n").unwrap();
        let task: Task = serde_yaml::from_str(&format!("!http\nurl: {url}/resource\nmethod: POST\ncheck:\n  url: {url}/resource\n  matches: '(eq response.json.handlerConfigs service_expected.handlerConfigs)'\nwith:\n  items: resources\n")).unwrap();
        for mode in [CheckMode::Yes, CheckMode::No, CheckMode::No] {
            handle.run_state.visitor.write().unwrap().check_mode = mode;
            let response = async_run_single_task(&handle.run_state, &connection, &host, &play, &task).unwrap();
            assert_eq!(response.status, TaskStatus::IsMatched);
        }
    }).await.unwrap();
}

#[test]
fn provisioning_reads_overlay_dns_and_preserves_inherited_connection_settings() {
    use crate::playbooks::provision_phase::{ProvisionOutcome, provision_host_with};
    use crate::provisioners::ProvisionResult;
    use crate::test_support::{RecordingConnection, test_handle};
    use std::sync::Mutex;
    let root = TempDir::new().unwrap();
    let overlay = TempDir::new().unwrap();
    write(root.path(), "groups/service", "hosts: [web1]\n");
    write(
        root.path(),
        "group_vars/service",
        "jet_ssh_user: deploy\njet_ssh_hostname: 10.0.0.9\n",
    );
    write(
        root.path(),
        "host_vars/web1",
        "provision:\n  type: proxmox_vm\n  cluster: test\n  ip: 10.0.0.8\n  ssh_user: root\n",
    );
    write(
        overlay.path(),
        "group_vars/service",
        "dns:\n  path: dns/example.com\n  zone: example.com\n  auto_sync: false\n",
    );
    let inventory = load(vec![root.path().into(), overlay.path().into()], Value::Null);
    let host = inventory.read().unwrap().get_host("web1");
    let handle = test_handle(Arc::new(Mutex::new(RecordingConnection::new())));
    let outcome = provision_host_with(&handle.run_state, &host, |_, name, _, dns, _| {
        assert_eq!(name, "web1");
        assert_eq!(dns.unwrap().zone.as_deref(), Some("example.com"));
        Ok(ProvisionResult::Created)
    });
    assert!(matches!(outcome, ProvisionOutcome::Ready));
    let vars = Value::Mapping(host.read().unwrap().get_inventory_variables());
    assert_eq!(vars["jet_ssh_user"], "deploy");
    assert_eq!(vars["jet_ssh_hostname"], "10.0.0.9");
    let declarations = host.read().unwrap().get_variables();
    assert!(!declarations.contains_key(Value::from("dns")));
    assert!(!declarations.contains_key(Value::from("jet_ssh_user")));
    assert!(!declarations.contains_key(Value::from("jet_ssh_hostname")));
}
