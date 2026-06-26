"""Tests for the fair failure-injection scheduler."""

from __future__ import annotations

import random

import pytest

from provisioning_barrier.model import HostState
from provisioning_barrier.scheduler import (
    FailurePattern,
    all_fail,
    all_ready,
    exact,
    random_failures,
    shuffled_all_ready,
)


def indices(p: FailurePattern) -> list[int]:
    return [r.index for r in p]


def test_all_ready_natural_order() -> None:
    p = all_ready(4)
    assert [r.outcome for r in p] == [HostState.READY] * 4
    assert indices(p) == [0, 1, 2, 3]
    assert p.fail_count() == 0
    assert p.n == 4


def test_all_fail() -> None:
    p = all_fail(3)
    assert all(r.outcome is HostState.FAILED for r in p)
    assert p.fail_count() == 3


def test_exact_failures_marked() -> None:
    p = exact(5, [1, 3])
    outcomes = [r.outcome for r in p]
    assert outcomes[1] is HostState.FAILED
    assert outcomes[3] is HostState.FAILED
    assert outcomes.count(HostState.FAILED) == 2


def test_fairness_every_host_reported_once() -> None:
    rng = random.Random(0)
    p = random_failures(100, 0.3, rng)
    assert sorted(indices(p)) == list(range(100))


def test_shuffled_is_a_permutation_all_ready() -> None:
    rng = random.Random(1)
    p = shuffled_all_ready(50, rng)
    assert sorted(indices(p)) == list(range(50))
    assert p.fail_count() == 0


def test_random_fail_fraction_is_roughly_target() -> None:
    rng = random.Random(42)
    p = random_failures(10000, 0.3, rng)
    frac = p.fail_count() / 10000
    assert 0.25 < frac < 0.35


def test_same_seed_is_deterministic() -> None:
    a = random_failures(200, 0.4, random.Random(7))
    b = random_failures(200, 0.4, random.Random(7))
    assert a == b


def test_rejects_out_of_range_failure_indices() -> None:
    with pytest.raises(ValueError):
        exact(3, [5])


def test_rejects_bad_fail_fraction() -> None:
    with pytest.raises(ValueError):
        random_failures(10, 1.5, random.Random(0))
    with pytest.raises(ValueError):
        random_failures(10, -0.1, random.Random(0))
