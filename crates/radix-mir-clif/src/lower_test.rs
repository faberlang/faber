//! Hand-built MIR fixtures for native lowering: supported runtime boundaries
//! name their imports in the emitted object, and unsupported representations
//! decline before an executable or in-memory image can be produced.

use crate::{StructMeta, emit_program};
use radix_lexer::{Interner, Span};
use radix_mir::*;
use radix_types::{Primitive, Type, TypeTable};

fn sample_span() -> Span {
    Span::default()
}

fn ty(types: &TypeTable, primitive: Primitive) -> MirType {
    MirType::semantic(types.primitive(primitive))
}

struct FunctionShape {
    params: Vec<MirParam>,
    locals: Vec<MirLocal>,
    temps: Vec<MirTemp>,
    statements: Vec<MirStatement>,
    terminator: MirTerminatorKind,
    return_ty: MirType,
    error_ty: Option<MirType>,
    is_async: bool,
    is_generator: bool,
}

#[allow(clippy::too_many_lines)]
fn single_function(shape: FunctionShape) -> MirProgram {
    MirProgram {
        functions: vec![MirFunction {
            id: MirFunctionId(0),
            source: None,
            name: None,
            params: shape.params,
            locals: shape.locals,
            temps: shape.temps,
            blocks: vec![MirBlock {
                id: MirBlockId(0),
                statements: shape.statements,
                terminator: MirTerminator {
                    kind: shape.terminator,
                    span: sample_span(),
                },
                span: sample_span(),
            }],
            return_ty: shape.return_ty,
            error_ty: shape.error_ty,
            is_async: shape.is_async,
            is_generator: shape.is_generator,
            shader_stage: None,
            span: sample_span(),
        }],
    }
}

fn local(id: u32, ty: MirType) -> MirLocal {
    MirLocal {
        id: MirLocalId(id),
        name: None,
        ty,
        mutable: false,
        span: sample_span(),
    }
}

/// `sermo` is a builtin frame struct: one builtin struct identity stands in
/// for the conversation handle.
fn sermo_handle(types: &mut TypeTable) -> MirType {
    MirType::semantic(types.intern(Type::Struct(radix_hir::DefId(1))))
}

fn runtime_call(
    intrinsic: MirIntrinsic,
    args: Vec<MirOperand>,
    destination: Option<MirPlace>,
    return_ty: MirType,
) -> MirStatement {
    MirStatement {
        kind: MirStatementKind::RuntimeCall {
            destination,
            call: MirRuntimeCall {
                intrinsic,
                args,
                return_ty,
            },
        },
        span: sample_span(),
    }
}

#[test]
fn sermo_lower_open_and_set_opener_emit_runtime_calls() {
    let mut types = TypeTable::new();
    let handle = sermo_handle(&mut types);
    let numerus = ty(&types, Primitive::Numerus);
    let vacuum = ty(&types, Primitive::Vacuum);
    let mut interner = Interner::new();
    let route = interner.intern("tempus:monotonicum");
    let program = single_function(FunctionShape {
        params: Vec::new(),
        locals: vec![local(0, handle), local(1, numerus)],
        temps: Vec::new(),
        statements: vec![
            runtime_call(
                MirIntrinsic::SermoOpen,
                vec![MirOperand::Constant(MirConstant::String(route))],
                Some(MirPlace::local(MirLocalId(0))),
                handle,
            ),
            runtime_call(
                MirIntrinsic::SermoSetOpener,
                vec![
                    MirOperand::Place(MirPlace::local(MirLocalId(0))),
                    MirOperand::Constant(MirConstant::Int(0)),
                ],
                None,
                vacuum,
            ),
        ],
        terminator: MirTerminatorKind::Return(None),
        return_ty: vacuum,
        error_ty: None,
        is_async: false,
        is_generator: false,
    });

    let object = emit_program(
        &program,
        &types,
        &interner,
        Some(MirFunctionId(0)),
        ["true", "false", "none", "tuple"].map(str::to_owned),
        &StructMeta::default(),
    )
    .expect("the sync scalar conversation lowers");

    for name in [
        "__faber_clif_v1_sermo_open",
        "__faber_clif_v1_sermo_set_opener_i64",
    ] {
        let named = object
            .bytes
            .windows(name.len())
            .any(|window| window == name.as_bytes());
        assert!(named, "the object names {name}");
    }
}

/// The 8.1 program plus a materializing conversion out of the handle.
fn open_then_convert(target: Primitive) -> (MirProgram, TypeTable, Interner) {
    let mut types = TypeTable::new();
    let handle = sermo_handle(&mut types);
    let vacuum = ty(&types, Primitive::Vacuum);
    let target_ty = ty(&types, target);
    let mut interner = Interner::new();
    let route = interner.intern("tempus:monotonicum");
    let program = single_function(FunctionShape {
        params: Vec::new(),
        locals: vec![local(0, handle), local(1, target_ty)],
        temps: Vec::new(),
        statements: vec![
            runtime_call(
                MirIntrinsic::SermoOpen,
                vec![MirOperand::Constant(MirConstant::String(route))],
                Some(MirPlace::local(MirLocalId(0))),
                handle,
            ),
            runtime_call(
                MirIntrinsic::SermoSetOpener,
                vec![
                    MirOperand::Place(MirPlace::local(MirLocalId(0))),
                    MirOperand::Constant(MirConstant::Int(0)),
                ],
                None,
                vacuum,
            ),
            runtime_call(
                MirIntrinsic::Convert(MirConversion {
                    store: None,
                    flavor: MirConversionFlavor::Runtime,
                    source_ty: handle,
                    target_ty,
                    params: Vec::new(),
                    recovery: Some(MirOperand::Constant(MirConstant::Int(0))),
                    struct_defaults: Vec::new(),
                }),
                vec![MirOperand::Place(MirPlace::local(MirLocalId(0)))],
                Some(MirPlace::local(MirLocalId(1))),
                target_ty,
            ),
        ],
        terminator: MirTerminatorKind::Return(None),
        return_ty: vacuum,
        error_ty: None,
        is_async: false,
        is_generator: false,
    });
    (program, types, interner)
}

#[test]
fn sermo_lower_valor_opener_emits_the_tagged_set_opener() {
    let mut types = TypeTable::new();
    let handle = sermo_handle(&mut types);
    let numerus = ty(&types, Primitive::Numerus);
    let valor = ty(&types, Primitive::Valor);
    let vacuum = ty(&types, Primitive::Vacuum);
    let mut interner = Interner::new();
    let route = interner.intern("tempus:dormiet");
    let program = single_function(FunctionShape {
        params: Vec::new(),
        locals: vec![local(0, handle)],
        temps: vec![MirTemp {
            id: MirTempId(0),
            ty: valor,
            span: sample_span(),
        }],
        statements: vec![
            runtime_call(
                MirIntrinsic::SermoOpen,
                vec![MirOperand::Constant(MirConstant::String(route))],
                Some(MirPlace::local(MirLocalId(0))),
                handle,
            ),
            runtime_call(
                MirIntrinsic::Convert(MirConversion {
                    store: None,
                    flavor: MirConversionFlavor::Runtime,
                    source_ty: numerus,
                    target_ty: valor,
                    params: Vec::new(),
                    recovery: None,
                    struct_defaults: Vec::new(),
                }),
                vec![MirOperand::Constant(MirConstant::Int(7))],
                Some(MirPlace::temp(MirTempId(0))),
                valor,
            ),
            runtime_call(
                MirIntrinsic::SermoSetOpener,
                vec![
                    MirOperand::Place(MirPlace::local(MirLocalId(0))),
                    MirOperand::Temp(MirTempId(0)),
                ],
                None,
                vacuum,
            ),
        ],
        terminator: MirTerminatorKind::Return(None),
        return_ty: vacuum,
        error_ty: None,
        is_async: false,
        is_generator: false,
    });
    let object = emit_program(
        &program,
        &types,
        &interner,
        Some(MirFunctionId(0)),
        ["true", "false", "none", "tuple"].map(str::to_owned),
        &StructMeta::default(),
    )
    .expect("the boxed valor opener lowers");

    let valor_name = b"__faber_clif_v1_sermo_set_opener_valor";
    let named = object
        .bytes
        .windows(valor_name.len())
        .any(|window| window == valor_name);
    assert!(named, "the object names the tagged set-opener call");
    let i64_name = b"__faber_clif_v1_sermo_set_opener_i64";
    let untagged = object
        .bytes
        .windows(i64_name.len())
        .any(|window| window == i64_name);
    assert!(!untagged, "the object names no untagged set-opener call");
}

#[test]
fn sermo_lower_identity_valor_opener_boxes_the_scalar() {
    // A leaf opener is typed `valor` in HIR while the operand keeps its
    // own scalar representation, so the conversion is an identity at the
    // type table. The lowering boxes what the operand carries instead of
    // copying a box that is not there.
    let mut types = TypeTable::new();
    let handle = sermo_handle(&mut types);
    let numerus = ty(&types, Primitive::Numerus);
    let valor = ty(&types, Primitive::Valor);
    let vacuum = ty(&types, Primitive::Vacuum);
    let mut interner = Interner::new();
    let route = interner.intern("tempus:dormiet");
    let program = single_function(FunctionShape {
        params: Vec::new(),
        locals: vec![local(0, handle), local(1, numerus)],
        temps: vec![MirTemp {
            id: MirTempId(0),
            ty: valor,
            span: sample_span(),
        }],
        statements: vec![
            runtime_call(
                MirIntrinsic::SermoOpen,
                vec![MirOperand::Constant(MirConstant::String(route))],
                Some(MirPlace::local(MirLocalId(0))),
                handle,
            ),
            runtime_call(
                MirIntrinsic::Convert(MirConversion {
                    store: None,
                    flavor: MirConversionFlavor::Runtime,
                    source_ty: valor,
                    target_ty: valor,
                    params: Vec::new(),
                    recovery: None,
                    struct_defaults: Vec::new(),
                }),
                vec![MirOperand::Place(MirPlace::local(MirLocalId(1)))],
                Some(MirPlace::temp(MirTempId(0))),
                valor,
            ),
            runtime_call(
                MirIntrinsic::SermoSetOpener,
                vec![
                    MirOperand::Place(MirPlace::local(MirLocalId(0))),
                    MirOperand::Temp(MirTempId(0)),
                ],
                None,
                vacuum,
            ),
        ],
        terminator: MirTerminatorKind::Return(None),
        return_ty: vacuum,
        error_ty: None,
        is_async: false,
        is_generator: false,
    });
    let object = emit_program(
        &program,
        &types,
        &interner,
        Some(MirFunctionId(0)),
        ["true", "false", "none", "tuple"].map(str::to_owned),
        &StructMeta::default(),
    )
    .expect("the identity valor opener boxes and lowers");

    for name in [
        "__faber_clif_v1_dyn_box",
        "__faber_clif_v1_sermo_set_opener_valor",
    ] {
        let named = object
            .bytes
            .windows(name.len())
            .any(|window| window == name.as_bytes());
        assert!(named, "the object names {name}");
    }
}

#[test]
fn sermo_lower_scalar_materialization_emits_the_leaf_call() {
    let (program, types, interner) = open_then_convert(Primitive::Numerus);
    let object = emit_program(
        &program,
        &types,
        &interner,
        Some(MirFunctionId(0)),
        ["true", "false", "none", "tuple"].map(str::to_owned),
        &StructMeta::default(),
    )
    .expect("the sync scalar materialization lowers");

    let name = b"__faber_clif_v1_sermo_materialize_i64";
    let named = object
        .bytes
        .windows(name.len())
        .any(|window| window == name);
    assert!(named, "the object names the materialize call");
}

#[test]
fn sermo_lower_unit_materialization_emits_the_drain_call() {
    let (program, types, interner) = open_then_convert(Primitive::Vacuum);
    let object = emit_program(
        &program,
        &types,
        &interner,
        Some(MirFunctionId(0)),
        ["true", "false", "none", "tuple"].map(str::to_owned),
        &StructMeta::default(),
    )
    .expect("the sync unit materialization lowers");

    let name = b"__faber_clif_v1_sermo_drain";
    let named = object
        .bytes
        .windows(name.len())
        .any(|window| window == name);
    assert!(named, "the object names the drain call");
}

#[test]
fn sermo_lower_void_materialization_needs_no_destination() {
    // The unit conversation is consumed for effects, so the drain
    // statement carries no destination in the MIR the package lowering
    // produces.
    let mut types = TypeTable::new();
    let handle = sermo_handle(&mut types);
    let vacuum = ty(&types, Primitive::Vacuum);
    let mut interner = Interner::new();
    let route = interner.intern("tempus:dormiet");
    let program = single_function(FunctionShape {
        params: Vec::new(),
        locals: vec![local(0, handle)],
        temps: Vec::new(),
        statements: vec![
            runtime_call(
                MirIntrinsic::SermoOpen,
                vec![MirOperand::Constant(MirConstant::String(route))],
                Some(MirPlace::local(MirLocalId(0))),
                handle,
            ),
            runtime_call(
                MirIntrinsic::Convert(MirConversion {
                    store: None,
                    flavor: MirConversionFlavor::Runtime,
                    source_ty: handle,
                    target_ty: vacuum,
                    params: Vec::new(),
                    recovery: None,
                    struct_defaults: Vec::new(),
                }),
                vec![MirOperand::Place(MirPlace::local(MirLocalId(0)))],
                None,
                vacuum,
            ),
        ],
        terminator: MirTerminatorKind::Return(None),
        return_ty: vacuum,
        error_ty: None,
        is_async: false,
        is_generator: false,
    });
    let object = emit_program(
        &program,
        &types,
        &interner,
        Some(MirFunctionId(0)),
        ["true", "false", "none", "tuple"].map(str::to_owned),
        &StructMeta::default(),
    )
    .expect("a destination-less drain lowers");

    let name = b"__faber_clif_v1_sermo_drain";
    let named = object
        .bytes
        .windows(name.len())
        .any(|window| window == name);
    assert!(named, "the object names the drain call");
}

#[test]
fn sermo_lower_instant_materialization_emits_from_epoch() {
    let (program, types, interner) = open_then_convert(Primitive::Instans);
    let object = emit_program(
        &program,
        &types,
        &interner,
        Some(MirFunctionId(0)),
        ["true", "false", "none", "tuple"].map(str::to_owned),
        &StructMeta::default(),
    )
    .expect("the sync instant materialization lowers");

    for name in [
        "__faber_clif_v1_sermo_materialize_i64",
        "__faber_clif_v1_ins_from_epoch",
    ] {
        let named = object
            .bytes
            .windows(name.len())
            .any(|window| window == name.as_bytes());
        assert!(named, "the object names {name}");
    }
}

#[test]
fn format_string_newline_escape_emits_a_real_newline() {
    let types = TypeTable::new();
    let textus = ty(&types, Primitive::Textus);
    let mut interner = Interner::new();
    let template = interner.intern(r"x,§\ny");
    let slot = interner.intern("slot");
    let program = single_function(FunctionShape {
        params: Vec::new(),
        locals: vec![local(0, textus)],
        temps: Vec::new(),
        statements: vec![runtime_call(
            MirIntrinsic::FormatString { template },
            vec![MirOperand::Constant(MirConstant::String(slot))],
            Some(MirPlace::local(MirLocalId(0))),
            textus,
        )],
        terminator: MirTerminatorKind::Return(Some(MirOperand::Place(MirPlace::local(
            MirLocalId(0),
        )))),
        return_ty: textus,
        error_ty: None,
        is_async: false,
        is_generator: false,
    });
    let object = emit_program(
        &program,
        &types,
        &interner,
        Some(MirFunctionId(0)),
        ["true", "false", "none", "tuple"].map(str::to_owned),
        &StructMeta::default(),
    )
    .expect("format string lowers");

    assert!(
        object.bytes.windows(2).any(|window| window == b"\ny"),
        "the emitted literal contains a newline before y"
    );
    assert!(
        !object.bytes.windows(3).any(|window| window == b"\\ny"),
        "the emitted literal does not retain the backslash-n escape"
    );
}

#[test]
fn sermo_lower_text_materialization_still_declines() {
    let (program, types, interner) = open_then_convert(Primitive::Textus);
    let lowered = emit_program(
        &program,
        &types,
        &interner,
        Some(MirFunctionId(0)),
        ["true", "false", "none", "tuple"].map(str::to_owned),
        &StructMeta::default(),
    );

    assert!(lowered.is_err(), "textus materialization stays declined");
}

#[test]
fn registered_call_handler_refuses_before_opening_a_native_conversation() {
    let (program, types, interner) = open_then_convert(Primitive::Numerus);
    let mut structs = StructMeta::default();
    structs
        .handler_routes
        .insert("tempus:monotonicum".to_owned());
    let error = emit_program(
        &program,
        &types,
        &interner,
        Some(MirFunctionId(0)),
        ["true", "false", "none", "tuple"].map(str::to_owned),
        &structs,
    )
    .expect_err("a registered MIR handler cannot be served by the leaf runtime");
    assert_eq!(error.shape, "call handler conversation");
}

#[test]
fn optional_parameter_metadata_refuses_instead_of_unwrapping_none_in_the_callee() {
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let optional = MirType::semantic(types.option(numerus));
    let vacuum = ty(&types, Primitive::Vacuum);
    let param_ty = MirType::semantic(numerus);
    let mut program = single_function(FunctionShape {
        params: vec![MirParam {
            local: MirLocalId(0),
            name: None,
            ty: param_ty,
            mode: MirParamMode::Owned,
            span: sample_span(),
        }],
        locals: vec![local(0, param_ty)],
        temps: Vec::new(),
        statements: Vec::new(),
        terminator: MirTerminatorKind::Return(None),
        return_ty: vacuum,
        error_ty: None,
        is_async: false,
        is_generator: false,
    });
    let mut entry = program.functions[0].clone();
    entry.id = MirFunctionId(1);
    entry.params.clear();
    entry.locals.clear();
    entry.blocks[0].statements.push(MirStatement {
        kind: MirStatementKind::Call {
            destination: None,
            callee: MirCallee::Function(MirFunctionId(0)),
            args: vec![MirOperand::Constant(MirConstant::Nil)],
        },
        span: sample_span(),
    });
    program.functions.push(entry);
    let mut structs = StructMeta::default();
    structs
        .optional_params
        .insert((MirFunctionId(0), 0), optional);
    let error = emit_program(
        &program,
        &types,
        &Interner::new(),
        Some(MirFunctionId(1)),
        ["true", "false", "none", "tuple"].map(str::to_owned),
        &structs,
    )
    .expect_err("plain optional parameter needs omission/default lowering");
    assert_eq!(error.shape, "optional parameter omission");
}
