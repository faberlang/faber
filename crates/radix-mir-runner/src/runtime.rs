//! In-process evaluation of MIR runtime calls, aggregates, and projections.
//!
//! This file is the runner's host-facing interpreter surface: runtime
//! intrinsics and constructs become [`Value`]s through the active host.
//! Control-flow stepping stays in `lib.rs`. Target-directed `↦` conversion
//! stays in `conversio`.

#![allow(
    clippy::bool_to_int_with_if,
    clippy::cast_lossless,
    clippy::float_cmp,
    clippy::match_bool,
    clippy::match_same_arms,
    clippy::match_wildcard_for_single_variants,
    clippy::needless_pass_by_value,
    clippy::too_many_lines,
    clippy::map_unwrap_or,
    clippy::unused_self
)]
// Semantic numeric conversions between host value kinds: collection and
// tensor sizes/shapes surface as `Value::Int` (usize/i64), indexes narrow to
// `usize`, and mixed-carrier math promotes to f64. Loss beyond the i64/f64
// host range is the runner's deliberate wrap-on-convert behavior.
#![expect(
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    reason = "semantic numeric conversions between host value kinds in the MIR runtime evaluator"
)]

use super::cursor::GeneratorCursor;
use super::sermo::SermoView;
use super::value::{
    GradientStorage, MapKey, TensorValue, Value, float_to_numerus_width, format_template,
    modular_operand, modular_word_mask, modular_word_result, uint_width_checked_add,
    uint_width_checked_mul, values_equal, width_checked_add, width_checked_mul,
};
use super::{Runner, RunnerError, gelu_f64, host::provider_identity};
use radix_hir::DefId;
use radix_lexer::{Span, Symbol};
use radix_mir::{
    MirAggregate, MirAggregateFields, MirAggregateItem, MirAggregateKind, MirBlockId,
    MirCollectionOp, MirConstant, MirConversion, MirConversionFlavor, MirGradientOp, MirIntrinsic,
    MirOperand, MirOptionChainLink, MirOptionOp, MirOptionUnwrapMode, MirPlaceBase, MirProvider,
    MirProviderKind, MirRuntimeCall, MirSermoOp, MirType, MirTypeCheckOp,
};
use radix_runtime_contract::tensor::{
    ERR_ACCIPE_INVALID_INDEX, ERR_CREA_INVALID_SHAPE, ERR_ELEMENT_COUNT_OVERFLOW,
    ERR_FORMA_ELEMENT_COUNT, ERR_FORMA_RESHAPE_COUNT, ERR_MATMUL_ARGUMENT_RANK,
    ERR_MATMUL_INNER_DIMENSION, ERR_MATMUL_RECEIVER_RANK, ERR_PONDE_INVALID_INDEX,
    tensor_dim_non_negative, tensor_flat_offset, tensor_shape_element_count,
};
use radix_types::{
    IndexExpr, IndexId, NumericWidth, Primitive, Type, TypeId, TypeTable, effective_width,
};
use rustc_hash::FxHashMap;
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Clone, Copy)]
enum RowSoftmaxAccess {
    Safe,
    Direct,
}

fn row_softmax(
    values: &[Value],
    base: usize,
    last_dim: usize,
    access: RowSoftmaxAccess,
    span: Span,
) -> Result<Vec<f64>, RunnerError> {
    let value_at = |index| match access {
        RowSoftmaxAccess::Safe => values
            .get(index)
            .ok_or_else(|| RunnerError::internal(span, "tensor softmax index out of bounds")),
        RowSoftmaxAccess::Direct => Ok(&values[index]),
    };

    // Subtract max before exp so the row softmax stays finite.
    let mut max_val = f64::NEG_INFINITY;
    for i in 0..last_dim {
        let value = value_at(base + i)?;
        max_val = max_val.max(numeric_as_f64(value, span)?);
    }
    let mut exps = Vec::with_capacity(last_dim);
    let mut exp_sum = 0.0_f64;
    for i in 0..last_dim {
        let value = value_at(base + i)?;
        let exp_val = (numeric_as_f64(value, span)? - max_val).exp();
        exps.push(exp_val);
        exp_sum += exp_val;
    }
    Ok(exps.into_iter().map(|exp_val| exp_val / exp_sum).collect())
}

impl<H: super::Host + ?Sized> Runner<'_, H> {
    // RUNTIME CALL DISPATCH

    pub(crate) fn eval_runtime_call(
        &mut self,
        call: &MirRuntimeCall,
        span: Span,
    ) -> Result<Value, RunnerError> {
        match &call.intrinsic {
            MirIntrinsic::Diagnostic(kind) => {
                let [arg] = call.args.as_slice() else {
                    return Err(RunnerError::internal(span, "diagnostic runtime arity"));
                };
                let value = self.eval_operand(arg, span)?;
                // 9007e12c3 follow-through: floats declared at a narrower
                // width than f64 display at that width.
                let float_width = self.operand_float_width(arg);
                let text = value.display_typed(self.interner, span, float_width)?;
                self.host.scribe(*kind, &text)?;
                Ok(Value::Unit)
            }
            MirIntrinsic::FormatString { template } => {
                let template_text = self.interner.resolve(*template);
                let args = call
                    .args
                    .iter()
                    .map(|arg| self.eval_operand(arg, span))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Value::Str(format_template(
                    template_text,
                    &args,
                    self.interner,
                    span,
                )?))
            }
            MirIntrinsic::FormatValue { spec } => {
                let [arg] = call.args.as_slice() else {
                    return Err(RunnerError::internal(span, "format_value runtime arity"));
                };
                let value = self.eval_operand(arg, span)?;
                let float_width = self.operand_float_width(arg);
                let spec = self.interner.resolve(*spec);
                format_value(&value, spec, float_width, self.interner, span).map(Value::Str)
            }
            MirIntrinsic::Convert(conversion) => self.eval_convert(conversion, &call.args, span),
            MirIntrinsic::Collection(op) => {
                self.eval_collection(*op, &call.args, call.return_ty, span)
            }
            MirIntrinsic::Atomic(op) => Err(RunnerError::unsupported(
                span,
                &format!("intrinsic Atomic({})", op.spelling()),
            )),
            MirIntrinsic::Assert => self.eval_assert(&call.args, span),
            MirIntrinsic::Panic => self.eval_panic(&call.args, span),
            MirIntrinsic::ReadLine => {
                if !call.args.is_empty() {
                    return Err(RunnerError::internal(span, "read_line runtime arity"));
                }
                match self.host.read_line()? {
                    Some(line) => Ok(Value::Str(line)),
                    None => Ok(Value::Nil),
                }
            }
            MirIntrinsic::Provider(provider) => {
                self.eval_provider(provider, &call.args, call.return_ty, span)
            }
            MirIntrinsic::SermoOpen => self.eval_sermo_open(&call.args, span),
            MirIntrinsic::SermoSetOpener => self.eval_sermo_set_opener(&call.args, span),
            MirIntrinsic::Sermo(op) => self.eval_sermo_op(op, &call.args, call.return_ty, span),
            MirIntrinsic::Cede => self.eval_cede(&call.args, span),
            MirIntrinsic::CursorStream(stream) => self.eval_cursor_stream(stream, &call.args, span),
            MirIntrinsic::Gradient(op) => self.eval_gradient(*op, &call.args, span),
            MirIntrinsic::TypeCheck(op) => self.eval_type_check(op, &call.args, span),
            MirIntrinsic::Duplicate => {
                let [source] = call.args.as_slice() else {
                    return Err(RunnerError::internal(span, "duplicate runtime arity"));
                };
                Ok(self.eval_operand(source, span)?.deep_copy())
            }
            other => Err(RunnerError::unsupported(
                span,
                &format!("intrinsic {other:?}"),
            )),
        }
    }

    fn eval_sermo_open(&mut self, args: &[MirOperand], span: Span) -> Result<Value, RunnerError> {
        let [route] = args else {
            return Err(RunnerError::internal(span, "sermo_open runtime arity"));
        };
        let route = self
            .eval_operand(route, span)?
            .display(self.interner, span)?;
        self.sermo_open(route, span)
    }

    fn eval_sermo_set_opener(
        &mut self,
        args: &[MirOperand],
        span: Span,
    ) -> Result<Value, RunnerError> {
        let [sermo, opener] = args else {
            return Err(RunnerError::internal(
                span,
                "sermo_set_opener runtime arity",
            ));
        };
        let Value::Sermo(sermo) = self.eval_operand(sermo, span)? else {
            return Err(RunnerError::internal(
                span,
                "sermo_set_opener requires sermo handle",
            ));
        };
        let opener = self.eval_operand(opener, span)?;
        self.sermo_set_opener(&sermo, opener, span)?;
        Ok(Value::Unit)
    }

    fn eval_sermo_op(
        &mut self,
        op: &MirSermoOp,
        args: &[MirOperand],
        return_ty: MirType,
        span: Span,
    ) -> Result<Value, RunnerError> {
        if let MirSermoOp::DrainCursor = op {
            let [cursor] = args else {
                return Err(RunnerError::internal(span, "sermo cursor drain arity"));
            };
            return match self.eval_operand(cursor, span)? {
                Value::Array(frames) => {
                    let frames = frames.borrow().clone();
                    Ok(Value::Array(Rc::new(RefCell::new(frames))))
                }
                // WHY: a conversation cursor stays lazy (D6.6) — the walk
                // pulls one frame per step, so a cursor handler produces
                // while the caller consumes.
                Value::Cursor(cursor) => Ok(Value::Cursor(cursor)),
                _ => Err(RunnerError::internal(
                    span,
                    "sermo cursor drain requires cursor carrier",
                )),
            };
        }
        let Some(receiver) = args.first() else {
            return Err(RunnerError::internal(
                span,
                "sermo operation requires receiver",
            ));
        };
        let Value::Sermo(sermo) = self.eval_operand(receiver, span)? else {
            return Err(RunnerError::internal(
                span,
                "sermo operation receiver mismatch",
            ));
        };
        match op {
            MirSermoOp::ViewMeus => Ok(Value::Sermo(Rc::new(RefCell::new(
                sermo.borrow().with_view(SermoView::Meus),
            )))),
            MirSermoOp::ViewTuus => Ok(Value::Sermo(Rc::new(RefCell::new(
                sermo.borrow().with_view(SermoView::Tuus),
            )))),
            MirSermoOp::Send => Ok(Value::Unit),
            MirSermoOp::Finish => {
                self.sermo_finish(&sermo, span)?;
                let Type::Enum(status) = self.types.get(return_ty.semantic_id()) else {
                    return Err(RunnerError::internal(
                        span,
                        "sermo finish status type mismatch",
                    ));
                };
                Ok(Value::EnumVariant(
                    *status,
                    Rc::new(RefCell::new(FxHashMap::default())),
                ))
            }
            MirSermoOp::Receive { frame, data_field } => {
                Ok(self.sermo_next(&sermo, span)?.map_or(Value::Nil, |data| {
                    crate::sermo::frame_value(*frame, *data_field, data)
                }))
            }
            MirSermoOp::Cursor { frame, data_field } => Ok(Value::Cursor(GeneratorCursor::sermo(
                sermo,
                *frame,
                *data_field,
            ))),
            MirSermoOp::DrainCursor => {
                Err(RunnerError::internal(span, "nested cursor drain dispatch"))
            }
        }
    }

    /// `cede <expr>` — yield inside a stream generator.
    ///
    /// WHY: inside a generator frame (`Frame::is_generator`) the value is
    /// appended to the active yield buffer and `cede` returns `Unit`, letting
    /// the generator continue to the next yield.
    pub(crate) fn eval_cede(
        &mut self,
        args: &[MirOperand],
        span: Span,
    ) -> Result<Value, RunnerError> {
        let [arg] = args else {
            return Err(RunnerError::internal(span, "cede runtime arity"));
        };
        let value = self.eval_operand(arg, span)?;
        let is_generator = self.frames.last().is_some_and(|frame| frame.is_generator);
        if is_generator {
            // The run loop parks this frame once the statement completes.
            self.yielded = Some(value);
            Ok(Value::Unit)
        } else {
            Ok(value)
        }
    }

    /// Start a `@ cursor` generator call (D6.6: cursors are lazy).
    ///
    /// Mirrors `invoke_call` but builds the callee frame off the stack as a
    /// suspended generator. A stepped call returns the cursor for its `itera
    /// ex` walk to pull one `cede` per step; any other call materializes the
    /// `lista<T>` by pulling until the generator returns (its own return value
    /// is discarded).
    pub(crate) fn eval_cursor_stream(
        &mut self,
        stream: &radix_mir::MirCursorStream,
        args: &[MirOperand],
        span: Span,
    ) -> Result<Value, RunnerError> {
        let func_id = self.resolve_definition(stream.callee, span)?;
        self.refuse_kernel_execution(func_id, span)?;
        let param_locals = self
            .function_by_id(func_id, span)?
            .params
            .iter()
            .map(|param| param.local)
            .collect::<Vec<_>>();
        let arg_values = args
            .iter()
            .map(|arg| self.eval_operand(arg, span))
            .collect::<Result<Vec<_>, _>>()?;
        if param_locals.len() != arg_values.len() {
            return Err(RunnerError::internal(
                span,
                "cursor stream argument count mismatch",
            ));
        }

        let mut frame = self.new_frame(func_id, MirBlockId(0));
        frame.is_generator = true;
        for (local, arg) in param_locals.into_iter().zip(arg_values) {
            frame.locals.insert(local, arg);
        }
        let cursor = GeneratorCursor::new(frame);
        if stream.stepped {
            return Ok(Value::Cursor(cursor));
        }
        let items = self.cursor_collect(&cursor, span)?;
        Ok(Value::Array(Rc::new(RefCell::new(items))))
    }

    // GRADIENT HANDLES

    /// Evaluate an in-process gradient-handle operation.
    ///
    /// Mirrors the host LLVM gradient ABI semantics
    /// (`__faber_rt_v1_gradient_create/accumulate/read/zero`) as `Value`
    /// carriers: a plain host-side map keyed by opaque handle id, element-wise
    /// accumulation, and zero fill on `Zero`. This is the runner's
    /// clean-break in-process contract — the device ABI ownership surface
    /// (LLVM/WGSL/Wasm) is untouched.
    fn eval_gradient(
        &mut self,
        op: MirGradientOp,
        args: &[MirOperand],
        span: Span,
    ) -> Result<Value, RunnerError> {
        match op {
            MirGradientOp::Create => self.gradient_create(args, span),
            MirGradientOp::Accumulate => self.gradient_accumulate(args, span),
            MirGradientOp::Read => self.gradient_read(args, span),
            MirGradientOp::Zero => self.gradient_zero(args, span),
        }
    }

    /// `gradient create(shape, rank, kind)` — allocate zero-filled gradient
    /// storage and return an opaque handle id.
    ///
    /// Argument contract mirrors `validate_gradient_call`: `args[0]` is the
    /// shape (a `lista` of dims or a tensor whose shape provides the dims),
    /// `args[1]` the rank, `args[2]` the element kind. The kind is accepted
    /// as-is — the in-process carrier stores typed `Value`s, so the host's
    /// f32-only ABI restriction is not runner policy.
    fn gradient_create(&mut self, args: &[MirOperand], span: Span) -> Result<Value, RunnerError> {
        let [shape_arg, rank_arg, _kind_arg, ..] = args else {
            return Err(RunnerError::internal(span, "gradient create arity"));
        };
        let shape_value = self.eval_operand(shape_arg, span)?;
        let rank = self.eval_operand(rank_arg, span)?.as_index(span)?;
        let shape = gradient_shape_dims(&shape_value, rank, span)?;
        let count = tensor_element_count(&shape, span)?;
        let id = self.next_gradient_handle;
        self.next_gradient_handle = self
            .next_gradient_handle
            .checked_add(1)
            .ok_or_else(|| RunnerError::internal(span, "gradient handle id overflow"))?;
        self.gradient_handles.insert(
            id,
            GradientStorage {
                data: vec![Value::Float(0.0); count],
                shape,
            },
        );
        Ok(Value::GradientHandle(id))
    }

    /// `gradient accumulate(handle, data, shape, rank, scale)` — element-wise
    /// accumulate incoming gradient data into stored storage.
    ///
    /// Mirrors the host ABI: incoming shape must match the stored shape and
    /// the backing length must match before adding. The `scale` argument is
    /// accepted but unused, matching the LLVM emitter which lowers only
    /// `(handle, data, shape, rank)`.
    fn gradient_accumulate(
        &mut self,
        args: &[MirOperand],
        span: Span,
    ) -> Result<Value, RunnerError> {
        let [handle_arg, data_arg, shape_arg, rank_arg, _scale_arg] = args else {
            return Err(RunnerError::internal(span, "gradient accumulate arity"));
        };
        let handle = self.gradient_handle_id(handle_arg, span)?;
        let shape_value = self.eval_operand(shape_arg, span)?;
        let rank = self.eval_operand(rank_arg, span)?.as_index(span)?;
        let incoming_shape = gradient_shape_dims(&shape_value, rank, span)?;
        let incoming = self.eval_operand(data_arg, span)?;
        let storage = self.gradient_handles.get_mut(&handle).ok_or_else(|| {
            RunnerError::internal(span, format!("gradient accumulate unknown handle {handle}"))
        })?;
        if incoming_shape != storage.shape {
            return Err(RunnerError::internal(
                span,
                format!(
                    "gradient accumulate shape mismatch {:?} vs stored {:?}",
                    incoming_shape, storage.shape
                ),
            ));
        }
        match &incoming {
            Value::Tensor(tensor) => {
                accumulate_gradient_data(&mut storage.data, &tensor.borrow().data, span)?;
            }
            Value::Array(items) => {
                accumulate_gradient_data(&mut storage.data, &items.borrow(), span)?;
            }
            _ => {
                return Err(RunnerError::internal(
                    span,
                    "gradient accumulate data is not tensor storage",
                ));
            }
        }
        Ok(Value::Unit)
    }

    /// `gradient read(handle)` — return the accumulated gradient as a tensor.
    fn gradient_read(&mut self, args: &[MirOperand], span: Span) -> Result<Value, RunnerError> {
        let [handle_arg] = args else {
            return Err(RunnerError::internal(span, "gradient read arity"));
        };
        let handle = self.gradient_handle_id(handle_arg, span)?;
        let storage = self.gradient_handles.get(&handle).ok_or_else(|| {
            RunnerError::internal(span, format!("gradient read unknown handle {handle}"))
        })?;
        Ok(Value::Tensor(Rc::new(RefCell::new(TensorValue {
            data: storage.data.clone(),
            shape: storage.shape.clone(),
        }))))
    }

    /// `gradient zero(handle)` — reset the gradient storage to zero.
    fn gradient_zero(&mut self, args: &[MirOperand], span: Span) -> Result<Value, RunnerError> {
        let [handle_arg] = args else {
            return Err(RunnerError::internal(span, "gradient zero arity"));
        };
        let handle = self.gradient_handle_id(handle_arg, span)?;
        let storage = self.gradient_handles.get_mut(&handle).ok_or_else(|| {
            RunnerError::internal(span, format!("gradient zero unknown handle {handle}"))
        })?;
        for slot in &mut storage.data {
            *slot = Value::Float(0.0);
        }
        Ok(Value::Unit)
    }

    fn gradient_handle_id(&mut self, operand: &MirOperand, span: Span) -> Result<u64, RunnerError> {
        match self.eval_operand(operand, span)? {
            Value::GradientHandle(id) => Ok(id),
            other => Err(RunnerError::internal(
                span,
                format!("gradient operation requires opaque handle, got {other:?}"),
            )),
        }
    }

    // AGGREGATE CONSTRUCTION

    pub(crate) fn eval_construct(
        &mut self,
        aggregate: &MirAggregate,
        span: Span,
    ) -> Result<Value, RunnerError> {
        match aggregate.kind {
            MirAggregateKind::Tuple => {
                let items = self.eval_ordered_fields(&aggregate.fields, span)?;
                Ok(Value::Tuple(items))
            }
            MirAggregateKind::Array => {
                let items = self.eval_ordered_fields(&aggregate.fields, span)?;
                let items = self.apply_element_float_width(items, aggregate.ty.semantic_id());
                Ok(Value::Array(Rc::new(RefCell::new(items))))
            }
            MirAggregateKind::Record => {
                let map = self.eval_named_fields(
                    &aggregate.fields,
                    span,
                    "record construct requires named fields",
                )?;
                Ok(Value::Record(Rc::new(RefCell::new(map))))
            }
            MirAggregateKind::Map => {
                let mut map = FxHashMap::default();
                let MirAggregateFields::Keyed(entries) = &aggregate.fields else {
                    return Err(RunnerError::internal(
                        span,
                        "map construct requires keyed fields",
                    ));
                };
                for entry in entries {
                    let key = self.eval_operand(&entry.key, span)?.to_key(span)?;
                    let value = self.eval_operand(&entry.value, span)?;
                    map.insert(key, value);
                }
                Ok(Value::Map(Rc::new(RefCell::new(map))))
            }
            MirAggregateKind::Set => {
                let mut set = rustc_hash::FxHashSet::default();
                let items = self.eval_ordered_fields(&aggregate.fields, span)?;
                for item in items {
                    set.insert(item.to_key(span)?);
                }
                Ok(Value::Set(Rc::new(RefCell::new(set))))
            }
            MirAggregateKind::Tensor => {
                let MirAggregateFields::Ordered(items) = &aggregate.fields else {
                    return Err(RunnerError::internal(
                        span,
                        "tensor construct requires ordered fields",
                    ));
                };
                if !items.is_empty() {
                    return Err(RunnerError::internal(
                        span,
                        "tensor construct supports only vacua",
                    ));
                }
                Ok(Value::Tensor(Rc::new(RefCell::new(TensorValue {
                    data: Vec::new(),
                    shape: Vec::new(),
                }))))
            }
            MirAggregateKind::SparseTensor => {
                let MirAggregateFields::Ordered(items) = &aggregate.fields else {
                    return Err(RunnerError::internal(
                        span,
                        "sparse tensor construct requires ordered fields",
                    ));
                };
                if !items.is_empty() {
                    return Err(RunnerError::internal(
                        span,
                        "sparse tensor construct supports only vacua",
                    ));
                }
                let Type::Sparsa(element, shape) = self.types.get(aggregate.ty.semantic_id())
                else {
                    return Err(RunnerError::internal(
                        span,
                        "sparse tensor construct type mismatch",
                    ));
                };
                let shape = literal_sparse_shape(self.types, *shape, span)?;
                let zero = sparse_zero_value(self.types, *element, span)?;
                let count = tensor_element_count(&shape, span)?;
                Ok(tensor_value(vec![zero; count], shape))
            }
            MirAggregateKind::Interval { inclusive } => {
                let [start, end] = self
                    .eval_ordered_fields(&aggregate.fields, span)?
                    .try_into()
                    .map_err(|_| {
                        RunnerError::internal(span, "interval construct requires two bounds")
                    })?;
                // F-082: reject reversed bounds (start > end) — an interval must
                // be monotonic so every materialized element is reachable.
                let reversed = match (&start, &end) {
                    (Value::Int(a), Value::Int(b)) => a > b,
                    (Value::UInt(a), Value::UInt(b)) => a > b,
                    (Value::Float(a), Value::Float(b)) => a > b,
                    _ => false,
                };
                if reversed {
                    return Err(RunnerError::internal(
                        span,
                        "interval bounds must be monotonic (start ≤ end)",
                    ));
                }
                Ok(interval_value(start, end, inclusive))
            }
            MirAggregateKind::Vector => {
                let Type::Vector(_, width) = self.types.get(aggregate.ty.semantic_id()) else {
                    return Err(RunnerError::internal(
                        span,
                        "vector construct type mismatch",
                    ));
                };
                let Some(width) = radix_mir::vector_width::literal_vector_width(*width, self.types)
                else {
                    return Err(RunnerError::unsupported(
                        span,
                        "vector construct non-literal width",
                    ));
                };
                let values = self.eval_ordered_fields(&aggregate.fields, span)?;
                if values.len() != usize::from(width) {
                    return Err(RunnerError::internal(
                        span,
                        format!(
                            "vector construct expected {width} lanes, got {}",
                            values.len()
                        ),
                    ));
                }
                let values = self.apply_element_float_width(values, aggregate.ty.semantic_id());
                Ok(Value::Vector(Rc::new(RefCell::new(values))))
            }
            MirAggregateKind::Matrix => {
                let Type::Matrix(_, shape) = self.types.get(aggregate.ty.semantic_id()) else {
                    return Err(RunnerError::internal(
                        span,
                        "matrix construct type mismatch",
                    ));
                };
                let Some((rows, columns)) =
                    radix_mir::matrix_shape::literal_matrix_shape(*shape, self.types)
                else {
                    return Err(RunnerError::unsupported(
                        span,
                        "matrix construct non-literal shape",
                    ));
                };
                let values = self.eval_ordered_fields(&aggregate.fields, span)?;
                let expected = usize::from(rows) * usize::from(columns);
                if values.len() != expected {
                    return Err(RunnerError::internal(
                        span,
                        format!(
                            "matrix construct expected {expected} cells, got {}",
                            values.len()
                        ),
                    ));
                }
                let values = self.apply_element_float_width(values, aggregate.ty.semantic_id());
                Ok(Value::Matrix(Rc::new(RefCell::new(
                    super::value::MatrixValue {
                        rows,
                        columns,
                        data: values,
                    },
                ))))
            }
            MirAggregateKind::SharedCell => {
                let mut items = self.eval_ordered_fields(&aggregate.fields, span)?;
                let (Some(content), true) = (items.pop(), items.is_empty()) else {
                    return Err(RunnerError::internal(
                        span,
                        "shared cell construct requires exactly one item",
                    ));
                };
                Ok(Value::SharedCell(Rc::new(RefCell::new(content))))
            }
            MirAggregateKind::ClosureEnvironment(environment_id) => {
                let map = self.eval_named_fields(
                    &aggregate.fields,
                    span,
                    "closure environment construct requires named fields",
                )?;
                Ok(Value::ClosureEnvironment(
                    environment_id,
                    Rc::new(RefCell::new(map)),
                ))
            }
            MirAggregateKind::Struct(def_id) => {
                let map = self.eval_named_fields(
                    &aggregate.fields,
                    span,
                    "struct construct requires named fields",
                )?;
                Ok(Value::Struct(def_id, Rc::new(RefCell::new(map))))
            }
            MirAggregateKind::EnumVariant(def_id) => {
                let items = self.eval_variant_fields(&aggregate.fields, span)?;
                Ok(Value::EnumVariant(def_id, Rc::new(RefCell::new(items))))
            }
        }
    }

    // OPTION, CONVERSION, AND ASSERT

    pub(crate) fn eval_option_value(
        &mut self,
        op: &MirOptionOp,
        span: Span,
    ) -> Result<Value, RunnerError> {
        match op {
            MirOptionOp::None => Ok(Value::Nil),
            MirOptionOp::Some(operand) => self.eval_operand(operand, span),
            MirOptionOp::IsNil(operand) => {
                let value = self.eval_operand(operand, span)?;
                Ok(Value::Bool(matches!(value, Value::Nil)))
            }
            MirOptionOp::IsNotNil(operand) => {
                let value = self.eval_operand(operand, span)?;
                Ok(Value::Bool(!matches!(value, Value::Nil)))
            }
            MirOptionOp::Unwrap { value, mode } => {
                let value = self.eval_operand(value, span)?;
                match value {
                    Value::Nil => match mode {
                        MirOptionUnwrapMode::Assert => self.host.abort("unwrap of nihil"),
                        MirOptionUnwrapMode::Assume => {
                            Err(RunnerError::internal(span, "unwrap assume on nihil"))
                        }
                    },
                    other => Ok(other),
                }
            }
            MirOptionOp::Coalesce { value, fallback } => {
                let value = self.eval_operand(value, span)?;
                if matches!(value, Value::Nil) {
                    self.eval_operand(fallback, span)
                } else {
                    Ok(value)
                }
            }
            MirOptionOp::Chain { base, link } => self.eval_option_chain(base, link, span),
        }
    }

    fn eval_ordered_fields(
        &mut self,
        fields: &MirAggregateFields,
        span: Span,
    ) -> Result<Vec<Value>, RunnerError> {
        let MirAggregateFields::Ordered(items) = fields else {
            return Err(RunnerError::internal(
                span,
                "expected ordered aggregate fields",
            ));
        };
        let mut out = Vec::new();
        for item in items {
            match item {
                MirAggregateItem::Operand(operand) => out.push(self.eval_operand(operand, span)?),
                MirAggregateItem::Spread(operand) => {
                    let spread = self.eval_operand(operand, span)?;
                    match spread {
                        Value::Array(cell) => out.extend(cell.borrow().clone()),
                        _ => {
                            return Err(RunnerError::internal(span, "spread operand is not array"));
                        }
                    }
                }
            }
        }
        Ok(out)
    }

    fn eval_named_fields(
        &mut self,
        fields: &MirAggregateFields,
        span: Span,
        mismatch: &'static str,
    ) -> Result<FxHashMap<Symbol, Value>, RunnerError> {
        let MirAggregateFields::Named(fields) = fields else {
            return Err(RunnerError::internal(span, mismatch));
        };
        let mut map = FxHashMap::default();
        for field in fields {
            map.insert(field.name, self.eval_operand(&field.value, span)?);
        }
        Ok(map)
    }

    fn eval_variant_fields(
        &mut self,
        fields: &MirAggregateFields,
        span: Span,
    ) -> Result<FxHashMap<Symbol, Value>, RunnerError> {
        match fields {
            MirAggregateFields::Ordered(_) => {
                let mut out = FxHashMap::default();
                for (idx, value) in self
                    .eval_ordered_fields(fields, span)?
                    .into_iter()
                    .enumerate()
                {
                    out.insert(Symbol(idx as u32), value);
                }
                Ok(out)
            }
            MirAggregateFields::Named(items) => {
                let mut out = FxHashMap::default();
                for item in items {
                    let value = match self.eval_operand(&item.value, span) {
                        Ok(value) => value,
                        Err(err) if err.issue == "runner_variant_field_projection_mismatch" => {
                            Value::Nil
                        }
                        Err(err) => return Err(err),
                    };
                    out.insert(item.name, value);
                }
                Ok(out)
            }
            MirAggregateFields::Keyed(_) => Err(RunnerError::internal(
                span,
                "variant construct requires payload fields",
            )),
        }
    }

    pub(crate) fn eval_convert(
        &mut self,
        conversion: &MirConversion,
        args: &[MirOperand],
        span: Span,
    ) -> Result<Value, RunnerError> {
        let [source] = args else {
            return Err(RunnerError::internal(span, "conversion runtime arity"));
        };
        let value = self.eval_operand(source, span)?;
        if matches!(value, Value::Nil)
            && let Some(recovery) = &conversion.recovery
        {
            return self.eval_recovery(recovery, span);
        }
        match conversion.flavor {
            MirConversionFlavor::Cast => self.eval_cast(value, &conversion.target_ty, span),
            MirConversionFlavor::Runtime => self.eval_runtime_convert(conversion, value, span),
        }
    }

    fn eval_cast(
        &self,
        value: Value,
        target_ty: &radix_mir::MirType,
        span: Span,
    ) -> Result<Value, RunnerError> {
        let target = self.types.get(target_ty.semantic_id());
        match (value, target) {
            (
                Value::Int(v),
                Type::Primitive(Primitive::Fractus) | Type::SizedNumeric(Primitive::Fractus, _),
            ) => Ok(Value::Float(v as f64)),
            // A cast into `numerus<W>` saturates at the declared width exactly
            // as the `as W` cast codegen emits; the width-agnostic carrier
            // (bare `numerus`) stays i64.
            (Value::Float(v), Type::SizedNumeric(Primitive::Numerus, width)) => {
                Ok(float_to_numerus_width(v, *width))
            }
            (Value::Float(v), Type::Primitive(Primitive::Numerus)) => {
                Ok(float_to_numerus_width(v, NumericWidth::I64))
            }
            (
                Value::Bool(v),
                Type::Primitive(Primitive::Numerus) | Type::SizedNumeric(Primitive::Numerus, _),
            ) => Ok(Value::Int(if v { 1 } else { 0 })),
            (other, target) => Err(RunnerError::internal(
                span,
                format!("unsupported cast from {other:?} to {target:?}"),
            )),
        }
    }

    /// Evaluate `est <type>` / `non est <type>` against the operand's runtime
    /// variant tag.
    ///
    /// The runner's `Value` variants are the runtime tags (Valor boxing in
    /// `↦ value` is identity), so the tag check maps the operand carrier to
    /// its corresponding valor-carried type. Inner type parameters are erased
    /// at boxing time and never checked (mirrors the archived `est`
    /// variant-check surface).
    fn nominal_type_check_matches(&self, value: &Value, target: &Type) -> bool {
        match (value, target) {
            (Value::Struct(def, _), Type::Struct(target)) => def == target,
            (Value::EnumVariant(def, _), Type::Enum(target)) => self
                .context
                .variant_parents
                .get(def)
                .is_some_and(|parent| parent == target),
            _ => false,
        }
    }

    /// Type-check a runtime value against a target, recursively for unions.
    ///
    /// A union target describes a set of accepted carriers; treating the union
    /// itself as an opaque tag makes every `value is A ∪ B` check false even
    /// when the runtime value is one of its declared members.
    fn type_check_matches(&self, value: &Value, target: &Type) -> bool {
        if let Type::Union(members) = target {
            return members
                .iter()
                .any(|member| self.type_check_matches(value, self.types.get(*member)));
        }
        let nominal_match = self.nominal_type_check_matches(value, target);
        nominal_match
            || matches!(
                (value, target),
                (Value::Nil, Type::Primitive(Primitive::Nihil))
                    | (Value::Bool(_), Type::Primitive(Primitive::Bivalens))
                    | (
                        Value::Int(_) | Value::UInt(_),
                        Type::Primitive(Primitive::Numerus)
                            | Type::SizedNumeric(Primitive::Numerus, _)
                    )
                    | (
                        Value::Float(_),
                        Type::Primitive(Primitive::Fractus)
                            | Type::SizedNumeric(Primitive::Fractus, _)
                    )
                    | (
                        Value::Str(_),
                        Type::Primitive(Primitive::Textus | Primitive::Ascii)
                    )
                    | (
                        Value::Ascii(_),
                        Type::Primitive(Primitive::Ascii) | Type::BoundedAscii { .. }
                    )
                    | (Value::Octeti(_), Type::Primitive(Primitive::Octeti))
                    | (Value::Array(_), Type::Array(_))
                    | (Value::Map(_), Type::Map(_, _))
                    | (Value::Set(_), Type::Set(_))
                    | (
                        Value::Instans(_),
                        Type::Primitive(Primitive::Instans) | Type::SizedInstans(_)
                    )
                    | (Value::Regex(_), Type::Primitive(Primitive::Regex))
                    | (Value::Unit, Type::Primitive(Primitive::Vacuum))
                    | (_, Type::Primitive(Primitive::Valor | Primitive::Ignotum))
            )
    }

    fn eval_type_check(
        &mut self,
        op: &MirTypeCheckOp,
        args: &[MirOperand],
        span: Span,
    ) -> Result<Value, RunnerError> {
        let [operand] = args else {
            return Err(RunnerError::internal(span, "type_check runtime arity"));
        };
        let value = self.eval_operand(operand, span)?;
        let target = self.types.get(op.ty.semantic_id());
        let matches = self.type_check_matches(&value, target);
        Ok(Value::Bool(if op.positive { matches } else { !matches }))
    }

    fn eval_assert(&mut self, args: &[MirOperand], span: Span) -> Result<Value, RunnerError> {
        let Some(cond) = args.first() else {
            return Err(RunnerError::internal(span, "assert runtime arity"));
        };
        if !self.eval_operand(cond, span)?.as_bool(span)? {
            let message = args
                .get(1)
                .map(|arg| self.eval_operand(arg, span))
                .transpose()?
                .map(|value| value.display(self.interner, span))
                .transpose()?
                .unwrap_or_else(|| "assertion failed".to_owned());
            self.host.abort(&message);
        }
        Ok(Value::Unit)
    }

    fn eval_panic(&mut self, args: &[MirOperand], span: Span) -> Result<Value, RunnerError> {
        let [arg] = args else {
            return Err(RunnerError::internal(span, "panic runtime arity"));
        };
        let message = self.eval_operand(arg, span)?.display(self.interner, span)?;
        self.host.abort(&message);
    }

    // COLLECTION DISPATCH

    fn eval_collection(
        &mut self,
        op: MirCollectionOp,
        args: &[MirOperand],
        return_ty: MirType,
        span: Span,
    ) -> Result<Value, RunnerError> {
        match op {
            MirCollectionOp::Length => {
                let [receiver] = args else {
                    return Err(RunnerError::internal(span, "collection length arity"));
                };
                let value = self.eval_operand(receiver, span)?;
                let len = match value {
                    Value::Array(cell) => cell.borrow().len(),
                    Value::Cursor(cursor) => self.cursor_step_length(&cursor, span)?,
                    Value::Map(cell) => cell.borrow().len(),
                    Value::Set(cell) => cell.borrow().len(),
                    Value::Tensor(cell) => cell.borrow().shape.len(),
                    Value::Str(text) => text.chars().count(),
                    Value::Ascii(_) => 1,
                    Value::Octeti(bytes) => bytes.len(),
                    _ => return Err(RunnerError::internal(span, "length receiver type mismatch")),
                };
                Ok(Value::Int(len as i64))
            }
            MirCollectionOp::SparseNonzeroCount => {
                let [receiver] = args else {
                    return Err(RunnerError::internal(span, "sparse nonzero-count arity"));
                };
                let Value::Tensor(tensor) = self.eval_operand(receiver, span)? else {
                    return Err(RunnerError::internal(
                        span,
                        "sparse nonzero-count receiver type mismatch",
                    ));
                };
                let count =
                    tensor
                        .borrow()
                        .data
                        .iter()
                        .try_fold(0_i64, |count, value| match value {
                            Value::Int(value) => Ok(count + i64::from(*value != 0)),
                            Value::Float(value) => Ok(count + i64::from(*value != 0.0)),
                            _ => Err(RunnerError::internal(
                                span,
                                "sparse nonzero-count element is not numeric",
                            )),
                        })?;
                Ok(Value::Int(count))
            }
            MirCollectionOp::SparseDensify => {
                let [receiver] = args else {
                    return Err(RunnerError::internal(span, "sparse densify arity"));
                };
                let Value::Tensor(tensor) = self.eval_operand(receiver, span)? else {
                    return Err(RunnerError::internal(
                        span,
                        "sparse densify receiver type mismatch",
                    ));
                };
                // BO-M-8: layout-compatible densify path shares the Rc (current
                // densify is identity on the in-memory TensorValue representation).
                Ok(Value::Tensor(tensor))
            }
            MirCollectionOp::IntervalIntersect => self.interval_intersect(args, span),
            MirCollectionOp::IntervalUnion => self.interval_union(args, span),
            MirCollectionOp::IntervalLength => self.interval_length(args, span),
            MirCollectionOp::IntervalContains => self.interval_contains(args, span),
            MirCollectionOp::IntervalClamp => self.interval_clamp(args, span),
            MirCollectionOp::IntervalMaterialize => {
                self.interval_materialize(args, return_ty, span)
            }
            MirCollectionOp::IsEmpty => {
                let [receiver] = args else {
                    return Err(RunnerError::internal(span, "collection is_empty arity"));
                };
                Ok(Value::Bool(
                    self.eval_operand(receiver, span)?.is_empty_collection(),
                ))
            }
            MirCollectionOp::Index => {
                let [receiver, index] = args else {
                    return Err(RunnerError::internal(span, "collection index arity"));
                };
                let receiver = self.eval_operand(receiver, span)?;
                let index = self.eval_operand(index, span)?;
                if matches!(self.types.get(return_ty.semantic_id()), Type::Option(_)) {
                    self.optional_collection_index(receiver, index, span)
                } else {
                    self.collection_index(receiver, index, span)
                }
            }
            MirCollectionOp::Contains => {
                let [receiver, needle] = args else {
                    return Err(RunnerError::internal(span, "collection contains arity"));
                };
                let receiver = self.eval_operand(receiver, span)?;
                let needle = self.eval_operand(needle, span)?;
                Ok(Value::Bool(
                    self.collection_contains(receiver, needle, span)?,
                ))
            }
            MirCollectionOp::Append => {
                let [receiver_operand, item] = args else {
                    return Err(RunnerError::internal(span, "collection append arity"));
                };
                let receiver = self.eval_operand(receiver_operand, span)?;
                let item = self.eval_operand(item, span)?;
                match receiver {
                    Value::Array(cell) => {
                        if let Some(capacity) = self.collection_capacity(receiver_operand, span)?
                            && cell.borrow().len() >= capacity
                        {
                            return Err(RunnerError::internal_with_issue(
                                span,
                                "bounded collection push at capacity",
                                "runner_collection_push_capacity",
                            ));
                        }
                        cell.borrow_mut().push(item);
                        Ok(Value::Unit)
                    }
                    Value::Set(cell) => {
                        cell.borrow_mut().insert(item.to_key(span)?);
                        Ok(Value::Unit)
                    }
                    Value::Str(mut text) => {
                        if let Value::Str(add) = item {
                            text.push_str(&add);
                            Ok(Value::Str(text))
                        } else {
                            Err(RunnerError::internal(
                                span,
                                "append to textus requires textus",
                            ))
                        }
                    }
                    Value::Octeti(mut bytes) => {
                        bytes.push(byte_from_value(item, span)?);
                        // F-086: when the receiver is a Place, write back so
                        // the mutation persists in the frame; otherwise return
                        // the owned modified value (same pattern as Str).
                        if let MirOperand::Place(place) = receiver_operand {
                            self.write_place(place, Value::Octeti(bytes), span)?;
                            Ok(Value::Unit)
                        } else {
                            Ok(Value::Octeti(bytes))
                        }
                    }
                    _ => Err(RunnerError::internal(span, "append receiver type mismatch")),
                }
            }
            MirCollectionOp::AppendImmutable => {
                let [receiver, item] = args else {
                    return Err(RunnerError::internal(
                        span,
                        "collection append_immutable arity",
                    ));
                };
                let receiver = self.eval_operand(receiver, span)?;
                let item = self.eval_operand(item, span)?;
                match receiver {
                    Value::Array(cell) => {
                        let mut next = cell.borrow().clone();
                        next.push(item);
                        Ok(Value::Array(Rc::new(RefCell::new(next))))
                    }
                    Value::Str(text) => match item {
                        Value::Str(add) => Ok(Value::Str(format!("{text}{add}"))),
                        _ => Err(RunnerError::internal(
                            span,
                            "append_immutable textus requires textus",
                        )),
                    },
                    _ => Err(RunnerError::internal(
                        span,
                        "append_immutable receiver type mismatch",
                    )),
                }
            }
            MirCollectionOp::First | MirCollectionOp::Last => {
                let [receiver] = args else {
                    return Err(RunnerError::internal(span, "collection first/last arity"));
                };
                let receiver = self.eval_operand(receiver, span)?;
                match receiver {
                    Value::Array(cell) => {
                        let borrow = cell.borrow();
                        let item = match op {
                            MirCollectionOp::First => borrow.first(),
                            MirCollectionOp::Last => borrow.last(),
                            _ => None,
                        };
                        Ok(item.cloned().unwrap_or(Value::Nil))
                    }
                    Value::Str(text) => {
                        let item = match op {
                            MirCollectionOp::First => {
                                text.chars().next().map(|ch| Value::Str(ch.to_string()))
                            }
                            MirCollectionOp::Last => {
                                text.chars().last().map(|ch| Value::Str(ch.to_string()))
                            }
                            _ => None,
                        };
                        Ok(item.unwrap_or(Value::Nil))
                    }
                    _ => Err(RunnerError::internal(
                        span,
                        "first/last receiver type mismatch",
                    )),
                }
            }
            MirCollectionOp::RemoveFirst => {
                let [receiver] = args else {
                    return Err(RunnerError::internal(span, "collection remove_first arity"));
                };
                let receiver = self.eval_operand(receiver, span)?;
                match receiver {
                    Value::Array(cell) => {
                        let mut borrow = cell.borrow_mut();
                        if borrow.is_empty() {
                            Ok(Value::Nil)
                        } else {
                            Ok(borrow.remove(0))
                        }
                    }
                    _ => Err(RunnerError::internal(
                        span,
                        "remove_first receiver type mismatch",
                    )),
                }
            }
            MirCollectionOp::RemoveLast => {
                let [receiver] = args else {
                    return Err(RunnerError::internal(span, "collection remove_last arity"));
                };
                let receiver = self.eval_operand(receiver, span)?;
                match receiver {
                    Value::Array(cell) => Ok(cell.borrow_mut().pop().unwrap_or(Value::Nil)),
                    _ => Err(RunnerError::internal(
                        span,
                        "remove_last receiver type mismatch",
                    )),
                }
            }
            MirCollectionOp::Reverse => {
                let [receiver] = args else {
                    return Err(RunnerError::internal(span, "collection reverse arity"));
                };
                let receiver = self.eval_operand(receiver, span)?;
                match receiver {
                    Value::Array(cell) => {
                        let mut next = cell.borrow().clone();
                        next.reverse();
                        Ok(Value::Array(Rc::new(RefCell::new(next))))
                    }
                    Value::Str(text) => Ok(Value::Str(text.chars().rev().collect())),
                    _ => Err(RunnerError::internal(
                        span,
                        "reverse receiver type mismatch",
                    )),
                }
            }
            MirCollectionOp::ReverseInPlace => {
                let [receiver] = args else {
                    return Err(RunnerError::internal(
                        span,
                        "collection reverse_in_place arity",
                    ));
                };
                let receiver = self.eval_operand(receiver, span)?;
                match receiver {
                    Value::Array(cell) => {
                        cell.borrow_mut().reverse();
                        Ok(Value::Unit)
                    }
                    _ => Err(RunnerError::internal(
                        span,
                        "reverse_in_place receiver type mismatch",
                    )),
                }
            }
            MirCollectionOp::Sort => {
                let [receiver] = args else {
                    return Err(RunnerError::internal(span, "collection sort arity"));
                };
                let receiver = self.eval_operand(receiver, span)?;
                match receiver {
                    Value::Array(cell) => {
                        let mut next = cell.borrow().clone();
                        next.sort_by(super::value::compare_sort_values);
                        Ok(Value::Array(Rc::new(RefCell::new(next))))
                    }
                    _ => Err(RunnerError::internal(span, "sort receiver type mismatch")),
                }
            }
            MirCollectionOp::SortByKeys => {
                let [receiver, keys] = args else {
                    return Err(RunnerError::internal(span, "collection sort_by_keys arity"));
                };
                let (Value::Array(items), Value::Array(keys)) = (
                    self.eval_operand(receiver, span)?,
                    self.eval_operand(keys, span)?,
                ) else {
                    return Err(RunnerError::internal(
                        span,
                        "sort_by_keys operand type mismatch",
                    ));
                };
                let items = items.borrow();
                let keys = keys.borrow();
                if items.len() != keys.len() {
                    return Err(RunnerError::internal(span, "sort_by_keys length mismatch"));
                }
                // WHY: `sort_by` is stable, so equal keys keep source order (D1.7).
                let mut order: Vec<usize> = (0..items.len()).collect();
                order.sort_by(|&a, &b| super::value::compare_sort_values(&keys[a], &keys[b]));
                let sorted = order
                    .into_iter()
                    .map(|index| items[index].clone())
                    .collect();
                Ok(Value::Array(Rc::new(RefCell::new(sorted))))
            }
            MirCollectionOp::Uppercase => self.map_str_arg(args, span, |text| text.to_uppercase()),
            MirCollectionOp::Lowercase => self.map_str_arg(args, span, |text| text.to_lowercase()),
            MirCollectionOp::Trim => self.map_str_arg(args, span, |text| text.trim().to_owned()),
            MirCollectionOp::StartsWith => {
                self.str_pair_bool(args, span, |left, right| left.starts_with(&right))
            }
            MirCollectionOp::EndsWith => {
                self.str_pair_bool(args, span, |left, right| left.ends_with(&right))
            }
            MirCollectionOp::CordCursor => self.eval_cord_cursor(args, span),
            MirCollectionOp::RegexMatches => {
                let [receiver, text] = args else {
                    return Err(RunnerError::internal(span, "regex consentit arity"));
                };
                let Value::Regex(pattern) = self.eval_operand(receiver, span)? else {
                    return Err(RunnerError::internal(
                        span,
                        "consentit receiver is not regex",
                    ));
                };
                let Value::Str(text) = self.eval_operand(text, span)? else {
                    return Err(RunnerError::internal(
                        span,
                        "consentit argument is not textus",
                    ));
                };
                let regex = regex::Regex::new(&pattern).map_err(|err| {
                    RunnerError::internal(span, format!("regex compile failed: {err}"))
                })?;
                Ok(Value::Bool(regex.is_match(&text)))
            }
            MirCollectionOp::Split => {
                let [receiver, delim] = args else {
                    return Err(RunnerError::internal(span, "collection split arity"));
                };
                let Value::Str(text) = self.eval_operand(receiver, span)? else {
                    return Err(RunnerError::internal(span, "split receiver is not textus"));
                };
                let Value::Str(delim) = self.eval_operand(delim, span)? else {
                    return Err(RunnerError::internal(span, "split delimiter is not textus"));
                };
                let parts = text
                    .split(&delim)
                    .map(|part| Value::Str(part.to_owned()))
                    .collect::<Vec<_>>();
                Ok(Value::Array(Rc::new(RefCell::new(parts))))
            }
            MirCollectionOp::Replace => {
                let [receiver, from, to] = args else {
                    return Err(RunnerError::internal(span, "collection replace arity"));
                };
                let Value::Str(text) = self.eval_operand(receiver, span)? else {
                    return Err(RunnerError::internal(
                        span,
                        "replace receiver is not textus",
                    ));
                };
                let Value::Str(from) = self.eval_operand(from, span)? else {
                    return Err(RunnerError::internal(span, "replace from is not textus"));
                };
                let Value::Str(to) = self.eval_operand(to, span)? else {
                    return Err(RunnerError::internal(span, "replace to is not textus"));
                };
                Ok(Value::Str(text.replace(&from, &to)))
            }
            MirCollectionOp::Absolute => self.map_numeric(args, span, return_ty, f64::abs),
            MirCollectionOp::Signum => self.map_numeric(args, span, return_ty, f64::signum),
            MirCollectionOp::Minimum => self.numeric_extremum(
                args,
                span,
                return_ty,
                numeric_min,
                super::value::ieee_minimum,
            ),
            MirCollectionOp::Maximum => self.numeric_extremum(
                args,
                span,
                return_ty,
                numeric_max,
                super::value::ieee_maximum,
            ),
            MirCollectionOp::Approximate => self.approximate_fractus(args, span),
            MirCollectionOp::Sum => self.sum_numeric_array(args, return_ty, span),
            MirCollectionOp::ArgMax => self.arg_extremum_index(args, span, true),
            MirCollectionOp::ArgMin => self.arg_extremum_index(args, span, false),
            MirCollectionOp::ArrayAdd => {
                self.array_elementwise(args, span, "lista add", numeric_add, return_ty)
            }
            MirCollectionOp::ArraySub => {
                self.array_elementwise(args, span, "lista sub", numeric_sub, return_ty)
            }
            MirCollectionOp::ArrayMul => {
                self.array_elementwise(args, span, "lista mul", numeric_mul, return_ty)
            }
            MirCollectionOp::ArrayDiv => {
                self.array_elementwise(args, span, "lista div", numeric_div, return_ty)
            }
            MirCollectionOp::TensorSumAxis => self.tensor_sum_axis(args, span, return_ty),
            MirCollectionOp::TensorShape => self.tensor_shape(args, span),
            MirCollectionOp::TensorReshape => self.tensor_reshape(args, span),
            MirCollectionOp::TensorGet => self.tensor_get(args, return_ty, span),
            MirCollectionOp::TensorSet => self.tensor_set(args, span),
            MirCollectionOp::TensorFill => self.tensor_fill(args, span),
            MirCollectionOp::TensorCreate => self.tensor_create(args, span),
            MirCollectionOp::TensorFromFlat => self.tensor_from_flat(args, return_ty, span),
            MirCollectionOp::TensorFlatten => self.tensor_flatten(args, span),
            MirCollectionOp::TensorMaterialize => self.tensor_materialize(args, span),
            MirCollectionOp::TensorSlice => self.tensor_slice(args, span),
            MirCollectionOp::TensorAdd => {
                self.tensor_elementwise_or_scalar(args, span, "tensor add", numeric_add, return_ty)
            }
            MirCollectionOp::TensorSub => {
                self.tensor_elementwise_or_scalar(args, span, "tensor sub", numeric_sub, return_ty)
            }
            MirCollectionOp::TensorMul => {
                self.tensor_elementwise_or_scalar(args, span, "tensor mul", numeric_mul, return_ty)
            }
            MirCollectionOp::TensorMatMul => self.tensor_matmul(args, span, return_ty),
            MirCollectionOp::TensorMean => self.tensor_mean(args, span),
            MirCollectionOp::TensorMax => self.tensor_extremum(args, span, f64::max, "max"),
            MirCollectionOp::TensorMin => self.tensor_extremum(args, span, f64::min, "min"),
            MirCollectionOp::TensorLayerNorm => self.tensor_layernorm(args, span, return_ty),
            MirCollectionOp::TensorRmsNorm => self.tensor_rms_norm(args, span),
            MirCollectionOp::TensorRope => self.tensor_rope(args, span, false),
            MirCollectionOp::TensorRopeHalf => self.tensor_rope(args, span, true),
            MirCollectionOp::TensorGather => self.tensor_gather(args, span),
            // GI3-1: the causal masked-row softmax op is admitted on the
            // plan surface with NO runner body — the CPU-oracle semantics
            // (masked stable row softmax) are a recorded residual
            // (gi3-contract.md); executing it fails closed with the honest
            // unsupported diagnostic, never a silent pass.
            MirCollectionOp::TensorCausalMaskedSoftmax => {
                Err(RunnerError::unsupported(span, "causal masked softmax"))
            }
            // R-PACK-01 packed kernels have no CPU-runner body. The
            // interpreter fails closed with a named diagnostic — never a
            // silent skip or invented F32 expansion.
            MirCollectionOp::TensorQuantizedMatMul => {
                Err(RunnerError::unsupported(span, "quantized matmul"))
            }
            MirCollectionOp::TensorGroupedMatMul => {
                Err(RunnerError::unsupported(span, "grouped matmul"))
            }
            MirCollectionOp::TensorBatchedAttention => {
                Err(RunnerError::unsupported(span, "batched attention"))
            }
            MirCollectionOp::TensorSsmConv1d => Err(RunnerError::unsupported(span, "ssm conv1d")),
            MirCollectionOp::TensorSsmScan => Err(RunnerError::unsupported(span, "ssm scan")),
            MirCollectionOp::TensorDiv => {
                self.tensor_elementwise_or_scalar(args, span, "tensor div", numeric_div, return_ty)
            }
            MirCollectionOp::TensorNeg => self.tensor_neg(args, span),
            MirCollectionOp::TensorGelu => self.tensor_gelu(args, span),
            MirCollectionOp::TensorRelu => self.tensor_relu(args, span),
            MirCollectionOp::TensorSoftmax => self.tensor_softmax(args, span),
            MirCollectionOp::TensorCruxEntropia => self.tensor_crux_entropia(args, span),
            MirCollectionOp::TensorTranspose => self.tensor_transpose(args, span),
            MirCollectionOp::SparseGet => self.sparse_get(args, span),
            MirCollectionOp::SparseSet => self.sparse_set(args, span),
            MirCollectionOp::Round => self.map_numeric(args, span, return_ty, f64::round),
            MirCollectionOp::Floor => self.map_numeric(args, span, return_ty, f64::floor),
            MirCollectionOp::Ceiling => self.map_numeric(args, span, return_ty, f64::ceil),
            MirCollectionOp::Truncate => self.map_numeric(args, span, return_ty, f64::trunc),
            MirCollectionOp::Keys => self.eval_map_snapshot(args, span, false),
            MirCollectionOp::Values => self.eval_map_snapshot(args, span, true),
            MirCollectionOp::Put => {
                let [receiver, key, value] = args else {
                    return Err(RunnerError::internal(span, "collection put arity"));
                };
                let receiver = self.eval_operand(receiver, span)?;
                let key = self.eval_operand(key, span)?;
                let value = self.eval_operand(value, span)?;
                match receiver {
                    Value::Map(cell) => {
                        cell.borrow_mut().insert(key.to_key(span)?, value);
                        Ok(Value::Unit)
                    }
                    _ => Err(RunnerError::internal(span, "put receiver is not map")),
                }
            }
            MirCollectionOp::Delete => {
                let [receiver, key] = args else {
                    return Err(RunnerError::internal(span, "collection delete arity"));
                };
                let receiver = self.eval_operand(receiver, span)?;
                let key = self.eval_operand(key, span)?;
                match receiver {
                    Value::Map(cell) => {
                        let map_key = key.to_key(span)?;
                        Ok(Value::Bool(cell.borrow_mut().remove(&map_key).is_some()))
                    }
                    Value::Set(cell) => {
                        let set_key = key.to_key(span)?;
                        Ok(Value::Bool(cell.borrow_mut().remove(&set_key)))
                    }
                    _ => Err(RunnerError::internal(
                        span,
                        "delete receiver is not map or set",
                    )),
                }
            }
            MirCollectionOp::Slice => {
                let [receiver, start, end] = args else {
                    return Err(RunnerError::internal(span, "collection slice arity"));
                };
                let receiver = self.eval_operand(receiver, span)?;
                let start = self.eval_operand(start, span)?.as_index(span)?;
                let end = self.eval_operand(end, span)?.as_index(span)?;
                match receiver {
                    Value::Array(cell) => {
                        let borrow = cell.borrow();
                        let end = end.min(borrow.len());
                        let start = start.min(end);
                        Ok(array_value(borrow[start..end].to_vec()))
                    }
                    Value::Str(text) => {
                        let chars: Vec<char> = text.chars().collect();
                        let end = end.min(chars.len());
                        let start = start.min(end);
                        Ok(Value::Str(chars[start..end].iter().collect()))
                    }
                    _ => Err(RunnerError::internal(
                        span,
                        "slice receiver is not textus or array",
                    )),
                }
            }
            MirCollectionOp::Take | MirCollectionOp::TakeLast | MirCollectionOp::DropFirst => {
                let [receiver, count] = args else {
                    return Err(RunnerError::internal(span, "collection copy/view arity"));
                };
                let receiver = self.eval_operand(receiver, span)?;
                let count = self.eval_operand(count, span)?.as_index(span)?;
                match receiver {
                    Value::Array(cell) => {
                        let borrow = cell.borrow();
                        let len = borrow.len();
                        let (start, end) = match op {
                            MirCollectionOp::Take => (0, count.min(len)),
                            MirCollectionOp::DropFirst => (count.min(len), len),
                            MirCollectionOp::TakeLast => (len.saturating_sub(count), len),
                            _ => {
                                return Err(RunnerError::internal(
                                    span,
                                    "collection copy/view arity",
                                ));
                            }
                        };
                        Ok(array_value(borrow[start..end].to_vec()))
                    }
                    _ => Err(RunnerError::internal(
                        span,
                        "copy/view receiver is not array",
                    )),
                }
            }
            MirCollectionOp::Union
            | MirCollectionOp::Intersection
            | MirCollectionOp::Difference
            | MirCollectionOp::SymmetricDifference
            | MirCollectionOp::IsSubset
            | MirCollectionOp::IsSuperset => self.eval_set_algebra(op, args, span),
        }
    }

    // TENSOR, SPARSE, AND INTERVAL OPS

    fn tensor_shape(&mut self, args: &[MirOperand], span: Span) -> Result<Value, RunnerError> {
        let [receiver] = args else {
            return Err(RunnerError::internal(span, "tensor shape arity"));
        };
        let tensor = self.tensor_arg(receiver, span)?;
        let shape = tensor
            .borrow()
            .shape
            .iter()
            .copied()
            .map(Value::Int)
            .collect();
        Ok(array_value(shape))
    }

    fn tensor_reshape(&mut self, args: &[MirOperand], span: Span) -> Result<Value, RunnerError> {
        let [receiver, shape] = args else {
            return Err(RunnerError::internal(span, "tensor reshape arity"));
        };
        let tensor = self.tensor_arg(receiver, span)?;
        let shape = self.shape_arg(shape, span, ERR_FORMA_RESHAPE_COUNT)?;
        let data = tensor.borrow().data.clone();
        self.ensure_tensor_element_count(&shape, data.len(), span, ERR_FORMA_ELEMENT_COUNT)?;
        Ok(tensor_value(data, shape))
    }

    fn tensor_get(
        &mut self,
        args: &[MirOperand],
        return_ty: MirType,
        span: Span,
    ) -> Result<Value, RunnerError> {
        let [receiver, index] = args else {
            return Err(RunnerError::internal(span, "tensor get arity"));
        };
        let tensor = self.tensor_arg(receiver, span)?;
        let index = self.shape_arg(index, span, ERR_ACCIPE_INVALID_INDEX)?;
        let checked = !matches!(self.types.get(return_ty.semantic_id()), Type::Option(_));
        let borrow = tensor.borrow();
        let Some(offset) = tensor_flat_offset(&borrow.shape, &index) else {
            return if checked {
                Err(RunnerError::internal(span, ERR_ACCIPE_INVALID_INDEX))
            } else {
                Ok(Value::Nil)
            };
        };
        match borrow.data.get(offset).cloned() {
            Some(value) => Ok(value),
            None if checked => Err(RunnerError::internal(span, ERR_ACCIPE_INVALID_INDEX)),
            None => Ok(Value::Nil),
        }
    }

    fn tensor_set(&mut self, args: &[MirOperand], span: Span) -> Result<Value, RunnerError> {
        let [receiver, index, value] = args else {
            return Err(RunnerError::internal(span, "tensor set arity"));
        };
        self.write_tensor_element(receiver, index, value, span)
    }

    fn sparse_get(&mut self, args: &[MirOperand], span: Span) -> Result<Value, RunnerError> {
        let [receiver, index] = args else {
            return Err(RunnerError::internal(span, "sparse get arity"));
        };
        let tensor = self.tensor_arg(receiver, span)?;
        let index = self.shape_arg(index, span, ERR_ACCIPE_INVALID_INDEX)?;
        let borrow = tensor.borrow();
        let Some(offset) = tensor_flat_offset(&borrow.shape, &index) else {
            return Err(RunnerError::internal(span, ERR_ACCIPE_INVALID_INDEX));
        };
        borrow
            .data
            .get(offset)
            .cloned()
            .ok_or_else(|| RunnerError::internal(span, ERR_ACCIPE_INVALID_INDEX))
    }

    fn interval_intersect(
        &mut self,
        args: &[MirOperand],
        span: Span,
    ) -> Result<Value, RunnerError> {
        let [left, right] = args else {
            return Err(RunnerError::internal(span, "interval intersect arity"));
        };
        let left = self.eval_operand(left, span)?;
        let right = self.eval_operand(right, span)?;
        interval_intersection(&left, &right, span).map(|value| value.unwrap_or(Value::Nil))
    }

    fn interval_union(&mut self, args: &[MirOperand], span: Span) -> Result<Value, RunnerError> {
        let [left, right] = args else {
            return Err(RunnerError::internal(span, "interval union arity"));
        };
        let left = self.eval_operand(left, span)?;
        let right = self.eval_operand(right, span)?;
        interval_union_value(&left, &right, span).map(|value| value.unwrap_or(Value::Nil))
    }

    fn interval_length(&mut self, args: &[MirOperand], span: Span) -> Result<Value, RunnerError> {
        let [interval] = args else {
            return Err(RunnerError::internal(span, "interval length arity"));
        };
        let interval = self.eval_operand(interval, span)?;
        let (start, end, inclusive) = interval_parts(&interval, span)?;
        // Int-only bounds are not a hidden carrier gap: `intervallum` over an
        // unsigned-64 element type is rejected semantically, so interval
        // bounds can never surface as `UInt` here.
        let (Value::Int(start), Value::Int(end)) = (start, end) else {
            return Err(RunnerError::internal(
                span,
                "interval length requires numerus bounds",
            ));
        };
        Ok(Value::Int((end - start + i64::from(inclusive)).max(0)))
    }

    fn interval_contains(&mut self, args: &[MirOperand], span: Span) -> Result<Value, RunnerError> {
        let [interval, value] = args else {
            return Err(RunnerError::internal(span, "interval contains arity"));
        };
        let interval = self.eval_operand(interval, span)?;
        let value = self.eval_operand(value, span)?;
        let (start, end, inclusive) = interval_parts(&interval, span)?;
        let value = numeric_as_f64(&value, span)?;
        let start = numeric_as_f64(&start, span)?;
        let end = numeric_as_f64(&end, span)?;
        Ok(Value::Bool(
            value >= start && if inclusive { value <= end } else { value < end },
        ))
    }

    fn interval_clamp(&mut self, args: &[MirOperand], span: Span) -> Result<Value, RunnerError> {
        let [source, target] = args else {
            return Err(RunnerError::internal(span, "interval clamp arity"));
        };
        let source = self.eval_operand(source, span)?;
        let target = self.eval_operand(target, span)?;
        if matches!(source, Value::Tuple(_)) {
            return interval_intersection(&source, &target, span)?.ok_or_else(|| {
                RunnerError::internal(span, "interval clamp produced an empty interval")
            });
        }
        let (start, end, inclusive) = interval_parts(&target, span)?;
        match (source, start, end) {
            (Value::Int(value), Value::Int(start), Value::Int(end)) => {
                let upper = if inclusive {
                    end
                } else {
                    end.saturating_sub(1)
                };
                if upper < start {
                    // F-088: an empty exclusive interval (`[5, 5)` — monotonic
                    // bounds with `end == start`) has no valid clamped output.
                    // `value.clamp(start, upper)` with `min > max` panics, so
                    // return the source unchanged (identity on the empty set).
                    Ok(Value::Int(value))
                } else {
                    Ok(Value::Int(value.clamp(start, upper)))
                }
            }
            (Value::Float(value), Value::Float(start), Value::Float(end)) if inclusive => {
                Ok(Value::Float(value.clamp(start, end)))
            }
            _ => Err(RunnerError::internal(
                span,
                "unsupported interval clamp bound type",
            )),
        }
    }

    fn interval_materialize(
        &mut self,
        args: &[MirOperand],
        return_ty: MirType,
        span: Span,
    ) -> Result<Value, RunnerError> {
        let [interval] = args else {
            return Err(RunnerError::internal(span, "interval materialize arity"));
        };
        let interval = self.eval_operand(interval, span)?;
        let (Value::Int(start), Value::Int(end), inclusive) = interval_parts(&interval, span)?
        else {
            return Err(RunnerError::internal(
                span,
                "interval materialize requires numerus bounds",
            ));
        };
        // F-087: when end == i64::MAX and the interval is inclusive, the
        // exclusive stop `end.saturating_add(1)` saturates back to i64::MAX,
        // silently dropping the final element. Use checked_add to detect the
        // overflow and fall back to an inclusive range iteration.
        let values: Vec<Value> = if inclusive {
            if let Some(stop) = end.checked_add(1) {
                (start..stop).map(Value::Int).collect()
            } else {
                (start..=end).map(Value::Int).collect()
            }
        } else {
            (start..end).map(Value::Int).collect()
        };
        match self.types.get(return_ty.semantic_id()) {
            Type::Array(_) => Ok(array_value(values)),
            Type::Tensor(_, _) => {
                let len = i64::try_from(values.len()).map_err(|_| {
                    RunnerError::internal(span, "interval materialization length overflow")
                })?;
                Ok(tensor_value(values, vec![len]))
            }
            _ => Err(RunnerError::internal(
                span,
                "interval materialize result type mismatch",
            )),
        }
    }

    fn modular_word_width_of(&self, ty: MirType) -> Option<NumericWidth> {
        match self.types.get(ty.semantic_id()) {
            Type::ModularWord(width) => Some(*width),
            _ => None,
        }
    }

    fn sparse_set(&mut self, args: &[MirOperand], span: Span) -> Result<Value, RunnerError> {
        let [receiver, index, value] = args else {
            return Err(RunnerError::internal(span, "sparse set arity"));
        };
        self.write_tensor_element(receiver, index, value, span)
    }

    fn write_tensor_element(
        &mut self,
        receiver: &MirOperand,
        index: &MirOperand,
        value: &MirOperand,
        span: Span,
    ) -> Result<Value, RunnerError> {
        let tensor = self.tensor_arg(receiver, span)?;
        let index = self.shape_arg(index, span, ERR_PONDE_INVALID_INDEX)?;
        let value = self.eval_operand(value, span)?;
        let mut borrow = tensor.borrow_mut();
        let Some(offset) = tensor_flat_offset(&borrow.shape, &index) else {
            return Err(RunnerError::internal(span, ERR_PONDE_INVALID_INDEX));
        };
        let Some(slot) = borrow.data.get_mut(offset) else {
            return Err(RunnerError::internal(span, ERR_PONDE_INVALID_INDEX));
        };
        *slot = value;
        Ok(Value::Unit)
    }

    fn tensor_fill(&mut self, args: &[MirOperand], span: Span) -> Result<Value, RunnerError> {
        let [receiver, value] = args else {
            return Err(RunnerError::internal(span, "tensor fill arity"));
        };
        let tensor = self.tensor_arg(receiver, span)?;
        let value = self.eval_operand(value, span)?;
        let borrow = tensor.borrow();
        let shape = borrow.shape.clone();
        let count = borrow.data.len();
        // When the fill value is itself a tensor, broadcast/tile its elements
        // to fill the receiver shape. This supports the backward companion's
        // broadcasts: filling [B,D] with [B] (prefix repeat each element)
        // or with [D] (suffix cycle the block).
        match &value {
            Value::Tensor(fill_cell) => {
                let fill_borrow = fill_cell.borrow();
                let fill_count = fill_borrow.data.len();
                let fill_shape = &fill_borrow.shape;
                if fill_count == 0 || count % fill_count != 0 {
                    return Err(RunnerError::internal(
                        span,
                        "tensor fill: fill tensor element count must divide receiver element count",
                    ));
                }
                let repeats = count / fill_count;
                let data: Vec<Value> = if shape.starts_with(fill_shape) {
                    // Fill shape is a prefix (e.g. [B] → [B,D]):
                    // repeat each element to fill suffix dimensions.
                    fill_borrow
                        .data
                        .iter()
                        .flat_map(|v| std::iter::repeat_n(v, repeats))
                        .cloned()
                        .collect()
                } else {
                    // Fill shape is a suffix (e.g. [D] → [B,D])
                    // or neither prefix nor suffix: cycle the entire block.
                    fill_borrow
                        .data
                        .iter()
                        .cycle()
                        .take(count)
                        .cloned()
                        .collect()
                };
                Ok(tensor_value(data, shape))
            }
            _ => Ok(tensor_value(vec![value; count], shape)),
        }
    }

    fn tensor_create(&mut self, args: &[MirOperand], span: Span) -> Result<Value, RunnerError> {
        let [_receiver, fill, shape] = args else {
            return Err(RunnerError::internal(span, "tensor create arity"));
        };
        let shape = self.shape_arg(shape, span, ERR_CREA_INVALID_SHAPE)?;
        let fill = self.eval_operand(fill, span)?;
        let count = tensor_element_count(&shape, span)?;
        Ok(tensor_value(vec![fill; count], shape))
    }

    fn tensor_from_flat(
        &mut self,
        args: &[MirOperand],
        return_ty: MirType,
        span: Span,
    ) -> Result<Value, RunnerError> {
        // A cast lowered inside a generic template builds its shape operand
        // before instantiation, when the target dims are still size params
        // (the `InferOneD` fallback). The substituted static return type is
        // the shape authority (ABI source of truth): prefer its literal dims
        // whenever they resolve, and fall back to the lowered shape operand.
        let static_shape = radix_mir::lista_tensor_conversio::literal_tensor_dims(
            return_ty.semantic_id(),
            self.types,
        );
        let [_receiver, flat, shape, fill] = args else {
            // AEU-5 Route C: the padded form carries a trailing fill operand
            // for slots `len..N` of the bounded-lista snapshot.
            let [_receiver, flat, shape] = args else {
                return Err(RunnerError::internal(span, "tensor from flat arity"));
            };
            return self.tensor_from_flat_exact(flat, shape, static_shape, span);
        };
        let flat = self.array_arg(flat, span)?;
        let shape = self.shape_arg(
            shape,
            span,
            radix_mir::lista_tensor_conversio::LISTA_TO_TENSOR_STRUCTA_COUNT_MSG,
        )?;
        let shape = static_shape.unwrap_or(shape);
        let count = tensor_shape_element_count(&shape)
            .ok_or_else(|| RunnerError::internal(span, ERR_ELEMENT_COUNT_OVERFLOW))?;
        // The fill is a real caller value, never a read of the carrier's
        // uninitialized slots; a flat longer than the shape still fails.
        if flat.len() > count {
            return Err(RunnerError::internal(
                span,
                radix_mir::lista_tensor_conversio::LISTA_TO_TENSOR_STRUCTA_COUNT_MSG,
            ));
        }
        let fill = self.eval_operand(fill, span)?;
        let mut data = flat;
        data.resize(count, fill);
        Ok(tensor_value(data, shape))
    }

    fn tensor_from_flat_exact(
        &mut self,
        flat: &MirOperand,
        shape: &MirOperand,
        static_shape: Option<Vec<i64>>,
        span: Span,
    ) -> Result<Value, RunnerError> {
        let flat = self.array_arg(flat, span)?;
        let shape = self.shape_arg(
            shape,
            span,
            radix_mir::lista_tensor_conversio::LISTA_TO_TENSOR_STRUCTA_COUNT_MSG,
        )?;
        let shape = static_shape.unwrap_or(shape);
        self.ensure_tensor_element_count(
            &shape,
            flat.len(),
            span,
            radix_mir::lista_tensor_conversio::LISTA_TO_TENSOR_STRUCTA_COUNT_MSG,
        )?;
        Ok(tensor_value(flat, shape))
    }

    fn tensor_flatten(&mut self, args: &[MirOperand], span: Span) -> Result<Value, RunnerError> {
        let [receiver] = args else {
            return Err(RunnerError::internal(span, "tensor flatten arity"));
        };
        let tensor = self.tensor_arg(receiver, span)?;
        let data = tensor.borrow().data.clone();
        Ok(array_value(data))
    }

    fn tensor_materialize(
        &mut self,
        args: &[MirOperand],
        span: Span,
    ) -> Result<Value, RunnerError> {
        let [receiver] = args else {
            return Err(RunnerError::internal(span, "tensor materialize arity"));
        };
        let tensor = self.tensor_arg(receiver, span)?;
        let borrow = tensor.borrow();
        Ok(tensor_value(borrow.data.clone(), borrow.shape.clone()))
    }

    fn tensor_slice(&mut self, args: &[MirOperand], span: Span) -> Result<Value, RunnerError> {
        let [receiver, start, end] = args else {
            return Err(RunnerError::internal(span, "tensor slice arity"));
        };
        let tensor = self.tensor_arg(receiver, span)?;
        let start = self.eval_operand(start, span)?.as_index(span)?;
        let end = self.eval_operand(end, span)?.as_index(span)?;
        let borrow = tensor.borrow();
        let axis = borrow.shape.first().copied().unwrap_or(0).max(0) as usize;
        let end = end.min(axis);
        let start = start.min(end);
        let stride = borrow
            .shape
            .iter()
            .skip(1)
            .try_fold(1_usize, |acc, dim| {
                usize::try_from((*dim).max(0))
                    .ok()
                    .and_then(|d| acc.checked_mul(d))
            })
            .ok_or_else(|| RunnerError::internal(span, "tensor slice shape overflow"))?;
        let data_start = start.saturating_mul(stride);
        let data_end = end.saturating_mul(stride).min(borrow.data.len());
        let mut shape = borrow.shape.clone();
        if let Some(first) = shape.first_mut() {
            *first = (end - start) as i64;
        }
        Ok(tensor_value(
            borrow.data[data_start..data_end].to_vec(),
            shape,
        ))
    }

    /// Elementwise array-backed collection op with scalar broadcast. Unlike
    /// tensor broadcast, collection pairs have one legal shape: equal live
    /// runtime length. A mismatch is a hard internal runner error, matching
    /// the `ponde` strict-index family rather than a recoverable `⇥` path.
    fn array_elementwise(
        &mut self,
        args: &[MirOperand],
        span: Span,
        label: &'static str,
        op: fn(&Value, &Value, NumericWidth, Span) -> Result<Value, RunnerError>,
        return_ty: MirType,
    ) -> Result<Value, RunnerError> {
        let [left, right] = args else {
            return Err(RunnerError::internal(span, format!("{label} arity")));
        };
        let left = self.eval_operand(left, span)?;
        let right = self.eval_operand(right, span)?;
        let element_ty = match self.types.get(return_ty.semantic_id()) {
            Type::Array(element)
            | Type::BoundedArray { element, .. }
            | Type::Queue(element)
            | Type::BoundedQueue { element, .. }
            | Type::Stack(element)
            | Type::BoundedStack { element, .. } => *element,
            _ => {
                return Err(RunnerError::internal(
                    span,
                    format!("{label} result type mismatch"),
                ));
            }
        };
        let width = effective_width(self.types, element_ty).unwrap_or(NumericWidth::I64);
        match (&left, &right) {
            (Value::Array(left), Value::Array(right)) => {
                let left = left.borrow();
                let right = right.borrow();
                if left.len() != right.len() {
                    return Err(RunnerError::internal_with_issue(
                        span,
                        format!("{label} length mismatch"),
                        "runner_lista_elementwise_length_mismatch",
                    ));
                }
                let values = left
                    .iter()
                    .zip(right.iter())
                    .map(|(left, right)| op(left, right, width, span))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(array_value(values))
            }
            (Value::Array(left), scalar) => {
                let left = left.borrow();
                let values = left
                    .iter()
                    .map(|left| op(left, scalar, width, span))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(array_value(values))
            }
            (scalar, Value::Array(right)) => {
                let right = right.borrow();
                let values = right
                    .iter()
                    .map(|right| op(scalar, right, width, span))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(array_value(values))
            }
            _ => Err(RunnerError::internal(
                span,
                format!("{label} operand type mismatch"),
            )),
        }
    }

    /// Elementwise tensor op with scalar broadcast: if one arg is a scalar,
    /// broadcast it across all elements of the tensor arg.
    fn tensor_elementwise_or_scalar(
        &mut self,
        args: &[MirOperand],
        span: Span,
        label: &'static str,
        op: fn(&Value, &Value, NumericWidth, Span) -> Result<Value, RunnerError>,
        return_ty: MirType,
    ) -> Result<Value, RunnerError> {
        let [left, right] = args else {
            return Err(RunnerError::internal(span, format!("{label} arity")));
        };
        let left_val = self.eval_operand(left, span)?;
        let right_val = self.eval_operand(right, span)?;
        let width =
            effective_width(self.types, return_ty.semantic_id()).unwrap_or(NumericWidth::I64);
        match (&left_val, &right_val) {
            (Value::Tensor(l_cell), Value::Tensor(r_cell)) => {
                let l = l_cell.borrow();
                let r = r_cell.borrow();
                if l.shape != r.shape {
                    // S6-C2: the tensor-tensor elementwise family learns the
                    // bounded rank-extension broadcast (the `addita_bias`
                    // contract): the lower-rank operand is replicated across
                    // the leading axis of the higher-rank operand when its
                    // dims equal the higher operand's trailing dims. Any
                    // other differing pair fails closed with a typed
                    // diagnostic — the broadcast fact must be recorded, never
                    // guessed (A3).
                    let (higher_is_left, higher_shape, lower_shape, higher_data, lower_data) =
                        if l.shape.len() >= r.shape.len() {
                            (true, &l.shape, &r.shape, &l.data, &r.data)
                        } else {
                            (false, &r.shape, &l.shape, &r.data, &l.data)
                        };
                    if higher_shape.len() != lower_shape.len() + 1
                        || lower_shape != &higher_shape[1..]
                    {
                        return Err(RunnerError::internal_with_issue(
                            span,
                            format!(
                                "{label} tensor-tensor shape pair {:?} + {:?} is neither same-shape nor a rank-extension broadcast",
                                l.shape, r.shape
                            ),
                            "runner_tensor_tensor_broadcast_unsupported",
                        ));
                    }
                    let rows: usize = usize::try_from(higher_shape[0].max(0)).map_err(|_| {
                        RunnerError::internal(span, format!("{label} broadcast row overflow"))
                    })?;
                    // Stride over the lower-rank operand instead of cloning it
                    // `rows` times (perf-A F1). Operand order is preserved
                    // because Sub is not commutative.
                    if lower_data.is_empty() {
                        if !higher_data.is_empty() {
                            return Err(RunnerError::internal(
                                span,
                                format!("{label} backing-length mismatch"),
                            ));
                        }
                        return Ok(tensor_value(Vec::new(), higher_shape.clone()));
                    }
                    let expected = rows.saturating_mul(lower_data.len());
                    if higher_data.len() != expected {
                        return Err(RunnerError::internal(
                            span,
                            format!("{label} backing-length mismatch"),
                        ));
                    }
                    let data = (0..higher_data.len())
                        .map(|index| {
                            let lower = &lower_data[index % lower_data.len()];
                            if higher_is_left {
                                op(&higher_data[index], lower, width, span)
                            } else {
                                op(lower, &higher_data[index], width, span)
                            }
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    return Ok(tensor_value(data, higher_shape.clone()));
                }
                // Equal shapes with unequal backing lengths would silently
                // truncate via zip — reject explicitly (PRP-077).
                if l.data.len() != r.data.len() {
                    return Err(RunnerError::internal(
                        span,
                        format!("{label} backing-length mismatch"),
                    ));
                }
                let data = l
                    .data
                    .iter()
                    .zip(r.data.iter())
                    .map(|(a, b)| op(a, b, width, span))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(tensor_value(data, l.shape.clone()))
            }
            (Value::Tensor(cell), scalar) => {
                let tensor = cell.borrow();
                debug_assert!(!tensor.data.is_empty(), "{label} broadcast on empty tensor");
                let data = tensor
                    .data
                    .iter()
                    .map(|elem| op(elem, scalar, width, span))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(tensor_value(data, tensor.shape.clone()))
            }
            (scalar, Value::Tensor(cell)) => {
                let tensor = cell.borrow();
                debug_assert!(!tensor.data.is_empty(), "{label} broadcast on empty tensor");
                let data = tensor
                    .data
                    .iter()
                    .map(|elem| op(scalar, elem, width, span))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(tensor_value(data, tensor.shape.clone()))
            }
            _ => op(&left_val, &right_val, width, span),
        }
    }

    fn tensor_mean(&mut self, args: &[MirOperand], span: Span) -> Result<Value, RunnerError> {
        let [receiver] = args else {
            return Err(RunnerError::internal(span, "tensor mean arity"));
        };
        let tensor = self.tensor_arg(receiver, span)?;
        let borrow = tensor.borrow();
        if borrow.data.is_empty() {
            return Ok(Value::Float(0.0));
        }
        let mut sum = 0.0;
        for value in &borrow.data {
            sum += numeric_as_f64(value, span)?;
        }
        Ok(Value::Float(sum / borrow.data.len() as f64))
    }

    fn tensor_extremum(
        &mut self,
        args: &[MirOperand],
        span: Span,
        op: fn(f64, f64) -> f64,
        label: &str,
    ) -> Result<Value, RunnerError> {
        let Some(receiver) = args.first() else {
            return Err(RunnerError::internal(span, format!("tensor {label} arity")));
        };
        if args.len() > 2 {
            return Err(RunnerError::internal(span, format!("tensor {label} arity")));
        }
        let identity = if let Some(identity) = args.get(1) {
            Some(numeric_as_f64(&self.eval_operand(identity, span)?, span)?)
        } else {
            None
        };
        let tensor = self.tensor_arg(receiver, span)?;
        let borrow = tensor.borrow();
        let mut values = borrow.data.iter();
        let Some(first) = values.next() else {
            return Ok(match identity {
                Some(identity) => Value::Float(identity),
                None => Value::Nil,
            });
        };
        let mut result = numeric_as_f64(first, span)?;
        for value in values {
            result = op(result, numeric_as_f64(value, span)?);
        }
        Ok(Value::Float(result))
    }

    #[rustfmt::skip]
    pub(super)
    fn map_tensor_unary(
        &self,
        tensor: Rc<RefCell<TensorValue>>,
        op: &mut dyn FnMut(&Value) -> Result<Value, RunnerError>,
    ) -> Result<Value, RunnerError> {
        let borrow = tensor.borrow();
        let data = borrow
            .data
            .iter()
            .map(&mut *op)
            .collect::<Result<Vec<_>, _>>()?;
        Ok(tensor_value(data, borrow.shape.clone()))
    }

    fn tensor_neg(&mut self, args: &[MirOperand], span: Span) -> Result<Value, RunnerError> {
        let [receiver] = args else {
            return Err(RunnerError::internal(span, "tensor neg arity"));
        };
        let tensor = self.tensor_arg(receiver, span)?;
        self.map_tensor_unary(tensor, &mut |v| {
            Ok(match v {
                Value::Int(n) => Value::Int(-n),
                Value::Float(f) => Value::Float(-f),
                _ => {
                    return Err(RunnerError::internal(
                        span,
                        "tensor neg element type mismatch",
                    ));
                }
            })
        })
    }

    fn tensor_gelu(&mut self, args: &[MirOperand], span: Span) -> Result<Value, RunnerError> {
        let [receiver] = args else {
            return Err(RunnerError::internal(span, "tensor gelu arity"));
        };
        let tensor = self.tensor_arg(receiver, span)?;
        self.map_tensor_unary(tensor, &mut |v| match v {
            Value::Float(f) => Ok(Value::Float(gelu_f64(*f))),
            _ => Err(RunnerError::internal(
                span,
                "tensor gelu element type mismatch",
            )),
        })
    }

    fn tensor_relu(&mut self, args: &[MirOperand], span: Span) -> Result<Value, RunnerError> {
        let [receiver] = args else {
            return Err(RunnerError::internal(span, "tensor relu arity"));
        };
        let tensor = self.tensor_arg(receiver, span)?;
        self.map_tensor_unary(tensor, &mut |v| match v {
            Value::Float(f) => Ok(Value::Float(f.max(0.0))),
            Value::Int(i) => Ok(Value::Int((*i).max(0))),
            _ => Err(RunnerError::internal(
                span,
                "tensor relu element type mismatch",
            )),
        })
    }

    /// Softmax: `exp(x_i` - max(x)) / `sum(exp(x_j` - max(x))) with numerical
    /// stability. Operates on rank-1 (vector) or rank-2 (batched row-wise,
    /// axis 1 — the last axis). Uses inline `f64::exp()` from Rust stdlib.
    fn tensor_softmax(&mut self, args: &[MirOperand], span: Span) -> Result<Value, RunnerError> {
        let [receiver] = args else {
            return Err(RunnerError::internal(span, "tensor softmax arity"));
        };
        let tensor = self.tensor_arg(receiver, span)?;
        let borrow = tensor.borrow();
        let shape = &borrow.shape;

        if shape.is_empty() {
            return Err(RunnerError::internal(
                span,
                "tensor softmax requires non-empty tensor",
            ));
        }
        if shape.len() > 2 {
            return Err(RunnerError::internal(
                span,
                "tensor softmax requires rank-1 or rank-2 tensor",
            ));
        }
        if shape.contains(&0) {
            return Err(RunnerError::internal(
                span,
                "tensor softmax requires all dimensions to be positive",
            ));
        }

        let rank = shape.len();
        let last_dim = shape[rank - 1] as usize;
        let batch = borrow.data.len() / last_dim;

        let mut out_data = Vec::with_capacity(borrow.data.len());
        for b in 0..batch {
            let base = b * last_dim;
            let row = row_softmax(&borrow.data, base, last_dim, RowSoftmaxAccess::Safe, span)?;
            out_data.extend(row.into_iter().map(Value::Float));
        }
        Ok(tensor_value(out_data, shape.clone()))
    }

    /// Categorical cross-entropy loss: `-sum(targets * log(softmax(logits) + ε)) / N`.
    /// Loss is normalized by N = last dimension (number of classes), not batch size.
    /// Takes 2 args: logits (receiver) and targets. Returns scalar f32 loss.
    /// Domain constraints: finite inputs, targets in [0,1], matching shapes, rank 1-2.
    fn tensor_crux_entropia(
        &mut self,
        args: &[MirOperand],
        span: Span,
    ) -> Result<Value, RunnerError> {
        let [logits, targets] = args else {
            return Err(RunnerError::internal(span, "tensor crux_entropia arity"));
        };
        let logits = self.tensor_arg(logits, span)?;
        let targets = self.tensor_arg(targets, span)?;
        let logits_borrow = logits.borrow();
        let targets_borrow = targets.borrow();

        let shape = &logits_borrow.shape;
        if shape.is_empty() {
            return Err(RunnerError::internal(
                span,
                "tensor crux_entropia requires non-empty tensor",
            ));
        }
        if shape.len() > 2 {
            return Err(RunnerError::internal(
                span,
                "tensor crux_entropia requires rank-1 or rank-2 tensor",
            ));
        }
        if shape.contains(&0) {
            return Err(RunnerError::internal(
                span,
                "tensor crux_entropia requires all dimensions to be positive",
            ));
        }
        if logits_borrow.shape != targets_borrow.shape {
            return Err(RunnerError::internal(
                span,
                "tensor crux_entropia shape mismatch",
            ));
        }

        let rank = shape.len();
        let last_dim = shape[rank - 1] as usize;
        let batch = logits_borrow.data.len() / last_dim;
        let eps = 1e-7_f64;

        let mut total_loss = 0.0_f64;
        for b in 0..batch {
            let base = b * last_dim;
            let softmax = row_softmax(
                &logits_borrow.data,
                base,
                last_dim,
                RowSoftmaxAccess::Direct,
                span,
            )?;
            // Compute cross-entropy: -sum(targets * log(softmax + eps))
            let mut row_loss = 0.0_f64;
            for (i, softmax_val) in softmax.iter().enumerate() {
                let target = numeric_as_f64(&targets_borrow.data[base + i], span)?;
                row_loss -= target * (softmax_val + eps).ln();
            }
            total_loss += row_loss;
        }
        // Normalize by N = last_dim (class count), not by batch size.
        // For rank-1 shape [C]: N = C, batch = 1.
        // For rank-2 shape [B, C]: N = C, batch = B.
        Ok(Value::Float(total_loss / last_dim as f64))
    }

    /// Matrix multiply / GEMV over runner `Value` tensors.
    ///
    /// Validates rank-2 on the receiver and rank-1 or rank-2 on the argument,
    /// enforces the matching inner dimension, and performs the O(M·K·N)
    /// contraction. A rank-1 argument produces a rank-1 result. Integer
    /// overflow and element width mismatch are the author's responsibility
    /// (per v1 scope).
    fn tensor_matmul(
        &mut self,
        args: &[MirOperand],
        span: Span,
        return_ty: MirType,
    ) -> Result<Value, RunnerError> {
        let [left, right] = args else {
            return Err(RunnerError::internal(span, "tensor matmul arity"));
        };
        let width =
            effective_width(self.types, return_ty.semantic_id()).unwrap_or(NumericWidth::I64);
        let left = self.tensor_arg(left, span)?;
        let right = self.tensor_arg(right, span)?;
        let borrow = left.borrow();
        let borrow_r = right.borrow();
        let l_shape = &borrow.shape;
        let r_shape = &borrow_r.shape;
        if l_shape.len() != 2 {
            return Err(RunnerError::internal(span, ERR_MATMUL_RECEIVER_RANK));
        }
        let rhs_rank1 = r_shape.len() == 1;
        if !rhs_rank1 && r_shape.len() != 2 {
            return Err(RunnerError::internal(span, ERR_MATMUL_ARGUMENT_RANK));
        }
        let k_left = l_shape[1];
        let k_right = r_shape[0];
        if k_left != k_right {
            return Err(RunnerError::internal(span, ERR_MATMUL_INNER_DIMENSION));
        }
        let m = l_shape[0];
        let n = if rhs_rank1 { 1 } else { r_shape[1] };
        let output_shape = if rhs_rank1 { vec![m] } else { vec![m, n] };
        // Homogeneous numerus tensors take a typed ikj kernel: one classify
        // walk, then a tight numeric loop with row-major right-hand access.
        // Mixed / non-numeric tensors keep the borrowed Value fallback.
        if !borrow.data.is_empty() && !borrow_r.data.is_empty() {
            if let (Some(left_f), Some(right_f)) =
                (extract_f64s(&borrow.data), extract_f64s(&borrow_r.data))
            {
                let cells = matmul_f64(&left_f, &right_f, m, k_left, n);
                return Ok(tensor_value(
                    cells.into_iter().map(Value::Float).collect(),
                    output_shape.clone(),
                ));
            }
            if let (Some(left_i), Some(right_i)) =
                (extract_i64s(&borrow.data), extract_i64s(&borrow_r.data))
            {
                let cells = matmul_i64(&left_i, &right_i, m, k_left, n, width, span)?;
                return Ok(tensor_value(
                    cells.into_iter().map(Value::Int).collect(),
                    output_shape.clone(),
                ));
            }
            if let (Some(left_u), Some(right_u)) =
                (extract_u64s(&borrow.data), extract_u64s(&borrow_r.data))
            {
                let cells = matmul_u64(&left_u, &right_u, m, k_left, n, width, span)?;
                return Ok(tensor_value(
                    cells.into_iter().map(Value::UInt).collect(),
                    output_shape.clone(),
                ));
            }
        }
        Ok(tensor_value(
            matmul_values(&borrow.data, &borrow_r.data, m, k_left, n, width, span)?,
            output_shape,
        ))
    }

    /// Sum across a specified axis: `sum(x, axis)`.
    ///
    /// Backward-only `LayerNorm` VJP helper. Result rank is input rank - 1.
    /// v1: rank-2 only with axis 0 (sum rows) or axis = last (sum columns).
    ///
    /// Args layout: `[input, axis_const]`.
    fn tensor_sum_axis(
        &mut self,
        args: &[MirOperand],
        span: Span,
        return_ty: MirType,
    ) -> Result<Value, RunnerError> {
        if args.len() != 2 {
            return Err(RunnerError::internal(span, "tensor sum_axis arity"));
        }
        let width =
            effective_width(self.types, return_ty.semantic_id()).unwrap_or(NumericWidth::I64);
        let axis = match &args[1] {
            MirOperand::Constant(MirConstant::Int(i)) => *i,
            other => {
                let v = self.eval_operand(other, span)?;
                match v {
                    Value::Int(i) => i,
                    _ => {
                        return Err(RunnerError::internal(
                            span,
                            "tensor sum_axis axis must be int",
                        ));
                    }
                }
            }
        };
        let input = self.tensor_arg(&args[0], span)?;
        let borrow = input.borrow();
        let shape = &borrow.shape;
        if shape.is_empty() || shape.len() > 2 {
            return Err(RunnerError::internal(
                span,
                "tensor sum_axis requires rank 1 or 2",
            ));
        }
        let rank = shape.len();
        if axis < 0 || axis as usize >= rank {
            return Err(RunnerError::internal(
                span,
                "tensor sum_axis axis out of range",
            ));
        }
        if rank == 1 {
            // Sum over the only axis → scalar.
            let mut sum = Value::Float(0.0);
            let mut first = true;
            for item in &borrow.data {
                if first {
                    sum = item.clone();
                    first = false;
                } else {
                    sum = numeric_add(&sum, item, width, span)?;
                }
            }
            return Ok(sum);
        }
        // rank == 2
        let rows = shape[0] as usize;
        let cols = shape[1] as usize;
        if axis == 0 {
            // Sum across rows → result has shape [cols].
            let mut result = vec![Value::Float(0.0); cols];
            for (ci, slot) in result.iter_mut().enumerate() {
                let mut first = true;
                for ri in 0..rows {
                    let val = &borrow.data[ri * cols + ci];
                    if first {
                        *slot = val.clone();
                        first = false;
                    } else {
                        *slot = numeric_add(slot, val, width, span)?;
                    }
                }
            }
            Ok(tensor_value(result, vec![cols as i64]))
        } else {
            // axis == 1 (last axis) — sum across columns → result has shape [rows].
            let mut result = Vec::with_capacity(rows);
            for ri in 0..rows {
                let mut first = true;
                let mut sum = Value::Float(0.0);
                for ci in 0..cols {
                    let val = &borrow.data[ri * cols + ci];
                    if first {
                        sum = val.clone();
                        first = false;
                    } else {
                        sum = numeric_add(&sum, val, width, span)?;
                    }
                }
                result.push(sum);
            }
            Ok(tensor_value(result, vec![rows as i64]))
        }
    }

    /// Layer normalization with manual shape-loops.
    ///
    /// Computes `(x - mean) / sqrt(var + eps)` with optional affine
    /// transform `* gamma + beta`. Axis and epsilon are trailing const
    /// operands — `MirIntrinsic::Collection` is a bare discriminant with
    /// no payload fields.
    ///
    /// Args layout: `[input, axis_const, epsilon_const, gamma?, beta?]`.
    /// Arity: 3 (no affine), 4 (gamma only), or 5 (gamma + beta).
    /// Method call order: `input.laminatio(axis, eps, gamma?, beta?)`.
    fn tensor_layernorm(
        &mut self,
        args: &[MirOperand],
        span: Span,
        return_ty: MirType,
    ) -> Result<Value, RunnerError> {
        let n = args.len();
        if !(3..=5).contains(&n) {
            return Err(RunnerError::internal(span, "tensor layernorm arity"));
        }

        let width =
            effective_width(self.types, return_ty.semantic_id()).unwrap_or(NumericWidth::I64);

        // Extract axis and epsilon from args[1] and args[2],
        // resolving through `eval_operand` in case the literal was
        // assigned to a sized-fractus temp during MIR lowering.
        let axis = match self.eval_operand(&args[1], span)? {
            Value::Int(i) => i,
            other => {
                return Err(RunnerError::internal(
                    span,
                    format!("tensor layernorm axis must be int, got {other:?}"),
                ));
            }
        };
        let epsilon = match self.eval_operand(&args[2], span)? {
            Value::Float(f) => f,
            other => {
                return Err(RunnerError::internal(
                    span,
                    format!("tensor layernorm epsilon must be float, got {other:?}"),
                ));
            }
        };

        let input = self.tensor_arg(&args[0], span)?;
        let borrow = input.borrow();
        let shape = &borrow.shape;

        if shape.is_empty() || shape.len() > 2 {
            return Err(RunnerError::internal(
                span,
                "tensor layernorm requires rank 1 or 2",
            ));
        }

        let rank = shape.len();
        if axis < 0 || axis as usize >= rank {
            return Err(RunnerError::internal(
                span,
                "tensor layernorm axis out of range",
            ));
        }

        // v1 constraint: axis must be the last axis.
        if axis as usize != rank - 1 {
            return Err(RunnerError::internal(
                span,
                "tensor layernorm axis must be the last axis (v1)",
            ));
        }

        let total_len = borrow.data.len();
        if total_len == 0 {
            return Err(RunnerError::internal(
                span,
                "tensor layernorm requires non-empty tensor",
            ));
        }

        let norm_dim = shape[rank - 1] as usize;
        let batch = total_len / norm_dim;

        let (gamma, beta) = match n {
            4 => {
                let gamma = self.tensor_arg(&args[3], span)?;
                require_rank1_extent(
                    &gamma.borrow(),
                    norm_dim,
                    span,
                    "tensor layernorm gamma shape mismatch",
                )?;
                (Some(gamma), None)
            }
            5 => {
                let gamma = self.tensor_arg(&args[3], span)?;
                require_rank1_extent(
                    &gamma.borrow(),
                    norm_dim,
                    span,
                    "tensor layernorm gamma shape mismatch",
                )?;
                let beta = self.tensor_arg(&args[4], span)?;
                require_rank1_extent(
                    &beta.borrow(),
                    norm_dim,
                    span,
                    "tensor layernorm beta shape mismatch",
                )?;
                (Some(gamma), Some(beta))
            }
            _ => (None, None),
        };

        // --- Compute mean over axis ---
        let mut means = vec![0.0f64; batch];
        for (row, mean_slot) in means.iter_mut().enumerate() {
            let mut sum = 0.0f64;
            let base = row * norm_dim;
            for c in 0..norm_dim {
                sum += numeric_as_f64(&borrow.data[base + c], span)?;
            }
            *mean_slot = sum / norm_dim as f64;
        }

        // --- Compute centered = input - mean (broadcast) ---
        let mut x_centered = Vec::with_capacity(total_len);
        for (row, mean) in means.iter().enumerate() {
            let base = row * norm_dim;
            for c in 0..norm_dim {
                let val = numeric_as_f64(&borrow.data[base + c], span)?;
                let centered = val - mean;
                x_centered.push(Value::Float(centered));
            }
        }

        // --- Compute variance = mean(centered²) over axis ---
        let mut inv_stds = vec![0.0f64; batch];
        for (row, inv_std_slot) in inv_stds.iter_mut().enumerate() {
            let mut sum_sq = 0.0f64;
            let base = row * norm_dim;
            for c in 0..norm_dim {
                let centered = x_centered[base + c].clone();
                let val = numeric_as_f64(&centered, span)?;
                sum_sq += val * val;
            }
            let var = sum_sq / norm_dim as f64;
            *inv_std_slot = 1.0 / (var + epsilon).sqrt();
        }

        // --- Compute normalized = centered * inv_std (broadcast) ---
        let mut normalized = Vec::with_capacity(total_len);
        for (row, istd) in inv_stds.iter().enumerate() {
            let base = row * norm_dim;
            for c in 0..norm_dim {
                let centered = &x_centered[base + c];
                let val = numeric_as_f64(centered, span)?;
                normalized.push(Value::Float(val * istd));
            }
        }

        // --- Apply affine transform if present ---
        let output: Vec<Value> = match (gamma, beta) {
            (Some(g), Some(b)) => {
                let g_borrow = g.borrow();
                let b_borrow = b.borrow();
                let mut out = Vec::with_capacity(total_len);
                for row in 0..batch {
                    let base = row * norm_dim;
                    for c in 0..norm_dim {
                        let norm = &normalized[base + c];
                        let g_val = &g_borrow.data[c];
                        let b_val = &b_borrow.data[c];
                        // norm * gamma + beta
                        let prod = numeric_mul(norm, g_val, width, span)?;
                        let sum = numeric_add(&prod, b_val, width, span)?;
                        out.push(sum);
                    }
                }
                out
            }
            (Some(g), None) => {
                let g_borrow = g.borrow();
                let mut out = Vec::with_capacity(total_len);
                for row in 0..batch {
                    let base = row * norm_dim;
                    for c in 0..norm_dim {
                        let norm = &normalized[base + c];
                        let g_val = &g_borrow.data[c];
                        out.push(numeric_mul(norm, g_val, width, span)?);
                    }
                }
                out
            }
            _ => normalized,
        };

        Ok(tensor_value(output, shape.clone()))
    }

    fn tensor_rms_norm(&mut self, args: &[MirOperand], span: Span) -> Result<Value, RunnerError> {
        // MIR layout: [x, axis, eps, gamma].
        let [input, axis_arg, eps_arg, gamma_arg] = args else {
            return Err(RunnerError::internal(span, "tensor rms_norm arity"));
        };

        let axis = match self.eval_operand(axis_arg, span)? {
            Value::Int(i) => i,
            other => {
                return Err(RunnerError::internal(
                    span,
                    format!("tensor rms_norm axis must be int, got {other:?}"),
                ));
            }
        };
        let epsilon = match self.eval_operand(eps_arg, span)? {
            Value::Float(f) => f,
            other => {
                return Err(RunnerError::internal(
                    span,
                    format!("tensor rms_norm epsilon must be float, got {other:?}"),
                ));
            }
        };

        let input = self.tensor_arg(input, span)?;
        let borrow = input.borrow();
        let shape = &borrow.shape;

        if shape.is_empty() || shape.len() > 2 {
            return Err(RunnerError::internal(
                span,
                "tensor rms_norm requires rank 1 or 2",
            ));
        }
        let rank = shape.len();
        if axis < 0 || usize::try_from(axis).map_or(true, |a| a >= rank) {
            return Err(RunnerError::internal(
                span,
                "tensor rms_norm axis out of range",
            ));
        }
        if usize::try_from(axis).map_or(true, |a| a != rank - 1) {
            return Err(RunnerError::internal(
                span,
                "tensor rms_norm axis must be the last axis (v1)",
            ));
        }

        let total_len = borrow.data.len();
        if total_len == 0 {
            return Err(RunnerError::internal(
                span,
                "tensor rms_norm requires non-empty tensor",
            ));
        }
        let norm_dim = shape[rank - 1] as usize;
        let batch = total_len / norm_dim;

        // Gamma is the mandatory affine peer: rank-1, width == axis extent.
        let gamma = self.tensor_arg(gamma_arg, span)?;
        require_rank1_extent(
            &gamma.borrow(),
            norm_dim,
            span,
            "tensor rms_norm gamma shape mismatch",
        )?;
        let g_borrow = gamma.borrow();

        // rms_norm: y = x·(1/sqrt(mean(x²)+eps))·gamma, no mean subtraction.
        // f64 sum of squares → f32 mean → f32 scale (the in-repo
        // `rms_norm_reference` operation order).
        let mut output = Vec::with_capacity(total_len);
        for row in 0..batch {
            let base = row * norm_dim;
            let mut sum_sq = 0.0f64;
            for c in 0..norm_dim {
                let val = numeric_as_f64(&borrow.data[base + c], span)?;
                sum_sq += val * val;
            }
            let mean = (sum_sq / norm_dim as f64) as f32;
            let eps_f32 = epsilon as f32;
            let scale = f64::from(1.0f32 / (mean + eps_f32).sqrt());
            for c in 0..norm_dim {
                let x = numeric_as_f64(&borrow.data[base + c], span)?;
                let g = numeric_as_f64(&g_borrow.data[c], span)?;
                output.push(Value::Float(x * scale * g));
            }
        }
        Ok(tensor_value(output, shape.clone()))
    }

    fn tensor_rope(
        &mut self,
        args: &[MirOperand],
        span: Span,
        rotate_half: bool,
    ) -> Result<Value, RunnerError> {
        // MIR layout: [x, pos, dim, rope_table]. Shared by TensorRope and
        // TensorRopeHalf. The table is the sole angle source; the runner
        // mirrors the device interleaved `[rows, dim/2, 3]` contract.
        let [input, pos_arg, dim_arg, table_arg] = args else {
            return Err(RunnerError::internal(span, "tensor rope arity"));
        };
        let _pos = match self.eval_operand(pos_arg, span)? {
            Value::Int(i) => i,
            other => {
                return Err(RunnerError::internal(
                    span,
                    format!("tensor rope pos must be int, got {other:?}"),
                ));
            }
        };
        let dim = match self.eval_operand(dim_arg, span)? {
            Value::Int(i) => i,
            other => {
                return Err(RunnerError::internal(
                    span,
                    format!("tensor rope dim must be int, got {other:?}"),
                ));
            }
        };

        let input = self.tensor_arg(input, span)?;
        let borrow = input.borrow();
        let shape = &borrow.shape;
        if shape.is_empty() || shape.len() > 2 {
            return Err(RunnerError::internal(
                span,
                "tensor rope requires rank 1 or 2",
            ));
        }
        let last_extent = shape[shape.len() - 1] as usize;
        if dim <= 0 || dim % 2 != 0 || usize::try_from(dim).map_or(true, |d| d > last_extent) {
            return Err(RunnerError::internal(
                span,
                format!(
                    "tensor rope dim {dim} must be even and within the last-axis extent {last_extent}"
                ),
            ));
        }
        let batch = borrow.data.len() / last_extent;
        if batch == 0 {
            return Err(RunnerError::internal(
                span,
                "tensor rope requires non-empty tensor",
            ));
        }

        let table = self.tensor_arg(table_arg, span)?;
        let table_borrow = table.borrow();
        let expected_table_shape = vec![batch as i64, (dim / 2), 3];
        if table_borrow.shape != expected_table_shape {
            return Err(RunnerError::internal(
                span,
                format!(
                    "tensor rope table shape {:?} must be {:?}",
                    table_borrow.shape, expected_table_shape
                ),
            ));
        }

        // llama-arch NORM consecutive-pair, or Qwen/NeoX rotate_half. The
        // table stores [angle, cos(angle), sin(angle)] for each row/pair.
        let half = (dim / 2) as usize;
        let mut output = Vec::with_capacity(borrow.data.len());
        for row in 0..batch {
            let base = row * last_extent;
            let mut row_data: Vec<Value> = borrow.data[base..base + last_extent].to_vec();
            for k in 0..half {
                let table_base = (row * half + k) * 3;
                let cos = numeric_as_f64(&table_borrow.data[table_base + 1], span)?;
                let sin = numeric_as_f64(&table_borrow.data[table_base + 2], span)?;
                let (i0, i1) = if rotate_half {
                    (k, k + half)
                } else {
                    (2 * k, 2 * k + 1)
                };
                let x0 = numeric_as_f64(&row_data[i0], span)?;
                let x1 = numeric_as_f64(&row_data[i1], span)?;
                row_data[i0] = Value::Float(x0 * cos - x1 * sin);
                row_data[i1] = Value::Float(x0 * sin + x1 * cos);
            }
            output.extend(row_data);
        }
        Ok(tensor_value(output, shape.clone()))
    }

    fn tensor_gather(&mut self, args: &[MirOperand], span: Span) -> Result<Value, RunnerError> {
        // MIR layout: [table, ids].
        let [table_arg, ids_arg] = args else {
            return Err(RunnerError::internal(span, "tensor gather arity"));
        };
        let table = self.tensor_arg(table_arg, span)?;
        let borrow = table.borrow();
        let shape = &borrow.shape;
        if shape.len() < 2 {
            return Err(RunnerError::internal(
                span,
                "tensor gather requires a rank-2 table receiver",
            ));
        }
        let rows = shape[0];
        // Tensor dims are non-negative i64; clamp via try_from if ever negative.
        let cols = usize::try_from(shape[1]).unwrap_or(0);
        let ids = self.array_arg(ids_arg, span)?;
        let mut indices = Vec::with_capacity(ids.len());
        for value in &ids {
            let Value::Int(id) = value else {
                return Err(RunnerError::internal(
                    span,
                    format!("tensor gather id is not numerus: {value:?}"),
                ));
            };
            if *id < 0 || *id >= rows {
                return Err(RunnerError::internal(
                    span,
                    format!("tensor gather id {id} is out of range for table rows {rows}"),
                ));
            }
            let index = usize::try_from(*id).map_err(|_| {
                RunnerError::internal(
                    span,
                    format!("tensor gather id {id} is out of range for table rows {rows}"),
                )
            })?;
            indices.push(index);
        }
        // out[t] = table[ids[t]] → [T, D]; repeated/out-of-order ids preserved.
        let mut output = Vec::with_capacity(indices.len() * cols);
        for index in &indices {
            let base = index * cols;
            output.extend(borrow.data[base..base + cols].iter().cloned());
        }
        let result_shape = vec![indices.len() as i64, cols as i64];
        Ok(tensor_value(output, result_shape))
    }

    fn tensor_transpose(&mut self, args: &[MirOperand], span: Span) -> Result<Value, RunnerError> {
        let [tensor] = args else {
            return Err(RunnerError::internal(span, "tensor transpose arity"));
        };
        let tensor = self.tensor_arg(tensor, span)?;
        let borrow = tensor.borrow();
        let shape = &borrow.shape;
        if shape.len() != 2 {
            return Err(RunnerError::internal(
                span,
                "tensor transpose requires rank 2",
            ));
        }
        let m = shape[0];
        let n = shape[1];
        let mut result_data = Vec::with_capacity((m * n) as usize);
        for j in 0..n {
            for i in 0..m {
                result_data.push(borrow.data[(i * n + j) as usize].clone());
            }
        }
        Ok(tensor_value(result_data, vec![n, m]))
    }

    // COLLECTION HELPERS

    /// Read a literal capacity from a direct collection place. MIR values do
    /// not carry their semantic type, so bounded queue/stack/list push uses
    /// the active function's place declaration as the capacity witness.
    fn collection_capacity(
        &self,
        operand: &MirOperand,
        span: Span,
    ) -> Result<Option<usize>, RunnerError> {
        let MirOperand::Place(place) = operand else {
            return Ok(None);
        };
        if !place.projections.is_empty() {
            return Ok(None);
        }
        let Some(frame) = self.frames.last() else {
            return Ok(None);
        };
        let function = self.function_by_id(frame.function, span)?;
        let ty = match &place.base {
            MirPlaceBase::Local(id) => function
                .locals
                .iter()
                .find(|local| local.id == *id)
                .map(|local| local.ty)
                .or_else(|| {
                    function
                        .params
                        .iter()
                        .find(|param| param.local == *id)
                        .map(|param| param.ty)
                }),
            MirPlaceBase::Temp(id) => function
                .temps
                .iter()
                .find(|temp| temp.id == *id)
                .map(|temp| temp.ty),
        };
        let Some(ty) = ty else {
            return Ok(None);
        };
        let capacity = match self.types.get(ty.semantic_id()) {
            Type::BoundedArray { capacity, .. }
            | Type::BoundedQueue { capacity, .. }
            | Type::BoundedStack { capacity, .. } => *capacity,
            _ => return Ok(None),
        };
        let IndexExpr::Literal(capacity) = self.types.get_index(capacity) else {
            return Ok(None);
        };
        usize::try_from(*capacity).map(Some).map_err(|_| {
            RunnerError::internal(span, "bounded collection capacity does not fit runner")
        })
    }

    fn eval_map_snapshot(
        &mut self,
        args: &[MirOperand],
        span: Span,
        values: bool,
    ) -> Result<Value, RunnerError> {
        let label = if values { "values" } else { "keys" };
        let [receiver] = args else {
            return Err(RunnerError::internal(
                span,
                format!("collection {label} arity"),
            ));
        };
        let receiver = self.eval_operand(receiver, span)?;
        match receiver {
            Value::Map(cell) => {
                let items = if values {
                    cell.borrow().values().cloned().collect::<Vec<_>>()
                } else {
                    cell.borrow()
                        .keys()
                        .map(Self::key_to_value)
                        .collect::<Vec<_>>()
                };
                Ok(Value::Array(Rc::new(RefCell::new(items))))
            }
            _ => Err(RunnerError::internal(
                span,
                format!("{label} receiver is not map"),
            )),
        }
    }

    fn collection_index(
        &self,
        receiver: Value,
        index: Value,
        span: Span,
    ) -> Result<Value, RunnerError> {
        match receiver {
            Value::Cursor(cursor) => cursor.index(index.as_index(span)?, span),
            Value::Array(cell) => {
                // A semantically octeti local can retain the array carrier
                // after a lista<u8> assignment; normalize its window back to
                // octeti so endian conversion sees the declared source type.
                if let Ok((start, end, inclusive)) = interval_parts(&index, span) {
                    let start = start.as_index(span)?;
                    let end = end.as_index(span)?;
                    let end = if inclusive {
                        end.checked_add(1).ok_or_else(|| {
                            RunnerError::internal(span, "octeti index out of bounds")
                        })?
                    } else {
                        end
                    };
                    let bytes = cell
                        .borrow()
                        .iter()
                        .map(|item| byte_from_value(item.clone(), span))
                        .collect::<Result<Vec<_>, _>>()?;
                    if start > end || end > bytes.len() {
                        return Err(RunnerError::internal(span, "octeti index out of bounds"));
                    }
                    return Ok(Value::Octeti(bytes[start..end].to_vec()));
                }
                let idx = index.as_index(span)?;
                cell.borrow()
                    .get(idx)
                    .cloned()
                    .ok_or_else(|| RunnerError::internal(span, "array index out of bounds"))
            }
            Value::Tuple(elems) => {
                let idx = index.as_index(span)?;
                elems
                    .get(idx)
                    .cloned()
                    .ok_or_else(|| RunnerError::internal(span, "tuple index out of bounds"))
            }
            Value::Str(text) => {
                // A range index is a char-window read (`textus[lo‥hi]` stays a
                // textus substring). Mirrors the octeti window above, but walks
                // chars and truncates past the end like Rust codegen's
                // `chars().skip(lo).take(hi - lo)` — no out-of-bounds trap.
                if let Ok((start, end, inclusive)) = interval_parts(&index, span) {
                    let start = start.as_index(span)?;
                    let end = end.as_index(span)?;
                    let end = if inclusive {
                        end.checked_add(1).ok_or_else(|| {
                            RunnerError::internal(span, "textus index out of bounds")
                        })?
                    } else {
                        end
                    };
                    let window: String = text
                        .chars()
                        .skip(start)
                        .take(end.saturating_sub(start))
                        .collect();
                    return Ok(Value::Str(window));
                }
                let idx = index.as_index(span)?;
                crate::value::scalar_text_index(&text, idx, span)
            }
            Value::Ascii(byte) => {
                let idx = index.as_index(span)?;
                if idx == 0 {
                    Ok(Value::Ascii(byte))
                } else {
                    Err(RunnerError::internal(span, "text index out of bounds"))
                }
            }
            Value::Octeti(bytes) => {
                if let Ok((start, end, inclusive)) = interval_parts(&index, span) {
                    let start = start.as_index(span)?;
                    let end = end.as_index(span)?;
                    let end = if inclusive {
                        end.checked_add(1).ok_or_else(|| {
                            RunnerError::internal(span, "octeti index out of bounds")
                        })?
                    } else {
                        end
                    };
                    if start > end || end > bytes.len() {
                        return Err(RunnerError::internal(span, "octeti index out of bounds"));
                    }
                    return Ok(Value::Octeti(bytes[start..end].to_vec()));
                }
                let idx = index.as_index(span)?;
                bytes
                    .get(idx)
                    .map(|byte| Value::Int(i64::from(*byte)))
                    .ok_or_else(|| RunnerError::internal(span, "octeti index out of bounds"))
            }
            Value::Map(cell) => {
                let key = index.to_key(span)?;
                Ok(cell.borrow().get(&key).cloned().unwrap_or(Value::Nil))
            }
            // Scalar flat-lane index over a tensor (itera ex element gather);
            // tensors define every lane by construction, so bounds are the
            // only failure mode.
            Value::Tensor(tensor) => {
                let idx = index.as_index(span)?;
                tensor
                    .borrow()
                    .data
                    .get(idx)
                    .cloned()
                    .ok_or_else(|| RunnerError::internal(span, "tensor index out of bounds"))
            }
            _ => Err(RunnerError::internal(span, "index receiver type mismatch")),
        }
    }

    fn optional_collection_index(
        &self,
        receiver: Value,
        index: Value,
        span: Span,
    ) -> Result<Value, RunnerError> {
        let optional_index = match &index {
            Value::Int(value) if *value >= 0 => Some(*value as usize),
            Value::Int(_) => None,
            _ => return self.collection_index(receiver, index, span),
        };
        match receiver {
            Value::Array(cell) => Ok(optional_index
                .and_then(|idx| cell.borrow().get(idx).cloned())
                .unwrap_or(Value::Nil)),
            Value::Str(text) => Ok(optional_index
                .and_then(|idx| crate::value::scalar_text_index(&text, idx, span).ok())
                .unwrap_or(Value::Nil)),
            Value::Ascii(byte) => Ok(optional_index
                .and_then(|idx| (idx == 0).then_some(Value::Ascii(byte)))
                .unwrap_or(Value::Nil)),
            Value::Octeti(bytes) => Ok(optional_index
                .and_then(|idx| bytes.get(idx).copied())
                .map(|byte| Value::Int(i64::from(byte)))
                .unwrap_or(Value::Nil)),
            Value::Map(cell) => Ok(cell
                .borrow()
                .get(&index.to_key(span)?)
                .cloned()
                .unwrap_or(Value::Nil)),
            other => self.collection_index(other, index, span),
        }
    }

    fn tensor_arg(
        &mut self,
        operand: &MirOperand,
        span: Span,
    ) -> Result<Rc<RefCell<TensorValue>>, RunnerError> {
        match self.eval_operand(operand, span)? {
            Value::Tensor(cell) => Ok(cell),
            other => Err(RunnerError::internal(
                span,
                format!("tensor receiver type mismatch: {other:?}"),
            )),
        }
    }

    fn array_arg(&mut self, operand: &MirOperand, span: Span) -> Result<Vec<Value>, RunnerError> {
        match self.eval_operand(operand, span)? {
            Value::Array(cell) => Ok(cell.borrow().clone()),
            other => Err(RunnerError::internal(
                span,
                format!("tensor lista argument type mismatch: {other:?}"),
            )),
        }
    }

    fn shape_arg(
        &mut self,
        operand: &MirOperand,
        span: Span,
        negative_message: &'static str,
    ) -> Result<Vec<i64>, RunnerError> {
        self.array_arg(operand, span)?
            .into_iter()
            .map(|value| match value {
                Value::Int(dim) if tensor_dim_non_negative(dim) => Ok(dim),
                Value::Int(_) => Err(RunnerError::internal(span, negative_message)),
                other => Err(RunnerError::internal(
                    span,
                    format!("tensor shape element is not numerus: {other:?}"),
                )),
            })
            .collect()
    }

    fn ensure_tensor_element_count(
        &self,
        shape: &[i64],
        actual: usize,
        span: Span,
        message: &'static str,
    ) -> Result<(), RunnerError> {
        match tensor_shape_element_count(shape) {
            Some(expected) if expected == actual => Ok(()),
            Some(_) => Err(RunnerError::internal(span, message)),
            None => Err(RunnerError::internal(span, ERR_ELEMENT_COUNT_OVERFLOW)),
        }
    }

    fn collection_contains(
        &self,
        receiver: Value,
        needle: Value,
        span: Span,
    ) -> Result<bool, RunnerError> {
        match receiver {
            Value::Array(cell) => Ok(cell.borrow().iter().any(|item| values_equal(item, &needle))),
            Value::Set(cell) => Ok(cell.borrow().contains(&needle.to_key(span)?)),
            Value::Map(cell) => Ok(cell.borrow().contains_key(&needle.to_key(span)?)),
            Value::Str(text) => {
                let Value::Str(needle) = needle else {
                    return Err(RunnerError::internal(span, "contains needle is not textus"));
                };
                Ok(text.contains(&needle))
            }
            _ => Err(RunnerError::internal(
                span,
                "contains receiver type mismatch",
            )),
        }
    }

    fn eval_cord_cursor(&mut self, args: &[MirOperand], span: Span) -> Result<Value, RunnerError> {
        let [receiver] = args else {
            return Err(RunnerError::internal(span, "cord cursor arity"));
        };
        let value = self.eval_operand(receiver, span)?;
        let elems = match value {
            // D10.1: one `littera` per Unicode scalar; no non-ASCII trap.
            Value::Str(text) => text.chars().map(Value::Char).collect(),
            Value::Ascii(byte) => vec![Value::Char(char::from(byte))],
            _ => {
                return Err(RunnerError::internal(
                    span,
                    "cord cursor receiver type mismatch",
                ));
            }
        };
        Ok(Value::Array(Rc::new(RefCell::new(elems))))
    }

    fn map_str_arg(
        &mut self,
        args: &[MirOperand],
        span: Span,
        f: impl FnOnce(String) -> String,
    ) -> Result<Value, RunnerError> {
        let [receiver] = args else {
            return Err(RunnerError::internal(span, "text collection arity"));
        };
        let Value::Str(text) = self.eval_operand(receiver, span)? else {
            return Err(RunnerError::internal(span, "text receiver is not textus"));
        };
        Ok(Value::Str(f(text)))
    }

    fn str_pair_bool(
        &mut self,
        args: &[MirOperand],
        span: Span,
        f: impl FnOnce(String, String) -> bool,
    ) -> Result<Value, RunnerError> {
        let [left, right] = args else {
            return Err(RunnerError::internal(span, "text pair arity"));
        };
        let Value::Str(left) = self.eval_operand(left, span)? else {
            return Err(RunnerError::internal(span, "text pair left is not textus"));
        };
        let Value::Str(right) = self.eval_operand(right, span)? else {
            return Err(RunnerError::internal(span, "text pair right is not textus"));
        };
        Ok(Value::Bool(f(left, right)))
    }

    fn map_numeric(
        &mut self,
        args: &[MirOperand],
        span: Span,
        return_ty: MirType,
        f: impl FnOnce(f64) -> f64,
    ) -> Result<Value, RunnerError> {
        let [receiver] = args else {
            return Err(RunnerError::internal(span, "numeric collection arity"));
        };
        let value = self.eval_operand(receiver, span)?;
        let numeric = match value {
            Value::Int(v) => f(v as f64),
            Value::UInt(v) => f(v as f64),
            Value::Float(v) => f(v),
            _ => {
                return Err(RunnerError::internal(
                    span,
                    "numeric receiver type mismatch",
                ));
            }
        };
        // Rounding ops (`round`/`floor`/`ceil`/`trunc`) are typed numerus;
        // abs/signum keep the receiver's carrier. Honor the declared result
        // so a later comparison (`n ≺ minimum_i32`) sees Int vs Int, not
        // Float vs Int ("comparison type mismatch").
        match self.types.get(return_ty.semantic_id()) {
            Type::Primitive(Primitive::Numerus) | Type::SizedNumeric(Primitive::Numerus, _) => {
                Ok(Value::Int(numeric as i64))
            }
            Type::Primitive(Primitive::Fractus) | Type::SizedNumeric(Primitive::Fractus, _) => {
                Ok(Value::Float(numeric))
            }
            _ => Err(RunnerError::internal(
                span,
                "numeric collection result type mismatch",
            )),
        }
    }

    fn numeric_extremum(
        &mut self,
        args: &[MirOperand],
        span: Span,
        return_ty: MirType,
        op: fn(&Value, &Value, NumericWidth, Span) -> Result<Value, RunnerError>,
        scalar_op: fn(f64, f64) -> f64,
    ) -> Result<Value, RunnerError> {
        let is_array = matches!(
            self.types.get(return_ty.semantic_id()),
            Type::Array(_) | Type::BoundedArray { .. }
        );
        let is_tensor = matches!(self.types.get(return_ty.semantic_id()), Type::Tensor(_, _));
        if is_array {
            return self.array_elementwise(args, span, "lista extremum", op, return_ty);
        }
        if is_tensor {
            return self.tensor_elementwise_or_scalar(args, span, "tensor extremum", op, return_ty);
        }
        self.map_numeric_pair(args, span, scalar_op)
    }

    fn map_numeric_pair(
        &mut self,
        args: &[MirOperand],
        span: Span,
        f: impl FnOnce(f64, f64) -> f64,
    ) -> Result<Value, RunnerError> {
        let [left, right] = args else {
            return Err(RunnerError::internal(span, "numeric pair arity"));
        };
        let left = self.eval_operand(left, span)?;
        let right = self.eval_operand(right, span)?;
        match (left, right) {
            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(f(a as f64, b as f64) as i64)),
            (Value::Int(a), Value::Float(b)) => Ok(Value::Float(f(a as f64, b))),
            (Value::Float(a), Value::Int(b)) => Ok(Value::Float(f(a, b as f64))),
            (Value::Float(a), Value::Float(b)) => Ok(Value::Float(f(a, b))),
            _ => Err(RunnerError::internal(span, "numeric pair type mismatch")),
        }
    }

    fn sum_numeric_array(
        &mut self,
        args: &[MirOperand],
        return_ty: MirType,
        span: Span,
    ) -> Result<Value, RunnerError> {
        let [receiver] = args else {
            return Err(RunnerError::internal(span, "collection sum arity"));
        };
        let values = match self.eval_operand(receiver, span)? {
            Value::Array(cell) => cell.borrow().clone(),
            Value::Tensor(cell) => cell.borrow().data.clone(),
            _ => return Err(RunnerError::internal(span, "sum receiver type mismatch")),
        };

        // Modular-word sum: fold with wrapping_add and mask to the declared
        // width so the result respects 2^W wraparound.
        if let Some(width) = self.modular_word_width_of(return_ty) {
            let mask = modular_word_mask(width);
            let mut acc: u64 = 0;
            for value in &values {
                let v = modular_operand(value.clone(), span, "modular sum element type mismatch")?;
                acc = acc.wrapping_add(v) & mask;
            }
            return Ok(modular_word_result(acc, width));
        }

        // The integer path applies the same width-checking discipline as every
        // other integer op (`numeric_binop`): the declared `numerus<W>` width
        // governs the accumulation, so `sum` overflows exactly where a
        // width-checked add would. Unsigned carriers accumulate in u64 and
        // never sign-encode upper-half magnitudes.
        let width =
            effective_width(self.types, return_ty.semantic_id()).unwrap_or(NumericWidth::I64);
        let mut int_sum = 0_i64;
        let mut uint_sum = 0_u64;
        let mut saw_uint = false;
        let mut float_sum = 0.0_f64;
        let mut saw_float = false;
        for value in &values {
            match value {
                Value::Int(v) if saw_float => float_sum += *v as f64,
                Value::Int(v) if saw_uint => {
                    uint_sum = uint_width_checked_add(
                        uint_sum,
                        u64::try_from(*v).map_err(|_| {
                            RunnerError::internal(span, "sum element type mismatch")
                        })?,
                        width,
                        span,
                    )?;
                }
                Value::Int(v) => int_sum = width_checked_add(int_sum, *v, width, span)?,
                Value::UInt(v) if saw_float => float_sum += *v as f64,
                Value::UInt(v) => {
                    if !saw_uint {
                        // Carry any prior signed accumulation into the unsigned
                        // lane; a negative prior sum cannot exist in an
                        // unsigned-width context (width checks reject it).
                        uint_sum = u64::try_from(int_sum).map_err(|_| {
                            RunnerError::internal(span, "sum element type mismatch")
                        })?;
                        saw_uint = true;
                    }
                    uint_sum = uint_width_checked_add(uint_sum, *v, width, span)?;
                }
                Value::Float(v) => {
                    if !saw_float {
                        float_sum = if saw_uint {
                            uint_sum as f64
                        } else {
                            int_sum as f64
                        };
                        saw_float = true;
                    }
                    float_sum += *v;
                }
                _ => return Err(RunnerError::internal(span, "sum element type mismatch")),
            }
        }
        if saw_float {
            Ok(Value::Float(float_sum))
        } else if saw_uint {
            Ok(Value::UInt(uint_sum))
        } else {
            Ok(Value::Int(int_sum))
        }
    }

    /// LSR-W0 argmax/argmin scan over an array-backed collection.
    ///
    /// One law serves host scan and device tree-combine: among equal values
    /// the least index wins, so partials combine as `(value desc, index
    /// asc)` pairs — the sequential scan and any tree-combine of partials
    /// produce the identical winner. The NaN law is pinned to the
    /// `TensorMax`/`TensorMin` folds (`f64::max`/`f64::min`, maxNum
    /// semantics): a NaN element never wins while any number participates,
    /// and an all-NaN source yields index 0 carrying NaN. An empty source
    /// yields `nihil`; there is no −1 sentinel path anywhere.
    fn arg_extremum_index(
        &mut self,
        args: &[MirOperand],
        span: Span,
        want_max: bool,
    ) -> Result<Value, RunnerError> {
        let [receiver] = args else {
            return Err(RunnerError::internal(
                span,
                "collection argmax/argmin arity",
            ));
        };
        let values = match self.eval_operand(receiver, span)? {
            Value::Array(cell) => cell.borrow().clone(),
            _ => {
                return Err(RunnerError::internal(
                    span,
                    "argmax/argmin receiver type mismatch",
                ));
            }
        };
        let mut winner: Option<usize> = None;
        for (index, value) in values.iter().enumerate() {
            match winner {
                None => winner = Some(index),
                Some(best) => {
                    if arg_extremum_replaces(&values[best], value, want_max, span)? {
                        winner = Some(index);
                    }
                }
            }
        }
        Ok(match winner {
            Some(index) => Value::Int(index as i64),
            None => Value::Nil,
        })
    }

    fn approximate_fractus(
        &mut self,
        args: &[MirOperand],
        span: Span,
    ) -> Result<Value, RunnerError> {
        match args {
            [receiver, other, tolerance] => {
                let receiver = numeric_as_f64(&self.eval_operand(receiver, span)?, span)?;
                let other = numeric_as_f64(&self.eval_operand(other, span)?, span)?;
                let tolerance = numeric_as_f64(&self.eval_operand(tolerance, span)?, span)?;
                Ok(Value::Bool((receiver - other).abs() <= tolerance))
            }
            [receiver, other, rel_tol, abs_tol, negated] => {
                let width = self.fuzzy_comparison_width(receiver, other, span);
                let receiver = numeric_as_f64(&self.eval_operand(receiver, span)?, span)?;
                let other = numeric_as_f64(&self.eval_operand(other, span)?, span)?;
                let rel_tol = numeric_as_f64(&self.eval_operand(rel_tol, span)?, span)?;
                let abs_tol = numeric_as_f64(&self.eval_operand(abs_tol, span)?, span)?;
                let receiver = self.fuzzy_value(receiver, width);
                let other = self.fuzzy_value(other, width);
                let rel_tol = self.fuzzy_value(rel_tol, width);
                let abs_tol = self.fuzzy_value(abs_tol, width);
                let negated = matches!(self.eval_operand(negated, span)?, Value::Bool(true));
                let limit = (rel_tol * receiver.abs().max(other.abs())).max(abs_tol);
                let equal = (receiver - other).abs() <= limit;
                Ok(Value::Bool(if negated { !equal } else { equal }))
            }
            _ => Err(RunnerError::internal(span, "approximate comparison arity")),
        }
    }

    fn fuzzy_value(&self, value: f64, width: NumericWidth) -> f64 {
        if matches!(width, NumericWidth::F64) {
            value
        } else {
            #[expect(clippy::cast_possible_truncation)]
            {
                value as f32 as f64
            }
        }
    }

    fn fuzzy_comparison_width(
        &self,
        lhs: &MirOperand,
        rhs: &MirOperand,
        span: Span,
    ) -> NumericWidth {
        let mut saw_f32 = false;
        for operand in [lhs, rhs] {
            let Some(ty) = self.operand_semantic_type(operand, span) else {
                continue;
            };
            match self.types.get(ty) {
                Type::Primitive(Primitive::Fractus) => return NumericWidth::F64,
                Type::SizedNumeric(Primitive::Fractus, width) => {
                    if matches!(width, NumericWidth::F64) {
                        return NumericWidth::F64;
                    }
                    saw_f32 = true;
                }
                _ => {}
            }
        }
        if saw_f32 {
            NumericWidth::F32
        } else {
            NumericWidth::F64
        }
    }

    fn operand_semantic_type(&self, operand: &MirOperand, span: Span) -> Option<TypeId> {
        let MirOperand::Place(place) = operand else {
            return None;
        };
        let frame = self.frames.last()?;
        let function = self.function_by_id(frame.function, span).ok()?;
        let ty = match place.base {
            radix_mir::MirPlaceBase::Local(id) => function
                .locals
                .iter()
                .find(|local| local.id == id)
                .map(|local| local.ty)
                .or_else(|| {
                    function
                        .params
                        .iter()
                        .find(|param| param.local == id)
                        .map(|param| param.ty)
                }),
            radix_mir::MirPlaceBase::Temp(id) => function
                .temps
                .iter()
                .find(|temp| temp.id == id)
                .map(|temp| temp.ty),
        }?;
        Some(ty.semantic_id())
    }

    fn eval_option_chain(
        &mut self,
        base: &MirOperand,
        link: &MirOptionChainLink,
        span: Span,
    ) -> Result<Value, RunnerError> {
        let current = self.eval_operand(base, span)?;
        if matches!(current, Value::Nil) {
            return Ok(Value::Nil);
        }
        match link {
            MirOptionChainLink::Field(field) => self.project_field(current, *field, span),
            MirOptionChainLink::VariantField { variant, field } => {
                self.project_variant_field(current, *variant, *field, span)
            }
            MirOptionChainLink::Index(index) => {
                let index = self.eval_operand(index, span)?;
                self.optional_collection_index(current, index, span)
            }
            MirOptionChainLink::Call { .. } => {
                Err(RunnerError::unsupported(span, "option chain call"))
            }
        }
    }

    // PLACE PROJECTION

    pub(crate) fn project(
        &mut self,
        value: Value,
        projection: &radix_mir::MirProjection,
        span: Span,
    ) -> Result<Value, RunnerError> {
        match projection {
            radix_mir::MirProjection::Field(field) => self.project_field(value, *field, span),
            radix_mir::MirProjection::VariantField { variant, field } => {
                self.project_variant_field(value, *variant, *field, span)
            }
            radix_mir::MirProjection::Index(index) => {
                let index = self.eval_operand(index, span)?;
                self.collection_index(value, index, span)
            }
            radix_mir::MirProjection::VectorLane(lane) => match value {
                Value::Vector(lanes) => lanes
                    .borrow()
                    .get(usize::from(*lane))
                    .cloned()
                    .ok_or_else(|| RunnerError::internal(span, "vector lane out of bounds")),
                _ => Err(RunnerError::internal(span, "vector lane projection base")),
            },
            radix_mir::MirProjection::MatrixCell { row, column } => match value {
                Value::Matrix(matrix) => matrix
                    .borrow()
                    .cell(*row, *column)
                    .cloned()
                    .ok_or_else(|| RunnerError::internal(span, "matrix cell out of bounds")),
                _ => Err(RunnerError::internal(span, "matrix cell projection base")),
            },
            radix_mir::MirProjection::ClosureCapture {
                environment,
                source,
            } => self.project_closure_capture(value, *environment, *source, span),
            radix_mir::MirProjection::SharedCell => match value {
                Value::SharedCell(cell) => Ok(cell.borrow().clone()),
                _ => Err(RunnerError::internal(
                    span,
                    "shared cell projection base is not a cell",
                )),
            },
        }
    }

    fn project_closure_capture(
        &self,
        value: Value,
        environment_id: radix_mir::MirClosureEnvironmentId,
        source: radix_hir::DefId,
        span: Span,
    ) -> Result<Value, RunnerError> {
        let Value::ClosureEnvironment(value_environment, cell) = value else {
            return Err(RunnerError::internal(
                span,
                "closure capture projection base is not an environment",
            ));
        };
        if value_environment != environment_id {
            return Err(RunnerError::internal(
                span,
                "closure capture environment mismatch",
            ));
        }
        let Some(environment) = self.context.closure_environments.get(&environment_id) else {
            return Err(RunnerError::internal(
                span,
                "closure capture metadata is missing",
            ));
        };
        let Some(capture) = environment
            .captures
            .iter()
            .find(|capture| capture.source == source)
        else {
            return Err(RunnerError::internal(
                span,
                "closure capture source is missing",
            ));
        };
        let capture_value = cell
            .borrow()
            .get(&capture.name)
            .cloned()
            .ok_or_else(|| RunnerError::internal(span, "closure capture value is missing"))?;
        Ok(capture_value)
    }

    fn project_field(&self, value: Value, field: Symbol, span: Span) -> Result<Value, RunnerError> {
        match value {
            Value::Struct(def_id, cell) => {
                if let Some(value) = cell.borrow().get(&field).cloned() {
                    return Ok(value);
                }
                if self
                    .context
                    .optional_struct_fields
                    .get(&def_id)
                    .is_some_and(|fields| fields.contains(&field))
                {
                    return Ok(Value::Nil);
                }
                Err(RunnerError::internal(
                    span,
                    format!("missing struct field sym#{}", field.0),
                ))
            }
            Value::Record(cell) => cell.borrow().get(&field).cloned().ok_or_else(|| {
                RunnerError::internal(span, format!("missing record field sym#{}", field.0))
            }),
            Value::Tuple(items) => {
                let idx = field.0 as usize;
                items
                    .get(idx)
                    .cloned()
                    .ok_or_else(|| RunnerError::internal(span, "tuple field out of bounds"))
            }
            Value::EnumVariant(_, cell) => cell.borrow().get(&field).cloned().ok_or_else(|| {
                RunnerError::internal(span, format!("missing enum field sym#{}", field.0))
            }),
            _ => Err(RunnerError::internal(
                span,
                "field projection on non-aggregate",
            )),
        }
    }

    pub(crate) fn project_variant_field(
        &self,
        value: Value,
        variant: DefId,
        field: Symbol,
        span: Span,
    ) -> Result<Value, RunnerError> {
        match value {
            Value::EnumVariant(id, cell) if id == variant => {
                if let Some(value) = cell.borrow().get(&field).cloned() {
                    return Ok(value);
                }
                // F-083: optional fields on enum variants fall back to Nil,
                // matching the Struct projection contract.
                if self
                    .context
                    .optional_struct_fields
                    .get(&variant)
                    .is_some_and(|fields| fields.contains(&field))
                {
                    return Ok(Value::Nil);
                }
                Err(RunnerError::internal(span, "variant field out of bounds"))
            }
            _ => Err(RunnerError::internal(
                span,
                "variant field projection mismatch",
            )),
        }
    }

    fn key_to_value(key: &MapKey) -> Value {
        match key {
            MapKey::Int(v) => Value::Int(*v),
            MapKey::UInt(v) => Value::UInt(*v),
            MapKey::Bool(v) => Value::Bool(*v),
            MapKey::Str(v) => Value::Str(v.clone()),
            MapKey::Unit => Value::Unit,
        }
    }

    // PROVIDERS

    fn eval_provider(
        &mut self,
        provider: &MirProvider,
        args: &[MirOperand],
        _return_ty: MirType,
        span: Span,
    ) -> Result<Value, RunnerError> {
        let identity = provider_identity(provider, self.interner);
        if let MirProviderKind::Kernel(module) = provider.kind {
            let mut evaluated = Vec::with_capacity(args.len());
            for arg in args {
                evaluated.push(self.eval_operand(arg, span)?);
            }
            let verb = self.interner.resolve(provider.name);
            return self
                .host
                .kernel_call(module, verb, &evaluated, span)
                .map_err(|error| error.with_provider_identity(&identity));
        }
        self.host
            .provider(provider)
            .map_err(|error| error.with_provider_identity(&identity))
    }

    fn eval_set_algebra(
        &mut self,
        op: MirCollectionOp,
        args: &[MirOperand],
        span: Span,
    ) -> Result<Value, RunnerError> {
        let [left, right] = args else {
            return Err(RunnerError::internal(span, "set algebra arity"));
        };
        let left = self.eval_operand(left, span)?;
        let right = self.eval_operand(right, span)?;
        let (Value::Set(left_cell), Value::Set(right_cell)) = (left, right) else {
            return Err(RunnerError::internal(
                span,
                "set algebra requires copia operands",
            ));
        };
        let left = left_cell.borrow().clone();
        let right = right_cell.borrow().clone();
        match op {
            MirCollectionOp::Union => {
                let mut out = left.clone();
                out.extend(right.iter().cloned());
                Ok(Value::Set(Rc::new(RefCell::new(out))))
            }
            MirCollectionOp::Intersection => {
                let out = left.intersection(&right).cloned().collect();
                Ok(Value::Set(Rc::new(RefCell::new(out))))
            }
            MirCollectionOp::Difference => {
                let out = left.difference(&right).cloned().collect();
                Ok(Value::Set(Rc::new(RefCell::new(out))))
            }
            MirCollectionOp::SymmetricDifference => {
                let out = left.symmetric_difference(&right).cloned().collect();
                Ok(Value::Set(Rc::new(RefCell::new(out))))
            }
            MirCollectionOp::IsSubset => Ok(Value::Bool(left.is_subset(&right))),
            MirCollectionOp::IsSuperset => Ok(Value::Bool(left.is_superset(&right))),
            _ => Err(RunnerError::unsupported(
                span,
                &format!("set algebra {op:?}"),
            )),
        }
    }
}

// LOCAL VALUE HELPERS

fn require_rank1_extent(
    tensor: &TensorValue,
    extent: usize,
    span: Span,
    message: &'static str,
) -> Result<(), RunnerError> {
    if tensor.shape.len() != 1 || tensor.shape[0] as usize != extent {
        return Err(RunnerError::internal(span, message));
    }
    Ok(())
}

fn byte_from_value(value: Value, span: Span) -> Result<u8, RunnerError> {
    match value {
        Value::Int(value) => u8::try_from(value)
            .map_err(|_| RunnerError::internal(span, "octeti append byte out of range")),
        _ => Err(RunnerError::internal(
            span,
            "octeti append requires numerus byte",
        )),
    }
}

fn array_value(items: Vec<Value>) -> Value {
    Value::Array(Rc::new(RefCell::new(items)))
}

fn tensor_value(data: Vec<Value>, shape: Vec<i64>) -> Value {
    Value::Tensor(Rc::new(RefCell::new(TensorValue { data, shape })))
}

fn interval_value(start: Value, end: Value, inclusive: bool) -> Value {
    Value::Tuple(vec![start, end, Value::Bool(inclusive)])
}

fn interval_parts(value: &Value, span: Span) -> Result<(Value, Value, bool), RunnerError> {
    let Value::Tuple(parts) = value else {
        return Err(RunnerError::internal(
            span,
            "interval value carrier mismatch",
        ));
    };
    let [start, end, Value::Bool(inclusive)] = parts.as_slice() else {
        return Err(RunnerError::internal(span, "interval value shape mismatch"));
    };
    Ok((start.clone(), end.clone(), *inclusive))
}

fn interval_intersection(
    left: &Value,
    right: &Value,
    span: Span,
) -> Result<Option<Value>, RunnerError> {
    let (left_start, left_end, left_inclusive) = interval_parts(left, span)?;
    let (right_start, right_end, right_inclusive) = interval_parts(right, span)?;
    let left_start_number = numeric_as_f64(&left_start, span)?;
    let right_start_number = numeric_as_f64(&right_start, span)?;
    let left_end_number = numeric_as_f64(&left_end, span)?;
    let right_end_number = numeric_as_f64(&right_end, span)?;
    let start = if left_start_number >= right_start_number {
        left_start
    } else {
        right_start
    };
    let (end, inclusive) = if left_end_number < right_end_number {
        (left_end, left_inclusive)
    } else if right_end_number < left_end_number {
        (right_end, right_inclusive)
    } else {
        (left_end, left_inclusive && right_inclusive)
    };
    let start_number = numeric_as_f64(&start, span)?;
    let end_number = numeric_as_f64(&end, span)?;
    Ok(
        (start_number < end_number || (inclusive && start_number == end_number))
            .then(|| interval_value(start, end, inclusive)),
    )
}

fn interval_union_value(
    left: &Value,
    right: &Value,
    span: Span,
) -> Result<Option<Value>, RunnerError> {
    let (left_start, left_end, left_inclusive) = interval_parts(left, span)?;
    let (right_start, right_end, right_inclusive) = interval_parts(right, span)?;
    let left_start_number = numeric_as_f64(&left_start, span)?;
    let right_start_number = numeric_as_f64(&right_start, span)?;
    let (start, first_end, first_inclusive, second_start, second_end, second_inclusive) =
        if left_start_number <= right_start_number {
            (
                left_start,
                left_end,
                left_inclusive,
                right_start,
                right_end,
                right_inclusive,
            )
        } else {
            (
                right_start,
                right_end,
                right_inclusive,
                left_start,
                left_end,
                left_inclusive,
            )
        };
    if numeric_as_f64(&second_start, span)? > numeric_as_f64(&first_end, span)? {
        return Ok(None);
    }
    let first_end_number = numeric_as_f64(&first_end, span)?;
    let second_end_number = numeric_as_f64(&second_end, span)?;
    let (end, inclusive) = if second_end_number > first_end_number {
        (second_end, second_inclusive)
    } else if first_end_number > second_end_number {
        (first_end, first_inclusive)
    } else {
        (first_end, first_inclusive || second_inclusive)
    };
    Ok(Some(interval_value(start, end, inclusive)))
}

fn literal_sparse_shape(
    types: &TypeTable,
    shape: IndexId,
    span: Span,
) -> Result<Vec<i64>, RunnerError> {
    match types.get_index(shape) {
        IndexExpr::Tuple(dimensions) => dimensions
            .iter()
            .map(|dimension| match types.get_index(*dimension) {
                IndexExpr::Literal(value) => i64::try_from(*value)
                    .map_err(|_| RunnerError::internal(span, "sparse shape dimension overflow")),
                _ => Err(RunnerError::internal(
                    span,
                    "sparse vacua requires literal shape",
                )),
            })
            .collect(),
        IndexExpr::Literal(value) => i64::try_from(*value)
            .map(|value| vec![value])
            .map_err(|_| RunnerError::internal(span, "sparse shape dimension overflow")),
        _ => Err(RunnerError::internal(
            span,
            "sparse vacua requires literal shape",
        )),
    }
}

fn sparse_zero_value(
    types: &TypeTable,
    mut element: TypeId,
    span: Span,
) -> Result<Value, RunnerError> {
    loop {
        match types.get(element) {
            Type::Alias(_, inner) => element = *inner,
            Type::Primitive(Primitive::Numerus) | Type::SizedNumeric(Primitive::Numerus, _) => {
                return Ok(Value::Int(0));
            }
            Type::Primitive(Primitive::Fractus) | Type::SizedNumeric(Primitive::Fractus, _) => {
                return Ok(Value::Float(0.0));
            }
            _ => {
                return Err(RunnerError::internal(
                    span,
                    "sparse element type is not numeric",
                ));
            }
        }
    }
}

fn tensor_element_count(shape: &[i64], span: Span) -> Result<usize, RunnerError> {
    for dim in shape {
        if !tensor_dim_non_negative(*dim) {
            return Err(RunnerError::internal(
                span,
                "tensor shape dimension overflow",
            ));
        }
    }
    tensor_shape_element_count(shape)
        .ok_or_else(|| RunnerError::internal(span, ERR_ELEMENT_COUNT_OVERFLOW))
}

/// Extract non-negative gradient shape dims from a shape operand.
///
/// Accepts a `lista` of dims or a tensor (whose declared shape provides the
/// dims), and verifies the dim count matches the declared `rank` — mirroring
/// the host ABI's rank/negative-dim validation before storage allocation.
fn gradient_shape_dims(
    shape_value: &Value,
    rank: usize,
    span: Span,
) -> Result<Vec<i64>, RunnerError> {
    let dims: Vec<i64> = match shape_value {
        Value::Array(items) => items
            .borrow()
            .iter()
            .map(|dim| match dim {
                Value::Int(v) if *v >= 0 => Ok(*v),
                Value::UInt(v) => i64::try_from(*v)
                    .map_err(|_| RunnerError::internal(span, "gradient shape dim out of range")),
                _ => Err(RunnerError::internal(
                    span,
                    "gradient shape dim is not a non-negative numerus",
                )),
            })
            .collect::<Result<Vec<_>, _>>()?,
        Value::Tensor(tensor) => tensor.borrow().shape.clone(),
        _ => {
            return Err(RunnerError::internal(
                span,
                "gradient shape is not a lista or tensor",
            ));
        }
    };
    if dims.len() != rank {
        return Err(RunnerError::internal(span, "gradient shape rank mismatch"));
    }
    if dims.iter().any(|dim| *dim < 0) {
        return Err(RunnerError::internal(
            span,
            "gradient shape has negative dim",
        ));
    }
    Ok(dims)
}

fn accumulate_gradient_data(
    stored: &mut [Value],
    incoming: &[Value],
    span: Span,
) -> Result<(), RunnerError> {
    if incoming.len() != stored.len() {
        return Err(RunnerError::internal(
            span,
            "gradient accumulate backing-length mismatch",
        ));
    }
    for (slot, value) in stored.iter_mut().zip(incoming) {
        *slot = numeric_add(slot, value, NumericWidth::F64, span)?;
    }
    Ok(())
}

fn extract_f64s(data: &[Value]) -> Option<Vec<f64>> {
    let mut out = Vec::with_capacity(data.len());
    for value in data {
        match value {
            Value::Float(v) => out.push(*v),
            _ => return None,
        }
    }
    Some(out)
}

fn extract_i64s(data: &[Value]) -> Option<Vec<i64>> {
    let mut out = Vec::with_capacity(data.len());
    for value in data {
        match value {
            Value::Int(v) => out.push(*v),
            _ => return None,
        }
    }
    Some(out)
}

fn extract_u64s(data: &[Value]) -> Option<Vec<u64>> {
    let mut out = Vec::with_capacity(data.len());
    for value in data {
        match value {
            Value::UInt(v) => out.push(*v),
            _ => return None,
        }
    }
    Some(out)
}

fn matmul_dims(m: i64, k: i64, n: i64) -> (usize, usize, usize) {
    (m.max(0) as usize, k.max(0) as usize, n.max(0) as usize)
}

fn matmul_f64(left: &[f64], right: &[f64], m: i64, k: i64, n: i64) -> Vec<f64> {
    let (m, k, n) = matmul_dims(m, k, n);
    let mut out = vec![0.0; m.saturating_mul(n)];
    for i in 0..m {
        for kk in 0..k {
            let a = left[i * k + kk];
            let right_row = &right[kk * n..(kk + 1) * n];
            let out_row = &mut out[i * n..(i + 1) * n];
            for j in 0..n {
                out_row[j] += a * right_row[j];
            }
        }
    }
    out
}

fn matmul_i64(
    left: &[i64],
    right: &[i64],
    m: i64,
    k: i64,
    n: i64,
    width: NumericWidth,
    span: Span,
) -> Result<Vec<i64>, RunnerError> {
    let (m, k, n) = matmul_dims(m, k, n);
    let mut out = vec![0_i64; m.saturating_mul(n)];
    for i in 0..m {
        for kk in 0..k {
            let a = left[i * k + kk];
            let right_row = &right[kk * n..(kk + 1) * n];
            let out_row = &mut out[i * n..(i + 1) * n];
            for j in 0..n {
                let prod = width_checked_mul(a, right_row[j], width, span)?;
                out_row[j] = width_checked_add(out_row[j], prod, width, span)?;
            }
        }
    }
    Ok(out)
}

fn matmul_u64(
    left: &[u64],
    right: &[u64],
    m: i64,
    k: i64,
    n: i64,
    width: NumericWidth,
    span: Span,
) -> Result<Vec<u64>, RunnerError> {
    let (m, k, n) = matmul_dims(m, k, n);
    let mut out = vec![0_u64; m.saturating_mul(n)];
    for i in 0..m {
        for kk in 0..k {
            let a = left[i * k + kk];
            let right_row = &right[kk * n..(kk + 1) * n];
            let out_row = &mut out[i * n..(i + 1) * n];
            for j in 0..n {
                let prod = uint_width_checked_mul(a, right_row[j], width, span)?;
                out_row[j] = uint_width_checked_add(out_row[j], prod, width, span)?;
            }
        }
    }
    Ok(out)
}

fn matmul_values(
    left: &[Value],
    right: &[Value],
    m: i64,
    k: i64,
    n: i64,
    width: NumericWidth,
    span: Span,
) -> Result<Vec<Value>, RunnerError> {
    let (m, k, n) = matmul_dims(m, k, n);
    let mut out = vec![Value::Int(0); m.saturating_mul(n)];
    for i in 0..m {
        for kk in 0..k {
            let a = &left[i * k + kk];
            for j in 0..n {
                let prod = numeric_mul(a, &right[kk * n + j], width, span)?;
                let slot = &mut out[i * n + j];
                *slot = numeric_add(slot, &prod, width, span)?;
            }
        }
    }
    Ok(out)
}

fn numeric_as_f64(value: &Value, span: Span) -> Result<f64, RunnerError> {
    match value {
        Value::Int(value) => Ok(*value as f64),
        Value::UInt(value) => Ok(*value as f64),
        Value::Float(value) => Ok(*value),
        _ => Err(RunnerError::internal(
            span,
            "tensor numeric element type mismatch",
        )),
    }
}

/// Whether `candidate` replaces the incumbent arg-extremum winner: strict
/// improvement only (the least index keeps ties — the stated-once law that
/// is also the parallel-combine order). NaN law pinned to the `f64::max` /
/// `f64::min` folds: NaN loses to every number and never replaces one;
/// NaN-vs-NaN keeps the incumbent, so an all-NaN source keeps index 0.
fn arg_extremum_replaces(
    incumbent: &Value,
    candidate: &Value,
    want_max: bool,
    span: Span,
) -> Result<bool, RunnerError> {
    // Integer pairs compare exactly — never through an f64 round-trip.
    if let (Value::Int(best), Value::Int(value)) = (incumbent, candidate) {
        return Ok(if want_max { value > best } else { value < best });
    }
    if let (Value::UInt(best), Value::UInt(value)) = (incumbent, candidate) {
        return Ok(if want_max { value > best } else { value < best });
    }
    let best = numeric_as_f64(incumbent, span)?;
    let value = numeric_as_f64(candidate, span)?;
    if best.is_nan() {
        return Ok(!value.is_nan());
    }
    if value.is_nan() {
        return Ok(false);
    }
    Ok(if want_max { value > best } else { value < best })
}

/// Numeric pair dispatch with width-checked integer arithmetic and zero-div guard.
///
/// Routes integer operands through `width_checked_*` / `uint_width_checked_*`
/// so overflow honors `numerus<W>`. Float operands remain width-agnostic
/// (f32 vs f64 fidelity is a separate gap). Mixed int/float promotes to float.
///
/// F-029: replaces raw i64 +/−/*// with width-checked ops; adds `UInt` support
/// and zero-div guard on the tensor arithmetic path.
fn numeric_pair(
    left: &Value,
    right: &Value,
    width: NumericWidth,
    span: Span,
    int_op: fn(i64, i64, NumericWidth, Span) -> Result<i64, RunnerError>,
    uint_op: fn(u64, u64, NumericWidth, Span) -> Result<u64, RunnerError>,
    float_op: impl FnOnce(f64, f64) -> f64,
) -> Result<Value, RunnerError> {
    match (left, right) {
        // Signed integer pair → width-checked.
        (Value::Int(a), Value::Int(b)) => int_op(*a, *b, width, span).map(Value::Int),
        // Unsigned integer pair → width-checked.
        (Value::UInt(a), Value::UInt(b)) => uint_op(*a, *b, width, span).map(Value::UInt),
        // Mixed signed/unsigned: promote to float for consistency with the
        // rest of the runner's non-binop numeric paths (the binop path uses
        // numeric_binop which has stricter carrier rules).
        (Value::UInt(a), Value::Int(b)) | (Value::Int(b), Value::UInt(a)) => {
            Ok(Value::Float(float_op(*a as f64, *b as f64)))
        }
        // Float pair or mixed int/float → promote to float.
        _ => Ok(Value::Float(float_op(
            numeric_as_f64(left, span)?,
            numeric_as_f64(right, span)?,
        ))),
    }
}

fn numeric_add(
    left: &Value,
    right: &Value,
    width: NumericWidth,
    span: Span,
) -> Result<Value, RunnerError> {
    numeric_pair(
        left,
        right,
        width,
        span,
        super::value::width_checked_add,
        super::value::uint_width_checked_add,
        |a, b| a + b,
    )
}

fn numeric_sub(
    left: &Value,
    right: &Value,
    width: NumericWidth,
    span: Span,
) -> Result<Value, RunnerError> {
    numeric_pair(
        left,
        right,
        width,
        span,
        super::value::width_checked_sub,
        super::value::uint_width_checked_sub,
        |a, b| a - b,
    )
}

fn numeric_mul(
    left: &Value,
    right: &Value,
    width: NumericWidth,
    span: Span,
) -> Result<Value, RunnerError> {
    numeric_pair(
        left,
        right,
        width,
        span,
        super::value::width_checked_mul,
        super::value::uint_width_checked_mul,
        |a, b| a * b,
    )
}

fn numeric_div(
    left: &Value,
    right: &Value,
    width: NumericWidth,
    span: Span,
) -> Result<Value, RunnerError> {
    numeric_pair(
        left,
        right,
        width,
        span,
        super::value::width_checked_div,
        super::value::uint_width_checked_div,
        |a, b| a / b,
    )
}

fn numeric_min(
    left: &Value,
    right: &Value,
    width: NumericWidth,
    span: Span,
) -> Result<Value, RunnerError> {
    numeric_pair(
        left,
        right,
        width,
        span,
        |a, b, _, _| Ok(a.min(b)),
        |a, b, _, _| Ok(a.min(b)),
        super::value::ieee_minimum,
    )
}

fn numeric_max(
    left: &Value,
    right: &Value,
    width: NumericWidth,
    span: Span,
) -> Result<Value, RunnerError> {
    numeric_pair(
        left,
        right,
        width,
        span,
        |a, b, _, _| Ok(a.max(b)),
        |a, b, _, _| Ok(a.max(b)),
        super::value::ieee_maximum,
    )
}

/// Render one value for `value ¶ "spec"` (D2.1) through the shared reference
/// renderer. Typecheck already validated the spec against the value's type,
/// so a spec that does not parse here, or a value family `¶` does not
/// admit, is an internal error, never a user-facing failure (D2.4).
fn format_value(
    value: &Value,
    spec: &str,
    float_width: Option<radix_types::NumericWidth>,
    interner: &radix_lexer::Interner,
    span: Span,
) -> Result<String, RunnerError> {
    use radix_types::format_spec::{
        FormatSpec, parse_format_spec, render_float, render_instans_preset, render_integer,
        render_text,
    };
    let parsed = parse_format_spec(spec)
        .map_err(|_| RunnerError::internal(span, "format_value spec did not parse"))?;
    match (value, parsed) {
        (Value::Int(v), FormatSpec::Layout(layout)) => Ok(render_integer(i128::from(*v), &layout)),
        (Value::UInt(v), FormatSpec::Layout(layout)) => Ok(render_integer(i128::from(*v), &layout)),
        (Value::Float(v), FormatSpec::Layout(layout)) => {
            let mut display_error = None;
            let text = render_float(*v, &layout, |magnitude| {
                Value::Float(magnitude)
                    .display_typed(interner, span, float_width)
                    .unwrap_or_else(|err| {
                        display_error = Some(err);
                        String::new()
                    })
            });
            match display_error {
                Some(err) => Err(err),
                None => Ok(text),
            }
        }
        (Value::Str(v), FormatSpec::Layout(layout)) => Ok(render_text(v, &layout)),
        (Value::Instans(v), FormatSpec::Preset(preset)) => {
            Ok(render_instans_preset(&v.to_rfc3339(), preset))
        }
        _ => Err(RunnerError::internal(
            span,
            "format_value operand not admitted by ¶",
        )),
    }
}
