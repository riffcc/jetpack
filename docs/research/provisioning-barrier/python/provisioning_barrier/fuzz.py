"""L4 — randomized scale validation of the provisioning-barrier gate.

Runs the gated engine across an adversarial suite of (policy × failure-pattern)
cells at 10^4 hosts and asserts every Lean-proven property holds on every
execution. This is the empirical bridge from the proofs to real-scale behaviour:
it cannot *find* a theorem the proofs missed, but it confirms the executable
mirror honours them at datacenter scale and that the barrier always resolves
under fair schedules — including the liveness cases (total failure, stragglers).

Deterministic for a fixed seed. CLI: `python -m provisioning_barrier.fuzz`.
"""

from __future__ import annotations

import math
import random
import sys
import time
from collections.abc import Callable
from dataclasses import dataclass, field

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
from provisioning_barrier.model import Phase
from provisioning_barrier.policies import Policy, combined, max_failures, strict_all, threshold
from provisioning_barrier.scheduler import (
    FailurePattern,
    all_fail,
    all_ready,
    random_failures,
    shuffled_all_ready,
)
from provisioning_barrier.simulator import TIGHT_BOUND, simulate

ScenarioBuilder = Callable[[int, random.Random], tuple[Policy, FailurePattern]]


def _s_strict_all_ready(n: int, rng: random.Random) -> tuple[Policy, FailurePattern]:
    return strict_all(), all_ready(n)


def _s_strict_all_fail(n: int, rng: random.Random) -> tuple[Policy, FailurePattern]:
    return strict_all(), all_fail(n)


def _s_strict_all_half_fail(n: int, rng: random.Random) -> tuple[Policy, FailurePattern]:
    return strict_all(), random_failures(n, 0.5, rng)


def _s_threshold_half(n: int, rng: random.Random) -> tuple[Policy, FailurePattern]:
    return threshold(math.ceil(n * 0.5)), random_failures(n, 0.3, rng)


def _s_threshold_near_all(n: int, rng: random.Random) -> tuple[Policy, FailurePattern]:
    return threshold(math.ceil(n * 0.9)), random_failures(n, 0.2, rng)


def _s_threshold_equals_n(n: int, rng: random.Random) -> tuple[Policy, FailurePattern]:
    return threshold(n), all_ready(n)  # full-length trace; ≡ strict-all


def _s_threshold_one_total_fail(n: int, rng: random.Random) -> tuple[Policy, FailurePattern]:
    return threshold(1), all_fail(n)


def _s_maxfail_zero(n: int, rng: random.Random) -> tuple[Policy, FailurePattern]:
    return max_failures(0), all_ready(n)  # degenerate gate; safety must still hold


def _s_combined_tight_cap(n: int, rng: random.Random) -> tuple[Policy, FailurePattern]:
    return combined(math.ceil(n * 0.5), math.floor(n * 0.1)), random_failures(n, 0.2, rng)


def _s_combined_loose_shuffled(n: int, rng: random.Random) -> tuple[Policy, FailurePattern]:
    return combined(math.ceil(n * 0.5), math.floor(n * 0.3)), shuffled_all_ready(n, rng)


def _s_threshold_zero(n: int, rng: random.Random) -> tuple[Policy, FailurePattern]:
    return threshold(0), all_fail(n)  # t=0: always-proceed gate


def _s_random_threshold_random_fail(n: int, rng: random.Random) -> tuple[Policy, FailurePattern]:
    t = rng.randint(0, n)
    frac = rng.random()
    return threshold(t), random_failures(n, frac, rng)


SCENARIOS: list[tuple[str, ScenarioBuilder]] = [
    ("strict-all / all-ready", _s_strict_all_ready),
    ("strict-all / all-fail", _s_strict_all_fail),
    ("strict-all / 50%-fail", _s_strict_all_half_fail),
    ("threshold(50%) / 30%-fail", _s_threshold_half),
    ("threshold(90%) / 20%-fail", _s_threshold_near_all),
    ("threshold(n) / all-ready", _s_threshold_equals_n),
    ("threshold(1) / all-fail", _s_threshold_one_total_fail),
    ("max-failures(0) / all-ready", _s_maxfail_zero),
    ("combined(50%,10%) / 20%-fail", _s_combined_tight_cap),
    ("combined(50%,30%) / shuffled-ready", _s_combined_loose_shuffled),
    ("threshold(0) / all-fail", _s_threshold_zero),
    ("random-threshold / random-fail", _s_random_threshold_random_fail),
]


@dataclass(frozen=True, slots=True)
class FuzzConfig:
    hosts: int = 10_000
    runs: int = 240
    seed: int = 0


@dataclass(frozen=True, slots=True)
class FuzzStats:
    runs: int
    converged: int
    failed: int
    violations: int
    elapsed_s: float = field(compare=False)
    per_scenario: dict[str, tuple[int, int]] = field(default_factory=dict)


def _check(trace, policy: Policy, pattern: FailurePattern, n: int) -> list[str]:
    bad: list[str] = []
    if not partition_holds(trace, n):
        bad.append("partition")
    if not ready_monotone(trace):
        bad.append("ready-monotone")
    if not failed_monotone(trace):
        bad.append("failed-monotone")
    if not safety(trace, policy):
        bad.append("safety")
    if not fail_sound(trace, policy):
        bad.append("fail-sound")
    if not terminated(trace):
        bad.append("terminated")
    if not within_budget(trace, n + TIGHT_BOUND):
        bad.append("budget")
    if not monotone_outcome_matches(trace, policy, pattern):
        bad.append("monotone-outcome")
    return bad


def run_fuzz(config: FuzzConfig) -> FuzzStats:
    start = time.perf_counter()
    converged = failed = violations = 0
    per_scenario: dict[str, tuple[int, int]] = {}
    for i in range(config.runs):
        name, builder = SCENARIOS[i % len(SCENARIOS)]
        rng = random.Random((config.seed * 1_000_003) ^ i)
        policy, pattern = builder(config.hosts, rng)
        trace = simulate(config.hosts, policy, pattern)
        bad = _check(trace, policy, pattern, config.hosts)
        c, f = per_scenario.get(name, (0, 0))
        if trace[-1].phase is Phase.CONVERGE:
            converged += 1
            c += 1
        else:
            failed += 1
            f += 1
        per_scenario[name] = (c, f)
        if bad:
            violations += 1
            raise AssertionError(
                f"violation in scenario {name!r} (run {i}, n={config.hosts}): {bad}\n"
                f"  policy={policy.name} pattern={pattern.name} "
                f"final={trace[-1]}"
            )
    return FuzzStats(
        config.runs, converged, failed, violations, time.perf_counter() - start, per_scenario
    )


def _parse_args(argv: list[str]) -> FuzzConfig:
    hosts, runs, seed = 10_000, len(SCENARIOS) * 20, 0
    it = iter(argv)
    for token in it:
        if token == "--hosts":
            hosts = int(next(it))
        elif token == "--runs":
            runs = int(next(it))
        elif token == "--seed":
            seed = int(next(it))
        else:
            raise SystemExit(f"unknown argument: {token}")
    return FuzzConfig(hosts=hosts, runs=runs, seed=seed)


def main(argv: list[str] | None = None) -> int:
    config = _parse_args(argv if argv is not None else sys.argv[1:])
    stats = run_fuzz(config)
    print(f"L4 fuzz — {config.hosts} hosts, {stats.runs} runs, seed {config.seed}")
    print(f"  {'converged:':<12}{stats.converged}")
    print(f"  {'failed:':<12}{stats.failed}")
    print(f"  {'violations:':<12}{stats.violations}")
    print(f"  {'elapsed:':<12}{stats.elapsed_s:.3f}s")
    print("  per-scenario (converge / fail):")
    for name, (c, f) in stats.per_scenario.items():
        print(f"    {name:<38} {c:>4} / {f:<4}")
    return 0 if stats.violations == 0 else 1


if __name__ == "__main__":
    raise SystemExit(main())
