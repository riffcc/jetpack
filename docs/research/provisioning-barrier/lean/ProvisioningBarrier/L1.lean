import ProvisioningBarrier.L0

/-!
# L1 — %-threshold policy (scale-tolerant convergence)

Layer 0 proved the gate correct under strict-all (converge only when *every* host is ready).
At datacenter scale that is the wrong rule: failure is a forcing function, so the play should
converge once a *threshold* of hosts are ready, tolerating stragglers.

The new content of this layer is **readiness monotonicity**: the count of ready hosts never
decreases — `ready`/`failed` are terminal, and the only host-state change touching the count
is `provisioning → ready`, which adds one. So once a threshold is met it is met forever; the
gate can proceed without waiting for the tail. Proven constructively over L0's existing
`Step` relation.
-/

namespace ProvisioningBarrier.L1

open L0 (HostState State Step Phase updateHost)

/-- Number of ready hosts (recursion over `Fin n`, mirroring `L0.provCount`). -/
def readyCount : {n : Nat} → (Fin n → HostState) → Nat
  | 0, _ => 0
  | _+1, f => (if f 0 = HostState.ready then 1 else 0) + readyCount (fun j => f j.succ)

/-- Flipping a provisioning host to `ready` raises `readyCount` by exactly one. -/
theorem readyCount_update_ready : ∀ {n : Nat} (f : Fin n → HostState) (i : Fin n),
    f i = HostState.provisioning →
    readyCount (updateHost f i HostState.ready) = readyCount f + 1 := by
  intro n
  induction n with
  | zero => intro f i hi; exact absurd i.isLt (Nat.not_lt_zero _)
  | succ m ih =>
    intro f i hi
    induction i using Fin.cases with
    | zero =>
      have hne : f 0 ≠ HostState.ready := by rw [hi]; decide
      simp only [readyCount, updateHost, Fin.succ_ne_zero, if_neg hne, if_true, if_false]
      omega
    | succ k =>
      have hi' : (fun j : Fin m => f j.succ) k = HostState.provisioning := hi
      have key := ih (fun j : Fin m => f j.succ) k hi'
      unfold updateHost at key
      simp only [readyCount, updateHost, Fin.succ_inj, if_neg (Ne.symm (Fin.succ_ne_zero k))]
                 at key ⊢
      omega

/-- Flipping a provisioning host to `failed` leaves `readyCount` unchanged. -/
theorem readyCount_update_failed : ∀ {n : Nat} (f : Fin n → HostState) (i : Fin n),
    f i = HostState.provisioning →
    readyCount (updateHost f i HostState.failed) = readyCount f := by
  intro n
  induction n with
  | zero => intro f i hi; exact absurd i.isLt (Nat.not_lt_zero _)
  | succ m ih =>
    intro f i hi
    induction i using Fin.cases with
    | zero =>
      have hne : f 0 ≠ HostState.ready := by rw [hi]; decide
      have hneF : HostState.failed ≠ HostState.ready := by decide
      simp only [readyCount, updateHost, Fin.succ_ne_zero, if_neg hne, if_neg hneF, if_true, if_false]
    | succ k =>
      have hi' : (fun j : Fin m => f j.succ) k = HostState.provisioning := hi
      have key := ih (fun j : Fin m => f j.succ) k hi'
      unfold updateHost at key
      simp only [readyCount, updateHost, Fin.succ_inj, if_neg (Ne.symm (Fin.succ_ne_zero k))]
                 at key ⊢
      omega

/-! ## Monotonicity: readiness only grows. -/

/-- Every transition leaves `readyCount` no smaller — readiness is monotone. -/
theorem readyCount_monotone {n : Nat} {s s' : State n} (h : Step n s s') :
    readyCount s.hosts ≤ readyCount s'.hosts := by
  cases h with
  | fanOut _ => exact Nat.le_refl _
  | hostReady i _ hi =>
    have key := readyCount_update_ready s.hosts i hi
    show readyCount s.hosts ≤ readyCount (updateHost s.hosts i HostState.ready)
    omega
  | hostFail i _ hi =>
    have key := readyCount_update_failed s.hosts i hi
    show readyCount s.hosts ≤ readyCount (updateHost s.hosts i HostState.failed)
    omega
  | allReadyProceed _ _ => exact Nat.le_refl _
  | someFailedEnd _ _ _ => exact Nat.le_refl _

/-- A threshold, once met, stays met across any transition. -/
theorem threshold_stable {n t : Nat} {s s' : State n} (hmet : t ≤ readyCount s.hosts)
    (h : Step n s s') : t ≤ readyCount s'.hosts :=
  Nat.le_trans hmet (readyCount_monotone h)

/-! ## Axiom basis. Constructive throughout — no excluded middle, no choice. -/

#print axioms readyCount_update_ready
#print axioms readyCount_update_failed
#print axioms readyCount_monotone
#print axioms threshold_stable

end ProvisioningBarrier.L1
