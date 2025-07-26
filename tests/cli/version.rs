use jetpack::cli::version::*;

#[test]
fn test_git_version_is_not_empty() {
    assert!(!GIT_VERSION.is_empty());
    // Version should be a valid git hash (40 characters) or a shorter hash
    assert!(GIT_VERSION.len() >= 7); // Minimum git short hash length
}

#[test]
fn test_git_branch_is_not_empty() {
    assert!(!GIT_BRANCH.is_empty());
    // Branch name should contain valid characters
    assert!(GIT_BRANCH.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '/' || c == '.'));
}

#[test]
fn test_build_time_is_not_empty() {
    assert!(!BUILD_TIME.is_empty());
    // Build time should contain expected date/time components
    assert!(BUILD_TIME.contains(" "));
}

#[test]
fn test_constants_are_static() {
    // Verify that the constants can be used in static contexts
    const _VERSION: &str = GIT_VERSION;
    const _BRANCH: &str = GIT_BRANCH;
    const _TIME: &str = BUILD_TIME;
}

#[test]
fn test_version_format() {
    // Git version should only contain hex characters (for a full hash)
    // or could be a short hash
    let is_valid_hash = GIT_VERSION.chars().all(|c| c.is_ascii_hexdigit());
    assert!(is_valid_hash || GIT_VERSION.contains('-')); // Could be a describe format
}

#[test]
fn test_version_constants_consistency() {
    // All constants should be defined (not empty placeholders)
    assert!(GIT_VERSION.len() > 0);
    assert!(GIT_BRANCH.len() > 0);
    assert!(BUILD_TIME.len() > 0);
    
    // They should all be &str types
    let _v: &str = GIT_VERSION;
    let _b: &str = GIT_BRANCH;
    let _t: &str = BUILD_TIME;
}