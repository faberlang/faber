//! MU-5a type-position parameter-mode fixtures.
//!
//! Proves the mode-unification fact: HIR parameter modes derive from the
//! type-position access marker when present (closure params and fn-type
//! params), and the marked type carries the mode. The legacy prefix slot stays
//! legal and is covered by the corpus + lint deprecation warning, not here.

use crate::test_support::{analyze_ok, analyze_source, issue};
use crate::{SemanticMutability, Type};

/// Pull the first `Clausura` from the implicit entry block of the fixture.
fn first_clausura_params(hir: &radix_hir::HirModule) -> &Vec<radix_hir::HirParam> {
    let entry = hir.entry.as_ref().expect("entry block");
    for stmt in &entry.statements {
        if let radix_hir::HirStatementKind::Local(local) = &stmt.kind {
            if let Some(init) = &local.init {
                if let radix_hir::HirExpressionKind::Clausura(params, _, _, _) = &init.kind {
                    return params;
                }
            }
        }
    }
    panic!("expected a clausura in the entry block")
}

#[test]
fn closure_params_derive_modes_from_type_position_markers() {
    let fixture = analyze_ok(
        r"
incipit {
    fixum _ f ← (de lista<numerus> a, in lista<numerus> b, own lista<numerus> c, copy lista<numerus> d) ∴ a
}
",
    );
    let hir = fixture.hir.as_ref().expect("hir");
    let params = first_clausura_params(hir);
    assert_eq!(params.len(), 4);
    assert_eq!(params[0].mode, radix_hir::HirParamMode::De);
    assert_eq!(params[1].mode, radix_hir::HirParamMode::In);
    assert_eq!(params[2].mode, radix_hir::HirParamMode::Owned);
    assert_eq!(params[3].mode, radix_hir::HirParamMode::Owned);

    // The type itself carries the mode as an access marker.
    assert!(matches!(
        fixture.types.get(params[0].ty),
        Type::Ref(SemanticMutability::Immutable, _)
    ));
    assert!(matches!(
        fixture.types.get(params[1].ty),
        Type::Ref(SemanticMutability::Mutable, _)
    ));
    assert!(matches!(
        fixture.types.get(params[2].ty),
        Type::Ref(SemanticMutability::Own, _)
    ));
    assert!(matches!(
        fixture.types.get(params[3].ty),
        Type::Ref(SemanticMutability::Copy, _)
    ));
}

#[test]
fn unmarked_closure_params_stay_owned() {
    let fixture = analyze_ok(
        r"
incipit {
    fixum _ f ← (lista<numerus> a, numerus n) ∴ n
}
",
    );
    let hir = fixture.hir.as_ref().expect("hir");
    let params = first_clausura_params(hir);
    assert_eq!(params[0].mode, radix_hir::HirParamMode::Owned);
    assert!(matches!(fixture.types.get(params[0].ty), Type::Array(_)));
}

#[test]
fn fn_type_params_grow_mode_slots() {
    let fixture = analyze_ok(
        r"
typus Handler = (de textus, own lista<numerus>, numerus) → vacuum
",
    );
    // The `Handler` alias resolves to a `Type::Func`; find the func signature
    // in the type table and assert its per-param semantic modes.
    let mut func_sig = None;
    for i in 0..fixture.types.type_count() {
        if let Type::Func(sig) = fixture.types.get(crate::TypeId(i as u32)) {
            func_sig = Some(sig.params.clone());
        }
    }
    let params = func_sig.expect("fn type present");
    assert_eq!(params.len(), 3);
    assert_eq!(params[0].mode, crate::SemanticParamMode::Ref);
    assert_eq!(params[1].mode, crate::SemanticParamMode::Owned);
    assert_eq!(params[2].mode, crate::SemanticParamMode::Owned);
}

#[test]
fn legacy_parameter_prefix_warns_without_rejecting() {
    let result = analyze_source(
        r#"
functio legacy(de lista<numerus> values) → vacuum {
    redde
}
"#,
    );
    assert!(
        result.is_ok(),
        "legacy prefix remains legal, got {:?}",
        result.diagnostics
    );
    assert!(
        result.diagnostics.iter().any(|err| {
            err.kind == crate::SemanticErrorKind::Warning(crate::WarningKind::DeprecatedParamPrefix)
                && issue(err) == Some("deprecated_param_prefix")
        }),
        "legacy prefix should emit WARN020, got {:?}",
        result.diagnostics
    );
}

#[test]
fn scalar_type_position_marker_stays_rejected() {
    // The scalar-marker rule is unchanged in MU-5a: a scalar cannot carry a
    // type-position access marker (fork remains operator-pending).
    let result = analyze_source(
        r"
incipit {
    fixum _ f ← (in numerus n) ∴ n
}
",
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|err| err.is_error() && issue(err) == Some(crate::ISSUE_SCALAR_ACCESS_MARKER)),
        "scalar access marker must stay rejected, got {:?}",
        result.diagnostics
    );
}
