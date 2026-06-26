"""Tests for the core state model (mirror of Lean L0 `State n`)."""

from __future__ import annotations

import pytest

from provisioning_barrier.model import Counts, HostState, Phase, State


def test_host_state_terminal() -> None:
    assert not HostState.PROVISIONING.is_terminal
    assert HostState.READY.is_terminal
    assert HostState.FAILED.is_terminal


def test_phase_terminal() -> None:
    assert not Phase.IMAGING.is_terminal
    assert not Phase.BARRIER.is_terminal
    assert Phase.CONVERGE.is_terminal
    assert Phase.FAILED.is_terminal


def test_fresh_state_all_provisioning() -> None:
    s = State(5)
    assert s.n == 5
    assert s.phase is Phase.IMAGING
    assert all(h is HostState.PROVISIONING for h in s.hosts)
    assert s.counts == Counts(ready=0, failed=0, prov=5)
    assert s.counts.total == 5


def test_set_host_ready_increments_ready_decrements_prov() -> None:
    s = State(3)
    s.set_host(1, HostState.READY)
    assert s.hosts[1] is HostState.READY
    assert s.counts == Counts(ready=1, failed=0, prov=2)


def test_set_host_failed_increments_failed_decrements_prov() -> None:
    s = State(3)
    s.set_host(0, HostState.FAILED)
    s.set_host(2, HostState.FAILED)
    assert s.counts == Counts(ready=0, failed=2, prov=1)


def test_set_host_rejects_non_provisioning() -> None:
    s = State(2)
    s.set_host(0, HostState.READY)
    # a terminal host must not report again (engine contract)
    with pytest.raises(ValueError):
        s.set_host(0, HostState.FAILED)


def test_partition_invariant_holds_after_mixed_reports() -> None:
    s = State(6)
    s.set_host(0, HostState.READY)
    s.set_host(3, HostState.FAILED)
    s.set_host(5, HostState.READY)
    # ready + failed + prov == n always (the L2 partition identity)
    assert s.counts.total == 6


def test_phase_assignment() -> None:
    s = State(1)
    s.phase = Phase.BARRIER
    assert s.phase is Phase.BARRIER
    assert not s.phase.is_terminal
