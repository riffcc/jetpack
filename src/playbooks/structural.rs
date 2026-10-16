// Copyright (C) Riff Labs <team@riff.cc>
// SPDX-License-Identifier: GPL-3.0-or-later

//! Structural matching of typed values, independent of any response transport.

use crate::playbooks::templar::{Templar, TemplateMode};
use serde::{Deserialize, Deserializer};
use serde_json::Value;

#[derive(Clone, Copy)]
pub enum MatchMode {
    Contains,
    ContainsExactly,
}

#[derive(Clone)]
pub struct StructuralMatch {
    pub path: String,
    pub expected: Value,
    pub mode: MatchMode,
}

// Distinguish an absent operator from an explicit null expectation.
fn present<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<Value>, D::Error> {
    Value::deserialize(deserializer).map(Some)
}

impl<'de> Deserialize<'de> for StructuralMatch {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Input {
            path: String,
            #[serde(default, deserialize_with = "present")]
            contains: Option<Value>,
            #[serde(default, deserialize_with = "present")]
            contains_exactly: Option<Value>,
        }
        let input = Input::deserialize(deserializer)?;
        let (expected, mode) = match (input.contains, input.contains_exactly) {
            (Some(value), None) => (value, MatchMode::Contains),
            (None, Some(value)) => (value, MatchMode::ContainsExactly),
            _ => {
                return Err(serde::de::Error::custom(
                    "specify exactly one structural matching operator",
                ));
            }
        };
        Ok(Self {
            path: input.path,
            expected,
            mode,
        })
    }
}

impl StructuralMatch {
    pub fn test(&self, data: &serde_yaml::Mapping) -> Result<bool, String> {
        let templar = Templar::new();
        let expected = templar.render_value(&self.expected, data, TemplateMode::Strict)?;
        let actual = templar.lookup_value(&self.path, data)?;
        Ok(actual.is_some_and(|value| matches_value(&value, &expected, self.mode)))
    }
}

pub fn matches_value(actual: &Value, expected: &Value, mode: MatchMode) -> bool {
    match (actual, expected) {
        (Value::Object(actual), Value::Object(expected)) => expected.iter().all(|(key, value)| {
            actual
                .get(key)
                .is_some_and(|actual| matches_value(actual, value, mode))
        }),
        (Value::Array(actual), Value::Array(expected)) => match mode {
            MatchMode::ContainsExactly => {
                actual.len() == expected.len()
                    && actual
                        .iter()
                        .zip(expected)
                        .all(|(actual, expected)| matches_value(actual, expected, mode))
            }
            MatchMode::Contains => contains_array(actual, expected),
        },
        _ => actual == expected,
    }
}

fn contains_array(actual: &[Value], expected: &[Value]) -> bool {
    if expected.len() > actual.len() {
        return false;
    }
    // Build a bipartite match rather than greedily consuming entries. A broad
    // object pattern may need to move aside for a later, more specific one.
    let candidates: Vec<Vec<usize>> = expected
        .iter()
        .map(|expected| {
            actual
                .iter()
                .enumerate()
                .filter_map(|(index, actual)| {
                    matches_value(actual, expected, MatchMode::Contains).then_some(index)
                })
                .collect()
        })
        .collect();
    let mut assignments = vec![None; actual.len()];
    for index in 0..expected.len() {
        let mut visited = vec![false; actual.len()];
        if !assign(index, &candidates, &mut assignments, &mut visited) {
            return false;
        }
    }
    true
}

fn assign(
    expected: usize,
    candidates: &[Vec<usize>],
    assignments: &mut [Option<usize>],
    visited: &mut [bool],
) -> bool {
    for &actual in &candidates[expected] {
        if visited[actual] {
            continue;
        }
        visited[actual] = true;
        if assignments[actual]
            .is_none_or(|previous| assign(previous, candidates, assignments, visited))
        {
            assignments[actual] = Some(expected);
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests;
