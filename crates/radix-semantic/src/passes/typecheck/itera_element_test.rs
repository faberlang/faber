//! D19.4: `itera` element typing has no silent `numerus` fallback.
//!
//! Every iterable binds its real element type (`copia<T>` binds `T`,
//! `textus` binds `littera` (D10.1), tensors bind their element), and a
//! non-iterable source is `itera_source_not_iterable`. Assertions are
//! structured (issue identity), never rendered prose.

use crate::test_support;
use radix_hir::{HirExpressionKind, HirItemKind, HirStatementKind};

const NOT_ITERABLE: &str = "itera_source_not_iterable";

/// Type recorded on the `itera` source expression of the first function body.
fn itera_source_type(unit: &test_support::AnalyzedFixture) -> Option<radix_types::Type> {
    let hir = unit.hir.as_ref()?;
    let body = hir.items.iter().find_map(|item| match &item.kind {
        HirItemKind::Function(function) => function.body.as_ref(),
        _ => None,
    })?;
    let expr = body
        .statements
        .iter()
        .find_map(|statement| match &statement.kind {
            HirStatementKind::Expr(expr) => Some(expr),
            _ => None,
        })?;
    let HirExpressionKind::Itera { iter, .. } = &expr.kind else {
        return None;
    };
    Some(unit.types.get(iter.ty?).clone())
}

#[test]
fn itera_ex_copia_textus_binds_textus() {
    test_support::assert_accepts(
        "functio f(copia<textus> t) → vacuum {
    itera ex t sit x {
        sit textus y ← x
        nota y
    }
}",
    );
}

#[test]
fn itera_ex_copia_textus_does_not_bind_numerus() {
    test_support::assert_rejects_with_issue(
        "functio f(copia<textus> t) → vacuum {
    itera ex t sit x {
        sit numerus y ← x
        nota y
    }
}",
        "initializer_annotation_mismatch",
    );
}

#[test]
fn itera_ex_genus_rejects_as_not_iterable() {
    test_support::assert_rejects_with_issue(
        "genus Arbor { varia numerus magnitudo }
functio f(Arbor a) → vacuum {
    itera ex a sit n {
        nota n
    }
}",
        NOT_ITERABLE,
    );
}

#[test]
fn itera_de_genus_rejects_as_not_iterable() {
    test_support::assert_rejects_with_issue(
        "genus Arbor { varia numerus magnitudo }
functio f(Arbor a) → vacuum {
    itera de a sit n {
        nota n
    }
}",
        NOT_ITERABLE,
    );
}

#[test]
fn itera_ex_bivalens_rejects_as_not_iterable() {
    test_support::assert_rejects_with_issue(
        "functio f(bivalens b) → vacuum {
    itera ex b sit n {
        nota n
    }
}",
        NOT_ITERABLE,
    );
}

#[test]
fn itera_ex_textus_binds_littera() {
    test_support::assert_accepts(
        "functio f(textus s) → vacuum {
    itera ex s sit c {
        sit littera x ← c
        nota x
    }
}",
    );
}

#[test]
fn itera_ex_textus_element_compares_with_char_literal() {
    test_support::assert_accepts(
        "functio f(textus s) → bivalens {
    itera ex s sit c {
        si c ≡ 'b' { redde verum }
    }
    redde falsum
}",
    );
}

#[test]
fn itera_ex_textus_element_is_not_numerus() {
    test_support::assert_rejects_with_issue(
        "functio f(textus s) → vacuum {
    itera ex s sit c {
        sit numerus x ← c
        nota x
    }
}",
        "initializer_annotation_mismatch",
    );
}

#[test]
fn itera_ex_octeti_binds_index_element_type() {
    test_support::assert_accepts(
        "functio f(lista<b8> b) → vacuum {
    itera ex b sit x {
        sit b8 y ← x
        nota y
    }
}",
    );
}

#[test]
fn itera_ex_symbolic_rank1_tensor_binds_element_type() {
    test_support::assert_accepts(
        "functio f<magnitudo N>(tensor<f32, [N]> v) → vacuum {
    itera ex v sit x {
        sit f32 y ← x
        nota y
    }
}",
    );
}

#[test]
fn itera_ordo_ex_members_check_and_bind_the_enum() {
    // Operator ruling 2026-10-15: an enum name in `itera ex`
    // source position iterates the enum's own members in declared order. The
    // loop checks with no error and the binder takes the enum type; the
    // negative annotation is refused by the binder's real row, never by
    // falling back to `error_type` or `numerus`.
    let unit = test_support::analyze_source(
        "ordo Color { rubrum, viridis, caeruleum }
functio f() → vacuum {
    itera ex Color sit c {
        sit Color x ← c
        nota x
    }
}",
    );
    assert!(unit.is_ok(), "diagnostics: {:?}", unit.diagnostics);
    let source_ty = itera_source_type(&unit).expect("enum iteration source type");
    assert!(
        matches!(source_ty, radix_types::Type::Enum(_)),
        "binder source dispatch type is the enum itself, got {source_ty:?}"
    );
    test_support::assert_rejects_with_issue(
        "ordo Color { rubrum, viridis, caeruleum }
functio f() → vacuum {
    itera ex Color sit c {
        sit numerus x ← c
        nota x
    }
}",
        "initializer_annotation_mismatch",
    );
}

#[test]
fn itera_ordo_ex_genus_value_source_rejects_not_iterable() {
    test_support::assert_rejects_with_issue(
        "genus Arbor { varia numerus magnitudo }
functio f(Arbor a) → vacuum {
    itera ex a sit n {
        nota n
    }
}",
        NOT_ITERABLE,
    );
}

#[test]
fn itera_ordo_de_enum_source_rejects_not_iterable() {
    test_support::assert_rejects_with_issue(
        "ordo Color { rubrum, viridis, caeruleum }
functio f() → vacuum {
    itera de Color sit c {
        nota c
    }
}",
        NOT_ITERABLE,
    );
}
