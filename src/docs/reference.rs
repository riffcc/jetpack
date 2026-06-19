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

//! The code-generated module + CLI reference. Reads structural metadata from
//! the code (`registry::list::Task` via `strum::EnumIter`, `cli::parser`) and
//! DeepWiki-style prose overrides from `docs/reference.json`, then emits Hugo
//! markdown into `docs/content/docs/reference/`. Structural facts (which modules
//! exist, their tags/categories, the CLI surface) can't drift from the code;
//! all prose is human-owned in the override file.

use crate::cli::parser::{Arguments, all_mode_names};
use crate::registry::list::Task;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::Path;
use strum::IntoEnumIterator;

const CATEGORY_ORDER: &[&str] = &[
    "access",
    "commands",
    "control",
    "files",
    "integrations",
    "inventory",
    "packages",
    "proxmox",
    "services",
];

/// Flags that take no value (the parser's standalone-flag set). Everything else
/// consumes the next argument. Mirrors the standalone match in parser.rs.
const STANDALONE_FLAGS: &[&str] = &[
    "--allow-localhost-delegation",
    "--forward-agent",
    "-v",
    "-vv",
    "-vvv",
    "--ask-login-password",
    "--async",
    "--no-browser",
];

// =================================================================================================
// Override model (adapted DeepWiki `wiki.json`)
// =================================================================================================

#[derive(Deserialize, Default)]
pub struct ReferenceOverrides {
    #[serde(default)]
    pub repo_notes: Vec<Note>,
    #[serde(default)]
    pub modules: BTreeMap<String, ModuleOverride>,
    #[serde(default)]
    pub cli: CliOverrides,
}

#[derive(Deserialize, Default, Clone)]
pub struct Note {
    pub content: String,
    #[serde(default)]
    pub author: Option<String>,
}

#[derive(Deserialize, Default, Clone)]
pub struct ModuleOverride {
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub notes: Vec<String>,
    #[serde(default)]
    pub examples: Vec<String>,
    #[serde(default)]
    pub parameters: Vec<ParamDoc>,
    #[serde(default)]
    pub replace: bool,
    #[serde(default)]
    pub exclude: bool,
    #[serde(default)]
    pub source_link: Option<String>,
}

#[derive(Deserialize, Default, Clone)]
pub struct ParamDoc {
    pub name: String,
    #[serde(rename = "type", default)]
    pub param_type: Option<String>,
    #[serde(default)]
    pub required: Option<bool>,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Deserialize, Default, Clone)]
pub struct CliOverrides {
    #[serde(default)]
    pub modes: BTreeMap<String, CliItemOverride>,
    #[serde(default)]
    pub flags: BTreeMap<String, CliItemOverride>,
}

#[derive(Deserialize, Default, Clone)]
pub struct CliItemOverride {
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub exclude: bool,
}

/// Load overrides from `docs/reference.json`. A missing file is fine (everything
/// renders as a stub); a malformed file is an error.
pub fn load_override(path: &Path) -> Result<ReferenceOverrides, String> {
    match std::fs::read_to_string(path) {
        Ok(s) => serde_json::from_str(&s).map_err(|e| format!("{}: {}", path.display(), e)),
        Err(_) => Ok(ReferenceOverrides::default()),
    }
}

// =================================================================================================
// Code metadata (the part that can't drift)
// =================================================================================================

struct RenderInput {
    /// (tag, category), one entry per `Task` variant (yum and dnf both appear).
    modules: Vec<(String, String)>,
    modes: Vec<String>,
    flags: Vec<String>,
}

fn collect() -> RenderInput {
    let modules: Vec<(String, String)> = Task::iter()
        .map(|t| (t.as_ref().to_string(), t.category().to_string()))
        .collect();
    let flags: Vec<String> = Arguments::iter().map(|a| a.as_str().to_string()).collect();
    RenderInput {
        modules,
        modes: all_mode_names().iter().map(|s| s.to_string()).collect(),
        flags,
    }
}

// =================================================================================================
// Generate / check
// =================================================================================================

/// Write every reference page into `out_dir` (docs/content/docs/reference/).
pub fn generate(overrides: &ReferenceOverrides, out_dir: &Path) -> Result<(), String> {
    let files = render_all(&collect(), overrides);
    for (rel, content) in &files {
        let path = out_dir.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("mkdir {}: {}", parent.display(), e))?;
        }
        std::fs::write(&path, content).map_err(|e| format!("write {}: {}", path.display(), e))?;
    }
    Ok(())
}

/// Verify the committed reference matches what `generate` would produce. Errors
/// on any missing, changed, or stale file — the CI guard (`gen-reference --check`).
pub fn check(overrides: &ReferenceOverrides, out_dir: &Path) -> Result<(), String> {
    let expected = render_all(&collect(), overrides);
    for (rel, content) in &expected {
        let on_disk = std::fs::read_to_string(out_dir.join(rel)).unwrap_or_default();
        if on_disk != *content {
            return Err(format!(
                "reference docs are out of date: {} — run `cargo run -- gen-reference` and commit",
                rel
            ));
        }
    }
    for rel in list_files(out_dir, out_dir)? {
        if !expected.contains_key(&rel) {
            return Err(format!(
                "stale reference file: {} — run `cargo run -- gen-reference` and commit",
                rel
            ));
        }
    }
    Ok(())
}

fn list_files(root: &Path, base: &Path) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let entries =
        std::fs::read_dir(root).map_err(|e| format!("read_dir {}: {}", root.display(), e))?;
    for entry in entries {
        let path = entry.map_err(|e| format!("dir entry: {}", e))?.path();
        if path.is_dir() {
            out.extend(list_files(&path, base)?);
        } else {
            out.push(
                path.strip_prefix(base)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
            );
        }
    }
    Ok(out)
}

// =================================================================================================
// Rendering
// =================================================================================================

/// Render every reference file: relative path -> markdown content.
fn render_all(input: &RenderInput, ov: &ReferenceOverrides) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    out.insert("_index.md".to_string(), render_index(input, ov));
    out.insert("cli.md".to_string(), render_cli(input, ov));
    let mut mods: Vec<&(String, String)> = input.modules.iter().collect();
    mods.sort_by(|a, b| a.0.cmp(&b.0));
    for (i, (tag, category)) in mods.iter().enumerate() {
        if let Some(content) = render_module(tag, category, ov.modules.get(tag), i + 1) {
            out.insert(format!("modules/{}.md", tag), content);
        }
    }
    out
}

fn render_index(input: &RenderInput, ov: &ReferenceOverrides) -> String {
    let mut s = String::from(
        "---\ntitle: Reference\nweight: 8\ncascade:\n  type: docs\nsidebar:\n  open: true\n---\n\n",
    );
    s.push_str(
        "This section is **auto-generated** from the source (`src/registry/list.rs` and \
         `src/cli/parser.rs`). Don't edit these pages by hand — run `jetpack gen-reference` to \
         regenerate, and edit [`reference.json`](https://github.com/riffcc/jetpack/blob/main/docs/reference.json) \
         to add descriptions, examples, or parameter docs.\n\n",
    );
    if !ov.repo_notes.is_empty() {
        s.push_str("> **Notes:**\n>\n");
        for note in &ov.repo_notes {
            s.push_str(&format!("> {}\n>\n", note.content));
        }
        s.push('\n');
    }
    s.push_str("## Modules\n\n");
    for &cat in CATEGORY_ORDER {
        let mut tags: Vec<String> = input
            .modules
            .iter()
            .filter(|m| m.1 == cat)
            .map(|m| m.0.clone())
            .collect();
        tags.sort();
        if tags.is_empty() {
            continue;
        }
        s.push_str(&format!(
            "### {}\n\n| Module | Description |\n|---|---|\n",
            cat
        ));
        for tag in &tags {
            let desc = ov
                .modules
                .get(tag)
                .and_then(|m| m.description.as_ref())
                .map(|d| first_line(d))
                .unwrap_or_else(|| "—".to_string());
            s.push_str(&format!("| [`!{}`](modules/{}) | {} |\n", tag, tag, desc));
        }
        s.push('\n');
    }
    s.push_str("## Command line\n\nSee [CLI reference](cli) for modes and flags.\n");
    s
}

/// Render one module page, or `None` if excluded.
fn render_module(
    tag: &str,
    category: &str,
    ov: Option<&ModuleOverride>,
    weight: usize,
) -> Option<String> {
    let ov = ov.cloned().unwrap_or_default();
    if ov.exclude {
        return None;
    }
    let desc_fm = ov
        .description
        .as_ref()
        .map(|d| first_line(d))
        .unwrap_or_else(|| format!("The {} module", tag));
    let mut s = String::new();
    s.push_str(&format!(
        "---\ntitle: {}\nweight: {}\ndescription: \"{}\"\n---\n\n",
        tag,
        weight,
        desc_fm.replace('"', "\\\"")
    ));
    s.push_str("<!-- AUTO-GENERATED by `jetpack gen-reference`. Edit docs/reference.json, not this file. -->\n\n");
    s.push_str(&format!("# `!{}`\n\n**Category:** {}\n\n", tag, category));
    if ov.replace {
        if let Some(d) = &ov.description {
            s.push_str(d);
            s.push_str("\n\n");
        } else {
            s.push_str("<!-- replace=true but no description set in reference.json -->\n\n");
        }
    } else if let Some(d) = &ov.description {
        s.push_str(d);
        s.push_str("\n\n");
    } else {
        s.push_str("_Documentation for this module has not been written yet. Add it via `docs/reference.json`._\n\n");
    }
    if !ov.notes.is_empty() {
        s.push_str("## Notes\n\n");
        for n in &ov.notes {
            s.push_str(&format!("- {}\n", n));
        }
        s.push('\n');
    }
    if !ov.parameters.is_empty() {
        s.push_str(
            "## Parameters\n\n| Name | Type | Required | Description |\n|---|---|---|---|\n",
        );
        for p in &ov.parameters {
            let ty = p.param_type.as_deref().unwrap_or("—");
            let req = p
                .required
                .map(|b| if b { "yes" } else { "no" })
                .unwrap_or("—");
            let desc = p.description.as_deref().unwrap_or("—");
            s.push_str(&format!("| `{}` | {} | {} | {} |\n", p.name, ty, req, desc));
        }
        s.push('\n');
    }
    if !ov.examples.is_empty() {
        s.push_str("## Examples\n\n");
        for ex in &ov.examples {
            s.push_str(&format!("```yaml\n{}\n```\n\n", ex));
        }
    }
    Some(s)
}

fn render_cli(input: &RenderInput, ov: &ReferenceOverrides) -> String {
    let mut s = String::from("---\ntitle: CLI\nweight: 99\ncascade:\n  type: docs\n---\n\n");
    s.push_str("<!-- AUTO-GENERATED by `jetpack gen-reference`. Edit docs/reference.json, not this file. -->\n\n");
    s.push_str("# Command-line reference\n\n");
    s.push_str("## Modes\n\n| Mode | Description |\n|---|---|\n");
    for mode in &input.modes {
        let item = ov.cli.modes.get(mode);
        if item.is_some_and(|i| i.exclude) {
            continue;
        }
        let desc = item.and_then(|i| i.description.as_deref()).unwrap_or("—");
        s.push_str(&format!("| `{}` | {} |\n", mode, desc));
    }
    s.push_str("\n## Flags\n\n| Flag | Takes value | Description |\n|---|---|---|\n");
    let mut flags = input.flags.clone();
    flags.sort();
    for flag in &flags {
        let item = ov.cli.flags.get(flag);
        if item.is_some_and(|i| i.exclude) {
            continue;
        }
        let takes = if STANDALONE_FLAGS.contains(&flag.as_str()) {
            "no"
        } else {
            "yes"
        };
        let desc = item.and_then(|i| i.description.as_deref()).unwrap_or("—");
        s.push_str(&format!("| `{}` | {} | {} |\n", flag, takes, desc));
    }
    s
}

fn first_line(s: &str) -> String {
    s.lines().next().unwrap_or("").trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_module_stub_when_no_override() {
        let out = render_module("file", "files", None, 1).unwrap();
        assert!(out.contains("# `!file`"));
        assert!(out.contains("**Category:** files"));
        assert!(out.contains("has not been written yet"));
    }

    #[test]
    fn render_module_exclude_returns_none() {
        let ov = ModuleOverride {
            exclude: true,
            ..Default::default()
        };
        assert!(render_module("dnf", "packages", Some(&ov), 2).is_none());
    }

    #[test]
    fn render_module_replace_uses_override_only() {
        let ov = ModuleOverride {
            replace: true,
            description: Some("Custom lead.".to_string()),
            ..Default::default()
        };
        let out = render_module("facts", "control", Some(&ov), 3).unwrap();
        assert!(out.contains("Custom lead."));
        assert!(!out.contains("has not been written yet"));
    }

    #[test]
    fn render_all_has_index_cli_and_modules() {
        let input = RenderInput {
            modules: vec![
                ("apt".into(), "packages".into()),
                ("file".into(), "files".into()),
            ],
            modes: vec!["local".into()],
            flags: vec!["--playbook".into()],
        };
        let files = render_all(&input, &ReferenceOverrides::default());
        assert!(files.contains_key("_index.md"));
        assert!(files.contains_key("cli.md"));
        assert!(files.contains_key("modules/apt.md"));
        assert!(files.contains_key("modules/file.md"));
        assert!(files["cli.md"].contains("| `--playbook` | yes |"));
    }
}
