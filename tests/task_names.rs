// Copyright (C) Riff Labs <team@riff.cc>
// SPDX-License-Identifier: GPL-3.0-or-later

use jetpack::inventory::hosts::Host;
use jetpack::tasks::{TaskRequest, TaskResponse};
use jetpack::{Inventory, JetpackConfig, LogLevel, OutputHandler, PlaybookRunner, RecapData};
use std::sync::{Arc, Mutex, RwLock};

#[derive(Default)]
struct Labels(Mutex<Vec<String>>);
impl OutputHandler for Labels {
    fn on_playbook_start(&self, _: &str) {}
    fn on_playbook_end(&self, _: &str, _: bool) {}
    fn on_play_start(&self, _: &str, _: Vec<String>) {}
    fn on_play_end(&self, _: &str) {}
    fn on_task_start(&self, name: &str, _: usize) {
        self.0.lock().unwrap().push(name.into());
    }
    fn on_task_host_result(&self, _: &Host, _: &TaskRequest, _: &TaskResponse) {}
    fn on_task_end(&self, _: &str) {}
    fn on_handler_start(&self, _: &str) {}
    fn on_handler_end(&self, _: &str) {}
    fn on_recap(&self, _: RecapData) {}
    fn log(&self, _: LogLevel, _: &str) {}
}

#[test]
fn task_and_host_parallel_output_use_one_stable_description_per_declaration() {
    // The library currently initializes Rayon's global pool once per run.
    // Isolate the two engine modes rather than changing that unrelated API.
    let mode = std::env::var("JETPACK_NAMING_TEST_MODE").ok();
    if mode.is_none() {
        for mode in ["task", "host", "task-batched", "host-batched"] {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "task_and_host_parallel_output_use_one_stable_description_per_declaration",
                    "--nocapture",
                ])
                .env("JETPACK_NAMING_TEST_MODE", mode)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        return;
    }
    let asynchronous = mode.as_ref().unwrap().starts_with("host");
    let batched = mode.as_ref().unwrap().ends_with("batched");
    {
        let mut inventory = Inventory::new();
        for id in 0..8 {
            let name = format!("host-{id}");
            inventory.store_host("all", &name);
            inventory.store_host_variables(&name, serde_yaml::from_str(&format!("packages: [nginx, curl]\npackage: private-host-{id}\npath: /etc/resolv.conf\nruntime_package: oldpkg\napi_token: secret-token-{id}\n")).unwrap());
        }
        let config = JetpackConfig::new()
            .simulate()
            .threads(4)
            .async_mode(asynchronous)
            .playbook_content(
                "naming",
                r#"
- name: Logical descriptions
  groups: [all]
  tasks:
    - !apt
      package: [nginx, curl]
      with:
        condition: 'false'
    - !apt
      package: '{{ item }}'
      with:
        items: packages
        condition: 'false'
    - !apt
      package: '{{ package }}'
      with:
        condition: 'false'
    - !template
      src: /nonexistent/template
      dest: '{{ path }}'
      with:
        condition: 'false'
    - !command
      cmd: "examplectl login --token '{{ api_token }}'"
      with:
        condition: 'false'
    - !set
      vars:
        runtime_package: newpkg
    - !apt
      package: '{{ runtime_package }}'
      with:
        condition: 'false'
    - !command
      name: Explain the purpose
      cmd: '{{ (unknown_helper missing) }}'
      with:
        condition: 'false'
"#
                .replace(
                    "  groups: [all]",
                    if batched {
                        "  groups: [all]\n  batch_size: 2"
                    } else {
                        "  groups: [all]"
                    },
                ),
            );
        let labels = Arc::new(Labels::default());
        let result = PlaybookRunner::new(config)
            .with_inventory(Arc::new(RwLock::new(inventory)))
            .with_output_handler(labels.clone())
            .run();
        assert!(result.is_ok(), "{result:?}");
        let labels = labels.0.lock().unwrap();
        let per_declaration = if asynchronous {
            8
        } else if batched {
            4
        } else {
            1
        };
        assert_eq!(labels.len(), 8 * per_declaration);
        for (label, count) in [
            ("Install nginx, curl", 2),
            (
                "Install packages",
                if asynchronous || batched { 2 } else { 1 },
            ),
            ("Set variables", 1),
            (
                "Install newpkg",
                if asynchronous || batched { 0 } else { 1 },
            ),
            ("Render /etc/resolv.conf", 1),
            ("Run examplectl login --token '{{ api_token }}'", 1),
            ("Explain the purpose", 1),
        ] {
            assert_eq!(
                labels.iter().filter(|name| name.as_str() == label).count(),
                count * per_declaration,
                "{labels:?}"
            );
        }
        assert!(!labels.iter().any(|name| name.contains("secret-token")
            || name.contains("private-host")
            || name.contains("oldpkg")));
    }
}

#[cfg(unix)]
#[test]
fn generated_package_labels_preserve_batch_and_loop_transaction_counts() {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;
    for (parameters, transactions) in [
        ("package: [nginx, curl]", 1),
        (
            "package: '{{ item }}'\n      with:\n        items: [nginx, curl]",
            2,
        ),
        (
            "package: ['{{ item }}', '{{ item }}']\n      with:\n        items: [nginx, curl]",
            2,
        ),
    ] {
        let root = tempfile::tempdir().unwrap();
        let tools = root.path().join("bin");
        fs::create_dir(&tools).unwrap();
        for (name, source) in [
            ("dpkg", "exit 1\n"),
            (
                "dpkg-query",
                "while [ \"$#\" -gt 0 ]; do\n  if [ \"$1\" = '--' ]; then shift; break; fi\n  shift\ndone\nprintf 'dpkg-query: no packages found matching %s\\n' \"$@\"\nexit 1\n",
            ),
            (
                "apt-get",
                "if [ \"$1\" = update ]; then exit 0; fi\nprintf '%s\\n' \"$*\" >> \"$NAMING_TRANSACTIONS\"\n",
            ),
        ] {
            let path = tools.join(name);
            fs::write(&path, format!("#!/bin/sh\n{source}")).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        }
        let book = root.path().join("playbook.yml");
        fs::write(&book, format!("- name: Package naming test\n  groups: [all]\n  tasks:\n    - !apt\n      {parameters}\n")).unwrap();
        let log = root.path().join("transactions");
        let output = Command::new(env!("CARGO_BIN_EXE_jetpack"))
            .args(["local", "-p"])
            .arg(&book)
            .env("PATH", format!("{}:/usr/bin:/bin", tools.display()))
            .env("NAMING_TRANSACTIONS", &log)
            .current_dir(root.path())
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&output.stdout);
        assert!(
            output.status.success(),
            "{text}{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            text.matches("begin task: Install nginx, curl").count(),
            1,
            "{text}"
        );
        let log = fs::read_to_string(log).unwrap();
        assert_eq!(log.lines().count(), transactions, "{log}");
        assert!(log.contains("nginx") && log.contains("curl"), "{log}");
    }
}
