// Integration tests for the `syntax-check`, `inventory-check` and `full-check`
// CLI modes. These exercise the real `playbook_syntax_check` / `inventory_check`
// entry points against on-disk fixtures so the validation behavior (good ->
// exit 0, each failure mode -> exit 1) is proven end to end.

use jetpack::cli::parser::CliParser;
use jetpack::cli::playbooks::{inventory_check, playbook_syntax_check};
use jetpack::inventory::inventory::Inventory;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};
use tempfile::TempDir;

// playbook_traversal calls env::set_current_dir, which is process-global. This
// test binary is its own process, so the lock only needs to serialize the
// traversal tests within this file.
static CWD_LOCK: Mutex<()> = Mutex::new(());

fn write(path: &std::path::Path, contents: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, contents).unwrap();
}

// Build a CliParser wired to a playbook + roles directory (the minimal inputs
// syntax-check needs). Paths are absolute (TempDir), so canonicalization is not
// required.
fn syntax_parser(playbook: PathBuf, roles_dir: PathBuf) -> CliParser {
    let parser = CliParser::new();
    parser.playbook_paths.write().unwrap().push(playbook);
    parser.role_paths.write().unwrap().push(roles_dir);
    parser
}

fn empty_inventory() -> Arc<RwLock<Inventory>> {
    Arc::new(RwLock::new(Inventory::new()))
}

const VALID_PLAYBOOK: &str =
    "- name: smoke\n  groups:\n    - anygroup\n  roles:\n    - role: myrole\n";
const VALID_ROLE: &str = "name: myrole\ntasks:\n  - main.yml\n";
const VALID_TASKS: &str = "- !directory\n  name: make dir\n  path: /tmp/jetpack-smoke\n- !template\n  name: render\n  src: hello.hb\n  dest: /tmp/jetpack-smoke-out\n";
const VALID_TEMPLATE: &str = "Hello {{ name }}\n";

fn valid_fixture(root: &std::path::Path) -> (PathBuf, PathBuf) {
    write(&root.join("playbook.yml"), VALID_PLAYBOOK);
    write(&root.join("roles/myrole/role.yml"), VALID_ROLE);
    write(&root.join("roles/myrole/tasks/main.yml"), VALID_TASKS);
    write(
        &root.join("roles/myrole/templates/hello.hb"),
        VALID_TEMPLATE,
    );
    (root.join("playbook.yml"), root.join("roles"))
}

#[test]
fn syntax_check_valid_playbook_passes() {
    let _lock = CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = TempDir::new().unwrap();
    // playbook_traversal chdirs and, on the error path, does not restore cwd; a
    // prior test's TempDir may also have been deleted. Re-anchor to this test's
    // fixture directory so current_dir() stays valid.
    std::env::set_current_dir(tmp.path()).expect("chdir into temp fixture");
    let (playbook, roles) = valid_fixture(tmp.path());
    let parser = syntax_parser(playbook, roles);
    assert_eq!(playbook_syntax_check(&empty_inventory(), &parser), 0);
}

#[test]
fn syntax_check_missing_role_fails() {
    let _lock = CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = TempDir::new().unwrap();
    // playbook_traversal chdirs and, on the error path, does not restore cwd; a
    // prior test's TempDir may also have been deleted. Re-anchor to this test's
    // fixture directory so current_dir() stays valid.
    std::env::set_current_dir(tmp.path()).expect("chdir into temp fixture");
    write(&tmp.path().join("playbook.yml"), VALID_PLAYBOOK);
    // no roles/myrole on disk
    let parser = syntax_parser(tmp.path().join("playbook.yml"), tmp.path().join("roles"));
    assert_eq!(playbook_syntax_check(&empty_inventory(), &parser), 1);
}

#[test]
fn syntax_check_unknown_module_tag_fails() {
    let _lock = CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = TempDir::new().unwrap();
    // playbook_traversal chdirs and, on the error path, does not restore cwd; a
    // prior test's TempDir may also have been deleted. Re-anchor to this test's
    // fixture directory so current_dir() stays valid.
    std::env::set_current_dir(tmp.path()).expect("chdir into temp fixture");
    let (playbook, roles) = valid_fixture(tmp.path());
    // typo'd module tag must be rejected by task deserialization
    write(
        &tmp.path().join("roles/myrole/tasks/main.yml"),
        "- !directori\n  name: typo\n  path: /tmp/jetpack-smoke\n",
    );
    let parser = syntax_parser(playbook, roles);
    assert_eq!(playbook_syntax_check(&empty_inventory(), &parser), 1);
}

#[test]
fn syntax_check_missing_template_fails() {
    let _lock = CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = TempDir::new().unwrap();
    // playbook_traversal chdirs and, on the error path, does not restore cwd; a
    // prior test's TempDir may also have been deleted. Re-anchor to this test's
    // fixture directory so current_dir() stays valid.
    std::env::set_current_dir(tmp.path()).expect("chdir into temp fixture");
    let (playbook, roles) = valid_fixture(tmp.path());
    write(
        &tmp.path().join("roles/myrole/tasks/main.yml"),
        "- !template\n  name: render\n  src: nope.hb\n  dest: /tmp/out\n",
    );
    let parser = syntax_parser(playbook, roles);
    assert_eq!(playbook_syntax_check(&empty_inventory(), &parser), 1);
}

#[test]
fn syntax_check_malformed_template_fails() {
    let _lock = CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = TempDir::new().unwrap();
    // playbook_traversal chdirs and, on the error path, does not restore cwd; a
    // prior test's TempDir may also have been deleted. Re-anchor to this test's
    // fixture directory so current_dir() stays valid.
    std::env::set_current_dir(tmp.path()).expect("chdir into temp fixture");
    let (playbook, roles) = valid_fixture(tmp.path());
    // unbalanced handlebars expression -> compile error
    write(
        &tmp.path().join("roles/myrole/templates/hello.hb"),
        "Hello {{ name\n",
    );
    let parser = syntax_parser(playbook, roles);
    assert_eq!(playbook_syntax_check(&empty_inventory(), &parser), 1);
}

#[test]
fn syntax_check_malformed_task_yaml_fails() {
    let _lock = CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = TempDir::new().unwrap();
    // playbook_traversal chdirs and, on the error path, does not restore cwd; a
    // prior test's TempDir may also have been deleted. Re-anchor to this test's
    // fixture directory so current_dir() stays valid.
    std::env::set_current_dir(tmp.path()).expect("chdir into temp fixture");
    let (playbook, roles) = valid_fixture(tmp.path());
    write(
        &tmp.path().join("roles/myrole/tasks/main.yml"),
        "- !directory\n  name: broken\n    path: [unclosed\n",
    );
    let parser = syntax_parser(playbook, roles);
    assert_eq!(playbook_syntax_check(&empty_inventory(), &parser), 1);
}

#[test]
fn syntax_check_malformed_playbook_yaml_fails() {
    let _lock = CWD_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let tmp = TempDir::new().unwrap();
    // playbook_traversal chdirs and, on the error path, does not restore cwd; a
    // prior test's TempDir may also have been deleted. Re-anchor to this test's
    // fixture directory so current_dir() stays valid.
    std::env::set_current_dir(tmp.path()).expect("chdir into temp fixture");
    write(
        &tmp.path().join("playbook.yml"),
        "- name: broken\n   groups: [x\n",
    );
    let parser = syntax_parser(tmp.path().join("playbook.yml"), tmp.path().join("roles"));
    assert_eq!(playbook_syntax_check(&empty_inventory(), &parser), 1);
}

// --- inventory-check ---

fn inventory_parser(paths: Vec<PathBuf>) -> CliParser {
    let mut parser = CliParser::new();
    {
        let mut inv_paths = parser.inventory_paths.write().unwrap();
        for p in paths {
            inv_paths.push(p);
        }
    }
    parser.inventory_set = true;
    parser
}

#[test]
fn inventory_check_requires_inventory_flag() {
    let parser = CliParser::new(); // inventory_set == false
    assert_eq!(inventory_check(&empty_inventory(), &parser), 1);
}

#[test]
fn inventory_check_valid_passes() {
    let tmp = TempDir::new().unwrap();
    write(
        &tmp.path().join("groups/webservers"),
        "hosts:\n  - web01.example\n",
    );
    write(&tmp.path().join("group_vars/all"), "foo: bar\n");
    let parser = inventory_parser(vec![tmp.path().to_path_buf()]);
    assert_eq!(inventory_check(&empty_inventory(), &parser), 0);
}

#[test]
fn inventory_check_missing_groups_dir_fails() {
    let tmp = TempDir::new().unwrap();
    // inventory path exists but has no groups/
    fs::create_dir_all(tmp.path().join("host_vars")).unwrap();
    let parser = inventory_parser(vec![tmp.path().to_path_buf()]);
    assert_eq!(inventory_check(&empty_inventory(), &parser), 1);
}

#[test]
fn inventory_check_malformed_groups_fails() {
    let tmp = TempDir::new().unwrap();
    write(&tmp.path().join("groups/webservers"), "hosts: [unclosed\n");
    let parser = inventory_parser(vec![tmp.path().to_path_buf()]);
    assert_eq!(inventory_check(&empty_inventory(), &parser), 1);
}
