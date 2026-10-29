//! In-process evaluation of MIR runtime calls, aggregates, and projections.
//!
//! This file is the runner's host-facing interpreter surface: runtime
//! intrinsics and constructs become [`Value`]s through the active host.
//! Control-flow stepping stays in `lib.rs`. Target-directed `↦` conversion
//! stays in `conversio`.

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
use super::dense::{DenseKind, DenseStorage, dense_kind_of_type};
use super::sermo::SermoView;
use super::tensor_edge::{
    BuiltinEdgePolicy, CustomEdgePolicyId, CustomEdgeValueError, TensorEdgeResolution,
    resolve_custom_value,
};
use super::value::{
    GradientStorage, LedgerValue, MapKey, MatrixValue, TensorEdgeOp, TensorInverseMap, TensorValue,
    Value, as_text_str, coordinate_fits, format_template_shaped, inverse_view_coordinate,
    round_float, uint_width_checked_add, values_equal, width_checked_add, window_bounds,
};
use super::{
    CallOutcome, RUNNER_BOUNDED_LISTA_LENGTH_MISMATCH, RUNNER_LISTA_LENGTH_MISMATCH, Runner,
    RunnerError, gelu_f64, host::provider_identity, write_slot,
};
use handroll::FxHashMap;
use radix_hir::DefId;
use radix_lexer::{Span, Symbol};
use radix_mir::{
    MirAggregate, MirAggregateFields, MirAggregateItem, MirAggregateKind, MirBlockId,
    MirBuiltinEdgePolicy, MirCallee, MirCollectionOp, MirConstant, MirConversion,
    MirConversionFlavor, MirEdgePolicy, MirFunctionId, MirGradientOp, MirIntrinsic, MirLiftOp,
    MirOperand, MirOptionChainLink, MirOptionOp, MirOptionUnwrapMode, MirPlaceBase, MirProjection,
    MirProvider, MirProviderKind, MirRuntimeCall, MirSermoOp, MirShapeCheck, MirShapeExpr, MirType,
    MirTypeCheckOp, MirValueId,
};
use radix_runtime_contract::tensor::{
    ERR_ACCIPE_INVALID_INDEX, ERR_CREA_INVALID_SHAPE, ERR_ELEMENT_COUNT_OVERFLOW,
    ERR_FORMA_ELEMENT_COUNT, ERR_FORMA_RESHAPE_COUNT, ERR_MATMUL_ARGUMENT_RANK,
    ERR_MATMUL_INNER_DIMENSION, ERR_MATMUL_RECEIVER_RANK, ERR_PONDE_INVALID_INDEX,
    tensor_dim_non_negative, tensor_flat_offset, tensor_shape_element_count,
};
use radix_runtime_contract::trap::text::{
    LIST_INDEX_OUT_OF_BOUNDS, MAP_MISSING_KEY, NUMERUS_OVERFLOW, TEXT_INDEX_OUT_OF_BOUNDS,
    list_window_outside, text_window_outside, window_inside,
};
use radix_types::{
    IndexExpr, IndexId, MirMathFunction, NumericWidth, Primitive, Type, TypeId, TypeTable,
    effective_width,
};
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

/// The stable `ERRnnn` code for a lowering display name (`"<Family>.<Variant>"`).
///
/// The code is the fault's locale-independent identity; the variant word after
/// it stays in the program's code locale. A name outside the registry has no
/// code and keeps the plain display.
fn fault_code_for_display_name(name: &str) -> Option<&'static str> {
    let (family, variant) = name.split_once('.')?;
    radix_runtime_contract::fault_codes::fault_code(family, variant)
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
                let value = self.prepare_tensor_escape(value, span)?;
                // 9007e12c3 follow-through: floats declared at a narrower
                // width than f64 display at that width.
                let float_width = self.operand_float_width(arg);
                let tuple = self.operand_tuple_parts(arg);
                let order = self
                    .field_order()
                    .with_dynamic(self.operand_is_dynamic(arg))
                    .with_char_text(self.operand_holds_littera(arg))
                    .with_tuple(tuple.as_ref())
                    .with_bits(self.operand_bits_width(arg))
                    .with_interval(self.operand_holds_interval(arg));
                let text = value.display_typed(self.interner, order, span, float_width)?;
                self.host.scribe(*kind, &text)?;
                Ok(Value::Unit)
            }
            MirIntrinsic::FormatString { template } => {
                // The template is source text with escapes undecoded; decode it
                // exactly as a plain string constant is, so `"a\tb §"` agrees.
                let template_text = self.interned_textus(*template);
                let args = call
                    .args
                    .iter()
                    .map(|arg| self.eval_operand(arg, span))
                    .collect::<Result<Vec<_>, _>>()?;
                let args = args
                    .into_iter()
                    .map(|value| self.prepare_tensor_escape(value, span))
                    .collect::<Result<Vec<_>, _>>()?;
                // One value, one display: interpolation renders each argument
                // at its declared float width, as `print` does.
                let widths: Vec<_> = call
                    .args
                    .iter()
                    .map(|arg| self.operand_float_width(arg))
                    .collect();
                let char_text: Vec<_> = call
                    .args
                    .iter()
                    .map(|arg| self.operand_holds_littera(arg))
                    .collect();
                let tuples: Vec<_> = call
                    .args
                    .iter()
                    .map(|arg| self.operand_tuple_parts(arg))
                    .collect();
                let bits: Vec<_> = call
                    .args
                    .iter()
                    .map(|arg| self.operand_bits_width(arg))
                    .collect();
                let intervals: Vec<_> = call
                    .args
                    .iter()
                    .map(|arg| self.operand_holds_interval(arg))
                    .collect();
                Ok(Value::Str(format_template_shaped(
                    &template_text,
                    &args,
                    &widths,
                    &char_text,
                    &tuples,
                    &bits,
                    &intervals,
                    self.interner,
                    self.field_order(),
                    span,
                )?))
            }
            MirIntrinsic::FormatValue { spec } => {
                let [arg] = call.args.as_slice() else {
                    return Err(RunnerError::internal(span, "format_value runtime arity"));
                };
                let value = self.eval_operand(arg, span)?;
                let value = self.prepare_tensor_escape(value, span)?;
                let float_width = self.operand_float_width(arg);
                let spec = self.interner.resolve(*spec);
                format_value(&value, spec, float_width, span).map(Value::Str)
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
            MirIntrinsic::ReadAll => {
                if !call.args.is_empty() {
                    return Err(RunnerError::internal(span, "read_all runtime arity"));
                }
                match self.host.read_all()? {
                    Some(text) => Ok(Value::Str(text)),
                    None => Ok(Value::Nil),
                }
            }
            MirIntrinsic::Provider(provider) => self.eval_provider(provider, span),
            MirIntrinsic::SermoOpen => self.eval_sermo_open(&call.args, span),
            MirIntrinsic::SermoSetOpener => self.eval_sermo_set_opener(&call.args, span),
            MirIntrinsic::Sermo(op) => self.eval_sermo_op(op, &call.args, call.return_ty, span),
            MirIntrinsic::Cede => self.eval_cede(&call.args, span),
            MirIntrinsic::CursorStream(stream) => self.eval_cursor_stream(stream, &call.args, span),
            MirIntrinsic::Gradient(op) => self.eval_gradient(*op, &call.args, span),
            MirIntrinsic::TypeCheck(op) => self.eval_type_check(op, &call.args, span),
            MirIntrinsic::TensorShapeCheck(check) => {
                self.eval_tensor_shape_check(check, &call.args, span)
            }
            MirIntrinsic::TensorShapeSize(expression) => {
                self.eval_tensor_shape_size(expression, &call.args, span)
            }
            MirIntrinsic::TensorLimes(policy) => self.tensor_limes(policy, &call.args, span),
            MirIntrinsic::TensorLift(op) => {
                self.eval_tensor_lift(*op, &call.args, call.return_ty, span)
            }
            MirIntrinsic::Pow => self.eval_float_pow(&call.args, span),
            MirIntrinsic::Scan(op) => self.eval_scan(op, &call.args, span),
            MirIntrinsic::FilumReduce(op) if op.form == radix_mir::MirFilumForm::Grid => {
                self.eval_filum_grid_reduce(op, &call.args, call.return_ty, span)
            }
            MirIntrinsic::Duplicate => {
                let [source] = call.args.as_slice() else {
                    return Err(RunnerError::internal(span, "duplicate runtime arity"));
                };
                let source = self.eval_operand(source, span)?;
                Ok(self.prepare_tensor_escape(source, span)?.deep_copy())
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
        let route = self.eval_operand(route, span)?;
        let route = self.prepare_tensor_escape(route, span)?;
        let route = route.display(self.interner, self.field_order(), span)?;
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
            MirSermoOp::Flush => self.sermo_flush(&sermo, span),
            MirSermoOp::Finish => {
                let terminal = self.sermo_finish(&sermo, span)?;
                let Type::Enum(status) = self.types.get(return_ty.semantic_id()) else {
                    return Err(RunnerError::internal(
                        span,
                        "sermo finish status type mismatch",
                    ));
                };
                let variant = self.status_variant(*status, &terminal, span)?;
                Ok(Value::EnumVariant(
                    variant,
                    Rc::new(RefCell::new(FxHashMap::default())),
                ))
            }
            MirSermoOp::Close => Ok(Value::Bool(self.sermo_close(&sermo))),
            MirSermoOp::CloseFailed => Ok(Value::Bool(Self::sermo_close_failed(&sermo))),
            MirSermoOp::CloseError => Ok(Value::Str(Self::sermo_take_close_error(&sermo))),
            MirSermoOp::Receive { frame, .. } => self.sermo_receive(*frame, &sermo, span),
            MirSermoOp::Cursor { frame, .. } => {
                Ok(Value::Cursor(GeneratorCursor::sermo(sermo, *frame)))
            }
            MirSermoOp::DrainCursor => {
                Err(RunnerError::internal(span, "nested cursor drain dispatch"))
            }
        }
    }

    /// The `status` variant naming a conversation terminal. The builtin
    /// enum declares `request item byte bulk done error cancel` in that
    /// order (`radix-semantic` `frame_types::STATUS_VARIANTS`).
    fn status_variant(
        &self,
        status: radix_hir::DefId,
        terminal: &crate::sermo::SermoTerminal,
        span: Span,
    ) -> Result<radix_hir::DefId, RunnerError> {
        let position = match terminal {
            crate::sermo::SermoTerminal::Done => 4,
            crate::sermo::SermoTerminal::Error(_) => 5,
            crate::sermo::SermoTerminal::Cancel => 6,
        };
        self.status_variant_at(status, position, span)
    }

    /// The `status` variant declared at `position` (`request` is 0, `item` 1).
    pub(crate) fn status_variant_at(
        &self,
        status: radix_hir::DefId,
        position: usize,
        span: Span,
    ) -> Result<radix_hir::DefId, RunnerError> {
        self.context
            .enum_variants
            .get(&status)
            .and_then(|variants| variants.get(position))
            .copied()
            .ok_or_else(|| RunnerError::internal(span, "status enum variants missing"))
    }

    /// `cede <expr>` — yield inside a stream generator.
    ///
    /// WHY: inside a generator frame (`Frame::is_generator`) the value is stored
    /// in `Runner::yielded` and `cede` returns `Unit`; the run loop parks the
    /// frame once the statement completes and the cursor takes the item.
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
        let dispatched = self.dispatches_kernel(func_id, span)?;
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
        if dispatched {
            let mut arg_values = arg_values;
            return self.dispatch_kernel_call(func_id, &mut arg_values, span);
        }

        let mut frame = self.new_frame(func_id, MirBlockId(0));
        frame.is_generator = true;
        for (local, arg) in param_locals.into_iter().zip(arg_values) {
            write_slot(&mut frame.locals, local.0 as usize, arg, span, "local")?;
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
        let stored_shape = self
            .gradient_handles
            .get(&handle)
            .ok_or_else(|| {
                RunnerError::internal(span, format!("gradient accumulate unknown handle {handle}"))
            })?
            .shape
            .clone();
        if incoming_shape != stored_shape {
            return Err(RunnerError::internal(
                span,
                format!(
                    "gradient accumulate shape mismatch {incoming_shape:?} vs stored {stored_shape:?}"
                ),
            ));
        }
        let values = match &incoming {
            Value::Tensor(tensor) => self.tensor_logical_values(tensor, span)?,
            Value::Array(items) => items.borrow().clone(),
            _ => {
                return Err(RunnerError::internal(
                    span,
                    "gradient accumulate data is not tensor storage",
                ));
            }
        };
        let storage = self.gradient_handles.get_mut(&handle).ok_or_else(|| {
            RunnerError::internal(span, format!("gradient accumulate unknown handle {handle}"))
        })?;
        accumulate_gradient_data(&mut storage.data, &values, span)?;
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
        tensor_value(storage.data.clone(), storage.shape.clone(), span)
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
                let items = self.apply_element_display_width(items, aggregate.ty.semantic_id());
                Ok(Value::Array(Rc::new(RefCell::new(items))))
            }
            MirAggregateKind::Record => {
                let map = self.eval_named_fields(
                    &aggregate.fields,
                    span,
                    "record construct requires named fields",
                )?;
                // The construct's named-field vector is the record's written
                // order; display renders in it (the map is unordered).
                let written: Rc<[Symbol]> = match &aggregate.fields {
                    MirAggregateFields::Named(fields) => {
                        fields.iter().map(|field| field.name).collect()
                    }
                    _ => Rc::from([]),
                };
                Ok(Value::Record(Rc::new(RefCell::new(map)), written))
            }
            MirAggregateKind::Map => {
                let MirAggregateFields::Keyed(entries) = &aggregate.fields else {
                    return Err(RunnerError::internal(
                        span,
                        "map construct requires keyed fields",
                    ));
                };
                // WHY: a ledger literal lowers to the same keyed aggregate as a
                // map; the aggregate type keeps insertion order so a duplicate
                // key retains its first position with the later value.
                if matches!(
                    self.types.get(aggregate.ty.semantic_id()),
                    Type::Ledger(_, _)
                ) {
                    let mut ledger = LedgerValue::new();
                    for entry in entries {
                        let key = self.eval_operand(&entry.key, span)?.to_key(span)?;
                        let value = self.eval_operand(&entry.value, span)?;
                        ledger.insert(key, value);
                    }
                    return Ok(Value::Ledger(Rc::new(RefCell::new(ledger))));
                }
                let mut map = FxHashMap::default();
                for entry in entries {
                    let key = self.eval_operand(&entry.key, span)?.to_key(span)?;
                    let value = self.eval_operand(&entry.value, span)?;
                    map.insert(key, value);
                }
                Ok(Value::Map(Rc::new(RefCell::new(map))))
            }
            MirAggregateKind::Set => {
                let mut set = handroll::FxHashSet::default();
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
                Ok(Value::Tensor(Rc::new(RefCell::new(TensorValue::empty()))))
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
                tensor_value_typed(self.types, vec![zero; count], Some(*element), shape, span)
            }
            MirAggregateKind::Interval { inclusive } => {
                let mut bounds = self.eval_ordered_fields(&aggregate.fields, span)?;
                let step = if bounds.len() == 3 {
                    bounds.pop()
                } else {
                    None
                };
                let [start, end] = bounds.try_into().map_err(|_| {
                    RunnerError::internal(span, "interval construct requires two bounds")
                })?;
                // A stored stepped range takes its direction from its bounds:
                // `10‥0 per 2` descends (operator ruling, need). The
                // step itself stays positive; only its presence unlocks a
                // descending range.
                let stepped = step.is_some();
                let step = interval_step(step.as_ref(), span)?;
                // F-082: reject reversed bounds (start > end) — an unstepped
                // interval must be monotonic so every materialized element is
                // reachable.
                if !stepped && interval_bounds_reversed(&start, &end) {
                    return Err(RunnerError::internal(
                        span,
                        "interval bounds must be monotonic (start ≤ end)",
                    ));
                }
                let mut interval = interval_value(start, end, inclusive);
                if stepped && let Value::Tuple(parts) = &mut interval {
                    parts.push(Value::Int(step));
                }
                Ok(interval)
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
                let values = self.apply_element_display_width(values, aggregate.ty.semantic_id());
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
                let values = self.apply_element_display_width(values, aggregate.ty.semantic_id());
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
        let (source, shape) = match args {
            [source] => (source, None),
            // `octeti ↦ tensor<E, [.., size n]>`: the run-time extents.
            [source, shape] => (source, Some(self.conversion_shape_arg(shape, span)?)),
            _ => return Err(RunnerError::internal(span, "conversion runtime arity")),
        };
        let value = self.eval_operand(source, span)?;
        if matches!(value, Value::Nil)
            && let Some(recovery) = &conversion.recovery
        {
            return self.eval_recovery(recovery, span);
        }
        if let Some(site) = &conversion.store
            && let Some(stored) =
                self.eval_implicit_store(site, conversion.target_ty.semantic_id(), &value, span)
        {
            return stored;
        }
        match conversion.flavor {
            MirConversionFlavor::Cast => self.eval_cast(value, &conversion.target_ty, span),
            MirConversionFlavor::Runtime => match shape {
                Some(shape) => self.eval_runtime_convert_shaped(conversion, value, shape, span),
                None => self.eval_runtime_convert(conversion, value, span),
            },
            MirConversionFlavor::Proven => {
                self.eval_proven_narrow(value, &conversion.target_ty, span)
            }
        }
    }

    /// Evaluate a [`MirConversionFlavor::Proven`] narrowing: the value was
    /// proven to fit at compile time, so reduce it to the target's exact
    /// carrier without a runtime range check.
    fn eval_proven_narrow(
        &self,
        value: Value,
        target_ty: &radix_mir::MirType,
        span: Span,
    ) -> Result<Value, RunnerError> {
        let target = self.types.get(target_ty.semantic_id());
        match (value, target) {
            (Value::Int(v), Type::Primitive(Primitive::Numerus)) => Ok(Value::Int(v)),
            (Value::Int(v), Type::SizedNumeric(Primitive::Numerus, width)) => {
                crate::exact_int::exact_carrier(i128::from(v), *width).ok_or_else(|| {
                    RunnerError::internal(span, "proven narrowing does not fit its target")
                })
            }
            (other, target) => Err(RunnerError::internal(
                span,
                format!("unsupported proven narrowing from {other:?} to {target:?}"),
            )),
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
            (
                Value::Bool(v),
                Type::Primitive(Primitive::Numerus) | Type::SizedNumeric(Primitive::Numerus, _),
            ) => Ok(Value::Int(i64::from(v))),
            (other, target) => Err(RunnerError::internal(
                span,
                format!("unsupported cast from {other:?} to {target:?}"),
            )),
        }
    }

    /// Evaluate [`MirIntrinsic::Pow`]: IEEE 754 pow of two fractus operands
    /// through the host libm.
    fn eval_float_pow(&mut self, args: &[MirOperand], span: Span) -> Result<Value, RunnerError> {
        let [base, exponent] = args else {
            return Err(RunnerError::internal(span, "pow arity"));
        };
        let base = self.eval_operand(base, span)?;
        let exponent = self.eval_operand(exponent, span)?;
        let as_float = |value: Value| match value {
            Value::Float(v) => Ok(v),
            Value::Int(v) => Ok(v as f64),
            Value::UInt(v) => Ok(v as f64),
            other => Err(RunnerError::internal(
                span,
                format!("pow requires float operands, got {other:?}"),
            )),
        };
        Ok(Value::Float(as_float(base)?.powf(as_float(exponent)?)))
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
            (Value::EnumVariant(def, _), Type::Enum(target)) => self
                .context
                .variant_parents
                .get(def)
                .is_some_and(|parent| parent == target),
            // A union member's `is <variant>` lowers to the variant's member
            // type (a `Type::Struct` carrying the variant DefId); a variant
            // DefId never collides with a real struct DefId, so the tag
            // comparison cannot false-positive against plain structs.
            (Value::Struct(def, _) | Value::EnumVariant(def, _), Type::Struct(target)) => {
                def == target
            }
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
            || integer_tag_matches(value, target)
            || matches!(
                (value, target),
                (Value::Nil, Type::Primitive(Primitive::Nihil))
                    | (Value::Bool(_), Type::Primitive(Primitive::Bivalens))
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
                    | (
                        Value::Octeti(_),
                        Type::Primitive(Primitive::Octeti)
                            | Type::Array(_)
                            | Type::BoundedOcteti { .. }
                    )
                    | (
                        Value::Array(_),
                        Type::Array(_) | Type::BoundedArray { .. } | Type::RangedArray { .. }
                    )
                    | (Value::Map(_), Type::Map(_, _))
                    | (Value::Ledger(_), Type::Ledger(_, _))
                    | (Value::Set(_), Type::Set(_))
                    | (
                        Value::Instans(_),
                        Type::Primitive(Primitive::Instans) | Type::SizedInstans(_)
                    )
                    | (Value::Regex(_), Type::Primitive(Primitive::Regex))
                    | (Value::Match(_), Type::Primitive(Primitive::Match))
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

    fn eval_tensor_shape_check(
        &mut self,
        check: &MirShapeCheck,
        args: &[MirOperand],
        span: Span,
    ) -> Result<Value, RunnerError> {
        let arguments = args
            .iter()
            .map(|argument| self.eval_operand(argument, span))
            .collect::<Result<Vec<_>, _>>()?;
        for binding in &check.size_bindings {
            // size-type Z1: the size slot is a lift store, so a witness that
            // fails here is the lift failing its `>= 0` check — name it as
            // such (shape-expression relation failures keep their own issue).
            recoverable_shape_expression_value(&binding.witness, &arguments, span).map_err(
                |error| RunnerError::internal_with_issue(span, error.message, "size_lift_negative"),
            )?;
        }
        for relation in &check.relations {
            let left = recoverable_shape_expression_value(&relation.left, &arguments, span)?;
            let right = recoverable_shape_expression_value(&relation.right, &arguments, span)?;
            if left != right {
                return Err(RunnerError::internal_with_issue(
                    span,
                    format!("tensor runtime shape mismatch: {left} != {right}"),
                    "runner_tensor_shape_mismatch",
                ));
            }
        }
        Ok(Value::Unit)
    }

    fn eval_tensor_shape_size(
        &mut self,
        expression: &MirShapeExpr,
        args: &[MirOperand],
        span: Span,
    ) -> Result<Value, RunnerError> {
        let arguments = args
            .iter()
            .map(|argument| self.eval_operand(argument, span))
            .collect::<Result<Vec<_>, _>>()?;
        let extent = recoverable_shape_expression_value(expression, &arguments, span)?;
        Ok(Value::Float(extent as f64))
    }

    fn eval_assert(&mut self, args: &[MirOperand], span: Span) -> Result<Value, RunnerError> {
        let Some(cond) = args.first() else {
            return Err(RunnerError::internal(span, "assert runtime arity"));
        };
        if !self.eval_operand(cond, span)?.as_bool(span)? {
            let message = if let Some(arg) = args.get(1) {
                let value = self.eval_operand(arg, span)?;
                self.prepare_tensor_escape(value, span)?.display(
                    self.interner,
                    self.field_order(),
                    span,
                )?
            } else {
                "assertion failed".to_owned()
            };
            self.host.abort(&message);
        }
        Ok(Value::Unit)
    }

    fn eval_panic(&mut self, args: &[MirOperand], span: Span) -> Result<Value, RunnerError> {
        let [arg] = args else {
            return Err(RunnerError::internal(span, "panic runtime arity"));
        };
        let message = self.eval_operand(arg, span)?;
        // A compiler-managed fault value carries its stable code ahead of the
        // variant display; the variant word and its data stay unchanged.
        let fault_code = match &message {
            Value::EnumVariant(def, _) => self
                .context
                .display_names
                .get(def)
                .and_then(|name| fault_code_for_display_name(name)),
            _ => None,
        };
        let message = self.prepare_tensor_escape(message, span)?;
        let message = message.display(self.interner, self.field_order(), span)?;
        let message = match fault_code {
            Some(code) => format!("{code} {message}"),
            None => message,
        };
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
                    Value::Ledger(cell) => cell.borrow().len(),
                    Value::Set(cell) => cell.borrow().len(),
                    // Q-X1: the element count (rank 0 holds one), not the rank.
                    Value::Tensor(cell) => cell
                        .borrow()
                        .shape
                        .iter()
                        .try_fold(1_usize, |count, extent| {
                            usize::try_from(*extent)
                                .ok()
                                .and_then(|extent| count.checked_mul(extent))
                        })
                        .ok_or_else(|| {
                            RunnerError::internal(span, "tensor element count overflows")
                        })?,
                    Value::Matrix(matrix) => {
                        let borrow = matrix.borrow();
                        usize::from(borrow.rows) * usize::from(borrow.columns)
                    }
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
                let values = self.tensor_logical_values(&tensor, span)?;
                let count = values.iter().try_fold(0_i64, |count, value| match value {
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
                // A list window: a range over a list whose declared result is
                // a list is a copied range, fully in bounds or a failure.
                if let (Value::Array(cell), Type::Array(_)) =
                    (&receiver, self.types.get(return_ty.semantic_id()))
                    && let Some((start, end)) = window_bounds(&index, span)?
                {
                    let items = cell.borrow();
                    let Some((start, end)) = window_inside(start, end, items.len()) else {
                        return Err(window_failure(
                            RunnerError::internal_with_issue(
                                span,
                                list_window_outside(start, end, items.len()),
                                "runner_list_window_out_of_bounds",
                            ),
                            start,
                            end,
                            items.len(),
                        ));
                    };
                    let step = window_step(&index, span)?;
                    return Ok(array_value(
                        items[start..end].iter().step_by(step).cloned().collect(),
                    ));
                }
                // WHY: a `value`/`ignotum` cell admits nihil by name behavior, so a missing key
                // is an optional read even when the declared type is not an option form; a
                // strict cell that cannot hold nihil still fails hard.
                let item = if matches!(
                    self.types.get(return_ty.semantic_id()),
                    Type::Option(_) | Type::Primitive(Primitive::Valor | Primitive::Ignotum)
                ) {
                    Self::optional_collection_index(receiver, &index, span)?
                } else {
                    Self::collection_index(receiver, &index, span)?
                };
                Ok(self.widen_declared_textus(return_ty.semantic_id(), item))
            }
            MirCollectionOp::Contains => {
                let [receiver, needle] = args else {
                    return Err(RunnerError::internal(span, "collection contains arity"));
                };
                let receiver = self.eval_operand(receiver, span)?;
                let needle = self.eval_operand(needle, span)?;
                Ok(Value::Bool(Self::collection_contains(
                    receiver, needle, span,
                )?))
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
                        if let Some(capacity) = self.collection_capacity(receiver_operand, span)?
                            && bytes.len() >= capacity
                        {
                            return Err(RunnerError::internal_with_issue(
                                span,
                                "bounded collection push at capacity",
                                "runner_collection_push_capacity",
                            ));
                        }
                        bytes.push(byte_from_value(&item, span)?);
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
            MirCollectionOp::Compare => {
                let [lhs, rhs] = args else {
                    return Err(RunnerError::internal(span, "collection compare arity"));
                };
                let lhs = self.eval_operand(lhs, span)?;
                let rhs = self.eval_operand(rhs, span)?;
                let ordering = super::value::contract_compare_values(&lhs, &rhs)
                    .ok_or_else(|| RunnerError::internal(span, "compare operand type mismatch"))?;
                Ok(Value::Int(ordering as i64))
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
                let Value::Regex(program) = self.eval_operand(receiver, span)? else {
                    return Err(RunnerError::internal(
                        span,
                        "consentit receiver is not regex",
                    ));
                };
                let Some(text) = as_text_str(self.eval_operand(text, span)?) else {
                    return Err(RunnerError::internal(
                        span,
                        "consentit argument is not textus",
                    ));
                };
                Ok(Value::Bool(program.is_match(&text)))
            }
            MirCollectionOp::RegexFind => self.eval_regex_find(args, span),
            MirCollectionOp::RegexFindAll | MirCollectionOp::RegexSplit => {
                self.eval_regex_bulk(op, args, span)
            }
            MirCollectionOp::RegexReplace => self.eval_regex_replace(args, span),
            MirCollectionOp::RegexReplaceEach => self.eval_regex_replace_each(args, span),
            MirCollectionOp::TextEscape => {
                self.map_str_arg(args, span, |text| super::regex_program::escape(&text))
            }
            MirCollectionOp::MatchText
            | MirCollectionOp::MatchStart
            | MirCollectionOp::MatchEnd
            | MirCollectionOp::MatchGroup
            | MirCollectionOp::MatchNamed => self.eval_match_accessor(op, args, span),
            MirCollectionOp::Split => {
                let [receiver, delim] = args else {
                    return Err(RunnerError::internal(span, "collection split arity"));
                };
                let Some(text) = as_text_str(self.eval_operand(receiver, span)?) else {
                    return Err(RunnerError::internal(span, "split receiver is not textus"));
                };
                let Some(delim) = as_text_str(self.eval_operand(delim, span)?) else {
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
                let Some(text) = as_text_str(self.eval_operand(receiver, span)?) else {
                    return Err(RunnerError::internal(
                        span,
                        "replace receiver is not textus",
                    ));
                };
                let Some(from) = as_text_str(self.eval_operand(from, span)?) else {
                    return Err(RunnerError::internal(span, "replace from is not textus"));
                };
                let Some(to) = as_text_str(self.eval_operand(to, span)?) else {
                    return Err(RunnerError::internal(span, "replace to is not textus"));
                };
                Ok(Value::Str(text.replace(&from, &to)))
            }
            MirCollectionOp::Absolute => self.map_numeric(
                args,
                span,
                return_ty,
                f64::abs,
                i128::abs,
                Some(handroll::Magnus::abs),
            ),
            MirCollectionOp::Signum => {
                if self.resolve_int_width(return_ty).is_decimal() {
                    let [receiver] = args else {
                        return Err(RunnerError::internal(span, "numeric collection arity"));
                    };
                    let value = self.eval_operand(receiver, span)?;
                    super::decimal::signum(&value, span)
                } else {
                    self.map_numeric(
                        args,
                        span,
                        return_ty,
                        f64::signum,
                        i128::signum,
                        Some(|value| handroll::Magnus::from_i64(i64::from(value.signum()))),
                    )
                }
            }
            MirCollectionOp::Minimum => {
                self.numeric_extremum(args, span, return_ty, numeric_min, false)
            }
            MirCollectionOp::Maximum => {
                self.numeric_extremum(args, span, return_ty, numeric_max, true)
            }
            MirCollectionOp::Approximate => self.approximate_fractus(args, span),
            MirCollectionOp::Sum => self.sum_numeric_array(args, return_ty, span),
            MirCollectionOp::ArgMax => self.arg_extremum_index(args, span, true),
            MirCollectionOp::ArgMin => self.arg_extremum_index(args, span, false),
            MirCollectionOp::ArrayAdd => self.array_lift(args, span, MirLiftOp::Add, return_ty),
            MirCollectionOp::ArraySub => self.array_lift(args, span, MirLiftOp::Sub, return_ty),
            MirCollectionOp::ArrayMul => self.array_lift(args, span, MirLiftOp::Mul, return_ty),
            MirCollectionOp::ArrayDiv => self.array_lift(args, span, MirLiftOp::Div, return_ty),
            MirCollectionOp::ArrayMod => self.array_lift(args, span, MirLiftOp::Mod, return_ty),
            MirCollectionOp::ArrayTrueDiv => {
                self.array_lift(args, span, MirLiftOp::TrueDiv, return_ty)
            }
            MirCollectionOp::ArrayNeg => self.array_lift(args, span, MirLiftOp::Neg, return_ty),
            MirCollectionOp::ArrayAnd => self.array_lift(args, span, MirLiftOp::And, return_ty),
            MirCollectionOp::ArrayOr => self.array_lift(args, span, MirLiftOp::Or, return_ty),
            MirCollectionOp::ArrayNot => self.array_lift(args, span, MirLiftOp::Not, return_ty),
            MirCollectionOp::ArrayLt => self.array_lift(args, span, MirLiftOp::Lt, return_ty),
            MirCollectionOp::ArrayGt => self.array_lift(args, span, MirLiftOp::Gt, return_ty),
            MirCollectionOp::ArrayLtEq => self.array_lift(args, span, MirLiftOp::LtEq, return_ty),
            MirCollectionOp::ArrayGtEq => self.array_lift(args, span, MirLiftOp::GtEq, return_ty),
            MirCollectionOp::ArrayValueEq => self.array_lift(args, span, MirLiftOp::Eq, return_ty),
            MirCollectionOp::ArrayValueNotEq => {
                self.array_lift(args, span, MirLiftOp::NotEq, return_ty)
            }
            MirCollectionOp::TensorSumAxis => self.tensor_sum_axis(args, span, return_ty),
            MirCollectionOp::TensorShape => self.tensor_shape(args, span),
            MirCollectionOp::TensorReshape => self.tensor_reshape(args, span),
            MirCollectionOp::TensorGet => self.tensor_get(args, return_ty, span),
            MirCollectionOp::TensorSet => self.tensor_set(args, span),
            MirCollectionOp::TensorFill => self.tensor_fill(args, span),
            MirCollectionOp::TensorCreate => self.tensor_create(args, return_ty, span),
            MirCollectionOp::TensorFromFlat => self.tensor_from_flat(args, return_ty, span),
            MirCollectionOp::TensorFlatten => self.tensor_flatten(args, span),
            MirCollectionOp::TensorMaterialize => self.tensor_materialize(args, span),
            MirCollectionOp::TensorSlice => self.tensor_slice(args, span),
            MirCollectionOp::TensorSliceNd => self.tensor_slice_nd(args, span),
            MirCollectionOp::TensorRank => self.tensor_rank(args, span),
            MirCollectionOp::TensorNorm => self.tensor_norm(args, span),
            MirCollectionOp::TensorExpand => self.tensor_expand(args, span),
            MirCollectionOp::TensorShift => self.tensor_shift(args, span),
            MirCollectionOp::TensorCopyInto => self.tensor_copy_into(args, span),
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
            MirCollectionOp::TensorScatterAdd => self.tensor_scatter_add(args, return_ty, span),
            MirCollectionOp::TensorCausalMaskedSoftmax => {
                self.tensor_causal_masked_softmax(args, span)
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
            MirCollectionOp::TensorMath(function) => {
                self.tensor_math(function, args, span, return_ty)
            }
            MirCollectionOp::ScalarMath(function) => {
                self.scalar_math(function, args, span, return_ty)
            }
            MirCollectionOp::TensorGelu => self.tensor_gelu(args, span),
            MirCollectionOp::TensorRelu => self.tensor_relu(args, span),
            MirCollectionOp::TensorSoftmax => self.tensor_softmax(args, span),
            MirCollectionOp::TensorCruxEntropia => self.tensor_crux_entropia(args, span),
            MirCollectionOp::TensorTranspose => self.tensor_transpose(args, span),
            MirCollectionOp::SparseGet => self.sparse_get(args, span),
            MirCollectionOp::SparseSet => self.sparse_set(args, span),
            MirCollectionOp::Round => self.map_numeric(
                args,
                span,
                return_ty,
                f64::round_ties_even,
                std::convert::identity,
                None,
            ),
            MirCollectionOp::Floor => self.map_numeric(
                args,
                span,
                return_ty,
                f64::floor,
                std::convert::identity,
                None,
            ),
            MirCollectionOp::Ceiling => self.map_numeric(
                args,
                span,
                return_ty,
                f64::ceil,
                std::convert::identity,
                None,
            ),
            MirCollectionOp::Truncate => self.map_numeric(
                args,
                span,
                return_ty,
                f64::trunc,
                std::convert::identity,
                None,
            ),
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
                    Value::Ledger(cell) => {
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
                    Value::Ledger(cell) => {
                        let ledger_key = key.to_key(span)?;
                        Ok(Value::Bool(cell.borrow_mut().remove(&ledger_key).is_some()))
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
        match self.eval_operand(receiver, span)? {
            Value::Tensor(cell) => {
                let shape = cell
                    .borrow()
                    .shape
                    .iter()
                    .copied()
                    .map(Value::Int)
                    .collect();
                Ok(array_value(shape))
            }
            Value::Matrix(matrix) => {
                let borrow = matrix.borrow();
                let shape = vec![
                    Value::Int(i64::from(borrow.rows)),
                    Value::Int(i64::from(borrow.columns)),
                ];
                Ok(array_value(shape))
            }
            other => Err(RunnerError::internal(
                span,
                format!("tensor receiver type mismatch: {other:?}"),
            )),
        }
    }

    fn tensor_reshape(&mut self, args: &[MirOperand], span: Span) -> Result<Value, RunnerError> {
        let [receiver, shape] = args else {
            return Err(RunnerError::internal(span, "tensor reshape arity"));
        };
        let tensor = self.tensor_arg(receiver, span)?;
        let shape = self.shape_arg(shape, span, ERR_FORMA_RESHAPE_COUNT)?;
        let borrow = tensor.borrow();
        let count = borrow
            .logical_len()
            .ok_or_else(|| RunnerError::internal(span, "tensor reshape input layout is invalid"))?;
        Self::ensure_tensor_element_count(&shape, count, span, ERR_FORMA_ELEMENT_COUNT)?;
        let view = borrow.reshaped_view(shape).ok_or_else(|| {
            RunnerError::internal(span, "tensor reshape requires a layout-preserving view")
        })?;
        Ok(Value::Tensor(Rc::new(RefCell::new(view))))
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
        let tensor = match self.eval_operand(receiver, span)? {
            Value::Tensor(cell) => cell,
            // A `matrix` receiver shares the coordinate read: the same index
            // lista contract, the same invalid-index constant, and the same
            // optional-vs-bare miss posture. It is a separate value kind, so
            // it takes its own read helper rather than the tensor views.
            Value::Matrix(matrix) => {
                return self.matrix_tensor_get(&matrix, index, return_ty, span);
            }
            other => {
                return Err(RunnerError::internal(
                    span,
                    format!("tensor receiver type mismatch: {other:?}"),
                ));
            }
        };
        // The index lista is validated and read in place: no extent `Vec`, and
        // no clone of the tensor shape.
        let index = self.index_cells_arg(index, span, ERR_ACCIPE_INVALID_INDEX)?;
        let checked = !matches!(self.types.get(return_ty.semantic_id()), Type::Option(_));
        let located = {
            let borrow = tensor.borrow();
            tensor_flat_offset_cells(&borrow.shape, &index.borrow()).map(|offset| {
                let dense = borrow.is_dense_row_major()
                    && borrow.logical_len().is_some_and(|count| offset < count);
                (offset, dense)
            })
        };
        let Some((offset, dense)) = located else {
            return if checked {
                Err(RunnerError::internal(span, ERR_ACCIPE_INVALID_INDEX))
            } else {
                Ok(Value::Nil)
            };
        };
        // Dense row-major read: the flat offset already is the physical
        // offset, so read the cell directly instead of handing it to
        // `tensor_read_at`, which re-unravels it into coordinates.
        let read = if dense {
            let borrow = tensor.borrow();
            match borrow.read_physical(offset) {
                Some(value) => Some(value),
                // A failed device download is the first error, not a miss.
                None => match borrow.take_residency_error() {
                    Some(error) => return Err(error),
                    None => None,
                },
            }
        } else {
            self.tensor_read_at(&tensor, offset, span)?
        };
        match read {
            Some(value) => Ok(value),
            None if checked => Err(RunnerError::internal(span, ERR_ACCIPE_INVALID_INDEX)),
            None => Ok(Value::Nil),
        }
    }

    /// Coordinate read through a `matrix` receiver. The coordinate contract is
    /// the tensor one (two non-negative cells, the same invalid-index
    /// constant, the same optional-vs-bare miss posture); the read itself is
    /// row-major `row * columns + column` against the shared cell.
    fn matrix_tensor_get(
        &mut self,
        matrix: &Rc<RefCell<MatrixValue>>,
        index: &MirOperand,
        return_ty: MirType,
        span: Span,
    ) -> Result<Value, RunnerError> {
        let index = self.index_cells_arg(index, span, ERR_ACCIPE_INVALID_INDEX)?;
        let checked = !matches!(self.types.get(return_ty.semantic_id()), Type::Option(_));
        let cell = matrix_coordinates(&index.borrow()).and_then(|(row, column)| {
            let borrow = matrix.borrow();
            if row < borrow.rows && column < borrow.columns {
                borrow.cell(row, column).cloned()
            } else {
                None
            }
        });
        match cell {
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
        let shape = tensor.borrow().shape.clone();
        let Some(offset) = tensor_flat_offset(&shape, &index) else {
            return Err(RunnerError::internal(span, ERR_ACCIPE_INVALID_INDEX));
        };
        self.tensor_read_at(&tensor, offset, span)?
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
        let (start, end, inclusive, step) = interval_parts(&interval, span)?;
        // Int-only bounds are not a hidden carrier gap: `intervallum` over an
        // unsigned-64 element type is rejected semantically, so interval
        // bounds can never surface as `UInt` here.
        let (Value::Int(start), Value::Int(end)) = (start, end) else {
            return Err(RunnerError::internal(
                span,
                "interval length requires numerus bounds",
            ));
        };
        let step = interval_step(step.as_ref(), span)?;
        Ok(Value::Int(interval_member_count(
            start, end, inclusive, step,
        )))
    }

    fn interval_contains(&mut self, args: &[MirOperand], span: Span) -> Result<Value, RunnerError> {
        let [interval, value] = args else {
            return Err(RunnerError::internal(span, "interval contains arity"));
        };
        let interval = self.eval_operand(interval, span)?;
        let value = self.eval_operand(value, span)?;
        let (start, end, inclusive, step) = interval_parts(&interval, span)?;
        let step = interval_step(step.as_ref(), span)?;
        let ascending =
            interval_numeric_ordering(&start, &end, span)?.is_some_and(std::cmp::Ordering::is_le);
        let item_from_start = interval_numeric_ordering(&value, &start, span)?;
        let item_from_end = interval_numeric_ordering(&value, &end, span)?;
        let on_window = if ascending {
            item_from_start.is_some_and(std::cmp::Ordering::is_ge)
                && if inclusive {
                    item_from_end.is_some_and(std::cmp::Ordering::is_le)
                } else {
                    item_from_end.is_some_and(std::cmp::Ordering::is_lt)
                }
        } else {
            item_from_start.is_some_and(std::cmp::Ordering::is_le)
                && if inclusive {
                    item_from_end.is_some_and(std::cmp::Ordering::is_ge)
                } else {
                    item_from_end.is_some_and(std::cmp::Ordering::is_gt)
                }
        };
        if !on_window {
            return Ok(Value::Bool(false));
        }
        Ok(Value::Bool(interval_on_step(
            &start, &value, step, ascending, span,
        )?))
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
        let (start, end, inclusive, _step) = interval_parts(&target, span)?;
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
        let (Value::Int(start), Value::Int(end), inclusive, step) =
            interval_parts(&interval, span)?
        else {
            return Err(RunnerError::internal(
                span,
                "interval materialize requires numerus bounds",
            ));
        };
        let step = interval_step(step.as_ref(), span)?;
        let values = interval_members(start, end, inclusive, step, span)?;
        match self.types.get(return_ty.semantic_id()) {
            Type::Array(_) => Ok(array_value(values)),
            Type::Tensor(element, _) => {
                let element = *element;
                let len = i64::try_from(values.len()).map_err(|_| {
                    RunnerError::internal(span, "interval materialization length overflow")
                })?;
                tensor_value_typed(self.types, values, Some(element), vec![len], span)
            }
            _ => Err(RunnerError::internal(
                span,
                "interval materialize result type mismatch",
            )),
        }
    }

    fn word_family_width_of(&self, ty: MirType) -> Option<NumericWidth> {
        match self.types.get(ty.semantic_id()) {
            Type::ModularWord(width) | Type::SaturatingWord(width) => Some(*width),
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
        let tensor = match self.eval_operand(receiver, span)? {
            Value::Tensor(cell) => cell,
            // Stores through a matrix receiver write the shared `Rc`, so the
            // aliasing `ponde` semantics match the tensor path.
            Value::Matrix(matrix) => {
                return self.write_matrix_element(&matrix, index, value, span);
            }
            other => {
                return Err(RunnerError::internal(
                    span,
                    format!("tensor receiver type mismatch: {other:?}"),
                ));
            }
        };
        let index = self.shape_arg(index, span, ERR_PONDE_INVALID_INDEX)?;
        let value = self.eval_operand(value, span)?;
        let borrow = tensor.borrow();
        let Some(offset) = tensor_flat_offset(&borrow.shape, &index) else {
            return Err(RunnerError::internal(span, ERR_PONDE_INVALID_INDEX));
        };
        // `set_value_at` refuses out-of-bounds cells and every edge tensor
        // outright. A plain `offset < data.len()` test would NOT be equivalent:
        // it would break writes through expanded views, whose physical offset
        // is not the logical ordinal.
        borrow
            .set_value_at(offset, value)
            .ok_or_else(|| RunnerError::internal(span, ERR_PONDE_INVALID_INDEX))?;
        Ok(Value::Unit)
    }

    /// Coordinate write through a `matrix` receiver. The store writes the
    /// shared `Rc<RefCell<MatrixValue>>`, so a later read through the same
    /// handle observes the cell; an off-rank or out-of-range coordinate takes
    /// the tensor store's invalid-index error.
    fn write_matrix_element(
        &mut self,
        matrix: &Rc<RefCell<MatrixValue>>,
        index: &MirOperand,
        value: &MirOperand,
        span: Span,
    ) -> Result<Value, RunnerError> {
        let index = self.index_cells_arg(index, span, ERR_PONDE_INVALID_INDEX)?;
        let value = self.eval_operand(value, span)?;
        let written = matrix_coordinates(&index.borrow()).is_some_and(|(row, column)| {
            let mut borrow = matrix.borrow_mut();
            row < borrow.rows
                && column < borrow.columns
                && borrow
                    .cell_mut(row, column)
                    .map(|cell| *cell = value)
                    .is_some()
        });
        if !written {
            return Err(RunnerError::internal(span, ERR_PONDE_INVALID_INDEX));
        }
        Ok(Value::Unit)
    }

    fn tensor_fill(&mut self, args: &[MirOperand], span: Span) -> Result<Value, RunnerError> {
        let [receiver, value] = args else {
            return Err(RunnerError::internal(span, "tensor fill arity"));
        };
        let tensor = self.tensor_arg(receiver, span)?;
        let value = self.eval_operand(value, span)?;
        let values = self.tensor_fill_values(&tensor, value, span)?;
        let shape = tensor.borrow().shape.clone();
        tensor_value_kind(values, tensor_kind(&tensor), shape, span)
    }

    /// Statement posture: the dropped fill result is the in-place mutation
    /// statement — the same law the HIR→Rust emit lane's dropped arm spells
    /// as a direct receiver `reple`. The bound form (`tensor_fill`) builds
    /// a fresh tensor instead of touching the source.
    pub(crate) fn tensor_fill_in_place(
        &mut self,
        args: &[MirOperand],
        span: Span,
    ) -> Result<Value, RunnerError> {
        let [receiver, value] = args else {
            return Err(RunnerError::internal(span, "tensor fill arity"));
        };
        let tensor = self.tensor_arg(receiver, span)?;
        let value = self.eval_operand(value, span)?;
        let values = self.tensor_fill_values(&tensor, value, span)?;
        // Write-through mirror of `tensor_copy_into`: the cloned snapshot
        // shares the flat allocation, so `set_value_at` mutates the receiver.
        let snapshot = tensor.borrow().clone();
        for (index, value) in values.into_iter().enumerate() {
            snapshot.set_value_at(index, value).ok_or_else(|| {
                RunnerError::internal(span, "tensor fill receiver is a lazy edge view")
            })?;
        }
        Ok(Value::Unit)
    }

    /// Filled element values for `value` broadcast across the receiver's
    /// shape (shared by both fill postures).
    fn tensor_fill_values(
        &mut self,
        tensor: &Rc<RefCell<TensorValue>>,
        value: Value,
        span: Span,
    ) -> Result<Vec<Value>, RunnerError> {
        let borrow = tensor.borrow();
        let shape = borrow.shape.clone();
        let count = borrow
            .logical_len()
            .ok_or_else(|| RunnerError::internal(span, "tensor fill input layout is invalid"))?;
        // When the fill value is itself a tensor, broadcast/tile its elements
        // to fill the receiver shape. This supports the backward companion's
        // broadcasts: filling [B,D] with [B] (prefix repeat each element)
        // or with [D] (suffix cycle the block).
        match value {
            Value::Tensor(fill_cell) => {
                let fill_shape = fill_cell.borrow().shape.clone();
                let fill_data = self.tensor_logical_values(&fill_cell, span)?;
                let fill_count = fill_data.len();
                if fill_count == 0 || count % fill_count != 0 {
                    return Err(RunnerError::internal(
                        span,
                        "tensor fill: fill tensor element count must divide receiver element count",
                    ));
                }
                let repeats = count / fill_count;
                if shape.starts_with(&fill_shape) {
                    // Fill shape is a prefix (e.g. [B] → [B,D]):
                    // repeat each element to fill suffix dimensions.
                    Ok(fill_data
                        .iter()
                        .flat_map(|v| std::iter::repeat_n(v, repeats))
                        .cloned()
                        .collect())
                } else {
                    // Fill shape is a suffix (e.g. [D] → [B,D])
                    // or neither prefix nor suffix: cycle the entire block.
                    Ok(fill_data.iter().cycle().take(count).cloned().collect())
                }
            }
            _ => Ok(vec![value; count]),
        }
    }

    fn tensor_create(
        &mut self,
        args: &[MirOperand],
        return_ty: MirType,
        span: Span,
    ) -> Result<Value, RunnerError> {
        let [_receiver, fill, shape] = args else {
            return Err(RunnerError::internal(span, "tensor create arity"));
        };
        let shape = self.shape_arg(shape, span, ERR_CREA_INVALID_SHAPE)?;
        let fill = self.eval_operand(fill, span)?;
        let count = tensor_element_count(&shape, span)?;
        let element = self.tensor_element_type(return_ty);
        tensor_value_typed(self.types, vec![fill; count], element, shape, span)
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
            return self.tensor_from_flat_exact(flat, shape, static_shape, return_ty, span);
        };
        let flat = self.array_arg(flat, span)?;
        let shape = self.conversion_shape_arg(shape, span)?;
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
        let element = self.tensor_element_type(return_ty);
        tensor_value_typed(self.types, data, element, shape, span)
    }

    fn tensor_from_flat_exact(
        &mut self,
        flat: &MirOperand,
        shape: &MirOperand,
        static_shape: Option<Vec<i64>>,
        return_ty: MirType,
        span: Span,
    ) -> Result<Value, RunnerError> {
        let flat = self.array_arg(flat, span)?;
        let shape = self.conversion_shape_arg(shape, span)?;
        let shape = static_shape.unwrap_or(shape);
        Self::ensure_tensor_element_count(
            &shape,
            flat.len(),
            span,
            radix_mir::lista_tensor_conversio::LISTA_TO_TENSOR_STRUCTA_COUNT_MSG,
        )?;
        let element = self.tensor_element_type(return_ty);
        tensor_value_typed(self.types, flat, element, shape, span)
    }

    fn tensor_flatten(&mut self, args: &[MirOperand], span: Span) -> Result<Value, RunnerError> {
        let [receiver] = args else {
            return Err(RunnerError::internal(span, "tensor flatten arity"));
        };
        let tensor = self.tensor_arg(receiver, span)?;
        if tensor.borrow().has_unresolved_edge_shift() {
            return Err(RunnerError::internal(
                span,
                "tensor flatten cannot escape unresolved shifted edges",
            ));
        }
        let data = self.tensor_logical_values(&tensor, span)?;
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
        let shape = tensor.borrow().shape.clone();
        if tensor.borrow().has_unresolved_edge_shift() {
            return Err(RunnerError::internal(
                span,
                "tensor materialize cannot escape unresolved shifted edges",
            ));
        }
        let data = self.tensor_logical_values(&tensor, span)?;
        tensor_value_kind(data, tensor_kind(&tensor), shape, span)
    }

    fn tensor_expand(&mut self, args: &[MirOperand], span: Span) -> Result<Value, RunnerError> {
        let [receiver, target_shape] = args else {
            return Err(RunnerError::internal(span, "tensor expand arity"));
        };
        let target_shape = match self.eval_operand(target_shape, span)? {
            // Device specialization replaces the shape list with a tensor
            // whose exact static shape is validated as the target witness.
            Value::Tensor(witness) => witness.borrow().shape.clone(),
            _ => self.shape_arg(target_shape, span, ERR_CREA_INVALID_SHAPE)?,
        };
        let tensor = self.tensor_arg(receiver, span)?;
        let borrow = tensor.borrow();
        let view = borrow.expanded_view(target_shape).ok_or_else(|| {
            RunnerError::internal(span, "tensor expanded shape is incompatible with receiver")
        })?;
        Ok(Value::Tensor(Rc::new(RefCell::new(view))))
    }

    /// `TensorCopyInto(dest, src)`: write every logical value of `src` through
    /// the destination's strides into the destination's existing storage.
    ///
    /// The source is read in full before the first write, so a source that
    /// shares storage with the destination (a view of it) still copies the
    /// values it had when the statement began. The destination keeps its
    /// allocation identity: every alias of it sees the new values. A shape
    /// mismatch (possible only for runtime extents) is a runner error value
    /// naming the issue, not a panic.
    fn tensor_copy_into(&mut self, args: &[MirOperand], span: Span) -> Result<Value, RunnerError> {
        let [dest, src] = args else {
            return Err(RunnerError::internal(span, "tensor copy into arity"));
        };
        let dest = self.tensor_arg(dest, span)?;
        let src = self.tensor_arg(src, span)?;
        let values = self.tensor_logical_values(&src, span)?;
        let src_shape = src.borrow().shape.clone();
        let dest_snapshot = dest.borrow().clone();
        if dest_snapshot.shape != src_shape {
            return Err(RunnerError::internal(
                span,
                "tensor copy into shape mismatch",
            ));
        }
        if !dest_snapshot.edge_ops.is_empty() {
            return Err(RunnerError::internal(
                span,
                "tensor copy into destination is a lazy view",
            ));
        }
        for (index, value) in values.into_iter().enumerate() {
            dest_snapshot.set_value_at(index, value).ok_or_else(|| {
                RunnerError::internal(span, "tensor copy into destination layout")
            })?;
        }
        Ok(Value::Unit)
    }

    fn tensor_shift(&mut self, args: &[MirOperand], span: Span) -> Result<Value, RunnerError> {
        let [receiver, offsets] = args else {
            return Err(RunnerError::internal(span, "tensor shift arity"));
        };
        let tensor = self.tensor_arg(receiver, span)?;
        let offsets = match self.eval_operand(offsets, span)? {
            Value::Array(offsets) | Value::Vector(offsets) => offsets.borrow().clone(),
            other => {
                return Err(RunnerError::internal(
                    span,
                    format!("tensor shift offsets are not an array or vector: {other:?}"),
                ));
            }
        };
        let shift = offsets
            .into_iter()
            .map(|offset| match offset {
                Value::Int(offset) => Ok(i128::from(offset)),
                other => Err(RunnerError::internal(
                    span,
                    format!("tensor shift offset is not numerus: {other:?}"),
                )),
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut view = tensor.borrow().clone();
        if shift.len() != view.shape.len() {
            return Err(RunnerError::internal(
                span,
                "tensor shift offset rank differs from receiver rank",
            ));
        }
        view.push_edge_shift(super::tensor_edge::TensorEdgeDescriptor::shifted(shift));
        Ok(Value::Tensor(Rc::new(RefCell::new(view))))
    }

    fn tensor_limes(
        &mut self,
        policy: &MirEdgePolicy,
        args: &[MirOperand],
        span: Span,
    ) -> Result<Value, RunnerError> {
        let [receiver] = args else {
            return Err(RunnerError::internal(span, "tensor limes arity"));
        };
        let tensor = self.tensor_arg(receiver, span)?;
        let mut view = tensor.borrow().clone();
        let resolution = match policy {
            MirEdgePolicy::Builtin(policy) => TensorEdgeResolution::Builtin(match policy {
                MirBuiltinEdgePolicy::Clamp => BuiltinEdgePolicy::Clamp,
                MirBuiltinEdgePolicy::Reflect => BuiltinEdgePolicy::Reflect,
                MirBuiltinEdgePolicy::Wrap => BuiltinEdgePolicy::Wrap,
            }),
            MirEdgePolicy::Custom {
                policy_def,
                remap,
                transform,
            } => TensorEdgeResolution::Custom(CustomEdgePolicyId {
                policy_def: *policy_def,
                remap: *remap,
                transform: *transform,
            }),
        };
        if !view.resolve_edge_shift(resolution) {
            return Err(RunnerError::internal(
                span,
                "tensor limes requires an unresolved edge descriptor",
            ));
        }
        Ok(Value::Tensor(Rc::new(RefCell::new(view))))
    }

    /// Invoke an ordinary MIR function with runner values that were produced
    /// by a lazy tensor edge callback. Arguments live in temporary caller
    /// value slots so the normal call path preserves step budgets, deadlines,
    /// frame accounting, and error propagation.
    pub(crate) fn invoke_runtime_values(
        &mut self,
        function: MirFunctionId,
        arguments: Vec<Value>,
        span: Span,
    ) -> Result<Value, RunnerError> {
        let slots = {
            let frame = self
                .frames
                .last_mut()
                .ok_or_else(|| RunnerError::internal(span, "edge callback has no caller frame"))?;
            let mut next = frame
                .values
                .keys()
                .map(|id| id.0)
                .max()
                .map_or(Some(0), |max| max.checked_add(1))
                .ok_or_else(|| RunnerError::internal(span, "edge callback value id overflow"))?;
            let mut slots = Vec::with_capacity(arguments.len());
            for argument in arguments {
                let id = MirValueId(next);
                next = next.checked_add(1).ok_or_else(|| {
                    RunnerError::internal(span, "edge callback value id overflow")
                })?;
                frame.values.insert(id, argument);
                slots.push(id);
            }
            slots
        };
        let operands = slots
            .iter()
            .copied()
            .map(MirOperand::Value)
            .collect::<Vec<_>>();
        let result = self.invoke_call(&MirCallee::Function(function), &operands, span);
        if let Some(frame) = self.frames.last_mut() {
            for slot in &slots {
                frame.values.remove(slot);
            }
        }
        match result? {
            CallOutcome::Ok(value) => Ok(value),
            CallOutcome::Err(value) => Err(RunnerError::internal(
                span,
                format!("tensor edge callback exited through alternate error: {value:?}"),
            )),
        }
    }

    /// Fallible logical read for a tensor that may carry a custom limes policy.
    /// Ordinary, optional, and built-in views use the shared value-level path;
    /// custom remapping and transforms execute only for displaced edge cells.
    pub(crate) fn tensor_read_at(
        &mut self,
        tensor: &Rc<RefCell<TensorValue>>,
        logical_index: usize,
        span: Span,
    ) -> Result<Option<Value>, RunnerError> {
        let snapshot = tensor.borrow().clone();
        if !tensor_has_custom_edge(&snapshot) {
            return match snapshot.value_at(logical_index) {
                Some(value) => Ok(Some(value)),
                None => snapshot.take_residency_error().map_or(Ok(None), Err),
            };
        }
        let Some(coordinate) = snapshot.logical_coordinates(logical_index) else {
            return Ok(None);
        };
        self.tensor_read_edge_chain(&snapshot, snapshot.edge_ops.len(), &coordinate, span)
    }

    fn tensor_read_edge_chain(
        &mut self,
        tensor: &TensorValue,
        operation_count: usize,
        coordinate: &[usize],
        span: Span,
    ) -> Result<Option<Value>, RunnerError> {
        if operation_count == 0 {
            let base = tensor
                .edge_base
                .as_ref()
                .ok_or_else(|| RunnerError::internal(span, "tensor edge base layout is missing"))?;
            let physical = TensorValue::physical_offset_in_layout(
                coordinate,
                &base.shape,
                &base.strides,
                base.offset,
            )
            .ok_or_else(|| RunnerError::internal(span, "tensor edge base offset is invalid"))?;
            return match tensor.read_physical(physical) {
                Some(value) => Ok(Some(value)),
                None => tensor.take_residency_error().map_or(Ok(None), Err),
            };
        }
        let operation = tensor
            .edge_ops
            .get(operation_count - 1)
            .ok_or_else(|| RunnerError::internal(span, "tensor edge operation is missing"))?;
        match operation {
            TensorEdgeOp::View {
                input_shape,
                output_shape,
                inverse,
            } => {
                if !coordinate_fits(coordinate, output_shape) {
                    return Ok(None);
                }
                let mapped =
                    inverse_view_coordinate(coordinate, input_shape, output_shape, inverse)
                        .ok_or_else(|| {
                            RunnerError::internal(span, "tensor edge view mapping is invalid")
                        })?;
                self.tensor_read_edge_chain(tensor, operation_count - 1, &mapped, span)
            }
            TensorEdgeOp::Shift {
                input_shape,
                descriptor,
            } => {
                let extents = tensor_extents(input_shape, span)?;
                let coordinate = coordinates_as_i128(coordinate, span)?;
                match &descriptor.resolution {
                    TensorEdgeResolution::Optional => {
                        let Some(mapped) = super::tensor_edge::resolve_optional(
                            &coordinate,
                            &descriptor.shift,
                            &extents,
                        ) else {
                            return Ok(Some(Value::Nil));
                        };
                        self.tensor_read_edge_chain(tensor, operation_count - 1, &mapped, span)
                    }
                    TensorEdgeResolution::Builtin(policy) => {
                        let Some(mapped) = super::tensor_edge::resolve_builtin(
                            *policy,
                            &coordinate,
                            &descriptor.shift,
                            &extents,
                        ) else {
                            return Ok(None);
                        };
                        self.tensor_read_edge_chain(tensor, operation_count - 1, &mapped, span)
                    }
                    TensorEdgeResolution::Custom(policy) => {
                        let receiver = Value::Struct(
                            policy.policy_def,
                            Rc::new(RefCell::new(FxHashMap::default())),
                        );
                        let runner = RefCell::new(self);
                        let result = resolve_custom_value(
                            &coordinate,
                            &descriptor.shift,
                            &extents,
                            |displaced| {
                                let value = runner.borrow_mut().invoke_runtime_values(
                                    policy.remap,
                                    vec![receiver.clone(), vector_of_i64(displaced)],
                                    span,
                                )?;
                                vector_to_i64(&value, span)
                            },
                            |mapped| {
                                runner
                                    .borrow_mut()
                                    .tensor_read_edge_chain(
                                        tensor,
                                        operation_count - 1,
                                        mapped,
                                        span,
                                    )?
                                    .ok_or_else(|| {
                                        RunnerError::internal(
                                            span,
                                            "resolved tensor edge chain returned no value",
                                        )
                                    })
                            },
                            |displaced, value| {
                                let Some(transform) = policy.transform else {
                                    return Ok(value);
                                };
                                runner.borrow_mut().invoke_runtime_values(
                                    transform,
                                    vec![receiver.clone(), vector_of_i64(displaced), value],
                                    span,
                                )
                            },
                        );
                        result
                            .map(Some)
                            .map_err(|error| custom_edge_error(error, span))
                    }
                }
            }
        }
    }

    pub(crate) fn tensor_logical_values(
        &mut self,
        tensor: &Rc<RefCell<TensorValue>>,
        span: Span,
    ) -> Result<Vec<Value>, RunnerError> {
        let borrow = tensor.borrow();
        if !tensor_has_custom_edge(&borrow) {
            if borrow.has_contiguous_backing_mismatch() {
                return Err(RunnerError::internal(
                    span,
                    "tensor backing-length mismatch",
                ));
            }
            // No custom policy can reenter the runner mid-read, so the
            // values are read through the borrowed tensor; only the
            // custom-edge chain below needs the reentrancy clone.
            return borrow.logical_values().ok_or_else(|| {
                borrow.host_read_error(|| {
                    RunnerError::internal(span, "tensor logical layout is invalid")
                })
            });
        }
        drop(borrow);
        let snapshot = tensor.borrow().clone();
        let count = snapshot
            .logical_len()
            .ok_or_else(|| RunnerError::internal(span, "tensor logical shape overflows"))?;
        (0..count)
            .map(|index| {
                self.tensor_read_at(tensor, index, span)?.ok_or_else(|| {
                    RunnerError::internal(span, "resolved tensor edge returned no value")
                })
            })
            .collect()
    }

    /// Prepare a value for a boundary that cannot execute lazy tensor
    /// callbacks itself (display or a host kernel). Resolved custom edge
    /// tensors are copied at that boundary; ordinary affine and built-in
    /// views keep their current representation because their value-level
    /// logical reader is complete.
    pub(crate) fn prepare_tensor_escape(
        &mut self,
        value: Value,
        span: Span,
    ) -> Result<Value, RunnerError> {
        if !value_needs_tensor_escape_preparation(&value) {
            return Ok(value);
        }
        match value {
            Value::Tensor(tensor) => {
                let snapshot = tensor.borrow().clone();
                if snapshot.has_unresolved_edge_shift() {
                    return Err(RunnerError::internal(
                        span,
                        "unresolved shifted tensor cannot cross this boundary",
                    ));
                }
                let data = self.tensor_logical_values(&tensor, span)?;
                tensor_value_kind(data, tensor_kind(&tensor), snapshot.shape, span)
            }
            Value::Array(items) => {
                let items = items.borrow().clone();
                let items = items
                    .into_iter()
                    .map(|item| self.prepare_tensor_escape(item, span))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Value::Array(Rc::new(RefCell::new(items))))
            }
            Value::Tuple(items) => items
                .into_iter()
                .map(|item| self.prepare_tensor_escape(item, span))
                .collect::<Result<Vec<_>, _>>()
                .map(Value::Tuple),
            Value::Map(items) => {
                let items = items.borrow().clone();
                let mut prepared = FxHashMap::default();
                for (key, item) in items {
                    prepared.insert(key, self.prepare_tensor_escape(item, span)?);
                }
                Ok(Value::Map(Rc::new(RefCell::new(prepared))))
            }
            Value::Ledger(cell) => {
                // A ledger's insertion order is part of its observable surface
                // (O6): rebuild the prepared entries in the same slot order.
                let entries = cell
                    .borrow()
                    .iter()
                    .map(|(key, item)| (key.clone(), item.clone()))
                    .collect::<Vec<_>>();
                let mut prepared = LedgerValue::new();
                for (key, item) in entries {
                    prepared.insert(key, self.prepare_tensor_escape(item, span)?);
                }
                Ok(Value::Ledger(Rc::new(RefCell::new(prepared))))
            }
            Value::Record(fields, order) => {
                let fields = fields.borrow().clone();
                let mut prepared = FxHashMap::default();
                for (field, item) in fields {
                    prepared.insert(field, self.prepare_tensor_escape(item, span)?);
                }
                Ok(Value::Record(Rc::new(RefCell::new(prepared)), order))
            }
            Value::Struct(def, fields) => {
                let fields = fields.borrow().clone();
                let mut prepared = FxHashMap::default();
                for (field, item) in fields {
                    prepared.insert(field, self.prepare_tensor_escape(item, span)?);
                }
                Ok(Value::Struct(def, Rc::new(RefCell::new(prepared))))
            }
            Value::EnumVariant(def, fields) => {
                let fields = fields.borrow().clone();
                let mut prepared = FxHashMap::default();
                for (field, item) in fields {
                    prepared.insert(field, self.prepare_tensor_escape(item, span)?);
                }
                Ok(Value::EnumVariant(def, Rc::new(RefCell::new(prepared))))
            }
            Value::SharedCell(cell) => {
                let value = cell.borrow().clone();
                Ok(Value::SharedCell(Rc::new(RefCell::new(
                    self.prepare_tensor_escape(value, span)?,
                ))))
            }
            Value::Vector(items) => {
                let items = items.borrow().clone();
                let items = items
                    .into_iter()
                    .map(|item| self.prepare_tensor_escape(item, span))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Value::Vector(Rc::new(RefCell::new(items))))
            }
            Value::Matrix(matrix) => {
                let mut matrix = matrix.borrow().clone();
                let values = std::mem::take(&mut matrix.data)
                    .into_iter()
                    .map(|item| self.prepare_tensor_escape(item, span))
                    .collect::<Result<Vec<_>, _>>()?;
                matrix.data = values;
                Ok(Value::Matrix(Rc::new(RefCell::new(matrix))))
            }
            other => Ok(other),
        }
    }

    fn tensor_slice(&mut self, args: &[MirOperand], span: Span) -> Result<Value, RunnerError> {
        let (receiver, start, end, step) = match args {
            [receiver, start, end] => (receiver, start, end, 1_usize),
            [receiver, start, end, step] => {
                let step = self.eval_operand(step, span)?.as_index(span)?;
                (receiver, start, end, step)
            }
            _ => return Err(RunnerError::internal(span, "tensor slice arity")),
        };
        if step == 0 {
            return Err(RunnerError::internal(
                span,
                "tensor slice step must be positive",
            ));
        }
        let tensor = self.tensor_arg(receiver, span)?;
        let start = self.eval_operand(start, span)?.as_index(span)?;
        let end = self.eval_operand(end, span)?.as_index(span)?;
        let borrow = tensor.borrow();
        let axis = borrow
            .shape
            .first()
            .copied()
            .and_then(|dimension| usize::try_from(dimension.max(0)).ok())
            .unwrap_or(0);
        let end = end.min(axis);
        let start = start.min(end);
        let Some(first_stride) = borrow.strides.first().copied() else {
            return Err(RunnerError::internal(
                span,
                "tensor slice requires rank at least one",
            ));
        };
        let length = if start == end {
            0
        } else {
            end.checked_sub(start)
                .and_then(|extent| extent.checked_add(step - 1))
                .map(|extent| extent / step)
                .ok_or_else(|| RunnerError::internal(span, "tensor slice shape overflow"))?
        };
        let mut shape = borrow.shape.clone();
        shape[0] = i64::try_from(length)
            .map_err(|_| RunnerError::internal(span, "tensor slice shape overflow"))?;
        let step_size = step;
        let step = isize::try_from(step_size)
            .map_err(|_| RunnerError::internal(span, "tensor slice step overflow"))?;
        let offset =
            if length == 0 {
                borrow.offset
            } else {
                let start = isize::try_from(start)
                    .map_err(|_| RunnerError::internal(span, "tensor slice offset overflow"))?;
                borrow
                    .offset
                    .checked_add(start.checked_mul(first_stride).ok_or_else(|| {
                        RunnerError::internal(span, "tensor slice offset overflow")
                    })?)
                    .ok_or_else(|| RunnerError::internal(span, "tensor slice offset overflow"))?
            };
        let mut strides = borrow.strides.clone();
        strides[0] = first_stride
            .checked_mul(step)
            .ok_or_else(|| RunnerError::internal(span, "tensor slice stride overflow"))?;
        let mut view = borrow.logical_view(shape, strides, offset).ok_or_else(|| {
            RunnerError::internal(span, "tensor slice view exceeds backing storage")
        })?;
        view.push_edge_view(
            borrow.shape.clone(),
            TensorInverseMap::SliceAxis0 {
                start,
                step: step_size,
            },
        );
        Ok(Value::Tensor(Rc::new(RefCell::new(view))))
    }

    /// `TensorSliceNd(receiver, starts, ends, steps, keeps, inclusives)`: the
    /// ruled multi-axis strided logical view.
    ///
    /// Unlike `tensor_slice`, the bracket dies: no bound is clamped, so an
    /// out-of-range window is `ERR_ACCIPE_INVALID_INDEX` rather than a silently
    /// shortened view.
    fn tensor_slice_nd(&mut self, args: &[MirOperand], span: Span) -> Result<Value, RunnerError> {
        let [receiver, starts, ends, steps, keeps, inclusives] = args else {
            return Err(RunnerError::internal(span, "tensor slice-nd arity"));
        };
        let tensor = self.tensor_arg(receiver, span)?;
        let starts = self.index_cells_arg(starts, span, ERR_ACCIPE_INVALID_INDEX)?;
        let ends = self.index_cells_arg(ends, span, ERR_ACCIPE_INVALID_INDEX)?;
        let steps = self.index_cells_arg(steps, span, ERR_ACCIPE_INVALID_INDEX)?;
        let keeps = self.bivalens_arg(keeps, span)?;
        let inclusives = self.bivalens_arg(inclusives, span)?;
        let starts = self.index_cells_i64(&starts, span)?;
        let ends = self.index_cells_i64(&ends, span)?;
        let steps = self.index_cells_i64(&steps, span)?;
        let borrow = tensor.borrow();
        let rank = borrow.shape.len();
        if starts.len() != rank
            || ends.len() != rank
            || steps.len() != rank
            || keeps.len() != rank
            || inclusives.len() != rank
        {
            return Err(RunnerError::internal(span, ERR_ACCIPE_INVALID_INDEX));
        }
        let mut shape = Vec::with_capacity(rank);
        let mut strides = Vec::with_capacity(rank);
        let mut offset = borrow.offset;
        for axis in 0..rank {
            let step = steps[axis];
            if step < 1 {
                return Err(RunnerError::internal(span, ERR_ACCIPE_INVALID_INDEX));
            }
            let end_eff = ends[axis]
                .checked_add(i64::from(inclusives[axis]))
                .ok_or_else(|| RunnerError::internal(span, "tensor slice-nd shape overflow"))?;
            let start = starts[axis];
            if start > end_eff || end_eff > borrow.shape[axis] {
                return Err(RunnerError::internal(span, ERR_ACCIPE_INVALID_INDEX));
            }
            if !keeps[axis] && start >= borrow.shape[axis] {
                return Err(RunnerError::internal(span, ERR_ACCIPE_INVALID_INDEX));
            }
            let step_stride = isize::try_from(step)
                .map_err(|_| RunnerError::internal(span, "tensor slice-nd stride overflow"))?;
            let axis_stride = borrow.strides[axis];
            offset = offset
                .checked_add(
                    isize::try_from(start)
                        .map_err(|_| {
                            RunnerError::internal(span, "tensor slice-nd offset overflow")
                        })?
                        .checked_mul(axis_stride)
                        .ok_or_else(|| {
                            RunnerError::internal(span, "tensor slice-nd stride overflow")
                        })?,
                )
                .ok_or_else(|| RunnerError::internal(span, "tensor slice-nd offset overflow"))?;
            if keeps[axis] {
                let extent = usize::try_from(end_eff - start)
                    .map_err(|_| RunnerError::internal(span, "tensor slice-nd shape overflow"))?
                    .div_ceil(usize::try_from(step).map_err(|_| {
                        RunnerError::internal(span, "tensor slice-nd shape overflow")
                    })?);
                shape.push(
                    i64::try_from(extent).map_err(|_| {
                        RunnerError::internal(span, "tensor slice-nd shape overflow")
                    })?,
                );
                strides.push(axis_stride.checked_mul(step_stride).ok_or_else(|| {
                    RunnerError::internal(span, "tensor slice-nd stride overflow")
                })?);
            }
        }
        // Gradient inverse maps for this op ride the deferred K6 gradient surface.
        let view = borrow.logical_view(shape, strides, offset).ok_or_else(|| {
            RunnerError::internal(span, "tensor slice-nd view exceeds backing storage")
        })?;
        Ok(Value::Tensor(Rc::new(RefCell::new(view))))
    }

    /// Evaluate one `bivalens` list operand of `tensor_slice_nd`.
    fn bivalens_arg(&mut self, operand: &MirOperand, span: Span) -> Result<Vec<bool>, RunnerError> {
        let value = self.eval_operand(operand, span)?;
        let Value::Array(cell) = &value else {
            return Err(RunnerError::internal(
                span,
                format!("tensor slice-nd lista argument type mismatch: {value:?}"),
            ));
        };
        cell.borrow()
            .iter()
            .map(|element| match element {
                Value::Bool(flag) => Ok(*flag),
                other => Err(RunnerError::internal(
                    span,
                    format!("tensor slice-nd lista argument type mismatch: {other:?}"),
                )),
            })
            .collect()
    }

    /// The arithmetic and unary operators of a numeric `lista` (or queue/stack):
    /// each element is computed exactly and stored under the result element
    /// type (ruling 19(g)); the live lengths of the collection operands pair.
    fn array_lift(
        &mut self,
        args: &[MirOperand],
        span: Span,
        op: MirLiftOp,
        return_ty: MirType,
    ) -> Result<Value, RunnerError> {
        let label = lista_label(op);
        let operands = args
            .iter()
            .map(|arg| self.eval_operand(arg, span))
            .collect::<Result<Vec<_>, _>>()?;
        if operands.len() != op.arity() {
            return Err(RunnerError::internal(span, format!("{label} arity")));
        }
        let (element_ty, bounded) = match self.types.get(return_ty.semantic_id()) {
            Type::Array(element)
            | Type::Queue(element)
            | Type::BoundedQueue { element, .. }
            | Type::Stack(element)
            | Type::BoundedStack { element, .. } => (*element, false),
            Type::BoundedArray { element, .. } | Type::RangedArray { element, .. } => {
                (*element, true)
            }
            _ => {
                return Err(RunnerError::internal(
                    span,
                    format!("{label} result type mismatch"),
                ));
            }
        };
        // The live length is what pairs: two collections must agree on it, a
        // scalar broadcasts. A bounded list reports a mismatch through the
        // recoverable runtime-failure path (D6), like a push at capacity.
        let mut length: Option<usize> = None;
        for operand in &operands {
            let Value::Array(cell) = operand else {
                continue;
            };
            let len = cell.borrow().len();
            if length.is_some_and(|seen| seen != len) {
                return Err(RunnerError::internal_with_issue(
                    span,
                    format!("{label} length mismatch"),
                    if bounded {
                        RUNNER_BOUNDED_LISTA_LENGTH_MISMATCH
                    } else {
                        RUNNER_LISTA_LENGTH_MISMATCH
                    },
                ));
            }
            length = Some(len);
        }
        let Some(length) = length else {
            return Err(RunnerError::internal(
                span,
                format!("{label} operand type mismatch"),
            ));
        };
        // Each element is computed exactly and then stored under the result
        // element type (ruling 19(g)): the range check happens at the store.
        let mut values = Vec::with_capacity(length);
        for index in 0..length {
            let picked = operands
                .iter()
                .map(|operand| match operand {
                    Value::Array(cell) => cell.borrow()[index].clone(),
                    scalar => scalar.clone(),
                })
                .collect::<Vec<_>>();
            values.push(self.lift_element(op, &picked, Some(element_ty), span)?);
        }
        Ok(array_value(values))
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
            | Type::RangedArray { element, .. }
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
                        RUNNER_LISTA_LENGTH_MISMATCH,
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
    ///
    /// Each element is computed exactly (checked only at the 64-bit cap,
    /// ruling 17) and then stored under the result element type's slot
    /// policy (ruling 19).
    fn tensor_elementwise_or_scalar(
        &mut self,
        args: &[MirOperand],
        span: Span,
        label: &'static str,
        op: fn(&Value, &Value, NumericWidth, Span) -> Result<Value, RunnerError>,
        return_ty: MirType,
    ) -> Result<Value, RunnerError> {
        let element = self.tensor_element_type(return_ty);
        self.tensor_elementwise_exact(args, span, label, op, element)
    }

    /// The element type of a tensor or matrix result type.
    fn tensor_element_type(&self, ty: MirType) -> Option<TypeId> {
        match self.types.get(ty.semantic_id()) {
            Type::Tensor(element, _) | Type::Matrix(element, _) => Some(*element),
            _ => None,
        }
    }

    fn tensor_elementwise_exact(
        &mut self,
        args: &[MirOperand],
        span: Span,
        label: &'static str,
        op: fn(&Value, &Value, NumericWidth, Span) -> Result<Value, RunnerError>,
        element: Option<TypeId>,
    ) -> Result<Value, RunnerError> {
        let [left, right] = args else {
            return Err(RunnerError::internal(span, format!("{label} arity")));
        };
        let left_val = self.eval_operand(left, span)?;
        let right_val = self.eval_operand(right, span)?;
        let width = NumericWidth::I64;
        match (&left_val, &right_val) {
            (Value::Tensor(l_cell), Value::Tensor(r_cell)) => {
                let left_shape = l_cell.borrow().shape.clone();
                let right_shape = r_cell.borrow().shape.clone();
                let left_data = self.tensor_logical_values(l_cell, span)?;
                let right_data = self.tensor_logical_values(r_cell, span)?;
                if left_shape != right_shape {
                    // S6-C2: the tensor-tensor elementwise family learns the
                    // bounded rank-extension broadcast (the `addita_bias`
                    // contract): the lower-rank operand is replicated across
                    // the leading axis of the higher-rank operand when its
                    // dims equal the higher operand's trailing dims. Any
                    // other differing pair fails closed with a typed
                    // diagnostic — the broadcast fact must be recorded, never
                    // guessed (A3).
                    let (higher_is_left, higher_shape, lower_shape, higher_data, lower_data) =
                        if left_shape.len() >= right_shape.len() {
                            (true, &left_shape, &right_shape, &left_data, &right_data)
                        } else {
                            (false, &right_shape, &left_shape, &right_data, &left_data)
                        };
                    if higher_shape.len() != lower_shape.len() + 1
                        || lower_shape != &higher_shape[1..]
                    {
                        return Err(RunnerError::internal_with_issue(
                            span,
                            format!(
                                "{label} tensor-tensor shape pair {left_shape:?} + {right_shape:?} is neither same-shape nor a rank-extension broadcast"
                            ),
                            "runner_tensor_tensor_broadcast_unsupported",
                        ));
                    }
                    let rows: usize = usize::try_from(higher_shape[0].max(0)).map_err(|_| {
                        RunnerError::internal(span, format!("{label} broadcast row overflow"))
                    })?;
                    // Stride over the lower-rank operand instead of cloning it
                    // `rows` times. Operand order is preserved because Sub is
                    // not commutative.
                    if lower_data.is_empty() {
                        if !higher_data.is_empty() {
                            return Err(RunnerError::internal(
                                span,
                                format!("{label} backing-length mismatch"),
                            ));
                        }
                        return tensor_value_typed(
                            self.types,
                            Vec::new(),
                            element,
                            higher_shape.clone(),
                            span,
                        );
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
                    let data = self.store_computed_cells(element, data, span)?;
                    return tensor_value_typed(
                        self.types,
                        data,
                        element,
                        higher_shape.clone(),
                        span,
                    );
                }
                // Equal shapes with unequal backing lengths would silently
                // truncate via zip — reject explicitly (PRP-077).
                if left_data.len() != right_data.len() {
                    return Err(RunnerError::internal(
                        span,
                        format!("{label} backing-length mismatch"),
                    ));
                }
                let data = left_data
                    .iter()
                    .zip(right_data.iter())
                    .map(|(a, b)| op(a, b, width, span))
                    .collect::<Result<Vec<_>, _>>()?;
                let data = self.store_computed_cells(element, data, span)?;
                tensor_value_typed(self.types, data, element, left_shape, span)
            }
            (Value::Tensor(cell), scalar) => {
                let shape = cell.borrow().shape.clone();
                let values = self.tensor_logical_values(cell, span)?;
                debug_assert!(!values.is_empty(), "{label} broadcast on empty tensor");
                let data = values
                    .iter()
                    .map(|elem| op(elem, scalar, width, span))
                    .collect::<Result<Vec<_>, _>>()?;
                let data = self.store_computed_cells(element, data, span)?;
                tensor_value_typed(self.types, data, element, shape, span)
            }
            (scalar, Value::Tensor(cell)) => {
                let shape = cell.borrow().shape.clone();
                let values = self.tensor_logical_values(cell, span)?;
                debug_assert!(!values.is_empty(), "{label} broadcast on empty tensor");
                let data = values
                    .iter()
                    .map(|elem| op(scalar, elem, width, span))
                    .collect::<Result<Vec<_>, _>>()?;
                let data = self.store_computed_cells(element, data, span)?;
                tensor_value_typed(self.types, data, element, shape, span)
            }
            _ => op(&left_val, &right_val, width, span),
        }
    }

    fn tensor_mean(&mut self, args: &[MirOperand], span: Span) -> Result<Value, RunnerError> {
        let [receiver] = args else {
            return Err(RunnerError::internal(span, "tensor mean arity"));
        };
        let tensor = self.tensor_arg(receiver, span)?;
        let values = self.tensor_logical_values(&tensor, span)?;
        // The mean is sum / count: an empty source is 0.0 / 0.0, which is NaN.
        let mut sum = 0.0;
        for value in &values {
            sum += numeric_as_f64(value, span)?;
        }
        Ok(Value::Float(sum / values.len() as f64))
    }

    fn tensor_rank(&mut self, args: &[MirOperand], span: Span) -> Result<Value, RunnerError> {
        let [receiver] = args else {
            return Err(RunnerError::internal(span, "tensor rank arity"));
        };
        let tensor = self.tensor_arg(receiver, span)?;
        Ok(Value::Int(tensor.borrow().shape.len() as i64))
    }

    fn tensor_norm(&mut self, args: &[MirOperand], span: Span) -> Result<Value, RunnerError> {
        let [receiver] = args else {
            return Err(RunnerError::internal(span, "tensor norm arity"));
        };
        let tensor = self.tensor_arg(receiver, span)?;
        let kind = tensor_kind(&tensor);
        let values = self.tensor_logical_values(&tensor, span)?;
        let mut sum = 0.0;
        for value in &values {
            let x = numeric_as_f64(value, span)?;
            sum += x * x;
        }
        let norm = sum.sqrt();
        if matches!(kind, Some(DenseKind::F32 | DenseKind::F64))
            || (kind.is_none() && values.iter().any(|v| matches!(v, Value::Float(_))))
        {
            return Ok(Value::Float(norm));
        }
        Ok(match kind {
            Some(DenseKind::U64) => Value::UInt(norm as u64),
            _ => Value::Int(norm as i64),
        })
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
        let values = self.tensor_logical_values(&tensor, span)?;
        let mut values = values.iter();
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

    pub(super) fn map_tensor_unary(
        &mut self,
        tensor: &Rc<RefCell<TensorValue>>,
        span: Span,
        op: &mut dyn FnMut(&Value) -> Result<Value, RunnerError>,
    ) -> Result<Value, RunnerError> {
        let shape = tensor.borrow().shape.clone();
        let values = self.tensor_logical_values(tensor, span)?;
        let data = values.iter().map(&mut *op).collect::<Result<Vec<_>, _>>()?;
        tensor_value_kind(data, tensor_kind(tensor), shape, span)
    }

    fn tensor_neg(&mut self, args: &[MirOperand], span: Span) -> Result<Value, RunnerError> {
        let [receiver] = args else {
            return Err(RunnerError::internal(span, "tensor neg arity"));
        };
        let tensor = self.tensor_arg(receiver, span)?;
        self.map_tensor_unary(&tensor, span, &mut |v| {
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

    /// Elementwise built-in math function over a tensor (gpu-reset rule 15).
    ///
    /// Each element is the scalar function of the same name, IEEE 754 per
    /// element: a domain miss is NaN, an overflow is infinity, `-0.0` and NaN
    /// follow the libm special values. Float results are stored at the result
    /// element width (`f32` rounds through `f32`). Integer `abs` is exact and
    /// stored under the result element type's slot policy (the range rule
    /// types a scalar `abs` of a non-device integer as the smallest holder).
    fn tensor_math(
        &mut self,
        function: MirMathFunction,
        args: &[MirOperand],
        span: Span,
        return_ty: MirType,
    ) -> Result<Value, RunnerError> {
        let [receiver] = args else {
            return Err(RunnerError::internal(span, "tensor math arity"));
        };
        let tensor = self.tensor_arg(receiver, span)?;
        let element = self.tensor_element_type(return_ty);
        let width = element.and_then(|element| effective_width(self.types, element));
        let int_width = width.unwrap_or(NumericWidth::I64);
        let float_width = width
            .filter(|width| width.is_float())
            .unwrap_or(NumericWidth::F64);
        let shape = tensor.borrow().shape.clone();
        let values = self.tensor_logical_values(&tensor, span)?;
        let mut data = Vec::with_capacity(values.len());
        for value in &values {
            let mapped = match value {
                Value::Float(number) => Value::Float(round_float(
                    apply_math_function(function, *number),
                    float_width,
                )),
                Value::Int(_) | Value::UInt(_) if function == MirMathFunction::Abs => {
                    let exact = super::exact_int::int_value(value)
                        .ok_or_else(|| RunnerError::internal(span, "tensor abs element"))?
                        .abs();
                    super::exact_int::exact_carrier(exact, int_width)
                        .ok_or_else(|| RunnerError::internal(span, NUMERUS_OVERFLOW))?
                }
                _ => {
                    return Err(RunnerError::internal(
                        span,
                        format!("tensor {} element type mismatch", function.name()),
                    ));
                }
            };
            data.push(match element {
                Some(element) => self.store_tensor_element(element, mapped, span)?,
                None => mapped,
            });
        }
        tensor_value_typed(self.types, data, element, shape, span)
    }

    /// Built-in math function on a scalar float (`x.sqrt()`, `x.sin()`).
    ///
    /// The same IEEE function the tensor lift applies per element: a domain
    /// miss is NaN, an overflow is infinity, nothing traps. The function runs
    /// unbounded in `f64`; the result is rounded to the result width only
    /// here, where the call's value lands in a typed cell (`f32` rounds
    /// through `f32`).
    fn scalar_math(
        &mut self,
        function: MirMathFunction,
        args: &[MirOperand],
        span: Span,
        return_ty: MirType,
    ) -> Result<Value, RunnerError> {
        let [receiver] = args else {
            return Err(RunnerError::internal(span, "scalar math arity"));
        };
        let number = match self.eval_operand(receiver, span)? {
            Value::Float(number) => number,
            Value::Int(number) => number as f64,
            Value::UInt(number) => number as f64,
            _ => {
                return Err(RunnerError::internal(
                    span,
                    format!("scalar {} receiver type mismatch", function.name()),
                ));
            }
        };
        let width = effective_width(self.types, return_ty.semantic_id())
            .filter(|width| width.is_float())
            .unwrap_or(NumericWidth::F64);
        Ok(Value::Float(round_float(
            apply_math_function(function, number),
            width,
        )))
    }

    fn tensor_gelu(&mut self, args: &[MirOperand], span: Span) -> Result<Value, RunnerError> {
        let [receiver] = args else {
            return Err(RunnerError::internal(span, "tensor gelu arity"));
        };
        let tensor = self.tensor_arg(receiver, span)?;
        self.map_tensor_unary(&tensor, span, &mut |v| match v {
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
        self.map_tensor_unary(&tensor, span, &mut |v| match v {
            Value::Float(f) => Ok(Value::Float(f.max(0.0))),
            Value::Int(i) => Ok(Value::Int((*i).max(0))),
            _ => Err(RunnerError::internal(
                span,
                "tensor relu element type mismatch",
            )),
        })
    }

    /// Softmax: `exp(x_i` - max(x)) / `sum(exp(x_j` - max(x))) with numerical
    /// stability. Operates over the last axis of any rank >= 1 tensor; every
    /// leading axis is a batch.
    fn tensor_softmax(&mut self, args: &[MirOperand], span: Span) -> Result<Value, RunnerError> {
        let [receiver] = args else {
            return Err(RunnerError::internal(span, "tensor softmax arity"));
        };
        let tensor = self.tensor_arg(receiver, span)?;
        let shape = tensor.borrow().shape.clone();

        if shape.is_empty() {
            return Err(RunnerError::internal(
                span,
                "tensor softmax requires non-empty tensor",
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
        let values = self.tensor_logical_values(&tensor, span)?;
        let batch = values.len() / last_dim;

        let mut out_data = Vec::with_capacity(values.len());
        for b in 0..batch {
            let base = b * last_dim;
            let row = row_softmax(&values, base, last_dim, RowSoftmaxAccess::Safe, span)?;
            out_data.extend(row.into_iter().map(Value::Float));
        }
        tensor_value_kind(out_data, tensor_kind(&tensor), shape, span)
    }

    /// Causal masked-row softmax (GI3-1): row `i` of a rank-2 score matrix is
    /// a numerically stable softmax over columns `0..=i`; entries above the
    /// diagonal are exactly zero. No mask tensor operand — the causal
    /// structure is the recipe's semantics.
    fn tensor_causal_masked_softmax(
        &mut self,
        args: &[MirOperand],
        span: Span,
    ) -> Result<Value, RunnerError> {
        let [receiver] = args else {
            return Err(RunnerError::internal(
                span,
                "tensor causal masked softmax arity",
            ));
        };
        let tensor = self.tensor_arg(receiver, span)?;
        let shape = tensor.borrow().shape.clone();
        let [rows, cols] = shape.as_slice() else {
            return Err(RunnerError::internal(
                span,
                "tensor causal masked softmax requires a rank-2 tensor",
            ));
        };
        if *rows <= 0 || *cols <= 0 {
            return Err(RunnerError::internal(
                span,
                "tensor causal masked softmax requires positive dimensions",
            ));
        }
        let (rows, cols) = (*rows as usize, *cols as usize);

        let values = self.tensor_logical_values(&tensor, span)?;
        let mut out_data = Vec::with_capacity(values.len());
        for row_index in 0..rows {
            let visible = (row_index + 1).min(cols);
            let row = row_softmax(
                &values,
                row_index * cols,
                visible,
                RowSoftmaxAccess::Safe,
                span,
            )?;
            out_data.extend(row.into_iter().map(Value::Float));
            for _ in visible..cols {
                out_data.push(Value::Float(0.0));
            }
        }
        tensor_value_kind(out_data, tensor_kind(&tensor), shape, span)
    }

    /// Categorical cross-entropy loss: `-sum(targets * log(softmax(logits) + ε)) / N`.
    /// Loss is normalized by N = last dimension (number of classes), not batch size.
    /// Takes 2 args: logits (receiver) and targets of the same shape, rank 1 or 2.
    /// Returns the scalar loss as a `Value::Float`.
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
        let logits_shape = logits.borrow().shape.clone();
        let targets_shape = targets.borrow().shape.clone();

        let shape = &logits_shape;
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
        if logits_shape != targets_shape {
            return Err(RunnerError::internal(
                span,
                "tensor crux_entropia shape mismatch",
            ));
        }

        let rank = shape.len();
        let last_dim = shape[rank - 1] as usize;
        let logits_values = self.tensor_logical_values(&logits, span)?;
        let target_values = self.tensor_logical_values(&targets, span)?;
        let batch = logits_values.len() / last_dim;
        let eps = 1e-7_f64;

        let mut total_loss = 0.0_f64;
        for b in 0..batch {
            let base = b * last_dim;
            let softmax = row_softmax(
                &logits_values,
                base,
                last_dim,
                RowSoftmaxAccess::Direct,
                span,
            )?;
            // Compute cross-entropy: -sum(targets * log(softmax + eps))
            let mut row_loss = 0.0_f64;
            for (i, softmax_val) in softmax.iter().enumerate() {
                let target = numeric_as_f64(&target_values[base + i], span)?;
                row_loss -= target * (softmax_val + eps).ln();
            }
            total_loss += row_loss;
        }
        // Mean over the batch rows (ruling XENTDIV, 2026-10-08), not over classes.
        // For rank-1 shape [C]: batch = 1.
        // For rank-2 shape [B, C]: batch = B.
        Ok(Value::Float(total_loss / batch as f64))
    }

    /// Matrix multiply / GEMV over runner `Value` tensors.
    ///
    /// Rank-2 receivers support a rank-1 GEMV argument or a rank-2 matrix.
    /// Rank-3 and rank-4 receivers contract with a rank-2+ argument whose
    /// leading batch axes are an exact prefix of the receiver's batch axes.
    /// Views are read in logical row-major order; each result cell is stored
    /// under the return element type's slot policy.
    fn tensor_matmul(
        &mut self,
        args: &[MirOperand],
        span: Span,
        return_ty: MirType,
    ) -> Result<Value, RunnerError> {
        let [left, right] = args else {
            return Err(RunnerError::internal(span, "tensor matmul arity"));
        };
        // Each finished cell is stored under the result element type's slot
        // policy (ruling 19); the dot-product partials are exact.
        let element = self.tensor_element_type(return_ty);
        let left = self.tensor_arg(left, span)?;
        let right = self.tensor_arg(right, span)?;
        let l_shape = left.borrow().shape.clone();
        let r_shape = right.borrow().shape.clone();
        let left_len = left.borrow().logical_len();
        let right_len = right.borrow().logical_len();
        let left_values = self.tensor_logical_values(&left, span)?;
        let right_values = self.tensor_logical_values(&right, span)?;
        if !(2..=4).contains(&l_shape.len()) {
            return Err(RunnerError::internal(span, ERR_MATMUL_RECEIVER_RANK));
        }
        let rhs_rank1 = r_shape.len() == 1 && l_shape.len() == 2;
        if !rhs_rank1 && !(2..=l_shape.len()).contains(&r_shape.len()) {
            return Err(RunnerError::internal(span, ERR_MATMUL_ARGUMENT_RANK));
        }
        let l_batch = &l_shape[..l_shape.len() - 2];
        let r_batch = if rhs_rank1 {
            &[][..]
        } else {
            &r_shape[..r_shape.len() - 2]
        };
        if r_batch.len() > l_batch.len() || l_batch[..r_batch.len()] != *r_batch {
            return Err(RunnerError::internal(span, ERR_MATMUL_INNER_DIMENSION));
        }
        let k_left = l_shape[l_shape.len() - 1];
        let k_right = if rhs_rank1 {
            r_shape[0]
        } else {
            r_shape[r_shape.len() - 2]
        };
        if k_left != k_right {
            return Err(RunnerError::internal(span, ERR_MATMUL_INNER_DIMENSION));
        }
        let m = l_shape[l_shape.len() - 2];
        let n = if rhs_rank1 {
            1
        } else {
            r_shape[r_shape.len() - 1]
        };
        let mut output_shape = l_batch.to_vec();
        if rhs_rank1 {
            output_shape.push(m);
        } else {
            output_shape.extend([m, n]);
        }
        if left_len != Some(left_values.len()) || right_len != Some(right_values.len()) {
            return Err(RunnerError::internal(
                span,
                "matmul tensor layout is invalid",
            ));
        }
        let batch_count = shape_product(l_batch, span)?;
        let rhs_batch_count = shape_product(r_batch, span)?;
        let trailing_repeat = shape_product(&l_batch[r_batch.len()..], span)?;
        let m_usize = dimension_to_usize(m, span)?;
        let k_usize = dimension_to_usize(k_left, span)?;
        let n_usize = dimension_to_usize(n, span)?;
        let left_matrix_len = m_usize
            .checked_mul(k_usize)
            .ok_or_else(|| RunnerError::internal(span, "matmul shape overflow"))?;
        let right_matrix_len = k_usize
            .checked_mul(n_usize)
            .ok_or_else(|| RunnerError::internal(span, "matmul shape overflow"))?;
        let mut cells = Vec::with_capacity(
            batch_count
                .checked_mul(m_usize)
                .and_then(|count| count.checked_mul(n_usize))
                .ok_or_else(|| RunnerError::internal(span, "matmul shape overflow"))?,
        );
        for batch in 0..batch_count {
            let rhs_batch = if r_batch.is_empty() {
                0
            } else {
                batch
                    .checked_div(trailing_repeat)
                    .ok_or_else(|| RunnerError::internal(span, "matmul batch shape is empty"))?
            };
            if rhs_batch >= rhs_batch_count {
                return Err(RunnerError::internal(span, ERR_MATMUL_INNER_DIMENSION));
            }
            let left_start = batch
                .checked_mul(left_matrix_len)
                .ok_or_else(|| RunnerError::internal(span, "matmul offset overflow"))?;
            let right_start = rhs_batch
                .checked_mul(right_matrix_len)
                .ok_or_else(|| RunnerError::internal(span, "matmul offset overflow"))?;
            let left_matrix = &left_values[left_start..left_start + left_matrix_len];
            let right_matrix = &right_values[right_start..right_start + right_matrix_len];
            if let (Some(left_f), Some(right_f)) =
                (extract_f64s(left_matrix), extract_f64s(right_matrix))
            {
                cells.extend(
                    matmul_f64(&left_f, &right_f, m, k_left, n)
                        .into_iter()
                        .map(Value::Float),
                );
            } else if let (Some(left_i), Some(right_i)) =
                (extract_i64s(left_matrix), extract_i64s(right_matrix))
            {
                cells.extend(
                    matmul_i64(&left_i, &right_i, m, k_left, n, span)?
                        .into_iter()
                        .map(Value::Int),
                );
            } else if let (Some(left_u), Some(right_u)) =
                (extract_u64s(left_matrix), extract_u64s(right_matrix))
            {
                cells.extend(
                    matmul_u64(&left_u, &right_u, m, k_left, n, span)?
                        .into_iter()
                        .map(Value::UInt),
                );
            } else {
                cells.extend(matmul_values(
                    left_matrix,
                    right_matrix,
                    m,
                    k_left,
                    n,
                    NumericWidth::I64,
                    span,
                )?);
            }
        }
        let mut cells = self.store_computed_cells(element, cells, span)?;
        // PRECISION (typed dense U2): a matmul over `tensor<f32>` keeps its
        // dot-product cells as unrounded f64 under inferred storage. The cell
        // is born `F32`, so each result rounds to f32 once, here, toward the
        // device's arithmetic.
        if element.and_then(|element| dense_kind_of_type(self.types, element))
            == Some(DenseKind::F32)
        {
            for cell in &mut cells {
                if let Value::Float(value) = cell {
                    *value = f64::from(*value as f32);
                }
            }
        }
        tensor_value_typed(self.types, cells, element, output_shape, span)
    }

    /// Sum across a specified axis: `sum(x, axis)`.
    ///
    /// Backward-only `LayerNorm` VJP helper. Result rank is input rank - 1.
    /// Rank 1 sums to a scalar. Rank 2 with axis 0 sums down the rows to `[cols]`;
    /// the last axis sums across the columns to `[rows]`.
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
        // Partial sums are exact; only the finished result element is stored
        // under its type's slot policy (ruling 19). The element width picks
        // the carrier of an exact finished cell before that one store.
        let element = self
            .tensor_element_type(return_ty)
            .unwrap_or_else(|| return_ty.semantic_id());
        let element_width = effective_width(self.types, element).unwrap_or(NumericWidth::I64);
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
        let shape = input.borrow().shape.clone();
        let values = self.tensor_logical_values(&input, span)?;
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
            let cell: Vec<&Value> = values.iter().collect();
            let sum = sum_axis_cell(&cell, element_width, span)?;
            return self.store_tensor_element(element, sum, span);
        }
        // rank == 2
        let rows = shape[0] as usize;
        let cols = shape[1] as usize;
        if axis == 0 {
            // Sum across rows → result has shape [cols].
            let mut result = Vec::with_capacity(cols);
            for ci in 0..cols {
                let cell: Vec<&Value> = (0..rows).map(|ri| &values[ri * cols + ci]).collect();
                result.push(sum_axis_cell(&cell, element_width, span)?);
            }
            let result = self.store_tensor_elements(element, result, span)?;
            tensor_value_typed(self.types, result, Some(element), vec![cols as i64], span)
        } else {
            // axis == 1 (last axis) — sum across columns → result has shape [rows].
            let mut result = Vec::with_capacity(rows);
            for ri in 0..rows {
                let cell: Vec<&Value> = values[ri * cols..(ri + 1) * cols].iter().collect();
                result.push(sum_axis_cell(&cell, element_width, span)?);
            }
            let result = self.store_tensor_elements(element, result, span)?;
            tensor_value_typed(self.types, result, Some(element), vec![rows as i64], span)
        }
    }

    fn store_tensor_elements(
        &mut self,
        element: TypeId,
        values: Vec<Value>,
        span: Span,
    ) -> Result<Vec<Value>, RunnerError> {
        values
            .into_iter()
            .map(|value| self.store_tensor_element(element, value, span))
            .collect()
    }

    /// Slot-store a freshly computed buffer before it is wrapped in a tensor,
    /// so the built result is never re-read out of its own backing and cloned
    /// back. The op pass completes before the first store runs.
    fn store_computed_cells(
        &mut self,
        element: Option<TypeId>,
        data: Vec<Value>,
        span: Span,
    ) -> Result<Vec<Value>, RunnerError> {
        match element {
            Some(element) => self.store_tensor_elements(element, data, span),
            None => Ok(data),
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
        let shape = input.borrow().shape.clone();
        let input_values = self.tensor_logical_values(&input, span)?;

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

        let total_len = input_values.len();
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
                sum += numeric_as_f64(&input_values[base + c], span)?;
            }
            *mean_slot = sum / norm_dim as f64;
        }

        // --- Compute centered = input - mean (broadcast) ---
        let mut x_centered = Vec::with_capacity(total_len);
        for (row, mean) in means.iter().enumerate() {
            let base = row * norm_dim;
            for c in 0..norm_dim {
                let val = numeric_as_f64(&input_values[base + c], span)?;
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
                let gamma_values = self.tensor_logical_values(&g, span)?;
                let beta_values = self.tensor_logical_values(&b, span)?;
                let mut out = Vec::with_capacity(total_len);
                for row in 0..batch {
                    let base = row * norm_dim;
                    for c in 0..norm_dim {
                        let norm = &normalized[base + c];
                        let g_val = &gamma_values[c];
                        let b_val = &beta_values[c];
                        // norm * gamma + beta
                        let prod = numeric_mul(norm, g_val, width, span)?;
                        let sum = numeric_add(&prod, b_val, width, span)?;
                        out.push(sum);
                    }
                }
                out
            }
            (Some(g), None) => {
                let gamma_values = self.tensor_logical_values(&g, span)?;
                let mut out = Vec::with_capacity(total_len);
                for row in 0..batch {
                    let base = row * norm_dim;
                    for c in 0..norm_dim {
                        let norm = &normalized[base + c];
                        let g_val = &gamma_values[c];
                        out.push(numeric_mul(norm, g_val, width, span)?);
                    }
                }
                out
            }
            _ => normalized,
        };

        tensor_value_kind(output, tensor_kind(&input), shape, span)
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
        let shape = input.borrow().shape.clone();
        let input_values = self.tensor_logical_values(&input, span)?;

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

        let total_len = input_values.len();
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
        let gamma_values = self.tensor_logical_values(&gamma, span)?;

        // rms_norm: y = x·(1/sqrt(mean(x²)+eps))·gamma, no mean subtraction.
        // f64 sum of squares → f32 mean → f32 scale (the in-repo
        // `rms_norm_reference` operation order).
        let mut output = Vec::with_capacity(total_len);
        for row in 0..batch {
            let base = row * norm_dim;
            let mut sum_sq = 0.0f64;
            for c in 0..norm_dim {
                let val = numeric_as_f64(&input_values[base + c], span)?;
                sum_sq += val * val;
            }
            let mean = (sum_sq / norm_dim as f64) as f32;
            let eps_f32 = epsilon as f32;
            let scale = f64::from(1.0f32 / (mean + eps_f32).sqrt());
            for c in 0..norm_dim {
                let x = numeric_as_f64(&input_values[base + c], span)?;
                let g = numeric_as_f64(&gamma_values[c], span)?;
                output.push(Value::Float(x * scale * g));
            }
        }
        tensor_value_kind(output, tensor_kind(&input), shape, span)
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
        let shape = input.borrow().shape.clone();
        let input_values = self.tensor_logical_values(&input, span)?;
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
        let batch = input_values.len() / last_extent;
        if batch == 0 {
            return Err(RunnerError::internal(
                span,
                "tensor rope requires non-empty tensor",
            ));
        }

        let table = self.tensor_arg(table_arg, span)?;
        let table_shape = table.borrow().shape.clone();
        let table_values = self.tensor_logical_values(&table, span)?;
        let expected_table_shape = vec![batch as i64, (dim / 2), 3];
        if table_shape != expected_table_shape {
            return Err(RunnerError::internal(
                span,
                format!("tensor rope table shape {table_shape:?} must be {expected_table_shape:?}"),
            ));
        }

        // llama-arch NORM consecutive-pair, or Qwen/NeoX rotate_half. The
        // table stores [angle, cos(angle), sin(angle)] for each row/pair.
        let half = (dim / 2) as usize;
        let mut output = Vec::with_capacity(input_values.len());
        for row in 0..batch {
            let base = row * last_extent;
            let mut row_data: Vec<Value> = input_values[base..base + last_extent].to_vec();
            for k in 0..half {
                let table_base = (row * half + k) * 3;
                let cos = numeric_as_f64(&table_values[table_base + 1], span)?;
                let sin = numeric_as_f64(&table_values[table_base + 2], span)?;
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
        tensor_value_kind(output, tensor_kind(&input), shape, span)
    }

    fn tensor_gather(&mut self, args: &[MirOperand], span: Span) -> Result<Value, RunnerError> {
        // MIR layout: [table, ids].
        let [table_arg, ids_arg] = args else {
            return Err(RunnerError::internal(span, "tensor gather arity"));
        };
        let table = self.tensor_arg(table_arg, span)?;
        let shape = table.borrow().shape.clone();
        if shape.len() != 2 {
            return Err(RunnerError::internal(
                span,
                "tensor gather requires a rank-2 table receiver",
            ));
        }
        let rows = shape[0];
        // Tensor dims are non-negative i64; clamp via try_from if ever negative.
        let cols = usize::try_from(shape[1]).unwrap_or(0);
        // The ids are a rank-1 `tensor<u32, [n]>`; its flat data is the row
        // id per output row, so the extent is the tensor's own length.
        let ids_tensor = self.tensor_arg(ids_arg, span)?;
        if ids_tensor.borrow().shape.len() != 1 {
            return Err(RunnerError::internal(
                span,
                "tensor gather ids must be a rank-1 tensor",
            ));
        }
        let ids = self.tensor_logical_values(&ids_tensor, span)?;
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
        let table_values = self.tensor_logical_values(&table, span)?;
        let mut output = Vec::with_capacity(indices.len() * cols);
        for index in &indices {
            let base = index * cols;
            output.extend(table_values[base..base + cols].iter().cloned());
        }
        let result_shape = vec![indices.len() as i64, cols as i64];
        tensor_value_kind(output, tensor_kind(&table), result_shape, span)
    }

    /// Zero-initialized row scatter-add, the AIR reverse-AD Gather VJP:
    /// `result[ids[t]] += updates[t]` into an implicit `[V, D]` destination.
    /// The destination row count `V` is the declared result's first extent
    /// (the untouched slots stay zero); `D` is the updates row width.
    fn tensor_scatter_add(
        &mut self,
        args: &[MirOperand],
        return_ty: MirType,
        span: Span,
    ) -> Result<Value, RunnerError> {
        // MIR layout: [ids, updates].
        let [ids_arg, updates_arg] = args else {
            return Err(RunnerError::internal(span, "tensor scatter-add arity"));
        };
        let ids_tensor = self.tensor_arg(ids_arg, span)?;
        if ids_tensor.borrow().shape.len() != 1 {
            return Err(RunnerError::internal(
                span,
                "tensor scatter-add ids must be a rank-1 tensor",
            ));
        }
        let updates = self.tensor_arg(updates_arg, span)?;
        let updates_shape = updates.borrow().shape.clone();
        if updates_shape.len() != 2 {
            return Err(RunnerError::internal(
                span,
                "tensor scatter-add updates must be a rank-2 tensor",
            ));
        }
        let update_rows = usize::try_from(updates_shape[0]).unwrap_or(0);
        let cols = usize::try_from(updates_shape[1]).unwrap_or(0);
        let destination_rows = self.scatter_add_rows(return_ty, span)?;
        let ids = self.tensor_logical_values(&ids_tensor, span)?;
        if ids.len() != update_rows {
            return Err(RunnerError::internal(
                span,
                format!(
                    "tensor scatter-add has {} ids for {update_rows} update rows",
                    ids.len()
                ),
            ));
        }
        let mut indices = Vec::with_capacity(ids.len());
        for value in &ids {
            let Value::Int(id) = value else {
                return Err(RunnerError::internal(
                    span,
                    format!("tensor scatter-add id is not numerus: {value:?}"),
                ));
            };
            if *id < 0 || *id >= destination_rows as i64 {
                return Err(RunnerError::internal(
                    span,
                    format!(
                        "tensor scatter-add id {id} is out of range for destination rows {destination_rows}"
                    ),
                ));
            }
            indices.push(usize::try_from(*id).map_err(|_| {
                RunnerError::internal(
                    span,
                    format!(
                        "tensor scatter-add id {id} is out of range for destination rows {destination_rows}"
                    ),
                )
            })?);
        }
        let kind = tensor_kind(&updates);
        let zero = match kind {
            Some(DenseKind::F32 | DenseKind::F64) => Value::Float(0.0),
            Some(DenseKind::U64) => Value::UInt(0),
            _ => Value::Int(0),
        };
        let add_width = match kind {
            Some(DenseKind::U64) => NumericWidth::U64,
            Some(DenseKind::F32 | DenseKind::F64) => NumericWidth::F64,
            _ => NumericWidth::I64,
        };
        let mut output = vec![zero; destination_rows * cols];
        let update_values = self.tensor_logical_values(&updates, span)?;
        for (row, index) in indices.iter().enumerate() {
            let source = row * cols;
            let target = index * cols;
            for column in 0..cols {
                output[target + column] = numeric_add(
                    &output[target + column],
                    &update_values[source + column],
                    add_width,
                    span,
                )?;
            }
        }
        let result_shape = vec![destination_rows as i64, cols as i64];
        tensor_value_kind(output, kind, result_shape, span)
    }

    /// The `[V, D]` destination row count of a scatter-add is implicit in the
    /// operands, so it comes from the declared result type's first extent.
    fn scatter_add_rows(&self, ty: MirType, span: Span) -> Result<usize, RunnerError> {
        let Type::Tensor(_, shape) = self.types.get(ty.semantic_id()) else {
            return Err(RunnerError::internal(
                span,
                "tensor scatter-add result is not a tensor",
            ));
        };
        let IndexExpr::Tuple(dims) = self.types.get_index(*shape) else {
            return Err(RunnerError::internal(
                span,
                "tensor scatter-add result shape is not a rank-2 tuple",
            ));
        };
        let Some(IndexExpr::Literal(rows)) = dims.first().map(|dim| self.types.get_index(*dim))
        else {
            return Err(RunnerError::internal(
                span,
                "tensor scatter-add result row count is not a static literal",
            ));
        };
        usize::try_from(*rows).map_err(|_| {
            RunnerError::internal(span, "tensor scatter-add result row count overflows usize")
        })
    }

    fn tensor_transpose(&mut self, args: &[MirOperand], span: Span) -> Result<Value, RunnerError> {
        let [tensor] = args else {
            return Err(RunnerError::internal(span, "tensor transpose arity"));
        };
        let tensor = self.tensor_arg(tensor, span)?;
        let borrow = tensor.borrow();
        let input_shape = borrow.shape.clone();
        let mut shape = borrow.shape.clone();
        let mut strides = borrow.strides.clone();
        if shape.len() >= 2 {
            let rank = shape.len();
            shape.swap(rank - 2, rank - 1);
            strides.swap(rank - 2, rank - 1);
        }
        let mut view = borrow
            .logical_view(shape, strides, borrow.offset)
            .ok_or_else(|| RunnerError::internal(span, "tensor transpose view is invalid"))?;
        view.push_edge_view(input_shape, TensorInverseMap::TransposeLastTwo);
        Ok(Value::Tensor(Rc::new(RefCell::new(view))))
    }

    // COLLECTION HELPERS

    /// Read a literal capacity from a direct collection place. MIR values do
    /// not carry their semantic type, so bounded queue/stack/list/octeti push
    /// uses the active function's place declaration as the capacity witness.
    fn collection_capacity(
        &self,
        operand: &MirOperand,
        span: Span,
    ) -> Result<Option<usize>, RunnerError> {
        let MirOperand::Place(place) = operand else {
            return Ok(None);
        };
        let Some(frame) = self.frames.last() else {
            return Ok(None);
        };
        let Some(slots) = self.prepared.slot_types.get(&frame.function) else {
            return Ok(None);
        };
        let mut ty = match &place.base {
            MirPlaceBase::Local(id) => slots.local_ty(*id),
            MirPlaceBase::Temp(id) => slots.temp_ty(*id),
        };
        let Some(mut ty_id) = ty.take() else {
            return Ok(None);
        };
        for projection in &place.projections {
            let mut payload = ty_id;
            loop {
                payload = self.types.peel_access_marker(payload).1;
                match self.types.get(payload) {
                    Type::Alias(_, inner) => payload = *inner,
                    _ => break,
                }
            }
            ty_id = match projection {
                MirProjection::Field(field) => {
                    let def = match self.types.get(payload) {
                        Type::Struct(def) => *def,
                        Type::Applied(base, _) => match self.types.get(*base) {
                            Type::Struct(def) => *def,
                            _ => return Ok(None),
                        },
                        _ => return Ok(None),
                    };
                    let Some(field_ty) = self
                        .context
                        .struct_fields
                        .get(&def)
                        .and_then(|fields| fields.get(field))
                    else {
                        return Ok(None);
                    };
                    field_ty.semantic_id()
                }
                MirProjection::VariantField { variant, field } => {
                    let Some(field_ty) = self
                        .context
                        .variant_fields
                        .get(variant)
                        .and_then(|fields| fields.get(field))
                    else {
                        return Ok(None);
                    };
                    field_ty.semantic_id()
                }
                MirProjection::Index(MirOperand::Constant(MirConstant::Int(position)))
                | MirProjection::ProvenIndex(MirOperand::Constant(MirConstant::Int(position)))
                    if let Type::Tuple(parts) = self.types.get(payload) =>
                {
                    let Ok(position) = usize::try_from(*position) else {
                        return Ok(None);
                    };
                    let Some(part) = parts.get(position) else {
                        return Ok(None);
                    };
                    *part
                }
                MirProjection::Index(_) | MirProjection::ProvenIndex(_) => {
                    match radix_mir::ty::index_projection_result_ty(
                        MirType::semantic(payload),
                        self.types,
                    ) {
                        Ok(projected) => projected.semantic_id(),
                        Err(_) => return Ok(None),
                    }
                }
                MirProjection::SharedCell => payload,
                _ => return Ok(None),
            };
        }
        let capacity = match self.types.get(ty_id) {
            Type::BoundedArray { capacity, .. }
            | Type::BoundedQueue { capacity, .. }
            | Type::BoundedStack { capacity, .. }
            | Type::BoundedOcteti { capacity } => *capacity,
            Type::RangedArray {
                max: Some(capacity),
                ..
            } => {
                return usize::try_from(*capacity).map(Some).map_err(|_| {
                    RunnerError::internal(span, "bounded collection capacity does not fit runner")
                });
            }
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
                let mut items = if values {
                    cell.borrow().values().cloned().collect::<Vec<_>>()
                } else {
                    cell.borrow()
                        .keys()
                        .map(MapKey::to_value)
                        .collect::<Vec<_>>()
                };
                // D19.2/D19.6: tabula order is unspecified; the test-only
                // shuffle (RADIX_RUNNER_SHUFFLE_SEED) perturbs it so a
                // golden cannot quietly pin the runner's FxHashMap order.
                crate::shuffle::shuffle_unordered(&mut items);
                Ok(Value::Array(Rc::new(RefCell::new(items))))
            }
            Value::Ledger(cell) => {
                // O6: a ledger iterates in insertion order on every target;
                // the unordered-map shuffle never applies to it.
                let items = if values {
                    cell.borrow().values().cloned().collect::<Vec<_>>()
                } else {
                    cell.borrow()
                        .keys()
                        .map(MapKey::to_value)
                        .collect::<Vec<_>>()
                };
                Ok(Value::Array(Rc::new(RefCell::new(items))))
            }
            _ => Err(RunnerError::internal(
                span,
                format!("{label} receiver is not map or ledger"),
            )),
        }
    }

    pub(crate) fn collection_index(
        receiver: Value,
        index: &Value,
        span: Span,
    ) -> Result<Value, RunnerError> {
        match receiver {
            Value::Cursor(cursor) => cursor.index(index.as_index(span)?, span),
            Value::Array(cell) => {
                // A semantically octeti local can retain the array carrier
                // after a lista<u8> assignment; normalize its window back to
                // octeti so endian conversion sees the declared source type.
                if let Some((start, end)) = window_bounds(index, span)? {
                    let bytes = cell
                        .borrow()
                        .iter()
                        .map(|item| byte_from_value(item, span))
                        .collect::<Result<Vec<_>, _>>()?;
                    let Some((start, end)) = window_inside(start, end, bytes.len()) else {
                        return Err(window_failure(
                            RunnerError::internal(span, "list<b8> index out of bounds"),
                            start,
                            end,
                            bytes.len(),
                        ));
                    };
                    let step = window_step(index, span)?;
                    return Ok(Value::Octeti(
                        bytes[start..end].iter().step_by(step).copied().collect(),
                    ));
                }
                let idx = index.as_index(span)?;
                let items = cell.borrow();
                items.get(idx).cloned().ok_or_else(|| {
                    index_failure(
                        RunnerError::internal(span, LIST_INDEX_OUT_OF_BOUNDS),
                        idx,
                        items.len(),
                    )
                })
            }
            // A run-time lane select: the checked-index law (the failure is the
            // out-of-range `lista` index failure, text included).
            Value::Vector(lanes) => {
                let idx = index.as_index(span)?;
                let lanes = lanes.borrow();
                lanes.get(idx).cloned().ok_or_else(|| {
                    index_failure(
                        RunnerError::internal(span, LIST_INDEX_OUT_OF_BOUNDS),
                        idx,
                        lanes.len(),
                    )
                })
            }
            Value::Tuple(elems) => {
                let idx = index.as_index(span)?;
                elems.get(idx).cloned().ok_or_else(|| {
                    index_failure(
                        RunnerError::internal(span, "tuple index out of bounds"),
                        idx,
                        elems.len(),
                    )
                })
            }
            Value::Str(text) => {
                // A range index is a char-window read (`textus[lo‥hi]` stays a
                // textus substring). Like a list window, it must lie fully
                // inside the text or the program dies; an empty range at the
                // end is allowed.
                if let Some((start, end)) = window_bounds(index, span)? {
                    let len = text.chars().count();
                    let Some((start, end)) = window_inside(start, end, len) else {
                        return Err(window_failure(
                            RunnerError::internal_with_issue(
                                span,
                                text_window_outside(start, end, len),
                                "runner_text_range_out_of_bounds",
                            ),
                            start,
                            end,
                            len,
                        ));
                    };
                    let step = window_step(index, span)?;
                    let window: String = text
                        .chars()
                        .skip(start)
                        .take(end - start)
                        .step_by(step)
                        .collect();
                    return Ok(Value::Str(window));
                }
                let idx = index.as_index(span)?;
                crate::value::scalar_text_index(&text, idx, span)
                    .map_err(|error| index_failure(error, idx, text.chars().count()))
            }
            Value::Ascii(byte) => {
                let idx = index.as_index(span)?;
                if idx == 0 {
                    Ok(Value::Ascii(byte))
                } else {
                    Err(index_failure(
                        RunnerError::internal(span, TEXT_INDEX_OUT_OF_BOUNDS),
                        idx,
                        1,
                    ))
                }
            }
            Value::Octeti(bytes) => {
                if let Some((start, end)) = window_bounds(index, span)? {
                    let Some((start, end)) = window_inside(start, end, bytes.len()) else {
                        return Err(window_failure(
                            RunnerError::internal(span, "list<b8> index out of bounds"),
                            start,
                            end,
                            bytes.len(),
                        ));
                    };
                    let step = window_step(index, span)?;
                    return Ok(Value::Octeti(
                        bytes[start..end].iter().step_by(step).copied().collect(),
                    ));
                }
                let idx = index.as_index(span)?;
                bytes
                    .get(idx)
                    .map(|byte| Value::Int(i64::from(*byte)))
                    .ok_or_else(|| {
                        index_failure(
                            RunnerError::internal(span, "list<b8> index out of bounds"),
                            idx,
                            bytes.len(),
                        )
                    })
            }
            Value::Map(cell) => {
                let key = index.to_key(span)?;
                cell.borrow()
                    .get(&key)
                    .cloned()
                    .ok_or_else(|| RunnerError::internal(span, MAP_MISSING_KEY))
            }
            Value::Ledger(cell) => {
                let key = index.to_key(span)?;
                cell.borrow()
                    .get(&key)
                    .cloned()
                    .ok_or_else(|| RunnerError::internal(span, MAP_MISSING_KEY))
            }
            // Scalar flat-lane index over a tensor (itera ex element gather);
            // tensors define every lane by construction, so bounds are the
            // only failure mode.
            Value::Tensor(tensor) => {
                let idx = index.as_index(span)?;
                tensor.borrow().value_at(idx).ok_or_else(|| {
                    tensor.borrow().host_read_error(|| {
                        RunnerError::internal(span, "tensor index out of bounds")
                    })
                })
            }
            _ => Err(RunnerError::internal(span, "index receiver type mismatch")),
        }
    }

    fn optional_collection_index(
        receiver: Value,
        index: &Value,
        span: Span,
    ) -> Result<Value, RunnerError> {
        if let Value::Map(cell) = &receiver {
            return Ok(cell
                .borrow()
                .get(&index.to_key(span)?)
                .cloned()
                .unwrap_or(Value::Nil));
        }
        if let Value::Ledger(cell) = &receiver {
            return Ok(cell
                .borrow()
                .get(&index.to_key(span)?)
                .cloned()
                .unwrap_or(Value::Nil));
        }
        let optional_index = match index {
            Value::Int(value) if *value >= 0 => Some(*value as usize),
            Value::Int(_) => None,
            _ => return Self::collection_index(receiver, index, span),
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
                .map_or(Value::Nil, |byte| Value::Int(i64::from(byte)))),
            other => Self::collection_index(other, index, span),
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
        let value = self.eval_operand(operand, span)?;
        let Value::Array(cell) = &value else {
            return Err(RunnerError::internal(
                span,
                format!("tensor lista argument type mismatch: {value:?}"),
            ));
        };
        // Map the borrowed lista straight to extents instead of cloning the
        // whole `Vec<Value>` through `array_arg`.
        cell.borrow()
            .iter()
            .map(|value| shape_element(value, span, negative_message))
            .collect()
    }

    /// Evaluate an index `lista` operand and validate every element exactly as
    /// `shape_arg` does (same errors, same order), but keep the cells instead
    /// of collecting them into a fresh `Vec<i64>`. Every element of the
    /// returned lista is a non-negative `Value::Int`.
    fn index_cells_arg(
        &mut self,
        operand: &MirOperand,
        span: Span,
        negative_message: &'static str,
    ) -> Result<Rc<RefCell<Vec<Value>>>, RunnerError> {
        let value = self.eval_operand(operand, span)?;
        let Value::Array(cell) = value else {
            return Err(RunnerError::internal(
                span,
                format!("tensor lista argument type mismatch: {value:?}"),
            ));
        };
        for element in cell.borrow().iter() {
            shape_element(element, span, negative_message)?;
        }
        Ok(cell)
    }

    /// Flatten an already validated `index_cells_arg` lista into plain `i64`s.
    fn index_cells_i64(
        &self,
        cells: &Rc<RefCell<Vec<Value>>>,
        span: Span,
    ) -> Result<Vec<i64>, RunnerError> {
        cells
            .borrow()
            .iter()
            .map(|value| shape_element(value, span, ERR_ACCIPE_INVALID_INDEX))
            .collect()
    }

    fn conversion_shape_arg(
        &mut self,
        operand: &MirOperand,
        span: Span,
    ) -> Result<Vec<i64>, RunnerError> {
        self.array_arg(operand, span)?
            .into_iter()
            .map(|value| match value {
                Value::Int(dim) if tensor_dim_non_negative(dim) => Ok(dim),
                Value::Int(dim) => {
                    let mut error = RunnerError::internal_with_issue(
                        span,
                        format!("size lift must be non-negative: {dim}"),
                        "size_lift_negative",
                    );
                    error.args = vec![("value".to_owned(), dim.to_string())];
                    Err(error)
                }
                other => Err(RunnerError::internal(
                    span,
                    format!("tensor shape element is not numerus: {other:?}"),
                )),
            })
            .collect()
    }

    fn ensure_tensor_element_count(
        shape: &[i64],
        actual: usize,
        span: Span,
        message: &'static str,
    ) -> Result<(), RunnerError> {
        match tensor_shape_element_count(shape) {
            Some(expected) if expected == actual => Ok(()),
            Some(expected)
                if message
                    == radix_mir::lista_tensor_conversio::LISTA_TO_TENSOR_STRUCTA_COUNT_MSG =>
            {
                // WHY: a length that does not fill the shape is a failed
                // conversion (rule 11: a size mismatch is a recoverable
                // error, never a silent reshape), so it carries the
                // conversion code: `⊥`, `fac … cape` and `⇥` handle it and an
                // unhandled one takes the conversion exit class.
                let mut error = RunnerError::conversion_with_issue(
                    span,
                    format!("tensor conversion expected {expected} elements, got {actual}"),
                    crate::RUNNER_CONVERSION_LENGTH_MISMATCH,
                );
                error.args = vec![
                    ("expected".to_owned(), expected.to_string()),
                    ("actual".to_owned(), actual.to_string()),
                ];
                Err(error)
            }
            Some(_) => Err(RunnerError::internal(span, message)),
            None => Err(RunnerError::internal(span, ERR_ELEMENT_COUNT_OVERFLOW)),
        }
    }

    /// A scalar text index carries the one-byte `Value::Ascii` payload. When the
    /// intrinsic's declared result is plain `textus` (or `textus ∪ nihil`), the
    /// value must be a `Value::Str` so every `textus` consumer (`contains`,
    /// `append`, `+`, ...) sees the representation its static type promises.
    /// A declared `ascii<1>` / `littera` result keeps the compact carrier.
    fn widen_declared_textus(&self, declared: TypeId, value: Value) -> Value {
        let declared = match self.types.get(declared) {
            Type::Option(inner) => *inner,
            _ => declared,
        };
        match value {
            Value::Ascii(byte)
                if matches!(self.types.get(declared), Type::Primitive(Primitive::Textus)) =>
            {
                Value::Str(char::from(byte).to_string())
            }
            other => other,
        }
    }

    fn collection_contains(
        receiver: Value,
        needle: Value,
        span: Span,
    ) -> Result<bool, RunnerError> {
        match receiver {
            Value::Array(cell) => Ok(cell.borrow().iter().any(|item| values_equal(item, &needle))),
            Value::Set(cell) => Ok(cell.borrow().contains(&needle.to_key(span)?)),
            Value::Map(cell) => Ok(cell.borrow().contains_key(&needle.to_key(span)?)),
            Value::Ledger(cell) => Ok(cell.borrow().contains_key(&needle.to_key(span)?)),
            text @ (Value::Str(_) | Value::Ascii(_)) => {
                let (Some(text), Some(needle)) = (as_text_str(text), as_text_str(needle)) else {
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

    /// `regex.inventa(text)`: the leftmost-first match, or `nihil`.
    fn eval_regex_find(&mut self, args: &[MirOperand], span: Span) -> Result<Value, RunnerError> {
        let [receiver, text] = args else {
            return Err(RunnerError::internal(span, "regex inventa arity"));
        };
        let Value::Regex(program) = self.eval_operand(receiver, span)? else {
            return Err(RunnerError::internal(span, "inventa receiver is not regex"));
        };
        let Some(text) = as_text_str(self.eval_operand(text, span)?) else {
            return Err(RunnerError::internal(
                span,
                "inventa argument is not textus",
            ));
        };
        Ok(program
            .find(&text)
            .map_or(Value::Nil, |found| Value::Match(Rc::new(found))))
    }

    /// `regex.collecta(text)` and `regex.divide(text)`: one pass over the text.
    fn eval_regex_bulk(
        &mut self,
        op: MirCollectionOp,
        args: &[MirOperand],
        span: Span,
    ) -> Result<Value, RunnerError> {
        let [receiver, text] = args else {
            return Err(RunnerError::internal(span, "regex bulk operation arity"));
        };
        let Value::Regex(program) = self.eval_operand(receiver, span)? else {
            return Err(RunnerError::internal(span, "bulk receiver is not regex"));
        };
        let Some(text) = as_text_str(self.eval_operand(text, span)?) else {
            return Err(RunnerError::internal(span, "bulk argument is not textus"));
        };
        let items = if matches!(op, MirCollectionOp::RegexFindAll) {
            program
                .find_all(&text)
                .into_iter()
                .map(|found| Value::Match(Rc::new(found)))
                .collect()
        } else {
            program.split(&text).into_iter().map(Value::Str).collect()
        };
        Ok(Value::Array(Rc::new(RefCell::new(items))))
    }

    /// `regex.muta(text, replacement)`: every non-overlapping match replaced by
    /// the literal replacement, in one pass.
    fn eval_regex_replace(
        &mut self,
        args: &[MirOperand],
        span: Span,
    ) -> Result<Value, RunnerError> {
        let [receiver, text, replacement] = args else {
            return Err(RunnerError::internal(span, "regex replace arity"));
        };
        let Value::Regex(program) = self.eval_operand(receiver, span)? else {
            return Err(RunnerError::internal(span, "replace receiver is not regex"));
        };
        let Some(text) = as_text_str(self.eval_operand(text, span)?) else {
            return Err(RunnerError::internal(span, "replace text is not textus"));
        };
        let Some(replacement) = as_text_str(self.eval_operand(replacement, span)?) else {
            return Err(RunnerError::internal(
                span,
                "replace replacement is not textus",
            ));
        };
        Ok(Value::Str(program.replace_all_literal(&text, &replacement)))
    }

    /// Closure `regex.muta(text, closure)`, after the lowered loop has run the
    /// closure once per match: the text with the i-th match replaced by the
    /// i-th result, literally.
    fn eval_regex_replace_each(
        &mut self,
        args: &[MirOperand],
        span: Span,
    ) -> Result<Value, RunnerError> {
        let [receiver, text, replacements] = args else {
            return Err(RunnerError::internal(span, "regex replace-each arity"));
        };
        let Value::Regex(program) = self.eval_operand(receiver, span)? else {
            return Err(RunnerError::internal(
                span,
                "replace-each receiver is not regex",
            ));
        };
        let Some(text) = as_text_str(self.eval_operand(text, span)?) else {
            return Err(RunnerError::internal(
                span,
                "replace-each text is not textus",
            ));
        };
        let Value::Array(items) = self.eval_operand(replacements, span)? else {
            return Err(RunnerError::internal(
                span,
                "replace-each replacements are not a lista",
            ));
        };
        let mut pieces = Vec::new();
        for item in items.borrow().iter() {
            let Value::Str(piece) = item else {
                return Err(RunnerError::internal(
                    span,
                    "replace-each replacement is not textus",
                ));
            };
            pieces.push(piece.clone());
        }
        program
            .replace_each(&text, &pieces)
            .map(Value::Str)
            .ok_or_else(|| RunnerError::internal(span, "replace-each count mismatch"))
    }

    /// The five `match` accessors; offsets are code points.
    fn eval_match_accessor(
        &mut self,
        op: MirCollectionOp,
        args: &[MirOperand],
        span: Span,
    ) -> Result<Value, RunnerError> {
        let Some(receiver) = args.first() else {
            return Err(RunnerError::internal(span, "match accessor arity"));
        };
        let Value::Match(found) = self.eval_operand(receiver, span)? else {
            return Err(RunnerError::internal(
                span,
                "accessor receiver is not match",
            ));
        };
        let position = |value: usize| {
            i64::try_from(value)
                .map(Value::Int)
                .map_err(|_| RunnerError::internal(span, "match offset exceeds numerus"))
        };
        let optional_text =
            |text: Option<&str>| text.map_or(Value::Nil, |text| Value::Str(text.to_owned()));
        match (op, args) {
            (MirCollectionOp::MatchText, [_]) => Ok(Value::Str(found.text.clone())),
            (MirCollectionOp::MatchStart, [_]) => position(found.start),
            (MirCollectionOp::MatchEnd, [_]) => position(found.end),
            (MirCollectionOp::MatchGroup, [_, index]) => {
                let index = match self.eval_operand(index, span)? {
                    Value::Int(index) => usize::try_from(index).ok(),
                    Value::UInt(index) => usize::try_from(index).ok(),
                    _ => {
                        return Err(RunnerError::internal(span, "group index is not numerus"));
                    }
                };
                Ok(optional_text(index.and_then(|index| found.group(index))))
            }
            (MirCollectionOp::MatchNamed, [_, name]) => {
                let Some(name) = as_text_str(self.eval_operand(name, span)?) else {
                    return Err(RunnerError::internal(span, "group name is not textus"));
                };
                Ok(optional_text(found.named(&name)))
            }
            _ => Err(RunnerError::internal(span, "match accessor arity")),
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
        let Some(text) = as_text_str(self.eval_operand(receiver, span)?) else {
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
        let Some(left) = as_text_str(self.eval_operand(left, span)?) else {
            return Err(RunnerError::internal(span, "text pair left is not textus"));
        };
        let Some(right) = as_text_str(self.eval_operand(right, span)?) else {
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
        exact: impl FnOnce(i128) -> i128,
        unbounded: Option<fn(&handroll::Magnus) -> handroll::Magnus>,
    ) -> Result<Value, RunnerError> {
        let [receiver] = args else {
            return Err(RunnerError::internal(span, "numeric collection arity"));
        };
        let value = self.eval_operand(receiver, span)?;
        // An `inf` beyond the 64-bit carriers (`absolutum`, `signum`): exact.
        if let (Value::Magnus(big), Some(unbounded)) = (&value, unbounded) {
            return Ok(crate::inf::from_magnus(unbounded(big)));
        }
        // Integer receivers compute the exact result: `|i64::MIN|` is 2^63,
        // and the result carries the declared result width's carrier.
        if let Some(exact_value) = super::exact_int::int_value(&value) {
            let width = self.resolve_int_width(return_ty);
            return super::exact_int::exact_carrier(exact(exact_value), width)
                .ok_or_else(|| RunnerError::internal(span, NUMERUS_OVERFLOW));
        }
        let numeric = match value {
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
        take_greater: bool,
    ) -> Result<Value, RunnerError> {
        let is_array = matches!(
            self.types.get(return_ty.semantic_id()),
            Type::Array(_) | Type::BoundedArray { .. } | Type::RangedArray { .. }
        );
        let is_tensor = matches!(self.types.get(return_ty.semantic_id()), Type::Tensor(_, _));
        if is_array {
            return self.array_elementwise(args, span, "lista extremum", op, return_ty);
        }
        if is_tensor {
            return self.tensor_elementwise_or_scalar(args, span, "tensor extremum", op, return_ty);
        }
        // `minimus`/`maximus` of two `inf` values is exact (ruling 37: an
        // `inf` is totally ordered); `op` would round a `Magnus` through f64.
        if effective_width(self.types, return_ty.semantic_id()) == Some(NumericWidth::Inf) {
            return self.inf_extremum(args, span, take_greater);
        }
        let [left, right] = args else {
            return Err(RunnerError::internal(span, "numeric pair arity"));
        };
        let left = self.eval_operand(left, span)?;
        let right = self.eval_operand(right, span)?;
        op(&left, &right, NumericWidth::I64, span)
    }

    fn inf_extremum(
        &mut self,
        args: &[MirOperand],
        span: Span,
        take_greater: bool,
    ) -> Result<Value, RunnerError> {
        let [left, right] = args else {
            return Err(RunnerError::internal(span, "numeric pair arity"));
        };
        let left = self.eval_operand(left, span)?;
        let right = self.eval_operand(right, span)?;
        let ordering = super::exact_int::cmp_numeric(&left, &right)
            .ok_or_else(|| RunnerError::internal(span, "numeric pair type mismatch"))?;
        Ok(if ordering.is_ge() == take_greater {
            left
        } else {
            right
        })
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
        let receiver = self.eval_operand(receiver, span)?;
        let values = match receiver {
            Value::Array(cell) => cell.borrow().clone(),
            Value::Tensor(cell) => self.tensor_logical_values(&cell, span)?,
            _ => return Err(RunnerError::internal(span, "sum receiver type mismatch")),
        };

        // Word-family sum: fold exactly and let the store apply the slot
        // policy. "Math happens in the ether, and only at the store do we
        // apply any of the boundary conditions." (operator ruling, restated
        // 2026-10-01/02). A `wrapping<u8>` store of 300 is 44 and a
        // `saturating<u8>` store of 300 is 255, but the sum itself is 300.
        if let Some(width) = self.word_family_width_of(return_ty) {
            let overflow = || RunnerError::internal(span, NUMERUS_OVERFLOW);
            let mut total: i128 = 0;
            for value in &values {
                let v = super::exact_int::int_value(value)
                    .ok_or_else(|| RunnerError::internal(span, "word sum element type mismatch"))?;
                total = total.checked_add(v).ok_or_else(overflow)?;
                if !(super::exact_int::EXACT_MIN..=super::exact_int::EXACT_MAX).contains(&total) {
                    return Err(overflow());
                }
            }
            return super::exact_int::exact_carrier(total, width).ok_or_else(overflow);
        }

        // The integer path is exact (d11-6 rulings 17 and 21): partial sums
        // are held wide and checked only against the 64-bit cap, so
        // `[100, 100, -100]` of `numerus<i8>` sums to 100. The slot policy is
        // applied by the store the typechecker puts after the sum. Only a
        // narrower return type (a device function keeps the element type) is
        // checked at its own width.
        let width =
            effective_width(self.types, return_ty.semantic_id()).unwrap_or(NumericWidth::I64);
        // `summa` over `lista<inf>` (ruling 43) is exact and never a size
        // trap; it runs on the bignum whenever the result is `inf` or an
        // element already is a `Magnus`.
        if (width == NumericWidth::Inf || values.iter().any(|v| matches!(v, Value::Magnus(_))))
            && !values.iter().any(|v| matches!(v, Value::Float(_)))
        {
            let mut total = handroll::Magnus::zero();
            for value in &values {
                let term = crate::inf::to_magnus(value)
                    .ok_or_else(|| RunnerError::internal(span, "sum element type mismatch"))?;
                total = &total + &term;
                if total.exceeds_ceiling(crate::inf::RUNNER_INF_BIT_CEILING) {
                    return Err(crate::inf::exhausted(span, "summa"));
                }
            }
            return Ok(crate::inf::from_magnus(total));
        }
        // A declared float result selects the float fold even when the source
        // has no Float element (an empty `lista<fractus>`): the integer path
        // would return an `Int` zero where the type demands a float zero.
        let float_result = matches!(
            width,
            NumericWidth::F16 | NumericWidth::BF16 | NumericWidth::F32 | NumericWidth::F64
        );
        if !float_result && !values.iter().any(|v| matches!(v, Value::Float(_))) {
            let overflow = || RunnerError::internal(span, NUMERUS_OVERFLOW);
            let mut total: i128 = 0;
            for value in &values {
                let v = super::exact_int::int_value(value)
                    .ok_or_else(|| RunnerError::internal(span, "sum element type mismatch"))?;
                total = total.checked_add(v).ok_or_else(overflow)?;
                if !(super::exact_int::EXACT_MIN..=super::exact_int::EXACT_MAX).contains(&total) {
                    return Err(overflow());
                }
            }
            let value = super::exact_int::exact_carrier(total, width).ok_or_else(overflow)?;
            if matches!(width, NumericWidth::I64 | NumericWidth::U64) {
                return Ok(value);
            }
            return match value {
                Value::Int(v) => width_checked_add(v, 0, width, span).map(Value::Int),
                Value::UInt(v) => uint_width_checked_add(v, 0, width, span).map(Value::UInt),
                other => Ok(other),
            };
        }
        // Float inputs are represented as f64 in the runner, so each f32
        // tensor element widens exactly and the sum rounds only at its store.
        Ok(Value::Float(sum_float_values(values, span)?))
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
                let negated = matches!(self.eval_operand(negated, span)?, Value::Bool(true));
                // inf-ap: an integer pair (bounded, `u64`, `inf`) uses the one
                // exact rule; a float or decimal operand computes in floats.
                if let Some(equal) =
                    self.integer_fuzzy_equality(receiver, other, rel_tol, abs_tol, span)?
                {
                    return Ok(Value::Bool(equal != negated));
                }
                let width = self.fuzzy_comparison_width(receiver, other);
                let receiver = self.fuzzy_operand(receiver, span)?;
                let other = self.fuzzy_operand(other, span)?;
                let rel_tol = numeric_as_f64(&self.eval_operand(rel_tol, span)?, span)?;
                let abs_tol = numeric_as_f64(&self.eval_operand(abs_tol, span)?, span)?;
                let receiver = Self::fuzzy_value(receiver, width);
                let other = Self::fuzzy_value(other, width);
                let rel_tol = Self::fuzzy_value(rel_tol, width);
                let abs_tol = Self::fuzzy_value(abs_tol, width);
                let limit = (rel_tol * receiver.abs().max(other.abs())).max(abs_tol);
                let equal = (receiver - other).abs() <= limit;
                Ok(Value::Bool(if negated { !equal } else { equal }))
            }
            _ => Err(RunnerError::internal(span, "approximate comparison arity")),
        }
    }

    /// `≈` between two integer operands (neither a decimal, by static type):
    /// the exact relative-1e-9 rule of [`crate::inf::fuzzy_equal`], for the
    /// tolerances the lowering emits (`1e-9`, `0`). `None` for any other pair.
    #[expect(
        clippy::float_cmp,
        reason = "exact test for the default tolerance pair, not a closeness check"
    )]
    fn integer_fuzzy_equality(
        &mut self,
        lhs: &MirOperand,
        rhs: &MirOperand,
        rel_tol: &MirOperand,
        abs_tol: &MirOperand,
        span: Span,
    ) -> Result<Option<bool>, RunnerError> {
        let rel = numeric_as_f64(&self.eval_operand(rel_tol, span)?, span)?;
        let abs = numeric_as_f64(&self.eval_operand(abs_tol, span)?, span)?;
        if rel != 1e-9 || abs != 0.0 {
            return Ok(None);
        }
        let left = self.eval_operand(lhs, span)?;
        let right = self.eval_operand(rhs, span)?;
        if self.operand_decimal_width(lhs).is_some() || self.operand_decimal_width(rhs).is_some() {
            return Ok(None);
        }
        Ok(crate::inf::fuzzy_equal(&left, &right))
    }

    /// One `≈` operand as the float it stands for: a decimal carrier is a
    /// scaled integer, so it is unscaled by its static type (ruling 22); an
    /// `inf` converts by ruling 40 (nearest-even, ±∞ beyond range).
    fn fuzzy_operand(&mut self, operand: &MirOperand, span: Span) -> Result<f64, RunnerError> {
        let value = self.eval_operand(operand, span)?;
        if let Some(big) = crate::inf::to_magnus(&value)
            && (matches!(value, Value::Magnus(_)) || self.operand_is_inf(operand))
        {
            return Ok(crate::inf::to_float(&big, NumericWidth::F64));
        }
        if let Value::Int(scaled) = value
            && let Some(scale) = self
                .operand_decimal_width(operand)
                .and_then(crate::decimal_scale_factor)
        {
            return Ok(scaled as f64 / scale as f64);
        }
        numeric_as_f64(&value, span)
    }

    fn fuzzy_value(value: f64, width: NumericWidth) -> f64 {
        if matches!(width, NumericWidth::F64) {
            value
        } else {
            f64::from(value as f32)
        }
    }

    fn fuzzy_comparison_width(&self, lhs: &MirOperand, rhs: &MirOperand) -> NumericWidth {
        let mut saw_f32 = false;
        for operand in [lhs, rhs] {
            let Some(ty) = self.operand_semantic_type(operand) else {
                continue;
            };
            match self.types.get(ty) {
                Type::Primitive(Primitive::Fractus) => return NumericWidth::F64,
                Type::SizedNumeric(Primitive::Fractus, width) | Type::PolicyFloat(_, width) => {
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

    fn operand_semantic_type(&self, operand: &MirOperand) -> Option<TypeId> {
        let MirOperand::Place(place) = operand else {
            return None;
        };
        let frame = self.frames.last()?;
        let slots = self.prepared.slot_types.get(&frame.function)?;
        let ty = match place.base {
            radix_mir::MirPlaceBase::Local(id) => slots.local_ty(id),
            radix_mir::MirPlaceBase::Temp(id) => slots.temp_ty(id),
        }?;
        Some(ty)
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
                Self::optional_collection_index(current, &index, span)
            }
            // WHY: a nil base short-circuits above, so the callee is only
            // invoked when the optional base held a value.
            MirOptionChainLink::Call { callee, args } => {
                match self.invoke_call(callee, args, span)? {
                    CallOutcome::Ok(value) => Ok(value),
                    CallOutcome::Err(_) => Err(RunnerError::internal(
                        span,
                        "uncaught alternate return from option chain call",
                    )),
                }
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
                Self::collection_index(value, &index, span)
            }
            radix_mir::MirProjection::ProvenIndex(index) => {
                let index = self.eval_operand(index, span)?;
                Self::collection_index_proven(value, &index, span)
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
            Value::Record(cell, _) => cell.borrow().get(&field).cloned().ok_or_else(|| {
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

    // PROVIDERS

    fn eval_provider(&mut self, provider: &MirProvider, span: Span) -> Result<Value, RunnerError> {
        let identity = provider_identity(provider, self.interner);
        if let MirProviderKind::Kernel(module) = provider.kind {
            // Raw entry args lower to the legacy `processus.argv` provider, but
            // the runner has no kernel module to dispatch it to. `Host::argumenta`
            // already carries the script arguments, so serve it there instead.
            if module == radix_mir::KernelModule::Processus
                && self.interner.resolve(provider.name) == "argv"
            {
                let argv: Vec<Value> = self
                    .host
                    .argumenta()
                    .map_err(|error| error.with_provider_identity(&identity))?
                    .iter()
                    .map(|argument| Value::Str(argument.clone()))
                    .collect();
                return Ok(Value::Array(Rc::new(RefCell::new(argv))));
            }
            return Err(RunnerError::unsupported(
                span,
                &format!(
                    "kernel {} provider `{}` was deleted; no runner kernel serves it",
                    module.name(),
                    self.interner.resolve(provider.name)
                ),
            )
            .with_provider_identity(&identity));
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

/// Whether an integer carrier passes an `est` test against a `numerus` family
/// target (`int`, `int<w>`, `inf`).
///
/// The tag law (isint-carrier): `valor est inf` holds for every integer. Any
/// bounded integer target (`int`, `int<w>`; widths are erased exactly as in
/// `valor ↦ int<w>`) holds only for an integer inside `i64`, so
/// `est int` agrees with `↦ int` succeeding and a Rust `Valor::Numerus` tag.
/// A `u64` carrier above `i64::MAX` and every `Magnus` are beyond it.
pub(crate) fn integer_tag_matches(value: &Value, target: &Type) -> bool {
    let unbounded = matches!(
        target,
        Type::SizedNumeric(Primitive::Numerus, width) if width.is_unbounded()
    );
    let family = unbounded
        || matches!(
            target,
            Type::Primitive(Primitive::Numerus) | Type::SizedNumeric(Primitive::Numerus, _)
        );
    family
        && match value {
            Value::Int(_) => true,
            Value::UInt(magnitude) => unbounded || i64::try_from(*magnitude).is_ok(),
            Value::Magnus(_) => unbounded,
            _ => false,
        }
}

fn byte_from_value(value: &Value, span: Span) -> Result<u8, RunnerError> {
    match value {
        Value::Int(value) => u8::try_from(*value)
            .map_err(|_| RunnerError::internal(span, "list<b8> append byte out of range")),
        // A `b8` cell holds its unsigned pattern.
        Value::UInt(value) => u8::try_from(*value)
            .map_err(|_| RunnerError::internal(span, "list<b8> append byte out of range")),
        _ => Err(RunnerError::internal(
            span,
            "list<b8> append requires numerus byte",
        )),
    }
}

fn array_value(items: Vec<Value>) -> Value {
    Value::Array(Rc::new(RefCell::new(items)))
}

/// Boxed-storage tensor: the shim for the long tail of construction sites.
fn tensor_value(data: Vec<Value>, shape: Vec<i64>, span: Span) -> Result<Value, RunnerError> {
    tensor_value_kind(data, None, shape, span)
}

/// The single tensor constructor: `kind` is the dense element kind the tensor
/// is born with. The array is typed only when every cell reads back as the
/// exact same `Value` (`DenseStorage::from_values_exact`), so a read never
/// changes; `None` keeps boxed storage.
fn tensor_value_kind(
    data: Vec<Value>,
    kind: Option<DenseKind>,
    shape: Vec<i64>,
    span: Span,
) -> Result<Value, RunnerError> {
    let tensor =
        TensorValue::try_contiguous_typed(DenseStorage::from_values_exact(data, kind), shape)
            .ok_or_else(|| {
                RunnerError::internal(span, "tensor shape has unrepresentable strides")
            })?;
    Ok(Value::Tensor(Rc::new(RefCell::new(tensor))))
}

/// Tensor born typed from its declared element type (`None` or a non-scalar
/// element stays boxed).
pub(crate) fn tensor_value_typed(
    types: &TypeTable,
    data: Vec<Value>,
    element: Option<TypeId>,
    shape: Vec<i64>,
    span: Span,
) -> Result<Value, RunnerError> {
    let kind = element.and_then(|element| dense_kind_of_type(types, element));
    tensor_value_kind(data, kind, shape, span)
}

/// Dense kind of an existing tensor's backing, for results that keep the
/// element type of their input.
fn tensor_kind(tensor: &Rc<RefCell<TensorValue>>) -> Option<DenseKind> {
    tensor.borrow().data.kind()
}

/// One validated extent of a shape or index `lista`.
fn shape_element(
    value: &Value,
    span: Span,
    negative_message: &'static str,
) -> Result<i64, RunnerError> {
    match value {
        Value::Int(dim) if tensor_dim_non_negative(*dim) => Ok(*dim),
        Value::Int(_) => Err(RunnerError::internal(span, negative_message)),
        other => Err(RunnerError::internal(
            span,
            format!("tensor shape element is not numerus: {other:?}"),
        )),
    }
}

/// Two non-negative coordinates from an index already validated by
/// `index_cells_arg` (every cell a non-negative `Value::Int`); `None` for any
/// other rank, or for a coordinate beyond the `u8` row/column extent.
fn matrix_coordinates(index: &[Value]) -> Option<(u8, u8)> {
    let [Value::Int(row), Value::Int(column)] = index else {
        return None;
    };
    Some((u8::try_from(*row).ok()?, u8::try_from(*column).ok()?))
}

/// `tensor_flat_offset` over an index already validated by `index_cells_arg`
/// (every cell a non-negative `Value::Int`), without materialising `Vec<i64>`.
fn tensor_flat_offset_cells(shape: &[i64], index: &[Value]) -> Option<usize> {
    if shape.len() != index.len() {
        return None;
    }
    let mut offset = 0_usize;
    let mut stride = 1_usize;
    for (dim, idx) in shape.iter().zip(index.iter()).rev() {
        let Value::Int(idx) = idx else {
            return None;
        };
        let dim = usize::try_from(*dim).ok()?;
        let idx = usize::try_from(*idx).ok()?;
        if idx >= dim {
            return None;
        }
        offset = offset.checked_add(idx.checked_mul(stride)?)?;
        stride = stride.checked_mul(dim)?;
    }
    Some(offset)
}

fn interval_value(start: Value, end: Value, inclusive: bool) -> Value {
    Value::Tuple(vec![start, end, Value::Bool(inclusive)])
}

/// Three-way interval ordering without rounding integer carriers through `f64`.
fn interval_numeric_ordering(
    left: &Value,
    right: &Value,
    span: Span,
) -> Result<Option<std::cmp::Ordering>, RunnerError> {
    let is_numeric =
        |value: &Value| matches!(value, Value::Int(_) | Value::UInt(_) | Value::Float(_));
    if !is_numeric(left) || !is_numeric(right) {
        return Err(RunnerError::internal(
            span,
            "tensor numeric element type mismatch",
        ));
    }
    Ok(super::exact_int::cmp_numeric(left, right))
}

/// F-082: whether interval bounds run backwards (`start > end`).
fn interval_bounds_reversed(start: &Value, end: &Value) -> bool {
    match (start, end) {
        (Value::Int(a), Value::Int(b)) => a > b,
        (Value::UInt(a), Value::UInt(b)) => a > b,
        (Value::Float(a), Value::Float(b)) => a > b,
        // `inf` bounds beyond the 64-bit carriers: exact order.
        (Value::Magnus(_), Value::Int(_) | Value::UInt(_) | Value::Magnus(_))
        | (Value::Int(_) | Value::UInt(_), Value::Magnus(_)) => {
            super::exact_int::cmp_numeric(start, end).is_some_and(std::cmp::Ordering::is_gt)
        }
        _ => false,
    }
}

/// The positive step of a stored interval construct: `1` when the aggregate
/// carries no step, the stored fourth field otherwise. The direction of a
/// stepped range comes from its bounds, so a zero or negative step dies rather
/// than flipping the walk.
fn interval_step(step: Option<&Value>, span: Span) -> Result<i64, RunnerError> {
    let Some(step) = step else {
        return Ok(1);
    };
    let positive = match step {
        Value::Int(step) => Some(*step).filter(|step| *step > 0),
        Value::UInt(step) => i64::try_from(*step).ok().filter(|step| *step > 0),
        _ => None,
    };
    positive.ok_or_else(|| {
        RunnerError::internal_with_issue(
            span,
            "interval step must be positive",
            "runner_interval_step_not_positive",
        )
    })
}

/// How many members a stepped interval holds. The span is the distance
/// between the bounds, so it is direction-blind and never negative; the step
/// is the stride, so an exclusive end contributes only whole strides.
fn interval_member_count(start: i64, end: i64, inclusive: bool, step: i64) -> i64 {
    let span = (i128::from(end) - i128::from(start)).unsigned_abs();
    let stride = u128::try_from(step).unwrap_or(1);
    let count = if inclusive {
        span / stride + 1
    } else {
        span.div_ceil(stride)
    };
    i64::try_from(count).unwrap_or(i64::MAX)
}

/// Whether `item` lands on a stride of the interval beginning at `start`.
/// Direction comes from the bounds, so a descending span negates the offset
/// before the remainder test. An on-step floating member is an exact
/// remainder-zero hit, matching the carrier's arithmetic; the zero comparison
/// is a literal, so `clippy::float_cmp` stays quiet.
fn interval_on_step(
    start: &Value,
    item: &Value,
    step: i64,
    ascending: bool,
    span: Span,
) -> Result<bool, RunnerError> {
    if let (Value::Int(start), Value::Int(item)) = (start, item) {
        let offset = i128::from(*item) - i128::from(*start);
        let offset = if ascending { offset } else { -offset };
        return Ok(offset.rem_euclid(i128::from(step)) == 0);
    }
    let start = numeric_as_f64(start, span)?;
    let item = numeric_as_f64(item, span)?;
    let offset = if ascending {
        item - start
    } else {
        start - item
    };
    Ok(offset.rem_euclid(step as f64) == 0.0)
}

/// The members a stepped interval lists, from `start` toward `end`. The count
/// is fixed first, so an arithmetic overflow on the stride is a length error
/// rather than a silently dropped final member (F-087).
fn interval_members(
    start: i64,
    end: i64,
    inclusive: bool,
    step: i64,
    span: Span,
) -> Result<Vec<Value>, RunnerError> {
    let count = interval_member_count(start, end, inclusive, step);
    let ascending = start <= end;
    let mut members = Vec::new();
    let mut current = start;
    for index in 0..count {
        members.push(Value::Int(current));
        if index + 1 == count {
            break;
        }
        current = if ascending {
            current.checked_add(step)
        } else {
            current.checked_sub(step)
        }
        .ok_or_else(|| RunnerError::internal(span, "interval materialization length overflow"))?;
    }
    Ok(members)
}

/// The positive stride of a range index: `1` for an unstepped window, the
/// `per` step of `s[a‥b per n]` otherwise. A zero or negative step dies.
fn window_step(index: &Value, span: Span) -> Result<usize, RunnerError> {
    let Value::Tuple(parts) = index else {
        return Ok(1);
    };
    let Some(step) = parts.get(3) else {
        return Ok(1);
    };
    let positive = match step {
        Value::Int(step) => usize::try_from(*step).ok().filter(|step| *step > 0),
        Value::UInt(step) => usize::try_from(*step).ok().filter(|step| *step > 0),
        _ => None,
    };
    positive.ok_or_else(|| {
        RunnerError::internal_with_issue(
            span,
            "range step must be positive",
            "runner_range_step_not_positive",
        )
    })
}

/// The `[start, end)` window a range index selects, or `None` when `index`
/// is not a range. An inclusive range ending at `usize::MAX` reports `overflow`.
/// Attach the structured facts of an out-of-bounds window read (`start`, `end`,
/// `len`) so a caller keys on the numbers, never on the rendered message.
fn window_failure(mut error: RunnerError, start: i128, end: i128, len: usize) -> RunnerError {
    error.args.extend([
        ("start".to_owned(), start.to_string()),
        ("end".to_owned(), end.to_string()),
        ("len".to_owned(), len.to_string()),
    ]);
    error
}

/// Attach the structured facts of an out-of-bounds scalar index read (`index`,
/// `len`).
fn index_failure(mut error: RunnerError, index: usize, len: usize) -> RunnerError {
    error.args.extend([
        ("index".to_owned(), index.to_string()),
        ("len".to_owned(), len.to_string()),
    ]);
    error
}

fn interval_parts(
    value: &Value,
    span: Span,
) -> Result<(Value, Value, bool, Option<Value>), RunnerError> {
    let Value::Tuple(parts) = value else {
        return Err(RunnerError::internal(
            span,
            "interval value carrier mismatch",
        ));
    };
    let ([start, end, Value::Bool(inclusive)] | [start, end, Value::Bool(inclusive), _]) =
        parts.as_slice()
    else {
        return Err(RunnerError::internal(span, "interval value shape mismatch"));
    };
    Ok((
        start.clone(),
        end.clone(),
        *inclusive,
        parts.get(3).cloned(),
    ))
}

fn interval_intersection(
    left: &Value,
    right: &Value,
    span: Span,
) -> Result<Option<Value>, RunnerError> {
    let (left_start, left_end, left_inclusive, _step) = interval_parts(left, span)?;
    let (right_start, right_end, right_inclusive, _step) = interval_parts(right, span)?;
    let start_order = interval_numeric_ordering(&left_start, &right_start, span)?;
    let end_order = interval_numeric_ordering(&left_end, &right_end, span)?;
    let start = if start_order.is_some_and(std::cmp::Ordering::is_ge) {
        left_start
    } else {
        right_start
    };
    let (end, inclusive) = if end_order == Some(std::cmp::Ordering::Less) {
        (left_end, left_inclusive)
    } else if end_order == Some(std::cmp::Ordering::Greater) {
        (right_end, right_inclusive)
    } else {
        (left_end, left_inclusive && right_inclusive)
    };
    let bounds_order = interval_numeric_ordering(&start, &end, span)?;
    Ok((bounds_order.is_some_and(std::cmp::Ordering::is_lt)
        || (inclusive && bounds_order == Some(std::cmp::Ordering::Equal)))
    .then(|| interval_value(start, end, inclusive)))
}

fn interval_union_value(
    left: &Value,
    right: &Value,
    span: Span,
) -> Result<Option<Value>, RunnerError> {
    let (left_start, left_end, left_inclusive, _step) = interval_parts(left, span)?;
    let (right_start, right_end, right_inclusive, _step) = interval_parts(right, span)?;
    let start_order = interval_numeric_ordering(&left_start, &right_start, span)?;
    let (start, first_end, first_inclusive, second_start, second_end, second_inclusive) =
        if start_order.is_some_and(std::cmp::Ordering::is_le) {
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
    if interval_numeric_ordering(&second_start, &first_end, span)?
        .is_some_and(std::cmp::Ordering::is_gt)
    {
        return Ok(None);
    }
    let end_order = interval_numeric_ordering(&first_end, &second_end, span)?;
    let (end, inclusive) = if end_order.is_some_and(std::cmp::Ordering::is_lt) {
        (second_end, second_inclusive)
    } else if end_order.is_some_and(std::cmp::Ordering::is_gt) {
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

fn tensor_extents(shape: &[i64], span: Span) -> Result<Vec<usize>, RunnerError> {
    shape
        .iter()
        .map(|dimension| {
            usize::try_from(*dimension)
                .map_err(|_| RunnerError::internal(span, "tensor edge has a negative extent"))
        })
        .collect()
}

fn coordinates_as_i128(coordinates: &[usize], span: Span) -> Result<Vec<i128>, RunnerError> {
    coordinates
        .iter()
        .map(|coordinate| {
            i128::try_from(*coordinate)
                .map_err(|_| RunnerError::internal(span, "tensor edge coordinate overflow"))
        })
        .collect()
}

fn vector_of_i64(coordinates: &[i64]) -> Value {
    Value::Vector(Rc::new(RefCell::new(
        coordinates.iter().copied().map(Value::Int).collect(),
    )))
}

fn vector_to_i64(value: &Value, span: Span) -> Result<Vec<i64>, RunnerError> {
    let Value::Vector(values) = value else {
        return Err(RunnerError::internal(
            span,
            "tensor edge remap did not return a coordinate vector",
        ));
    };
    values
        .borrow()
        .iter()
        .map(|coordinate| match coordinate {
            Value::Int(coordinate) => Ok(*coordinate),
            other => Err(RunnerError::internal(
                span,
                format!("tensor edge remap coordinate is not numerus: {other:?}"),
            )),
        })
        .collect()
}

fn custom_edge_error(error: CustomEdgeValueError<RunnerError>, span: Span) -> RunnerError {
    match error {
        CustomEdgeValueError::Policy(error) => RunnerError::internal(
            span,
            format!("tensor edge policy mapping failed: {error:?}"),
        ),
        CustomEdgeValueError::Remap(error)
        | CustomEdgeValueError::Read(error)
        | CustomEdgeValueError::Transform(error) => error,
    }
}

fn tensor_has_custom_edge(tensor: &TensorValue) -> bool {
    tensor.edge_ops.iter().any(|operation| {
        matches!(
            operation,
            TensorEdgeOp::Shift {
                descriptor: super::tensor_edge::TensorEdgeDescriptor {
                    resolution: TensorEdgeResolution::Custom(_),
                    ..
                },
                ..
            }
        )
    })
}

fn value_needs_tensor_escape_preparation(value: &Value) -> bool {
    match value {
        Value::Tensor(tensor) => {
            let tensor = tensor.borrow();
            tensor.has_unresolved_edge_shift() || tensor_has_custom_edge(&tensor)
        }
        Value::Array(items) | Value::Vector(items) => items
            .borrow()
            .iter()
            .any(value_needs_tensor_escape_preparation),
        Value::Tuple(items) => items.iter().any(value_needs_tensor_escape_preparation),
        Value::Map(items) => items
            .borrow()
            .values()
            .any(value_needs_tensor_escape_preparation),
        Value::Ledger(ledger) => ledger
            .borrow()
            .values()
            .any(value_needs_tensor_escape_preparation),
        Value::Record(fields, _) | Value::Struct(_, fields) | Value::EnumVariant(_, fields) => {
            fields
                .borrow()
                .values()
                .any(value_needs_tensor_escape_preparation)
        }
        Value::SharedCell(cell) => value_needs_tensor_escape_preparation(&cell.borrow()),
        Value::Matrix(matrix) => matrix
            .borrow()
            .data
            .iter()
            .any(value_needs_tensor_escape_preparation),
        Value::Int(_)
        | Value::UInt(_)
        | Value::Magnus(_)
        | Value::Float(_)
        | Value::Dec(..)
        | Value::Bool(_)
        | Value::Str(_)
        | Value::Ascii(_)
        | Value::Char(_)
        | Value::Nil
        | Value::Unit
        | Value::Octeti(_)
        | Value::Instans(_)
        | Value::Regex(_)
        | Value::Match(_)
        | Value::Sermo(_)
        | Value::Function(_)
        | Value::Closure(_, _, _)
        | Value::ClosureEnvironment(_, _)
        | Value::GradientHandle(_)
        | Value::Cursor(_)
        | Value::Set(_) => false,
    }
}

fn shape_expression_value(
    expression: &MirShapeExpr,
    arguments: &[Value],
    span: Span,
) -> Result<u64, RunnerError> {
    match expression {
        MirShapeExpr::Literal(value) => Ok(*value),
        MirShapeExpr::ScalarArgument { argument } => {
            shape_extent(shape_argument(arguments, *argument, span)?, span)
        }
        MirShapeExpr::TensorAxis { argument, axis } => {
            shape_axis(shape_argument(arguments, *argument, span)?, *axis, span)
        }
        MirShapeExpr::TensorFieldAxis {
            argument,
            fields,
            axis,
        } => {
            let mut value = shape_argument(arguments, *argument, span)?.clone();
            for field in fields {
                value = shape_field_value(&value, *field, span)?;
            }
            shape_axis(&value, *axis, span)
        }
        MirShapeExpr::Affine { base, offset } => {
            let base = i128::from(shape_expression_value(base, arguments, span)?);
            let affine = base.checked_add(*offset).ok_or_else(|| {
                RunnerError::internal(span, "tensor shape affine expression overflow")
            })?;
            u64::try_from(affine).map_err(|_| {
                RunnerError::internal(span, "tensor shape affine expression is negative")
            })
        }
        MirShapeExpr::Divide { numerator, divisor } => {
            if *divisor == 0 {
                return Err(RunnerError::internal(
                    span,
                    "tensor shape expression divisor is zero",
                ));
            }
            let numerator = shape_expression_value(numerator, arguments, span)?;
            if numerator % divisor != 0 {
                return Err(RunnerError::internal(
                    span,
                    "tensor shape expression division must be exact",
                ));
            }
            Ok(numerator / divisor)
        }
        MirShapeExpr::Product(factors) => factors.iter().try_fold(1_u64, |product, factor| {
            product
                .checked_mul(shape_expression_value(factor, arguments, span)?)
                .ok_or_else(|| RunnerError::internal(span, "tensor shape product overflow"))
        }),
    }
}

fn recoverable_shape_expression_value(
    expression: &MirShapeExpr,
    arguments: &[Value],
    span: Span,
) -> Result<u64, RunnerError> {
    shape_expression_value(expression, arguments, span).map_err(|error| {
        RunnerError::internal_with_issue(span, error.message, "runner_tensor_shape_invalid_extent")
    })
}

fn shape_argument(arguments: &[Value], argument: u32, span: Span) -> Result<&Value, RunnerError> {
    let index = usize::try_from(argument)
        .map_err(|_| RunnerError::internal(span, "tensor shape argument index overflow"))?;
    arguments.get(index).ok_or_else(|| {
        RunnerError::internal(span, "tensor shape expression argument is out of range")
    })
}

fn shape_axis(value: &Value, axis: u32, span: Span) -> Result<u64, RunnerError> {
    let axis = usize::try_from(axis)
        .map_err(|_| RunnerError::internal(span, "tensor shape axis index overflow"))?;
    match value {
        Value::Tensor(tensor) => {
            let dimension =
                tensor.borrow().shape.get(axis).copied().ok_or_else(|| {
                    RunnerError::internal(span, "tensor shape axis is out of range")
                })?;
            u64::try_from(dimension)
                .map_err(|_| RunnerError::internal(span, "tensor shape extent is negative"))
        }
        Value::Vector(values) if axis == 0 => u64::try_from(values.borrow().len())
            .map_err(|_| RunnerError::internal(span, "tensor shape extent exceeds u64")),
        Value::Vector(_) => Err(RunnerError::internal(
            span,
            "tensor shape axis is out of range",
        )),
        _ => Err(RunnerError::internal(
            span,
            "tensor shape axis argument is not a tensor or vector",
        )),
    }
}

fn shape_extent(value: &Value, span: Span) -> Result<u64, RunnerError> {
    match value {
        Value::Int(value) => u64::try_from(*value)
            .map_err(|_| RunnerError::internal(span, "tensor shape scalar is negative")),
        Value::UInt(value) => Ok(*value),
        _ => Err(RunnerError::internal(
            span,
            "tensor shape scalar argument is not an integer",
        )),
    }
}

fn shape_field_value(value: &Value, field: Symbol, span: Span) -> Result<Value, RunnerError> {
    let fields = match value {
        Value::Record(fields, _) | Value::Struct(_, fields) | Value::EnumVariant(_, fields) => {
            fields
                .borrow()
                .get(&field)
                .cloned()
                .ok_or_else(|| RunnerError::internal(span, "tensor shape field is missing"))?
        }
        Value::SharedCell(cell) => return shape_field_value(&cell.borrow(), field, span),
        _ => {
            return Err(RunnerError::internal(
                span,
                "tensor shape field path crosses a non-record value",
            ));
        }
    };
    Ok(fields)
}

fn matmul_dims(m: i64, k: i64, n: i64) -> (usize, usize, usize) {
    (m.max(0) as usize, k.max(0) as usize, n.max(0) as usize)
}

fn dimension_to_usize(dimension: i64, span: Span) -> Result<usize, RunnerError> {
    usize::try_from(dimension)
        .map_err(|_| RunnerError::internal(span, "matmul shape contains a negative dimension"))
}

fn shape_product(shape: &[i64], span: Span) -> Result<usize, RunnerError> {
    shape
        .iter()
        .try_fold(1_usize, |count, dimension| {
            dimension_to_usize(*dimension, span)
                .ok()
                .and_then(|dimension| count.checked_mul(dimension))
        })
        .ok_or_else(|| RunnerError::internal(span, "matmul shape overflow"))
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

/// Exact dot-product accumulation (d11-6 rulings 17 and 19): every partial sum
/// is held wide and checked only against the 64-bit cap; the finished cell is
/// then stored under the element width, so a transient excursion outside the
/// element type does not trap while a cell that does not fit does.
fn matmul_exact(
    left: &[i128],
    right: &[i128],
    (m, k, n): (i64, i64, i64),
    span: Span,
) -> Result<Vec<i128>, RunnerError> {
    let overflow = || RunnerError::internal(span, NUMERUS_OVERFLOW);
    let (m, k, n) = matmul_dims(m, k, n);
    let mut out = vec![0_i128; m.saturating_mul(n)];
    for i in 0..m {
        for kk in 0..k {
            let a = left[i * k + kk];
            let right_row = &right[kk * n..(kk + 1) * n];
            let out_row = &mut out[i * n..(i + 1) * n];
            for j in 0..n {
                let prod = a.checked_mul(right_row[j]).ok_or_else(overflow)?;
                let next = out_row[j].checked_add(prod).ok_or_else(overflow)?;
                if !(super::exact_int::EXACT_MIN..=super::exact_int::EXACT_MAX).contains(&next) {
                    return Err(overflow());
                }
                out_row[j] = next;
            }
        }
    }
    Ok(out)
}

fn matmul_i64(
    left: &[i64],
    right: &[i64],
    m: i64,
    k: i64,
    n: i64,
    span: Span,
) -> Result<Vec<i64>, RunnerError> {
    let wide = |values: &[i64]| values.iter().map(|v| i128::from(*v)).collect::<Vec<_>>();
    let cells = matmul_exact(&wide(left), &wide(right), (m, k, n), span)?;
    cells
        .into_iter()
        .map(|cell| i64::try_from(cell).map_err(|_| RunnerError::internal(span, NUMERUS_OVERFLOW)))
        .collect()
}

fn matmul_u64(
    left: &[u64],
    right: &[u64],
    m: i64,
    k: i64,
    n: i64,
    span: Span,
) -> Result<Vec<u64>, RunnerError> {
    let wide = |values: &[u64]| values.iter().map(|v| i128::from(*v)).collect::<Vec<_>>();
    let cells = matmul_exact(&wide(left), &wide(right), (m, k, n), span)?;
    cells
        .into_iter()
        .map(|cell| u64::try_from(cell).map_err(|_| RunnerError::internal(span, NUMERUS_OVERFLOW)))
        .collect()
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

fn sum_float_values(
    values: impl IntoIterator<Item = Value>,
    span: Span,
) -> Result<f64, RunnerError> {
    values.into_iter().try_fold(0.0, |total, value| {
        let value = match value {
            Value::Int(value) => value as f64,
            Value::UInt(value) => value as f64,
            Value::Float(value) => value,
            _ => return Err(RunnerError::internal(span, "sum element type mismatch")),
        };
        Ok(total + value)
    })
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
    // Any other integer pair (a `lista<inf>` mixes `Int`, `UInt` and `Magnus`
    // carriers) compares by exact value.
    if let (Some(best), Some(value)) = (
        crate::inf::to_magnus(incumbent),
        crate::inf::to_magnus(candidate),
    ) {
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

/// One `tensor_sum_axis` output cell. Integer elements fold exact in the
/// ether (ruling 17): every partial is held in `i128` and checked against
/// the single 64-bit range `EXACT_MIN..=EXACT_MAX` at every step, so a
/// transient excursion outside the element width does not trap; only the
/// finished cell reaches the caller's element-width store. Any non-integer
/// element keeps the historical width-checked `numeric_add` fold.
fn sum_axis_cell(values: &[&Value], width: NumericWidth, span: Span) -> Result<Value, RunnerError> {
    let overflow = || RunnerError::internal(span, NUMERUS_OVERFLOW);
    let ints: Option<Vec<_>> = values
        .iter()
        .map(|value| super::exact_int::int_value(value))
        .collect();
    if let Some(ints) = ints {
        let total = ints.into_iter().try_fold(0_i128, |total, item| {
            let next = total.checked_add(item).ok_or_else(overflow)?;
            if !(super::exact_int::EXACT_MIN..=super::exact_int::EXACT_MAX).contains(&next) {
                return Err(overflow());
            }
            Ok(next)
        })?;
        return super::exact_int::exact_carrier(total, width).ok_or_else(overflow);
    }
    let mut sum = Value::Float(0.0);
    let mut first = true;
    for item in values {
        if first {
            sum = (*item).clone();
            first = false;
        } else {
            sum = numeric_add(&sum, item, NumericWidth::I64, span)?;
        }
    }
    Ok(sum)
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
pub(crate) fn format_value(
    value: &Value,
    spec: &str,
    float_width: Option<radix_types::NumericWidth>,
    span: Span,
) -> Result<String, RunnerError> {
    use radix_types::format_spec::{
        FormatSpec, parse_format_spec, render_decimal, render_float, render_instans_preset,
        render_integer, render_text,
    };
    let parsed = parse_format_spec(spec)
        .map_err(|_| RunnerError::internal(span, "format_value spec did not parse"))?;
    match (value, parsed) {
        // A stored `d64` is a scale-8 carrier; only the operand's static type
        // tells it from an integer. An unstored intermediate carries its scale.
        (Value::Int(v), FormatSpec::Layout(layout))
            if float_width.is_some_and(radix_types::NumericWidth::is_decimal) =>
        {
            Ok(render_decimal(
                i128::from(*v),
                crate::decimal::STORED_SCALE,
                &layout,
            ))
        }
        (Value::Dec(carrier, scale), FormatSpec::Layout(layout)) => {
            Ok(render_decimal(*carrier, *scale, &layout))
        }
        (Value::Int(v), FormatSpec::Layout(layout)) => Ok(render_integer(i128::from(*v), &layout)),
        (Value::UInt(v), FormatSpec::Layout(layout)) => Ok(render_integer(i128::from(*v), &layout)),
        // Ruling 42: an `inf` beyond the 64-bit carriers takes the integer
        // specs over the exact digits.
        (Value::Magnus(v), FormatSpec::Layout(layout)) => {
            Ok(radix_types::format_spec::render_magnus(v, &layout))
        }
        (Value::Float(v), FormatSpec::Layout(layout)) => {
            Ok(render_float(*v, &layout, |magnitude| {
                crate::value::display_float_at_width(magnitude, float_width)
            }))
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

/// The scalar IEEE function behind one math function (scalar or lifted). Float `abs` is
/// the sign-bit clear; the rest are the libm functions, so `sqrt(-x)` is NaN,
/// `ln(0)` is `-inf`, and `exp` overflows to `+inf`.
fn apply_math_function(function: MirMathFunction, value: f64) -> f64 {
    match function {
        MirMathFunction::Abs => value.abs(),
        MirMathFunction::Sqrt => value.sqrt(),
        MirMathFunction::Cbrt => value.cbrt(),
        MirMathFunction::Exp => value.exp(),
        MirMathFunction::Ln => value.ln(),
        MirMathFunction::Log10 => value.log10(),
        MirMathFunction::Sin => value.sin(),
        MirMathFunction::Cos => value.cos(),
        MirMathFunction::Tan => value.tan(),
    }
}

#[cfg(test)]
#[path = "runtime_index_test.rs"]
mod runtime_index_test;
#[cfg(test)]
#[path = "runtime_test.rs"]
mod runtime_test;

/// Operation label of one elementwise list operator in runner diagnostics.
fn lista_label(op: MirLiftOp) -> &'static str {
    match op {
        MirLiftOp::Add => "lista add",
        MirLiftOp::Sub => "lista sub",
        MirLiftOp::Mul => "lista mul",
        MirLiftOp::Div => "lista div",
        MirLiftOp::Mod => "lista mod",
        MirLiftOp::TrueDiv => "lista true div",
        MirLiftOp::Lt => "lista lt",
        MirLiftOp::Gt => "lista gt",
        MirLiftOp::LtEq => "lista lt-eq",
        MirLiftOp::GtEq => "lista gt-eq",
        MirLiftOp::Eq => "lista value-eq",
        MirLiftOp::NotEq => "lista value-not-eq",
        MirLiftOp::And => "lista and",
        MirLiftOp::Or => "lista or",
        MirLiftOp::Not => "lista not",
        _ => "lista neg",
    }
}
