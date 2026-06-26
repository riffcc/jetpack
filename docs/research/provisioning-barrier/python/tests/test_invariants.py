"""Tests for the invariant checks — each is a checkable form of a Lean theorem."""

from __future__ import annotations

from provisioning_barrier.invariants import (
    fail_sound,
    failed_monotone,
    monotone_outcome_matches,
    partition_holds,
    ready_monotone,
    safety,
    terminated,
    within_budget,
)
from provisioning_barrier.model import Phase, TracePoint
from provisioning_barrier.policies import strict_all, threshold
from provisioning_barrier.scheduler import FailurePattern, all_fail, all_ready


def tp(phase: Phase, ready: int, failed: int, prov: int) -> TracePoint:
    return TracePoint(phase, ready, failed, prov)


def test_partition_holds_on_good_trace() -> None:
    trace = [
        tp(Phase.IMAGING, 0, 0, 4),
        tp(Phase.BARRIER, 0, 0, 4),
        tp(Phase.BARRIER, 1, 0, 3),
        tp(Phase.BARRIER, 2, 1, 1),
        tp(Phase.CONVERGE, 2, 1, 1),
    ]
    assert partition_holds(trace, 4)


def test_partition_catches_a_leak() -> None:
    trace = [tp(Phase.BARRIER, 1, 0, 4)]  # 1+0+4 = 5 != 4
    assert not partition_holds(trace, 4)


def test_ready_monotone_and_its_violation() -> None:
    good = [tp(Phase.BARRIER, 0, 0, 3), tp(Phase.BARRIER, 1, 0, 2), tp(Phase.BARRIER, 2, 0, 1)]
    assert ready_monotone(good)
    bad = [tp(Phase.BARRIER, 2, 0, 1), tp(Phase.BARRIER, 1, 0, 2)]  # ready shrank
    assert not ready_monotone(bad)


def test_failed_monotone_and_its_violation() -> None:
    good = [tp(Phase.BARRIER, 0, 0, 3), tp(Phase.BARRIER, 0, 1, 2), tp(Phase.BARRIER, 0, 2, 1)]
    assert failed_monotone(good)
    bad = [tp(Phase.BARRIER, 0, 2, 1), tp(Phase.BARRIER, 0, 1, 2)]
    assert not failed_monotone(bad)


def test_safety_holds_when_converge_satisfies_policy() -> None:
    # converge with everyone ready → strict-all satisfied
    trace = [tp(Phase.BARRIER, 0, 0, 2), tp(Phase.CONVERGE, 2, 0, 0)]
    assert safety(trace, strict_all())


def test_safety_catches_unsafe_converge() -> None:
    # converge with a failure present → strict-all violated
    trace = [tp(Phase.CONVERGE, 1, 1, 0)]
    assert not safety(trace, strict_all())


def test_fail_sound_requires_all_terminal_and_unmet() -> None:
    sound = [tp(Phase.FAILED, 1, 1, 0)]  # all terminal, policy unmet
    assert fail_sound(sound, strict_all())
    unsound = [tp(Phase.FAILED, 1, 0, 1)]  # not all terminal (prov=1)
    assert not fail_sound(unsound, strict_all())


def test_terminated_and_within_budget() -> None:
    trace = [tp(Phase.BARRIER, 0, 0, 1), tp(Phase.CONVERGE, 1, 0, 0)]
    assert terminated(trace)
    assert within_budget(trace, 5)
    assert not within_budget(trace, 1)
    assert not terminated([tp(Phase.BARRIER, 0, 0, 1)])


def test_monotone_outcome_strict_all_matches_pattern() -> None:
    # all ready → converge; any failure → fail (monotone policy)
    assert monotone_outcome_matches(
        [tp(Phase.BARRIER, 0, 0, 2), tp(Phase.CONVERGE, 2, 0, 0)], strict_all(), all_ready(2)
    )
    assert monotone_outcome_matches(
        [tp(Phase.BARRIER, 0, 0, 2), tp(Phase.FAILED, 1, 1, 0)], strict_all(), all_fail(2)
    )


def test_monotone_outcome_skipped_for_non_monotone() -> None:
    pat: FailurePattern = all_ready(2)
    # combined is non-monotone → check is always vacuously True (not applicable)
    assert monotone_outcome_matches([tp(Phase.CONVERGE, 1, 0, 1)], threshold(0), pat) is True


def test_monotone_outcome_catches_wrong_prediction() -> None:
    # threshold(2): two ready in pattern → should converge; a FAILED trace is wrong
    pat = all_ready(2)
    bad = [tp(Phase.BARRIER, 0, 0, 2), tp(Phase.FAILED, 2, 0, 0)]
    assert not monotone_outcome_matches(bad, threshold(2), pat)
