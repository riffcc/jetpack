# Proofs for the missing-secrets variable diagnostic

The diagnostic in `src/cli/secrets_diagnostic.rs` names the template variables
that would be undefined when `secrets_inventory` is declared but missing. It uses
the per-play scope formula

```
Missing_p = R(p) \ ( D(p) ∪ G ∪ B ∪ ⋂_{h ∈ H(p)} I(h) )
```

These artifacts prove that formula is **exact** — equal to the per-(play, host)
rendering semantics, with no false positives or false negatives.

## `secrets_scope.lean` — formal proof (Lean 4, no mathlib)

`missing_per_play_exact` proves, by membership over arbitrary targeted-host
predicate `H` (empty included):

```
R x ∧ ¬(X x ∨ (∀ h, H h → I h x))  ↔  (∃ h, H h ∧ (R x ∧ ¬(X x ∨ I h x)))
```

i.e. `R \ (X ∪ ⋂_{h∈H} I h) = ⋃_{h∈H} (R \ (X ∪ I h))`.

Verify with:

```
lean proofs/secrets_scope.lean     # exits 0, no output = accepted
```

## `secrets_scope_theory.py` — brute-force cross-check (Python 3)

Models plays/hosts/scopes as Python sets and checks, over 20,000 random
instances, that the per-play intersection formula equals the per-(play,host)
union (0 failures), and characterizes where the prior run-wide-union
approximation diverged (over-reports on empty-target plays; under-reports on the
groupA/groupB scope split).

```
python3 proofs/secrets_scope_theory.py
```
