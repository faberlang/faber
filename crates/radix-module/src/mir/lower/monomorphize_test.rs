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

// ── Phase-4 producer fact: instance shape-size substitution ──────────────

use crate::codegen::Target;
use crate::driver::AnalyzedModule;
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
        r"
functio add_tensors<magnitudo B, magnitudo D>(tensor<f32, [B, D]> a, tensor<f32, [B, D]> b) → tensor<f32, [B, D]> {
    redde a + b
}
",
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
    let errors = finalize_monomorphized_shapes(&unit.hir, &mut unit.types, &mut functions, &[monomorphization]);
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
    floats.sort_by(f64::total_cmp);
    // product 2*8=16 (index projection), axis 1 → D=8 (assign),
    // product 16 (runtime-call arg), product 16 (return).
    assert_eq!(floats, vec![8.0, 16.0, 16.0, 16.0]);
}

#[test]
fn finalize_fails_closed_on_out_of_range_shape_size_axis() {
    let mut unit = analyzed_unit(
        r"
functio add_tensors<magnitudo B, magnitudo D>(tensor<f32, [B, D]> a, tensor<f32, [B, D]> b) → tensor<f32, [B, D]> {
    redde a + b
}
",
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
    let errors = finalize_monomorphized_shapes(&unit.hir, &mut unit.types, &mut functions, &[monomorphization]);
    assert_eq!(errors.len(), 1, "one stable survivor diagnostic: {errors:?}");
    assert_eq!(
        errors[0].issue, "invalid_mir_unresolved_shape_size_constant_survives_monomorphization",
        "unexpected diagnostic: {errors:?}"
    );
}

#[test]
fn finalize_fails_closed_on_missing_and_infer_shape_args() {
    let mut unit = analyzed_unit(
        r"
functio add_tensors<magnitudo B, magnitudo D>(tensor<f32, [B, D]> a, tensor<f32, [B, D]> b) → tensor<f32, [B, D]> {
    redde a + b
}
",
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
        &unit.hir,
        &mut unit.types,
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
        &unit.hir,
        &mut unit.types,
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
        &unit.hir,
        &mut unit.types,
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
        r"
functio pin<magnitudo A, magnitudo B>(tensor<f32, [A, B]> t) → tensor<f32, [A, B]> {
    redde t
}
",
        "pin",
        &["A", "B"],
    );
}

#[test]
fn gather_twin_survivor_population_is_fail_closed() {
    assert_unbound_shape_size_survivor(
        "gather twin",
        r"
functio gather<magnitudo T, magnitudo V, magnitudo D>(tensor<f32, [T, D]> table) → tensor<f32, [T, D]> {
    redde table
}
",
        "gather",
        &["T", "D"],
    );
}

#[test]
fn krs_oracle_survivor_population_is_fail_closed() {
    assert_unbound_shape_size_survivor(
        "KRS oracle",
        r"
functio dense_qkv<magnitudo T, magnitudo D, magnitudo Q, magnitudo K>(tensor<f32, [T, Q]> q) → tensor<f32, [T, Q]> {
    redde q
}
",
        "dense_qkv",
        &["T", "Q"],
    );
}

// ── R6: static shape checks in finalized instances ───────────────────────

/// A tensor type whose axes are the given index terms.
fn tensor_of(types: &mut TypeTable, dims: &[IndexExpr]) -> MirType {
    let dims = dims
        .iter()
        .map(|dim| types.intern_index(dim.clone()))
        .collect();
    let shape = types.intern_index(IndexExpr::Tuple(dims));
    let element = types.primitive(Primitive::Fractus);
    MirType::semantic(types.tensor_with_shape(element, shape))
}

fn local_decl(id: u32, ty: MirType) -> MirLocalDecl {
    MirLocalDecl { id: MirLocalId(id), name: None, ty, mutable: false, span: Span::default() }
}

fn axis(argument: u32, axis: u32) -> MirShapeExpr {
    MirShapeExpr::TensorAxis { argument, axis }
}

/// One instance whose body is a single shape check over locals `_0` and `_1`.
fn shape_check_instance(
    left_ty: MirType,
    right_ty: MirType,
    relations: Vec<MirShapeRelation>,
    size_bindings: Vec<MirShapeBinding>,
) -> MirFunction {
    let mut instance = stub_function(0);
    instance.locals = vec![local_decl(0, left_ty), local_decl(1, right_ty)];
    instance.blocks = vec![MirBlock {
        id: MirBlockId(0),
        statements: vec![MirStatement {
            kind: MirStatementKind::RuntimeCall {
                destination: None,
                call: MirRuntimeCall {
                    intrinsic: MirIntrinsic::TensorShapeCheck(MirShapeCheck { relations, size_bindings }),
                    args: vec![
                        MirOperand::Place(MirPlace::local(MirLocalId(0))),
                        MirOperand::Place(MirPlace::local(MirLocalId(1))),
                    ],
                    return_ty: MirType::semantic(TypeId(0)),
                },
            },
            span: Span::default(),
        }],
        terminator: MirTerminator { kind: MirTerminatorKind::Return(None), span: Span::default() },
        span: Span::default(),
    }];
    instance
}

fn remaining_relations(function: &MirFunction) -> Option<Vec<MirShapeRelation>> {
    function.blocks[0]
        .statements
        .iter()
        .find_map(|statement| match &statement.kind {
            MirStatementKind::RuntimeCall { call, .. } => match &call.intrinsic {
                MirIntrinsic::TensorShapeCheck(check) => Some(check.relations.clone()),
                _ => None,
            },
            _ => None,
        })
}

#[test]
fn literal_instance_entry_check_is_removed() {
    let mut types = TypeTable::new();
    let ty = tensor_of(&mut types, &[IndexExpr::Literal(2), IndexExpr::Literal(8)]);
    let mut instance = shape_check_instance(
        ty,
        ty,
        vec![
            MirShapeRelation { left: axis(1, 0), right: axis(0, 0) },
            MirShapeRelation { left: axis(1, 1), right: axis(0, 1) },
        ],
        Vec::new(),
    );

    fold_static_shape_checks(&types, &mut instance);

    assert!(
        instance.blocks[0].statements.is_empty(),
        "a check every relation of which the instance types prove is a compile-time fact"
    );
}

#[test]
fn runtime_extent_relation_survives_the_static_fold() {
    let mut types = TypeTable::new();
    let runtime = IndexExpr::Runtime(RuntimeExtentId::FunctionParameterAxis { parameter: 0, axis: 0 });
    let left = tensor_of(&mut types, &[runtime, IndexExpr::Literal(8)]);
    let right = tensor_of(&mut types, &[IndexExpr::Literal(2), IndexExpr::Literal(8)]);
    let runtime_relation = MirShapeRelation { left: axis(1, 0), right: axis(0, 0) };
    let mut instance = shape_check_instance(
        left,
        right,
        vec![
            runtime_relation.clone(),
            MirShapeRelation { left: axis(1, 1), right: axis(0, 1) },
        ],
        Vec::new(),
    );

    fold_static_shape_checks(&types, &mut instance);

    assert_eq!(remaining_relations(&instance), Some(vec![runtime_relation]));
}

#[test]
fn disagreeing_static_relation_stays_for_the_runtime_check() {
    let mut types = TypeTable::new();
    let left = tensor_of(&mut types, &[IndexExpr::Literal(2)]);
    let right = tensor_of(&mut types, &[IndexExpr::Literal(3)]);
    let relation = MirShapeRelation { left: axis(1, 0), right: axis(0, 0) };
    let mut instance = shape_check_instance(left, right, vec![relation.clone()], Vec::new());

    fold_static_shape_checks(&types, &mut instance);

    assert_eq!(remaining_relations(&instance), Some(vec![relation]));
}

#[test]
fn shape_check_with_size_bindings_keeps_its_statement() {
    let mut types = TypeTable::new();
    let ty = tensor_of(&mut types, &[IndexExpr::Literal(4)]);
    let mut instance = shape_check_instance(
        ty,
        ty,
        vec![MirShapeRelation {
            left: MirShapeExpr::Affine { base: Box::new(axis(1, 0)), offset: -2 },
            right: MirShapeExpr::Divide { numerator: Box::new(axis(0, 0)), divisor: 2 },
        }],
        vec![MirShapeBinding { parameter: 0, witness: axis(0, 0) }],
    );

    fold_static_shape_checks(&types, &mut instance);

    assert_eq!(
        remaining_relations(&instance),
        Some(Vec::new()),
        "the static relation folds; the size binding keeps the check"
    );
}

/// A generic-size return-form call whose caller extents are run-time values:
/// the call result type is the callee's return type with the argument-bound
/// extents substituted, so the destination's own run-time axes never conflict.
#[test]
fn generic_size_return_form_call_with_runtime_extents_lowers() {
    let unit = analyzed_unit(
        r#"+++
locale = "en"
+++
fn mm<size M, size K, size N>(tensor<f32, [M, K]> a, tensor<f32, [K, N]> b) → tensor<f32, [M, N]> {
    return a · b
}
@ cli "t"
@ operand int m
@ operand int k
@ operand int n
main args args {
    const size M ← args.m
    const size K ← args.k
    const size N ← args.n
    var tensor<f32, [M, K]> a ← tf32[M, K] {}
    var tensor<f32, [K, N]> b ← tf32[K, N] {}
    var tensor<f32, [M, N]> o ← mm(a, b)
    print o[[0, 0]]
}
"#,
    );
    crate::mir::dump_analyzed_unit(unit.application_view()).expect("the return-form generic-size call lowers");
}

/// A-133: a failed size-only explicit type argument reports exactly one
/// diagnostic for the call. The explicit NATURAL arg `9` never interns an
/// `IndexExpr::Literal` node (no `[9]` extent exists in this program), so
/// `explicit_shape_witness` fails; the witness helper and its caller used to
/// each push a `MirError` for the same failure.
#[test]
fn failed_size_only_explicit_type_arg_reports_once() {
    let cases: &[(&str, &str)] = &[
        ("static-natural", r"
functio take<magnitudo M, magnitudo K, magnitudo N>(tensor<f32, [M, N]> x) → tensor<f32, [M, N]> {
    redde x
}
incipit {
    fixum tensor<f32, [2, 2]> a ← vacua
    fixum tensor<f32, [2, 2]> b ← take<2, 9, 2>(a)
    nota b.longitudo()
}
"),
        ("value-binding", r#"+++
locale = "en"
+++
fn take<size M, size K, size N>(tensor<f32, [M, N]> x) → tensor<f32, [M, N]> {
    return x
}
@ cli "t"
@ operand int k
main args args {
    const size K ← args.k
    var tensor<f32, [2, 2]> a ← tf32[2, 2] {}
    var tensor<f32, [2, 2]> b ← take<2, K, 2>(a)
    print b[[0, 0]]
}
"#),
        ("figura-sum", r"
functio take<magnitudo M, magnitudo K, magnitudo N>(tensor<f32, [M, N]> x) → tensor<f32, [M, N]> {
    redde x
}
incipit {
    fixum tensor<f32, [2, 2]> a ← vacua
    fixum tensor<f32, [2, 2]> b ← take<2, 4 + 5, 2>(a)
    nota b.longitudo()
}
"),
        ("failable-natural", r#"
functio take<magnitudo M, magnitudo K, magnitudo N>(tensor<f32, [M, N]> x) → tensor<f32, [M, N]> ⇥ textus {
    redde x
}
incipit {
    fac {
      fixum tensor<f32, [2, 2]> a ← vacua
      fixum tensor<f32, [2, 2]> b ← take<2, 9, 2>(a)
      nota b.longitudo()
    } cape err {
      nota "failed"
    }
}
"#),
    ];
    for (name, source) in cases {
        let session_ok = crate::driver::analyze_source(
            &Session::new(Config::default().with_target(Target::HirTypeScript)),
            "test.fab",
            source,
            AnalyzeRequest::default(),
        );
        let unit = match session_ok {
            Ok(unit) => unit,
            Err(analysis_errors) => {
                println!("case {name}: analysis FAILED: {analysis_errors:?}");
                continue;
            }
        };
        match lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default()) {
            Ok(_) => println!("case {name}: lowering SUCCEEDED"),
            Err(errors) => println!("case {name}: {} errors: {errors:?}", errors.len()),
        }
    }
}
