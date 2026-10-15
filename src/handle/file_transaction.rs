// Copyright (C) Riff Labs <team@riff.cc>
// SPDX-License-Identifier: GPL-3.0-or-later

//! Target-side candidate staging, validation and atomic promotion. No file data
//! or validator output is emitted through the command visitor.
use crate::handle::handle::TaskHandle;
use crate::inventory::hosts::HostOSType;
use crate::tasks::{FileAttributesEvaluated, TaskRequest, TaskResponse};
use std::path::Path;
use std::sync::Arc;

pub(crate) fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Snapshot {
    pub content: String,
    pub metadata: String,
}
impl Snapshot {
    fn fields(&self) -> Vec<&str> {
        self.metadata.split_whitespace().collect()
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Validator(Vec<String>);
impl Validator {
    pub fn parse(command: &str) -> Result<Self, String> {
        let argv = shlex::split(command).ok_or("validate: unmatched quotes")?;
        if argv.is_empty()
            || argv[0].is_empty()
            || argv[0] == "{}"
            || argv.iter().filter(|a| a.as_str() == "{}").count() != 1
            || argv.iter().any(|a| a.contains("{}") && a != "{}")
        {
            return Err(
                "validate requires exactly one standalone {} argument after the executable".into(),
            );
        }
        // Validators are argv commands, not a shell language. Shell control
        // syntax would become inert arguments, but reject it to expose mistakes.
        for argument in argv.iter().filter(|a| a.as_str() != "{}") {
            if argument.contains(['|', '&', '\n', '\r', '\0']) {
                return Err("validate contains unsupported shell syntax".into());
            }
            crate::tasks::cmd_library::screen_general_input_loose(argument)
                .map_err(|_| "validate contains unsupported shell syntax".to_string())?;
        }
        Ok(Self(argv))
    }
    fn command(&self, path: &str) -> String {
        self.0
            .iter()
            .map(|arg| quote(if arg == "{}" { path } else { arg }))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

fn fail(handle: &TaskHandle, request: &Arc<TaskRequest>, message: &str) -> Arc<TaskResponse> {
    handle.response.is_failed(request, message)
}
fn checked(
    handle: &TaskHandle,
    request: &Arc<TaskRequest>,
    command: &str,
    sudo: bool,
) -> Result<String, Arc<TaskResponse>> {
    let (rc, out) = handle.remote.file_command(request, command, sudo)?;
    if rc != 0 {
        return Err(fail(
            handle,
            request,
            "target file transaction failed (output withheld)",
        ));
    }
    Ok(out)
}

pub(crate) fn resolve_path(
    handle: &TaskHandle,
    request: &Arc<TaskRequest>,
    path: &str,
) -> Result<String, Arc<TaskResponse>> {
    if path.starts_with('/') {
        return Ok(path.into());
    }
    let directory = checked(handle, request, "pwd -P", true)?;
    let resolved = format!("{directory}/{path}");
    absolute_path(&resolved).map_err(|e| fail(handle, request, &e))?;
    Ok(resolved)
}

pub(crate) fn absolute_path(path: &str) -> Result<(), String> {
    if !path.starts_with('/') || path.contains('\0') || Path::new(path).file_name().is_none() {
        return Err("file transaction requires an absolute target file path".into());
    }
    Ok(())
}

pub(crate) fn snapshot(
    handle: &TaskHandle,
    request: &Arc<TaskRequest>,
    path: &str,
) -> Result<Option<Snapshot>, Arc<TaskResponse>> {
    absolute_path(path).map_err(|e| fail(handle, request, &e))?;
    let path = quote(path);
    let stat = match handle.remote.get_os_type() {
        HostOSType::MacOS => format!("stat -f '%Lp %u %g %i %z %m %c' {path}"),
        _ => format!("stat -c '%a %u %g %i %s %Y %Z' {path}"),
    };
    // cat followed by a sentinel retains trailing newlines trimmed by connection
    // implementations. The suffix length is known, so content cannot spoof it.
    let script = format!(
        "if test -L {path}; then exit 40; fi\nif ! test -e {path}; then exit 44; fi\ntest -f {path} || exit 41\n{stat} || exit 42\ncat {path} || exit 43\nprintf '\\nJETPACK-SNAPSHOT-END\\n'\n{stat}"
    );
    let (rc, output) = handle.remote.file_command(request, &script, true)?;
    if rc == 44 {
        return Ok(None);
    }
    if rc != 0 {
        return Err(fail(
            handle,
            request,
            "cannot read target file; expected a readable regular file without a destination symlink",
        ));
    }
    let (metadata, rest) = output
        .split_once('\n')
        .ok_or_else(|| fail(handle, request, "invalid target file metadata"))?;
    let (content, after) = rest
        .rsplit_once("\nJETPACK-SNAPSHOT-END\n")
        .ok_or_else(|| fail(handle, request, "incomplete target file read"))?;
    if metadata != after.trim_end() || metadata.split_whitespace().count() != 7 {
        return Err(fail(handle, request, "target file changed while reading"));
    }
    Ok(Some(Snapshot {
        content: content.into(),
        metadata: metadata.into(),
    }))
}

/// Compare the observed source once more immediately before rename. This is
/// optimistic detection, not compare-and-swap against uncooperative writers.
pub(crate) fn install(
    handle: &TaskHandle,
    request: &Arc<TaskRequest>,
    path: &str,
    content: &str,
    expected: &Option<Snapshot>,
    attributes: &Option<FileAttributesEvaluated>,
    validator: Option<&Validator>,
) -> Result<(), Arc<TaskResponse>> {
    absolute_path(path).map_err(|e| fail(handle, request, &e))?;
    let parent = Path::new(path).parent().unwrap().to_str().unwrap();
    let candidate = checked(
        handle,
        request,
        &format!(
            "umask 077\nmktemp {}",
            quote(&format!("{parent}/.jetpack-candidate.XXXXXXXX"))
        ),
        true,
    )?;
    // An unprivileged connection uploads into its own private directory; a
    // privileged command copies that into the same-filesystem candidate.
    let mut upload = None;
    let result = (|| {
        let directory = checked(
            handle,
            request,
            "umask 077\nmktemp -d /tmp/jetpack-upload.XXXXXXXX",
            false,
        )?;
        upload = Some(directory.clone());
        let source = format!("{directory}/data");
        handle.remote.upload_private(request, content, &source)?;
        checked(
            handle,
            request,
            &format!("cat {} > {}", quote(&source), quote(&candidate)),
            true,
        )?;
        if let Some(validator) = validator {
            let command = format!("{} >/dev/null 2>&1", validator.command(&candidate));
            let (rc, _) = handle.remote.file_command(request, &command, true)?;
            if rc != 0 {
                return Err(fail(
                    handle,
                    request,
                    &format!(
                        "candidate validation failed (exit {rc}; output withheld); destination unchanged"
                    ),
                ));
            }
        }
        // Apply metadata only to the candidate. Never expose live metadata
        // changes if validation, ownership, or the concurrency check fails.
        let old = expected.as_ref().map(Snapshot::fields);
        let attrs = attributes.as_ref();
        let mode = attrs
            .and_then(|a| a.mode.as_deref())
            .or_else(|| old.as_ref().map(|f| f[0]))
            .unwrap_or("600");
        let owner = attrs
            .and_then(|a| a.owner.as_deref())
            .or_else(|| old.as_ref().map(|f| f[1]));
        let group = attrs
            .and_then(|a| a.group.as_deref())
            .or_else(|| old.as_ref().map(|f| f[2]));
        if let Some(owner) = owner {
            checked(
                handle,
                request,
                &format!("chown {} {}", quote(owner), quote(&candidate)),
                true,
            )?;
        }
        if let Some(group) = group {
            checked(
                handle,
                request,
                &format!("chgrp {} {}", quote(group), quote(&candidate)),
                true,
            )?;
        }
        checked(
            handle,
            request,
            &format!("chmod {} {}", quote(mode), quote(&candidate)),
            true,
        )?;
        if snapshot(handle, request, path)? != *expected {
            return Err(fail(
                handle,
                request,
                "destination changed concurrently; refusing replacement",
            ));
        }
        // -f is safe after the explicit regular-file check; rename is atomic
        // because candidate and destination are on the same filesystem.
        checked(
            handle,
            request,
            &format!("mv -f {} {}", quote(&candidate), quote(path)),
            true,
        )?;
        Ok(())
    })();
    let _ = handle
        .remote
        .file_command(request, &format!("rm -f {}", quote(&candidate)), true);
    if let Some(upload) = upload {
        let _ = handle.remote.file_command(
            request,
            &format!(
                "rm -f {}\nrmdir {}",
                quote(&format!("{upload}/data")),
                quote(&upload)
            ),
            false,
        );
    }
    result
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::connection::{connection::Connection, local::LocalConnection};
    use crate::inventory::hosts::Host;
    use crate::test_support::{query_request, test_handle};
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    use std::sync::{Mutex, RwLock};

    pub(crate) fn local_handle() -> Arc<TaskHandle> {
        let host = Arc::new(RwLock::new(Host::new("localhost")));
        let conn: Arc<Mutex<dyn Connection>> = Arc::new(Mutex::new(LocalConnection::new(&host)));
        let handle = test_handle(conn);
        handle.host.write().unwrap().os_type = Some(if cfg!(target_os = "macos") {
            HostOSType::MacOS
        } else {
            HostOSType::Linux
        });
        handle
    }
    fn script(root: &Path, name: &str, body: &str) -> String {
        let path = root.join(name);
        std::fs::write(&path, format!("#!/bin/sh\nset -eu\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        path.to_str().unwrap().into()
    }
    fn no_candidates(root: &Path) {
        assert!(!std::fs::read_dir(root).unwrap().any(|e| {
            e.unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".jetpack-candidate")
        }));
    }
    #[test]
    fn validator_placeholder_is_exactly_one_argv_argument() {
        for input in [
            "",
            "tool",
            "{} tool",
            "tool {} {}",
            "tool --file={}",
            "tool '{}' trailing{}",
            "tool 'unterminated",
            "tool {} ; evil",
            "tool {} | evil",
        ] {
            assert!(Validator::parse(input).is_err(), "{input}");
        }
        let validator = Validator::parse("tool --check '{}' 'two words'").unwrap();
        let cmd = validator.command("/tmp/it's a path; $(evil)");
        assert_eq!(
            shlex::split(&cmd).unwrap(),
            vec!["tool", "--check", "/tmp/it's a path; $(evil)", "two words"]
        );
    }
    #[test]
    fn install_is_atomic_preserves_metadata_and_cleans_up_on_validation_failure() {
        let _lock = crate::test_support::ENV_LOCK.lock().unwrap();
        let root = tempfile::tempdir().unwrap();
        let handle = local_handle();
        let req = query_request();
        let path = root.path().join("live config's.xml");
        let path = path.to_str().unwrap();
        std::fs::write(path, "original\n\n").unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o640)).unwrap();
        let before = std::fs::metadata(path).unwrap();
        let old = snapshot(&handle, &req, path).unwrap();
        assert_eq!(old.as_ref().unwrap().content, "original\n\n");
        let bad = script(root.path(), "reject", "echo secret-output; exit 9");
        let validator = Validator::parse(&format!("{} {{}}", quote(&bad))).unwrap();
        let error = install(
            &handle,
            &req,
            path,
            "new secret",
            &old,
            &None,
            Some(&validator),
        )
        .unwrap_err();
        assert!(!format!("{error:?}").contains("secret-output"));
        assert_eq!(old, snapshot(&handle, &req, path).unwrap());
        assert_eq!(before.ino(), std::fs::metadata(path).unwrap().ino());
        no_candidates(root.path());
        let valid = script(
            root.path(),
            "accept",
            "test -f \"$1\"; test \"$(dirname \"$1\")\" = \"$(dirname \"$2\")\"; test \"$(cat \"$2\")\" = original",
        );
        let validator =
            Validator::parse(&format!("{} {{}} {}", quote(&valid), quote(path))).unwrap();
        install(
            &handle,
            &req,
            path,
            "valid\n",
            &old,
            &None,
            Some(&validator),
        )
        .unwrap();
        let after = std::fs::metadata(path).unwrap();
        assert_ne!(before.ino(), after.ino());
        assert_eq!(before.mode(), after.mode());
        assert_eq!(before.uid(), after.uid());
        assert_eq!(before.gid(), after.gid());
        assert_eq!(std::fs::read_to_string(path).unwrap(), "valid\n");
        no_candidates(root.path());
    }
    #[test]
    fn observed_concurrent_change_and_symlinks_refuse_replacement() {
        let _lock = crate::test_support::ENV_LOCK.lock().unwrap();
        let root = tempfile::tempdir().unwrap();
        let handle = local_handle();
        let req = query_request();
        let live = root.path().join("live");
        let path = live.to_str().unwrap();
        std::fs::write(path, "original").unwrap();
        let old = snapshot(&handle, &req, path).unwrap();
        let script = script(root.path(), "racer", "printf concurrent > \"$2\"");
        let validator =
            Validator::parse(&format!("{} {{}} {}", quote(&script), quote(path))).unwrap();
        assert!(
            install(
                &handle,
                &req,
                path,
                "candidate",
                &old,
                &None,
                Some(&validator)
            )
            .is_err()
        );
        assert_eq!(std::fs::read_to_string(path).unwrap(), "concurrent");
        no_candidates(root.path());
        let link = root.path().join("link");
        std::os::unix::fs::symlink(&live, &link).unwrap();
        assert!(snapshot(&handle, &req, link.to_str().unwrap()).is_err());
    }
}

#[cfg(test)]
mod connection_tests {
    use super::*;
    use crate::connection::connection::Connection;
    use crate::tasks::request::SudoDetails;
    use crate::test_support::{RecordingConnection, test_handle};
    use std::sync::Mutex;

    #[test]
    fn snapshot_uses_selected_target_and_task_privileges_instead_of_controller_files() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("controller-present");
        std::fs::write(&path, "controller-only").unwrap();
        let connection = RecordingConnection::with_result(|command| {
            assert!(command.starts_with("privilege-wrapper "));
            (44, String::new())
        });
        let connection: Arc<Mutex<dyn Connection>> = Arc::new(Mutex::new(connection));
        let handle = test_handle(connection);
        handle.host.write().unwrap().os_type = Some(HostOSType::Linux);
        let request = TaskRequest::query(&SudoDetails {
            user: Some("root".into()),
            template: "privilege-wrapper {{ jet_command }}".into(),
        });
        assert!(
            snapshot(&handle, &request, path.to_str().unwrap())
                .unwrap()
                .is_none()
        );
    }
}
