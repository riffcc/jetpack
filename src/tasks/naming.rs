// Copyright (C) Riff Labs <team@riff.cc>
// SPDX-License-Identifier: GPL-3.0-or-later

//! Read-only descriptions of declarations. No task evaluation, template
//! rendering, helpers, I/O, or mutation of execution variables happens here.

use crate::tasks::logic::ItemsInput;
use crate::tasks::resources::{ResourceInput, retain_unique_resources};
use handlebars::template::{Parameter, Template, TemplateElement};
use serde_yaml::{Mapping, Value};

pub const MAX_LABEL_CHARS: usize = 120;
const PREVIEW_RESOURCES: usize = 4;

pub struct NamingContext<'a> {
    variables: &'a Mapping,
    items: Option<&'a ItemsInput>,
    context: Option<handlebars::Context>,
}

impl<'a> NamingContext<'a> {
    pub fn new(variables: &'a Mapping, items: Option<&'a ItemsInput>) -> Self {
        let mut available = variables.clone();
        available.remove(Value::String("item".into()));
        Self {
            variables,
            items,
            context: handlebars::Context::wraps(&available).ok(),
        }
    }

    /// Only plain variable paths may read already available values. Complex
    /// templates and aliases are deliberately left unresolved, never rendered.
    fn read(&self, source: &str, item: Option<&Value>) -> Option<Value> {
        if !source.contains("{{") {
            return Some(Value::String(source.into()));
        }
        let template = Template::compile(source).ok()?;
        let [TemplateElement::Expression(helper) | TemplateElement::HtmlExpression(helper)] =
            template.elements.as_slice()
        else {
            return None;
        };
        if !helper.params.is_empty()
            || !helper.hash.is_empty()
            || !matches!(helper.name, Parameter::Path(_))
        {
            return None;
        }
        // Reuse Handlebars path resolution without invoking helpers or
        // serializing the full variable snapshot for every resource in a loop.
        let path = helper.name.as_name()?;
        let lookup = |context: &handlebars::Context| {
            let value = handlebars::RenderContext::new(None)
                .evaluate(context, path)
                .ok()?;
            if value.is_missing() {
                None
            } else {
                serde_yaml::to_value(value.as_json()).ok()
            }
        };
        if let Some(value) = self.context.as_ref().and_then(lookup) {
            return Some(value);
        }
        // Item is an isolated namespace, never a leftover fact from another
        // declaration or an update to the execution host's variables.
        let mut local = Mapping::new();
        local.insert("item".into(), item?.clone());
        lookup(&handlebars::Context::wraps(&local).ok()?)
    }

    fn scalar(value: Value) -> Option<String> {
        match value {
            Value::String(value) if !value.contains("{{") && !value.trim().is_empty() => {
                Some(value.trim().into())
            }
            Value::Number(value) => Some(value.to_string()),
            Value::Bool(value) => Some(value.to_string()),
            _ => None,
        }
    }

    fn iteration_values(&self) -> Option<Vec<Option<Value>>> {
        match self.items {
            None => Some(vec![None]),
            Some(ItemsInput::ItemsList(items)) => items
                .iter()
                .map(|value| {
                    // Literal item lists are string inputs in the execution engine.
                    Self::scalar(self.read(value, None)?).map(|value| Some(Value::String(value)))
                })
                .collect(),
            Some(ItemsInput::ItemsString(name)) => {
                let Value::Sequence(items) = self.variables.get(Value::String(name.clone()))?
                else {
                    return None;
                };
                items
                    .iter()
                    .map(|value| match value {
                        Value::String(value) => Self::scalar(self.read(value, None)?)
                            .map(|value| Some(Value::String(value))),
                        other => Some(Some(other.clone())),
                    })
                    .collect()
            }
        }
    }

    /// Missing flags use the module's default; unresolved flags are unknown.
    pub fn flag(&self, value: &Option<String>, default: bool) -> Option<bool> {
        match value {
            None => Some(default),
            Some(value) => Self::scalar(self.read(value, None)?)?.parse().ok(),
        }
    }

    pub fn resource(&self, operation: &str, source: &str, fallback: &str) -> String {
        self.resources(operation, &ResourceInput::Scalar(source.into()), fallback)
    }

    pub fn resources(&self, operation: &str, input: &ResourceInput, fallback: &str) -> String {
        let known = (|| {
            let items = self.iteration_values()?;
            if items.is_empty() {
                return None;
            }
            let inputs: Vec<&str> = match input {
                ResourceInput::Scalar(value) => vec![value],
                ResourceInput::List(values) => values.iter().map(String::as_str).collect(),
            };
            let mut all = Vec::new();
            for item in &items {
                let mut names: Vec<String> = inputs
                    .iter()
                    .map(|source| Self::scalar(self.read(source, item.as_ref())?))
                    .collect::<Option<_>>()?;
                if input.is_list() {
                    // Share the package batch's identity/order rule. Loops
                    // retain their own iteration boundaries and repetitions.
                    retain_unique_resources(&mut names, String::clone);
                }
                all.extend(names);
            }
            if all.is_empty() { None } else { Some(all) }
        })();
        match known {
            Some(names) => {
                let preview = names
                    .iter()
                    .take(PREVIEW_RESOURCES)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ");
                let suffix = if names.len() > PREVIEW_RESOURCES {
                    format!(" (+{} more)", names.len() - PREVIEW_RESOURCES)
                } else {
                    String::new()
                };
                format!("{operation} {preview}{suffix}")
            }
            None => fallback.into(),
        }
    }

    pub fn packages(
        &self,
        input: &ResourceInput,
        remove: &Option<String>,
        update: &Option<String>,
    ) -> String {
        let operation = match (self.flag(remove, false), self.flag(update, false)) {
            (Some(true), _) => "Remove",
            (Some(false), Some(true)) => "Upgrade",
            (Some(false), Some(false)) => "Install",
            _ => "Manage",
        };
        self.resources(operation, input, &format!("{operation} packages"))
    }

    pub fn removable(
        &self,
        operation: &str,
        remove: &Option<String>,
        source: &str,
        fallback: &str,
    ) -> String {
        let operation = match self.flag(remove, false) {
            Some(true) => "Remove",
            Some(false) => operation,
            None => "Manage",
        };
        let verb = operation.split_whitespace().next().unwrap_or(operation);
        self.resource(operation, source, &format!("{verb} {fallback}"))
    }
}

/// Normalize whitespace and control characters without
/// attempting to identify credentials or altering the source used for execution.
pub fn bounded(label: &str) -> String {
    let clean: String = label
        .chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect();
    let clean = clean.split_whitespace().collect::<Vec<_>>().join(" ");
    if clean.chars().count() <= MAX_LABEL_CHARS {
        clean
    } else {
        format!(
            "{}…",
            clean.chars().take(MAX_LABEL_CHARS - 1).collect::<String>()
        )
    }
}

/// Only equal top-level values from all hosts are eligible for resolution.
/// Heterogeneous values use a common generic label, never an arbitrary host.
pub fn common_variables(values: impl IntoIterator<Item = Mapping>) -> Mapping {
    let mut values = values.into_iter();
    let Some(mut common) = values.next() else {
        return Mapping::new();
    };
    for variables in values {
        common.retain(|key, value| variables.get(key) == Some(value));
    }
    common.remove(Value::String("item".into()));
    common
}

/// HTTP descriptions never interpolate endpoints or display credentials,
/// query strings, fragments, request headers, or request bodies.
pub fn http_label(method: Option<&str>, url: &str) -> String {
    let method = method.unwrap_or("GET").trim().to_ascii_uppercase();
    let operation = if [
        "GET", "HEAD", "POST", "PUT", "PATCH", "DELETE", "OPTIONS", "TRACE", "CONNECT",
    ]
    .contains(&method.as_str())
    {
        format!("HTTP {method}")
    } else {
        "HTTP request".into()
    };
    if !url.contains("{{")
        && let Ok(url) = reqwest::Url::parse(url)
        && matches!(url.scheme(), "http" | "https")
        && let Some(host) = url.host_str()
    {
        return format!("{operation} {host}");
    }
    operation
}

#[cfg(test)]
mod tests;

/// Conservative output metadata, not execution or prediction of future values.
/// Unknown modules cannot leave potentially stale values eligible for display.
pub enum NameVariableWrites {
    None,
    Keys(Vec<String>),
    Unknown,
}

impl NameVariableWrites {
    pub fn saved(name: Option<&str>) -> Self {
        match name {
            None => Self::None,
            Some(name) if name.contains("{{") => Self::Unknown,
            Some(name) => Self::Keys(vec![name.trim().into()]),
        }
    }

    pub fn forget(self, variables: &mut Mapping) {
        match self {
            Self::None => {}
            Self::Keys(keys) => {
                for key in keys {
                    variables.remove(Value::String(key));
                }
            }
            Self::Unknown => variables.clear(),
        }
    }
}
