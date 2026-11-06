use jetpack::cli::parser::CliParser;
use jetpack::playbooks::{
    context::PlaybookContext,
    visitor::{CheckMode, PlaybookVisitor},
};
use std::sync::{Arc, RwLock};

#[test]
fn visitor_keeps_check_mode_and_unique_run_identity() {
    let first = PlaybookVisitor::new(CheckMode::Yes);
    let second = PlaybookVisitor::new(CheckMode::No);
    assert!(first.check_mode == CheckMode::Yes);
    assert!(second.check_mode == CheckMode::No);
    assert_ne!(first.run_id, second.run_id);
    assert!(first.output_handler.is_none());
}

#[test]
fn log_entry_captures_playbook_and_task_context() {
    let visitor = PlaybookVisitor::new(CheckMode::No);
    let mut context = PlaybookContext::new(&CliParser::new());
    context.playbook_path = Some("playbooks/site.yml".into());
    context.play = Some("services".into());
    context.task = Some("install package".into());
    context.task_count = 7;
    let entry = visitor.log_entry("task_start", Arc::new(RwLock::new(context)));
    assert_eq!(entry.event, "task_start");
    assert_eq!(entry.playbook_path.as_deref(), Some("playbooks/site.yml"));
    assert_eq!(entry.play.as_deref(), Some("services"));
    assert_eq!(entry.task.as_deref(), Some("install package"));
    assert_eq!(entry.task_ct, Some(7));
    assert!(entry.cmd.is_none());
    assert!(entry.host.is_none());
}
