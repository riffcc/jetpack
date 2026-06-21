// Jetpack
// Copyright (C) Riff Labs Limited <team@riff.cc>
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

//! Shared Tokio runtime for sync-outer code paths that need async I/O.
//!
//! The engine is synchronous at its boundaries — the `Provisioner` trait and
//! playbook traversal are not async — yet several operations (Proxmox,
//! Dragonfly and Gravity HTTP calls via `reqwest`) are inherently async. The
//! historical answer was for every call site to mint a throwaway
//! `current_thread` runtime and `block_on` it, which is wasteful (a runtime
//! built and torn down per call) and fragile (panics if a caller is already on
//! an executor).
//!
//! This module owns a single shared multi-threaded runtime and exposes
//! [`block_on`], which is safe to call from any non-executor thread, including
//! concurrently from parallel provisioner workers. See `tests/runtime.rs` for
//! the concurrency proof.

use once_cell::sync::Lazy;
use std::future::Future;

static RUNTIME: Lazy<tokio::runtime::Runtime> = Lazy::new(|| {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("jetpack-rt")
        .build()
        .expect("failed to build shared Jetpack Tokio runtime")
});

/// Run `future` to completion on the shared runtime, blocking the calling
/// thread until it resolves.
///
/// Safe to call concurrently from multiple OS threads (e.g. parallel provisioner
/// workers) and from ordinary synchronous code. **Must not** be called from
/// within an existing Tokio execution context — callers are the engine's
/// synchronous boundary code, not async tasks.
pub fn block_on<F>(future: F) -> F::Output
where
    F: Future,
{
    RUNTIME.block_on(future)
}
