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

use std::path::Path;
use std::fs::read_to_string;
use crate::util::terminal::banner;

const YAML_ERROR_SHOW_LINES:usize = 10;
const YAML_ERROR_WIDTH:usize = 180; // things will wrap in terminal anyway

// ==============================================================================================================
// PUBLIC API
// ==============================================================================================================

pub fn show_yaml_error_in_context(yaml_error: &serde_yaml::Error, path: &Path) {

    println!("");

    let location = yaml_error.location();
    let mut yaml_error_str = String::from(format!("{}", yaml_error));

    yaml_error_str.truncate(YAML_ERROR_WIDTH);
    if yaml_error_str.len() > YAML_ERROR_WIDTH - 3 {
        yaml_error_str.push_str("...");
    }

    if location.is_none() {
        let markdown_table = format!("|:-|\n\
                                      |Error reading YAML file: {}|\n\
                                      |{}|\n\
                                      |-", path.display(), yaml_error_str);
        crate::util::terminal::markdown_print(&markdown_table);
        return;
    }

    // get the line/column info out of the location object
    let location = location.unwrap();
    let error_line = location.line();
    let error_column = location.column();

    let lines: Vec<String> = read_to_string(path).unwrap().lines().map(String::from).collect();
    let line_count = lines.len();

    banner(&format!("Error reading YAML file: {}, {}", path.display(), yaml_error_str).to_string());

    let show_start: usize;
    let mut show_stop : usize = error_line + YAML_ERROR_SHOW_LINES;
    
    if error_line < YAML_ERROR_SHOW_LINES {
        show_start = 0; 
    } else {
        show_start = error_line - YAML_ERROR_SHOW_LINES;
    }

    if show_stop > line_count {
        show_stop = line_count;
    }

    println!("");

    let mut count: usize = 0;

    for line in lines.iter() {
        count = count + 1;
        if count >= show_start && count <= show_stop {
            if count ==  error_line {
                println!("     {count:5}:{error_column:5} | >>> | {}", line);
            } else {
                println!("     {count:5}       |     | {}", line);
            }
        }
    }

    println!("");

}

pub fn blend_variables(a: &mut serde_yaml::Value, b: serde_yaml::Value) {

    match (a, b) {

        (_a @ &mut serde_yaml::Value::Mapping(_), serde_yaml::Value::Null) => {
        },

        (a @ &mut serde_yaml::Value::Mapping(_), serde_yaml::Value::Mapping(b)) => {
            let a = a.as_mapping_mut().unwrap();
            for (k, v) in b {
                if v.is_sequence() && a.contains_key(&k) && a[&k].is_sequence() {
                    let mut _b = a.get(&k).unwrap().as_sequence().unwrap().to_owned();
                    _b.append(&mut v.as_sequence().unwrap().to_owned());
                    a[&k] = serde_yaml::Value::from(_b);
                    continue;
                }
                if !a.contains_key(&k) {
                    a.insert(k.to_owned(), v.to_owned());
                }
                else {
                    blend_variables(&mut a[&k], v);
                }

            }
        }
        (a, b) => {
            *a = b
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::{self, File};
    use std::io::Write;
    use tempfile::TempDir;

    #[test]
    fn test_blend_variables_null_into_mapping() {
        let mut a = serde_yaml::from_str("key: value").unwrap();
        let b = serde_yaml::Value::Null;
        
        blend_variables(&mut a, b);
        
        assert_eq!(a["key"], "value");
    }

    #[test]
    fn test_blend_variables_mapping_into_mapping() {
        let mut a = serde_yaml::from_str("key1: value1").unwrap();
        let b = serde_yaml::from_str("key2: value2").unwrap();
        
        blend_variables(&mut a, b);
        
        assert_eq!(a["key1"], "value1");
        assert_eq!(a["key2"], "value2");
    }

    #[test]
    fn test_blend_variables_override_value() {
        let mut a = serde_yaml::from_str("key: old_value").unwrap();
        let b = serde_yaml::from_str("key: new_value").unwrap();
        
        blend_variables(&mut a, b);
        
        assert_eq!(a["key"], "new_value");
    }

    #[test]
    fn test_blend_variables_merge_sequences() {
        let mut a = serde_yaml::from_str("list: [1, 2]").unwrap();
        let b = serde_yaml::from_str("list: [3, 4]").unwrap();
        
        blend_variables(&mut a, b);
        
        let list = a["list"].as_sequence().unwrap();
        assert_eq!(list.len(), 4);
        assert_eq!(list[0], 1);
        assert_eq!(list[1], 2);
        assert_eq!(list[2], 3);
        assert_eq!(list[3], 4);
    }

    #[test]
    fn test_blend_variables_nested_mappings() {
        let mut a = serde_yaml::from_str("
parent:
  child1: value1
").unwrap();
        let b = serde_yaml::from_str("
parent:
  child2: value2
").unwrap();
        
        blend_variables(&mut a, b);
        
        assert_eq!(a["parent"]["child1"], "value1");
        assert_eq!(a["parent"]["child2"], "value2");
    }

    #[test]
    fn test_blend_variables_replace_non_mapping() {
        // When 'a' is a non-mapping, it gets completely replaced by 'b'
        let mut a = serde_yaml::Value::String("original".to_string());
        let b = serde_yaml::from_str("key: value").unwrap();
        
        blend_variables(&mut a, b);
        
        // 'a' should now be the mapping from 'b'
        assert!(a.is_mapping());
        assert_eq!(a["key"], "value");
    }

    #[test]
    fn test_show_yaml_error_without_location() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.yaml");
        let mut file = File::create(&file_path).unwrap();
        writeln!(file, "invalid: yaml: content").unwrap();
        
        // Create a YAML error without location info
        let yaml_content = "invalid: yaml: content";
        let error = serde_yaml::from_str::<serde_yaml::Value>(yaml_content).unwrap_err();
        
        // This should not panic
        show_yaml_error_in_context(&error, &file_path);
    }

    #[test]
    fn test_show_yaml_error_with_location() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.yaml");
        let mut file = File::create(&file_path).unwrap();
        
        // Write a file with multiple lines
        for i in 1..25 {
            writeln!(file, "line{}: value{}", i, i).unwrap();
        }
        writeln!(file, "invalid: [unclosed").unwrap();
        
        // Try to parse the invalid file
        let content = fs::read_to_string(&file_path).unwrap();
        let error = serde_yaml::from_str::<serde_yaml::Value>(&content).unwrap_err();
        
        // This should not panic and should show context
        show_yaml_error_in_context(&error, &file_path);
    }

    #[test]
    fn test_show_yaml_error_near_start() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.yaml");
        let mut file = File::create(&file_path).unwrap();
        
        writeln!(file, "invalid: [unclosed").unwrap();
        writeln!(file, "line2: value2").unwrap();
        writeln!(file, "line3: value3").unwrap();
        
        let content = fs::read_to_string(&file_path).unwrap();
        let error = serde_yaml::from_str::<serde_yaml::Value>(&content).unwrap_err();
        
        // This should handle edge case where error is near start
        show_yaml_error_in_context(&error, &file_path);
    }

    #[test]
    fn test_show_yaml_error_near_end() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.yaml");
        let mut file = File::create(&file_path).unwrap();
        
        writeln!(file, "line1: value1").unwrap();
        writeln!(file, "line2: value2").unwrap();
        writeln!(file, "invalid: [unclosed").unwrap();
        
        let content = fs::read_to_string(&file_path).unwrap();
        let error = serde_yaml::from_str::<serde_yaml::Value>(&content).unwrap_err();
        
        // This should handle edge case where error is near end
        show_yaml_error_in_context(&error, &file_path);
    }

    #[test]
    fn test_show_yaml_error_truncation() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.yaml");
        let mut file = File::create(&file_path).unwrap();
        
        // Create invalid YAML that will generate a long error message
        let long_content = "a".repeat(200);
        writeln!(file, "key: [{}unclosed", long_content).unwrap();
        
        let content = fs::read_to_string(&file_path).unwrap();
        let error = serde_yaml::from_str::<serde_yaml::Value>(&content).unwrap_err();
        
        // This should truncate long error messages - just verify it doesn't panic
        show_yaml_error_in_context(&error, &file_path);
    }
}
