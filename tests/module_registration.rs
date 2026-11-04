//! Check textual registration for the conventional layout documented in tests/README.md.
//! This does not evaluate cfg attributes or prove that every test is compiled/run.
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn rust_files(directory: &Path, files: &mut BTreeSet<PathBuf>) {
    for entry in std::fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, files);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            files.insert(path);
        }
    }
}

#[test]
fn conventional_test_sources_are_registered() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let mut files = BTreeSet::new();
    rust_files(&root, &mut files);
    let mut pending: Vec<_> = files
        .iter()
        .filter(|path| path.parent() == Some(root.as_path()))
        .cloned()
        .collect();
    let mut reachable = BTreeSet::new();
    let declarations =
        regex::Regex::new(r"(?m)^\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+(\w+)\s*;").unwrap();
    while let Some(path) = pending.pop() {
        if !reachable.insert(path.clone()) {
            continue;
        }
        let directory =
            if path.parent() == Some(root.as_path()) || path.file_name().unwrap() == "mod.rs" {
                path.parent().unwrap().to_path_buf()
            } else {
                path.parent().unwrap().join(path.file_stem().unwrap())
            };
        let source = std::fs::read_to_string(&path).unwrap();
        for capture in declarations.captures_iter(&source) {
            let name = &capture[1];
            let candidates = [
                directory.join(format!("{name}.rs")),
                directory.join(name).join("mod.rs"),
            ];
            let found: Vec<_> = candidates
                .into_iter()
                .filter(|candidate| candidate.is_file())
                .collect();
            assert_eq!(
                found.len(),
                1,
                "ambiguous or missing test module {name} in {}",
                path.display()
            );
            pending.push(found[0].clone());
        }
    }
    let orphans: Vec<_> = files
        .difference(&reachable)
        .map(|path| path.strip_prefix(&root).unwrap().display().to_string())
        .collect();
    assert!(
        orphans.is_empty(),
        "test sources missing mod declarations: {}",
        orphans.join(", ")
    );
}
