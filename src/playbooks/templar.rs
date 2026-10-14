// Jetporch
// Copyright (C) 2023 - Michael DeHaan <michael@michaeldehaan.net> + contributors
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// long with this program.  If not, see <http://www.gnu.org/licenses/>.

use handlebars::Handlebars;
use once_cell::sync::Lazy;
use serde_yaml;

use crate::playbooks::t_helpers::register_helpers;

// templar contains low-level wrapping around handlebars.
// this is not used directly when evaluating templates and template
// expressions, for this, see handle/template.rs

static HANDLEBARS: Lazy<Handlebars> = Lazy::new(|| {
    let mut hb = Handlebars::new();
    // very important: we are not plugging variables into HTML, turn escaping off
    hb.register_escape_fn(handlebars::no_escape);
    hb.set_strict_mode(true);
    register_helpers(&mut hb);
    hb
});

// 'off' mode is used in a bit of a weird traversal/engine
// situation where we need to get access to some task parameters
// before templates are evaluated. You will notice there is no way
// to evaluate templates in unstrict mode. This is by design.

#[derive(PartialEq, Copy, Clone, Debug)]
pub enum TemplateMode {
    Strict,
    Off,
}

pub struct Templar {}

impl Default for Templar {
    fn default() -> Self {
        Self::new()
    }
}

impl Templar {
    pub fn new() -> Self {
        Self {}
    }

    // evaluate a string

    pub fn render(
        &self,
        template: &str,
        data: serde_yaml::Mapping,
        template_mode: TemplateMode,
    ) -> Result<String, String> {
        match template_mode {
            TemplateMode::Off => Ok(String::from("empty")),
            TemplateMode::Strict => {
                let mut rendered = template.to_string();
                for _ in 0..8 {
                    let next = HANDLEBARS
                        .render_template(&rendered, &data)
                        .map_err(|y| format!("Template error: {}", y))?;
                    if next == rendered || !next.contains("{{") {
                        return Ok(next);
                    }
                    rendered = next;
                }
                Err("Template error: exceeded recursive render limit".to_string())
            }
        }
    }

    /// Render a JSON-shaped value without stringifying whole-variable values.
    /// Object keys stay literal; mixed text keeps the existing text renderer.
    pub fn render_value(
        &self,
        value: &serde_json::Value,
        data: &serde_yaml::Mapping,
        template_mode: TemplateMode,
    ) -> Result<serde_json::Value, String> {
        if template_mode == TemplateMode::Off {
            return Ok(value.clone());
        }
        self.render_value_inner(value, data, 0)
    }

    fn render_value_inner(
        &self,
        value: &serde_json::Value,
        data: &serde_yaml::Mapping,
        expansions: usize,
    ) -> Result<serde_json::Value, String> {
        use handlebars::template::{Parameter, Template, TemplateElement};
        use handlebars::{Context, RenderContext};
        use serde_json::Value;
        match value {
            Value::String(text) if text.contains("{{") => {
                if expansions >= 8 {
                    return Err("exceeded typed template render limit".into());
                }
                let template = Template::compile(text).map_err(|e| e.to_string())?;
                if let [
                    TemplateElement::Expression(helper) | TemplateElement::HtmlExpression(helper),
                ] = template.elements.as_slice()
                    && helper.params.is_empty()
                    && helper.hash.is_empty()
                    && matches!(helper.name, Parameter::Path(_))
                {
                    let context = Context::wraps(data).map_err(|e| e.to_string())?;
                    let expanded = helper
                        .name
                        .expand(&HANDLEBARS, &context, &mut RenderContext::new(None))
                        .map_err(|e| e.to_string())?;
                    if expanded.is_value_missing() {
                        return Err("undefined typed template variable".into());
                    }
                    return self.render_value_inner(expanded.value(), data, expansions + 1);
                }
                let rendered = self.render(text, data.clone(), TemplateMode::Strict)?;
                // Mixed text always remains text, even when it resembles JSON.
                Ok(Value::String(rendered))
            }
            Value::Array(values) => values
                .iter()
                .map(|value| self.render_value_inner(value, data, expansions))
                .collect(),
            Value::Object(values) => values
                .iter()
                .map(|(key, value)| {
                    Ok((
                        key.clone(),
                        self.render_value_inner(value, data, expansions)?,
                    ))
                })
                .collect(),
            _ => Ok(value.clone()),
        }
    }

    /// Use Handlebars' path resolver, retaining the distinction between a
    /// missing path and an explicitly present JSON null.
    pub fn lookup_value(
        &self,
        path: &str,
        data: &serde_yaml::Mapping,
    ) -> Result<Option<serde_json::Value>, String> {
        let path = handlebars::template::Parameter::Path(self.parse_value_path(path)?);
        let context = handlebars::Context::wraps(data).map_err(|e| e.to_string())?;
        let value = path
            .expand(
                &HANDLEBARS,
                &context,
                &mut handlebars::RenderContext::new(None),
            )
            .map_err(|e| e.to_string())?;
        Ok((!value.is_value_missing()).then(|| value.value().clone()))
    }

    /// Require one complete path, excluding helper calls and trailing tokens.
    pub fn parse_value_path(&self, path: &str) -> Result<handlebars::Path, String> {
        use handlebars::template::{Parameter, Template, TemplateElement};
        let template =
            Template::compile(&format!("{{{{ {path} }}}}")).map_err(|e| e.to_string())?;
        if let [TemplateElement::Expression(helper)] = template.elements.as_slice()
            && helper.params.is_empty()
            && helper.hash.is_empty()
            && let Parameter::Path(path) = &helper.name
        {
            return Ok(path.clone());
        }
        Err("expected a single value path".into())
    }

    // used for with/cond and also in the shell module

    pub fn test_condition(
        &self,
        expr: &str,
        data: serde_yaml::Mapping,
        template_mode: TemplateMode,
    ) -> Result<bool, String> {
        if template_mode == TemplateMode::Off {
            /* this is only used to get back the raw 'items' collection inside the task FSM */
            return Ok(true);
        }
        let result = super::conditions::evaluate(&HANDLEBARS, expr, &data);
        match result {
            Ok(value) => Ok(value),
            Err(y) => {
                if y.find("Couldn't read parameter").is_some() {
                    Err(format!(
                        "failed to parse conditional: {}: one or more parameters may be undefined",
                        expr
                    ))
                } else {
                    Err(format!("failed to parse conditional: {}: {}", expr, y))
                }
            }
        }
    }

    /// Evaluate with an invocation-local path validator. The callback is never
    /// called for an operand skipped by a logical short circuit.
    pub fn test_condition_with_path_check(
        &self,
        expr: &str,
        data: serde_yaml::Mapping,
        path_check: &mut impl FnMut(&handlebars::Path) -> Result<(), String>,
    ) -> Result<bool, String> {
        super::conditions::evaluate_with_path_check(&HANDLEBARS, expr, &data, path_check)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_resolves_nested_templates() {
        let templar = Templar::new();
        let mut data = serde_yaml::Mapping::new();
        data.insert(
            serde_yaml::Value::String("JET_USER_HOME".to_string()),
            serde_yaml::Value::String("/home/wings".to_string()),
        );
        data.insert(
            serde_yaml::Value::String("codex_desktop_remote_root".to_string()),
            serde_yaml::Value::String(
                "{{ JET_USER_HOME }}/projects/riffenvironment/projects/codex-desktop-linux"
                    .to_string(),
            ),
        );

        let result = templar
            .render(
                &"{{ codex_desktop_remote_root }}/codex-app".to_string(),
                data,
                TemplateMode::Strict,
            )
            .unwrap();

        assert_eq!(
            result,
            "/home/wings/projects/riffenvironment/projects/codex-desktop-linux/codex-app"
        );
    }
}

#[cfg(test)]
mod typed_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn whole_variable_templates_keep_types_and_mixed_text_stays_text() {
        let data = serde_yaml::from_str("ttl: 3600\nflag: true\nempty: null\ntext: '3600'\nhandlers: [{type: memory}, {type: etcd}]\nobject: {enabled: true}\n").unwrap();
        let expected = json!({"ttl":"{{ ttl }}", "flag":"{{ flag }}", "empty":"{{ empty }}", "text":"{{ text }}", "handlers":"{{ handlers }}", "object":"{{ object }}", "mixed":"ttl={{ ttl }}", "json_text":"{\"ttl\":{{ ttl }}}"});
        let rendered = Templar::new()
            .render_value(&expected, &data, TemplateMode::Strict)
            .unwrap();
        assert_eq!(
            rendered,
            json!({"ttl":3600, "flag":true, "empty":null, "text":"3600", "handlers":[{"type":"memory"},{"type":"etcd"}], "object":{"enabled":true}, "mixed":"ttl=3600", "json_text":"{\"ttl\":3600}"})
        );
    }

    #[test]
    fn typed_templates_resolve_aliases_and_nested_values_with_a_recursion_bound() {
        let data = serde_yaml::from_str("ttl: 3600\nalias: '{{ ttl }}'\nhandlers: [{type: '{{ handler_type }}'}]\nhandler_type: memory\ncycle: '{{ cycle }}'\n").unwrap();
        let templar = Templar::new();
        assert_eq!(
            templar
                .render_value(
                    &json!({"ttl":"{{ alias }}", "handlers":"{{ handlers }}"}),
                    &data,
                    TemplateMode::Strict
                )
                .unwrap(),
            json!({"ttl":3600,"handlers":[{"type":"memory"}]})
        );
        assert!(
            templar
                .render_value(&json!("{{ cycle }}"), &data, TemplateMode::Strict)
                .is_err()
        );
        assert!(
            templar
                .render_value(&json!("{{ missing }}"), &data, TemplateMode::Strict)
                .is_err()
        );
        assert_eq!(
            templar
                .render_value(&json!("{{ missing }}"), &data, TemplateMode::Off)
                .unwrap(),
            json!("{{ missing }}")
        );
    }

    #[test]
    fn path_lookup_preserves_missing_null_and_existing_array_path_syntax() {
        let data = serde_yaml::from_str(
            "response:\n  json:\n    zones: [{name: example.com, hook: null}]\n",
        )
        .unwrap();
        let templar = Templar::new();
        assert_eq!(
            templar
                .lookup_value("response.json.zones.0.name", &data)
                .unwrap(),
            Some(json!("example.com"))
        );
        assert_eq!(
            templar
                .lookup_value("response.json.zones.0.hook", &data)
                .unwrap(),
            Some(json!(null))
        );
        assert!(
            templar
                .lookup_value("response.json.zones.1", &data)
                .unwrap()
                .is_none()
        );
        for path in [
            "(eq response.json true)",
            "response.json extra",
            "",
            "{{ response.json }}",
        ] {
            assert!(templar.lookup_value(path, &data).is_err(), "{path}");
        }
    }
}
