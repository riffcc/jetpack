"""The gated engine — the controller's step relation, a mirror of Lean L3 `GatedStep`.

The controller drives the machine in three moves:
  1. `fan_out`   — Imaging → Barrier (kick off parallel provisioning).
  2. `report`    — a host surfaces Ready/Failed evidence (Barrier only).
  3. `decide`    — the gate: PROCEED if the policy holds on the evidence so far,
                   FAIL if every host is terminal yet the policy is unmet,
                   HOLD otherwise (keep collecting evidence).

`apply_decision` moves to a terminal phase on PROCEED/FAIL; CONVERGE/FAILED absorb.
Every move that changes the state is a legal `GatedStep` constructor; the engine
raises loudly on any illegal one so a bug can never silently corrupt the trace.
"""

from __future__ import annotations

from enum import Enum, auto

from provisioning_barrier.model import HostState, Phase, State
from provisioning_barrier.policies import Policy


class Decision(Enum):
    PROCEED = auto()
    FAIL = auto()
    HOLD = auto()


def fan_out(state: State) -> None:
    if state.phase is not Phase.IMAGING:
        raise ValueError(f"fan_out only from IMAGING, phase is {state.phase.name}")
    state.phase = Phase.BARRIER


def report(state: State, i: int, outcome: HostState) -> None:
    if state.phase is not Phase.BARRIER:
        raise ValueError(f"host report only in BARRIER, phase is {state.phase.name}")
    state.set_host(i, outcome)


def decide(state: State, policy: Policy) -> Decision:
    if state.phase is not Phase.BARRIER:
        raise ValueError(f"decide only in BARRIER, phase is {state.phase.name}")
    if policy(state.counts):
        return Decision.PROCEED
    if state.all_terminal():
        return Decision.FAIL
    return Decision.HOLD


def apply_decision(state: State, decision: Decision) -> None:
    if decision is Decision.PROCEED:
        state.phase = Phase.CONVERGE
    elif decision is Decision.FAIL:
        state.phase = Phase.FAILED
    # HOLD: remain in BARRIER and keep collecting evidence.


def is_terminal(state: State) -> bool:
    return state.phase.is_terminal
