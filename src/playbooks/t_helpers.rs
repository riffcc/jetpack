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

