"""provisioning_barrier — L4 empirical mirror of the Lean provisioning-barrier proofs.

Modules:
    model       — core state machine (mirror of Lean L0)
    policies    — count-based convergence policies (mirror of Lean L3)
    engine      — the gated step relation (one logical transition)
    scheduler   — fair host-report schedules under failure injection
    invariants  — checkable forms of the proven theorems
    simulator   — drive a full execution and record its trace
    fuzz        — randomized scale validation at 10^4 hosts
"""

from __future__ import annotations

from provisioning_barrier.model import Counts, HostState, Phase, State, TracePoint
from provisioning_barrier.policies import Policy, combined, max_failures, strict_all, threshold

__all__ = [
    "Counts",
    "HostState",
    "Phase",
    "State",
    "TracePoint",
    "Policy",
    "strict_all",
    "threshold",
    "max_failures",
    "combined",
]
