import ProvisioningBarrier.L0
import ProvisioningBarrier.L1

/-!
# L2 — max-failures policy (and coupling with the threshold)

L1's threshold watches `readyCount` in isolation. L2 adds the failure axis and, with it, the
result that *tightens* L1: the three host-counts **partition** the host set —
`readyCount + failedCount + provCount = n` always — so a threshold on ready and a cap on
failed are not independent; they draw from the same finite pool. The **failure ratchet**
(`failedCount` only grows — a host, once failed, stays failed) gives the failure bound the
opposite temporal character to the threshold: a lower bound on a growing quantity is stable
forward (L1), an *upper* bound on a growing quantity is not — which is why a max-failures
gate is evaluated at proceed, not pre-committed.
-/

namespace ProvisioningBarrier.L2

open L0 (HostState State Step Phase updateHost provCount)
open L1 (readyCount)

/-- Number of failed hosts (recursion over `Fin n`). -/
def failedCount : {n : Nat} → (Fin n → HostState) → Nat
  | 0, _ => 0
  | _+1, f => (if f 0 = HostState.failed then 1 else 0) + failedCount (fun j => f j.succ)

/-- Flipping a provisioning host to `failed` raises `failedCount` by exactly one. -/
theorem failedCount_update_failed : ∀ {n : Nat} (f : Fin n → HostState) (i : Fin n),
    f i = HostState.provisioning →
    failedCount (updateHost f i HostState.failed) = failedCount f + 1 := by
  intro n
  induction n with
  | zero => intro f i hi; exact absurd i.isLt (Nat.not_lt_zero _)
  | succ m ih =>
    intro f i hi
    induction i using Fin.cases with
    | zero =>
      have hne : f 0 ≠ HostState.failed := by rw [hi]; decide
      simp only [failedCount, updateHost, Fin.succ_ne_zero, if_neg hne, if_true, if_false]
      omega
    | succ k =>
      have hi' : (fun j : Fin m => f j.succ) k = HostState.provisioning := hi
      have key := ih (fun j : Fin m => f j.succ) k hi'
      unfold updateHost at key
      simp only [failedCount, updateHost, Fin.succ_inj, if_neg (Ne.symm (Fin.succ_ne_zero k))]
                 at key ⊢
      omega

/-- Flipping a provisioning host to `ready` leaves `failedCount` unchanged. -/
theorem failedCount_update_ready : ∀ {n : Nat} (f : Fin n → HostState) (i : Fin n),
    f i = HostState.provisioning →
    failedCount (updateHost f i HostState.ready) = failedCount f := by
  intro n
  induction n with
  | zero => intro f i hi; exact absurd i.isLt (Nat.not_lt_zero _)
  | succ m ih =>
    intro f i hi
    induction i using Fin.cases with
    | zero =>
      have hne : f 0 ≠ HostState.failed := by rw [hi]; decide
      have hneR : HostState.ready ≠ HostState.failed := by decide
      simp only [failedCount, updateHost, Fin.succ_ne_zero, if_neg hne, if_neg hneR, if_true, if_false]
    | succ k =>
      have hi' : (fun j : Fin m => f j.succ) k = HostState.provisioning := hi
      have key := ih (fun j : Fin m => f j.succ) k hi'
      unfold updateHost at key
      simp only [failedCount, updateHost, Fin.succ_inj, if_neg (Ne.symm (Fin.succ_ne_zero k))]
                 at key ⊢
      omega

/-! ## The failure ratchet. -/

/-- Every transition leaves `failedCount` no smaller — failures are monotone. -/
theorem failedCount_monotone {n : Nat} {s s' : State n} (h : Step n s s') :
    failedCount s.hosts ≤ failedCount s'.hosts := by
  cases h with
  | fanOut _ => exact Nat.le_refl _
  | hostReady i _ hi =>
    have key := failedCount_update_ready s.hosts i hi
    show failedCount s.hosts ≤ failedCount (updateHost s.hosts i HostState.ready)
    omega
  | hostFail i _ hi =>
    have key := failedCount_update_failed s.hosts i hi
    show failedCount s.hosts ≤ failedCount (updateHost s.hosts i HostState.failed)
    omega
  | allReadyProceed _ _ => exact Nat.le_refl _
  | someFailedEnd _ _ _ => exact Nat.le_refl _

/-! ## Partition: the counts couple the policies (the tightening of L1). -/

/-- Each host contributes exactly one across the three counts. -/
theorem one_per_host (st : HostState) :
    (if st = HostState.ready then 1 else 0) + (if st = HostState.failed then 1 else 0)
      + (if st = HostState.provisioning then 1 else 0) = 1 := by
  cases st <;> decide

/-- The ready, failed, and provisioning counts partition the host set. -/
theorem partition : ∀ (n : Nat) (f : Fin n → HostState),
    readyCount f + failedCount f + provCount f = n := by
  intro n
  induction n with
  | zero => intro f; simp [readyCount, failedCount, provCount]
  | succ m ih =>
    intro f
    simp only [readyCount, failedCount, provCount]
    have hih := ih (fun j : Fin m => f j.succ)
    have hw := one_per_host (f 0)
    omega

/-! ## Axiom basis. Constructive throughout — no excluded middle, no choice. -/

#print axioms failedCount_update_failed
#print axioms failedCount_update_ready
#print axioms failedCount_monotone
#print axioms partition

end ProvisioningBarrier.L2
