"""Tests for the gated engine — the controller's step relation (mirror of Lean L3)."""

from __future__ import annotations

import pytest

from provisioning_barrier.engine import (
    Decision,
    apply_decision,
    decide,
    fan_out,
    is_terminal,
    report,
)
from provisioning_barrier.model import Counts, HostState, Phase, State
from provisioning_barrier.policies import strict_all, threshold


def _barrier(n: int) -> State:
    s = State(n)
    fan_out(s)
    assert s.phase is Phase.BARRIER
    return s


def test_fan_out_moves_imaging_to_barrier() -> None:
    s = State(2)
    fan_out(s)
    assert s.phase is Phase.BARRIER


def test_fan_out_rejects_non_imaging() -> None:
    s = State(1)
    s.phase = Phase.BARRIER
    with pytest.raises(ValueError):
        fan_out(s)


def test_report_updates_host_and_counts() -> None:
    s = _barrier(3)
    report(s, 1, HostState.READY)
    assert s.hosts[1] is HostState.READY
    assert s.counts == Counts(1, 0, 2)


def test_report_requires_barrier_phase() -> None:
    s = State(2)  # still IMAGING
    with pytest.raises(ValueError):
        report(s, 0, HostState.READY)


def test_report_rejects_already_terminal_host() -> None:
    s = _barrier(2)
    report(s, 0, HostState.READY)
    with pytest.raises(ValueError):
        report(s, 0, HostState.FAILED)


def test_decide_hold_when_policy_unmet_and_hosts_remain() -> None:
    s = _barrier(3)
    assert decide(s, strict_all()) is Decision.HOLD


def test_decide_proceed_when_policy_met() -> None:
    s = _barrier(2)
    report(s, 0, HostState.READY)
    report(s, 1, HostState.READY)
    assert decide(s, strict_all()) is Decision.PROCEED


def test_decide_fail_when_all_terminal_and_policy_unmet() -> None:
    s = _barrier(2)
    report(s, 0, HostState.READY)
    report(s, 1, HostState.FAILED)
    assert decide(s, strict_all()) is Decision.FAIL


def test_decide_threshold_proceeds_before_all_terminal() -> None:
    s = _barrier(4)
    t = threshold(2)
    report(s, 0, HostState.READY)
    report(s, 1, HostState.READY)
    # ready=2>=2 even though two hosts still provisioning — scale-tolerant proceed
    assert decide(s, t) is Decision.PROCEED


def test_apply_decision_sets_terminal_phases() -> None:
    s = _barrier(2)
    apply_decision(s, Decision.PROCEED)
    assert s.phase is Phase.CONVERGE
    s2 = _barrier(2)
    apply_decision(s2, Decision.FAIL)
    assert s2.phase is Phase.FAILED


def test_apply_decision_hold_keeps_barrier() -> None:
    s = _barrier(2)
    apply_decision(s, Decision.HOLD)
    assert s.phase is Phase.BARRIER


def test_is_terminal_tracks_phase() -> None:
    s = _barrier(1)
    assert not is_terminal(s)
    apply_decision(s, Decision.PROCEED)
    assert is_terminal(s)
