use jetpack::tasks::files::*;

#[test]
fn test_recurse_enum() {
    assert_eq!(Recurse::Yes, Recurse::Yes);
    assert_eq!(Recurse::No, Recurse::No);
    assert_ne!(Recurse::Yes, Recurse::No);
}

#[test]
fn test_recurse_debug() {
    let r = Recurse::Yes;
    assert_eq!(format!("{:?}", r), "Yes");
    
    let r = Recurse::No;
    assert_eq!(format!("{:?}", r), "No");
}

#[test]
fn test_recurse_copy() {
    let r1 = Recurse::Yes;
    let r2 = r1; // Copy
    assert_eq!(r1, r2);
}

#[test]
fn test_recurse_clone() {
    let r1 = Recurse::Yes;
    let r2 = r1.clone();
    assert_eq!(r1, r2);
}

#[test]
fn test_file_attributes_input_is_octal_string() {
    // Valid octal strings
    assert!(FileAttributesInput::is_octal_string(&"755".to_string()));
    assert!(FileAttributesInput::is_octal_string(&"644".to_string()));
    assert!(FileAttributesInput::is_octal_string(&"000".to_string()));
    assert!(FileAttributesInput::is_octal_string(&"777".to_string()));
    assert!(FileAttributesInput::is_octal_string(&"0o755".to_string()));
    assert!(FileAttributesInput::is_octal_string(&"0o644".to_string()));
    
    // Invalid octal strings
    assert!(!FileAttributesInput::is_octal_string(&"999".to_string()));
    assert!(!FileAttributesInput::is_octal_string(&"888".to_string()));
    assert!(!FileAttributesInput::is_octal_string(&"abc".to_string()));
    assert!(!FileAttributesInput::is_octal_string(&"12x".to_string()));
    assert!(!FileAttributesInput::is_octal_string(&"0o999".to_string()));
    assert!(!FileAttributesInput::is_octal_string(&"0o888".to_string()));
}

#[test]
fn test_file_attributes_input_debug() {
    let attr = FileAttributesInput {
        owner: Some("user".to_string()),
        group: Some("group".to_string()),
        mode: Some("0o755".to_string()),
    };
    
    let debug_str = format!("{:?}", attr);
    assert!(debug_str.contains("owner"));
    assert!(debug_str.contains("group"));
    assert!(debug_str.contains("mode"));
}

#[test]
fn test_file_attributes_evaluated_debug() {
    let attr = FileAttributesEvaluated {
        owner: Some("user".to_string()),
        group: Some("group".to_string()),
        mode: Some("0o755".to_string()),
    };
    
    let debug_str = format!("{:?}", attr);
    assert!(debug_str.contains("owner"));
    assert!(debug_str.contains("group"));
    assert!(debug_str.contains("mode"));
}

#[test]
fn test_file_attributes_with_none_values() {
    let attr = FileAttributesInput {
        owner: None,
        group: None,
        mode: None,
    };
    
    assert!(attr.owner.is_none());
    assert!(attr.group.is_none());
    assert!(attr.mode.is_none());
}

#[test]
fn test_is_octal_string_edge_cases() {
    // Empty string
    assert!(!FileAttributesInput::is_octal_string(&"".to_string()));
    
    // Just the prefix
    assert!(!FileAttributesInput::is_octal_string(&"0o".to_string()));
    
    // Very long valid octal
    assert!(FileAttributesInput::is_octal_string(&"12345670".to_string()));
    
    // Negative numbers - from_str_radix accepts them
    assert!(FileAttributesInput::is_octal_string(&"-755".to_string()));
}