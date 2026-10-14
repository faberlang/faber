//! Lowering pins for `octeti ↦ tensor<f32, [N]>` inside a generic function
//! whose `size N` is witnessed by a tensor argument axis.
//!
//! The conversion must carry a shape operand that reads the witness
//! (`proto.magnitudines()[0]`) instead of a symbolic `ShapeSize` placeholder,
//! so the MIR runner can check the byte length. A `size` parameter with no
//! witnessing tensor keeps the symbolic path and stays declined by name.

use super::*;

const SIZE_NAME: Symbol = Symbol(7);
const PROTO_NAME: Symbol = Symbol(8);

/// Types for `fn decode<size N>(tensor<f32, [N]> proto, bytes raw)` and the
/// recorded `proto` first-axis witness.
struct Fixture {
    types: TypeTable,
    octeti: TypeId,
    target: TypeId,
    proto: TypeId,
    witness: IndexId,
}

impl Fixture {
    fn new() -> Self {
        let mut types = TypeTable::new();
        types.ensure_array(types.primitive(Primitive::Numerus));
        let f32_ty = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
        let octeti = types.primitive(Primitive::Octeti);
        let param = types.intern_index(IndexExpr::Param(SIZE_NAME));
        let shape = types.intern_index(IndexExpr::Tuple(vec![param]));
        let target = types.tensor_with_shape(f32_ty, shape);
        let proto = types.tensor_with_shape(f32_ty, shape);
        let witness = types.intern_index(IndexExpr::Runtime(RuntimeExtentId::FunctionParameterAxis {
            parameter: 0,
            axis: 0,
        }));
        Self { types, octeti, target, proto, witness }
    }

    fn builder(&self, witnessed: bool) -> FunctionBuilder<'_> {
        let mut builder = FunctionBuilder::new(&self.types);
        let size_def = DefId(1);
        builder.size_params.insert(size_def, SIZE_NAME);
        if witnessed {
            builder.size_param_sources.insert(size_def, self.witness);
        }
        builder.add_param(DefId(2), PROTO_NAME, self.proto, HirParamMode::Owned, Span::default());
        builder
    }
}

fn lowered_shape_reads_the_witness(builder: &FunctionBuilder<'_>) -> bool {
    builder
        .blocks
        .iter()
        .flat_map(|block| &block.statements)
        .any(|statement| {
            matches!(
                &statement.kind,
                MirStatementKind::RuntimeCall { call, .. }
                    if matches!(call.intrinsic, MirIntrinsic::TensorShapeSize(_))
            )
        })
}

/// Whether a shape dimension lowered as the symbolic `ShapeSize` term inside
/// its `fractus → numerus` conversion — the form package finalization
/// substitutes for a generic pin (GSF-2).
fn lowered_shape_stays_symbolic(builder: &FunctionBuilder<'_>) -> bool {
    builder
        .blocks
        .iter()
        .flat_map(|block| &block.statements)
        .any(|statement| {
            matches!(
                &statement.kind,
                MirStatementKind::RuntimeCall { call, .. }
                    if matches!(call.intrinsic, MirIntrinsic::Convert(_))
                        && matches!(
                            call.args.first(),
                            Some(MirOperand::Constant(MirConstant::ShapeSize { .. }))
                        )
            )
        })
}

#[test]
fn witnessed_size_param_extent_attaches_the_runtime_shape_operand() {
    let fixture = Fixture::new();
    let mut builder = fixture.builder(true);

    assert!(builder.is_octeti_runtime_extent_tensor(fixture.octeti, fixture.target));

    let block = builder.fresh_block(Span::default());
    builder.switch_to(block);
    let source = MirOperand::Constant(MirConstant::Int(0));
    let shape = builder.lower_octeti_tensor_shape(source, fixture.target, Span::default());
    assert!(shape.is_some(), "a witnessed size-param target lowers a shape operand");
    assert!(builder.errors.is_empty(), "lowering reported errors");
    assert!(
        lowered_shape_reads_the_witness(&builder),
        "the shape operand must read the tensor-axis witness at run time"
    );
}

#[test]
fn unwitnessed_size_param_extent_stays_symbolic() {
    let fixture = Fixture::new();
    let mut builder = fixture.builder(false);

    assert!(!builder.is_octeti_runtime_extent_tensor(fixture.octeti, fixture.target));

    let block = builder.fresh_block(Span::default());
    builder.switch_to(block);
    let source = MirOperand::Constant(MirConstant::Int(0));
    let shape = builder.lower_lista_to_tensor_shape(source, fixture.target, Span::default());
    assert!(shape.is_some(), "an unwitnessed size parameter keeps the symbolic shape path");
    assert!(builder.errors.is_empty(), "lowering reported errors");
    assert!(
        !lowered_shape_reads_the_witness(&builder),
        "an unwitnessed size parameter must not read a runtime witness"
    );
}

/// GSF-2 regression: the ordinary `lista ↦ tensor<f32, [N]>` row with the
/// same witnessed `size` parameter must keep the symbolic `ShapeSize` term —
/// package finalization substitutes it for a generic pin, and a run-time
/// witness read would leak an unfinalizable `TensorShapeSize` into the pin's
/// shape operand (the linked `[T, D]` pin bakes `[3, 4]`).
#[test]
fn lista_source_conversion_keeps_the_symbolic_shape_terms_despite_a_witness() {
    let fixture = Fixture::new();
    let mut builder = fixture.builder(true);

    let block = builder.fresh_block(Span::default());
    builder.switch_to(block);
    let source = MirOperand::Constant(MirConstant::Int(0));
    let shape = builder.lower_lista_to_tensor_shape(source, fixture.target, Span::default());
    assert!(shape.is_some(), "a witnessed size-param target lowers a shape operand");
    assert!(builder.errors.is_empty(), "lowering reported errors");
    assert!(
        lowered_shape_stays_symbolic(&builder),
        "a lista-source conversion must keep the finalizable ShapeSize term"
    );
    assert!(
        !lowered_shape_reads_the_witness(&builder),
        "a lista-source conversion must not read a runtime witness"
    );
}

/// E2-4.2 pin: the shared shape helper must keep the symbolic `ShapeSize`
/// term whenever `witnessed_size_params` is off, even with the lowered
/// receiver handed in as `expression_value` — the argument form the
/// `TensorFromFlat` collection-op shape site passes. A witnessed `size N`
/// must never turn into a run-time `TensorShapeSize` read there: the WASM
/// leaf has no `TensorShapeSize` row and would fail closed. (The conversio
/// route's wrapper call — the site the ordinary `lista ↦ tensor` expression
/// rides — is pinned by
/// [`lista_source_conversion_keeps_the_symbolic_shape_terms_despite_a_witness`]
/// above; a wrapper-flag flip fails that test.)
#[test]
fn lista_source_expression_site_keeps_the_symbolic_shape_terms_despite_a_witness() {
    let fixture = Fixture::new();
    let mut builder = fixture.builder(true);

    let block = builder.fresh_block(Span::default());
    builder.switch_to(block);
    let source = MirOperand::Constant(MirConstant::Int(0));
    let expression_value = (0, source.clone());
    let shape = builder.lower_lista_to_tensor_shape_with_expression(
        source,
        fixture.target,
        Some(&expression_value),
        Span::default(),
        false,
    );
    assert!(shape.is_some(), "a witnessed size-param target lowers a shape operand");
    assert!(builder.errors.is_empty(), "lowering reported errors");
    assert!(
        lowered_shape_stays_symbolic(&builder),
        "the expression site must keep the finalizable ShapeSize term"
    );
    assert!(
        !lowered_shape_reads_the_witness(&builder),
        "the expression site must not read a runtime witness for a lista source"
    );
}

/// Pre-371f48cfa1 behaviour on the shared path: a declared `Runtime` extent
/// (here a tensor-binding axis with a formal parameter present) lowers to
/// the run-time `TensorShapeSize` read even where witnessed `size`
/// parameters stay symbolic. The witness flag must gate parameters only,
/// never the run-time extents themselves.
#[test]
fn runtime_extent_axis_lowers_the_runtime_shape_operand_on_the_lista_source_path() {
    let mut types = TypeTable::new();
    types.ensure_array(types.primitive(Primitive::Numerus));
    let f32_ty = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let axis = types.intern_index(IndexExpr::Runtime(RuntimeExtentId::FunctionParameterAxis {
        parameter: 0,
        axis: 0,
    }));
    let shape = types.intern_index(IndexExpr::Tuple(vec![axis]));
    let target = types.tensor_with_shape(f32_ty, shape);

    let mut builder = FunctionBuilder::new(&types);
    builder.add_param(DefId(2), PROTO_NAME, target, HirParamMode::Owned, Span::default());

    let block = builder.fresh_block(Span::default());
    builder.switch_to(block);
    let source = MirOperand::Constant(MirConstant::Int(0));
    let lowered = builder.lower_lista_to_tensor_shape(source, target, Span::default());
    assert!(lowered.is_some(), "a run-time extent target lowers a shape operand");
    assert!(builder.errors.is_empty(), "lowering reported errors");
    assert!(
        lowered_shape_reads_the_witness(&builder),
        "a declared run-time extent must keep the TensorShapeSize read on the lista-source path"
    );
}
