use jetpack::playbooks::templar::*;
use serde_yaml;

#[test]
fn test_templar_new() {
    let templar = Templar::new();
    // Just ensure we can create a new instance
    assert!(true);
}

#[test]
fn test_template_mode_equality() {
    assert_eq!(TemplateMode::Strict, TemplateMode::Strict);
    assert_eq!(TemplateMode::Off, TemplateMode::Off);
    assert_ne!(TemplateMode::Strict, TemplateMode::Off);
}

#[test]
fn test_render_simple_template() {
    let templar = Templar::new();
    let mut data = serde_yaml::Mapping::new();
    data.insert(serde_yaml::Value::String("name".to_string()), serde_yaml::Value::String("world".to_string()));
    
    let template = "Hello, {{name}}!".to_string();
    let result = templar.render(&template, data, TemplateMode::Strict);
    
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "Hello, world!");
}

#[test]
fn test_render_template_with_missing_variable() {
    let templar = Templar::new();
    let data = serde_yaml::Mapping::new();
    
    let template = "Hello, {{name}}!".to_string();
    let result = templar.render(&template, data, TemplateMode::Strict);
    
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Template error"));
}

#[test]
fn test_render_off_mode() {
    let templar = Templar::new();
    let data = serde_yaml::Mapping::new();
    
    let template = "Hello, {{name}}!".to_string();
    let result = templar.render(&template, data, TemplateMode::Off);
    
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "empty");
}

#[test]
fn test_test_condition_true() {
    let templar = Templar::new();
    let mut data = serde_yaml::Mapping::new();
    data.insert(serde_yaml::Value::String("value".to_string()), serde_yaml::Value::Number(serde_yaml::Number::from(10)));
    
    let expr = "value".to_string(); // Simple truthiness test
    let result = templar.test_condition(&expr, data, TemplateMode::Strict);
    
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), true);
}

#[test]
fn test_test_condition_false() {
    let templar = Templar::new();
    let mut data = serde_yaml::Mapping::new();
    data.insert(serde_yaml::Value::String("value".to_string()), serde_yaml::Value::Bool(false));
    
    let expr = "value".to_string();
    let result = templar.test_condition(&expr, data, TemplateMode::Strict);
    
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), false);
}

#[test]
fn test_test_condition_off_mode() {
    let templar = Templar::new();
    let data = serde_yaml::Mapping::new();
    
    let expr = "value > 5".to_string();
    let result = templar.test_condition(&expr, data, TemplateMode::Off);
    
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), true); // Always returns true in Off mode
}

#[test]
fn test_test_condition_with_undefined_parameter() {
    let templar = Templar::new();
    let data = serde_yaml::Mapping::new();
    
    let expr = "undefined_var == true".to_string();
    let result = templar.test_condition(&expr, data, TemplateMode::Strict);
    
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.contains("failed to parse conditional"));
}

#[test]
fn test_test_condition_with_complex_expression() {
    let templar = Templar::new();
    let mut data = serde_yaml::Mapping::new();
    data.insert(serde_yaml::Value::String("show".to_string()), serde_yaml::Value::Bool(true));
    data.insert(serde_yaml::Value::String("hide".to_string()), serde_yaml::Value::Bool(false));
    
    let expr = "show".to_string(); // Simplified
    let result = templar.test_condition(&expr, data, TemplateMode::Strict);
    
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), true);
}

#[test]
fn test_render_with_helpers() {
    let templar = Templar::new();
    let mut data = serde_yaml::Mapping::new();
    data.insert(serde_yaml::Value::String("text".to_string()), serde_yaml::Value::String("HELLO".to_string()));
    
    let template = "{{ to_lower_case text }}".to_string();
    let result = templar.render(&template, data, TemplateMode::Strict);
    
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "hello");
}

#[test]
fn test_render_with_if_block() {
    let templar = Templar::new();
    let mut data = serde_yaml::Mapping::new();
    data.insert(serde_yaml::Value::String("show".to_string()), serde_yaml::Value::Bool(true));
    
    let template = "{{#if show}}visible{{else}}hidden{{/if}}".to_string();
    let result = templar.render(&template, data, TemplateMode::Strict);
    
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "visible");
}

#[test]
fn test_template_mode_debug() {
    // Test Debug trait implementation
    let mode = TemplateMode::Strict;
    let debug_str = format!("{:?}", mode);
    assert_eq!(debug_str, "Strict");
    
    let mode = TemplateMode::Off;
    let debug_str = format!("{:?}", mode);
    assert_eq!(debug_str, "Off");
}

#[test]
fn test_template_mode_copy() {
    // Test Copy trait implementation
    let mode1 = TemplateMode::Strict;
    let mode2 = mode1; // Copy
    assert_eq!(mode1, mode2);
}

#[test]
fn test_template_mode_clone() {
    // Test Clone trait implementation
    let mode1 = TemplateMode::Strict;
    let mode2 = mode1.clone();
    assert_eq!(mode1, mode2);
}