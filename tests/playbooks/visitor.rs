use jetpack::playbooks::visitor::*;

#[test]
fn test_check_mode_enum() {
    assert_ne!(CheckMode::Yes, CheckMode::No);
    assert_eq!(CheckMode::Yes, CheckMode::Yes);
    assert_eq!(CheckMode::No, CheckMode::No);
}

#[test]
fn test_check_mode_debug() {
    let check_yes = CheckMode::Yes;
    let check_no = CheckMode::No;
    
    assert_eq!(format!("{:?}", check_yes), "Yes");
    assert_eq!(format!("{:?}", check_no), "No");
}

#[test]
fn test_playbook_visitor_new() {
    let visitor = PlaybookVisitor::new(CheckMode::No);
    assert_eq!(visitor.play_count, 0);
    assert_eq!(visitor.role_count, 0);
    assert_eq!(visitor.task_count, 0);
    assert_eq!(visitor.change_count, 0);
    assert_eq!(visitor.failed_count, 0);
    assert_eq!(visitor.skipped_count, 0);
    assert_eq!(visitor.notified_hosts.len(), 0);
}

#[test]
fn test_playbook_visitor_with_check_mode() {
    let visitor = PlaybookVisitor::new(CheckMode::Yes);
    assert_eq!(visitor.play_count, 0);
    assert_eq!(visitor.check_mode, CheckMode::Yes);
}

#[test]
fn test_playbook_visitor_initial_state() {
    let visitor = PlaybookVisitor::new(CheckMode::No);
    
    // Verify all counters start at zero
    assert_eq!(visitor.play_count, 0);
    assert_eq!(visitor.role_count, 0);
    assert_eq!(visitor.task_count, 0);
    assert_eq!(visitor.change_count, 0);
    assert_eq!(visitor.failed_count, 0);
    assert_eq!(visitor.skipped_count, 0);
    
    // Verify collections are empty
    assert!(visitor.notified_hosts.is_empty());
    
    // Verify check mode
    assert_eq!(visitor.check_mode, CheckMode::No);
}