use jetpack::connection::{connection::Connection, local::LocalConnection};
use jetpack::handle::handle::{CheckRc, TaskHandle};
use std::sync::{Arc, Mutex};

#[test]
fn handle_checked_and_unchecked_commands_share_the_connection() {
    let run = crate::common::create_test_run_state();
    let host = run.inventory.read().unwrap().get_host("localhost");
    let connection: Arc<Mutex<dyn Connection>> = Arc::new(Mutex::new(LocalConnection::new(&host)));
    let handle = TaskHandle::new(run, connection, host);
    let request = crate::common::query_request();
    assert!(
        handle
            .remote
            .run(&request, "exit 7", CheckRc::Checked)
            .is_err()
    );
    let result = handle
        .remote
        .run(&request, "exit 7", CheckRc::Unchecked)
        .unwrap();
    assert_eq!(result.command_result.as_ref().as_ref().unwrap().rc, 7);
}
