"""Tests for the simulator — small, deterministic end-to-end executions."""

from __future__ import annotations

import random

import pytest

from provisioning_barrier.invariants import (
    failed_monotone,
    partition_holds,
    ready_monotone,
    safety,
    terminated,
)
from provisioning_barrier.model import Phase
from provisioning_barrier.policies import strict_all, threshold
from provisioning_barrier.scheduler import all_fail, all_ready, exact, random_failures
from provisioning_barrier.simulator import simulate


def test_strict_all_all_ready_converges() -> None:
    trace = simulate(4, strict_all(), all_ready(4))
    assert trace[-1].phase is Phase.CONVERGE
    assert partition_holds(trace, 4) and ready_monotone(trace) and safety(trace, strict_all())


def test_strict_all_with_one_failure_fails_cleanly() -> None:
    trace = simulate(4, strict_all(), exact(4, [2]))
    assert trace[-1].phase is Phase.FAILED  # no deadlock on the errored host


def test_threshold_converges_before_all_hosts_report() -> None:
    trace = simulate(10, threshold(3), all_ready(10))
    assert trace[-1].phase is Phase.CONVERGE
    assert trace[-1].prov > 0  # scale-tolerant: proceeded with hosts still provisioning


def test_threshold_unmet_under_total_failure_fails() -> None:
    trace = simulate(4, threshold(3), all_fail(4))
    assert trace[-1].phase is Phase.FAILED


def test_empty_cluster_converges_under_strict_all() -> None:
    trace = simulate(0, strict_all(), all_ready(0))
    assert trace[-1].phase is Phase.CONVERGE


def test_empty_cluster_fails_under_unreachable_threshold() -> None:
    trace = simulate(0, threshold(3), all_ready(0))
    assert trace[-1].phase is Phase.FAILED


def test_all_invariants_hold_on_a_mixed_run() -> None:
    rng = random.Random(123)
    pat = random_failures(8, 0.3, rng)
    trace = simulate(8, threshold(4), pat)
    assert partition_holds(trace, 8)
    assert ready_monotone(trace)
    assert failed_monotone(trace)
    assert safety(trace, threshold(4))
    assert terminated(trace)


def test_pattern_size_mismatch_raises() -> None:
    with pytest.raises(ValueError):
        simulate(4, strict_all(), all_ready(3))


def test_budget_bound_is_enforced() -> None:
    # a 4-host strict-all run produces 7 trace points (imaging, barrier, 4 reports, converge)
    with pytest.raises(RuntimeError):
        simulate(4, strict_all(), all_ready(4), budget=2)
