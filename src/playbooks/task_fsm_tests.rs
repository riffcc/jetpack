// Tests for task FSM functionality, particularly skip_if_exists

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;
    
    #[test]
    fn test_skip_if_exists_with_template() {
        // Create a temporary directory for testing
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("test_file.txt");
        
        // Create the test file
        fs::write(&test_file, "test content").unwrap();
        
        // Create a mock task with skip_if_exists using a template
        let yaml = format!(r#"
- name: Test skip_if_exists
  hosts: localhost
  tasks:
    - name: Test task
      !shell:
        cmd: echo "This should be skipped"
      with:
        skip_if_exists: "{}/{{{{ file_name }}}}"
"#, temp_dir.path().display());
        
        // Set up context with the template variable
        let mut context = PlaybookContext::new();
        context.set_var("file_name", "test_file.txt");
        
        // Run the playbook
        let result = run_playbook_from_string(&yaml, context);
        
        // The task should be skipped because the file exists
        assert!(result.is_ok());
        assert!(result.unwrap().tasks_skipped == 1);
    }
    
    #[test]
    fn test_skip_if_exists_without_template() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("static_file.txt");
        
        // Create the test file
        fs::write(&test_file, "test content").unwrap();
        
        // Create a task with skip_if_exists using a static path
        let yaml = format!(r#"
- name: Test skip_if_exists static
  hosts: localhost
  tasks:
    - name: Test task
      !shell:
        cmd: echo "This should be skipped"
      with:
        skip_if_exists: "{}"
"#, test_file.display());
        
        let result = run_playbook_from_string(&yaml, PlaybookContext::new());
        
        // Should be skipped
        assert!(result.is_ok());
        assert!(result.unwrap().tasks_skipped == 1);
    }
    
    #[test]
    fn test_skip_if_exists_file_not_exists() {
        let temp_dir = TempDir::new().unwrap();
        let test_file = temp_dir.path().join("nonexistent.txt");
        
        // Do NOT create the file
        
        let yaml = format!(r#"
- name: Test skip_if_exists no file
  hosts: localhost
  tasks:
    - name: Test task
      !shell:
        cmd: echo "This should run"
      with:
        skip_if_exists: "{}"
"#, test_file.display());
        
        let result = run_playbook_from_string(&yaml, PlaybookContext::new());
        
        // Should NOT be skipped
        assert!(result.is_ok());
        assert!(result.unwrap().tasks_run == 1);
        assert!(result.unwrap().tasks_skipped == 0);
    }
    
    #[test]
    fn test_skip_if_exists_with_illegal_chars_in_result() {
        // Test that illegal chars in the RESULT (not template) fail properly
        let yaml = r#"
- name: Test illegal result
  hosts: localhost
  tasks:
    - name: Test task
      !shell:
        cmd: echo "Should fail"
      with:
        skip_if_exists: "/path/with;semicolon"
"#;
        
        let result = run_playbook_from_string(yaml, PlaybookContext::new());
        
        // Should fail due to illegal character
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("illegal characters"));
    }
    
    #[test]
    fn test_skip_if_exists_complex_template() {
        let temp_dir = TempDir::new().unwrap();
        let user_dir = temp_dir.path().join("testuser");
        fs::create_dir(&user_dir).unwrap();
        
        let config_file = user_dir.join(".config");
        fs::write(&config_file, "config").unwrap();
        
        let yaml = format!(r#"
- name: Test complex template
  hosts: localhost
  tasks:
    - name: Test task
      !shell:
        cmd: echo "Should be skipped"
      with:
        skip_if_exists: "{}/{{{{ username }}}}/.config"
"#, temp_dir.path().display());
        
        let mut context = PlaybookContext::new();
        context.set_var("username", "testuser");
        
        let result = run_playbook_from_string(&yaml, context);
        
        assert!(result.is_ok());
        assert!(result.unwrap().tasks_skipped == 1);
    }
}