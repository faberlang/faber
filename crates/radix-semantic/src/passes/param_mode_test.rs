//! MU-5a type-position parameter-mode fixtures.
//!
//! Proves the mode-unification fact: HIR parameter modes derive from the
//! type-position access marker when present (named and closure params and
//! fn-type params), and the marked type carries the mode. Parameter prefixes
//! are retired at the parser boundary.

use crate::test_support::{analyze_ok_la, analyze_source_la, issue};
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
    let fixture = analyze_ok_la(
        r"
incipit {
    sit f ← (de lista<numerus> a, in lista<numerus> b, own lista<numerus> c, copy lista<numerus> d) ∴ a
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
    let fixture = analyze_ok_la(
        r"
incipit {
    sit f ← (lista<numerus> a, numerus n) ∴ n
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
    let fixture = analyze_ok_la(
        r"
typus Handler = (de textus, own lista<numerus>, numerus) → vacuum
",
    );
    // The `Handler` alias resolves to a `Type::Func`; find the func signature
    // in the type table and assert its per-param semantic modes.
    let mut func_sig = None;
    for i in 0..fixture.types.type_count() {
        if let Type::Func(sig) = fixture.types.get(crate::TypeId(
            u32::try_from(i).expect("type count fits u32"),
        )) {
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
fn parameter_prefix_is_a_parse_error() {
    let result = analyze_source_la(
        r"
functio legacy(ex lista<numerus> values) → vacuum {
    redde
}
",
    );
    assert!(
        !result.is_ok(),
        "parameter prefix must reject, got {:?}",
        result.diagnostics
    );
}

#[test]
fn scalar_ref_and_mut_parameter_markers_are_admitted() {
    let result = analyze_ok_la(
        r"
functio read(de numerus value) → numerus {
    redde value
}
functio bump(in numerus value) → vacuum {
    value ← value + 1
}
",
    );
    let functions = result
        .hir
        .as_ref()
        .expect("hir")
        .items
        .iter()
        .filter_map(|item| match &item.kind {
            radix_hir::HirItemKind::Function(func) => Some(func),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(functions.len(), 2);
    assert_eq!(functions[0].params[0].mode, radix_hir::HirParamMode::De);
    assert_eq!(functions[1].params[0].mode, radix_hir::HirParamMode::In);
    assert!(matches!(
        result.types.get(functions[0].params[0].ty),
        Type::Ref(SemanticMutability::Immutable, _)
    ));
    assert!(matches!(
        result.types.get(functions[1].params[0].ty),
        Type::Ref(SemanticMutability::Mutable, _)
    ));
}

#[test]
fn scalar_own_and_copy_parameter_markers_remain_rejected() {
    for marker in ["own", "copy"] {
        let result = analyze_source_la(&format!(
            "functio reject({marker} numerus value) → vacuum {{ redde }}"
        ));
        assert!(
            result
                .diagnostics
                .iter()
                .any(|err| err.is_error() && issue(err) == Some(crate::ISSUE_SCALAR_ACCESS_MARKER)),
            "scalar {marker} marker must reject, got {:?}",
            result.diagnostics
        );
    }
}

#[test]
fn scalar_function_type_mut_marker_is_admitted() {
    let fixture = analyze_ok_la(
        r"
typus Handler = (in numerus) → vacuum
",
    );
    let signature = (0..fixture.types.type_count()).find_map(|index| {
        match fixture.types.get(crate::TypeId(
            u32::try_from(index).expect("type count fits u32"),
        )) {
            Type::Func(sig) if sig.params.len() == 1 => Some(sig.clone()),
            _ => None,
        }
    });
    let signature = signature.expect("function type present");
    assert_eq!(signature.params[0].mode, crate::SemanticParamMode::MutRef);
    assert!(matches!(
        fixture.types.get(signature.params[0].ty),
        Type::Ref(SemanticMutability::Mutable, inner)
            if matches!(fixture.types.get(*inner), Type::Primitive(crate::Primitive::Numerus))
    ));
}
