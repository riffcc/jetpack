// Copyright (C) Riff Labs <team@riff.cc>
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use crate::registry::list::Task;

fn label(input: &str) -> String {
    serde_yaml::from_str::<Task>(input)
        .unwrap()
        .get_display_name()
}

#[test]
fn common_resources_and_operations_are_described_by_their_modules() {
    for (input, expected) in [
        ("!apt\npackage: nginx", "Install nginx"),
        ("!apt\npackage: nginx\nupgrade: true", "Upgrade nginx"),
        ("!apt\npackage: nginx\nupdate: true", "Install nginx"),
        ("!apt\npackage: nginx\nremove: true", "Remove nginx"),
        (
            "!apt\npackage: nginx\nremove: '{{ unknown }}'",
            "Manage nginx",
        ),
        (
            "!template\nsrc: unused\ndest: /etc/resolv.conf",
            "Render /etc/resolv.conf",
        ),
        (
            "!copy\nsrc: unused\ndest: /etc/app/config.yml",
            "Copy file to /etc/app/config.yml",
        ),
        ("!directory\npath: /opt/app", "Ensure directory /opt/app"),
        (
            "!file\npath: /tmp/archive.tar.gz\nremove: true",
            "Remove /tmp/archive.tar.gz",
        ),
        (
            "!sd_service\nservice: nginx\nrestart: true",
            "Restart nginx",
        ),
        (
            "!sd_service\nservice: nginx\nenabled: true\nstarted: true",
            "Enable and start nginx",
        ),
        (
            "!sd_service\nservice: nginx\nenabled: false",
            "Disable nginx",
        ),
        ("!sd_service\nservice: nginx\nstarted: false", "Stop nginx"),
    ] {
        assert_eq!(label(input), expected, "{input}");
    }
}

#[test]
fn native_resource_lists_and_item_loops_share_a_logical_label() {
    for backend in ["apt", "homebrew", "pacman", "yum", "dnf", "zypper"] {
        for parameters in [
            "package: [nginx, curl]",
            "package: '{{ item }}'\nwith:\n  items: [nginx, curl]",
        ] {
            assert_eq!(
                label(&format!("!{backend}\n{parameters}")),
                "Install nginx, curl"
            );
        }
        let update = if backend == "apt" {
            "upgrade"
        } else {
            "update"
        };
        assert_eq!(
            label(&format!(
                "!{backend}\npackage: [nginx, curl]\n{update}: true"
            )),
            "Upgrade nginx, curl"
        );
        assert_eq!(
            label(&format!(
                "!{backend}\npackage: [nginx, curl]\nremove: true\n{update}: true"
            )),
            "Remove nginx, curl"
        );
    }
}

#[test]
fn resolved_literal_variable_sets_and_combined_lists_keep_execution_order() {
    let variables = serde_yaml::from_str("packages: [nginx, curl]\nsets: [{primary: nginx, secondary: curl}, {primary: git, secondary: ripgrep}]\n").unwrap();
    let task: Task =
        serde_yaml::from_str("!apt\npackage: '{{ item }}'\nwith:\n  items: packages").unwrap();
    assert_eq!(
        task.get_display_name_with(&variables),
        "Install nginx, curl"
    );
    let task: Task = serde_yaml::from_str(
        "!apt\npackage: ['{{ item.primary }}', '{{ item.secondary }}']\nwith:\n  items: sets",
    )
    .unwrap();
    assert_eq!(
        task.get_display_name_with(&variables),
        "Install nginx, curl, git, ripgrep"
    );
    let task: Task =
        serde_yaml::from_str("!template\nsrc: nonexistent\ndest: '{{ path }}'").unwrap();
    assert_eq!(
        task.get_display_name_with(&serde_yaml::from_str("path: /etc/resolv.conf").unwrap()),
        "Render /etc/resolv.conf"
    );
}

#[test]
fn duplicate_handling_uses_batch_identity_without_coalescing_loops() {
    assert_eq!(
        label("!apt\npackage: [nginx, curl, nginx]"),
        "Install nginx, curl"
    );
    assert_eq!(
        label("!apt\npackage: '{{ item }}'\nwith:\n  items: [nginx, curl, nginx]"),
        "Install nginx, curl, nginx"
    );
    assert_eq!(
        label("!apt\npackage: ['{{ item }}', '{{ item }}']\nwith:\n  items: [nginx, curl]"),
        "Install nginx, curl"
    );
}

#[test]
fn naming_never_renders_helpers_recursive_aliases_or_unavailable_values() {
    let variables = serde_yaml::from_str(
        "alias: '{{ missing }}'\npackages: '{{ unknown }}'\nitem: stale-item\n",
    )
    .unwrap();
    for input in [
        "!apt\npackage: '{{ alias }}'",
        "!apt\npackage: '{{ item }}'",
        "!apt\npackage: '{{ (unknown_helper value) }}'",
        "!apt\npackage: '{{ broken'",
        "!apt\npackage: '{{ item }}'\nwith:\n  items: packages",
        "!apt\npackage: '{{ item }}'\nwith:\n  items: [nginx, '{{ unknown }}']",
        "!apt\npackage: []",
    ] {
        let task: Task = serde_yaml::from_str(input).unwrap();
        assert_eq!(
            task.get_display_name_with(&variables),
            "Install packages",
            "{input}"
        );
    }
    // A scalar resource input is not silently upgraded to typed list syntax.
    let task: Task = serde_yaml::from_str("!apt\npackage: '{{ packages }}'").unwrap();
    assert_eq!(
        task.get_display_name_with(&serde_yaml::from_str("packages: [nginx, curl]").unwrap()),
        "Install packages"
    );
}

#[test]
fn plain_variables_use_common_values_without_leaking_a_host_or_stale_item() {
    let a = serde_yaml::from_str("package: nginx\npath: /etc/app.conf\nitem: stale-a\n").unwrap();
    let b = serde_yaml::from_str("package: curl\npath: /etc/app.conf\nitem: stale-b\n").unwrap();
    let common = common_variables([a, b]);
    assert!(!common.contains_key(Value::String("item".into())));
    let task: Task = serde_yaml::from_str("!apt\npackage: '{{ package }}'").unwrap();
    assert_eq!(task.get_display_name_with(&common), "Install packages");
    let task: Task = serde_yaml::from_str("!template\nsrc: unused\ndest: '{{ path }}'").unwrap();
    assert_eq!(task.get_display_name_with(&common), "Render /etc/app.conf");
}

#[test]
fn commands_preserve_original_references_including_nested_command_variables() {
    let variables = serde_yaml::from_str(
        "api_token: secret-token\ncommand: \"examplectl login --token '{{ api_token }}'\"\n",
    )
    .unwrap();
    for tag in ["command", "shell"] {
        for cmd in [
            "examplectl login --token '{{ api_token }}'",
            "{{ command }}",
        ] {
            let input = serde_yaml::to_string(&serde_json::json!({"cmd":cmd})).unwrap();
            let task: Task = serde_yaml::from_str(&format!("!{tag}\n{input}")).unwrap();
            let name = task.get_display_name_with(&variables);
            assert_eq!(name, format!("Run {cmd}"));
            assert!(!name.contains("secret-token"));
        }
    }
    assert_eq!(
        label("!command\ncmd: examplectl --token literal-credential"),
        "Run examplectl --token literal-credential"
    );
}

#[test]
fn http_labels_exclude_all_sensitive_endpoint_and_payload_fields() {
    let task: Task = serde_yaml::from_str("!http\nurl: https://user:secret-password@api.example.com/private-token?key=secret-query#secret-fragment\nmethod: POST\nheaders: {Authorization: secret-header}\nbody: secret-body\n").unwrap();
    assert_eq!(task.get_display_name(), "HTTP POST api.example.com");
    for url in [
        "{{ private_endpoint }}",
        "https://{{ host }}/path?key=secret",
        "not a valid url",
    ] {
        assert_eq!(http_label(Some("POST"), url), "HTTP POST");
    }
    assert_eq!(
        http_label(Some("{{ private_method }}"), "{{ endpoint }}"),
        "HTTP request"
    );
    assert_eq!(
        label("!git\nrepo: https://user:secret@repo.example.com/repo\npath: /srv/app"),
        "Checkout /srv/app"
    );
}

#[test]
fn generated_labels_are_bounded_unicode_safe_and_collapse_multiline_commands() {
    assert_eq!(
        label("!shell\ncmd: |\n  echo one\n  echo two\n"),
        "Run echo one echo two"
    );
    let input = serde_yaml::to_string(&serde_json::json!({"cmd":"🦀".repeat(200)})).unwrap();
    let name = label(&format!("!command\n{input}"));
    assert_eq!(name.chars().count(), MAX_LABEL_CHARS);
    assert!(name.ends_with('…'));
    assert_eq!(
        label("!apt\npackage: [a, b, c, d, e, f]"),
        "Install a, b, c, d (+2 more)"
    );
    assert_eq!(
        bounded("echo\tfirst\nsecond\u{1b}third"),
        "echo first second third"
    );
}

#[test]
fn explicit_names_are_unchanged_and_parameter_changes_update_implicit_names() {
    for name in ["Purpose rather than mechanism", "", "{{ explicit_name }}"] {
        let input =
            serde_yaml::to_string(&serde_json::json!({"name":name,"package":"nginx"})).unwrap();
        assert_eq!(label(&format!("!apt\n{input}")), name);
    }
    assert_eq!(label("!apt\npackage: apache2"), "Install apache2");
    assert_eq!(label("!apt\npackage: nginx"), "Install nginx");
    assert_eq!(label("!wait_for_others\nmode: loose"), "wait_for_others");
}

#[test]
fn earlier_writers_invalidate_snapshot_values_without_predicting_their_results() {
    let initial: Mapping = serde_yaml::from_str("runtime: old\nkeep: unchanged").unwrap();
    for input in [
        "!set\nvars: {runtime: new}",
        "!command\ncmd: true\nsave: runtime",
        "!http\nurl: https://example.com\nsave: runtime",
    ] {
        let mut variables = initial.clone();
        serde_yaml::from_str::<Task>(input)
            .unwrap()
            .forget_name_variables(&mut variables);
        assert!(!variables.contains_key(Value::String("runtime".into())));
        assert_eq!(
            variables
                .get(Value::String("keep".into()))
                .unwrap()
                .as_str(),
            Some("unchanged")
        );
    }
    for input in [
        "!command\ncmd: true\nsave: '{{ destination }}'",
        "!facts {}",
        "!github_release\nrepo: owner/repo\nsave: runtime",
    ] {
        let mut variables = initial.clone();
        serde_yaml::from_str::<Task>(input)
            .unwrap()
            .forget_name_variables(&mut variables);
        assert!(variables.is_empty(), "{input}");
    }
    let mut variables = initial.clone();
    serde_yaml::from_str::<Task>("!apt\npackage: nginx")
        .unwrap()
        .forget_name_variables(&mut variables);
    assert_eq!(variables, initial);
}
