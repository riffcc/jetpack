use jetpack::modules::packages::homebrew::*;
use jetpack::tasks::*;

#[test]
fn test_homebrew_task_basic() {
    let task = HomebrewTask {
        name: Some("Install wget".to_string()),
        package: "wget".to_string(),
        version: None,
        update: None,
        remove: None,
        with: None,
        and: None,
    };

    assert_eq!(task.get_module(), "homebrew");
    assert_eq!(task.get_name(), Some("Install wget".to_string()));
    assert_eq!(task.package, "wget");
    assert!(task.version.is_none());
    assert!(task.get_with().is_none());
}

#[test]
fn test_homebrew_task_with_version() {
    let task = HomebrewTask {
        name: None,
        package: "node".to_string(),
        version: Some("18".to_string()),
        update: None,
        remove: None,
        with: None,
        and: None,
    };

    assert_eq!(task.get_module(), "homebrew");
    assert_eq!(task.package, "node");
    assert_eq!(task.version, Some("18".to_string()));
}

#[test]
fn test_homebrew_task_remove() {
    let task = HomebrewTask {
        name: Some("Uninstall old tool".to_string()),
        package: "python@3.9".to_string(),
        version: None,
        update: None,
        remove: Some("yes".to_string()),
        with: None,
        and: None,
    };

    assert!(task.remove.is_some());
    assert_eq!(task.remove.unwrap(), "yes");
}

#[test]
fn test_homebrew_task_with_update() {
    let task = HomebrewTask {
        name: Some("Update git".to_string()),
        package: "git".to_string(),
        version: None,
        update: Some("yes".to_string()),
        remove: None,
        with: None,
        and: None,
    };

    assert!(task.update.is_some());
    assert_eq!(task.update.unwrap(), "yes");
}

#[test]
fn test_homebrew_task_deserialization() {
    let yaml = r#"
name: Install development tools
package: rust
update: "yes"
"#;

    let task: Result<HomebrewTask, _> = serde_yaml::from_str(yaml);
    assert!(task.is_ok());
    
    let task = task.unwrap();
    assert_eq!(task.name, Some("Install development tools".to_string()));
    assert_eq!(task.package, "rust");
    assert_eq!(task.update, Some("yes".to_string()));
}

#[test]
fn test_homebrew_task_deserialization_minimal() {
    let yaml = r#"
package: tmux
"#;

    let task: Result<HomebrewTask, _> = serde_yaml::from_str(yaml);
    assert!(task.is_ok());
    
    let task = task.unwrap();
    assert_eq!(task.package, "tmux");
    assert!(task.name.is_none());
    assert!(task.version.is_none());
}

#[test]
fn test_homebrew_task_with_logic() {
    let yaml = r#"
package: "{{ tool_name }}"
with:
  condition: "{{ is_macos }}"
and:
  notify: "tool_installed"
"#;

    let task: Result<HomebrewTask, _> = serde_yaml::from_str(yaml);
    assert!(task.is_ok());
    
    let task = task.unwrap();
    assert!(task.package.contains("{{ tool_name }}"));
    assert!(task.with.is_some());
    assert!(task.and.is_some());
}