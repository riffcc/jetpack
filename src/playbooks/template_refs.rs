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
// long with this program.  If not, see <http://www.gnu.org/licenses/>.

//! Static extraction of the variables a Handlebars template references.
//!
//! This walks the `handlebars` engine's own parsed template AST — it never
//! renders. Parsing never resolves variables (only rendering does, where Strict
//! mode then errors on the missing ones), so this works on templates that
//! *cannot* be rendered. That is exactly what a "which variables would be
//! undefined?" diagnostic needs: it must inspect templates that reference
//! missing variables. Reusing the engine's parser — rather than a second grammar
//! such as tree-sitter-handlebars — means the result can never disagree with
//! what actually renders, and it costs no new dependency.

use std::collections::BTreeSet;

use handlebars::template::{
    BlockParam, DecoratorTemplate, HelperTemplate, Parameter, Subexpression, Template,
    TemplateElement,
};

/// The top-level data-variable names a template references.
///
/// Excluded: helper and block-helper names (`{{ upper x }}` → `x`, not `upper`),
/// block-param aliases (`{{#each xs as |item| }}` → `xs`, not `item`), Handlebars
/// locals (`@index`, `@key`, `@first`, `@last`, `this`), and literals. Nested
/// paths collapse to their top-level segment (`{{ user.name }}` → `user`) so the
/// result can be compared against top-level variable keys.
///
/// The set is sorted for deterministic diagnostics and tests. A malformed
/// template yields an `Err` (the caller decides whether a diagnostic parse
/// failure is fatal).
pub fn referenced_variables(template: &str) -> Result<BTreeSet<String>, String> {
    let compiled = Template::compile(template).map_err(|e| format!("Template error: {}", e))?;
    let mut out = BTreeSet::new();
    let no_locals = BTreeSet::new();
    walk_elements(&compiled.elements, &no_locals, &mut out);
    Ok(out)
}

/// Every variable referenced anywhere inside a parsed YAML value.
///
/// Walks the value recursively — through sequences, mappings, and `!tagged`
/// task nodes — and runs [`referenced_variables`] on each string leaf, unioning
/// the results. This is how the inline templated fields of *any* task type are
/// collected uniformly: a task file is parsed generically and every string in it
/// is fed through the extractor, with no need to enumerate the per-module task
/// structs (which would drift whenever a module is added or changes its fields).
///
/// A leaf that fails to parse as a template is skipped rather than failing the
/// whole scan: malformed templates are reported by `syntax-check`, and a
/// diagnostic should not abort because of one odd string.
pub fn referenced_variables_in_value(value: &serde_yaml::Value) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    walk_value(value, &mut out);
    out
}

fn walk_value(value: &serde_yaml::Value, out: &mut BTreeSet<String>) {
    match value {
        serde_yaml::Value::String(s) => {
            if let Ok(refs) = referenced_variables(s) {
                out.extend(refs);
            }
        }
        serde_yaml::Value::Sequence(seq) => {
            for item in seq {
                walk_value(item, out);
            }
        }
        serde_yaml::Value::Mapping(map) => {
            for (k, v) in map {
                walk_value(k, out);
                walk_value(v, out);
            }
        }
        serde_yaml::Value::Tagged(tagged) => walk_value(&tagged.value, out),
        _ => {}
    }
}

fn walk_elements(
    elements: &[TemplateElement],
    locals: &BTreeSet<String>,
    out: &mut BTreeSet<String>,
) {
    for element in elements {
        match element {
            TemplateElement::RawString(_) | TemplateElement::Comment(_) => {}
            TemplateElement::Expression(ht)
            | TemplateElement::HtmlExpression(ht)
            | TemplateElement::HelperBlock(ht) => walk_helper(ht, locals, out),
            TemplateElement::DecoratorExpression(dt)
            | TemplateElement::DecoratorBlock(dt)
            | TemplateElement::PartialExpression(dt)
            | TemplateElement::PartialBlock(dt) => walk_decorator(dt, locals, out),
            _ => {}
        }
    }
}

fn walk_helper(ht: &HelperTemplate, locals: &BTreeSet<String>, out: &mut BTreeSet<String>) {
    // A bare `{{ x }}` is a variable access: its name IS the reference. A helper
    // call `{{ f a b }}` or a block `{{#each xs }}` has a *helper* name we skip,
    // and its references live in params/hash. `block` (always set for a
    // HelperBlock) plus empty params/hash distinguishes the bare-variable case —
    // replicating the engine's own `is_name_only` check from public fields.
    let name_only = !ht.block && ht.params.is_empty() && ht.hash.is_empty();
    if name_only {
        collect_param(&ht.name, locals, out);
    } else {
        for param in &ht.params {
            collect_param(param, locals, out);
        }
        for value in ht.hash.values() {
            collect_param(value, locals, out);
        }
    }
    // `as |item|` aliases bind loop scope only *inside* the body/inverse, so they
    // shadow data there (not where the params are evaluated).
    let body_locals = block_param_aliases(ht, locals);
    if let Some(body) = &ht.template {
        walk_elements(&body.elements, &body_locals, out);
    }
    if let Some(inverse) = &ht.inverse {
        walk_elements(&inverse.elements, &body_locals, out);
    }
}

fn block_param_aliases(ht: &HelperTemplate, outer: &BTreeSet<String>) -> BTreeSet<String> {
    let mut combined = outer.clone();
    if let Some(bp) = &ht.block_param {
        match bp {
            BlockParam::Single(p) => {
                if let Some(name) = alias_name(p) {
                    combined.insert(name);
                }
            }
            BlockParam::Pair((p1, p2)) => {
                if let Some(n) = alias_name(p1) {
                    combined.insert(n);
                }
                if let Some(n) = alias_name(p2) {
                    combined.insert(n);
                }
            }
            _ => {}
        }
    }
    combined
}

// Aliases are always `Parameter::Name` (see the engine parser), never data paths.
fn alias_name(p: &Parameter) -> Option<String> {
    match p {
        Parameter::Name(n) => Some(n.clone()),
        _ => None,
    }
}

fn walk_decorator(dt: &DecoratorTemplate, locals: &BTreeSet<String>, out: &mut BTreeSet<String>) {
    // A decorator/partial name is not a data variable.
    for param in &dt.params {
        collect_param(param, locals, out);
    }
    for value in dt.hash.values() {
        collect_param(value, locals, out);
    }
    if let Some(body) = &dt.template {
        walk_elements(&body.elements, locals, out);
    }
}

fn collect_param(param: &Parameter, locals: &BTreeSet<String>, out: &mut BTreeSet<String>) {
    match param {
        Parameter::Name(_) | Parameter::Path(_) => {
            if let Some(raw) = param.as_name() {
                if let Some(top) = top_level_variable(raw) {
                    if !locals.contains(&top) {
                        out.insert(top);
                    }
                }
            }
        }
        Parameter::Subexpression(sub) => walk_subexpression(sub, locals, out),
        Parameter::Literal(_) => {}
        _ => {}
    }
}

fn walk_subexpression(sub: &Subexpression, locals: &BTreeSet<String>, out: &mut BTreeSet<String>) {
    match &*sub.element {
        TemplateElement::Expression(ht)
        | TemplateElement::HtmlExpression(ht)
        | TemplateElement::HelperBlock(ht) => walk_helper(ht, locals, out),
        TemplateElement::DecoratorExpression(dt)
        | TemplateElement::DecoratorBlock(dt)
        | TemplateElement::PartialExpression(dt)
        | TemplateElement::PartialBlock(dt) => walk_decorator(dt, locals, out),
        TemplateElement::RawString(_) | TemplateElement::Comment(_) => {}
        _ => {}
    }
}

// Reduce a raw reference (`user.name`, `../foo`, `items.[0]`, `@key`) to the
// single top-level data variable it depends on, or `None` when it is a Handlebars
// local / context pointer rather than a named variable.
fn top_level_variable(raw: &str) -> Option<String> {
    let mut rest = raw.trim();
    // Handlebars locals (@index, @key, @first, @last, @root, …) are not data vars.
    if rest.starts_with('@') {
        return None;
    }
    // `this` is the whole current context — no single variable to name.
    if rest.is_empty() || rest == "this" {
        return None;
    }
    // Climb out of block/local scopes (`../`, `./`) before reading the name.
    loop {
        if let Some(s) = rest.strip_prefix("../") {
            rest = s;
        } else if let Some(s) = rest.strip_prefix("./") {
            rest = s;
        } else {
            break;
        }
    }
    // `this.` is a context pointer prefix; a leading `.` is separator noise.
    rest = rest
        .strip_prefix("this.")
        .unwrap_or(rest)
        .trim_start_matches('.');
    let seg = rest.split(|c| c == '.' || c == '/').next()?.trim();
    if seg.is_empty() {
        None
    } else {
        Some(seg.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::referenced_variables;
    use std::collections::BTreeSet;

    fn refs(template: &str) -> BTreeSet<String> {
        referenced_variables(template).expect("template should compile")
    }

    fn set(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    // --- bare variable access -------------------------------------------------

    #[test]
    fn single_bare_variable() {
        assert_eq!(refs("{{ foo }}"), set(&["foo"]));
    }

    #[test]
    fn multiple_bare_variables() {
        assert_eq!(refs("{{ a }} and {{ b }}"), set(&["a", "b"]));
    }

    #[test]
    fn deduplicates_repeated_references() {
        assert_eq!(refs("{{ foo }}{{ foo }}{{ foo }}"), set(&["foo"]));
    }

    #[test]
    fn nested_path_collapses_to_top_level() {
        assert_eq!(refs("{{ user.name }}"), set(&["user"]));
        assert_eq!(refs("{{ a.b.c }}"), set(&["a"]));
    }

    #[test]
    fn unescaped_html_expression_is_still_a_reference() {
        assert_eq!(refs("{{{ raw }}}"), set(&["raw"]));
    }

    // --- helpers and blocks ---------------------------------------------------

    #[test]
    fn helper_call_skips_helper_name_keeps_params() {
        // `upper` is a helper; `name` is the referenced variable.
        assert_eq!(refs("{{ upper name }}"), set(&["name"]));
    }

    #[test]
    fn helper_with_literal_param() {
        // `5` is a literal, not a variable.
        assert_eq!(refs("{{ eq count 5 }}"), set(&["count"]));
    }

    #[test]
    fn helper_hash_values_are_references() {
        assert_eq!(refs("{{ log message=msg }}"), set(&["msg"]));
    }

    #[test]
    fn if_block_param_is_a_reference() {
        assert_eq!(refs("{{#if flag}}yes{{/if}}"), set(&["flag"]));
    }

    #[test]
    fn else_branch_does_not_duplicate() {
        assert_eq!(refs("{{#if flag}}a{{else}}b{{/if}}"), set(&["flag"]));
    }

    #[test]
    fn each_block_collection_is_a_reference() {
        assert_eq!(refs("{{#each items}}x{{/each}}"), set(&["items"]));
    }

    #[test]
    fn block_param_alias_is_not_a_reference() {
        // `y` is a loop alias, not data; only `x` is referenced.
        assert_eq!(refs("{{#with x as |y|}}{{ y }}{{/with}}"), set(&["x"]));
    }

    #[test]
    fn recurses_through_nested_blocks() {
        assert_eq!(
            refs("{{#each items}}{{#if flag}}{{ val }}{{/if}}{{/each}}"),
            set(&["flag", "items", "val"])
        );
    }

    #[test]
    fn subexpression_unwraps_to_its_references() {
        // `{{ (upper name) }}` → the subexpression references `name`.
        assert_eq!(refs("{{ (upper name) }}"), set(&["name"]));
    }

    // --- locals and builtins are excluded -------------------------------------

    #[test]
    fn handlebars_locals_are_excluded() {
        assert_eq!(
            refs("{{#each items}}{{ @index }}: {{ @key }}{{/each}}"),
            set(&["items"])
        );
    }

    #[test]
    fn current_context_is_excluded() {
        assert_eq!(refs("{{ this }}"), set(&[]));
    }

    // --- non-references and edge cases ----------------------------------------

    #[test]
    fn plain_text_has_no_references() {
        assert_eq!(refs("hello world, no tags here"), set(&[]));
    }

    #[test]
    fn escaped_mustache_is_not_a_reference() {
        // `\{{escaped}}` is a literal raw string in Handlebars.
        assert_eq!(refs(r"prefix \{{escaped}} suffix"), set(&[]));
    }

    #[test]
    fn parent_scope_reference_resolves_to_its_variable() {
        // `../foo` reads `foo` from an enclosing block's scope.
        assert_eq!(
            refs("{{#each items}}{{ ../foo }}{{/each}}"),
            set(&["foo", "items"])
        );
    }

    #[test]
    fn array_index_path_resolves_to_its_variable() {
        assert_eq!(refs("{{ items.[0] }}"), set(&["items"]));
    }

    // --- value-tree walking (inline fields of any task type) ------------------

    use super::referenced_variables_in_value;

    #[test]
    fn value_walk_collects_from_a_sequence_of_strings() {
        // Templated YAML scalars are quoted (else `{{ }}` would parse as a flow
        // mapping); this is how they appear in real task files.
        let value: serde_yaml::Value =
            serde_yaml::from_str("- \"{{ a }}\"\n- plain\n- \"{{ b.c }}\"").unwrap();
        assert_eq!(referenced_variables_in_value(&value), set(&["a", "b"]));
    }

    #[test]
    fn value_walk_collects_from_a_tagged_task_mapping() {
        // A `!command` task with a templated argument, as it parses generically.
        let value: serde_yaml::Value =
            serde_yaml::from_str("!command\nargv:\n  - \"{{ service }}\"\n  - restart").unwrap();
        assert_eq!(referenced_variables_in_value(&value), set(&["service"]));
    }

    #[test]
    fn value_walk_ignores_non_string_scalars() {
        let value: serde_yaml::Value =
            serde_yaml::from_str("count: 5\nenabled: true\nname: \"{{ app }}\"").unwrap();
        assert_eq!(referenced_variables_in_value(&value), set(&["app"]));
    }

    #[test]
    fn value_walk_skips_malformed_leaves_without_aborting() {
        // A leaf that is valid YAML but invalid handlebars (unbalanced tag) must
        // be skipped, not abort the scan; the well-formed leaf is still collected.
        let value: serde_yaml::Value =
            serde_yaml::from_str("ok: \"{{ good }}\"\nbad: \"{{ oops \"").unwrap();
        assert_eq!(referenced_variables_in_value(&value), set(&["good"]));
    }
}
