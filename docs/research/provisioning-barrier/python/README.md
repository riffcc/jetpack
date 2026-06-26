# Simulator + fuzz harness (L4)

Concurrent provisioning with randomized failure injection at scale (target 10^4
hosts). Asserts the Lean-proven properties hold under adversarial failure/timing,
and serves as a reference the Rust implementation diffs against. See `../README.md`.
