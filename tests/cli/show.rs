use jetpack::Inventory;
use jetpack::cli::show::{show_inventory_group, show_inventory_host};
use std::sync::{Arc, RwLock};

#[test]
fn missing_host_and_group_report_the_requested_name() {
    let inventory = Arc::new(RwLock::new(Inventory::new()));
    assert_eq!(
        show_inventory_host(&inventory, &"missing-host".into()).unwrap_err(),
        "no such host: missing-host"
    );
    assert_eq!(
        show_inventory_group(&inventory, &"missing-group".into()).unwrap_err(),
        "no such group: missing-group"
    );
}

#[test]
fn existing_host_and_group_can_be_displayed() {
    let mut inventory = Inventory::new();
    inventory.store_host("webservers", "web1");
    let inventory = Arc::new(RwLock::new(inventory));
    show_inventory_host(&inventory, &"web1".into()).unwrap();
    show_inventory_group(&inventory, &"webservers".into()).unwrap();
}

fn cli_report(mode: &str, names: &str) -> (bool, String) {
    let root = tempfile::tempdir().unwrap();
    let inventory = root.path().join("inventory");
    std::fs::create_dir_all(inventory.join("groups")).unwrap();
    std::fs::create_dir_all(inventory.join("group_vars")).unwrap();
    std::fs::create_dir_all(inventory.join("host_vars")).unwrap();
    std::fs::write(
        inventory.join("groups/webservers.yml"),
        "hosts: [web2, web1]\n",
    )
    .unwrap();
    std::fs::write(inventory.join("groups/databases.yml"), "hosts: [db1]\n").unwrap();
    std::fs::write(
        inventory.join("group_vars/webservers.yml"),
        "service_port: 8080\n",
    )
    .unwrap();
    std::fs::write(inventory.join("host_vars/web1.yml"), "node_role: primary\n").unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_jetpack"))
        .args(["show-inventory", "-i"])
        .arg(&inventory)
        .args([mode, names])
        .current_dir(root.path())
        .output()
        .unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let text = regex::Regex::new(r"\x1b\[[0-9;]*m")
        .unwrap()
        .replace_all(&text, "")
        .into_owned();
    (output.status.success(), text)
}

#[test]
fn cli_reports_multiple_hosts_and_their_blended_variables() {
    let (success, text) = cli_report("--show-hosts", "web1:web2:db1");
    assert!(success, "{text}");
    for host in ["web1", "web2", "db1"] {
        assert!(text.contains(&format!("Host: {host}")), "{text}");
    }
    assert!(text.contains("service_port: 8080"), "{text}");
    assert!(text.contains("node_role: primary"), "{text}");
}

#[test]
fn cli_reports_multiple_groups_with_sorted_membership() {
    let (success, text) = cli_report("--show-groups", "webservers:databases");
    assert!(success, "{text}");
    assert!(text.contains("Group: webservers"), "{text}");
    assert!(text.contains("Group: databases"), "{text}");
    assert!(text.contains("web1, web2"), "{text}");
    assert!(text.contains("db1"), "{text}");
}

#[test]
fn cli_reports_missing_inventory_entries_as_errors() {
    for (mode, error) in [
        ("--show-hosts", "no such host: missing"),
        ("--show-groups", "no such group: missing"),
    ] {
        let (success, text) = cli_report(mode, "missing");
        assert!(!success, "{text}");
        assert!(text.contains(error), "{text}");
    }
}
