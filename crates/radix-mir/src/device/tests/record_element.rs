//! gpu-reset Q01: a `tensor<Block, [n]>` PARAMETER of a lane-form kernel whose
//! element is a fixed-layout genus (a RECORD device view) passes the device
//! gate when the body indexes the view and reads admitted fields of the
//! element — a scalar field and a fixed-array (`list<u8, N>`) element.

use super::*;
use crate::validate::ValidatedMir;
use crate::{
    MirBlock, MirBlockId, MirConstant, MirDeviceViewElement, MirDeviceViewLayout, MirFunction,
    MirKernelShaderStage, MirLayoutKind, MirLocal, MirLocalId, MirOperand, MirParam, MirParamMode,
    MirPlace, MirPlaceBase, MirProjection, MirScalarLayout, MirStatement, MirStatementKind,
    MirTerminator, MirTerminatorKind, MirValidationContext, MirValue, MirValueId, MirValueKind,
};
use radix_hir::DefId;
use radix_lexer::{Interner, Symbol};
use radix_types::{Lowerability, TypeId};

const BLOCK: DefId = DefId(7);

#[derive(Clone, Copy)]
enum Field {
    F32,
    /// `list<u8, 2>`
    ListU8x2,
    F64,
}

#[derive(Clone, Copy)]
enum Body {
    /// read nothing
    Empty,
    /// `local = blocks[0].scale` and `local = blocks[0].values[1]`
    ReadScalarAndListElement,
}

fn sym(index: usize) -> Symbol {
    Symbol(100_000 - u32::try_from(index).expect("small index"))
}

fn constant_index(index: i64) -> MirOperand {
    MirOperand::Constant(MirConstant::Int(index))
}

fn run(fields: &[Field], body: Body) -> Lowerability {
    let mut types = TypeTable::new();
    let f32_id = f32_ty(&mut types).semantic_id();
    let u8_id = types.sized_numeric(Primitive::Numerus, NumericWidth::U8);
    let block = types.intern(Type::Struct(BLOCK));
    let field_ids: Vec<TypeId> = fields
        .iter()
        .map(|field| match field {
            Field::F32 => f32_id,
            Field::ListU8x2 => {
                let capacity = types.intern_index(IndexExpr::Literal(2));
                types.bounded_array(u8_id, capacity)
            }
            Field::F64 => types.sized_numeric(Primitive::Fractus, NumericWidth::F64),
        })
        .collect();
    let extent = types.intern_index(IndexExpr::Literal(2));
    let shape = types.intern_index(IndexExpr::Tuple(vec![extent]));
    let tensor_id = types.tensor_with_shape(block, shape);
    let tensor = MirType::semantic(tensor_id);
    let u32_id = types.sized_numeric(Primitive::Numerus, NumericWidth::U32);
    let u32t = MirType::semantic(u32_id);
    let vacuum = MirType::semantic(types.intern(Type::Primitive(Primitive::Vacuum)));
    let mut interner = Interner::new();

    let params = vec![
        MirParam {
            local: MirLocalId(0),
            name: Some(interner.intern("blocks")),
            ty: tensor,
            mode: MirParamMode::Ref,
            span: test_span(),
        },
        MirParam {
            local: MirLocalId(1),
            name: Some(interner.intern("lane")),
            ty: u32t,
            mode: MirParamMode::Lane,
            span: test_span(),
        },
    ];
    let mut locals = vec![
        MirLocal {
            id: MirLocalId(0),
            name: None,
            ty: tensor,
            mutable: false,
            span: test_span(),
        },
        MirLocal {
            id: MirLocalId(1),
            name: None,
            ty: u32t,
            mutable: false,
            span: test_span(),
        },
    ];

    let element_place = |field: usize, extra: Option<MirProjection>| MirPlace {
        base: MirPlaceBase::Local(MirLocalId(0)),
        projections: {
            let mut projections = vec![
                MirProjection::Index(constant_index(0)),
                MirProjection::Field(sym(field)),
            ];
            projections.extend(extra);
            projections
        },
    };

    let statements = match body {
        Body::Empty => Vec::new(),
        Body::ReadScalarAndListElement => {
            let reads = [
                (0usize, None, MirType::semantic(f32_id)),
                (
                    1usize,
                    Some(MirProjection::Index(constant_index(1))),
                    // the `u8` element widens into a device-admitted `u32`
                    u32t,
                ),
            ];
            let mut statements = Vec::new();
            for (offset, (field, extra, ty)) in reads.into_iter().enumerate() {
                let target = MirLocalId(2 + u32::try_from(offset).expect("small index"));
                locals.push(MirLocal {
                    id: target,
                    name: None,
                    ty,
                    mutable: true,
                    span: test_span(),
                });
                statements.push(MirStatement {
                    kind: MirStatementKind::Assign {
                        place: MirPlace::local(target),
                        value: MirValue {
                            id: MirValueId(0),
                            kind: MirValueKind::Operand(MirOperand::Place(element_place(
                                field, extra,
                            ))),
                            ty,
                            span: test_span(),
                        },
                    },
                    span: test_span(),
                });
            }
            statements
        }
    };

    let function = MirFunction {
        id: MirFunctionId(0),
        source: Some(DefId(1)),
        name: Some(interner.intern("record_kernel")),
        params,
        locals,
        temps: Vec::new(),
        blocks: vec![MirBlock {
            id: MirBlockId(0),
            statements,
            terminator: MirTerminator {
                kind: MirTerminatorKind::Return(None),
                span: test_span(),
            },
            span: test_span(),
        }],
        return_ty: vacuum,
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: Some(MirKernelShaderStage::Compute),
        span: test_span(),
    };

    let mut ctx = MirValidationContext::new(&types);
    let order_syms: Vec<Symbol> = (0..fields.len()).map(sym).collect();
    ctx.struct_fields.insert(
        BLOCK,
        order_syms
            .iter()
            .zip(&field_ids)
            .map(|(name, ty)| (*name, MirType::semantic(*ty)))
            .collect(),
    );
    ctx.struct_field_order.insert(BLOCK, order_syms);
    // The gate's host-owned entry row admits a host-staged tensor without
    // looking at its element; a record-element DEVICE view is what this unit
    // admits, so pin the tensor to the device locus exactly as the ABI test
    // helpers do.
    let mut handle = MirDeviceViewLayout::device_handle_row_major(MirScalarLayout::F32, 1);
    handle.element = MirDeviceViewElement::Record(block);
    ctx.layouts
        .set_type_layout(tensor_id, MirLayoutKind::DeviceView(handle));
    let validated = ValidatedMir::new(
        crate::MirProgram {
            functions: vec![function],
        },
        ctx,
    )
    .expect("the record-element kernel fixture validates");
    classify_device_safe_for_target_entry(&validated, &kernel_device_context())
}

fn shapes(gate: &Lowerability) -> Vec<String> {
    gate.gaps()
        .iter()
        .map(|gap| gap.shape().to_owned())
        .collect()
}

#[test]
fn gate_admits_record_tensor_field_read() {
    let gate = run(
        &[Field::F32, Field::ListU8x2],
        Body::ReadScalarAndListElement,
    );
    assert!(
        gate.is_capable(),
        "a record-element device view field read must pass the gate: {:?}",
        shapes(&gate)
    );
}

#[test]
fn gate_refuses_record_element_with_f64_field_by_name() {
    let gate = run(&[Field::F64], Body::Empty);
    assert!(
        shapes(&gate).contains(&"kernel device view element: f64 field".to_owned()),
        "got {:?}",
        shapes(&gate)
    );
}
