// Jetpack
// Copyright (C) Riff Labs Limited <team@riff.cc>
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// at your option) any later version.

//! CountdownBarrier — a thread-safe synchronization barrier that supports
//! dynamic withdrawal for failed hosts.
//!
//! Unlike `std::sync::Barrier`, a host that fails and never reaches the
//! barrier won't deadlock all other waiting threads. Instead, it calls
//! `withdraw()` to decrement the expected count, unblocking waiters if
//! the threshold is now met.
//!
//! Two modes:
//! - **Loose** (default): withdrawn hosts are tolerated. Barrier passes
//!   as long as remaining hosts arrive.
//! - **Strict**: if any host withdraws, the barrier returns an error to
//!   all waiters. Use this when ALL hosts must succeed.

use std::fmt;
use std::sync::{Condvar, Mutex};

/// Barrier mode controlling behavior when hosts withdraw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarrierMode {
    /// Barrier passes with remaining hosts. Default.
    Loose,
    /// Barrier fails if any host withdraws.
    Strict,
}

impl Default for BarrierMode {
    fn default() -> Self {
        BarrierMode::Loose
    }
}

/// Errors that can occur when waiting on a barrier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BarrierError {
    /// All hosts have withdrawn — nobody left to sync with.
    AllWithdrawn,
    /// A host withdrew while in strict mode.
    StrictWithdrawal,
}

impl fmt::Display for BarrierError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BarrierError::AllWithdrawn => write!(f, "all hosts have withdrawn from barrier"),
            BarrierError::StrictWithdrawal => write!(f, "host withdrew from strict barrier"),
        }
    }
}

struct BarrierState {
    expected: usize,
    arrived: usize,
    generation: u64,
    withdrawn: usize,
    poisoned: bool,
}

/// A countdown barrier that supports dynamic withdrawal.
pub struct CountdownBarrier {
    state: Mutex<BarrierState>,
    cv: Condvar,
    mode: BarrierMode,
    name: String,
}

impl CountdownBarrier {
    /// Create a new barrier expecting `count` participants.
    pub fn new(count: usize, mode: BarrierMode, name: String) -> Self {
        CountdownBarrier {
            state: Mutex::new(BarrierState {
                expected: count,
                arrived: 0,
                generation: 0,
                withdrawn: 0,
                poisoned: false,
            }),
            cv: Condvar::new(),
            mode,
            name,
        }
    }

    /// Get the barrier's name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get the barrier's mode.
    pub fn mode(&self) -> BarrierMode {
        self.mode
    }

    /// Wait at the barrier. Blocks until all expected participants arrive
    /// or enough have withdrawn that the threshold is met.
    ///
    /// Returns `Ok(())` on success, `Err` if all hosts withdrew or strict
    /// mode was violated.
    pub fn wait(&self) -> Result<(), BarrierError> {
        let mut state = self.state.lock().unwrap();
        let my_generation = state.generation;

        // Check for poison (strict mode violation)
        if state.poisoned {
            return Err(BarrierError::StrictWithdrawal);
        }

        // Check degenerate case: nobody expected
        if state.expected == 0 {
            return Err(BarrierError::AllWithdrawn);
        }

        state.arrived += 1;

        // Are we the last one? If so, bump generation and wake everyone.
        if state.arrived >= state.expected {
            state.generation += 1;
            state.arrived = 0;
            self.cv.notify_all();
            return Ok(());
        }

        // Wait until generation changes (meaning barrier was released)
        // or until the barrier becomes poisoned/all-withdrawn.
        while state.generation == my_generation && !state.poisoned && state.expected > 0 {
            state = self.cv.wait(state).unwrap();
        }

        if state.poisoned {
            Err(BarrierError::StrictWithdrawal)
        } else if state.expected == 0 {
            Err(BarrierError::AllWithdrawn)
        } else {
            Ok(())
        }
    }

    /// Withdraw a participant from the barrier. Called when a host fails
    /// and will never reach the barrier.
    ///
    /// In loose mode: decrements expected count, potentially releasing waiters.
    /// In strict mode: poisons the barrier, waking all waiters with an error.
    pub fn withdraw(&self) -> Result<(), BarrierError> {
        let mut state = self.state.lock().unwrap();
        state.withdrawn += 1;

        match self.mode {
            BarrierMode::Strict => {
                // Poison the barrier — everyone gets an error.
                state.poisoned = true;
                self.cv.notify_all();
                Err(BarrierError::StrictWithdrawal)
            }
            BarrierMode::Loose => {
                if state.expected > 0 {
                    state.expected -= 1;
                }

                if state.expected == 0 {
                    // Nobody left — wake anyone waiting so they get AllWithdrawn.
                    self.cv.notify_all();
                    return Err(BarrierError::AllWithdrawn);
                }

                // Check if the reduced expected count means we can release.
                if state.arrived >= state.expected {
                    state.generation += 1;
                    state.arrived = 0;
                    self.cv.notify_all();
                }

                Ok(())
            }
        }
    }

    /// Get the number of hosts that have withdrawn.
    pub fn withdrawn_count(&self) -> usize {
        self.state.lock().unwrap().withdrawn
    }

    /// Get the current expected count.
    pub fn expected_count(&self) -> usize {
        self.state.lock().unwrap().expected
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn test_basic_barrier_three_threads() {
        let barrier = Arc::new(CountdownBarrier::new(3, BarrierMode::Loose, "test".into()));
        let mut handles = Vec::new();

        for _ in 0..3 {
            let b = Arc::clone(&barrier);
            handles.push(thread::spawn(move || {
                b.wait().unwrap();
            }));
        }

        for h in handles {
            h.join().unwrap();
        }
    }

    #[test]
    fn test_withdraw_before_wait() {
        let barrier = Arc::new(CountdownBarrier::new(3, BarrierMode::Loose, "test".into()));

        // One host withdraws before anyone waits.
        barrier.withdraw().unwrap();
        assert_eq!(barrier.expected_count(), 2);

        let mut handles = Vec::new();
        for _ in 0..2 {
            let b = Arc::clone(&barrier);
            handles.push(thread::spawn(move || {
                b.wait().unwrap();
            }));
        }

        for h in handles {
            h.join().unwrap();
        }
    }

    #[test]
    fn test_withdraw_while_waiting() {
        let barrier = Arc::new(CountdownBarrier::new(3, BarrierMode::Loose, "test".into()));

        // Two threads wait...
        let mut handles = Vec::new();
        for _ in 0..2 {
            let b = Arc::clone(&barrier);
            handles.push(thread::spawn(move || {
                b.wait().unwrap();
            }));
        }

        // Give threads time to reach wait()
        thread::sleep(std::time::Duration::from_millis(50));

        // Third host withdraws instead of arriving.
        barrier.withdraw().unwrap();

        for h in handles {
            h.join().unwrap();
        }
    }

    #[test]
    fn test_all_withdrawn_error() {
        let barrier = CountdownBarrier::new(2, BarrierMode::Loose, "test".into());

        barrier.withdraw().unwrap(); // expected: 1
        let result = barrier.withdraw(); // expected: 0
        assert_eq!(result, Err(BarrierError::AllWithdrawn));
    }

    #[test]
    fn test_wait_on_zero_expected() {
        let barrier = CountdownBarrier::new(0, BarrierMode::Loose, "test".into());
        let result = barrier.wait();
        assert_eq!(result, Err(BarrierError::AllWithdrawn));
    }

    #[test]
    fn test_strict_mode_withdraw_poisons() {
        let barrier = Arc::new(CountdownBarrier::new(3, BarrierMode::Strict, "strict".into()));

        // One thread waits.
        let b = Arc::clone(&barrier);
        let waiter = thread::spawn(move || b.wait());

        // Give it time to block.
        thread::sleep(std::time::Duration::from_millis(50));

        // Withdraw poisons the barrier.
        let wd = barrier.withdraw();
        assert_eq!(wd, Err(BarrierError::StrictWithdrawal));

        // The waiter should also get StrictWithdrawal.
        let result = waiter.join().unwrap();
        assert_eq!(result, Err(BarrierError::StrictWithdrawal));
    }

    #[test]
    fn test_strict_mode_normal_completion() {
        let barrier = Arc::new(CountdownBarrier::new(2, BarrierMode::Strict, "strict".into()));
        let mut handles = Vec::new();

        for _ in 0..2 {
            let b = Arc::clone(&barrier);
            handles.push(thread::spawn(move || b.wait().unwrap()));
        }

        for h in handles {
            h.join().unwrap();
        }
    }

    #[test]
    fn test_multiple_sequential_barriers() {
        // Simulate a barrier being used across multiple sync points
        // by verifying generation counter behavior.
        let barrier = Arc::new(CountdownBarrier::new(2, BarrierMode::Loose, "multi".into()));

        for _round in 0..3 {
            let mut handles = Vec::new();
            for _ in 0..2 {
                let b = Arc::clone(&barrier);
                handles.push(thread::spawn(move || b.wait().unwrap()));
            }
            for h in handles {
                h.join().unwrap();
            }
        }
    }

    #[test]
    fn test_single_participant() {
        let barrier = CountdownBarrier::new(1, BarrierMode::Loose, "solo".into());
        barrier.wait().unwrap();
    }

    #[test]
    fn test_barrier_display_errors() {
        assert_eq!(
            format!("{}", BarrierError::AllWithdrawn),
            "all hosts have withdrawn from barrier"
        );
        assert_eq!(
            format!("{}", BarrierError::StrictWithdrawal),
            "host withdrew from strict barrier"
        );
    }

    #[test]
    fn test_barrier_name_and_mode() {
        let b = CountdownBarrier::new(5, BarrierMode::Strict, "cluster_ready".into());
        assert_eq!(b.name(), "cluster_ready");
        assert_eq!(b.mode(), BarrierMode::Strict);
    }
}
