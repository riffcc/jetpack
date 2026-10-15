// Copyright (C) Riff Labs <team@riff.cc>
// SPDX-License-Identifier: GPL-3.0-or-later

//! Invocation-local rendering metadata for outputs that plan did not produce.
//! No saved result (including an empty result) is invented in host variables.
use serde_yaml::{Mapping, Value};
use std::collections::HashSet;

const METADATA: &str = "__jetpack_internal_plan_unavailable";
const ERROR: &str = "JETPACK_INTERNAL_DEFERRED_PLAN_VALUE";
pub(crate) const MESSAGE: &str = "depends on output unavailable until apply";
pub(crate) fn is_deferred_error(message: &str) -> bool {
    message.contains(ERROR)
}

pub(crate) fn annotate(data: &mut Mapping, unavailable: &HashSet<String>) {
    if unavailable.is_empty() {
        return;
    }
    for key in unavailable {
        data.remove(Value::String(key.clone()));
    }
    let mut keys: Vec<_> = unavailable.iter().cloned().map(Value::String).collect();
    keys.sort_by(|a, b| a.as_str().cmp(&b.as_str()));
    data.insert(METADATA.into(), Value::Sequence(keys));
}
pub(crate) fn clear(data: &mut Mapping, key: &str) {
    if let Some(Value::Sequence(keys)) = data.get_mut(Value::String(METADATA.into())) {
        keys.retain(|v| v.as_str() != Some(key));
    }
}
fn unavailable(data: &Mapping, key: &str) -> bool {
    data.get(Value::String(METADATA.into()))
        .and_then(Value::as_sequence)
        .is_some_and(|keys| keys.iter().any(|v| v.as_str() == Some(key)))
}
fn check_variable(key: &str, data: &Mapping, seen: &mut HashSet<String>) -> Result<(), String> {
    if unavailable(data, key) {
        return Err(ERROR.into());
    }
    if !seen.insert(key.into()) {
        return Ok(());
    }
    if let Some(value) = data.get(Value::String(key.into())) {
        for dependency in super::template_refs::referenced_variables_in_value(value) {
            check_variable(&dependency, data, seen)?;
        }
    }
    Ok(())
}
pub(crate) fn check_path(path: &handlebars::Path, data: &Mapping) -> Result<(), String> {
    use handlebars::{Path, PathSeg};
    if !data.contains_key(Value::String(METADATA.into())) {
        return Ok(());
    }
    match path {
        Path::Relative((segments, _)) => {
            if let Some(root) = segments.iter().find_map(|s| match s {
                PathSeg::Named(n) => Some(n.as_str()),
                _ => None,
            }) {
                check_variable(root, data, &mut HashSet::new())?;
            }
        }
        Path::Local((0, name, raw)) if name == "root" => {
            if let Some(rest) = raw
                .strip_prefix("@root.")
                .or_else(|| raw.strip_prefix("@root/"))
                && let Ok(path) = Path::parse(rest)
            {
                check_path(&path, data)?;
            }
        }
        _ => {}
    }
    Ok(())
}
pub(crate) fn check_template(template: &str, data: &Mapping) -> Result<(), String> {
    if !data.contains_key(Value::String(METADATA.into())) {
        return Ok(());
    }
    let normalized = template.replace("@root.", "").replace("@root/", "");
    for key in super::template_refs::referenced_variables(&normalized)? {
        check_variable(&key, data, &mut HashSet::new())?;
    }
    Ok(())
}

/// Handlebars resolves lookup keys at render time. Guard the selected key,
/// preserving its lookup behavior without evaluating arguments twice.
pub(crate) fn register_lookup(registry: &mut handlebars::Handlebars<'_>) {
    struct Lookup;
    impl handlebars::HelperDef for Lookup {
        fn call_inner<'reg: 'rc, 'rc>(
            &self,
            helper: &handlebars::Helper<'rc>,
            registry: &'reg handlebars::Handlebars<'reg>,
            _context: &'rc handlebars::Context,
            _rc: &mut handlebars::RenderContext<'reg, 'rc>,
        ) -> Result<handlebars::ScopedJson<'rc>, handlebars::RenderError> {
            if let Some(base) = helper.param(0).map(|p| p.value())
                && let Some(key) = helper.param(1).and_then(|p| p.value().as_str())
                && base
                    .get(METADATA)
                    .and_then(serde_json::Value::as_array)
                    .is_some_and(|keys| keys.iter().any(|v| v.as_str() == Some(key)))
            {
                return Err(handlebars::RenderErrorReason::Other(ERROR.into()).into());
            }
            // Handlebars does not expose its built-in HelperDef. Preserve its
            // public lookup contract on the already-expanded arguments.
            let collection =
                helper
                    .param(0)
                    .ok_or(handlebars::RenderErrorReason::ParamNotFoundForIndex(
                        "lookup", 0,
                    ))?;
            let index =
                helper
                    .param(1)
                    .ok_or(handlebars::RenderErrorReason::ParamNotFoundForIndex(
                        "lookup", 1,
                    ))?;
            let value = match collection.value() {
                serde_json::Value::Array(values) => {
                    index.value().as_u64().and_then(|i| values.get(i as usize))
                }
                serde_json::Value::Object(values) => {
                    index.value().as_str().and_then(|key| values.get(key))
                }
                _ => None,
            };
            if registry.strict_mode() && value.is_none() {
                return Err(handlebars::RenderError::strict_error(None));
            }
            Ok(value.unwrap_or(&serde_json::Value::Null).clone().into())
        }
    }
    registry.register_helper("lookup", Box::new(Lookup));
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lookup_guard_preserves_native_lookup_results_errors_and_types() {
        let mut native = handlebars::Handlebars::new();
        native.set_strict_mode(true);
        native.register_escape_fn(handlebars::no_escape);
        let mut guarded = native.clone();
        register_lookup(&mut guarded);
        let values = serde_json::json!({"object":{"text":"<&>","number":42,"null":null,"nested":{"x":true}},"array":[false,"text"]});
        for expression in [
            "{{lookup object \"text\"}}",
            "{{lookup object \"number\"}}",
            "{{lookup object \"null\"}}",
            "{{lookup object \"nested\"}}",
            "{{lookup array 0}}",
            "{{lookup array 1}}",
            "{{lookup array 9}}",
            "{{lookup object \"missing\"}}",
            "{{lookup null 1}}",
            "{{lookup}}",
            "{{lookup array}}",
        ] {
            let expected = native
                .render_template(expression, &values)
                .map_err(|e| e.to_string());
            let actual = guarded
                .render_template(expression, &values)
                .map_err(|e| e.to_string());
            assert_eq!(actual, expected, "{expression}");
        }
    }
    #[test]
    fn unknown_read_metadata_is_local_and_does_not_replace_actual_saved_data() {
        let mut data: Mapping =
            serde_yaml::from_str("saved: {out: old}\nreal_empty: {out: ''}").unwrap();
        let original = data.clone();
        annotate(&mut data, &HashSet::from(["saved".into()]));
        assert!(!data.contains_key(Value::String("saved".into())));
        assert!(check_template("{{ saved.out }}", &data).is_err());
        assert!(check_template("{{ real_empty.out }}", &data).is_ok());
        assert!(original.contains_key(Value::String("saved".into())));
        clear(&mut data, "saved");
        assert!(check_template("{{ saved.out }}", &data).is_ok());
    }
}
