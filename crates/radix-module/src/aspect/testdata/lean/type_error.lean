/- A module with a Lean type error (Nat ≠ String). -/

theorem type_error_mismatch : (1 : Nat) = "hello" := rfl
