import ProvisioningBarrier.L0
import ProvisioningBarrier.L1
import ProvisioningBarrier.L2

/-!
# L3 — composition: the policy-gated engine

L0–L2 proved the gate correct under specific policies (strict-all, threshold, max-failures).
L3 unifies them under one parameterized theorem — **`proceed ⟹ policy(evidence)`** for *any*
policy — and draws the bridge to jetpack's play-level primitives.

A **policy** is a predicate over the *evidence* (the host-state function `s.hosts`), not over
the full state. This is the load-bearing choice: the gate the controller checks at the barrier
is exactly the invariant that holds at `converge`, because the evidence is unchanged by the
proceed step (only the phase moves). Safety then falls out by induction over reachability,
mirroring L0.

**Bridge to jetpack (informal — Lean cannot see Rust).** The policy-gated engine is what
jetpack's primitives compose into: `!wait_for_others` realizes the barrier (hold at `barrier`
until the evidence satisfies the policy); `!assert P` realizes the gated proceed (advance to
`converge` iff `P`); `!fail` realizes the failure path (`policyFail`). This layer proves the
*protocol-level* composition correct; the modules are its realization.
-/

namespace ProvisioningBarrier.L3

open L0 (HostState State Step Phase updateHost allTerminal)
open L1 (readyCount)
open L2 (failedCount)

/-- A convergence policy: a predicate over the controller's evidence (the host-state map). -/
abbrev Policy (n : Nat) := (Fin n → HostState) → Prop

/-- The policy-gated engine. Per-host dynamics are inherited from L0; the controller proceeds
to `converge` only when the policy holds, and fails cleanly when every host is terminal yet
the policy is unmet. -/
inductive GatedStep (n : Nat) (P : Policy n) : State n → State n → Prop where
  | fanOut (s : State n) (h : s.phase = Phase.imaging) :
      GatedStep n P s ⟨s.hosts, Phase.barrier⟩
  | hostReady (s : State n) (i : Fin n) (hBar : s.phase = Phase.barrier)
      (hi : s.hosts i = HostState.provisioning) :
      GatedStep n P s ⟨updateHost s.hosts i HostState.ready, Phase.barrier⟩
  | hostFail (s : State n) (i : Fin n) (hBar : s.phase = Phase.barrier)
      (hi : s.hosts i = HostState.provisioning) :
      GatedStep n P s ⟨updateHost s.hosts i HostState.failed, Phase.barrier⟩
  | policyProceed (s : State n) (hBar : s.phase = Phase.barrier) (hP : P s.hosts) :
      GatedStep n P s ⟨s.hosts, Phase.converge⟩
  | policyFail (s : State n) (hBar : s.phase = Phase.barrier) (hEnd : allTerminal s)
      (hnP : ¬ P s.hosts) :
      GatedStep n P s ⟨s.hosts, Phase.failed⟩

/-- Reflexive-transitive closure: `GatedMulti n P s s'` iff `s'` is reachable under policy P. -/
inductive GatedMulti (n : Nat) (P : Policy n) : State n → State n → Prop where
  | refl : ∀ s, GatedMulti n P s s
  | step : ∀ s s' s'', GatedStep n P s s' → GatedMulti n P s' s'' → GatedMulti n P s s''

/-! ## Gated safety: every reachable `converge` satisfies the policy. -/

/-- Safety invariant: `phase = converge ⟹ P evidence`. -/
@[reducible]
def GatedSafeInv {n : Nat} (P : Policy n) (s : State n) : Prop :=
  s.phase = Phase.converge → P s.hosts

private theorem barrier_ne_converge : ¬ (Phase.barrier = Phase.converge) := fun h => by cases h
private theorem failed_ne_converge : ¬ (Phase.failed = Phase.converge) := fun h => by cases h

/-- A single gated step cannot break the safety invariant. -/
theorem gated_step_preserves_safe {n : Nat} {P : Policy n} {s s' : State n}
    (h : GatedStep n P s s') : GatedSafeInv P s → GatedSafeInv P s' := by
  cases h with
  | fanOut _         => exact fun _ hc => absurd hc barrier_ne_converge
  | hostReady _ _ _  => exact fun _ hc => absurd hc barrier_ne_converge
  | hostFail _ _ _   => exact fun _ hc => absurd hc barrier_ne_converge
  | policyProceed _ hP => exact fun _ _ => hP
  | policyFail _ _ _ => exact fun _ hc => absurd hc failed_ne_converge

/-- Reachability under a policy preserves the safety invariant (induction over the derivation). -/
theorem gated_safety {n : Nat} {P : Policy n} {a b : State n}
    (m : GatedMulti n P a b) (h : GatedSafeInv P a) : GatedSafeInv P b := by
  induction m with
  | refl => exact h
  | step _ _ _ hs _ ih => exact ih (gated_step_preserves_safe hs h)

/-! ## L0/L1/L2 are instances of the unified gate. -/

/-- Strict-all (L0): every host ready. -/
def strictAll {n : Nat} (f : Fin n → HostState) : Prop := ∀ i, f i = HostState.ready

/-- %-threshold (L1): at least `t` ready. -/
def threshold {n : Nat} (t : Nat) (f : Fin n → HostState) : Prop := t ≤ readyCount f

/-- Max-failures (L2): at most `m` failed. -/
def maxFailures {n : Nat} (m : Nat) (f : Fin n → HostState) : Prop := failedCount f ≤ m

/-- Combined: threshold ∧ max-failures — a play may demand both. -/
def combined {n : Nat} (t m : Nat) (f : Fin n → HostState) : Prop :=
  threshold t f ∧ maxFailures m f

/-- Under strict-all, a reachable `converge` has every host ready (recovers L0 safety). -/
theorem strictAll_safety {n : Nat} {a b : State n} (m : GatedMulti n strictAll a b)
    (h : GatedSafeInv strictAll a) : GatedSafeInv strictAll b := gated_safety m h

/-- Under a threshold, a reachable `converge` has `t ≤ readyCount` (lands L1's gate semantics). -/
theorem threshold_safety {n t : Nat} {a b : State n} (m : GatedMulti n (threshold t) a b)
    (h : GatedSafeInv (threshold t) a) : GatedSafeInv (threshold t) b := gated_safety m h

/-- Under a failure cap, a reachable `converge` has `failedCount ≤ m` (lands L2's gate semantics). -/
theorem maxFailures_safety {n m : Nat} {a b : State n} (m' : GatedMulti n (maxFailures m) a b)
    (h : GatedSafeInv (maxFailures m) a) : GatedSafeInv (maxFailures m) b := gated_safety m' h

/-- Under the combined policy, a reachable `converge` meets both threshold and cap. -/
theorem combined_safety {n t m : Nat} {a b : State n} (m' : GatedMulti n (combined t m) a b)
    (h : GatedSafeInv (combined t m) a) : GatedSafeInv (combined t m) b := gated_safety m' h

/-! ## Axiom basis. Constructive throughout — no excluded middle, no choice. -/

#print axioms gated_step_preserves_safe
#print axioms gated_safety

end ProvisioningBarrier.L3
