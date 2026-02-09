// Jetpack
// Copyright (C) Riff Labs Limited <team@riff.cc>
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// at your option) any later version.

//! `wait_for_others` — barrier synchronization point for async mode.
//!
//! In `--async` mode, each host runs its task list independently. This module
//! inserts an explicit synchronization point where all hosts must arrive before
//! any can proceed past the barrier.
//!
//! In non-async (default) mode, this task is silently skipped — the implicit
//! per-task barrier already synchronizes all hosts.
//!
//! YAML syntax:
//! ```yaml
//! - wait_for_others:
//!   name: All nodes have rqlite installed
//!   mode: loose   # or "strict" — default is "loose"
//! ```
//!
//! Modes:
//! - `loose` (default): if a host fails before reaching the barrier, remaining
//!   hosts proceed without it.
//! - `strict`: if any host fails to reach the barrier, all waiting hosts get
//!   an error. Use when ALL hosts must succeed to continue.

use crate::tasks::*;
use crate::handle::handle::TaskHandle;
use serde::Deserialize;
use std::sync::Arc;

const MODULE: &str = "wait_for_others";

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct WaitForOthersTask {
    pub name: Option<String>,
    /// Barrier mode: "strict" or "loose" (default: "loose")
    pub mode: Option<String>,
    pub with: Option<PreLogicInput>,
    pub and: Option<PostLogicInput>,
}

pub struct WaitForOthersAction {
    pub name: String,
    pub strict: bool,
}

impl IsTask for WaitForOthersTask {
    fn get_module(&self) -> String {
        String::from(MODULE)
    }
    fn get_name(&self) -> Option<String> {
        self.name.clone()
    }
    fn get_with(&self) -> Option<PreLogicInput> {
        self.with.clone()
    }

    fn evaluate(
        &self,
        handle: &Arc<TaskHandle>,
        request: &Arc<TaskRequest>,
        tm: TemplateMode,
    ) -> Result<EvaluatedTask, Arc<TaskResponse>> {
        let strict = match &self.mode {
            Some(m) => {
                let rendered = handle.template.string_unsafe_for_shell(
                    request,
                    tm,
                    &String::from("mode"),
                    m,
                )?;
                rendered.eq_ignore_ascii_case("strict")
            }
            None => false,
        };

        Ok(EvaluatedTask {
            action: Arc::new(WaitForOthersAction {
                name: self.name.clone().unwrap_or_else(|| String::from(MODULE)),
                strict,
            }),
            with: Arc::new(PreLogicInput::template(handle, request, tm, &self.with)?),
            and: Arc::new(PostLogicInput::template(handle, request, tm, &self.and)?),
        })
    }
}

impl IsAction for WaitForOthersAction {
    fn dispatch(
        &self,
        handle: &Arc<TaskHandle>,
        request: &Arc<TaskRequest>,
    ) -> Result<Arc<TaskResponse>, Arc<TaskResponse>> {
        match request.request_type {
            TaskRequestType::Query => Ok(handle.response.needs_passive(request)),

            TaskRequestType::Passive => {
                // The actual barrier synchronization is handled by the async
                // execution loop, not by this module. This module is passive —
                // it just signals "I am a barrier task" and returns success.
                //
                // In non-async mode, this task is skipped entirely.
                Ok(handle.response.is_passive(request))
            }

            _ => Err(handle.response.not_supported(request)),
        }
    }
}
