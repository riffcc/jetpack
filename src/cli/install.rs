// Jetporch
// Copyright (C) 2023 - Michael DeHaan <michael@michaeldehaan.net> + contributors
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <http://www.gnu.org/licenses/>.

// `jetpack install` — copy the running jetpack binary to /usr/local/bin/jetpack
// and symlink /usr/local/bin/jetp to it. Works whether you downloaded the binary
// and ran ./jetpack, or built from source and ran the artifact out of target/.

use crate::cli::parser::CliParser;
use crate::util::terminal::banner;
use std::path::{Path, PathBuf};

const INSTALL_BIN: &str = "/usr/local/bin/jetpack";
const INSTALL_SYMLINK: &str = "/usr/local/bin/jetp";

/// Mode applied to the installed binary: rwxr-xr-x.
const EXEC_MODE: u32 = 0o755;

pub fn install(_parser: &CliParser) -> i32 {
    match install_inner() {
        Ok(report) => {
            banner(&format!(
                "installed jetpack ({} → {}); jetp → {}",
                report.source.display(),
                report.target.display(),
                report.target.display(),
            ));
            0
        }
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

struct InstallReport {
    source: PathBuf,
    target: PathBuf,
}

fn install_inner() -> Result<InstallReport, String> {
    let source = std::env::current_exe()
        .map_err(|e| format!("cannot determine the running jetpack binary path: {e}"))?;
    let target = PathBuf::from(INSTALL_BIN);
    let symlink = PathBuf::from(INSTALL_SYMLINK);
    install_to(&source, &target, &symlink)
}

/// Copy `source` to `target` (chmod 0755) and create/refresh `symlink` → `target`.
/// Pure of global state so it can be tested against temp dirs.
fn install_to(source: &Path, target: &Path, symlink: &Path) -> Result<InstallReport, String> {
    // Guard against copying the binary onto itself: `fs::copy` opens the
    // destination for writing (truncating it) before reading the source, so a
    // self-copy yields a zero-byte binary. This happens when re-running
    // `install` from the already-installed location (e.g. `jetp install`).
    let same = canonicalize_or(source) == canonicalize_or(target);

    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| write_fs_error("create directory", parent, e))?;
    }

    if !same {
        std::fs::copy(source, target).map_err(|e| write_fs_error("copy binary to", target, e))?;
        set_executable(target)?;
    }

    ensure_symlink(symlink, target)?;

    Ok(InstallReport {
        source: source.to_path_buf(),
        target: target.to_path_buf(),
    })
}

fn set_executable(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(EXEC_MODE))
        .map_err(|e| write_fs_error("chmod", path, e))
}

/// Remove anything currently at `symlink` (stale link, old file) then point it
/// at `target`. A directory there is left intact and reported — we never
/// recursively remove a path we didn't create.
fn ensure_symlink(symlink: &Path, target: &Path) -> Result<(), String> {
    match std::fs::symlink_metadata(symlink) {
        Ok(_) => std::fs::remove_file(symlink)
            .map_err(|e| write_fs_error("replace existing jetp at", symlink, e))?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(write_fs_error("inspect existing jetp at", symlink, e)),
    }
    std::os::unix::fs::symlink(target, symlink)
        .map_err(|e| write_fs_error("create jetp symlink at", symlink, e))
}

fn canonicalize_or(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn write_fs_error(action: &str, path: &Path, err: std::io::Error) -> String {
    let base = format!("failed to {action} {}: {err}", path.display());
    if err.kind() == std::io::ErrorKind::PermissionDenied {
        format!("{base}\n\nPermission denied — re-run with sudo: `sudo jetpack install`")
    } else {
        base
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;
    use tempfile::TempDir;

    fn make_binary(dir: &Path, name: &str, contents: &[u8]) -> PathBuf {
        let path = dir.join(name);
        let mut file = fs::File::create(&path).unwrap();
        file.write_all(contents).unwrap();
        let mut perms = file.metadata().unwrap().permissions();
        perms.set_mode(EXEC_MODE);
        fs::set_permissions(&path, perms).unwrap();
        path
    }

    fn is_executable(path: &Path) -> bool {
        fs::metadata(path)
            .map(|m| m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }

    #[test]
    fn copies_binary_and_creates_symlink() {
        let src_dir = TempDir::new().unwrap();
        let dest_dir = TempDir::new().unwrap();
        let source = make_binary(src_dir.path(), "jetpack", b"FAKE-BINARY-1");
        let target = dest_dir.path().join("jetpack");
        let symlink = dest_dir.path().join("jetp");

        let report = install_to(&source, &target, &symlink).expect("install succeeds");

        assert_eq!(report.source, source);
        assert_eq!(report.target, target);
        assert_eq!(fs::read(&target).unwrap(), b"FAKE-BINARY-1");
        assert!(is_executable(&target));
        assert!(symlink.is_symlink());
        assert_eq!(fs::read_link(&symlink).unwrap(), target);
        assert_eq!(fs::read(&symlink).unwrap(), b"FAKE-BINARY-1");
    }

    #[test]
    fn is_idempotent_on_rerun() {
        let src_dir = TempDir::new().unwrap();
        let dest_dir = TempDir::new().unwrap();
        let source = make_binary(src_dir.path(), "jetpack", b"FAKE-BINARY-2");
        let target = dest_dir.path().join("jetpack");
        let symlink = dest_dir.path().join("jetp");

        install_to(&source, &target, &symlink).expect("first install");
        install_to(&source, &target, &symlink).expect("second install");

        assert_eq!(fs::read(&target).unwrap(), b"FAKE-BINARY-2");
        assert!(is_executable(&target));
        assert_eq!(fs::read_link(&symlink).unwrap(), target);
    }

    #[test]
    fn replaces_stale_symlink() {
        let src_dir = TempDir::new().unwrap();
        let dest_dir = TempDir::new().unwrap();
        let source = make_binary(src_dir.path(), "jetpack", b"FAKE-BINARY-3");
        let target = dest_dir.path().join("jetpack");
        let symlink = dest_dir.path().join("jetp");
        // a pre-existing symlink pointing somewhere wrong
        std::os::unix::fs::symlink(dest_dir.path().join("does-not-exist"), &symlink).unwrap();

        install_to(&source, &target, &symlink).expect("install replaces stale link");

        assert_eq!(fs::read_link(&symlink).unwrap(), target);
    }

    #[test]
    fn replaces_existing_file_at_symlink_path() {
        let src_dir = TempDir::new().unwrap();
        let dest_dir = TempDir::new().unwrap();
        let source = make_binary(src_dir.path(), "jetpack", b"FAKE-BINARY-4");
        let target = dest_dir.path().join("jetpack");
        let symlink = dest_dir.path().join("jetp");
        fs::write(&symlink, b"OLD-JETP-FILE").unwrap();

        install_to(&source, &target, &symlink).expect("install replaces old file");

        assert!(symlink.is_symlink());
        assert_eq!(fs::read(&symlink).unwrap(), b"FAKE-BINARY-4");
    }

    #[test]
    fn creates_missing_parent_directory() {
        let src_dir = TempDir::new().unwrap();
        let dest_dir = TempDir::new().unwrap();
        let source = make_binary(src_dir.path(), "jetpack", b"FAKE-BINARY-5");
        let target = dest_dir.path().join("nested/deeper/jetpack");
        let symlink = dest_dir.path().join("nested/deeper/jetp");

        install_to(&source, &target, &symlink).expect("install creates parent dirs");

        assert_eq!(fs::read(&target).unwrap(), b"FAKE-BINARY-5");
        assert_eq!(fs::read_link(&symlink).unwrap(), target);
    }

    #[test]
    fn skips_self_copy_when_source_is_target() {
        // re-running install from the installed location must NOT copy the
        // binary over itself (fs::copy would truncate it to zero bytes).
        let dest_dir = TempDir::new().unwrap();
        let path = make_binary(dest_dir.path(), "jetpack", b"REAL-CONTENTS");
        let symlink = dest_dir.path().join("jetp");

        install_to(&path, &path, &symlink).expect("self-install is a no-op copy");

        assert_eq!(fs::read(&path).unwrap(), b"REAL-CONTENTS");
        assert!(symlink.is_symlink());
    }

    #[test]
    fn permission_denied_error_includes_sudo_hint() {
        let err = std::io::Error::from(std::io::ErrorKind::PermissionDenied);
        let msg = write_fs_error("copy binary to", Path::new("/usr/local/bin/jetpack"), err);
        assert!(
            msg.contains("sudo"),
            "permission-denied error should tell the user to re-run with sudo: {msg}"
        );
    }

    #[test]
    fn non_permission_error_has_no_sudo_hint() {
        let err = std::io::Error::from(std::io::ErrorKind::NotFound);
        let msg = write_fs_error("copy binary to", Path::new("/usr/local/bin/jetpack"), err);
        assert!(
            !msg.contains("sudo"),
            "non-permission error must not hint sudo: {msg}"
        );
    }
}
