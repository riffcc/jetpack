// Copyright (C) Riff Labs <team@riff.cc>
// SPDX-License-Identifier: GPL-3.0-or-later

use std::fs;
use std::process::Command;
use tempfile::TempDir;

#[test]
fn top_level_retries_drive_the_real_task_runner() {
    for (retry, succeed_on, expected_attempts, success) in [
        (Some(2), 3, 3, true),
        (Some(1), 3, 2, false),
        (None, 2, 1, false),
        (Some(5), 1, 1, true),
    ] {
        let dir = TempDir::new().unwrap();
        let script = dir.path().join("attempt.sh");
        fs::write(&script, format!("n=0\nif test -f attempts; then n=$(cat attempts); fi\nn=$((n + 1))\nprintf '%s' \"$n\" > attempts\ntest \"$n\" -ge {succeed_on}\n")).unwrap();
        let retry = retry
            .map(|n| format!("      retry: {n}\n"))
            .unwrap_or_default();
        let book = dir.path().join("playbook.yml");
        fs::write(&book, format!("- name: Retry runner test\n  groups: [all]\n  tasks:\n    - !command\n      cmd: sh {}\n      chdir: {}\n      delay: 0\n{retry}", script.display(), dir.path().display())).unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_jetpack"))
            .args(["local", "-p"])
            .arg(&book)
            .current_dir(dir.path())
            .output()
            .unwrap();
        assert_eq!(
            output.status.success(),
            success,
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            fs::read_to_string(dir.path().join("attempts")).unwrap(),
            expected_attempts.to_string()
        );
    }
}

#[test]
fn skip_conditions_defer_retry_validation_and_plan_does_not_execute() {
    let dir = TempDir::new().unwrap();
    let book = dir.path().join("playbook.yml");
    fs::write(&book, format!("- name: Skip invalid retry\n  groups: [all]\n  tasks:\n    - !command\n      cmd: touch {}\n      retry: '{{{{ missing }}}}'\n      with:\n        condition: 'false'\n", dir.path().join("ran").display())).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_jetpack"))
        .args(["local", "-p"])
        .arg(&book)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!dir.path().join("ran").exists());
    fs::write(&book, format!("- name: Plan retries\n  groups: [all]\n  tasks:\n    - !command\n      cmd: touch {}\n      retry: 3\n      delay: 0\n", dir.path().join("ran").display())).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_jetpack"))
        .args(["check-local", "-p"])
        .arg(&book)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!dir.path().join("ran").exists());
}
