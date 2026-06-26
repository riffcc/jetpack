"""Convergence policies — the count-based mirror of Lean L3's `Policy n`.

Lean defines `Policy n := (Fin n → HostState) → Prop` over the full host map. Every
instance we care about (strict-all, %-threshold, max-failures, and their combination)
is determined by the three host-counts alone, so a policy is represented here as a
predicate over `Counts` — O(1) to evaluate, which is what lets the gate be checked
after every host report at 10^4 scale.

`monotone_converge` records whether, once the policy holds, it keeps holding as hosts
report (ready/failed only grow, prov only shrinks). strict-all and threshold do;
max-failures and combined do not (the failure cap can be breached late). That flag
gates the strongest outcome-prediction check in the fuzz harness.
"""

from __future__ import annotations

from collections.abc import Callable
from dataclasses import dataclass

from provisioning_barrier.model import Counts


@dataclass(frozen=True, slots=True)
class Policy:
    name: str
    holds: Callable[[Counts], bool]
    monotone_converge: bool

    def __call__(self, counts: Counts) -> bool:
        return self.holds(counts)


def strict_all() -> Policy:
    """L0: every host ready — `prov == 0` AND nothing has failed."""
    return Policy("strict-all", lambda c: c.prov == 0 and c.failed == 0, True)


def threshold(t: int) -> Policy:
    """L1: at least `t` hosts ready."""
    _require_nonneg(t, "threshold")
    return Policy(f"threshold>={t}", lambda c: c.ready >= t, True)


def max_failures(m: int) -> Policy:
    """L2: at most `m` hosts failed."""
    _require_nonneg(m, "max-failures")
    return Policy(f"max-failures<={m}", lambda c: c.failed <= m, False)


def combined(t: int, m: int) -> Policy:
    """A play may demand both a readiness threshold and a failure cap (L1 ∧ L2)."""
    _require_nonneg(t, "threshold")
    _require_nonneg(m, "max-failures")
    return Policy(f"threshold>={t}&fail<={m}", lambda c: c.ready >= t and c.failed <= m, False)


def _require_nonneg(value: int, name: str) -> None:
    if value < 0:
        raise ValueError(f"{name} must be non-negative, got {value}")
