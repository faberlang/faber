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

#[test]
fn witnessed_size_param_extent_attaches_the_runtime_shape_operand() {
    let fixture = Fixture::new();
    let mut builder = fixture.builder(true);

    assert!(builder.is_octeti_runtime_extent_tensor(fixture.octeti, fixture.target));

    let block = builder.fresh_block(Span::default());
    builder.switch_to(block);
    let source = MirOperand::Constant(MirConstant::Int(0));
    let shape = builder.lower_lista_to_tensor_shape(source, fixture.target, Span::default());
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
