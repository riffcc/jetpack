# Simulator + fuzz harness (L4)

The empirical mirror of the Lean proofs. It runs the **same gated engine** the Lean
model specifies, as an executable Python program, and asserts every proven property
holds on every execution — including at 10⁴ hosts under adversarial failure injection.
It cannot find a theorem the proofs missed; it confirms the executable mirror honours
them at datacenter scale, and that the barrier always resolves under fair schedules
(the stand-in for the fair-lossy liveness hypothesis). It is also the reference the
Rust implementation diffs against. See `../README.md`.

## Run

```bash
uv sync                       # create the venv (pytest + ruff)
uv run pytest                 # the full suite (61 tests)
uv run ruff check . && uv run ruff format --check .
uv run python -m provisioning_barrier.fuzz --hosts 10000 --runs 240 --seed 0
```

The headline run is **0 violations** across 240 executions at 10⁴ hosts. Every
converge/fail outcome matches the partition arithmetic the proofs predict (e.g. 80%-
ready under a 90%-threshold fails; total failure under strict-all resolves to
`failed` with no deadlock).

## Module map (each a small file, <200 LoC)

| module | mirrors | role |
|---|---|---|
| `model` | L0 `State n` | host/phase enums, incremental cached counts |
| `policies` | L3 `Policy` | strict-all / threshold / max-failures / combined, O(1) over counts |
| `engine` | L3 `GatedStep` | fan-out, host report, the gated decide/proceed/fail |
| `scheduler` | fair-lossy liveness | fair failure-injection schedules (no sleeps) |
| `invariants` | every theorem | independent re-derivation from the trace |
| `simulator` | L0 progress/termination | drive one execution, record its trace |
| `fuzz` | L4 | the adversarial scale harness + CLI |

## Design notes

- **Counts are cached and updated in O(1)** per host report, so checking the gate after
  every report is cheap at 10⁴. The cached counts always satisfy the L2 partition
  identity `ready + failed + prov == n`.
- **Invariants re-derive from the trace**, never trusting the engine's own decisions —
  `safety` re-evaluates the policy at each `converge` point, etc.
- **Liveness is schedule fairness, not the clock.** Every host appears exactly once in a
  fair schedule (retransmitted evidence eventually arrives); there are no sleeps or
  timeouts anywhere.
- **Deterministic for a fixed seed** — randomized factories take an explicit
  `random.Random`, so every fuzz run is reproducible.
