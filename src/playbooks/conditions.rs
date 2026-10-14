// Copyright (C) Riff Labs <team@riff.cc>
// SPDX-License-Identifier: GPL-3.0-or-later

//! Lazy logical operators in the shared Handlebars condition language.
//! Parse with Handlebars, evaluate logical operands in order, and delegate all
//! other helpers and paths to the existing registry. An ordinary HelperDef
//! cannot implement this: Handlebars expands its arguments before calling it.

use handlebars::template::{Parameter, Template, TemplateElement};
use handlebars::{Context, Handlebars, JsonTruthy, RenderContext, Renderable, StringOutput};

pub(super) fn evaluate(
    registry: &Handlebars,
    expression: &str,
    data: &serde_yaml::Mapping,
) -> Result<bool, String> {
    evaluate_with_path_check(registry, expression, data, &mut |_| Ok(()))
}

/// Validate a path only when evaluation reaches it, including inside ordinary
/// helpers. This supports invocation-local deferred data without teaching the
/// condition engine about any particular module or response format.
pub(super) fn evaluate_with_path_check(
    registry: &Handlebars,
    expression: &str,
    data: &serde_yaml::Mapping,
    path_check: &mut impl FnMut(&handlebars::Path) -> Result<(), String>,
) -> Result<bool, String> {
    let mut template = Template::compile(&format!(
        "{{{{#if {expression} }}}}true{{{{else}}}}false{{{{/if}}}}"
    ))
    .map_err(|e| e.to_string())?;
    let context = Context::wraps(data).map_err(|e| e.to_string())?;
    // Keep the outer #if intact, preserving its truthiness, missing-variable
    // behavior and options such as includeZero. Only reduce expression nodes.
    for element in &mut template.elements {
        if let TemplateElement::HelperBlock(helper) = element {
            for parameter in &mut helper.params {
                *parameter = reduce(registry, &context, parameter, path_check)?;
            }
            for parameter in helper.hash.values_mut() {
                *parameter = reduce(registry, &context, parameter, path_check)?;
            }
        }
    }
    let mut output = StringOutput::new();
    template
        .render(
            registry,
            &context,
            &mut RenderContext::new(None),
            &mut output,
        )
        .map_err(|e| e.to_string())?;
    Ok(output.into_string().map_err(|e| e.to_string())? == "true")
}

fn reduce(
    registry: &Handlebars,
    context: &Context,
    parameter: &Parameter,
    path_check: &mut impl FnMut(&handlebars::Path) -> Result<(), String>,
) -> Result<Parameter, String> {
    if let Parameter::Path(path) = parameter {
        path_check(path)?;
    }
    let Parameter::Subexpression(subexpression) = parameter else {
        // Retain paths (including missing paths) so helpers like isdefined
        // still receive Handlebars' Missing value rather than a null literal.
        return Ok(parameter.clone());
    };
    let TemplateElement::Expression(helper) = subexpression.as_element() else {
        return Err("invalid condition subexpression".into());
    };
    let name = helper.name.as_name();
    if matches!(name, Some("and" | "or")) {
        if !helper.hash.is_empty() {
            return Err("logical operators do not accept hash arguments".into());
        }
        let is_and = name == Some("and");
        for operand in &helper.params {
            let operand = reduce(registry, context, operand, path_check)?;
            let value = operand
                .expand(registry, context, &mut RenderContext::new(None))
                .map_err(|e| e.to_string())?;
            if value.value().is_truthy(false) != is_and {
                return Ok(Parameter::Literal((!is_and).into()));
            }
        }
        // Boolean identities also cover the zero-argument forms.
        return Ok(Parameter::Literal(is_and.into()));
    }

    let mut reduced = subexpression.clone();
    if let TemplateElement::Expression(helper) = reduced.element.as_mut() {
        for operand in &mut helper.params {
            *operand = reduce(registry, context, operand, path_check)?;
        }
        for operand in helper.hash.values_mut() {
            *operand = reduce(registry, context, operand, path_check)?;
        }
        // Handlebars lookup is also a field access. Preserve validation when a
        // field name is supplied dynamically instead of written as a path.
        if helper.name.as_name() == Some("lookup")
            && helper.params.len() == 2
            && let Parameter::Path(_) = &helper.params[0]
        {
            let key = helper.params[1]
                .expand(registry, context, &mut RenderContext::new(None))
                .map_err(|e| e.to_string())?;
            if let Some(key) = key.value().as_str()
                && let Some(base) = helper.params[0].as_name()
                && let Ok(path) = handlebars::Path::parse(&format!("{base}.[{key}]"))
            {
                path_check(&path)?;
            }
        }
    }
    // Expand each ordinary helper exactly once, after reducing its operands.
    // Returning a typed literal preserves eq/string-helper behavior and keeps
    // nested logical expressions usable as ordinary helper arguments.
    let parameter = Parameter::Subexpression(reduced);
    let value = parameter
        .expand(registry, context, &mut RenderContext::new(None))
        .map_err(|e| e.to_string())?;
    Ok(Parameter::Literal(value.value().clone()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::playbooks::t_helpers::register_helpers;
    use handlebars::{Helper, HelperDef, RenderError, RenderErrorReason, ScopedJson};
    use std::sync::{Arc, Mutex};

    struct Probe(Arc<Mutex<Vec<String>>>);

    impl HelperDef for Probe {
        fn call_inner<'reg: 'rc, 'rc>(
            &self,
            helper: &Helper<'rc>,
            _: &'reg Handlebars,
            _: &'rc Context,
            _: &mut RenderContext<'reg, 'rc>,
        ) -> Result<ScopedJson<'rc>, RenderError> {
            let label = helper.param(0).unwrap().value().as_str().unwrap();
            self.0.lock().unwrap().push(label.into());
            if label == "fail" {
                return Err(RenderErrorReason::Other("evaluated".into()).into());
            }
            Ok(ScopedJson::Derived(
                helper.param(1).unwrap().value().clone(),
            ))
        }
    }

    fn registry() -> Handlebars<'static> {
        let mut registry = Handlebars::new();
        registry.set_strict_mode(true);
        register_helpers(&mut registry);
        registry
    }

    #[test]
    fn boolean_cases_and_identities() {
        let registry = registry();
        for (expression, expected) in [
            ("(and)", true),
            ("(or)", false),
            ("(and true)", true),
            ("(or false)", false),
            ("(and true true false)", false),
            ("(or false false true)", true),
            ("(and true true)", true),
            ("(or false false)", false),
            ("(and (or false true) (eq 1 1))", true),
            ("(eq (or false true) true)", true),
        ] {
            assert_eq!(
                evaluate(&registry, expression, &Default::default()).unwrap(),
                expected,
                "{expression}"
            );
        }
    }

    #[test]
    fn skipped_operands_never_run_and_reached_errors_are_preserved() {
        let mut registry = registry();
        let log = Arc::new(Mutex::new(Vec::new()));
        registry.register_helper("probe", Box::new(Probe(log.clone())));
        for (expression, expected) in [
            ("(and false (probe \"fail\" true))", false),
            ("(or true (probe \"fail\" true))", true),
            ("(and false (unknown_helper missing))", false),
            ("(or true (eq missing.path 1))", true),
        ] {
            assert_eq!(
                evaluate(&registry, expression, &Default::default()).unwrap(),
                expected
            );
        }
        assert!(log.lock().unwrap().is_empty());
        for expression in [
            "(and true (probe \"fail\" true))",
            "(or false (probe \"fail\" true))",
            "(and true (eq missing.path 1))",
            "(or false (unknown_helper missing))",
        ] {
            assert!(evaluate(&registry, expression, &Default::default()).is_err());
        }
        assert_eq!(*log.lock().unwrap(), ["fail", "fail"]);
    }

    #[test]
    fn helpers_run_once_in_left_to_right_order() {
        let mut registry = registry();
        let log = Arc::new(Mutex::new(Vec::new()));
        registry.register_helper("probe", Box::new(Probe(log.clone())));
        assert!(!evaluate(&registry,
            "(and (eq (probe \"first\" 1) 1) (or (probe \"second\" false) (probe \"third\" true)) (probe \"fourth\" false) (probe \"fail\" true))",
            &Default::default()).unwrap());
        assert_eq!(*log.lock().unwrap(), ["first", "second", "third", "fourth"]);
    }

    #[test]
    fn existing_conditions_match_handlebars_including_missing_and_truthiness() {
        let registry = registry();
        let data: serde_yaml::Mapping = serde_yaml::from_str(
            "zero: 0\nempty: ''\nlist: []\nobject: {}\nnull_value: null\ntext: hello\nnumber: 2\nflag: true\n"
        ).unwrap();
        for expression in [
            "true",
            "false",
            "zero",
            "zero includeZero=true",
            "empty",
            "list",
            "object",
            "null_value",
            "missing",
            "text",
            "number",
            "flag",
            "(eq number 2)",
            "(ne number 1)",
            "(gte number 2)",
            "(contains text \"ell\")",
            "(eq (trim \" hi \") \"hi\")",
            "(isdefined missing)",
            "(isdefined null_value)",
            "(eq missing 1)",
            "(unknown_helper text)",
        ] {
            let legacy = registry
                .render_template(
                    &format!("{{{{#if {expression} }}}}true{{{{else}}}}false{{{{/if}}}}"),
                    &data,
                )
                .map(|v| v == "true");
            let lazy = evaluate(&registry, expression, &data);
            assert_eq!(lazy.is_err(), legacy.is_err(), "{expression}");
            if let (Ok(lazy), Ok(legacy)) = (lazy, legacy) {
                assert_eq!(lazy, legacy, "{expression}");
            }
        }
    }

    #[test]
    fn logical_operands_use_handlebars_truthiness() {
        let registry = registry();
        let data: serde_yaml::Mapping = serde_yaml::from_str(
            "zero: 0\nempty: ''\nlist: []\nobject: {}\nnull_value: null\ntext: hello\n",
        )
        .unwrap();
        for field in [
            "zero",
            "empty",
            "list",
            "object",
            "null_value",
            "text",
            "missing",
        ] {
            let expected = registry
                .render_template(
                    &format!("{{{{#if {field}}}}}true{{{{else}}}}false{{{{/if}}}}"),
                    &data,
                )
                .unwrap()
                == "true";
            for op in ["and", "or"] {
                assert_eq!(
                    evaluate(&registry, &format!("({op} {field})"), &data).unwrap(),
                    expected
                );
            }
        }
    }

    #[test]
    fn malformed_conditions_and_logical_hash_arguments_fail() {
        let registry = registry();
        for expression in [
            "(and true",
            "(or false",
            "(and option=true)",
            "(or false option=true)",
        ] {
            assert!(
                evaluate(&registry, expression, &Default::default()).is_err(),
                "{expression}"
            );
        }
    }

    #[test]
    fn path_checks_are_called_only_for_evaluated_operands() {
        let registry = registry();
        let data: serde_yaml::Mapping = serde_yaml::from_str("first: 1\n").unwrap();
        let mut paths = Vec::new();
        let mut check = |path: &handlebars::Path| {
            paths.push(path.clone());
            if path == &handlebars::Path::parse("forbidden").unwrap() {
                return Err("forbidden path was accessed".into());
            }
            Ok(())
        };
        assert!(
            !evaluate_with_path_check(
                &registry,
                "(and (eq first 1) false (eq forbidden 2))",
                &data,
                &mut check
            )
            .unwrap()
        );
        assert!(
            evaluate_with_path_check(&registry, "(and true (eq forbidden 2))", &data, &mut check)
                .is_err()
        );
        assert_eq!(
            paths,
            [
                handlebars::Path::parse("first").unwrap(),
                handlebars::Path::parse("forbidden").unwrap()
            ]
        );
    }

    #[test]
    fn concurrent_conditions_keep_their_contexts_isolated() {
        let registry = Arc::new(registry());
        let threads: Vec<_> = (0..32)
            .map(|id| {
                let registry = registry.clone();
                std::thread::spawn(move || {
                    let data: serde_yaml::Mapping =
                        serde_yaml::from_str(&format!("id: {id}\n")).unwrap();
                    for _ in 0..50 {
                        assert!(
                            evaluate(
                                &registry,
                                &format!("(and (eq id {id}) (or false true))"),
                                &data
                            )
                            .unwrap()
                        );
                    }
                })
            })
            .collect();
        for thread in threads {
            thread.join().unwrap();
        }
    }
}
