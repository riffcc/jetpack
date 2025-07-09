use jetpack::handle::template::*;
use jetpack::tasks::request::{TaskRequest, TaskRequestType};
use jetpack::tasks::response::TaskResponse;
use jetpack::playbooks::templar::TemplateMode;
use jetpack::playbooks::traversal::RunState;
use jetpack::playbooks::context::PlaybookContext;
use jetpack::inventory::hosts::Host;
use jetpack::cli::parser::CliParser;
use jetpack::handle::response::Response;
use std::sync::{Arc, RwLock};
use std::path::PathBuf;

fn create_test_template() -> Template {
    let cli_args: Vec<String> = vec!["jetpack".to_string()];
    let parser = Arc::new(CliParser::new(&cli_args, false));
    let context = Arc::new(RwLock::new(PlaybookContext::new(
        Arc::clone(&parser), 
        None, 
        None,
        0
    )));
    let run_state = Arc::new(RunState::new(context));
    let host = Arc::new(RwLock::new(Host::new("testhost", "test_connection")));
    let response = Arc::new(Response::new(Arc::clone(&run_state), Arc::clone(&host)));
    
    Template::new(run_state, host, response)
}

fn create_test_request() -> Arc<TaskRequest> {
    Arc::new(TaskRequest::new(
        TaskRequestType::Query,
        "test_module".to_string(),
        "test_task".to_string(),
    ))
}

#[test]
fn test_template_new() {
    let template = create_test_template();
    // Template creation should succeed
    assert!(true);
}

#[test]
fn test_blend_target_enum() {
    assert_ne!(BlendTarget::NotTemplateModule, BlendTarget::TemplateModule);
    assert_eq!(BlendTarget::NotTemplateModule, BlendTarget::NotTemplateModule);
}

#[test]
fn test_template_string_literal() {
    let template = create_test_template();
    let request = create_test_request();
    
    let result = template.string(&request, TemplateMode::Strict, &"test_field".to_string(), &"literal_value".to_string());
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "literal_value");
}

#[test]
fn test_template_string_with_variable() {
    let template = create_test_template();
    let request = create_test_request();
    
    // Add a variable to the host
    template.host.write().unwrap().update_variables(
        serde_yaml::from_str("test_var: test_value").unwrap()
    );
    
    let result = template.string(&request, TemplateMode::Strict, &"test_field".to_string(), &"{{ test_var }}".to_string());
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "test_value");
}

#[test]
fn test_template_string_no_spaces() {
    let template = create_test_template();
    let request = create_test_request();
    
    let result = template.string_no_spaces(&request, TemplateMode::Strict, &"test_field".to_string(), &"value_no_spaces".to_string());
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "value_no_spaces");
    
    // Test with spaces should fail
    let result_with_spaces = template.string_no_spaces(&request, TemplateMode::Strict, &"test_field".to_string(), &"value with spaces".to_string());
    assert!(result_with_spaces.is_err());
}

#[test]
fn test_template_string_option() {
    let template = create_test_template();
    let request = create_test_request();
    
    let some_value = Some("test_value".to_string());
    let result = template.string_option(&request, TemplateMode::Strict, &"test_field".to_string(), &some_value);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Some("test_value".to_string()));
    
    let none_value: Option<String> = None;
    let result_none = template.string_option(&request, TemplateMode::Strict, &"test_field".to_string(), &none_value);
    assert!(result_none.is_ok());
    assert_eq!(result_none.unwrap(), None);
}

#[test]
fn test_template_string_option_default() {
    let template = create_test_template();
    let request = create_test_request();
    
    let some_value = Some("custom_value".to_string());
    let result = template.string_option_default(&request, TemplateMode::Strict, &"test_field".to_string(), &some_value, &"default_value".to_string());
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "custom_value");
    
    let none_value: Option<String> = None;
    let result_default = template.string_option_default(&request, TemplateMode::Strict, &"test_field".to_string(), &none_value, &"default_value".to_string());
    assert!(result_default.is_ok());
    assert_eq!(result_default.unwrap(), "default_value");
}

#[test]
fn test_template_path() {
    let template = create_test_template();
    let request = create_test_request();
    
    let result = template.path(&request, TemplateMode::Strict, &"test_path".to_string(), &"/tmp/test/path".to_string());
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "/tmp/test/path");
}

#[test]
fn test_template_boolean_string() {
    let template = create_test_template();
    let request = create_test_request();
    
    // Test "yes"
    let result_yes = template.boolean_string(&request, TemplateMode::Strict, &"test_bool".to_string(), &"yes".to_string());
    assert!(result_yes.is_ok());
    assert_eq!(result_yes.unwrap(), true);
    
    // Test "no"
    let result_no = template.boolean_string(&request, TemplateMode::Strict, &"test_bool".to_string(), &"no".to_string());
    assert!(result_no.is_ok());
    assert_eq!(result_no.unwrap(), false);
    
    // Test invalid boolean string
    let result_invalid = template.boolean_string(&request, TemplateMode::Strict, &"test_bool".to_string(), &"maybe".to_string());
    assert!(result_invalid.is_err());
}

#[test]
fn test_template_boolean_option_default_false() {
    let template = create_test_template();
    let request = create_test_request();
    
    let some_yes = Some("yes".to_string());
    let result_yes = template.boolean_option_default_false(&request, TemplateMode::Strict, &"test_bool".to_string(), &some_yes);
    assert!(result_yes.is_ok());
    assert_eq!(result_yes.unwrap(), true);
    
    let none_value: Option<String> = None;
    let result_default = template.boolean_option_default_false(&request, TemplateMode::Strict, &"test_bool".to_string(), &none_value);
    assert!(result_default.is_ok());
    assert_eq!(result_default.unwrap(), false);
}

#[test]
fn test_template_boolean_option_default_true() {
    let template = create_test_template();
    let request = create_test_request();
    
    let some_no = Some("no".to_string());
    let result_no = template.boolean_option_default_true(&request, TemplateMode::Strict, &"test_bool".to_string(), &some_no);
    assert!(result_no.is_ok());
    assert_eq!(result_no.unwrap(), false);
    
    let none_value: Option<String> = None;
    let result_default = template.boolean_option_default_true(&request, TemplateMode::Strict, &"test_bool".to_string(), &none_value);
    assert!(result_default.is_ok());
    assert_eq!(result_default.unwrap(), true);
}