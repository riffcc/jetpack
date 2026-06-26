"""Checkable forms of the Lean theorems, evaluated over an execution trace.

Each function is an independent re-derivation from the trace's recorded counts —
it does not trust the engine's own decisions. They map one-to-one onto the proven
results:

  partition_holds     ↔ L2.partition        (ready + failed + prov == n)
  ready_monotone      ↔ L1.readyCount_monotone
  failed_monotone     ↔ L2.failedCount_monotone
  safety              ↔ L3.gated_safety     (every reachable converge ⟹ policy)
  fail_sound          ↔ L3 policyFail       (fail only when all-terminal ∧ ¬policy)
  terminated          ↔ L0 termination      (every run ends in a terminal phase)
  monotone_outcome_*  ↔ L0 progress, under a monotone policy + fair schedule
"""

from __future__ import annotations

from collections.abc import Sequence
from itertools import pairwise

from provisioning_barrier.model import Counts, Phase, TracePoint
from provisioning_barrier.policies import Policy
from provisioning_barrier.scheduler import FailurePattern

Trace = Sequence[TracePoint]


def partition_holds(trace: Trace, n: int) -> bool:
    return all(p.ready + p.failed + p.prov == n for p in trace)


def ready_monotone(trace: Trace) -> bool:
    return all(a.ready <= b.ready for a, b in pairwise(trace))


def failed_monotone(trace: Trace) -> bool:
    return all(a.failed <= b.failed for a, b in pairwise(trace))


def safety(trace: Trace, policy: Policy) -> bool:
    """Every reachable CONVERGE satisfies the policy (L3 gated_safety)."""
    return all(
        policy(Counts(p.ready, p.failed, p.prov)) for p in trace if p.phase is Phase.CONVERGE
    )


def fail_sound(trace: Trace, policy: Policy) -> bool:
    """Every FAILED phase is all-terminal with the policy unmet (L3 policyFail)."""
    return all(
        (p.prov == 0 and not policy(Counts(p.ready, p.failed, p.prov)))
        for p in trace
        if p.phase is Phase.FAILED
    )


def terminated(trace: Trace) -> bool:
    return bool(trace) and trace[-1].phase.is_terminal


def within_budget(trace: Trace, budget: int) -> bool:
    return len(trace) <= budget


def monotone_outcome_matches(trace: Trace, policy: Policy, pattern: FailurePattern) -> bool:
    """For a monotone-converge policy + fair schedule: converge ⟺ policy holds on
    the full host assignment. Skipped (vacuously true) for non-monotone policies,
    where the proceed window can open and close mid-run."""
    if not policy.monotone_converge:
        return True
    final_assignment_satisfies = policy(Counts(pattern.ready_count(), pattern.fail_count(), 0))
    converged = bool(trace) and trace[-1].phase is Phase.CONVERGE
    return converged == final_assignment_satisfies
