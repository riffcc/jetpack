// Jetpack
// Copyright (C) Riff Labs Limited <team@riff.cc>
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// at your option) any later version.

//! AsyncExecutionContext — pre-scans the flattened task list and creates
//! barriers for each `wait_for_others` task.

use crate::playbooks::barrier::{BarrierMode, CountdownBarrier};
use crate::registry::list::Task;
use std::collections::HashMap;
use std::sync::Arc;

/// Context for async (host-parallel) execution. Maps task indices to barriers.
pub struct AsyncExecutionContext {
    /// One barrier per `wait_for_others` task.
    pub barriers: Vec<Arc<CountdownBarrier>>,
    /// Maps task index → barrier index.
    pub barrier_map: HashMap<usize, usize>,
}

impl AsyncExecutionContext {
    /// Build the async context by scanning the task list for barrier points.
    ///
    /// `tasks` is the flattened task list (role tasks + loose tasks in order).
    /// `host_count` is the number of hosts that will participate.
    pub fn from_tasks(tasks: &[&Task], host_count: usize) -> Self {
        let mut barriers = Vec::new();
        let mut barrier_map = HashMap::new();

        for (idx, task) in tasks.iter().enumerate() {
            if task.is_wait_for_others() {
                let mode = if task.is_wait_for_others_strict() {
                    BarrierMode::Strict
                } else {
                    BarrierMode::Loose
                };

                let name = task.get_display_name();
                let barrier = Arc::new(CountdownBarrier::new(host_count, mode, name));
                let barrier_idx = barriers.len();
                barriers.push(barrier);
                barrier_map.insert(idx, barrier_idx);
            }
        }

        AsyncExecutionContext {
            barriers,
            barrier_map,
        }
    }

    /// Get the barrier for a given task index, if one exists.
    pub fn get_barrier(&self, task_idx: usize) -> Option<&Arc<CountdownBarrier>> {
        self.barrier_map
            .get(&task_idx)
            .map(|&bi| &self.barriers[bi])
    }

    /// Withdraw a host from all barriers at or after the given task index.
    /// Called when a host fails mid-execution.
    pub fn withdraw_from(&self, from_task_idx: usize) {
        for (&task_idx, &barrier_idx) in &self.barrier_map {
            if task_idx >= from_task_idx {
                // Ignore errors — the barrier may already be released or all-withdrawn.
                let _ = self.barriers[barrier_idx].withdraw();
            }
        }
    }

    /// Total number of barrier sync points.
    pub fn barrier_count(&self) -> usize {
        self.barriers.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::control::wait_for_others::WaitForOthersTask;

    fn make_wait_task(name: &str, mode: Option<&str>) -> Task {
        Task::Wait_For_Others(WaitForOthersTask {
            name: Some(name.to_string()),
            mode: mode.map(|s| s.to_string()),
            with: None,
            and: None,
        })
    }

    fn make_echo_task() -> Task {
        use crate::modules::control::echo::EchoTask;
        Task::Echo(EchoTask {
            name: Some("test echo".into()),
            msg: "hello".into(),
            with: None,
            and: None,
        })
    }

    #[test]
    fn test_from_tasks_no_barriers() {
        let t1 = make_echo_task();
        let t2 = make_echo_task();
        let tasks: Vec<&Task> = vec![&t1, &t2];

        let ctx = AsyncExecutionContext::from_tasks(&tasks, 3);
        assert_eq!(ctx.barrier_count(), 0);
        assert!(ctx.get_barrier(0).is_none());
        assert!(ctx.get_barrier(1).is_none());
    }

    #[test]
    fn test_from_tasks_with_barriers() {
        let t1 = make_echo_task();
        let t2 = make_wait_task("sync1", None);
        let t3 = make_echo_task();
        let t4 = make_wait_task("sync2", Some("strict"));
        let tasks: Vec<&Task> = vec![&t1, &t2, &t3, &t4];

        let ctx = AsyncExecutionContext::from_tasks(&tasks, 5);
        assert_eq!(ctx.barrier_count(), 2);
        assert!(ctx.get_barrier(0).is_none());
        assert!(ctx.get_barrier(1).is_some());
        assert!(ctx.get_barrier(2).is_none());
        assert!(ctx.get_barrier(3).is_some());

        // Check modes
        assert_eq!(ctx.get_barrier(1).unwrap().mode(), BarrierMode::Loose);
        assert_eq!(ctx.get_barrier(3).unwrap().mode(), BarrierMode::Strict);

        // Check expected counts
        assert_eq!(ctx.get_barrier(1).unwrap().expected_count(), 5);
    }

    #[test]
    fn test_withdraw_from() {
        let t1 = make_echo_task();
        let t2 = make_wait_task("b1", None);
        let t3 = make_echo_task();
        let t4 = make_wait_task("b2", None);
        let tasks: Vec<&Task> = vec![&t1, &t2, &t3, &t4];

        let ctx = AsyncExecutionContext::from_tasks(&tasks, 3);

        // Withdraw from task 2 onwards (barrier at idx 1 and 3)
        ctx.withdraw_from(2);

        // Barrier at idx 1 should be unaffected (before from_task_idx=2)
        assert_eq!(ctx.get_barrier(1).unwrap().expected_count(), 3);
        // Barrier at idx 3 should have one less
        assert_eq!(ctx.get_barrier(3).unwrap().expected_count(), 2);
    }
}
