// Jetporch
// Copyright (C) 2023 - Jetporch Project Contributors
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

use handlebars::{Handlebars, RenderError, HelperDef, RenderContext, ScopedJson, JsonValue, Helper, Context, handlebars_helper};

//#[allow(non_camel_case_types)]
pub struct IsDefined;

impl HelperDef for IsDefined {
    fn call_inner<'reg: 'rc, 'rc>(
        &self,
        h: &Helper<'reg, 'rc>,
        _: &'reg Handlebars,
        _: &'rc Context,
        _: &mut RenderContext<'reg, 'rc>,
    ) -> Result<ScopedJson<'reg, 'rc>, RenderError> {
        let params = h.params();
        if params.len() != 1 {
            return Err(RenderError::new(
                "is_defined: requires one parameter".to_owned(),
            ));
        }
        let result = h.param(0)
            .and_then(|x| {
                if x.is_value_missing() {
                    Some(false)
                } else {
                    Some(true)
                }
            })
            .ok_or_else(|| RenderError::new("is_defined: Couldn't read parameter".to_owned()))?;

        Ok(ScopedJson::Derived(JsonValue::from(result)))
    }
}

pub fn register_helpers(handlebars: &mut Handlebars) {
    {
        handlebars_helper!(to_lower_case: |v: str| v.to_lowercase());
        handlebars.register_helper("to_lower_case", Box::new(to_lower_case))
    }
    {
        handlebars_helper!(to_upper_case: |v: str| v.to_uppercase());
        handlebars.register_helper("to_upper_case", Box::new(to_upper_case))
    }
    {
        handlebars_helper!(trim: |v: str| v.trim());
        handlebars.register_helper("trim", Box::new(trim))
    }
    {
        handlebars_helper!(trim_start: |v: str| v.trim_start());
        handlebars.register_helper("trim_start", Box::new(trim_start))
    }
    {
        handlebars_helper!(trim_end: |v: str| v.trim_end());
        handlebars.register_helper("trim_end", Box::new(trim_end))
    }
    {
        handlebars_helper!(contains: |v: str, s: str| v.contains(s));
        handlebars.register_helper("contains", Box::new(contains))
    }
    {
        handlebars_helper!(starts_with: |v: str, s: str| v.starts_with(s));
        handlebars.register_helper("starts_with", Box::new(starts_with))
    }
    {
        handlebars_helper!(ends_with: |v: str, s: str| v.ends_with(s));
        handlebars.register_helper("ends_with", Box::new(ends_with))
    }
    {
        handlebars.register_helper("isdefined", Box::new(IsDefined));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use handlebars::Handlebars;
    use serde_json::json;

    #[test]
    fn test_to_lower_case() {
        let mut handlebars = Handlebars::new();
        register_helpers(&mut handlebars);
        
        let template = "{{to_lower_case \"HELLO WORLD\"}}";
        let result = handlebars.render_template(template, &json!({})).unwrap();
        assert_eq!(result, "hello world");
    }
    
    #[test]
    fn test_to_upper_case() {
        let mut handlebars = Handlebars::new();
        register_helpers(&mut handlebars);
        
        let template = "{{to_upper_case \"hello world\"}}";
        let result = handlebars.render_template(template, &json!({})).unwrap();
        assert_eq!(result, "HELLO WORLD");
    }
    
    #[test]
    fn test_trim() {
        let mut handlebars = Handlebars::new();
        register_helpers(&mut handlebars);
        
        let template = "{{trim \"  hello world  \"}}";
        let result = handlebars.render_template(template, &json!({})).unwrap();
        assert_eq!(result, "hello world");
    }
    
    #[test]
    fn test_trim_start() {
        let mut handlebars = Handlebars::new();
        register_helpers(&mut handlebars);
        
        let template = "{{trim_start \"  hello world  \"}}";
        let result = handlebars.render_template(template, &json!({})).unwrap();
        assert_eq!(result, "hello world  ");
    }
    
    #[test]
    fn test_trim_end() {
        let mut handlebars = Handlebars::new();
        register_helpers(&mut handlebars);
        
        let template = "{{trim_end \"  hello world  \"}}";
        let result = handlebars.render_template(template, &json!({})).unwrap();
        assert_eq!(result, "  hello world");
    }
    
    #[test]
    fn test_contains_true() {
        let mut handlebars = Handlebars::new();
        register_helpers(&mut handlebars);
        
        let template = "{{contains \"hello world\" \"world\"}}";
        let result = handlebars.render_template(template, &json!({})).unwrap();
        assert_eq!(result, "true");
    }
    
    #[test]
    fn test_contains_false() {
        let mut handlebars = Handlebars::new();
        register_helpers(&mut handlebars);
        
        let template = "{{contains \"hello world\" \"foo\"}}";
        let result = handlebars.render_template(template, &json!({})).unwrap();
        assert_eq!(result, "false");
    }
    
    #[test]
    fn test_starts_with_true() {
        let mut handlebars = Handlebars::new();
        register_helpers(&mut handlebars);
        
        let template = "{{starts_with \"hello world\" \"hello\"}}";
        let result = handlebars.render_template(template, &json!({})).unwrap();
        assert_eq!(result, "true");
    }
    
    #[test]
    fn test_starts_with_false() {
        let mut handlebars = Handlebars::new();
        register_helpers(&mut handlebars);
        
        let template = "{{starts_with \"hello world\" \"world\"}}";
        let result = handlebars.render_template(template, &json!({})).unwrap();
        assert_eq!(result, "false");
    }
    
    #[test]
    fn test_ends_with_true() {
        let mut handlebars = Handlebars::new();
        register_helpers(&mut handlebars);
        
        let template = "{{ends_with \"hello world\" \"world\"}}";
        let result = handlebars.render_template(template, &json!({})).unwrap();
        assert_eq!(result, "true");
    }
    
    #[test]
    fn test_ends_with_false() {
        let mut handlebars = Handlebars::new();
        register_helpers(&mut handlebars);
        
        let template = "{{ends_with \"hello world\" \"hello\"}}";
        let result = handlebars.render_template(template, &json!({})).unwrap();
        assert_eq!(result, "false");
    }
    
    #[test]
    fn test_isdefined_with_defined_value() {
        let mut handlebars = Handlebars::new();
        register_helpers(&mut handlebars);
        
        let template = "{{isdefined myvar}}";
        let data = json!({ "myvar": "test" });
        let result = handlebars.render_template(template, &data).unwrap();
        assert_eq!(result, "true");
    }
    
    #[test]
    fn test_isdefined_with_undefined_value() {
        let mut handlebars = Handlebars::new();
        register_helpers(&mut handlebars);
        
        let template = "{{isdefined myvar}}";
        let data = json!({});
        let result = handlebars.render_template(template, &data).unwrap();
        assert_eq!(result, "false");
    }
    
    #[test]
    fn test_isdefined_with_null_value() {
        let mut handlebars = Handlebars::new();
        register_helpers(&mut handlebars);
        
        let template = "{{isdefined myvar}}";
        let data = json!({ "myvar": null });
        let result = handlebars.render_template(template, &data).unwrap();
        assert_eq!(result, "true");
    }
    
    #[test]
    fn test_isdefined_no_params_error() {
        let mut handlebars = Handlebars::new();
        register_helpers(&mut handlebars);
        
        let template = "{{isdefined}}";
        let result = handlebars.render_template(template, &json!({}));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("requires one parameter"));
    }
    
    #[test]
    fn test_isdefined_too_many_params_error() {
        let mut handlebars = Handlebars::new();
        register_helpers(&mut handlebars);
        
        let template = "{{isdefined myvar another}}";
        let result = handlebars.render_template(template, &json!({}));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("requires one parameter"));
    }
    
    #[test]
    fn test_multiple_helpers_in_template() {
        let mut handlebars = Handlebars::new();
        register_helpers(&mut handlebars);
        
        let template = "{{to_upper_case (trim \"  hello  \")}}";
        let result = handlebars.render_template(template, &json!({})).unwrap();
        assert_eq!(result, "HELLO");
    }
    
    #[test]
    fn test_helpers_with_variables() {
        let mut handlebars = Handlebars::new();
        register_helpers(&mut handlebars);
        
        let template = "{{to_lower_case name}}";
        let data = json!({ "name": "JOHN DOE" });
        let result = handlebars.render_template(template, &data).unwrap();
        assert_eq!(result, "john doe");
    }
    
    #[test]
    fn test_helpers_with_special_characters() {
        let mut handlebars = Handlebars::new();
        register_helpers(&mut handlebars);
        
        let template = "{{trim \"\\t\\n  hello\\r\\n  \"}}";
        let result = handlebars.render_template(template, &json!({})).unwrap();
        assert_eq!(result, "hello");
    }
    
    #[test]
    fn test_contains_empty_string() {
        let mut handlebars = Handlebars::new();
        register_helpers(&mut handlebars);
        
        let template = "{{contains \"hello\" \"\"}}";
        let result = handlebars.render_template(template, &json!({})).unwrap();
        assert_eq!(result, "true"); // Empty string is contained in any string
    }
    
    #[test]
    fn test_helpers_with_unicode() {
        let mut handlebars = Handlebars::new();
        register_helpers(&mut handlebars);
        
        let template = "{{to_upper_case \"café\"}}";
        let result = handlebars.render_template(template, &json!({})).unwrap();
        assert_eq!(result, "CAFÉ");
    }
}
