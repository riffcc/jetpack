use jetpack::tasks::request::*;
use jetpack::tasks::fields::Field;

#[test]
fn test_task_request_type_equality() {
    assert_eq!(TaskRequestType::Validate, TaskRequestType::Validate);
    assert_ne!(TaskRequestType::Query, TaskRequestType::Create);
    assert_ne!(TaskRequestType::Remove, TaskRequestType::Modify);
}

#[test]
fn test_sudo_details_creation() {
    let sudo_details = SudoDetails {
        user: Some("root".to_string()),
        template: "sudo -u {{ user }}".to_string(),
    };
    
    assert_eq!(sudo_details.user, Some("root".to_string()));
    assert_eq!(sudo_details.template, "sudo -u {{ user }}");
}

#[test]
fn test_sudo_details_clone() {
    let sudo_details = SudoDetails {
        user: Some("admin".to_string()),
        template: "sudo template".to_string(),
    };
    
    let cloned = sudo_details.clone();
    assert_eq!(cloned.user, sudo_details.user);
    assert_eq!(cloned.template, sudo_details.template);
}

#[test]
fn test_validate_request() {
    let request = TaskRequest::validate();
    assert_eq!(request.request_type, TaskRequestType::Validate);
    assert!(request.changes.is_empty());
    assert!(request.sudo_details.is_none());
}

#[test]
fn test_query_request() {
    let sudo_details = SudoDetails {
        user: Some("user1".to_string()),
        template: "sudo -u {{ user }}".to_string(),
    };
    
    let request = TaskRequest::query(&sudo_details);
    assert_eq!(request.request_type, TaskRequestType::Query);
    assert!(request.changes.is_empty());
    assert!(request.sudo_details.is_some());
    
    let sudo = request.sudo_details.as_ref().unwrap();
    assert_eq!(sudo.user, Some("user1".to_string()));
    assert_eq!(sudo.template, "sudo -u {{ user }}");
}

#[test]
fn test_create_request() {
    let sudo_details = SudoDetails {
        user: Some("user2".to_string()),
        template: "sudo template".to_string(),
    };
    
    let request = TaskRequest::create(&sudo_details);
    assert_eq!(request.request_type, TaskRequestType::Create);
    assert!(request.changes.is_empty());
    assert!(request.sudo_details.is_some());
    assert_eq!(request.sudo_details.as_ref().unwrap().user, Some("user2".to_string()));
}

#[test]
fn test_remove_request() {
    let sudo_details = SudoDetails {
        user: Some("user3".to_string()),
        template: "sudo template".to_string(),
    };
    
    let request = TaskRequest::remove(&sudo_details);
    assert_eq!(request.request_type, TaskRequestType::Remove);
    assert!(request.changes.is_empty());
    assert!(request.sudo_details.is_some());
}

#[test]
fn test_modify_request() {
    let sudo_details = SudoDetails {
        user: Some("user4".to_string()),
        template: "sudo template".to_string(),
    };
    
    let changes = vec![Field::Owner, Field::Mode, Field::Content];
    
    let request = TaskRequest::modify(&sudo_details, changes.clone());
    assert_eq!(request.request_type, TaskRequestType::Modify);
    assert_eq!(request.changes.len(), 3);
    assert_eq!(request.changes[0], Field::Owner);
    assert_eq!(request.changes[1], Field::Mode);
    assert_eq!(request.changes[2], Field::Content);
    assert!(request.sudo_details.is_some());
}

#[test]
fn test_execute_request() {
    let sudo_details = SudoDetails {
        user: Some("user5".to_string()),
        template: "sudo template".to_string(),
    };
    
    let request = TaskRequest::execute(&sudo_details);
    assert_eq!(request.request_type, TaskRequestType::Execute);
    assert!(request.changes.is_empty());
    assert!(request.sudo_details.is_some());
}

#[test]
fn test_passive_request() {
    let sudo_details = SudoDetails {
        user: Some("user6".to_string()),
        template: "sudo template".to_string(),
    };
    
    let request = TaskRequest::passive(&sudo_details);
    assert_eq!(request.request_type, TaskRequestType::Passive);
    assert!(request.changes.is_empty());
    assert!(request.sudo_details.is_some());
}

#[test]
fn test_is_sudoing_with_user() {
    let sudo_details = SudoDetails {
        user: Some("root".to_string()),
        template: "sudo template".to_string(),
    };
    
    let request = TaskRequest::query(&sudo_details);
    assert!(request.is_sudoing());
}

#[test]
fn test_is_sudoing_without_user() {
    let sudo_details = SudoDetails {
        user: None,
        template: "sudo template".to_string(),
    };
    
    let request = TaskRequest::query(&sudo_details);
    assert!(!request.is_sudoing());
}

#[test]
fn test_is_sudoing_no_sudo_details() {
    let request = TaskRequest::validate();
    assert!(!request.is_sudoing());
}

#[test]
fn test_all_task_request_types() {
    // Ensure all variants can be created
    let types = vec![
        TaskRequestType::Validate,
        TaskRequestType::Query,
        TaskRequestType::Create,
        TaskRequestType::Remove,
        TaskRequestType::Modify,
        TaskRequestType::Execute,
        TaskRequestType::Passive,
    ];
    
    // Test that each type is unique
    for (i, type1) in types.iter().enumerate() {
        for (j, type2) in types.iter().enumerate() {
            if i == j {
                assert_eq!(type1, type2);
            } else {
                assert_ne!(type1, type2);
            }
        }
    }
}