// Jetpack
// Copyright (C) Riff Labs Limited <team@riff.cc>

//! Proves the shared runtime's `block_on` is safe under the real access pattern
//! the engine uses: many parallel provisioner-worker threads each driving async
//! work through the single shared runtime. If concurrent `block_on` on the
//! multi-thread runtime were unsupported, this test would panic or deadlock.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

#[test]
fn block_on_is_safe_under_concurrent_callers() {
    let n = 16;
    let counter = Arc::new(AtomicUsize::new(0));
    let handles: Vec<_> = (0..n)
        .map(|_| {
            let counter = Arc::clone(&counter);
            std::thread::spawn(move || {
                jetpack::runtime::block_on(async move {
                    // Yield to the scheduler and touch the async I/O path so the
                    // runtime actually has work to coordinate across threads.
                    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                    counter.fetch_add(1, Ordering::SeqCst);
                })
            })
        })
        .collect();

    for h in handles {
        h.join().expect("provisioner-worker thread panicked");
    }

    assert_eq!(counter.load(Ordering::SeqCst), n);
}
