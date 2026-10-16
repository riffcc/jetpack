// Jetporch
// Copyright (C) 2023 - Michael DeHaan <michael@michaeldehaan.net> + contributors
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// long with this program.  If not, see <http://www.gnu.org/licenses/>.

use std::sync::Arc;
//use std::collections::HashMap;
use crate::connection::command::CommandResult;
use crate::tasks::fields::Field;
use crate::tasks::logic::{PostLogicEvaluated, PreLogicEvaluated};
use std::vec::Vec;

// task responses are returns from module calls - they are not
// created directly but by helper functions in handle.rs, see
// the various modules for examples/usage

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum TaskStatus {
    IsCreated,
    IsRemoved,
    IsModified,
    IsExecuted,
    IsPassive,
    IsMatched,
    IsSkipped,
    IsDeferred,
    NeedsCreation,
    NeedsRemoval,
    NeedsModification,
    NeedsExecution,
    NeedsPassive,
    Failed,
}

#[derive(Debug, Clone)]
pub struct TaskResponse {
    pub status: TaskStatus,
    /// True when this result contains an unknown plan outcome, including mixed loops.
    pub deferred: bool,
    pub changes: Vec<Field>,
    pub msg: Option<String>,
    pub command_result: Arc<Option<CommandResult>>,
    pub with: Arc<Option<PreLogicEvaluated>>,
    pub and: Arc<Option<PostLogicEvaluated>>,
}

//impl TaskResponse {
//}

/// Reduce loop outcomes without changing item execution or saved-result semantics.
/// Known changes dominate unchanged items; uncertainty is retained independently.
pub(crate) fn aggregate(
    previous: Option<Arc<TaskResponse>>,
    next: Arc<TaskResponse>,
) -> Arc<TaskResponse> {
    let Some(previous) = previous else {
        return next;
    };
    use TaskStatus::*;
    fn planned(status: TaskStatus) -> bool {
        matches!(
            status,
            NeedsCreation | NeedsRemoval | NeedsModification | NeedsExecution
        )
    }
    fn changed(status: TaskStatus) -> bool {
        matches!(status, IsCreated | IsRemoved | IsModified | IsExecuted)
    }
    let (a, b) = (previous.status, next.status);
    let status = if a == Failed || b == Failed {
        Failed
    } else if planned(a) && planned(b) {
        if a == b { a } else { NeedsModification }
    } else if planned(a) {
        a
    } else if planned(b) {
        b
    } else if changed(a) && changed(b) {
        if a == b { a } else { IsModified }
    } else if changed(a) {
        a
    } else if changed(b) {
        b
    } else if a == IsDeferred || b == IsDeferred {
        IsDeferred
    } else if a == IsMatched || b == IsMatched {
        IsMatched
    } else if a == IsPassive || b == IsPassive {
        IsPassive
    } else {
        IsSkipped
    };
    let deferred = previous.deferred || next.deferred;
    let mut result = if status == next.status {
        (*next).clone()
    } else {
        (*previous).clone()
    };
    result.status = status;
    result.deferred = deferred;
    result.changes = previous.changes.clone();
    for field in &next.changes {
        if !result.changes.contains(field) {
            result.changes.push(*field);
        }
    }
    result.changes.sort_by_key(|f| format!("{f:?}"));
    // A final-item command result would falsely describe the whole loop.
    if status != Failed {
        result.command_result = Arc::new(None);
    }
    if deferred {
        result.msg = Some("one or more items depend on output unavailable until apply".into());
    }
    Arc::new(result)
}

#[cfg(test)]
mod aggregate_tests {
    use super::*;
    fn response(status: TaskStatus, changes: Vec<Field>) -> Arc<TaskResponse> {
        Arc::new(TaskResponse {
            status,
            deferred: status == TaskStatus::IsDeferred,
            changes,
            msg: None,
            command_result: Arc::new(None),
            with: Arc::new(None),
            and: Arc::new(None),
        })
    }
    #[test]
    fn status_and_field_reduction_is_order_independent_and_retains_uncertainty() {
        use TaskStatus::*;
        let outcomes = [
            (NeedsModification, vec![Field::Mode]),
            (IsMatched, vec![]),
            (IsSkipped, vec![]),
            (IsDeferred, vec![]),
        ];
        for order in [[0, 1, 2, 3], [3, 2, 1, 0], [1, 3, 0, 2]] {
            let mut result = None;
            for index in order {
                result = Some(aggregate(
                    result,
                    response(outcomes[index].0, outcomes[index].1.clone()),
                ));
            }
            let result = result.unwrap();
            assert_eq!(result.status, NeedsModification);
            assert_eq!(result.changes, vec![Field::Mode]);
            assert!(result.deferred);
        }
        let result = aggregate(
            Some(response(IsModified, vec![Field::Owner])),
            response(IsModified, vec![Field::Mode, Field::Owner]),
        );
        assert_eq!(result.changes, vec![Field::Mode, Field::Owner]);
        assert_eq!(
            aggregate(
                Some(response(NeedsCreation, vec![])),
                response(NeedsRemoval, vec![])
            )
            .status,
            NeedsModification
        );
        assert_eq!(
            aggregate(
                Some(response(IsCreated, vec![])),
                response(IsRemoved, vec![])
            )
            .status,
            IsModified
        );
        assert_eq!(
            aggregate(Some(response(Failed, vec![])), response(IsMatched, vec![])).status,
            Failed
        );
    }
}
