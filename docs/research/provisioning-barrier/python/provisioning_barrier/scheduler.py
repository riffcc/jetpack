"""Fair failure-injection scheduler.

A `FailurePattern` is a *fair* schedule: every host is reported exactly once, in
some order, as either Ready or Failed. Fairness here stands in for the fair-lossy
liveness hypothesis — retransmitted evidence is eventually delivered, so no host is
starved. There are no sleeps or timeouts: liveness is a property of the schedule
(each host appears once), not of the clock.

Randomized factories take an explicit `random.Random` so every fuzz run is
reproducible from its seed.
"""

from __future__ import annotations

import random
from collections.abc import Iterable
from dataclasses import dataclass

from provisioning_barrier.model import HostState


@dataclass(frozen=True, slots=True)
class HostReport:
    index: int
    outcome: HostState


@dataclass(frozen=True, slots=True)
class FailurePattern:
    name: str
    reports: tuple[HostReport, ...]

    @property
    def n(self) -> int:
        return len(self.reports)

    def fail_count(self) -> int:
        return sum(1 for r in self.reports if r.outcome is HostState.FAILED)

    def ready_count(self) -> int:
        return sum(1 for r in self.reports if r.outcome is HostState.READY)

    def __iter__(self):
        return iter(self.reports)

    def __len__(self) -> int:
        return len(self.reports)


def _build(n: int, fail_set: Iterable[int], order: Iterable[int], name: str) -> FailurePattern:
    if n < 0:
        raise ValueError(f"host count must be non-negative, got {n}")
    order_list = list(order)
    if sorted(order_list) != list(range(n)):
        raise ValueError("report order must be a permutation of 0..n-1 (fairness)")
    fail_index_set = set(fail_set)
    bad = fail_index_set - set(range(n))
    if bad:
        raise ValueError(f"failure indices out of range for n={n}: {sorted(bad)}")
    reports = tuple(
        HostReport(i, HostState.FAILED if i in fail_index_set else HostState.READY)
        for i in order_list
    )
    return FailurePattern(name, reports)


def all_ready(n: int) -> FailurePattern:
    return _build(n, set(), range(n), "all-ready")


def all_fail(n: int) -> FailurePattern:
    return _build(n, range(n), range(n), "all-fail")


def exact(n: int, fail_indices: Iterable[int]) -> FailurePattern:
    return _build(n, fail_indices, range(n), "exact-failures")


def shuffled_all_ready(n: int, rng: random.Random) -> FailurePattern:
    order = list(range(n))
    rng.shuffle(order)
    return _build(n, set(), order, "stragglers-all-ready")


def random_failures(n: int, fail_fraction: float, rng: random.Random) -> FailurePattern:
    if not 0.0 <= fail_fraction <= 1.0:
        raise ValueError(f"fail_fraction must be in [0, 1], got {fail_fraction}")
    order = list(range(n))
    rng.shuffle(order)
    fail_set = {i for i in range(n) if rng.random() < fail_fraction}
    return _build(n, fail_set, order, f"random-{fail_fraction:g}")
