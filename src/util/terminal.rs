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

pub fn markdown_print(markdown: &String) {
    termimad::print_text(markdown);
}

pub fn banner(msg: &String) {
    let markdown = String::from(format!("|:-|\n\
                                        |{}|\n\
                                        |-", msg));
    markdown_print(&markdown);
}

pub fn two_column_table(header_a: &String, header_b: &String, elements: &Vec<(String,String)>) {
    let mut buffer = String::from("|:-|:-\n");
    buffer.push_str(
        &String::from(format!("|{}|{}\n", header_a, header_b))
    );
    for (a,b) in elements.iter() {
        buffer.push_str(&String::from("|-|-\n"));
        buffer.push_str(
            &String::from(format!("|{}|{}\n", a, b))
        );
    }
    buffer.push_str(&String::from("|-|-\n"));
    markdown_print(&buffer);
}

pub fn captioned_display(caption: &String, body: &String) {
    banner(caption);
    println!("");
    for line in body.lines() {
        println!("    {}", line);
    }
    println!("");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_markdown_print() {
        // Just verify it doesn't panic
        let markdown = String::from("# Test Heading\n\nSome **bold** text");
        markdown_print(&markdown);
    }

    #[test]
    fn test_banner() {
        // Just verify it doesn't panic
        let msg = String::from("Test Banner Message");
        banner(&msg);
    }

    #[test]
    fn test_two_column_table() {
        let header_a = String::from("Column A");
        let header_b = String::from("Column B");
        let elements = vec![
            (String::from("Row 1 A"), String::from("Row 1 B")),
            (String::from("Row 2 A"), String::from("Row 2 B")),
            (String::from("Row 3 A"), String::from("Row 3 B")),
        ];
        
        // Just verify it doesn't panic
        two_column_table(&header_a, &header_b, &elements);
    }

    #[test]
    fn test_two_column_table_empty() {
        let header_a = String::from("Empty A");
        let header_b = String::from("Empty B");
        let elements: Vec<(String, String)> = vec![];
        
        // Should handle empty tables gracefully
        two_column_table(&header_a, &header_b, &elements);
    }

    #[test]
    fn test_captioned_display() {
        let caption = String::from("Test Caption");
        let body = String::from("Line 1\nLine 2\nLine 3");
        
        // Just verify it doesn't panic
        captioned_display(&caption, &body);
    }

    #[test]
    fn test_captioned_display_multiline() {
        let caption = String::from("Multi-line Display");
        let body = String::from("First line\n    Indented line\n\nEmpty line above\nLast line");
        
        // Test with various line formats
        captioned_display(&caption, &body);
    }

    #[test]
    fn test_captioned_display_empty_body() {
        let caption = String::from("Empty Body Test");
        let body = String::from("");
        
        // Should handle empty body gracefully
        captioned_display(&caption, &body);
    }
}