/-!
# Correctness of the missing-secrets scope formula

Sets are membership predicates (`α → Prop`) — no mathlib needed.

Semantics: a template renders per host. Variable `x` referenced by a play is
*undefined* iff some targeted host `h` lacks `x` in its scope `X ∪ I h`, where
`X = D(p) ∪ G ∪ B` (play-defined + globals + builtins) and `I h` is host `h`'s
inventory scope. So the true per-play missing set is `⋃_{h ∈ H} (R \ (X ∪ I h))`.

The implementation uses the per-play intersection form `R \ (X ∪ ⋂_{h ∈ H} I h)`.
We prove the two agree on membership — i.e. the implemented formula is exact,
for arbitrary `H` (empty `H` ⇒ a play that renders on no host contributes nothing).
-/

variable {α β : Type}

theorem missing_per_play_exact
    (R X : α → Prop) (I : β → α → Prop) (H : β → Prop) (x : α) :
    (R x ∧ ¬ (X x ∨ (∀ h, H h → I h x)))
      ↔ (∃ h, H h ∧ (R x ∧ ¬ (X x ∨ I h x))) := by
  constructor
  · -- (⇒) a negated universal yields a counterexample witness (classical)
    intro hl
    apply Classical.byContradiction
    intro nex
    apply hl.right
    apply Or.inr
    intro h hh
    apply Classical.byContradiction
    intro nhi
    apply nex
    exact
      Exists.intro h
        (And.intro hh
          (And.intro hl.left
            (fun cond =>
              Or.elim cond (fun hx => hl.right (Or.inl hx)) (fun hih => nhi hih))))
  · -- (⇐) a single failing targeted host witnesses the miss (constructive)
    intro hr
    apply Exists.elim hr
    intro h hw
    have hh := And.left hw
    have r := And.left (And.right hw)
    have hnx := And.right (And.right hw)
    exact
      And.intro r
        (fun cond =>
          Or.elim cond
            (fun hx => hnx (Or.inl hx))
            (fun hall => hnx (Or.inr (hall h hh))))
