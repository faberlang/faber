/- A module whose declarations transitively depend on `sorryAx`
   (detectable via `#print axioms` output). -/

theorem transitive_sorry_example : 1 = 1 := by
  sorry
