"""Tests for the count-based convergence policies (mirror of Lean L3)."""

from __future__ import annotations

import pytest

from provisioning_barrier.model import Counts
from provisioning_barrier.policies import combined, max_failures, strict_all, threshold


def test_strict_all_truth_table() -> None:
    p = strict_all()
    assert not p(Counts(0, 0, 5))  # hosts still provisioning
    assert p(Counts(5, 0, 0))  # everyone ready
    assert not p(Counts(4, 1, 0))  # one failure → not all ready
    assert p.monotone_converge


def test_threshold_truth_table() -> None:
    p = threshold(3)
    assert not p(Counts(2, 0, 3))
    assert p(Counts(3, 0, 2))  # exactly at threshold
    assert p(Counts(5, 0, 0))
    assert p.monotone_converge


def test_max_failures_truth_table() -> None:
    p = max_failures(2)
    assert p(Counts(0, 0, 5))  # zero failures, trivially under cap
    assert p(Counts(8, 2, 0))  # exactly at cap
    assert not p(Counts(7, 3, 0))  # over cap
    assert not p.monotone_converge  # failures only grow → can flip true→false


def test_combined_truth_table() -> None:
    p = combined(3, 2)
    assert p(Counts(5, 1, 0))
    assert not p(Counts(2, 1, 0))  # below threshold
    assert not p(Counts(5, 3, 0))  # over failure cap
    assert not p.monotone_converge


def test_rejects_negative_params() -> None:
    with pytest.raises(ValueError):
        threshold(-1)
    with pytest.raises(ValueError):
        max_failures(-1)
    with pytest.raises(ValueError):
        combined(-1, 0)
