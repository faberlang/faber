//! Tensor operator shape laws at the semantic boundary.
//!
//! A tensor is never a scalar, even at rank zero. Glyphs consume exact shapes;
//! an explicit expanded view is the only shape-repair boundary tested here.

use crate::test_support::{AnalyzedFixture, analyze_source_with_config};
use crate::{PassConfig, SemanticErrorKind};
use radix_hir::HirStatementKind;
use radix_target::Target;
use radix_types::{IndexExpr, NumericWidth, Primitive, Type};

fn analyze(source: &str) -> AnalyzedFixture {
    let mut config = PassConfig::for_target(Target::HirRust);
    config.locale_pack = Some(
        crate::locale::LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("en"))
            .expect("English test surface"),
    );
    analyze_source_with_config(source, config)
}

fn rejects(source: &str, kind: SemanticErrorKind, issue: &str) {
    let result = analyze(source);
    assert!(!result.is_ok(), "unexpected acceptance:\n{source}");
    assert!(
        result
            .diagnostics
            .iter()
            .any(|err| err.is_error() && err.kind == kind && err.issue() == Some(issue)),
        "expected {kind:?}.{issue}, got {:?}\n{source}",
        result.diagnostics
    );
}

fn tensor_pair(left: &str, right: &str, op: &str) -> String {
    format!(
        "main {{
    let tensor<f32, {left}> a ← empty
    let tensor<f32, {right}> b ← empty
    let _ out ← a {op} b
}}"
    )
}

fn rejects_tensor_multiply(left: &str, right: &str) {
    rejects(
        &tensor_pair(left, right, "*"),
        SemanticErrorKind::OperandTypesInvalid,
        "tensor_multiply_requires_glyph",
    );
}

/// Inspect the inferred initializer, not an expected type that could mask a
/// wrong result shape. Every positive fixture ends with the output binding.
fn accepts_shape(source: &str, expected: &[u64]) {
    let result = analyze(source);
    assert!(result.is_ok(), "{:?}\n{source}", result.diagnostics);
    let hir = result.hir.as_ref().expect("typed HIR");
    let entry = hir.entry.as_ref().expect("main");
    let HirStatementKind::Local(local) = &entry.statements.last().expect("out binding").kind else {
        panic!("last statement must be the output binding");
    };
    let ty = local.init.as_ref().expect("initializer").ty.expect("type");
    let Type::Tensor(elem, shape) = result.types.get(ty) else {
        panic!("expected tensor result, got {:?}", result.types.get(ty));
    };
    assert_eq!(
        result.types.get(*elem),
        &Type::SizedNumeric(Primitive::Fractus, NumericWidth::F32),
        "scalar lifting must preserve the f32 element"
    );
    let IndexExpr::Tuple(dims) = result.types.get_index(*shape) else {
        panic!(
            "expected resolved shape, got {:?}",
            result.types.get_index(*shape)
        );
    };
    assert_eq!(dims.len(), expected.len(), "result rank");
    for (dim, extent) in dims.iter().zip(expected) {
        assert_eq!(result.types.get_index(*dim), &IndexExpr::Literal(*extent));
    }
}

#[test]
fn tensor_multiply_rejects_rank_zero() {
    rejects_tensor_multiply("[]", "[]");
}

#[test]
fn tensor_multiply_rejects_rank_one() {
    rejects_tensor_multiply("[3]", "[3]");
}

#[test]
fn tensor_multiply_rejects_rank_two() {
    rejects_tensor_multiply("[2, 3]", "[2, 3]");
}

#[test]
fn tensor_multiply_rejects_rank_three() {
    rejects_tensor_multiply("[2, 3, 4]", "[2, 3, 4]");
}

#[test]
fn tensor_multiply_rejects_unequal_ranks() {
    for (left, right) in [("[]", "[3]"), ("[3]", "[2, 3]"), ("[2, 3]", "[4, 2, 3]")] {
        rejects_tensor_multiply(left, right);
        rejects_tensor_multiply(right, left);
    }
}

#[test]
fn tensor_multiply_rejects_size_one_compatible_shapes() {
    for (left, right) in [
        ("[1]", "[3]"),
        ("[2, 1]", "[2, 3]"),
        ("[1, 3, 1]", "[2, 3, 4]"),
    ] {
        rejects_tensor_multiply(left, right);
        rejects_tensor_multiply(right, left);
    }
}

#[test]
fn tensor_scalar_lifting_preserves_shape() {
    for shape in [vec![], vec![3], vec![2, 3], vec![2, 3, 4]] {
        for op in ["*", "+", "-"] {
            accepts_shape(
                &format!(
                    "main {{ let tensor<f32, {shape:?}> a ← empty
    let _ out ← a {op} 2.0 }}"
                ),
                &shape,
            );
        }
    }
}

#[test]
fn scalar_tensor_lifting_preserves_shape() {
    for shape in [vec![], vec![3], vec![2, 3], vec![2, 3, 4]] {
        for op in ["*", "+", "-"] {
            accepts_shape(
                &format!(
                    "main {{ let tensor<f32, {shape:?}> a ← empty
    let _ out ← 2.0 {op} a }}"
                ),
                &shape,
            );
        }
    }
}

#[test]
fn tensor_plus_rejects_rank_extension() {
    for op in ["+", "-"] {
        for (left, right) in [("[]", "[3]"), ("[3]", "[2, 3]"), ("[2, 3]", "[4, 2, 3]")] {
            for (lhs, rhs) in [(left, right), (right, left)] {
                rejects(
                    &tensor_pair(lhs, rhs, op),
                    SemanticErrorKind::TypeMismatch,
                    "incompatible_tensor_index",
                );
            }
        }
    }
}

#[test]
fn tensor_minus_rejects_size_one_stretch() {
    for op in ["-", "+"] {
        for (left, right) in [
            ("[1]", "[3]"),
            ("[2, 1]", "[2, 3]"),
            ("[1, 3, 1]", "[2, 3, 4]"),
        ] {
            for (lhs, rhs) in [(left, right), (right, left)] {
                rejects(
                    &tensor_pair(lhs, rhs, op),
                    SemanticErrorKind::TypeMismatch,
                    "incompatible_tensor_index",
                );
            }
        }
    }
}

#[test]
fn hadamard_rejects_unequal_shapes() {
    for op in ["⊙", "⊘"] {
        for (left, right) in [("[3]", "[2]"), ("[3]", "[2, 3]"), ("[2, 1]", "[2, 3]")] {
            for (lhs, rhs) in [(left, right), (right, left)] {
                rejects(
                    &tensor_pair(lhs, rhs, op),
                    SemanticErrorKind::OperandTypesInvalid,
                    "glyph_hadamard_shapes_mismatch",
                );
            }
        }
    }
}

#[test]
fn explicit_expanded_view_allows_exact_shape_glyphs() {
    for op in ["⊙", "+", "-"] {
        accepts_shape(
            &format!(
                "main {{
    let tensor<f32, [3]> row ← empty
    let tensor<f32, [2, 3]> grid ← empty
    let _ out ← row.expanded([2, 3]) {op} grid
}}"
            ),
            &[2, 3],
        );
    }
}
