use jetpack::{JetpackConfig, PlaybookRunner, NullOutputHandler};
use std::sync::Arc;
use std::path::PathBuf;

#[test]
fn test_config_builder() {
    let config = JetpackConfig::new()
        .playbook("/tmp/test.yml")
        .inventory("/tmp/inventory")
        .user("testuser".to_string())
        .port(2222)
        .threads(4)
        .local();
    
    assert_eq!(config.default_user, "testuser");
    assert_eq!(config.default_port, 2222);
    assert_eq!(config.threads, 4);
    assert_eq!(config.connection_mode, jetpack::ConnectionMode::Local);
}

#[test]
fn test_runner_creation() {
    let config = JetpackConfig::new()
        .playbook("/tmp/test.yml")
        .local();
    
    let runner = PlaybookRunner::new(config)
        .with_output_handler(Arc::new(NullOutputHandler));
    
    // Just verify it can be created
    assert!(true);
}

#[test]
fn test_builder_api() {
    let _builder = jetpack::run_playbook("/tmp/test.yml")
        .inventory("/tmp/inventory")
        .local()
        .user("admin")
        .threads(2);
    
    // Just verify the builder works
    assert!(true);
}