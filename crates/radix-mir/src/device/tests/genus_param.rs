//! gpu-reset C4-6b: a `genus` (struct) PARAMETER of a lane-form kernel passes
//! the device gate when its layout resolves and the body only reads scalar or
//! vector fields; the ABI turns it into a count-one `GenusParam` resource that
//! binds at its declared parameter position.

use super::*;
use crate::abi::{MirKernelResourceKind, MirKernelSignature};
use crate::validate::ValidatedMir;
use crate::{
    MirAggregate, MirAggregateFields, MirAggregateKind, MirBlock, MirBlockId, MirConstant,
    MirFunction, MirKernelShaderStage, MirLocal, MirLocalId, MirNamedOperand, MirOperand, MirParam,
    MirParamMode, MirPlace, MirProjection, MirStatement, MirStatementKind, MirStructLayout,
    MirTerminator, MirTerminatorKind, MirValidationContext, MirValue, MirValueId, MirValueKind,
};
use radix_hir::DefId;
use radix_lexer::{Interner, Symbol};
use radix_types::{Lowerability, TypeId};

const GENUS: DefId = DefId(7);

#[derive(Clone, Copy)]
enum Field {
    F32,
    Vec2,
    Bool,
    NestedGenus,
}

#[derive(Clone, Copy)]
enum Body {
    Empty,
    /// `local = p.field[idx]`
    Read(usize),
    /// `p.field[idx] = 1.0`
    Write(usize),
    /// `local = p` (the whole genus used as a value)
    WholeCopy,
    /// declare a mutable genus local, construct it field-by-field, then read
    /// one field back into a `fields[0]`-typed local.
    LocalConstructRead,
    /// declare a mutable genus local only; its field types are the only
    /// thing the gate sees (used when a nested field cannot be constructed).
    LocalDeclare,
}

fn sym(index: usize) -> Symbol {
    Symbol(100_000 - u32::try_from(index).expect("small index"))
}

struct Run {
    gate: Lowerability,
    signature: Result<MirKernelSignature, String>,
    layout: Result<MirStructLayout, crate::StructLayoutRefusal>,
    reflection: Option<crate::abi::MirKernelReflection>,
}

/// Build and run a lane-form kernel. `order` lists the declared parameters
/// before the trailing Lane parameter: `g` genus, `i` input tensor, `m` `mut`
/// output tensor. `lane_form == false` drops the Lane parameter.
fn run(fields: &[Field], order: &str, body: Body, lane_form: bool) -> Run {
    let mut types = TypeTable::new();
    let f32_id = f32_ty(&mut types).semantic_id();
    let genus = types.intern(Type::Struct(GENUS));
    let field_ids: Vec<TypeId> = fields
        .iter()
        .map(|field| match field {
            Field::F32 => f32_id,
            Field::Vec2 => {
                let width = types.intern_index(IndexExpr::Literal(2));
                types.intern(Type::Vector(f32_id, width))
            }
            Field::Bool => types.primitive(Primitive::Bivalens),
            Field::NestedGenus => types.intern(Type::Struct(DefId(8))),
        })
        .collect();
    let tensor = rank2_tensor_ty(&mut types, [2, 4]);
    let u32t = MirType::semantic(types.sized_numeric(Primitive::Numerus, NumericWidth::U32));
    let vacuum = MirType::semantic(types.intern(Type::Primitive(Primitive::Vacuum)));
    let genus = MirType::semantic(genus);
    let mut interner = Interner::new();

    let mut params = Vec::new();
    let mut locals = Vec::new();
    for (index, kind) in order.chars().enumerate() {
        let (ty, mode, mutable) = match kind {
            'g' => (genus, MirParamMode::Owned, false),
            'i' => (tensor, MirParamMode::Ref, false),
            'm' => (tensor, MirParamMode::MutRef, true),
            other => panic!("unknown param kind {other}"),
        };
        let id = u32::try_from(index).unwrap();
        params.push(MirParam {
            local: MirLocalId(id),
            name: Some(interner.intern(&format!("p{index}"))),
            ty,
            mode,
            span: test_span(),
        });
        locals.push(MirLocal {
            id: MirLocalId(id),
            name: None,
            ty,
            mutable,
            span: test_span(),
        });
    }
    let mut next = u32::try_from(order.len()).unwrap();
    if lane_form {
        params.push(MirParam {
            local: MirLocalId(next),
            name: Some(interner.intern("lane")),
            ty: u32t,
            mode: MirParamMode::Lane,
            span: test_span(),
        });
        locals.push(MirLocal {
            id: MirLocalId(next),
            name: None,
            ty: u32t,
            mutable: false,
            span: test_span(),
        });
        next += 1;
    }
    let genus_local = MirLocalId(u32::try_from(order.find('g').unwrap_or(0)).unwrap());
    let field_place = |index: usize| MirPlace {
        base: crate::MirPlaceBase::Local(genus_local),
        projections: vec![MirProjection::Field(sym(index))],
    };
    let value = |kind, ty| MirValue {
        id: MirValueId(0),
        kind,
        ty,
        span: test_span(),
    };
    let assign = |place, value| MirStatement {
        kind: MirStatementKind::Assign { place, value },
        span: test_span(),
    };
    let statements = match body {
        Body::Empty => Vec::new(),
        Body::Read(index) => {
            let ty = MirType::semantic(field_ids[index]);
            let target = MirLocalId(next);
            locals.push(MirLocal {
                id: target,
                name: None,
                ty,
                mutable: true,
                span: test_span(),
            });
            vec![assign(
                MirPlace::local(target),
                value(
                    MirValueKind::Operand(MirOperand::Place(field_place(index))),
                    ty,
                ),
            )]
        }
        Body::Write(index) => vec![assign(
            field_place(index),
            value(
                MirValueKind::Operand(MirOperand::Constant(MirConstant::Float(1.0))),
                MirType::semantic(field_ids[index]),
            ),
        )],
        Body::WholeCopy => {
            let target = MirLocalId(next);
            locals.push(MirLocal {
                id: target,
                name: None,
                ty: genus,
                mutable: true,
                span: test_span(),
            });
            vec![assign(
                MirPlace::local(target),
                value(
                    MirValueKind::Operand(MirOperand::Place(MirPlace::local(genus_local))),
                    genus,
                ),
            )]
        }
        Body::LocalConstructRead => {
            let local = MirLocalId(next);
            locals.push(MirLocal {
                id: local,
                name: None,
                ty: genus,
                mutable: true,
                span: test_span(),
            });
            next += 1;
            let ty = MirType::semantic(field_ids[0]);
            let field_local = MirLocalId(next);
            locals.push(MirLocal {
                id: field_local,
                name: None,
                ty,
                mutable: true,
                span: test_span(),
            });
            let field_local_place = MirPlace::local(field_local);
            let named = (0..fields.len())
                .map(|index| MirNamedOperand {
                    name: sym(index),
                    value: MirOperand::Place(field_local_place.clone()),
                })
                .collect();
            vec![
                assign(
                    field_local_place.clone(),
                    value(
                        MirValueKind::Operand(MirOperand::Constant(MirConstant::Float(1.0))),
                        ty,
                    ),
                ),
                MirStatement {
                    kind: MirStatementKind::Construct {
                        destination: MirPlace::local(local),
                        aggregate: MirAggregate {
                            kind: MirAggregateKind::Struct(GENUS),
                            ty: genus,
                            fields: MirAggregateFields::Named(named),
                        },
                    },
                    span: test_span(),
                },
                assign(
                    field_local_place,
                    value(
                        MirValueKind::Operand(MirOperand::Place(MirPlace {
                            base: crate::MirPlaceBase::Local(local),
                            projections: vec![MirProjection::Field(sym(0))],
                        })),
                        ty,
                    ),
                ),
            ]
        }
        Body::LocalDeclare => {
            let local = MirLocalId(next);
            locals.push(MirLocal {
                id: local,
                name: None,
                ty: genus,
                mutable: true,
                span: test_span(),
            });
            Vec::new()
        }
    };
    let function = MirFunction {
        id: MirFunctionId(0),
        source: Some(DefId(1)),
        name: Some(interner.intern("genus_kernel")),
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
        GENUS,
        order_syms
            .iter()
            .zip(&field_ids)
            .map(|(name, ty)| (*name, MirType::semantic(*ty)))
            .collect(),
    );
    ctx.struct_field_order.insert(GENUS, order_syms);
    let validated = ValidatedMir::new(
        crate::MirProgram {
            functions: vec![function],
        },
        ctx,
    )
    .expect("the genus kernel fixture validates");
    let layout = crate::resolve_struct_layout(genus, validated.validation());
    let signature = MirKernelSignature::storage_buffer_kernel_with_interner_for_target_entry(
        &validated.program().functions[0],
        validated.validation(),
        &interner,
    )
    .map_err(|error| error.message);
    let reflection = signature.as_ref().ok().map(|signature| {
        crate::abi::MirKernelReflection::from_signature("k", signature, validated.validation())
            .expect("a genus resource reflects without a scalar element")
    });
    Run {
        reflection,
        gate: classify_device_safe_for_target_entry(&validated, &kernel_device_context()),
        signature,
        layout,
    }
}

fn shapes(gate: &Lowerability) -> Vec<String> {
    gate.gaps()
        .iter()
        .map(|gap| gap.shape().to_owned())
        .collect()
}

const PARAMS_LIKE: [Field; 3] = [Field::F32, Field::Vec2, Field::F32];

#[test]
fn gate_admits_a_params_like_genus_read_through_field() {
    for index in 0..PARAMS_LIKE.len() {
        let run = run(&PARAMS_LIKE, "gim", Body::Read(index), true);
        assert!(
            run.gate.is_capable(),
            "genus field {index} read must pass the gate: {:?}",
            shapes(&run.gate)
        );
    }
}

#[test]
fn gate_refuses_a_genus_with_a_bool_field_by_name() {
    let run = run(&[Field::F32, Field::Bool], "gim", Body::Empty, true);
    assert!(
        shapes(&run.gate).contains(&"device genus parameter: bool field".to_owned()),
        "got {:?}",
        shapes(&run.gate)
    );
}

#[test]
fn gate_refuses_a_nested_genus_by_name() {
    let run = run(&[Field::F32, Field::NestedGenus], "gim", Body::Empty, true);
    assert!(
        shapes(&run.gate).contains(&"device genus parameter: nested genus field".to_owned()),
        "got {:?}",
        shapes(&run.gate)
    );
}

#[test]
fn gate_refuses_a_field_write() {
    let run = run(&PARAMS_LIKE, "gim", Body::Write(0), true);
    assert!(
        shapes(&run.gate).contains(&"device genus parameter: field write".to_owned()),
        "got {:?}",
        shapes(&run.gate)
    );
}

#[test]
fn gate_admits_a_genus_local_constructed_and_read_in_lane_form() {
    let run = run(
        &[Field::F32, Field::F32],
        "im",
        Body::LocalConstructRead,
        true,
    );
    assert!(
        run.gate.is_capable(),
        "a constructed genus local and its field read must pass the gate: {:?}",
        shapes(&run.gate)
    );
}

#[test]
fn gate_refuses_a_genus_local_outside_lane_form() {
    let run = run(
        &[Field::F32, Field::F32],
        "im",
        Body::LocalConstructRead,
        false,
    );
    assert!(
        shapes(&run.gate)
            .iter()
            .any(|shape| shape.starts_with("device aggregate value type")),
        "got {:?}",
        shapes(&run.gate)
    );
}

#[test]
fn gate_refuses_a_genus_local_with_a_nested_genus_field_by_name() {
    let run = run(
        &[Field::F32, Field::NestedGenus],
        "im",
        Body::LocalDeclare,
        true,
    );
    assert!(
        shapes(&run.gate).contains(&"device genus local: nested genus field".to_owned()),
        "got {:?}",
        shapes(&run.gate)
    );
}

#[test]
fn gate_refuses_a_whole_genus_parameter_copy_by_name() {
    let run = run(&PARAMS_LIKE, "gim", Body::WholeCopy, true);
    assert!(
        shapes(&run.gate).contains(
            &"device genus parameter: only a direct scalar or vector field read is admitted"
                .to_owned()
        ),
        "got {:?}",
        shapes(&run.gate)
    );
}

#[test]
fn gate_refuses_a_genus_parameter_outside_lane_form() {
    let run = run(&PARAMS_LIKE, "gim", Body::Empty, false);
    assert!(
        !run.gate.is_capable(),
        "only lane-form kernels admit a genus"
    );
}

#[test]
fn abi_binds_a_genus_between_tensors_at_its_declared_position() {
    // declared order: input tensor, genus, mut output.
    let run = run(&PARAMS_LIKE, "igm", Body::Read(0), true);
    let signature = run
        .signature
        .expect("the genus kernel has an ABI signature");
    let layout = run.layout.expect("Params-like layout");
    let mut slots: Vec<(u32, MirKernelResourceKind, u64)> = signature
        .resources()
        .map(|resource| (resource.binding, resource.kind, resource.element_count))
        .collect();
    slots.sort_by_key(|(binding, _, _)| *binding);
    assert_eq!(
        slots,
        vec![
            (0, MirKernelResourceKind::StorageBuffer, 8),
            (
                1,
                MirKernelResourceKind::GenusParam {
                    size_bytes: layout.size
                },
                1
            ),
            (2, MirKernelResourceKind::StorageBuffer, 8),
        ]
    );
    // f32 + vec2 (align 8) + f32, rounded to 8.
    assert_eq!(layout.size, 24);
    let genus = signature
        .resources()
        .find(|resource| matches!(resource.kind, MirKernelResourceKind::GenusParam { .. }))
        .expect("genus resource");
    assert_eq!(genus.source_local, Some(MirLocalId(1)));
}

#[test]
fn reflection_carries_the_genus_as_raw_bytes_of_the_struct_size() {
    let run = run(&PARAMS_LIKE, "igm", Body::Empty, true);
    let reflection = run.reflection.expect("reflection");
    let genus = reflection
        .resources
        .iter()
        .find(|resource| matches!(resource.kind, MirKernelResourceKind::GenusParam { .. }))
        .expect("genus reflection");
    assert_eq!((genus.element_byte_width, genus.buffer_byte_len), (24, 24));
}

#[test]
fn abi_binds_a_genus_first_when_it_is_the_only_input() {
    // The single non-output input is the genus: it must not take the
    // single-tensor-input arm.
    let run = run(&PARAMS_LIKE, "gm", Body::Read(0), true);
    let signature = run
        .signature
        .expect("the genus kernel has an ABI signature");
    assert!(matches!(
        signature.input.kind,
        MirKernelResourceKind::GenusParam { size_bytes: 24 }
    ));
    assert_eq!(signature.output.binding, 1);
}

#[test]
fn abi_refuses_an_unresolvable_genus_by_name() {
    let run = run(&[Field::Bool], "igm", Body::Empty, true);
    assert_eq!(
        run.signature.err().as_deref(),
        Some("device genus parameter: bool field")
    );
}
