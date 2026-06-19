// Jetporch
// Copyright (C) 2023 - Michael DeHaan <michael@michaeldehaan.net> + contributors
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <http://www.gnu.org/licenses/>.

// `jetpack gen-reference` — (re)generate the code-driven module + CLI reference
// under docs/content/docs/reference/ from `docs/reference.json` overrides.
// `--check` verifies the committed output is up to date (the CI guard).

use crate::cli::docs::find_docs_root;
use crate::cli::parser::CliParser;
use crate::docs::reference;
use crate::util::terminal::banner;

pub fn gen_reference(parser: &CliParser) -> i32 {
    match gen_reference_inner(parser) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("{}", e);
            1
        }
    }
}

fn gen_reference_inner(parser: &CliParser) -> Result<(), String> {
    let docs_root = find_docs_root()?;
    let override_path = docs_root.join("reference.json");
    let out_dir = docs_root.join("content").join("docs").join("reference");
    let overrides = reference::load_override(&override_path)?;

    if parser.check {
        reference::check(&overrides, &out_dir)?;
        banner("reference docs are up to date");
    } else {
        reference::generate(&overrides, &out_dir)?;
        banner(&format!(
            "generated reference docs into {}",
            out_dir.display()
        ));
    }
    Ok(())
}
