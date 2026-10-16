// Copyright (C) Riff Labs <team@riff.cc>
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::tasks::*;
use serde::Deserialize;
use std::sync::Arc;

/// Modules opt individual fields into scalar-or-list resource syntax.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum ResourceInput {
    Scalar(String),
    List(Vec<String>),
}

impl Default for ResourceInput {
    fn default() -> Self {
        Self::Scalar(String::new())
    }
}

impl From<String> for ResourceInput {
    fn from(value: String) -> Self {
        Self::Scalar(value)
    }
}

impl PartialEq<&str> for ResourceInput {
    fn eq(&self, other: &&str) -> bool {
        matches!(self, Self::Scalar(value) if value == other)
    }
}

impl ResourceInput {
    pub fn is_list(&self) -> bool {
        matches!(self, Self::List(_))
    }

    /// Reuse normal field templating and validation, including indexed errors.
    pub fn template_no_spaces(
        &self,
        handle: &Arc<TaskHandle>,
        request: &Arc<TaskRequest>,
        mode: TemplateMode,
        field: &str,
    ) -> Result<Vec<String>, Arc<TaskResponse>> {
        let inputs: Vec<&str> = match self {
            Self::Scalar(value) => vec![value],
            Self::List(values) => values.iter().map(String::as_str).collect(),
        };
        if inputs.is_empty() {
            if mode == TemplateMode::Off {
                return Ok(vec![String::new()]);
            }
            return Err(handle.response.is_failed(
                request,
                &format!("field {field}: resource list must not be empty"),
            ));
        }
        inputs
            .iter()
            .enumerate()
            .map(|(index, value)| {
                let name = if self.is_list() {
                    format!("{field}[{index}]")
                } else {
                    field.into()
                };
                let rendered = handle
                    .template
                    .string_no_spaces(request, mode, &name, value)?;
                if self.is_list() && mode != TemplateMode::Off && (rendered.is_empty() || rendered.starts_with('-') || rendered.chars().any(char::is_whitespace)) {
                    return Err(handle.response.is_failed(
                        request,
                        &format!("field {name}: resource names must be nonempty, contain no whitespace, and must not start with '-'"),
                    ));
                }
                Ok(rendered)
            })
            .collect()
    }
}

/// The identity/order rule used by native resource batching and its labels.
pub fn retain_unique_resources<T>(resources: &mut Vec<T>, name: impl Fn(&T) -> String) {
    let mut seen = std::collections::HashSet::new();
    resources.retain(|resource| seen.insert(name(resource)));
}
