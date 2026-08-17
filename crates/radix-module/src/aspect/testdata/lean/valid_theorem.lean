/- A valid theorem with no axioms and no sorry. -/

theorem valid_simple_id : 1 = 1 := rfl

theorem valid_add_zero (n : Nat) : n + 0 = n := by
  simp

theorem valid_zero_add (n : Nat) : 0 + n = n := by
  simp
