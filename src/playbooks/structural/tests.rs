// Copyright (C) Riff Labs <team@riff.cc>
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;
use serde_json::json;

#[test]
fn objects_are_recursive_subsets_and_scalars_keep_their_types() {
    for mode in [MatchMode::Contains, MatchMode::ContainsExactly] {
        assert!(matches_value(
            &json!({"a": {"b": true, "extra": 5}, "metadata": 1}),
            &json!({"a": {"b": true}}),
            mode
        ));
        assert!(!matches_value(&json!({}), &json!({"a": null}), mode));
        assert!(matches_value(
            &json!({"a": null}),
            &json!({"a": null}),
            mode
        ));
        for (actual, expected) in [
            (json!(3600), json!("3600")),
            (json!(true), json!("true")),
            (json!(false), json!(null)),
            (json!([]), json!({})),
        ] {
            assert!(!matches_value(&actual, &expected, mode));
        }
    }
}

#[test]
fn exact_arrays_require_length_and_order_but_allow_object_metadata() {
    let mode = MatchMode::ContainsExactly;
    assert!(matches_value(
        &json!([{ "name": "one", "id": 9 }, { "name": "two" }]),
        &json!([{ "name": "one" }, { "name": "two" }]),
        mode
    ));
    assert!(!matches_value(&json!([1, 2]), &json!([2, 1]), mode));
    assert!(!matches_value(&json!([1, 1]), &json!([1]), mode));
    assert!(!matches_value(&json!([1]), &json!([1, 1]), mode));
    assert!(matches_value(&json!([]), &json!([]), mode));
    assert!(!matches_value(&json!([1]), &json!([]), mode));
    assert!(!matches_value(
        &json!({"nested": [1, 2]}),
        &json!({"nested": [1]}),
        mode
    ));
}

#[test]
fn contains_arrays_are_unordered_and_use_distinct_entries() {
    let mode = MatchMode::Contains;
    assert!(matches_value(&json!([1, 2, 3]), &json!([3, 1]), mode));
    assert!(!matches_value(&json!([1, 2]), &json!([1, 1]), mode));
    assert!(matches_value(&json!([1, 1, 2]), &json!([1, 1]), mode));
    assert!(matches_value(&json!([1]), &json!([]), mode));
    assert!(!matches_value(&json!([]), &json!([1]), mode));
    // A greedy algorithm would consume the specific object for the broad
    // pattern and fail; a valid one-to-one assignment exists here.
    assert!(matches_value(
        &json!([{ "a": 1 }, { "b": 2 }]),
        &json!([{}, { "a": 1 }]),
        mode
    ));
    assert!(!matches_value(
        &json!([{ "a": 1 }, { "b": 2 }]),
        &json!([{ "a": 1 }, { "a": 1 }]),
        mode
    ));
    assert!(matches_value(
        &json!({"nested": [1, 2]}),
        &json!({"nested": [2]}),
        mode
    ));
}

#[test]
fn matcher_requires_path_and_exactly_one_explicit_operator() {
    for input in [
        "path: response.json\n",
        "contains: {}\n",
        "path: response.json\ncontains: {}\ncontains_exactly: {}\n",
        "path: response.json\nequals: {}\n",
        "path: response.json\ncontains: {}\nextra: secret\n",
    ] {
        assert!(
            serde_yaml::from_str::<StructuralMatch>(input).is_err(),
            "{input}"
        );
    }
    for operator in ["contains", "contains_exactly"] {
        let matcher: StructuralMatch =
            serde_yaml::from_str(&format!("path: value\n{operator}: null\n")).unwrap();
        assert!(matcher.expected.is_null());
        let null: serde_yaml::Mapping = serde_yaml::from_str("value: null").unwrap();
        assert!(matcher.test(&null).unwrap());
        assert!(!matcher.test(&serde_yaml::Mapping::new()).unwrap());
    }
}

#[test]
fn unordered_array_assignment_agrees_with_exhaustive_search() {
    fn oracle(actual: &[Value], expected: &[Value], used: &mut [bool], index: usize) -> bool {
        if index == expected.len() {
            return true;
        }
        for candidate in 0..actual.len() {
            if !used[candidate]
                && matches_value(&actual[candidate], &expected[index], MatchMode::Contains)
            {
                used[candidate] = true;
                if oracle(actual, expected, used, index + 1) {
                    return true;
                }
                used[candidate] = false;
            }
        }
        false
    }
    let values = [
        json!(1),
        json!(2),
        json!({}),
        json!({"a":1}),
        json!({"b":2}),
    ];
    let mut arrays = vec![vec![]];
    for length in 1..=3 {
        for encoded in 0..5usize.pow(length) {
            let mut code = encoded;
            let array = (0..length)
                .map(|_| {
                    let value = values[code % 5].clone();
                    code /= 5;
                    value
                })
                .collect::<Vec<_>>();
            arrays.push(array);
        }
    }
    for actual in &arrays {
        for expected in &arrays {
            assert_eq!(
                contains_array(actual, expected),
                oracle(actual, expected, &mut vec![false; actual.len()], 0),
                "actual={actual:?}, expected={expected:?}"
            );
        }
    }
}
