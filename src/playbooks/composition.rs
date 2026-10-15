// Copyright (C) Riff Labs <team@riff.cc>
// SPDX-License-Identifier: GPL-3.0-or-later

//! File templates are rendered once; inserted fragments are never rendered again.
use handlebars::{
    Context, Handlebars, Helper, HelperDef, Output, RenderContext, RenderError, Renderable,
};
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Clone)]
struct IncludeDir {
    roots: Arc<Vec<PathBuf>>,
    stack: Vec<PathBuf>,
}

fn error(message: impl Into<String>) -> RenderError {
    handlebars::RenderErrorReason::Other(format!("include_dir: {}", message.into())).into()
}

fn confined(path: &Path, roots: &[PathBuf]) -> Result<PathBuf, RenderError> {
    let canonical = path
        .canonicalize()
        .map_err(|_| error(format!("cannot read template source {}", path.display())))?;
    if !roots.iter().any(|root| canonical.starts_with(root)) {
        return Err(error(format!(
            "template include escapes permitted source roots: {}",
            path.display()
        )));
    }
    Ok(canonical)
}

fn render_file<'reg: 'rc, 'rc>(
    path: &Path,
    state: &IncludeDir,
    registry: &'reg Handlebars<'reg>,
    context: &'rc Context,
    rc: &mut RenderContext<'reg, 'rc>,
    out: &mut dyn Output,
) -> Result<(), RenderError> {
    let path = confined(path, &state.roots)?;
    if state.stack.contains(&path) || state.stack.len() >= 64 {
        return Err(error(format!(
            "recursive template include cycle or depth limit at {}",
            path.display()
        )));
    }
    if !path.is_file() {
        return Err(error(format!(
            "template source must be a regular file: {}",
            path.display()
        )));
    }
    let source = std::fs::read_to_string(&path)
        .map_err(|_| error(format!("cannot read template source {}", path.display())))?;
    let mut nested = state.clone();
    nested.stack.push(path.clone());
    let mut registry = registry.clone();
    registry.register_helper("include_dir", Box::new(nested));
    let template = handlebars::Template::compile(&source)
        .map_err(|_| error(format!("invalid template in {}", path.display())))?;
    // Cloning rc retains the caller's each/with scope, but cannot leak fragment locals.
    template.render(&registry, context, &mut rc.clone(), out).map_err(|e| {
        if let handlebars::RenderErrorReason::Other(message)=e.reason()
            && message.starts_with("include_dir:") { return e; }
        error(format!("template evaluation failed in {} at line {:?}, column {:?} (check variables and helpers)", path.display(),e.line_no,e.column_no))
    })
}

impl HelperDef for IncludeDir {
    fn call<'reg: 'rc, 'rc>(
        &self,
        helper: &Helper<'rc>,
        registry: &'reg Handlebars<'reg>,
        context: &'rc Context,
        rc: &mut RenderContext<'reg, 'rc>,
        out: &mut dyn Output,
    ) -> Result<(), RenderError> {
        if helper.params().len() != 1 || !helper.hash().is_empty() {
            return Err(error("include_dir requires exactly one directory argument"));
        }
        let name = helper
            .param(0)
            .and_then(|p| p.value().as_str())
            .ok_or_else(|| error("include_dir requires a string path"))?;
        let parent = self
            .stack
            .last()
            .and_then(|p| p.parent())
            .ok_or_else(|| error("include_dir requires a file template"))?;
        let directory = confined(&parent.join(name), &self.roots)?;
        let entries = std::fs::read_dir(&directory).map_err(|_| {
            error(format!(
                "cannot read include directory {}",
                directory.display()
            ))
        })?;
        let mut paths = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|_| error("cannot read include directory entry"))?;
            if entry.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            let path = confined(&entry.path(), &self.roots)?;
            let meta =
                std::fs::metadata(&path).map_err(|_| error("cannot read fragment metadata"))?;
            if meta.is_dir() {
                continue;
            }
            if !meta.is_file() {
                return Err(error(format!(
                    "unsupported fragment object: {}",
                    entry.path().display()
                )));
            }
            paths.push((entry.file_name(), path));
        }
        paths.sort_by(|a, b| a.0.cmp(&b.0));
        for (_, path) in paths {
            let mut rendered = handlebars::StringOutput::new();
            render_file(&path, self, registry, context, rc, &mut rendered)?;
            let text = rendered
                .into_string()
                .map_err(|_| error("fragment output is not UTF-8"))?;
            out.write(&text)?;
            if !text.is_empty() && !text.ends_with('\n') {
                out.write("\n")?;
            }
        }
        Ok(())
    }
}

/// Use Handlebars' AST to distinguish helper calls from text, comments,
/// escaped expressions, and similarly named variables. Name-only expressions
/// remain ordinary variables for backwards compatibility.
fn has_includes(template: &handlebars::Template) -> bool {
    use handlebars::template::{Parameter, TemplateElement};
    fn parameter(p: &Parameter) -> bool {
        match p {
            Parameter::Subexpression(s) => element(&s.element),
            _ => false,
        }
    }
    fn element(e: &TemplateElement) -> bool {
        match e {
            TemplateElement::Expression(h)
            | TemplateElement::HtmlExpression(h)
            | TemplateElement::HelperBlock(h) => {
                (h.name.as_name() == Some("include_dir")
                    && (h.block || !h.params.is_empty() || !h.hash.is_empty()))
                    || parameter(&h.name)
                    || h.params.iter().any(parameter)
                    || h.hash.values().any(parameter)
                    || h.template.as_ref().is_some_and(has_includes)
                    || h.inverse.as_ref().is_some_and(has_includes)
            }
            TemplateElement::DecoratorExpression(d)
            | TemplateElement::DecoratorBlock(d)
            | TemplateElement::PartialExpression(d)
            | TemplateElement::PartialBlock(d) => {
                parameter(&d.name)
                    || d.params.iter().any(parameter)
                    || d.hash.values().any(parameter)
                    || d.template.as_ref().is_some_and(has_includes)
            }
            _ => false,
        }
    }
    template.elements.iter().any(element)
}

/// Explicit sources outside the active playbook/role allow only their parent
/// directory. Otherwise that active source directory is the permitted root.
pub(crate) fn render(
    path: &Path,
    root: &Path,
    data: &serde_yaml::Mapping,
) -> Result<String, String> {
    let original_root = root;
    let root = root
        .canonicalize()
        .map_err(|_| "cannot resolve template source root".to_string())?;
    let root = if path.is_absolute()
        && !path.starts_with(original_root)
        && !path
            .canonicalize()
            .map_err(|_| "cannot resolve template source")?
            .starts_with(&root)
    {
        path.parent()
            .ok_or("template source has no parent")?
            .canonicalize()
            .map_err(|_| "cannot resolve explicit template root")?
    } else {
        root
    };
    let source = confined(path, std::slice::from_ref(&root)).map_err(|e| e.to_string())?;
    let text =
        std::fs::read_to_string(&source).map_err(|_| "cannot read template source".to_string())?;
    // Preserve recursive variable expansion for existing standalone templates.
    // Composed files use one render pass so fragment text is never interpreted twice.
    let parsed = handlebars::Template::compile(&text)
        .map_err(|_| format!("invalid template in {}", source.display()))?;
    if !has_includes(&parsed) {
        return super::templar::Templar::new()
            .render(&text, data.clone(), super::templar::TemplateMode::Strict)
            .map_err(|_| format!("template evaluation failed in {}", source.display()));
    }
    let state = IncludeDir {
        roots: Arc::new(vec![root]),
        stack: vec![],
    };
    let registry = super::templar::file_registry();
    let context = Context::wraps(data).map_err(|_| "invalid template context".to_string())?;
    let mut out = handlebars::StringOutput::new();
    render_file(
        path,
        &state,
        &registry,
        &context,
        &mut RenderContext::new(None),
        &mut out,
    )
    .map_err(|e| e.to_string())?;
    out.into_string()
        .map_err(|_| "template output is not UTF-8".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{create_dir_all, write};

    #[test]
    fn sorted_multiple_insertions_nested_relative_scopes_and_no_second_render() {
        let root = tempfile::tempdir().unwrap();
        let base = root.path();
        create_dir_all(base.join("templates")).unwrap();
        create_dir_all(base.join("fragments/nested")).unwrap();
        create_dir_all(base.join("empty")).unwrap();
        write(base.join("templates/main"),"[front]\n{{#each hosts}}{{include_dir \"../fragments\"}}{{/each}}[back]\n{{{include_dir \"../empty\"}}}").unwrap();
        write(base.join("fragments/20-last"), "<{{this}}> {{../literal}}").unwrap();
        write(
            base.join("fragments/10-first"),
            "first\n{{include_dir \"nested\"}}",
        )
        .unwrap();
        write(base.join("fragments/nested/one"), "nested={{this}}").unwrap();
        write(base.join("fragments/.ignored"), "{{missing}}").unwrap();
        let data = serde_yaml::from_str("hosts: [a, b]\nliteral: '{{not_rendered}}'").unwrap();
        let expected = "[front]\nfirst\nnested=a\n<a> {{not_rendered}}\nfirst\nnested=b\n<b> {{not_rendered}}\n[back]\n";
        assert_eq!(
            render(&base.join("templates/main"), base, &data).unwrap(),
            expected
        );
        std::fs::remove_file(base.join("fragments/20-last")).unwrap();
        assert!(
            !render(&base.join("templates/main"), base, &data)
                .unwrap()
                .contains("<a>")
        );
        write(base.join("fragments/30-new"), "added").unwrap();
        assert!(
            render(&base.join("templates/main"), base, &data)
                .unwrap()
                .contains("added\n")
        );
    }

    #[test]
    fn missing_bad_templates_and_cycles_have_source_diagnostics() {
        let root = tempfile::tempdir().unwrap();
        let main = root.path().join("main");
        write(&main, "{{include_dir \"missing\"}}").unwrap();
        assert!(
            render(&main, root.path(), &Default::default())
                .unwrap_err()
                .contains("missing")
        );
        write(&main, "{{include_dir \".\"}}").unwrap();
        assert!(
            render(&main, root.path(), &Default::default())
                .unwrap_err()
                .contains("cycle")
        );
        write(&main, "{{not_defined}}").unwrap();
        assert!(
            render(&main, root.path(), &Default::default())
                .unwrap_err()
                .contains("main")
        );
        write(&main, "{{#if").unwrap();
        assert!(render(&main, root.path(), &Default::default()).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn escaping_symlinks_and_special_objects_are_rejected_before_reading() {
        use std::os::unix::fs::symlink;
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        write(outside.path().join("private"), "secret").unwrap();
        create_dir_all(root.path().join("parts")).unwrap();
        let main = root.path().join("main");
        write(&main, "{{include_dir \"parts\"}}").unwrap();
        symlink(
            outside.path().join("private"),
            root.path().join("parts/escape"),
        )
        .unwrap();
        assert!(
            render(&main, root.path(), &Default::default())
                .unwrap_err()
                .contains("escapes")
        );
        std::fs::remove_file(root.path().join("parts/escape")).unwrap();
        symlink(&main, root.path().join("parts/loop")).unwrap();
        assert!(
            render(&main, root.path(), &Default::default())
                .unwrap_err()
                .contains("cycle")
        );
        std::fs::remove_file(root.path().join("parts/loop")).unwrap();
        assert!(
            std::process::Command::new("mkfifo")
                .arg(root.path().join("parts/pipe"))
                .status()
                .unwrap()
                .success()
        );
        assert!(
            render(&main, root.path(), &Default::default())
                .unwrap_err()
                .contains("unsupported")
        );
    }
}

#[cfg(test)]
mod compatibility_tests {
    use super::*;
    #[test]
    fn mentions_and_similarly_named_variables_keep_recursive_expansion() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        let data=serde_yaml::from_str("hostname: resolved-host\nvalue: '{{ hostname }}'\ninclude_directory: '{{ hostname }}'\ninclude_dir: '{{ hostname }}'").unwrap();
        for (text, expected) in [
            ("value={{ value }}\n", "value=resolved-host\n"),
            (
                "# include_dir documentation\nvalue={{ value }}\n",
                "# include_dir documentation\nvalue=resolved-host\n",
            ),
            (
                "{{! include_dir documentation }}\nvalue={{ value }}\n",
                "value=resolved-host\n",
            ),
            (
                "{{!-- include_dir documentation --}}\nvalue={{ value }}\n",
                "value=resolved-host\n",
            ),
            ("value={{ include_directory }}\n", "value=resolved-host\n"),
            ("value={{ include_dir }}\n", "value=resolved-host\n"),
            (
                "{{#if true}}# include_dir documentation\n{{ value }}{{else}}{{ include_directory }}{{/if}}",
                "# include_dir documentation\nresolved-host",
            ),
        ] {
            std::fs::write(&source, text).unwrap();
            assert_eq!(
                render(&source, root.path(), &data).unwrap(),
                expected,
                "{text}"
            );
        }
    }
    #[test]
    fn actual_calls_in_blocks_and_subexpressions_select_composition_but_escaped_text_does_not() {
        for source in [
            "{{include_dir \"parts\"}}",
            "{{{include_dir \"parts\"}}}",
            "{{#if true}}{{include_dir \"parts\"}}{{/if}}",
            "{{#if false}}x{{else}}{{include_dir \"parts\"}}{{/if}}",
            "{{#if (eq 1 1)}}{{include_dir \"parts\"}}{{/if}}",
            "{{concat (include_dir \"parts\") \"suffix\"}}",
        ] {
            assert!(
                has_includes(&handlebars::Template::compile(source).unwrap()),
                "{source}"
            );
        }
        for source in [
            "include_dir",
            "{{! include_dir \"parts\"}}",
            r#"\{{include_dir "parts"}}"#,
            "{{ include_dir }}",
            "{{ include_directory }}",
        ] {
            assert!(
                !has_includes(&handlebars::Template::compile(source).unwrap()),
                "{source}"
            );
        }
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("parts")).unwrap();
        std::fs::write(root.path().join("parts/one"), "{{ literal }}").unwrap();
        std::fs::write(
            root.path().join("source"),
            "{{#if true}}{{include_dir \"parts\"}}{{/if}}",
        )
        .unwrap();
        let data = serde_yaml::from_str("literal: '{{ must_stay_literal }}'").unwrap();
        assert_eq!(
            render(&root.path().join("source"), root.path(), &data).unwrap(),
            "{{ must_stay_literal }}\n"
        );
    }
}
