// Jetpack
// Copyright (C) Riff Labs Limited <team@riff.cc>
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// at your option) any later version.

//! Terminal UI for async (host-parallel) execution mode.
//!
//! Color-coded per-host output in chronological order as events arrive.
//! In non-TTY mode (pipe, CI), outputs simple `[hostname] message` lines.

use std::io::{self, Write};
use std::sync::mpsc;

/// Color palette for host prefixes. 16 colors cycling.
const HOST_COLORS: &[&str] = &[
    "\x1b[34m",   // blue
    "\x1b[32m",   // green
    "\x1b[36m",   // cyan
    "\x1b[33m",   // yellow
    "\x1b[35m",   // magenta
    "\x1b[91m",   // bright red
    "\x1b[92m",   // bright green
    "\x1b[93m",   // bright yellow
    "\x1b[94m",   // bright blue
    "\x1b[95m",   // bright magenta
    "\x1b[96m",   // bright cyan
    "\x1b[31m",   // red
    "\x1b[37m",   // white
    "\x1b[90m",   // bright black / gray
    "\x1b[97m",   // bright white
    "\x1b[38;5;208m", // orange
];

const RESET: &str = "\x1b[0m";

/// Events sent from Rayon worker threads to the UI thread.
#[derive(Debug, Clone)]
pub enum HostEvent {
    TaskStarted {
        host_idx: usize,
        task_name: String,
    },
    TaskCompleted {
        host_idx: usize,
        task_name: String,
        status: TaskDisplayStatus,
        output: Option<String>,
    },
    TaskFailed {
        host_idx: usize,
        task_name: String,
        error: String,
    },
    BarrierReached {
        host_idx: usize,
        barrier_name: String,
    },
    BarrierPassed {
        host_idx: usize,
    },
    BarrierFailed {
        host_idx: usize,
        error: String,
    },
    HostCompleted {
        host_idx: usize,
    },
    HostFailed {
        host_idx: usize,
        error: String,
    },
    /// All work is done — UI thread should exit.
    AllDone,
}

/// Simplified task status for display purposes.
#[derive(Debug, Clone, PartialEq)]
pub enum TaskDisplayStatus {
    Ok,
    Changed,
    Skipped,
}

impl TaskDisplayStatus {
    pub fn symbol(&self) -> &'static str {
        match self {
            TaskDisplayStatus::Ok => "\x1b[32m✓\x1b[0m",
            TaskDisplayStatus::Changed => "\x1b[33m✓\x1b[0m",
            TaskDisplayStatus::Skipped => "\x1b[90m⊘\x1b[0m",
        }
    }

    pub fn plain_symbol(&self) -> &'static str {
        match self {
            TaskDisplayStatus::Ok => "OK",
            TaskDisplayStatus::Changed => "CHANGED",
            TaskDisplayStatus::Skipped => "SKIPPED",
        }
    }
}

/// Per-host output record for summary.
struct HostOutput {
    hostname: String,
    color_idx: usize,
    task_count: usize,
    completed: bool,
    failed: bool,
}

/// Async UI renderer. Spawned as a thread that reads from the event channel.
pub struct AsyncUi {
    hostnames: Vec<String>,
    is_tty: bool,
}

impl AsyncUi {
    pub fn new(hostnames: Vec<String>) -> Self {
        // Simple TTY check: see if TERM is set and not "dumb"
        let is_tty = std::env::var("TERM")
            .map(|t| t != "dumb")
            .unwrap_or(false);
        AsyncUi { hostnames, is_tty }
    }

    /// Create an event channel. Returns (sender for workers, receiver for UI).
    pub fn channel() -> (mpsc::Sender<HostEvent>, mpsc::Receiver<HostEvent>) {
        mpsc::channel()
    }

    /// Run the UI loop. Consumes events until `AllDone` is received.
    /// This blocks the calling thread.
    ///
    /// IMPORTANT: stdout is locked per-event, NOT held across recv() calls.
    /// Holding stdout across recv() deadlocks worker threads that call println!()
    /// in visitor callbacks.
    pub fn run(&self, rx: mpsc::Receiver<HostEvent>) {
        let mut outputs: Vec<HostOutput> = self
            .hostnames
            .iter()
            .enumerate()
            .map(|(i, name)| HostOutput {
                hostname: name.clone(),
                color_idx: i % HOST_COLORS.len(),
                task_count: 0,
                completed: false,
                failed: false,
            })
            .collect();

        // Print header (brief stdout lock)
        {
            let stdout = io::stdout();
            let mut out = stdout.lock();
            if self.is_tty {
                let _ = writeln!(out, "── Async Mode: {} hosts ──────────────", self.hostnames.len());
                for (i, name) in self.hostnames.iter().enumerate() {
                    let color = HOST_COLORS[i % HOST_COLORS.len()];
                    let _ = write!(out, "  {}●{} {} ", color, RESET, name);
                }
                let _ = writeln!(out);
                let _ = writeln!(out, "──────────────────────────────────────");
            } else {
                let _ = writeln!(out, "# Async Mode: {} hosts", self.hostnames.len());
            }
            let _ = out.flush();
        } // stdout lock released before event loop

        for event in rx.iter() {
            match event {
                HostEvent::AllDone => break,

                HostEvent::TaskStarted {
                    host_idx,
                    task_name,
                } => {
                    if host_idx < outputs.len() {
                        self.print_line(&outputs[host_idx], &format!("● {}", task_name));
                    }
                }

                HostEvent::TaskCompleted {
                    host_idx,
                    task_name,
                    status,
                    output,
                } => {
                    if host_idx < outputs.len() {
                        let line = match output {
                            Some(ref msg) if !msg.is_empty() => {
                                if self.is_tty {
                                    format!("{} {} — {}", status.symbol(), task_name, msg)
                                } else {
                                    format!("{} {} — {}", status.plain_symbol(), task_name, msg)
                                }
                            }
                            _ => {
                                if self.is_tty {
                                    format!("{} {}", status.symbol(), task_name)
                                } else {
                                    format!("{} {}", status.plain_symbol(), task_name)
                                }
                            }
                        };
                        self.print_line(&outputs[host_idx], &line);
                        outputs[host_idx].task_count += 1;
                    }
                }

                HostEvent::TaskFailed {
                    host_idx,
                    task_name,
                    error,
                } => {
                    if host_idx < outputs.len() {
                        let line = if self.is_tty {
                            format!("\x1b[31m✗\x1b[0m {} — {}", task_name, error)
                        } else {
                            format!("FAILED {} — {}", task_name, error)
                        };
                        self.print_line(&outputs[host_idx], &line);
                        outputs[host_idx].task_count += 1;
                        outputs[host_idx].failed = true;
                    }
                }

                HostEvent::BarrierReached {
                    host_idx,
                    barrier_name,
                } => {
                    if host_idx < outputs.len() {
                        let line = format!("⏳ Waiting at barrier \"{}\"", barrier_name);
                        self.print_line(&outputs[host_idx], &line);
                    }
                }

                HostEvent::BarrierPassed { host_idx } => {
                    if host_idx < outputs.len() {
                        self.print_line(&outputs[host_idx], "↩ Barrier passed");
                    }
                }

                HostEvent::BarrierFailed { host_idx, error } => {
                    if host_idx < outputs.len() {
                        let line = if self.is_tty {
                            format!("\x1b[31m✗ Barrier failed: {}\x1b[0m", error)
                        } else {
                            format!("BARRIER FAILED: {}", error)
                        };
                        self.print_line(&outputs[host_idx], &line);
                        outputs[host_idx].failed = true;
                    }
                }

                HostEvent::HostCompleted { host_idx } => {
                    if host_idx < outputs.len() {
                        let line = if self.is_tty {
                            "\x1b[32m✓ All tasks complete\x1b[0m".to_string()
                        } else {
                            "COMPLETE".to_string()
                        };
                        self.print_line(&outputs[host_idx], &line);
                        outputs[host_idx].completed = true;
                    }
                }

                HostEvent::HostFailed { host_idx, error } => {
                    if host_idx < outputs.len() {
                        let line = if self.is_tty {
                            format!("\x1b[31m✗ Host failed: {}\x1b[0m", error)
                        } else {
                            format!("HOST FAILED: {}", error)
                        };
                        self.print_line(&outputs[host_idx], &line);
                        outputs[host_idx].failed = true;
                    }
                }
            }
        }

        // Print summary (brief stdout lock)
        {
            let stdout = io::stdout();
            let mut out = stdout.lock();
            let _ = writeln!(out);
            if self.is_tty {
                let _ = writeln!(out, "── Async Summary ──────────────────");
            } else {
                let _ = writeln!(out, "# Async Summary");
            }

            for ho in &outputs {
                let status_text = if ho.failed {
                    if self.is_tty { "\x1b[31mFAILED\x1b[0m" } else { "FAILED" }
                } else if ho.completed {
                    if self.is_tty { "\x1b[32mOK\x1b[0m" } else { "OK" }
                } else {
                    if self.is_tty { "\x1b[33mINCOMPLETE\x1b[0m" } else { "INCOMPLETE" }
                };

                if self.is_tty {
                    let color = HOST_COLORS[ho.color_idx];
                    let _ = writeln!(
                        out,
                        "  {}{}{} — {} ({} tasks)",
                        color, ho.hostname, RESET, status_text, ho.task_count
                    );
                } else {
                    let _ = writeln!(
                        out,
                        "  {} — {} ({} tasks)",
                        ho.hostname, status_text, ho.task_count
                    );
                }
            }

            if self.is_tty {
                let _ = writeln!(out, "───────────────────────────────────");
            }
            let _ = out.flush();
        }
    }

    /// Print a line with host prefix. Briefly locks stdout per call.
    fn print_line(&self, host: &HostOutput, line: &str) {
        let stdout = io::stdout();
        let mut out = stdout.lock();
        if self.is_tty {
            let color = HOST_COLORS[host.color_idx];
            let _ = writeln!(out, "{}[{}]{} {}", color, host.hostname, RESET, line);
        } else {
            let _ = writeln!(out, "[{}] {}", host.hostname, line);
        }
        let _ = out.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_task_display_status_symbol() {
        assert!(!TaskDisplayStatus::Ok.symbol().is_empty());
        assert!(!TaskDisplayStatus::Changed.symbol().is_empty());
        assert!(!TaskDisplayStatus::Skipped.symbol().is_empty());
    }

    #[test]
    fn test_task_display_status_plain() {
        assert_eq!(TaskDisplayStatus::Ok.plain_symbol(), "OK");
        assert_eq!(TaskDisplayStatus::Changed.plain_symbol(), "CHANGED");
        assert_eq!(TaskDisplayStatus::Skipped.plain_symbol(), "SKIPPED");
    }

    #[test]
    fn test_host_event_channel() {
        let (tx, rx) = AsyncUi::channel();
        tx.send(HostEvent::AllDone).unwrap();
        match rx.recv().unwrap() {
            HostEvent::AllDone => {}
            _ => panic!("expected AllDone"),
        }
    }

    #[test]
    fn test_host_colors_not_empty() {
        assert!(HOST_COLORS.len() >= 8);
    }
}
