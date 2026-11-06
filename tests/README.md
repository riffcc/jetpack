# Test layout and registration check

This repository uses Cargo’s default integration-test discovery: each `tests/*.rs` file is a test target. Shared suites live in subdirectories and are included by conventional out-of-line `mod name;` or `pub mod name;` declarations. A nested module is either `name.rs` or `name/mod.rs`; both forms for the same module are ambiguous. Helpers must also be registered through this graph.

`module_registration.rs` checks that every Rust source under `tests/` is textually reachable in that layout. It catches accidentally omitted module declarations, including files whose basename collides with a registered directory.

This is a structural check, not proof of compilation, execution, or coverage. It does not interpret conditional compilation (`cfg`), inline modules, `#[path]`, generated modules, `include!`, or custom Cargo target/discovery settings. If the layout adopts one of these features, update the check to understand it instead of treating its result as execution evidence. Keep Rust test fixtures outside `tests/` until the check supports explicitly distinguishing fixtures from test sources.

Use `cargo test -- --list` to inspect discovered test cases and `cargo test --locked` for their actual results. A larger test count does not by itself establish broader coverage: review which assertions and behaviors each suite exercises.
