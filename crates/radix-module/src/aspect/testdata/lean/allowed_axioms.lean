/- A module using only allowlisted axioms: `propext`, `Classical.choice`, `Quot.sound`. -/

theorem allowed_propext (p q : Prop) (h : p ↔ q) : p = q :=
  propext h

theorem allowed_classical_choice (p : Prop) [Nonempty p] : p :=
  Classical.choice inferInstance

theorem allowed_quot_sound {α : Type} (r : α → α → Prop) (a b : α) (h : r a b) :
    Quot.mk r a = Quot.mk r b :=
  Quot.sound h
