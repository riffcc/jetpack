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

use serde_yaml;
use once_cell::sync::Lazy;
use handlebars::Handlebars;

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
    return hb;
});

// 'off' mode is used in a bit of a weird traversal/engine
// situation where we need to get access to some task parameters
// before templates are evaluated. You will notice there is no way
// to evaluate templates in unstrict mode. This is by design.

#[derive(PartialEq,Copy,Clone,Debug)]
pub enum TemplateMode {
    Strict,
    Off
}

pub struct Templar {
}

impl Templar {

    pub fn new() -> Self {
        return Self {
        };
    }

    // evaluate a string

    pub fn render(&self, template: &String, data: serde_yaml::Mapping, template_mode: TemplateMode) -> Result<String, String> {
        match template_mode {
            TemplateMode::Off => Ok(String::from("empty")),
            TemplateMode::Strict => {
                let mut rendered = template.clone();
                for _ in 0..8 {
                    let next = HANDLEBARS
                        .render_template(&rendered, &data)
                        .map_err(|y| format!("Template error: {}", y.desc))?;
                    if next == rendered || !next.contains("{{") {
                        return Ok(next);
                    }
                    rendered = next;
                }
                Err("Template error: exceeded recursive render limit".to_string())
            }
        }
    }
    
    // used for with/cond and also in the shell module

    pub fn test_condition(&self, expr: &String, data: serde_yaml::Mapping, template_mode: TemplateMode) -> Result<bool, String> {
        if template_mode == TemplateMode::Off {
            /* this is only used to get back the raw 'items' collection inside the task FSM */
            return Ok(true);
        }
        // embed the expression in an if statement as a way to evaluate it for truth
        let template = format!("{{{{#if {expr} }}}}true{{{{ else }}}}false{{{{/if}}}}");
        let result = self.render(&template, data, TemplateMode::Strict);
        match result {
            Ok(x) => { 
                if x.as_str().eq("true") {
                    return Ok(true);
                } else {
                    return Ok(false);
                }
            },
            Err(y) => { 
                if y.find("Couldn't read parameter").is_some() {
                    return Err(format!("failed to parse conditional: {}: one or more parameters may be undefined", expr))
                }
                else {
                    return Err(format!("failed to parse conditional: {}: {}", expr, y))
                }
            }
        };
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
            serde_yaml::Value::String("{{ JET_USER_HOME }}/projects/riffenvironment/projects/codex-desktop-linux".to_string()),
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
