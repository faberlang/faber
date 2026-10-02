use super::*;
use radix_mir::{MirFunction, MirLocal, MirParam, MirParamMode};
use radix_types::{IndexExpr, RuntimeExtentId};

fn tensor_parameter_function(tensor_ty: TypeId) -> MirFunction {
    let ty = MirType::semantic(tensor_ty);
    let local = MirLocalId(0);
    MirFunction {
        id: MirFunctionId(0),
        source: None,
        name: None,
        params: vec![MirParam {
            local,
            name: None,
            ty,
            mode: MirParamMode::Owned,
            span: Span::default(),
        }],
        locals: vec![MirLocal {
            id: local,
            name: None,
            ty,
            mutable: false,
            span: Span::default(),
        }],
        temps: vec![],
        blocks: vec![],
        return_ty: MirType::semantic(tensor_ty),
        error_ty: None,
        is_async: false,
        is_generator: false,
        shader_stage: None,
        span: Span::default(),
    }
}

#[test]
fn shape_size_reads_runtime_formal_axes_and_checks_affine_division() {
    let mut types = TypeTable::new();
    let element = types.primitive(Primitive::Fractus);
    let runtime_axis = types.intern_index(IndexExpr::Runtime(
        RuntimeExtentId::FunctionParameterAxis {
            parameter: 0,
            axis: 1,
        },
    ));
    let plus_one = types.intern_index(IndexExpr::Affine {
        base: runtime_axis,
        offset: 1,
    });
    let divide_by_three = types.intern_index(IndexExpr::Divide {
        numerator: runtime_axis,
        divisor: 3,
    });
    let shape = types.intern_index(IndexExpr::Tuple(vec![plus_one, divide_by_three]));
    let tensor_ty = types.tensor_with_shape(element, shape);
    let function = tensor_parameter_function(tensor_ty);
    let mut program = MirProgram::new();
    program.functions.push(function);
    let context = MirValidationContext::new(&types);
    let interner = Interner::new();
    let mut host = BufferHost::default();
    let mut runner = Runner::new(&program, &context, &types, &interner, &mut host);
    runner.push_frame(MirFunctionId(0), MirBlockId(0));
    runner.frames[0].locals.insert(
        MirLocalId(0),
        Value::Tensor(Rc::new(RefCell::new(value::TensorValue::contiguous(
            vec![Value::Float(0.0); 15],
            vec![3, 6],
        )))),
    );

    let total = runner
        .eval_constant(&MirConstant::ShapeSize { shape, axis: None }, Span::default())
        .expect("dynamic dimensions produce the element count");
    assert_eq!(total, Value::Float(14.0));
    let second = runner
        .eval_constant(&MirConstant::ShapeSize { shape, axis: Some(1) }, Span::default())
        .expect("axis extent evaluates from its formal tensor");
    assert_eq!(second, Value::Float(2.0));
}

#[test]
fn shape_size_rejects_non_integral_runtime_dimension_division() {
    let mut types = TypeTable::new();
    let element = types.primitive(Primitive::Fractus);
    let runtime_axis = types.intern_index(IndexExpr::Runtime(
        RuntimeExtentId::FunctionParameterAxis {
            parameter: 0,
            axis: 0,
        },
    ));
    let divide_by_two = types.intern_index(IndexExpr::Divide {
        numerator: runtime_axis,
        divisor: 2,
    });
    let tensor_shape = types.intern_index(IndexExpr::Tuple(vec![runtime_axis]));
    let tensor_ty = types.tensor_with_shape(element, tensor_shape);
    let function = tensor_parameter_function(tensor_ty);
    let mut program = MirProgram::new();
    program.functions.push(function);
    let context = MirValidationContext::new(&types);
    let interner = Interner::new();
    let mut host = BufferHost::default();
    let mut runner = Runner::new(&program, &context, &types, &interner, &mut host);
    runner.push_frame(MirFunctionId(0), MirBlockId(0));
    runner.frames[0].locals.insert(
        MirLocalId(0),
        Value::Tensor(Rc::new(RefCell::new(value::TensorValue::contiguous(
            vec![Value::Float(0.0); 5],
            vec![5],
        )))),
    );

    let error = runner
        .eval_constant(
            &MirConstant::ShapeSize {
                shape: divide_by_two,
                axis: None,
            },
            Span::default(),
        )
        .expect_err("non-integral natural-number division must not truncate");
    assert_eq!(error.issue, "runner_runtime_shape_extent_division_is_not_exact");
}
