"""Core state model — a faithful mirror of Lean L0 (`State n`).

Host ∈ {Provisioning, Ready, Failed}; phase ∈ {Imaging, Barrier, Converge, Failed}.
`Ready`/`Failed` are terminal host states; `Converge`/`Failed` are terminal phases.

Counts (ready/failed/prov) are cached and updated incrementally so a single host
report is O(1) — the whole point of running the machine at 10^4 hosts. The cached
counts always satisfy the L2 partition identity `ready + failed + prov == n`.
"""

from __future__ import annotations

from dataclasses import dataclass
from enum import Enum, auto


class HostState(Enum):
    PROVISIONING = auto()
    READY = auto()
    FAILED = auto()

    @property
    def is_terminal(self) -> bool:
        return self is HostState.READY or self is HostState.FAILED


class Phase(Enum):
    IMAGING = auto()
    BARRIER = auto()
    CONVERGE = auto()
    FAILED = auto()

    @property
    def is_terminal(self) -> bool:
        return self is Phase.CONVERGE or self is Phase.FAILED


@dataclass(frozen=True, slots=True)
class Counts:
    """The three host-counts that partition the host set (L2)."""

    ready: int
    failed: int
    prov: int

    @property
    def total(self) -> int:
        return self.ready + self.failed + self.prov


@dataclass(frozen=True, slots=True)
class TracePoint:
    """Invariant-relevant projection of a state, recorded once per step.

    Recording only the counts (not the full host array) keeps a 10^4-host trace
    at O(n) integers rather than O(n^2). Every invariant is re-derived from these
    counts — nothing is trusted from the engine's own decisions.
    """

    phase: Phase
    ready: int
    failed: int
    prov: int


class State:
    """Mutable engine state mirroring Lean L0 `State n`."""

    __slots__ = ("hosts", "phase", "_ready", "_failed", "_prov")

    def __init__(self, n: int) -> None:
        if n < 0:
            raise ValueError(f"host count must be non-negative, got {n}")
        self.hosts: list[HostState] = [HostState.PROVISIONING] * n
        self.phase: Phase = Phase.IMAGING
        self._ready = 0
        self._failed = 0
        self._prov = n

    @property
    def n(self) -> int:
        return len(self.hosts)

    @property
    def counts(self) -> Counts:
        return Counts(self._ready, self._failed, self._prov)

    def all_terminal(self) -> bool:
        return self._prov == 0

    def snapshot(self) -> TracePoint:
        return TracePoint(self.phase, self._ready, self._failed, self._prov)

    def set_host(self, i: int, outcome: HostState) -> None:
        """Record a host report. Only a Provisioning host may report (engine contract)."""
        prev = self.hosts[i]
        if prev is not HostState.PROVISIONING:
            raise ValueError(f"host {i} is {prev.name}, only PROVISIONING hosts report")
        self._prov -= 1
        if outcome is HostState.READY:
            self._ready += 1
        elif outcome is HostState.FAILED:
            self._failed += 1
        else:  # pragma: no cover - outcome is always READY/FAILED from the engine
            raise ValueError(f"report outcome must be READY or FAILED, got {outcome}")
        self.hosts[i] = outcome
