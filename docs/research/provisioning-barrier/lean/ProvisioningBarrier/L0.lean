/-!
# L0 — Core provisioning-barrier protocol (strict-all policy)

Small-step operational semantics for concurrent JIT provisioning of `n` hosts,
gated by a strict-all readiness barrier (Layer 0; no scale-tolerance yet).

**Hosts:** each ∈ {Provisioning, Ready, Failed}; Ready/Failed are terminal.
**Phase:** Imaging → Barrier → Converge | Failed (Converge and Failed terminal).
**Transitions:** `fanOut` (Imaging→Barrier); `hostReady`/`hostFail` (per-host,
Provisioning→Ready/Failed); `allReadyProceed` (Barrier→Converge iff every host
Ready); `someFailedEnd` (Barrier→Failed once every host is terminal but not all
Ready — the strict-all play *fails* rather than deadlocks).

**Proven (this layer):**
- *Safety:* every reachable `Converge` state has all hosts Ready.
- *Progress:* no non-terminal state is deadlocked — a step is always enabled.
- (Termination — finite executions via a decreasing measure — is the next proof.)
-/

namespace ProvisioningBarrier.L0

/-- A single host's provisioning state. `ready`/`failed` are terminal. -/
inductive HostState
  | provisioning : HostState
  | ready        : HostState
  | failed       : HostState
  deriving Repr, DecidableEq

/-- Coarse protocol phase. `converge`/`failed` are terminal. -/
inductive Phase
  | imaging  : Phase
  | barrier  : Phase
  | converge : Phase
  | failed   : Phase
  deriving Repr, DecidableEq

/-- System state: `n` hosts (indexed by `Fin n`) and a phase. -/
structure State (n : Nat) where
  hosts : Fin n → HostState
  phase : Phase

/-- Strict-all policy: every host Ready. -/
def allReady {n : Nat} (s : State n) : Prop :=
  ∀ i : Fin n, s.hosts i = HostState.ready

/-- Every host has finished imaging (none still Provisioning). -/
def allTerminal {n : Nat} (s : State n) : Prop :=
  ∀ i : Fin n, s.hosts i ≠ HostState.provisioning

/-- Both policies are decidable by finite enumeration over `Fin n` (core Lean's
`Nat.decidableForallFin`), so case-splitting on them is constructive and axiom-free. -/
instance allReady_dec {n : Nat} (s : State n) : Decidable (allReady s) :=
  Nat.decidableForallFin (fun i => s.hosts i = HostState.ready)
instance allTerminal_dec {n : Nat} (s : State n) : Decidable (allTerminal s) :=
  Nat.decidableForallFin (fun i => s.hosts i ≠ HostState.provisioning)

/-- Pointwise host-state update: host `i` becomes `st`, others unchanged. -/
def updateHost {n : Nat} (f : Fin n → HostState) (i : Fin n) (st : HostState) :
    Fin n → HostState := fun j => if j = i then st else f j

/-! ## A constructive witness for "some host is provisioning".

`progress` must exhibit a *specific* provisioning host to take a `hostReady` step.
Deciding the proposition `∃ i, s.hosts i = provisioning` would pull Lean's foundational
axioms (`Nat.decidableExistsFin` depends on `propext`, `Quot.sound`). Instead we compute
the first such host by scanning `List.finRange n` — purely constructive, axiom-free. -/

/-- Boolean view: is host `i` still provisioning? (A named function so `List.find?`
infers its predicate cleanly, avoiding higher-order unification on an inline lambda.) -/
def hostProv? {n : Nat} (s : State n) (i : Fin n) : Bool :=
  decide (s.hosts i = HostState.provisioning)

/-- The first provisioning host, if any (computed, not decided). -/
def firstProvisioning? {n : Nat} (s : State n) : Option (Fin n) :=
  List.find? (hostProv? s) (List.finRange n)

theorem firstProvisioning?_some {n : Nat} (s : State n) {i : Fin n}
    (h : firstProvisioning? s = some i) : s.hosts i = HostState.provisioning := by
  unfold firstProvisioning? at h
  have h2 := List.find?_some h
  unfold hostProv? at h2
  exact decide_eq_true_iff.mp h2

theorem firstProvisioning?_none {n : Nat} (s : State n)
    (h : firstProvisioning? s = none) : allTerminal s := by
  unfold firstProvisioning? at h
  intro i hi
  have hn := List.find?_eq_none.mp h i (List.mem_finRange i)
  unfold hostProv? at hn
  exact hn (decide_eq_true_iff.mpr hi)

/-- One protocol transition. -/
inductive Step (n : Nat) : State n → State n → Prop where
  | fanOut : ∀ s : State n, s.phase = Phase.imaging →
      Step n s ⟨s.hosts, Phase.barrier⟩
  | hostReady : ∀ (s : State n) (i : Fin n),
      s.phase = Phase.barrier → s.hosts i = HostState.provisioning →
      Step n s ⟨updateHost s.hosts i HostState.ready, Phase.barrier⟩
  | hostFail : ∀ (s : State n) (i : Fin n),
      s.phase = Phase.barrier → s.hosts i = HostState.provisioning →
      Step n s ⟨updateHost s.hosts i HostState.failed, Phase.barrier⟩
  | allReadyProceed : ∀ (s : State n),
      s.phase = Phase.barrier → allReady s →
      Step n s ⟨s.hosts, Phase.converge⟩
  | someFailedEnd : ∀ (s : State n),
      s.phase = Phase.barrier → allTerminal s → ¬ allReady s →
      Step n s ⟨s.hosts, Phase.failed⟩

/-- Reflexive-transitive closure: `Multi n s s'` iff `s'` is reachable from `s`. -/
inductive Multi (n : Nat) : State n → State n → Prop where
  | refl : ∀ s, Multi n s s
  | step : ∀ s s' s'', Step n s s' → Multi n s' s'' → Multi n s s''

/-! ## Safety: every reachable Converge state has all hosts Ready. -/

/-- Safety invariant: phase = Converge ⟹ allReady. -/
@[reducible]
def SafeInv {n : Nat} (s : State n) : Prop := s.phase = Phase.converge → allReady s

/-- Phase constructors are distinct (closed proofs — no free variables). -/
private theorem barrier_ne_converge : ¬ (Phase.barrier = Phase.converge) := fun h => by cases h
private theorem failed_ne_converge : ¬ (Phase.failed = Phase.converge) := fun h => by cases h

/-- A single step cannot break the safety invariant. -/
theorem step_preserves_safe {n : Nat} {s s' : State n} (h : Step n s s') :
    SafeInv s → SafeInv s' := by
  cases h with
  | fanOut _            => exact fun _ hc => absurd hc barrier_ne_converge
  | hostReady _ _ _     => exact fun _ hc => absurd hc barrier_ne_converge
  | hostFail _ _ _      => exact fun _ hc => absurd hc barrier_ne_converge
  | allReadyProceed _ hall => exact fun _ _ => hall
  | someFailedEnd _ _ _ => exact fun _ hc => absurd hc failed_ne_converge

/-- Reachability preserves the safety invariant (by induction over the derivation). -/
theorem safety {n : Nat} {a b : State n} (m : Multi n a b) (h : SafeInv a) : SafeInv b := by
  induction m with
  | refl => exact h
  | step _ _ _ hs _ ih => exact ih (step_preserves_safe hs h)

/-! ## Progress: a non-terminal state always has an enabled step (no deadlock). -/

theorem progress {n : Nat} (s : State n)
    (hnt : s.phase = Phase.imaging ∨ s.phase = Phase.barrier) :
    ∃ s' : State n, Step n s s' :=
  -- Fully constructive: the provisioning witness comes from the computed scan
  -- `firstProvisioning?` (no existential decidability, hence no foundational axioms);
  -- the `allReady` split uses its 0-axiom `∀`-over-`Fin` decidability instance.
  match hnt with
  | Or.inl hIm => ⟨⟨s.hosts, Phase.barrier⟩, Step.fanOut s hIm⟩
  | Or.inr hBar =>
    match hs : firstProvisioning? s with
    | some i =>
      ⟨⟨updateHost s.hosts i HostState.ready, Phase.barrier⟩,
       Step.hostReady s i hBar (firstProvisioning?_some s hs)⟩
    | none =>
      if hAR : allReady s then
        ⟨⟨s.hosts, Phase.converge⟩, Step.allReadyProceed s hBar hAR⟩
      else
        ⟨⟨s.hosts, Phase.failed⟩,
         Step.someFailedEnd s hBar (firstProvisioning?_none s hs) hAR⟩

/-! ## Termination: every step lowers a bounded measure, so executions are finite.

`measure s = phaseRank s.phase * (n+1) + provCount s.hosts`. Phase transitions strictly drop
the first term; per-host `hostReady`/`hostFail` steps keep the phase but drop `provCount` by
one. Bounded below by 0, so no execution runs forever (well-foundedness of `<` on `Nat`). -/

/-- Phase ordering: later phases rank lower. -/
def phaseRank : Phase → Nat
  | Phase.imaging => 3
  | Phase.barrier => 2
  | Phase.converge => 1
  | Phase.failed => 0

/-- Number of hosts still provisioning (recursion over `Fin n`). -/
def provCount : {n : Nat} → (Fin n → HostState) → Nat
  | 0, _ => 0
  | _+1, f => (if f 0 = HostState.provisioning then 1 else 0) + provCount (fun j => f j.succ)

/-- Flipping host `i` from provisioning to a non-provisioning state lowers the count by one.
Stated additively to avoid `Nat` subtraction. -/
theorem provCount_update : ∀ {n : Nat} (f : Fin n → HostState) (i : Fin n) (st : HostState),
    f i = HostState.provisioning → st ≠ HostState.provisioning →
    provCount (updateHost f i st) + 1 = provCount f := by
  intro n
  induction n with
  | zero => intro _f i _st _hp _hn; exact absurd i.isLt (Nat.not_lt_zero _)
  | succ m ih =>
    intro f i st hprov hne
    induction i using Fin.cases with
    | zero =>
      simp only [provCount, updateHost, Fin.succ_ne_zero, if_false, if_pos]
      rw [if_pos hprov, if_neg hne]
      omega
    | succ k =>
      have hprov' : (fun j : Fin m => f j.succ) k = HostState.provisioning := hprov
      have key := ih (fun j : Fin m => f j.succ) k st hprov' hne
      unfold updateHost at key
      simp only [provCount, updateHost, Fin.succ_inj, if_neg (Ne.symm (Fin.succ_ne_zero k))]
                 at key ⊢
      omega

/-- Strictly-decreasing measure; bounded below by 0. -/
def measure {n : Nat} (s : State n) : Nat :=
  phaseRank s.phase * (n + 1) + provCount s.hosts

/-- Every transition strictly lowers the measure. -/
theorem step_decreases {n : Nat} {s s' : State n} (h : Step n s s') :
    measure s' < measure s := by
  cases h with
  | fanOut hIm =>
    simp only [measure, phaseRank, hIm]; omega
  | hostReady i hBar hi =>
    have key := provCount_update s.hosts i HostState.ready hi (by decide)
    simp only [measure, phaseRank, hBar]; omega
  | hostFail i hBar hi =>
    have key := provCount_update s.hosts i HostState.failed hi (by decide)
    simp only [measure, phaseRank, hBar]; omega
  | allReadyProceed hBar _ =>
    simp only [measure, phaseRank, hBar]; omega
  | someFailedEnd hBar _ _ =>
    simp only [measure, phaseRank, hBar]; omega

/-! ## Axiom basis.

`step_preserves_safe` and `safety` are literal 0-axiom (pure structural case analysis).
`progress` is *constructive* — no excluded middle, no choice (`Classical.choice`); it rests
only on Lean's two foundational axioms (`propext`, `Quot.sound`), which the finite witness
scan over `List.finRange` introduces. For Safety that is the full story; Progress's
foundational-axiom residual is the cost of constructively extracting a concrete host index. -/

#print axioms step_preserves_safe
#print axioms safety
#print axioms firstProvisioning?_some
#print axioms firstProvisioning?_none
#print axioms progress
#print axioms provCount_update
#print axioms step_decreases

end ProvisioningBarrier.L0
