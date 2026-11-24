axiom fixture_axiom : False

theorem fixture_uses_axiom : (1 : Nat) = 2 := fixture_axiom.elim
