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

use crate::handle::handle::TaskHandle;
use crate::tasks::request::TaskRequest;
use crate::tasks::response::TaskResponse;
use crate::tasks::TemplateMode;
use std::sync::Arc;
use serde::Deserialize;

// this is storage behind all 'and' and 'with' statements in the program, which
// are mostly implemented in task_fsm

#[derive(Deserialize,Debug)]
#[serde(deny_unknown_fields)]
pub struct FileAttributesInput {
    pub owner: Option<String>,
    pub group: Option<String>,
    pub mode: Option<String>
}

#[derive(Deserialize,Debug)]
#[serde(deny_unknown_fields)]
pub struct FileAttributesEvaluated {
    pub owner: Option<String>,
    pub group: Option<String>,
    pub mode: Option<String>
}

#[derive(Deserialize,Debug,Copy,Clone,PartialEq)]
pub enum Recurse {
    No,
    Yes
}

impl FileAttributesInput {

    // given an octal string (0o755, 0755, or bare 755), return whether it is valid
    pub fn is_octal_string(mode: &String) -> bool {
        let octal_part = Self::strip_octal_prefix(mode.as_str());
        match i32::from_str_radix(octal_part, 8) {
            Ok(_x) => true,
            Err(_y) => false
        }
    }

    /// Strip any supported octal prefix and return the bare digit string.
    ///
    /// Accepts:
    /// - `"0o755"` — Rust-style octal prefix  → returns `"755"`
    /// - `"0755"`  — Unix/C-style octal prefix → returns `"755"`
    /// - `"755"`   — bare octal digits          → returns `"755"`
    fn strip_octal_prefix(mode: &str) -> &str {
        if let Some(rest) = mode.strip_prefix("0o") {
            rest
        } else if mode.starts_with('0') && mode.len() > 1 {
            &mode[1..]
        } else {
            mode
        }
    }

    // given an octal string, like 0o755 or 755, return the numeric value
    /*
    fn octal_string_to_number(response: &Arc<Response>, request: &Arc<TaskRequest>, mode: &String) -> Result<i32,Arc<TaskResponse>> {
        let octal_no_prefix = str::replace(&mode, "0o", "");
        // this error should be screened out by template() below already but return types are important.
        return match i32::from_str_radix(&octal_no_prefix, 8) {
            Ok(x) => Ok(x),
            Err(y) => { return Err(response.is_failed(&request, &format!("invalid octal value extracted from mode, was {}, {:?}", octal_no_prefix,y))); }
        }
    }
    */

    // template **all** the fields in FileAttributesInput fields, checking values and returning errors as needed
    pub fn template(handle: &TaskHandle, request: &Arc<TaskRequest>, tm: TemplateMode, input: &Option<Self>) -> Result<Option<FileAttributesEvaluated>,Arc<TaskResponse>> {

        if tm == TemplateMode::Off {
            return Ok(None);
        }

        if input.is_none() {
            return Ok(None);
        }
        
        let input2 = input.as_ref().unwrap();
        let final_mode_value : Option<String>;

        // owner & group is easy but mode is complex
        // makes sure mode is octal and not accidentally enter decimal or hex or leave off the octal prefix
        // as the input field is a YAML string unwanted conversion shouldn't happen but we want to be strict with other tools
        // that might read the file and encourage users to use YAML-spec required input here even though YAML isn't doing
        // the evaluation.

        if input2.mode.is_some()  {
            let mode_input = input2.mode.as_ref().unwrap();
            let templated_mode_string = handle.template.string(request, tm, &String::from("mode"), &mode_input)?;

            // Accept both 0o755 (Rust-style) and 0755 (traditional Unix octal prefix).
            // Plain digits like "755" are NOT accepted — an explicit prefix is required to
            // prevent accidentally passing decimal values (e.g. "755" decimal ≠ 0755 octal).
            let octal_no_prefix = if templated_mode_string.starts_with("0o") {
                // Rust-style: 0o755 → "755"
                templated_mode_string[2..].to_string()
            } else if templated_mode_string.starts_with('0') && templated_mode_string.len() > 1 {
                // Unix/C-style: 0755 → "755"
                templated_mode_string[1..].to_string()
            } else {
                return Err(handle.response.is_failed(request, &format!(
                    "field (mode) must have an octal prefix: \
                     use 0o755 (Rust-style) or 0755 (Unix-style), was {}",
                    templated_mode_string
                )));
            };

            // Validate that the stripped digits are valid octal.
            match i32::from_str_radix(&octal_no_prefix, 8) {
                Ok(_x) => {
                    final_mode_value = Some(octal_no_prefix);
                },
                Err(_y) => {
                    return Err(handle.response.is_failed(request, &format!(
                        "field (mode) has invalid octal digits: {}", templated_mode_string
                    )));
                }
            };
        } else {
            // mode was left off in the automation content
            final_mode_value = None;
        }

        return Ok(Some(FileAttributesEvaluated {
            owner:         handle.template.string_option_no_spaces(request, tm, &String::from("owner"), &input2.owner)?,
            group:         handle.template.string_option_no_spaces(request, tm, &String::from("group"), &input2.group)?,
            mode:          final_mode_value,
        }));
    }
}


impl FileAttributesEvaluated {

    // if the action has an evaluated Attributes section, the mode will be stored as an octal string like "777", but we need
    // an integer for some internal APIs like the SSH connection put requests.

    /*
    pub fn get_numeric_mode(response: &Arc<Response>, request: &Arc<TaskRequest>, this: &Option<Self>) -> Result<Option<i32>, Arc<TaskResponse>> {

        return match this.is_some() {
            true => {
                let mode = &this.as_ref().unwrap().mode;
                match mode {
                    Some(x) => {
                        let value = FileAttributesInput::octal_string_to_number(response, &request, &x)?;
                        return Ok(Some(value));
                    },
                    None => Ok(None)
                }
            },
            false => Ok(None),
        };
    }
    */

}