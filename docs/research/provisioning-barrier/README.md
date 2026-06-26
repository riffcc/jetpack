# Provisioning Barrier — Formal Verification & Research Paper

**Thesis.** Coordinated cluster convergence is a *different* problem than the one the
impossibility results forbid — and it admits a complete, machine-verified solution **from
no axioms**. The Two Generals (Gray, 1978) and FLP (Fischer–Lynch–Paterson, 1985) results
are correct: deterministic, certain, ground-truth simultaneous agreement is unattainable on
unreliable channels. No operator asks for that. Operations ask for a *gate* — proceed to
converge iff the controller's received evidence satisfies the declared policy, with the
barrier guaranteed to resolve over a real (fair-lossy) network. We prove, in Lean, with zero
axioms, that this gate is total and complete, and leave the impossibility beside the point —
constructively, by solving everything beneath it.

## Posture

- **Evolved from, not built in the image of, two-generals.** We inherit its load-bearing
  machinery — the channel hierarchy (`NoChannel ⊊ Unreliable ⊊ Fair-Lossy ⊊ Reliable`),
  fair-lossy as the real-network model, and the *emergent gate, not a fragile decision
  point*. We leave behind its "we refuted Gray" posture, its probability theater, and its
  crypto/network axioms.
- **Fair-lossy is a stated liveness hypothesis, not a theorem.** No `axiom` dressed up as
  proven reliability; no liveness-tail numbers marketed as guarantees. Fair-lossy = the
  channel behaves like a real channel: retransmitted probes/evidence are eventually
  delivered. Liveness is proven *under* that hypothesis, never as a property of the channel.
- **Zero axioms, by topology.** A leaderless protocol needs the cryptographic knot to reach
  agreement — which is why its proofs rest on crypto/network axioms. The controller is a
  trusted arbiter, so it inherits the emergent-gate property *structurally, with no
  cryptographic machinery*. That is why our proofs are 0-axiom. This advantage lives in the
  **model**; the paper does not narrate it.
- **The gate is parameterized over evidence and policy, not bound to the controller.**
  Safety is proven over an abstract *evidence* set and a *policy* predicate:
  `proceed ⟹ policy(evidence)`. The controller is one (honest) aggregator of evidence; what
  varies across instances is how evidence is attested, not the gate.
- **Quiet confidence.** We do not claim to crack Two Generals. We solve the operational
  problem completely, 0-axiom, and let the implication sit for whoever cares to draw it.

## What's genuinely ours (four contributions two-generals lacks)

1. **N-party controller-mediated barrier** — the model is *controller view vs. ground
   truth*; the theorem is decision-predicate correctness w.r.t. the view.
2. **Scale-tolerance as a proven-composable object** — strict-all → %-threshold →
   max-failures, a play-defined convergence policy that composes with the proven engine
   invariant. At DC scale, failure is a forcing function; the policy is how the play says so.
3. **0-axiom discipline** — Safety is already 0-axiom; the layer plan drives every property
   to 0-axiom (constructive `Decidable` instances replace classical LEM).
4. **No probability theater** — the only residual risk is implementation, the same
   risk-decomposition signature as two-generals, minus the liveness-tail number.

## Horizon

The gate is phrased so that lifting the trusted-controller assumption — replacing honest
aggregation with *attested* evidence — yields a trustless, zero-trust agreement protocol
without changing the theorem. That generalization is not the subject of this paper; the door
is left ajar, not opened.

## Three tiers

1. **Verifier (Lean 4 + Python)** — the ammo. Lean proves the gate's invariants; Python
   simulates / fuzzes it at scale.
2. **Paper (LaTeX → PDF, build artifacts gitignored)** — the marketing engine / academic
   push.
3. **Rust (jetpack)** — the product: the verified parallel-provision fan-out + readiness
   state, composed with jetpack's existing play-level primitives (`!wait_for_others`,
   `!assert`, `!fail`).

## Layering (Lean, proven layer by layer)

- **L0 — core state machine + strict-all barrier.** Host ∈ {Provisioning, Ready,
  Failed}; phase ∈ {Imaging, Barrier, Converge}. Transitions: `fan_out`, `host_ready`,
  `host_fail`, `policy_check` (all-Ready), `proceed`. Prove Safety (`proceed ⟹ all Ready`),
  Liveness (every host eventually Ready-or-Failed; the barrier always resolves — no
  deadlock on an errored host), determinism, termination.
- **L1 — %-threshold policy.** `proceed ⟹ ready/total ≥ threshold`. Prove threshold
  semantics + monotonicity (ready count only grows ⇒ once met, stays met).
- **L2 — max-failures policy.** `proceed ⟹ failures ≤ max`. Prove bounds + composition
  with L1.
- **L3 — composition with play modules.** End-to-end: the engine's parallel provision +
  readiness state composes with `!wait_for_others` / `!assert` / `!fail` respecting the
  proven invariants.
- **L4 — scale validation (Python).** Fuzz the protocol at 10⁴ hosts with randomized
  failures / timing; assert the properties hold; liveness under adversarial patterns.
  Bridges the Lean proofs to real-scale behavior.

## Status

**Thesis locked** (2026-06-26): operational convergence as a different, fully-solvable
problem, proven 0-axiom; the impossibility results left beside-the-point, constructively.
See Posture above.

- [x] L0 — core + strict-all, **complete** (`lake build` green, no warnings):
      **Safety** literal 0-axiom (reachable `Converge` ⟹ all Ready); **Progress** + **Termination**
      constructive (no excluded middle, no choice — `[propext, Quot.sound]` only, the finite
      witness scan). A strictly-decreasing `measure = phaseRank·(n+1) + provCount` bounds every
      execution to a terminal phase.
- [~] L1 — %-threshold: **readiness monotonicity** (`readyCount` only grows) + **threshold
      stability** (once met, stays met) proven, constructive. Remaining: the threshold-proceed
      gate semantics (`converge ⟹ ready/total ≥ threshold`) folds into L3 composition.
- [x] L2 — max-failures: **failure ratchet** (`failedCount` only grows) + the **partition
      identity** (`readyCount + failedCount + provCount = n` — coupling the threshold and
      failure-cap policies, the tightening of L1), proven constructive.
- [x] L3 — play-module composition: the **policy-gated engine** unifies L0/L1/L2 under one
      parameterized theorem — `proceed ⟹ policy(evidence)` for *any* policy — **0-axiom**
      (the most general theorem is the cleanest: abstracting the policy away removes the
      concrete counting that pulled `propext`/`Quot.sound` into L0–L2). `strictAll` /
      `threshold` / `maxFailures` / `combined` are proven instances; bridge to jetpack
      primitives (`!wait_for_others` + `!assert P` + `!fail`) stated.
- [x] L4 — Python scale fuzz: an adversarial suite of (policy × failure-pattern) cells
      runs the **executable mirror** of the engine at 10⁴ hosts (240 runs, seed-stable),
      asserting every proven property holds on every execution — partition, readiness
      & failure monotonicity, gate safety, fail-soundness, termination, and the
      outcome-prediction for monotone policies. **0 violations**; every converge/fail
      outcome matches the partition arithmetic the proofs predict (e.g. 80%-ready under
      a 90%-threshold fails; total failure under strict-all resolves to `failed`, no
      deadlock). Liveness under fair schedules stands in for the fair-lossy hypothesis —
      no sleeps or timeouts; liveness is a property of the schedule. `python/` (pytest +
      ruff; `python -m provisioning_barrier.fuzz`).
- [ ] Paper draft
- [ ] Rust implementation in jetpack

## Layout

- `lean/` — Lean 4 model + proofs (L0 onward).
- `python/` — simulator + fuzz harness (L4).
- `paper/` — LaTeX (`main.tex`); the compiled PDF is a build artifact (gitignored).
