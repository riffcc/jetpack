"""Tests for the L4 scale-fuzz harness."""

from __future__ import annotations

import pytest

from provisioning_barrier.fuzz import SCENARIOS, FuzzConfig, FuzzStats, main, run_fuzz


def test_fuzz_small_runs_zero_violations() -> None:
    runs = len(SCENARIOS) * 3
    stats = run_fuzz(FuzzConfig(hosts=500, runs=runs, seed=0))
    assert isinstance(stats, FuzzStats)
    assert stats.runs == runs
    assert stats.violations == 0
    assert stats.converged + stats.failed == runs


def test_fuzz_at_ten_thousand_hosts() -> None:
    # the headline scale: every adversarial cell at 10^4 hosts, zero violations
    stats = run_fuzz(FuzzConfig(hosts=10_000, runs=len(SCENARIOS), seed=1))
    assert stats.violations == 0
    assert stats.converged > 0
    assert stats.failed > 0


def test_fuzz_broad_randomized_coverage() -> None:
    stats = run_fuzz(FuzzConfig(hosts=2_000, runs=len(SCENARIOS) * 8, seed=7))
    assert stats.violations == 0
    # every scenario cell is exercised at least once (cycling)
    assert set(stats.per_scenario) == {name for name, _ in SCENARIOS}


def test_fuzz_is_deterministic_for_a_fixed_seed() -> None:
    a = run_fuzz(FuzzConfig(hosts=300, runs=len(SCENARIOS) * 2, seed=42))
    b = run_fuzz(FuzzConfig(hosts=300, runs=len(SCENARIOS) * 2, seed=42))
    assert a == b


def test_empty_cluster_scenario_is_safe() -> None:
    stats = run_fuzz(FuzzConfig(hosts=0, runs=len(SCENARIOS), seed=3))
    assert stats.violations == 0


def test_violation_surfaces_as_nonzero() -> None:
    # sanity: the counter actually counts. A config with zero runs has zero of everything.
    stats = run_fuzz(FuzzConfig(hosts=10, runs=0, seed=0))
    assert stats.violations == 0
    assert stats.converged == 0 and stats.failed == 0


def test_cli_entrypoint_runs_clean(capsys: pytest.CaptureFixture[str]) -> None:
    rc = main(["--hosts", "200", "--runs", str(len(SCENARIOS)), "--seed", "0"])
    assert rc == 0
    out = capsys.readouterr().out
    assert "violations: 0" in out
