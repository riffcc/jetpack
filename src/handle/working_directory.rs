// Copyright (C) Riff Labs <team@riff.cc>
// SPDX-License-Identifier: GPL-3.0-or-later

//! Target-side working directories for the shared command execution path.
//! Paths are syntax-checked, never resolved against the controller filesystem.

pub(crate) fn validate(path: &str) -> Result<(), String> {
    if !path.starts_with('/') {
        return Err("chdir must be an absolute target path; relative, empty, and tilde paths are unsupported".into());
    }
    if path.contains('\0') {
        return Err("chdir must not contain NUL bytes".into());
    }
    Ok(())
}

fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

pub(super) struct PreparedCommand {
    pub command: String,
    pub directory_failure: Option<String>,
}

pub(super) fn prepare(cmd: &str, chdir: Option<&str>) -> Result<PreparedCommand, String> {
    let Some(path) = chdir else {
        return Ok(PreparedCommand {
            command: cmd.into(),
            directory_failure: None,
        });
    };
    validate(path)?;
    // An invocation-specific marker distinguishes directory entry failure from
    // the user's command returning the same exit code. It is consumed by the
    // execution layer before module failed_when/changed_when evaluation.
    let marker = format!("jetpack-chdir-failed-{}", guid_create::GUID::rand());
    let script = format!(
        "cd -- {} 2>/dev/null || {{ printf '%s\\n' {} >&2; exit 125; }}\n{}",
        quote(path),
        quote(&marker),
        cmd
    );
    Ok(PreparedCommand {
        command: format!("sh -c {}", quote(&script)),
        directory_failure: Some(marker),
    })
}

#[cfg(test)]
mod tests;
