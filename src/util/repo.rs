// Jetpack
// Copyright (C) Riff Labs Limited <team@riff.cc>
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

//! Automation-repository root detection.
//!
//! Jetpack anchors generated artifacts (DNS zone files today; future `!kubectl`
//! manifests and `!helm` values files) to a single "automation repository" root
//! rather than the bare process working directory. [`detect_automation_root`] resolves
//! that root in priority order: a real `git` checkout first, then a marker-file
//! walk-up, then the current directory as a last resort — so the README's
//! promise that paths resolve "relative to the current repository root" is
//! actually true.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Best-effort automation-repository root, resolved in priority order:
///
/// 1. `git rev-parse --show-toplevel` — authoritative inside a real checkout.
/// 2. A marker found by walking up from `start`: `.jetpack.yml`, then `.git`,
///    then a co-located `playbooks/` + `roles/` layout.
/// 3. `start` itself (the working directory) as a final fallback.
///
/// Never fails: a missing or non-canonical `start` falls back to the process
/// current directory. Resolution uses [`Path::join`], never `canonicalize`, so
/// it works for directories that do not exist yet (e.g. a freshly-declared
/// `dns/` tree a playbook is about to populate).
pub fn detect_automation_root(start: &Path) -> PathBuf {
    if let Some(root) = git_toplevel(start) {
        return root;
    }
    if let Some(root) = find_automation_root_by_marker(start) {
        return root;
    }
    // No git checkout and no marker: anchor to the working directory. If `start`
    // itself is unusable, fall back to the process CWD so callers always receive
    // a real path.
    if start.as_os_str().is_empty() || !start.exists() {
        return std::env::current_dir().unwrap_or_else(|_| start.to_path_buf());
    }
    start.to_path_buf()
}

/// Run `git rev-parse --show-toplevel` from `start`. Returns `None` on any
/// failure (git missing, not a checkout, non-zero exit, non-UTF-8 output) so the
/// caller can fall through to marker detection.
fn git_toplevel(start: &Path) -> Option<PathBuf> {
    let output = Command::new("git")
        .arg("rev-parse")
        .arg("--show-toplevel")
        .current_dir(start)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let root = String::from_utf8(output.stdout).ok()?;
    let trimmed = root.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(PathBuf::from(trimmed))
}

/// Walk up from `start` (inclusive) looking for an automation-repository
/// marker. At each ancestor directory the markers are considered in priority
/// order: an explicit `.jetpack.yml`, then a `.git` (file or directory —
/// worktrees and submodules carry a `.git` *file*), then a co-located
/// `playbooks/` + `roles/` layout. Returns the first directory that carries a
/// marker, or `None` if nothing matches.
///
/// Known limitation: this is a single innermost-first walk, so an inner `.git`
/// shadows an outer `.jetpack.yml`. In real checkouts the `git` fast path in
/// [`detect_automation_root`] dominates, so this only matters in unusual non-git
/// trees — acceptable for now.
fn find_automation_root_by_marker(start: &Path) -> Option<PathBuf> {
    for ancestor in start.ancestors() {
        if has_marker(ancestor) {
            return Some(ancestor.to_path_buf());
        }
    }
    None
}

/// Whether `dir` carries any automation-repository marker (see
/// [`find_automation_root_by_marker`] for the priority order).
fn has_marker(dir: &Path) -> bool {
    if dir.join(".jetpack.yml").is_file() {
        return true;
    }
    if dir.join(".git").exists() {
        return true;
    }
    dir.join("playbooks").is_dir() && dir.join("roles").is_dir()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn mkdir(p: &Path) {
        fs::create_dir_all(p).unwrap();
    }

    fn touch(p: &Path) {
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(p, b"").unwrap();
    }

    /// `git init` inside `dir`; returns true if git is available and succeeded.
    fn git_init(dir: &Path) -> bool {
        Command::new("git")
            .arg("init")
            .current_dir(dir)
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    #[test]
    fn marker_finds_jetpack_yml_from_nested_dir() {
        let root = TempDir::new().unwrap();
        touch(&root.path().join(".jetpack.yml"));
        let nested = root.path().join("playbooks").join("gravity");
        mkdir(&nested);
        assert_eq!(
            find_automation_root_by_marker(&nested),
            Some(root.path().to_path_buf())
        );
    }

    #[test]
    fn marker_finds_git_directory() {
        let root = TempDir::new().unwrap();
        mkdir(&root.path().join(".git"));
        let nested = root.path().join("sub");
        mkdir(&nested);
        assert_eq!(
            find_automation_root_by_marker(&nested),
            Some(root.path().to_path_buf())
        );
    }

    #[test]
    fn marker_finds_git_file_used_by_worktrees() {
        let root = TempDir::new().unwrap();
        // a `.git` *file* (gitdir pointer), as used by worktrees and submodules
        fs::write(root.path().join(".git"), b"gitdir: /elsewhere\n").unwrap();
        let nested = root.path().join("sub");
        mkdir(&nested);
        assert_eq!(
            find_automation_root_by_marker(&nested),
            Some(root.path().to_path_buf())
        );
    }

    #[test]
    fn marker_finds_playbooks_plus_roles_layout() {
        let root = TempDir::new().unwrap();
        mkdir(&root.path().join("playbooks"));
        mkdir(&root.path().join("roles"));
        let nested = root.path().join("playbooks").join("x");
        mkdir(&nested);
        assert_eq!(
            find_automation_root_by_marker(&nested),
            Some(root.path().to_path_buf())
        );
    }

    #[test]
    fn marker_returns_none_without_any_marker() {
        let root = TempDir::new().unwrap();
        let nested = root.path().join("a").join("b");
        mkdir(&nested);
        assert_eq!(find_automation_root_by_marker(&nested), None);
    }

    #[test]
    fn marker_matches_when_start_is_the_root() {
        let root = TempDir::new().unwrap();
        touch(&root.path().join(".jetpack.yml"));
        assert_eq!(
            find_automation_root_by_marker(root.path()),
            Some(root.path().to_path_buf())
        );
    }

    #[test]
    fn detect_falls_back_to_start_without_markers_or_git() {
        let root = TempDir::new().unwrap();
        // a tempdir is neither a git checkout nor a marker-bearing tree
        assert_eq!(
            detect_automation_root(root.path()),
            root.path().to_path_buf()
        );
    }

    #[test]
    fn detect_uses_marker_when_not_a_git_checkout() {
        let root = TempDir::new().unwrap();
        touch(&root.path().join(".jetpack.yml"));
        let nested = root.path().join("deploy").join("playbooks");
        mkdir(&nested);
        assert_eq!(detect_automation_root(&nested), root.path().to_path_buf());
    }

    #[test]
    fn git_toplevel_returns_none_outside_a_checkout() {
        let root = TempDir::new().unwrap();
        assert_eq!(git_toplevel(root.path()), None);
    }

    #[test]
    fn git_toplevel_returns_root_inside_a_checkout() {
        let root = TempDir::new().unwrap();
        if !git_init(root.path()) {
            return; // git assumed present in dev/CI; no-op if unavailable
        }
        let detected = git_toplevel(root.path()).expect("git checkout detected");
        assert_eq!(
            detected.canonicalize().unwrap(),
            root.path().canonicalize().unwrap()
        );
    }

    #[test]
    fn detect_prefers_inner_git_root_over_outer_marker() {
        // inner is its own git checkout; the outer dir carries a `.jetpack.yml`.
        // the git fast path wins, returning inner — not the outer marker root.
        let outer = TempDir::new().unwrap();
        touch(&outer.path().join(".jetpack.yml"));
        let inner = outer.path().join("inner");
        mkdir(&inner);
        if !git_init(&inner) {
            return; // git assumed present in dev/CI; no-op if unavailable
        }
        let detected = detect_automation_root(&inner);
        assert_eq!(
            detected.canonicalize().unwrap(),
            inner.canonicalize().unwrap()
        );
    }
}
