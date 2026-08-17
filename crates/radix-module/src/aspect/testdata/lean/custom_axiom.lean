/- A module declaring and using a custom axiom (not in the allowlist). -/

axiom my_custom_axiom : 1 = 2

theorem custom_axiom_example : 1 = 2 := my_custom_axiom
