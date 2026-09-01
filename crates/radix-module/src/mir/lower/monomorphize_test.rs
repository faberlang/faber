use super::*;

fn stub_function(id: u32) -> MirFunction {
    MirFunction {
        id: MirFunctionId(id),
        source: None,
        name: None,
        params: Vec::new(),
        locals: Vec::new(),
        temps: Vec::new(),
        blocks: Vec::new(),
        return_ty: MirType::semantic(TypeId(id)),
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    }
}

#[test]
fn function_by_id_hits_dense_index() {
    let functions = vec![stub_function(0), stub_function(1), stub_function(2)];
    let found = function_by_id(&functions, MirFunctionId(1)).expect("dense hit");
    assert_eq!(found.id, MirFunctionId(1));
    assert_eq!(found.return_ty.semantic_id(), TypeId(1));
}

#[test]
fn function_by_id_falls_back_when_ids_are_sparse() {
    // After template retain, vector index need not equal id.0.
    let functions = vec![stub_function(0), stub_function(5), stub_function(9)];
    let found = function_by_id(&functions, MirFunctionId(9)).expect("sparse fallback");
    assert_eq!(found.id, MirFunctionId(9));
    assert!(function_by_id(&functions, MirFunctionId(1)).is_none());
}

#[test]
fn function_by_id_mut_rewrites_dense_and_sparse() {
    let mut functions = vec![stub_function(0), stub_function(3)];
    {
        let f = function_by_id_mut(&mut functions, MirFunctionId(0)).expect("dense mut");
        f.name = Some(Symbol(1));
    }
    {
        let f = function_by_id_mut(&mut functions, MirFunctionId(3)).expect("sparse mut");
        f.name = Some(Symbol(2));
    }
    assert_eq!(functions[0].name, Some(Symbol(1)));
    assert_eq!(functions[1].name, Some(Symbol(2)));
    assert!(function_by_id_mut(&mut functions, MirFunctionId(2)).is_none());
}

// ── Phase-4 producer fact: instance shape-size substitution ──────────────

use crate::codegen::Target;
use crate::driver::{AnalyzeRequest, Config, Session};
use crate::semantic::IndexId;

fn analyzed_unit(source: &str) -> AnalyzedModule {
    let session = Session::new(Config::default().with_target(Target::HirTypeScript));
    crate::driver::analyze_source(&session, "test.fab", source, AnalyzeRequest::default()).expect("analysis succeeds")
}

fn generic_def_id(unit: &AnalyzedModule, name: &str) -> DefId {
    unit.hir
        .items
        .iter()
        .find_map(|item| match &item.kind {
            crate::hir::HirItemKind::Function(function) if unit.interner.resolve(function.name) == name => {
                Some(item.def_id)
            }
            _ => None,
        })
        .expect("generic function exists in the analyzed unit")
}

fn shape_size_operand(shape: IndexId, axis: Option<i64>) -> MirOperand {
    MirOperand::Constant(MirConstant::ShapeSize { shape, axis })
}

fn shape_size_instance(shape: IndexId, source: DefId) -> MirFunction {
    let mut instance = stub_function(0);
    instance.source = Some(source);
    instance.blocks = vec![MirBlock {
        id: MirBlockId(0),
        statements: vec![
            // Index-projection operand + assign-value operand.
            MirStatement {
                kind: MirStatementKind::Assign {
                    place: MirPlace {
                        base: MirPlaceBase::Local(MirLocalId(0)),
                        projections: vec![MirProjection::Index(shape_size_operand(shape, None))],
                    },
                    value: MirValue {
                        id: MirValueId(0),
                        kind: MirValueKind::Operand(shape_size_operand(shape, Some(1))),
                        ty: MirType::semantic(TypeId(0)),
                        span: Span::default(),
                    },
                },
                span: Span::default(),
            },
            // Runtime-call arg.
            MirStatement {
                kind: MirStatementKind::RuntimeCall {
                    destination: None,
                    call: MirRuntimeCall {
                        intrinsic: crate::mir::MirIntrinsic::Panic,
                        args: vec![shape_size_operand(shape, None)],
                        return_ty: MirType::semantic(TypeId(0)),
                    },
                },
                span: Span::default(),
            },
        ],
        terminator: MirTerminator {
            kind: MirTerminatorKind::Return(Some(shape_size_operand(shape, None))),
            span: Span::default(),
        },
        span: Span::default(),
    }];
    instance
}

#[test]
fn finalize_rewrites_instance_shape_size_from_shape_args() {
    let mut unit = analyzed_unit(
        r#"
functio add_tensors<magnitudo B, magnitudo D>(tensor<f32, [B, D]> a, tensor<f32, [B, D]> b) → tensor<f32, [B, D]> {
    redde a.addita(b)
}
"#,
    );
    let source = generic_def_id(&unit, "add_tensors");
    let symbol_b = unit.interner.intern("B");
    let symbol_d = unit.interner.intern("D");
    let param_b = unit.types.intern_index(IndexExpr::Param(symbol_b));
    let param_d = unit.types.intern_index(IndexExpr::Param(symbol_d));
    let symbolic_shape = unit
        .types
        .intern_index(IndexExpr::Tuple(vec![param_b, param_d]));
    let lit2 = unit.types.intern_index(IndexExpr::Literal(2));
    let lit8 = unit.types.intern_index(IndexExpr::Literal(8));

    let mut functions = vec![shape_size_instance(symbolic_shape, source)];
    let monomorphization = MirMonomorphization {
        key: MirMonomorphizationKey { source, type_args: Vec::new(), shape_args: vec![lit2, lit8] },
        function: MirFunctionId(0),
    };
    let errors = finalize_monomorphized_shapes(&mut unit, &mut functions, &[monomorphization]);
    assert!(errors.is_empty(), "concrete shape args must finalize cleanly: {errors:?}");

    // Every shape-size scalar rewrote to the concrete Float representation
    // from this instance's shape args (B → 2, D → 8).
    let mut floats = Vec::new();
    let _ = crate::mir::generic::rewrite_function_constants(&mut functions[0], &mut |constant| {
        match constant {
            MirConstant::Float(value) => floats.push(*value),
            other => panic!("expected only Float constants after finalize, got {other:?}"),
        }
        true
    });
    let mut floats = floats;
    floats.sort_by(f64::total_cmp);
    // product 2*8=16 (index projection), axis 1 → D=8 (assign),
    // product 16 (runtime-call arg), product 16 (return).
    assert_eq!(floats, vec![8.0, 16.0, 16.0, 16.0]);
}

#[test]
fn finalize_fails_closed_on_out_of_range_shape_size_axis() {
    let mut unit = analyzed_unit(
        r#"
functio add_tensors<magnitudo B, magnitudo D>(tensor<f32, [B, D]> a, tensor<f32, [B, D]> b) → tensor<f32, [B, D]> {
    redde a.addita(b)
}
"#,
    );
    let source = generic_def_id(&unit, "add_tensors");
    let symbol_b = unit.interner.intern("B");
    let symbol_d = unit.interner.intern("D");
    let param_b = unit.types.intern_index(IndexExpr::Param(symbol_b));
    let param_d = unit.types.intern_index(IndexExpr::Param(symbol_d));
    let symbolic_shape = unit
        .types
        .intern_index(IndexExpr::Tuple(vec![param_b, param_d]));
    let lit2 = unit.types.intern_index(IndexExpr::Literal(2));
    let lit8 = unit.types.intern_index(IndexExpr::Literal(8));

    let mut instance = shape_size_instance(symbolic_shape, source);
    // Corrupt one operand into an out-of-range axis on a rank-2 shape.
    let MirStatementKind::Assign { value, .. } = &mut instance.blocks[0].statements[0].kind else {
        panic!("first statement is the assign");
    };
    let MirValueKind::Operand(MirOperand::Constant(constant)) = &mut value.kind else {
        panic!("assign value is the axis operand");
    };
    *constant = MirConstant::ShapeSize { shape: symbolic_shape, axis: Some(7) };

    let mut functions = vec![instance];
    let monomorphization = MirMonomorphization {
        key: MirMonomorphizationKey { source, type_args: Vec::new(), shape_args: vec![lit2, lit8] },
        function: MirFunctionId(0),
    };
    let errors = finalize_monomorphized_shapes(&mut unit, &mut functions, &[monomorphization]);
    assert_eq!(errors.len(), 1, "one stable survivor diagnostic: {errors:?}");
    assert_eq!(
        errors[0].issue, "invalid_mir_unresolved_shape_size_constant_survives_monomorphization",
        "unexpected diagnostic: {errors:?}"
    );
}

#[test]
fn finalize_fails_closed_on_missing_and_infer_shape_args() {
    let mut unit = analyzed_unit(
        r#"
functio add_tensors<magnitudo B, magnitudo D>(tensor<f32, [B, D]> a, tensor<f32, [B, D]> b) → tensor<f32, [B, D]> {
    redde a.addita(b)
}
"#,
    );
    let source = generic_def_id(&unit, "add_tensors");
    let symbol_b = unit.interner.intern("B");
    let symbol_d = unit.interner.intern("D");
    let param_b = unit.types.intern_index(IndexExpr::Param(symbol_b));
    let param_d = unit.types.intern_index(IndexExpr::Param(symbol_d));
    let symbolic_shape = unit
        .types
        .intern_index(IndexExpr::Tuple(vec![param_b, param_d]));
    let lit2 = unit.types.intern_index(IndexExpr::Literal(2));
    let infer_shape = unit
        .types
        .intern_index(IndexExpr::Infer(radix_types::IndexVar(0)));

    // Missing binding for `D`: the key carries only one of two shape args.
    let mut functions = vec![shape_size_instance(symbolic_shape, source)];
    let errors = finalize_monomorphized_shapes(
        &mut unit,
        &mut functions,
        &[MirMonomorphization {
            key: MirMonomorphizationKey { source, type_args: Vec::new(), shape_args: vec![lit2] },
            function: MirFunctionId(0),
        }],
    );
    assert_eq!(errors.len(), 1, "missing shape binding fails closed: {errors:?}");
    assert_eq!(
        errors[0].issue,
        "invalid_mir_unresolved_shape_size_constant_survives_monomorphization"
    );

    // Infer shape arg: the instance binds `B` to an inference hole, which can
    // never fold to a concrete dimension.
    let mut functions = vec![shape_size_instance(symbolic_shape, source)];
    let errors = finalize_monomorphized_shapes(
        &mut unit,
        &mut functions,
        &[MirMonomorphization {
            key: MirMonomorphizationKey { source, type_args: Vec::new(), shape_args: vec![infer_shape, lit2] },
            function: MirFunctionId(0),
        }],
    );
    assert_eq!(errors.len(), 1, "infer shape arg fails closed: {errors:?}");
    assert_eq!(
        errors[0].issue,
        "invalid_mir_unresolved_shape_size_constant_survives_monomorphization"
    );
}

// ── MMG-D1: package-link survivor population ──────────────────────────────

/// Exercise the same fail-closed survivor net used by package linking. The
/// package-link inputs themselves live in the Gradus consumers, so their
/// verbatim package reds are recorded in `docs/factory/mir-monomorphization-gap/diagnosis.md`.
fn assert_unbound_shape_size_survivor(surface: &str, source_text: &str, function_name: &str, shape_names: &[&str]) {
    let mut unit = analyzed_unit(source_text);
    let source = generic_def_id(&unit, function_name);
    let shape_dims = shape_names
        .iter()
        .map(|name| {
            unit.types
                .intern_index(IndexExpr::Param(unit.interner.intern(name)))
        })
        .collect::<Vec<_>>();
    let symbolic_shape = unit.types.intern_index(IndexExpr::Tuple(shape_dims));
    let mut functions = vec![shape_size_instance(symbolic_shape, source)];
    let errors = finalize_monomorphized_shapes(
        &mut unit,
        &mut functions,
        &[MirMonomorphization {
            key: MirMonomorphizationKey {
                source,
                type_args: Vec::new(),
                // The package-link red is the empty per-instance map case.
                shape_args: Vec::new(),
            },
            function: MirFunctionId(0),
        }],
    );

    assert_eq!(errors.len(), 1, "{surface} must produce one survivor error: {errors:?}");
    assert_eq!(
        errors[0].issue, "invalid_mir_unresolved_shape_size_constant_survives_monomorphization",
        "{surface} survivor issue changed: {errors:?}"
    );
    assert!(
        crate::mir::generic::function_contains_shape_size(&mut functions[0]),
        "{surface} must retain the unresolved ShapeSize terms for the survivor net"
    );
}

#[test]
fn dense_ref01_survivor_population_is_fail_closed() {
    assert_unbound_shape_size_survivor(
        "dense REF-01",
        r#"
functio pin<magnitudo A, magnitudo B>(tensor<f32, [A, B]> t) → tensor<f32, [A, B]> {
    redde t
}
"#,
        "pin",
        &["A", "B"],
    );
}

#[test]
fn gather_twin_survivor_population_is_fail_closed() {
    assert_unbound_shape_size_survivor(
        "gather twin",
        r#"
functio gather<magnitudo T, magnitudo V, magnitudo D>(tensor<f32, [T, D]> table) → tensor<f32, [T, D]> {
    redde table
}
"#,
        "gather",
        &["T", "D"],
    );
}

#[test]
fn krs_oracle_survivor_population_is_fail_closed() {
    assert_unbound_shape_size_survivor(
        "KRS oracle",
        r#"
functio dense_qkv<magnitudo T, magnitudo D, magnitudo Q, magnitudo K>(tensor<f32, [T, Q]> q) → tensor<f32, [T, Q]> {
    redde q
}
"#,
        "dense_qkv",
        &["T", "Q"],
    );
}
