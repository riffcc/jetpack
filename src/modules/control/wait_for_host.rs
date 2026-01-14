// Jetpack
// Copyright (C) Riff Labs Limited <team@riff.cc>
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// at your option) any later version.

//! Wait for host to become available via SSH
//!
//! Useful after provisioning VMs/LXCs to wait for them to boot and SSH to be ready.

use crate::tasks::*;
use crate::handle::handle::{TaskHandle, CheckRc};
use serde::Deserialize;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

const MODULE: &str = "wait_for_host";

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct WaitForHostTask {
    pub name: Option<String>,
    /// Timeout in seconds (default: 300)
    pub timeout: Option<String>,
    /// Delay between retries in seconds (default: 5)
    pub delay: Option<String>,
    pub with: Option<PreLogicInput>,
    pub and: Option<PostLogicInput>,
}

struct WaitForHostAction {
    pub timeout: u64,
    pub delay: u64,
}

impl IsTask for WaitForHostTask {
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
        let timeout = match &self.timeout {
            Some(t) => handle
                .template
                .string_no_spaces(request, tm, &String::from("timeout"), t)?
                .parse::<u64>()
                .unwrap_or(300),
            None => 300,
        };
        let delay = match &self.delay {
            Some(d) => handle
                .template
                .string_no_spaces(request, tm, &String::from("delay"), d)?
                .parse::<u64>()
                .unwrap_or(5),
            None => 5,
        };

        Ok(EvaluatedTask {
            action: Arc::new(WaitForHostAction { timeout, delay }),
            with: Arc::new(PreLogicInput::template(handle, request, tm, &self.with)?),
            and: Arc::new(PostLogicInput::template(handle, request, tm, &self.and)?),
        })
    }
}

impl IsAction for WaitForHostAction {
    fn dispatch(
        &self,
        handle: &Arc<TaskHandle>,
        request: &Arc<TaskRequest>,
    ) -> Result<Arc<TaskResponse>, Arc<TaskResponse>> {
        match request.request_type {
            TaskRequestType::Query => {
                // Always need to execute - we're waiting for connectivity
                Ok(handle.response.needs_passive(request))
            }

            TaskRequestType::Passive => {
                let start = std::time::Instant::now();
                let timeout_duration = Duration::from_secs(self.timeout);
                let delay_duration = Duration::from_secs(self.delay);

                loop {
                    // Try a simple command to test connectivity
                    let result = handle.remote.run(request, &String::from("echo ok"), CheckRc::Unchecked);

                    match result {
                        Ok(response) => {
                            let (rc, _) = cmd_info(&response);
                            if rc == 0 {
                                // SSH is ready
                                return Ok(handle.response.is_passive(request));
                            }
                        }
                        Err(_) => {
                            // Connection failed, will retry
                        }
                    }

                    // Check timeout
                    if start.elapsed() >= timeout_duration {
                        return Err(handle.response.is_failed(
                            request,
                            &format!(
                                "Timeout waiting for host after {} seconds",
                                self.timeout
                            ),
                        ));
                    }

                    // Wait before retry
                    thread::sleep(delay_duration);
                }
            }

            _ => Err(handle.response.not_supported(request)),
        }
    }
}
