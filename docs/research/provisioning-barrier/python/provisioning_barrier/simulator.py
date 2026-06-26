"""Drive a single fair execution of the gated engine and record its trace.

The controller proceeds the moment the policy holds on the evidence seen so far
(scale-tolerant), and fails cleanly the instant every host is terminal yet the
policy is unmet (no deadlock). Under a fair schedule each host reports exactly
once, so the trace is bounded by `n + 3` points — the empirical form of L0
termination. `budget` defaults to that tight bound; exceeding it is a bug.
"""

from __future__ import annotations

from collections.abc import Sequence

from provisioning_barrier.engine import (
    Decision,
    apply_decision,
    decide,
    fan_out,
    is_terminal,
    report,
)
from provisioning_barrier.model import State, TracePoint
from provisioning_barrier.policies import Policy
from provisioning_barrier.scheduler import FailurePattern

TIGHT_BOUND = 3  # imaging + barrier + terminal, on top of at most n host reports


def simulate(
    n: int,
    policy: Policy,
    pattern: FailurePattern,
    *,
    budget: int | None = None,
) -> Sequence[TracePoint]:
    if pattern.n != n:
        raise ValueError(f"pattern covers {pattern.n} hosts but n={n}")
    if budget is None:
        budget = n + TIGHT_BOUND

    state = State(n)
    trace: list[TracePoint] = [state.snapshot()]

    fan_out(state)
    trace.append(state.snapshot())

    reports = iter(pattern)
    decision = decide(state, policy)  # the gate may already hold at barrier entry
    while decision is Decision.HOLD:
        nxt = next(reports, None)
        if nxt is None:
            break
        report(state, nxt.index, nxt.outcome)
        trace.append(state.snapshot())
        decision = decide(state, policy)

    apply_decision(state, decision)
    trace.append(state.snapshot())

    if not is_terminal(state):
        raise RuntimeError("fair schedule failed to drive the barrier to a terminal phase")
    if len(trace) > budget:
        raise RuntimeError(f"execution exceeded bound {budget}: {len(trace)} steps")
    return trace
