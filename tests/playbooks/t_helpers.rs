use jetpack::playbooks::t_helpers::*;
use handlebars::{Handlebars, no_escape, Helper, RenderContext, Context, HelperDef, ScopedJson, JsonValue, RenderError};
use serde_json::json;
use std::error::Error;
use std::collections::HashMap;

pub fn new_handlebars<'reg>() -> Handlebars<'reg> {
    let mut handlebars = Handlebars::new();
    handlebars.set_strict_mode(true);
    handlebars.register_escape_fn(no_escape); //html escaping is the default and cause issue
    register_helpers(&mut handlebars);
    handlebars
}

#[macro_export]
macro_rules! assert_renders {
    ($($arg:expr),+$(,)?) => {{
        use std::collections::HashMap;
        let vs: HashMap<String, String> = HashMap::new();
        let mut handlebars = new_handlebars();
        $({
            let sample: (&str, &str) = $arg;
            handlebars.register_template_string(&sample.0, &sample.0).expect("register_template_string");
            assert_eq!(handlebars.render(&sample.0, &vs).expect("render"), sample.1.to_owned());
        })*
        Ok(())
    }}
}

fn test_condition(condition: &str, expected: bool) {
    let handlebars = new_handlebars();
    let result = handlebars
        .render_template(
            &format!(
                "{{{{#if {condition}}}}}lorem{{{{else}}}}ipsum{{{{/if}}}}",
                condition = condition
            ),
            &json!({}),
        )
        .unwrap();
    assert_eq!(&result, if expected { "lorem" } else { "ipsum" }, "testing condition: {}", condition);
}

#[test]
fn test_register_string_helpers() -> Result<(), Box<dyn Error>> {
    assert_renders![
        (r##"{{ to_lower_case "Hello foo-bars" }}"##, r##"hello foo-bars"##),
        (r##"{{ to_upper_case "Hello foo-bars" }}"##, r##"HELLO FOO-BARS"##)
    ]
}

#[test]
fn test_helper_trim() -> Result<(), Box<dyn Error>> {
    assert_renders![
        (r##"{{ trim "foo" }}"##, r##"foo"##),
        (r##"{{ trim "  foo" }}"##, r##"foo"##),
        (r##"{{ trim "foo  " }}"##, r##"foo"##),
        (r##"{{ trim " foo " }}"##, r##"foo"##)
    ]
}

#[test]
fn test_helper_trim_start() -> Result<(), Box<dyn Error>> {
    assert_renders![
        (r##"{{ trim_start "foo" }}"##, r##"foo"##),
        (r##"{{ trim_start "  foo" }}"##, r##"foo"##),
        (r##"{{ trim_start "foo  " }}"##, r##"foo  "##),
        (r##"{{ trim_start " foo " }}"##, r##"foo "##)
    ]
}

#[test]
fn test_helper_trim_end() -> Result<(), Box<dyn Error>> {
    assert_renders![
        (r##"{{ trim_end "foo" }}"##, r##"foo"##),
        (r##"{{ trim_end "foo  " }}"##, r##"foo"##),
        (r##"{{ trim_end "  foo" }}"##, r##"  foo"##),
        (r##"{{ trim_end " foo " }}"##, r##" foo"##)
    ]
}

#[test]
fn test_helper_contains() -> Result<(), Box<dyn Error>> {
    test_condition(r#"( contains "foo" "bar" )"#, false);
    test_condition(r#"( contains "foo" "foo" )"#, true);
    test_condition(r#"( contains "barfoobar" "foo" )"#, true);
    test_condition(r#"( contains "foo" "barfoobar" )"#, false);

    Ok(())
}

#[test]
fn test_helper_starts_with() -> Result<(), Box<dyn Error>> {
    test_condition(r#"( starts_with "foo" "bar" )"#, false);
    test_condition(r#"( starts_with "foobar" "foo" )"#, true);
    test_condition(r#"( starts_with "foo" "foobar" )"#, false);

    Ok(())
}

#[test]
fn test_helper_ends_with() -> Result<(), Box<dyn Error>> {
    test_condition(r#"( ends_with "foo" "bar" )"#, false);
    test_condition(r#"( ends_with "foobar" "bar" )"#, true);
    test_condition(r#"( ends_with "foo" "foobar" )"#, false);

    Ok(())
}

#[test]
fn test_isdefined_none() -> Result<(), Box<dyn Error>> {
    let handlebars = new_handlebars();

    let result = handlebars.render_template(
        r#"{{isdefined a}} {{isdefined b}} {{#if (isdefined a)}}a{{/if}} {{#if (isdefined b)}}b{{/if}}"#,
        &json!({})
    );
    assert_eq!(result.unwrap(), "false false  ");
    Ok(())
}

#[test]
fn test_isdefined_a_and_b() -> Result<(), Box<dyn Error>> {
    let handlebars = new_handlebars();

    let result = handlebars.render_template(
        r#"{{isdefined a}} {{isdefined b}} {{#if (isdefined a)}}a{{/if}} {{#if (isdefined b)}}b{{/if}}"#,
        &json!({"a": 1, "b": 2})
    );
    assert_eq!(result.unwrap(), "true true a b");
    Ok(())
}

#[test]
fn test_isdefined_a() -> Result<(), Box<dyn Error>> {
    let handlebars = new_handlebars();

    let result = handlebars.render_template(
        r#"{{isdefined a}} {{isdefined b}} {{#if (isdefined a)}}a{{/if}} {{#if (isdefined b)}}b{{/if}}"#,
        &json!({"a": 1})
    );
    assert_eq!(result.unwrap(), "true false a ");
    Ok(())
}

#[test]
fn test_isdefined_helper_with_two_params() {
    let handlebars = new_handlebars();
    
    // Test isdefined with two parameters (should fail)
    let result = handlebars.render_template(
        r#"{{isdefined a b}}"#,
        &json!({"a": 1, "b": 2})
    );
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("requires one parameter"));
}