#!/usr/bin/env python3
"""
Set-theory model for the missing-secrets variable diagnostic.

Question: when secrets_inventory is missing, which variables "would be undefined"?

A template renders PER HOST, against that host's blended scope. So the precise
semantics is per (play, host):

    scope(p, h) = I(h) | D(p) | G | B        # host inventory | play-defined | globals | builtins
    Missing_true = U_p U_{h in H(p)} ( R(p) \ scope(p, h) )

The CURRENT implementation uses a run-wide union approximation:

    Missing_approx = (U_p R(p)) \ ( G | B | U_p D(p) | U_h I(h) )

Derived exact per-play form (membership algebra, see Lean proof):

    Missing_p = R(p) \ ( D(p) | G | B | (n_{h in H(p)} I(h)) )      # n = intersection
    Missing_true = U_p Missing_p

We brute-force check, over many random instances:
  (1) Missing_approx ⊆ Missing_true   (approximation is SOUND: never false-positives)
  (2) Missing_intersection == Missing_true  (the per-play intersection formula is EXACT)
  (3) construct the concrete false-negative the approximation misses.
"""
import random
from itertools import chain

UNIVERSE = list("abcdefghij")          # variable names
HOSTS = ["h1", "h2", "h3", "h4"]
GROUPS = ["all", "gA", "gB"]           # gA, gB disjoint subgroups of all


def rset(pool, k):
    return frozenset(random.sample(pool, random.randint(0, k)))


def rand_instance():
    """Build a random run: per-host inventory scopes, per-play (targets, R, D), globals, builtins."""
    # group membership: which hosts each group targets (all=everyone; gA,gB a split)
    hosts_of = {
        "all": set(HOSTS),
        "gA": set(random.sample(HOSTS, random.randint(0, 2))),
        "gB": set(random.sample(HOSTS, random.randint(0, 2))),
    }
    # per-host inventory scope I(h): drawn from group_vars-style sets
    I = {h: rset(UNIVERSE, 4) for h in HOSTS}
    # plays: each targets a group, references R(p), defines D(p)
    plays = []
    for _ in range(random.randint(1, 3)):
        g = random.choice(GROUPS)
        plays.append({
            "targets": hosts_of[g],
            "R": rset(UNIVERSE, 5),
            "D": rset(UNIVERSE, 3),
        })
    return {"I": I, "plays": plays,
            "G": rset(UNIVERSE, 3), "B": rset(UNIVERSE, 2)}


def missing_true(inst):
    """Exact: union over targeted (play, host) of (R(p) \ scope(p,h))."""
    out = set()
    for p in inst["plays"]:
        for h in p["targets"]:
            scope = inst["I"][h] | p["D"] | inst["G"] | inst["B"]
            out |= p["R"] - scope
    return frozenset(out)


def missing_intersection(inst):
    r"""Derived per-play form: U_p [ R(p) \ ( D(p) | G | B | (n_{h in H(p)} I(h)) ) ].

    Note n over an EMPTY family is the universal set, so a play that targets no
    hosts contributes no missing variables (it renders nowhere)."""
    out = set()
    for p in inst["plays"]:
        targets = list(p["targets"])
        if not targets:
            continue   # renders on no host -> cannot make anything undefined
        inter_I = set(inst["I"][targets[0]])
        for h in targets[1:]:
            inter_I &= inst["I"][h]
        avail = p["D"] | inst["G"] | inst["B"] | frozenset(inter_I)
        out |= p["R"] - avail
    return frozenset(out)


def missing_approx(inst):
    """Current implementation: (U R) \ ( G | B | U D | U I )."""
    UR = set().union(*[p["R"] for p in inst["plays"]]) if inst["plays"] else set()
    UD = set().union(*[p["D"] for p in inst["plays"]]) if inst["plays"] else set()
    UI = set().union(*inst["I"].values())
    avail = inst["G"] | inst["B"] | frozenset(UD) | frozenset(UI)
    return frozenset(UR - avail)


def main():
    random.seed(1337)
    exactness_fail = 0
    soundness_fail = 0
    soundness_fail_empty_target = 0
    for i in range(20000):
        inst = rand_instance()
        mt = missing_true(inst)
        mi = missing_intersection(inst)
        ma = missing_approx(inst)
        # (1) exactness: the per-play intersection formula equals the per-(play,host) union
        if mi != mt:
            exactness_fail += 1
            print(f"[{i}] EXACTNESS VIOLATION: intersection != true")
            print(f"     inter={set(mi)} true={set(mt)}")
        # (2) approximation soundness: does approx ever over-report?
        if not ma <= mt:
            soundness_fail += 1
            # is the over-report entirely due to plays that target no hosts?
            only_empty = all((not p["targets"]) for p in inst["plays"] if (p["R"] & (ma - mt)))
            if only_empty:
                soundness_fail_empty_target += 1
            else:
                print(f"[{i}] GENUINE SOUNDNESS VIOLATION (not just empty-target):")
                print(f"     approx={set(ma)} true={set(mt)} extra={set(ma - mt)}")
    print(f"\n20000 random instances:")
    print(f"  exactness (intersection == true) failures : {exactness_fail}")
    print(f"  approx over-reports (not subset of true)   : {soundness_fail}")
    print(f"    ...of which due to empty-target plays only: {soundness_fail_empty_target}")

    # (3) concrete false-negative the approximation misses
    print("\n--- false-negative demo (group gA/gB split) ---")
    demo = {
        "I": {"h1": frozenset(), "h2": frozenset(), "h3": frozenset(), "h4": frozenset({"x"})},
        "plays": [{
            "targets": {"h1", "h2"},          # play targets gA (h1,h2)
            "R": frozenset("x"),              # references x
            "D": frozenset(),
        }],
        "G": frozenset(), "B": frozenset(),
    }
    # simulate group_vars: x lives only in gB's hosts (h4). gA hosts (h1,h2) lack it.
    print(f"  x defined only in host h4 (gB); play targets gA hosts h1,h2 and references x")
    print(f"  true       (exact): {set(missing_true(demo))}   <- x IS undefined for h1,h2")
    print(f"  intersection(exact): {set(missing_intersection(demo))}")
    print(f"  approx     (current): {set(missing_approx(demo))}   <- FALSE NEGATIVE: misses x")


if __name__ == "__main__":
    main()
