// Tests for PreLogicInput templating

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handle::template::Template;
    use crate::handle::response::Response;
    use crate::tasks::request::{TaskRequest, TaskRequestType};
    use crate::tasks::TemplateMode;
    use std::sync::{Arc, RwLock};
    
    #[test]
    fn test_skip_if_exists_stores_raw_template() {
        // Test that PreLogicInput doesn't template skip_if_exists during evaluation
        let input = PreLogicInput {
            condition: None,
            subscribe: None,
            sudo: None,
            items: None,
            tags: None,
            delegate_to: None,
            skip_if_exists: Some("/home/{{ user }}/.config".to_string()),
        };
        
        // Create minimal handle for testing
        let handle = create_test_handle();
        let request = Arc::new(TaskRequest::new(TaskRequestType::Query));
        
        // Template with TemplateMode::Off (like first evaluation)
        let evaluated = PreLogicInput::template(&handle, &request, TemplateMode::Off, &Some(input.clone())).unwrap();
        
        // skip_if_exists should still contain the raw template
        assert!(evaluated.is_some());
        assert_eq!(
            evaluated.unwrap().skip_if_exists,
            Some("/home/{{ user }}/.config".to_string())
        );
    }
    
    #[test]
    fn test_template_renders_variables() {
        // Test that the template system actually renders variables
        let template = Template::new_for_test();
        let mut context = HashMap::new();
        context.insert("user", "testuser");
        
        let input = "/home/{{ user }}/.config";
        let result = template.render_with_context(input, &context).unwrap();
        
        assert_eq!(result, "/home/testuser/.config");
    }
    
    #[test]
    fn test_screen_path_allows_templated_result() {
        use crate::tasks::cmd_library::screen_path;
        
        // Templated result should pass screening
        let good_path = "/home/testuser/.config";
        assert!(screen_path(&good_path.to_string()).is_ok());
        
        // Path with template syntax should fail
        let template_path = "/home/{{ user }}/.config";
        assert!(screen_path(&template_path.to_string()).is_err());
        
        // Path with illegal chars should fail
        let bad_path = "/home/test;user/.config";
        assert!(screen_path(&bad_path.to_string()).is_err());
    }
}