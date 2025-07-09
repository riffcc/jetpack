use jetpack::modules::files::git::*;
use jetpack::tasks::*;
use std::collections::HashMap;

#[test]
fn test_git_task_basic() {
    let task = GitTask {
        name: Some("Clone repository".to_string()),
        repo: "https://github.com/example/repo.git".to_string(),
        path: "/opt/myapp".to_string(),
        branch: None,
        ssh_options: None,
        accept_keys: None,
        update: None,
        attributes: None,
        with: None,
        and: None,
    };

    assert_eq!(task.get_module(), "git");
    assert_eq!(task.get_name(), Some("Clone repository".to_string()));
    assert_eq!(task.repo, "https://github.com/example/repo.git");
    assert_eq!(task.path, "/opt/myapp");
    assert!(task.get_with().is_none());
}

#[test]
fn test_git_task_with_branch() {
    let task = GitTask {
        name: None,
        repo: "git@github.com:example/private-repo.git".to_string(),
        path: "/var/lib/app".to_string(),
        branch: Some("develop".to_string()),
        ssh_options: None,
        accept_keys: Some("yes".to_string()),
        update: Some("yes".to_string()),
        attributes: None,
        with: None,
        and: None,
    };

    assert_eq!(task.get_module(), "git");
    assert!(task.branch.is_some());
    assert_eq!(task.branch.unwrap(), "develop");
    assert!(task.accept_keys.is_some());
}

#[test]
fn test_git_task_with_ssh_options() {
    let mut ssh_opts = HashMap::new();
    ssh_opts.insert("IdentityFile".to_string(), "~/.ssh/deploy_key".to_string());
    ssh_opts.insert("Port".to_string(), "2222".to_string());

    let task = GitTask {
        name: Some("Clone with SSH options".to_string()),
        repo: "git@gitlab.com:company/project.git".to_string(),
        path: "/opt/project".to_string(),
        branch: Some("main".to_string()),
        ssh_options: Some(ssh_opts),
        accept_keys: Some("no".to_string()),
        update: None,
        attributes: None,
        with: None,
        and: None,
    };

    assert!(task.ssh_options.is_some());
    let opts = task.ssh_options.unwrap();
    assert_eq!(opts.get("IdentityFile"), Some(&"~/.ssh/deploy_key".to_string()));
    assert_eq!(opts.get("Port"), Some(&"2222".to_string()));
}

#[test]
fn test_git_task_with_attributes() {
    let attrs = FileAttributesInput {
        owner: Some("deploy".to_string()),
        group: Some("deploy".to_string()),
        mode: Some("0755".to_string()),
    };

    let task = GitTask {
        name: None,
        repo: "https://github.com/public/repo.git".to_string(),
        path: "/srv/app".to_string(),
        branch: None,
        ssh_options: None,
        accept_keys: None,
        update: Some("no".to_string()),
        attributes: Some(attrs),
        with: None,
        and: None,
    };

    assert!(task.attributes.is_some());
    let attrs = task.attributes.as_ref().unwrap();
    assert_eq!(attrs.owner, Some("deploy".to_string()));
}

#[test]
fn test_git_task_deserialization() {
    let yaml = r#"
name: Deploy application code
repo: https://github.com/mycompany/myapp.git
path: /opt/myapp
branch: production
update: "yes"
attributes:
  owner: app
  group: app
  mode: "0755"
"#;

    let task: Result<GitTask, _> = serde_yaml::from_str(yaml);
    assert!(task.is_ok());
    
    let task = task.unwrap();
    assert_eq!(task.name, Some("Deploy application code".to_string()));
    assert_eq!(task.repo, "https://github.com/mycompany/myapp.git");
    assert_eq!(task.path, "/opt/myapp");
    assert_eq!(task.branch, Some("production".to_string()));
    assert_eq!(task.update, Some("yes".to_string()));
    assert!(task.attributes.is_some());
}

#[test]
fn test_git_task_deserialization_minimal() {
    let yaml = r#"
repo: https://github.com/simple/repo.git
path: /tmp/simple-repo
"#;

    let task: Result<GitTask, _> = serde_yaml::from_str(yaml);
    assert!(task.is_ok());
    
    let task = task.unwrap();
    assert_eq!(task.repo, "https://github.com/simple/repo.git");
    assert_eq!(task.path, "/tmp/simple-repo");
    assert!(task.branch.is_none());
}

#[test]
fn test_git_task_with_logic() {
    let yaml = r#"
repo: "{{ git_repo_url }}"
path: "/opt/{{ app_name }}"
branch: "{{ git_branch | default('main') }}"
with:
  condition: "{{ deploy_code }}"
and:
  notify: "restart_application"
"#;

    let task: Result<GitTask, _> = serde_yaml::from_str(yaml);
    assert!(task.is_ok());
    
    let task = task.unwrap();
    assert!(task.repo.contains("{{ git_repo_url }}"));
    assert!(task.path.contains("{{ app_name }}"));
    assert!(task.with.is_some());
    assert!(task.and.is_some());
}

#[test]
fn test_git_task_ssh_with_options() {
    let yaml = r#"
repo: git@github.com:private/secure-repo.git
path: /secure/location
branch: main
accept_keys: "yes"
ssh_options:
  IdentityFile: "/home/deploy/.ssh/id_rsa"
  UserKnownHostsFile: "/home/deploy/.ssh/known_hosts"
  StrictHostKeyChecking: "no"
"#;

    let task: Result<GitTask, _> = serde_yaml::from_str(yaml);
    assert!(task.is_ok());
    
    let task = task.unwrap();
    assert!(task.ssh_options.is_some());
    let opts = task.ssh_options.unwrap();
    assert_eq!(opts.len(), 3);
    assert_eq!(opts.get("IdentityFile"), Some(&"/home/deploy/.ssh/id_rsa".to_string()));
}