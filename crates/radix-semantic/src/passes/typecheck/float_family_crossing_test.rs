//! d11-6 rulings 16 and 20: integers never cross into floats implicitly;
//! constants into float slots are checked once at compile time.

use crate::test_support::{assert_accepts_la, assert_rejects_with_issue_la};

#[test]
fn typed_integer_into_float_slot_rejects() {
    assert_rejects_with_issue_la(
        "incipit { fixum i32 n ← 1 \n fixum f64 g ← n }",
        "integer_to_float_requires_conversio",
    );
}

#[test]
fn typed_i64_into_float_slot_rejects() {
    assert_rejects_with_issue_la(
        "incipit { fixum i64 n ← 1 \n fixum f64 g ← n }",
        "integer_to_float_requires_conversio",
    );
}

#[test]
fn explicit_conversio_of_typed_integer_to_float_accepts() {
    assert_accepts_la("incipit { fixum i64 n ← 1 \n fixum f64 g ← n ↦ f64 }");
}

#[test]
fn integer_literal_into_float_slot_accepts() {
    assert_accepts_la("incipit { fixum fractus f ← 3 }");
    assert_accepts_la("incipit { fixum f64 f ← 9007199254740992 }");
    assert_accepts_la("incipit { fixum f32 f ← 16777216 }");
}

#[test]
fn inexact_integer_literal_into_float_slot_rejects() {
    assert_rejects_with_issue_la(
        "incipit { fixum f64 f ← 9007199254740993 }",
        "float_constant_inexact",
    );
    assert_rejects_with_issue_la(
        "incipit { fixum f32 f ← 16777217 }",
        "float_constant_inexact",
    );
}

#[test]
fn typed_integer_assignment_carries_conversio_issue() {
    assert_rejects_with_issue_la(
        "incipit { fixum i32 n ← 1 \n varia f64 g ← 1.0 \n g ← n }",
        "integer_to_float_requires_conversio",
    );
}

#[test]
fn negative_integer_literal_into_float_slot_accepts() {
    assert_accepts_la("incipit { fixum fractus f ← -3 }");
    assert_accepts_la("incipit { fixum f64 f ← -9007199254740992 }");
}

#[test]
fn inexact_negative_integer_literal_into_float_slot_rejects() {
    assert_rejects_with_issue_la(
        "incipit { fixum f64 f ← -9007199254740993 }",
        "float_constant_inexact",
    );
}

/// Ruling 20 follow-up: an integer constant admitted into a float slot must
/// leave typecheck as a float constant, not an integer literal with a float
/// type, so MIR lowering and every backend see one representation.
mod constant_representation {
    use crate::test_support::analyze_ok_la;
    use radix_hir::visit::{HirVisitor, walk_expr};
    use radix_hir::{HirExpression, HirExpressionKind, HirLiteral};

    #[derive(Default)]
    struct Literals {
        ints: Vec<u64>,
        floats: Vec<f64>,
    }

    impl HirVisitor for Literals {
        fn visit_expr(&mut self, expr: &HirExpression) {
            if let HirExpressionKind::Literal(lit) = &expr.kind {
                match lit {
                    HirLiteral::Int(v) => self.ints.push(*v),
                    HirLiteral::Float(v) => self.floats.push(*v),
                    _ => {}
                }
            }
            walk_expr(self, expr);
        }
    }

    fn literals(source: &str) -> Literals {
        let result = analyze_ok_la(source);
        let mut lits = Literals::default();
        lits.visit_program(result.hir.as_ref().expect("hir"));
        lits
    }

    #[test]
    fn local_initializer_becomes_float_constant() {
        let l = literals("incipit { fixum fractus f ← 3 }");
        assert!(l.ints.is_empty(), "ints: {:?}", l.ints);
        assert_eq!(l.floats, vec![3.0]);
    }

    #[test]
    fn argument_return_list_and_field_default_become_float_constants() {
        let l = literals(
            "functio f(fractus x) → fractus { redde 3 }\n\
             functio g() → fractus { redde f(4) }\n\
             genus B { varia fractus area = 5 }\n\
             incipit { fixum lista<fractus> xs ← [1, 2] }",
        );
        assert!(l.ints.is_empty(), "ints: {:?}", l.ints);
        assert_eq!(l.floats, vec![3.0, 4.0, 5.0, 1.0, 2.0]);
    }
}
