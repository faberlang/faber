use crate::{
    MirAggregateFields, MirAggregateItem, MirAggregateKind, MirFunction, MirLayoutKind, MirLocalId,
    MirOperand, MirParam, MirParamMode, MirPlace, MirPlaceBase, MirScalarLayout, MirStatementKind,
    MirTerminatorKind, MirType, MirValidationContext,
};
use radix_lexer::Interner;
use radix_types::{NumericWidth, Primitive, Type, TypeId};

use super::{
    apply_collection_op_contract, buffer_element_count, buffer_element_type,
    builtin_input_for_index_param, builtin_inputs_for_named_params, called_builtins_for_function,
    collection_op_contract, combined_builtin_dependencies, device_view_element_count,
    device_view_element_type, dispatch_size_for_1d_elements, is_vacuum, kernel_has_collection_op,
    named_builtin_for_param, param_source_name, reject_kernel_matrix_abi,
    require_same_element_type, require_u32_index, result_tuple_count, storage_buffer_limit_error,
    CollectionOpContract, MirAbiError, MirKernelBuiltin, MirKernelBuiltinInput,
    MirKernelBuiltinUsage, MirKernelDispatchSize, MirKernelResource, MirKernelResourceAccess,
    MirKernelResourceKind, MirKernelResourceRole, MirKernelSignature, MirWorkgroupSize,
    StorageBufferLimitRecord, DEFAULT_KERNEL_RESOURCE_GROUP, DEFAULT_STORAGE_BUFFER_LIMIT,
    ERR_INDEXED_VIEW_REQUIRES_INPUT, ERR_INDEXED_VIEW_RETURNS_VACUUM,
    ERR_KERNEL_IO_ELEMENT_MISMATCH, ERR_KERNEL_PARAM_RETURN_MISMATCH,
    ERR_KERNEL_RESOURCE_BINDING_OVERFLOW, ERR_KERNEL_RESULT_TUPLE_REQUIRED,
    ERR_KERNEL_RESULT_TUPLE_REQUIRES_INPUT,
};

struct IndexedViewKernelContext<'a> {
    validation: &'a MirValidationContext<'a>,
    builtin_inputs: Vec<MirKernelBuiltinInput>,
    interner: Option<&'a Interner>,
    allow_host_owned_tensor_entry: bool,
}

/// The dense matmul glyph admits BF16 weights only in the first input slot;
/// the result and all F32 peers retain their ordinary strict element rule.
fn require_indexed_element_compatibility(
    function: &MirFunction,
    input_element_ty: MirType,
    output_element_ty: MirType,
    validation: &MirValidationContext<'_>,
    allow_bf16_left: bool,
) -> Result<(), MirAbiError> {
    let is_bf16_left = allow_bf16_left
        && kernel_has_collection_op(function, |op| {
            matches!(op, crate::MirCollectionOp::TensorMatMul)
        })
        && matches!(
            validation.layouts.layout_for_type(input_element_ty),
            Ok(layout) if matches!(layout.kind, MirLayoutKind::Scalar(MirScalarLayout::Bf16))
        )
        && matches!(
            validation.layouts.layout_for_type(output_element_ty),
            Ok(layout) if matches!(layout.kind, MirLayoutKind::Scalar(MirScalarLayout::F32))
        );
    if is_bf16_left {
        Ok(())
    } else {
        require_same_element_type(input_element_ty, output_element_ty, validation)
    }
}

/// Well-known resource name for the runtime-extent count channel.
///
/// The host contract populates the extent binding with a `u32` element count
/// before dispatch (CTO #4 option b — well-known binding, `combineMetadata`
/// runtime-param precedent). L8/L9 dtype/stride headroom: the extent is
/// count-typed (`u32`) with no f32-width assumption; payload element types
/// stay per-resource.
const RUNTIME_EXTENT_RESOURCE_NAME: &str = "runtime_extent";

/// Build a runtime-extent count resource.
///
/// CXO F2: extents are `Vec`-capable from day one — one entry per extent
/// channel, appended to `extra_inputs`; Wave 1 populates at most one. The
/// binding is caller-assigned as the next free binding from the positional
/// allocator.
fn runtime_extent_resource(binding: u32, extent_ty: TypeId) -> MirKernelResource {
    MirKernelResource {
        group: DEFAULT_KERNEL_RESOURCE_GROUP,
        binding,
        kind: MirKernelResourceKind::RuntimeExtent,
        role: MirKernelResourceRole::Input,
        access: MirKernelResourceAccess::Read,
        element_ty: MirType::semantic(extent_ty),
        element_count: 1,
        source_local: None,
        source_name: Some(RUNTIME_EXTENT_RESOURCE_NAME.to_owned()),
        reduced_projection: None,
    }
}

/// Append a runtime-extent count resource when the kernel is scalar-shaped.
///
/// "Where the runtime extent applies" (U2): a kernel whose static IO element
/// count is 1 has `dispatch_size = (1, 1, 1)`, so a guard over the static
/// count culls every host-dispatched thread past the first. The extent
/// channel fixes that: the host supplies the element count at dispatch and
/// the guard reads the binding. Kernels with statically-known multi-element
/// shapes keep the static limit (correct), and collection-op contracts
/// (reduction/matmul) stay static per the U2 non-goals.
///
/// The count type (`numerus<u32>`, L8/L9) must already exist in the frozen
/// type table; when it cannot be resolved the kernel keeps the static
/// dispatch-axis fallback (pre-U2 behavior) — the guard still fires, but at
/// the static `oob_guard_limit` instead of the host-supplied count.
/// PRODUCTION INVARIANT: the GPU builtin registry interns `numerus<u32>`
/// for every kernel program, so real kernels always carry the extent; only
/// hand-built unit tables may lack it (`abi_test` pins the fallback).
fn maybe_append_runtime_extent(
    function: &MirFunction,
    signature: &mut MirKernelSignature,
    validation: &MirValidationContext<'_>,
) {
    if kernel_has_collection_op(function, |_| true) {
        return;
    }
    if signature.input.element_count != 1 || signature.output.element_count != 1 {
        return;
    }
    let Some(extent_ty) = validation
        .types
        .find_sized_numeric(Primitive::Numerus, NumericWidth::U32)
    else {
        return;
    };
    let binding = signature
        .resources()
        .map(|resource| resource.binding)
        .max()
        .unwrap_or(0)
        + 1;
    signature
        .extra_inputs
        .push(runtime_extent_resource(binding, extent_ty));
}

/// Build the read-input resources for a kernel's params, in binding order,
/// skipping `vacuum` params (the reverse-AD nil placeholder carries no
/// device storage). Returns the resources and the next free binding.
///
/// `element_type`/`element_count` select the element-shape law per kernel
/// family: the result-tuple arm reads buffer views (`buffer_element_*`), the
/// reduction arm reads device views (`device_view_element_*`).
fn input_resources_for_params(
    inputs: &[MirParam],
    validation: &MirValidationContext<'_>,
    interner: Option<&Interner>,
    allow_host_owned_tensor_entry: bool,
    element_type: impl Fn(MirType, &MirValidationContext<'_>, bool) -> Result<MirType, MirAbiError>,
    element_count: impl Fn(MirType, &MirValidationContext<'_>) -> Result<u64, MirAbiError>,
) -> Result<(Vec<MirKernelResource>, u32), MirAbiError> {
    let mut input_resources = Vec::with_capacity(inputs.len());
    let mut next_binding = 0u32;
    for input in inputs {
        if is_vacuum(input.ty, validation) {
            continue;
        }
        input_resources.push(MirKernelResource {
            group: DEFAULT_KERNEL_RESOURCE_GROUP,
            binding: next_binding,
            kind: MirKernelResourceKind::StorageBuffer,
            role: MirKernelResourceRole::Input,
            access: MirKernelResourceAccess::Read,
            element_ty: element_type(input.ty, validation, allow_host_owned_tensor_entry)?,
            element_count: element_count(input.ty, validation)?,
            source_local: Some(input.local),
            source_name: param_source_name(input, interner),
            reduced_projection: None,
        });
        next_binding += 1;
    }
    Ok((input_resources, next_binding))
}

impl MirKernelSignature {
    /// # Errors
    /// Returns an error when validation or type resolution fails. See [`MirAbiError`].
    pub fn storage_buffer_kernel(
        function: &MirFunction,
        validation: &MirValidationContext<'_>,
    ) -> Result<Self, MirAbiError> {
        Self::storage_buffer_kernel_with_limit(function, validation, DEFAULT_STORAGE_BUFFER_LIMIT)
    }

    /// Synthesize a storage-buffer kernel ABI against an explicit lane limit.
    ///
    /// The shared default remains the conservative minimum across admitted
    /// lanes. A backend lane that admits a different binding capacity passes
    /// its own [`StorageBufferLimitRecord`] here.
    ///
    /// # Errors
    /// Returns an error when validation, type resolution, or the selected
    /// lane's storage-buffer capacity fails. See [`MirAbiError`].
    pub fn storage_buffer_kernel_with_limit(
        function: &MirFunction,
        validation: &MirValidationContext<'_>,
        limit: StorageBufferLimitRecord,
    ) -> Result<Self, MirAbiError> {
        Self::storage_buffer_kernel_with_names(function, validation, None, false, limit)
    }

    /// # Errors
    /// Returns an error when validation or type resolution fails. See [`MirAbiError`].
    pub fn storage_buffer_kernel_with_interner(
        function: &MirFunction,
        validation: &MirValidationContext<'_>,
        interner: &Interner,
    ) -> Result<Self, MirAbiError> {
        Self::storage_buffer_kernel_with_interner_and_limit(
            function,
            validation,
            interner,
            DEFAULT_STORAGE_BUFFER_LIMIT,
        )
    }

    /// Synthesize a named storage-buffer kernel ABI against an explicit lane
    /// limit.
    ///
    /// # Errors
    /// Returns an error when validation, type resolution, or the selected
    /// lane's storage-buffer capacity fails. See [`MirAbiError`].
    pub fn storage_buffer_kernel_with_interner_and_limit(
        function: &MirFunction,
        validation: &MirValidationContext<'_>,
        interner: &Interner,
        limit: StorageBufferLimitRecord,
    ) -> Result<Self, MirAbiError> {
        Self::storage_buffer_kernel_with_names(function, validation, Some(interner), false, limit)
    }

    /// # Errors
    /// Returns an error when validation or type resolution fails. See [`MirAbiError`].
    pub fn storage_buffer_kernel_with_interner_for_target_entry(
        function: &MirFunction,
        validation: &MirValidationContext<'_>,
        interner: &Interner,
    ) -> Result<Self, MirAbiError> {
        Self::storage_buffer_kernel_with_interner_for_target_entry_and_limit(
            function,
            validation,
            interner,
            DEFAULT_STORAGE_BUFFER_LIMIT,
        )
    }

    /// Synthesize a target-entry storage-buffer ABI against an explicit lane
    /// limit.
    ///
    /// # Errors
    /// Returns an error when validation, type resolution, or the selected
    /// lane's storage-buffer capacity fails. See [`MirAbiError`].
    pub fn storage_buffer_kernel_with_interner_for_target_entry_and_limit(
        function: &MirFunction,
        validation: &MirValidationContext<'_>,
        interner: &Interner,
        limit: StorageBufferLimitRecord,
    ) -> Result<Self, MirAbiError> {
        Self::storage_buffer_kernel_with_names(function, validation, Some(interner), true, limit)
    }

    fn storage_buffer_kernel_with_names(
        function: &MirFunction,
        validation: &MirValidationContext<'_>,
        interner: Option<&Interner>,
        allow_host_owned_tensor_entry: bool,
        limit: StorageBufferLimitRecord,
    ) -> Result<Self, MirAbiError> {
        // Unit B: stable shape before arity matching so matrix kernel IO does not
        // fall through to the generic "one buffer parameter" message.
        reject_kernel_matrix_abi(function, validation)?;

        // S3-A1: a result-tuple return (a non-empty tuple of device views) is
        // the multi-output contract — every param is an input and the tuple
        // elements are DISTINCT output resources. Checked before the
        // single-output arms, which would reject the tuple as an aggregate.
        let signature =
            if !function.params.is_empty() && result_tuple_count(function, validation).is_some() {
                Self::multi_output_return_kernel(
                    function,
                    &function.params,
                    validation,
                    interner,
                    allow_host_owned_tensor_entry,
                )?
            } else if let Some(interner) = interner {
                if let Some(signature) = Self::source_named_indexed_view_kernel(
                    function,
                    validation,
                    interner,
                    allow_host_owned_tensor_entry,
                )? {
                    signature
                } else {
                    Self::fallback_kernel_signature(
                        function,
                        validation,
                        Some(interner),
                        allow_host_owned_tensor_entry,
                    )?
                }
            } else {
                Self::fallback_kernel_signature(
                    function,
                    validation,
                    interner,
                    allow_host_owned_tensor_entry,
                )?
            };
        // The per-kernel storage-buffer binding maximum (the active
        // per-lane `StorageBufferLimitRecord`; the shared default is the
        // minimum capacity across admitted lanes): a signature over
        // it cannot be emitted as one kernel, so the synthesis refuses it
        // fail-closed — the callers (the faber whole-function path, the
        // emitters' whole-kernel cross-check, the image input resolution)
        // fall through to the decomposition split.
        let buffer_count = signature
            .resources()
            .filter(|resource| resource.kind == MirKernelResourceKind::StorageBuffer)
            .count();
        if !limit.admits(buffer_count) {
            return Err(storage_buffer_limit_error(buffer_count, limit));
        }
        Ok(signature)
    }

    /// The single-input / multi-input / indexed-view kernel shape dispatch —
    /// the non-tuple, non-named-param arms of the storage-buffer synthesis.
    fn fallback_kernel_signature(
        function: &MirFunction,
        validation: &MirValidationContext<'_>,
        interner: Option<&Interner>,
        allow_host_owned_tensor_entry: bool,
    ) -> Result<Self, MirAbiError> {
        // S3-A5: a kernel whose body carries a Reduction contract and whose
        // return is a scalar (the mean/sum result) synthesizes the
        // indexed-view reduction signature WITHOUT explicit output/index
        // parameters: every tensor param becomes a read input and the output
        // is the per-workgroup partials buffer from the resolved contract.
        // The scalar return is the host-combined reduction result; the
        // emitter's reduction arm writes the per-workgroup partials (the
        // S3-B1 mul-mean forward `simple_loss(x, w) → f32` shape).
        if let Some(signature) = Self::reduction_scalar_return_kernel(
            function,
            validation,
            interner,
            allow_host_owned_tensor_entry,
        )? {
            return Ok(signature);
        }

        match function.params.as_slice() {
            [param] => Self::return_buffer_kernel(
                function,
                validation,
                param,
                interner,
                allow_host_owned_tensor_entry,
            ),
            inputs if !inputs.is_empty() && !is_vacuum(function.return_ty, validation) => {
                Self::multi_input_return_buffer_kernel(
                    function,
                    validation,
                    inputs,
                    interner,
                    allow_host_owned_tensor_entry,
                )
            }
            [inputs @ .., output, index] if !inputs.is_empty() => {
                let context = IndexedViewKernelContext {
                    validation,
                    builtin_inputs: vec![builtin_input_for_index_param(index, interner)],
                    interner,
                    allow_host_owned_tensor_entry,
                };
                if let [input] = inputs {
                    Self::indexed_view_kernel(function, input, output, index, context)
                } else {
                    Self::multi_input_indexed_view_kernel(function, inputs, output, index, context)
                }
            }
            _ => Err(MirAbiError::new(
                "kernel must have either one buffer parameter or input*/output/index parameters",
            )),
        }
    }

    fn source_named_indexed_view_kernel(
        function: &MirFunction,
        validation: &MirValidationContext<'_>,
        interner: &Interner,
        allow_host_owned_tensor_entry: bool,
    ) -> Result<Option<Self>, MirAbiError> {
        let params = function.params.as_slice();
        if params.len() < 3 {
            return Ok(None);
        }

        let mut builtin_start = params.len();
        while builtin_start > 0
            && (params[builtin_start - 1].mode == MirParamMode::Lane
                || named_builtin_for_param(&params[builtin_start - 1], interner).is_some())
        {
            builtin_start -= 1;
        }
        if builtin_start == params.len() {
            return Ok(None);
        }
        if builtin_start < 2 {
            return Err(MirAbiError::new(
                "indexed view kernel requires input/output parameters before builtin parameters",
            ));
        }

        let inputs = &params[..builtin_start - 1];
        let output = &params[builtin_start - 1];
        let builtin_params = &params[builtin_start..];
        let builtin_inputs = builtin_inputs_for_named_params(builtin_params, validation, interner)?;
        let context = IndexedViewKernelContext {
            validation,
            builtin_inputs,
            interner: Some(interner),
            allow_host_owned_tensor_entry,
        };
        if let [input] = inputs {
            Self::indexed_view_kernel(function, input, output, &builtin_params[0], context)
                .map(Some)
        } else {
            Self::multi_input_indexed_view_kernel(
                function,
                inputs,
                output,
                &builtin_params[0],
                context,
            )
            .map(Some)
        }
    }

    /// Synthesize the storage-buffer ABI for a **result-tuple** kernel
    /// (S3-A1): a function whose return type is a non-empty tuple of device
    /// views. Every param becomes a read input buffer; the tuple's elements
    /// become DISTINCT output resources — one per tuple element, in tuple
    /// order, each bound to its own slot (the first element is `output`, the
    /// rest `extra_outputs`).
    ///
    /// This is the device ABI for a backward companion's `(grad_x, grad_w)`
    /// tuple: N gradient outputs bind to N distinct output slots, and no
    /// kernel value carries an aggregate type — backend aggregate rejection
    /// is unreachable through this ABI because tuple outputs are separate
    /// resources, never an aggregate.
    fn multi_output_return_kernel(
        function: &MirFunction,
        inputs: &[MirParam],
        validation: &MirValidationContext<'_>,
        interner: Option<&Interner>,
        allow_host_owned_tensor_entry: bool,
    ) -> Result<Self, MirAbiError> {
        let Type::Tuple(elements) = validation.types.get(function.return_ty.semantic_id()) else {
            return Err(MirAbiError::new(ERR_KERNEL_RESULT_TUPLE_REQUIRED));
        };
        if elements.is_empty() {
            return Err(MirAbiError::new(ERR_KERNEL_RESULT_TUPLE_REQUIRED));
        }

        // Every non-vacuum param becomes a read input; a vacuum "residual"
        // param (the reverse-AD nil placeholder) carries no device storage
        // and is skipped (S3-A2 companion shape: `backward(x, w, nil(),
        // upstream) → (grad_x, grad_w)`). Output bindings start after the
        // inputs, so each output binds a distinct slot.
        let (input_resources, next_binding) = input_resources_for_params(
            inputs,
            validation,
            interner,
            allow_host_owned_tensor_entry,
            buffer_element_type,
            buffer_element_count,
        )?;
        let input = input_resources
            .first()
            .cloned()
            .ok_or_else(|| MirAbiError::new(ERR_KERNEL_RESULT_TUPLE_REQUIRES_INPUT))?;
        let extra_inputs = input_resources.into_iter().skip(1).collect();

        // Tuple elements → distinct output resources (first → `output`).
        // The output resources carry the return-tuple construct's field
        // locals as their `MirLocal` origin (the S5-U1 forward-save
        // identity): a decomposed source's subchain synthetics share the
        // source local table, so the (function, local) identity fact lets
        // the faber cross-kernel unification join a producer subchain's
        // output slot with a consumer subchain's input slot for the SAME
        // local — without which an elementwise split's intermediate
        // outputs would mint distinct PerProgram input buffers
        // (uninitialized reads / missing once-init values).
        let return_fields = Self::return_tuple_field_locals(function);
        let mut outputs = Vec::with_capacity(elements.len());
        for (index, element) in elements.iter().enumerate() {
            let element_ty = MirType::semantic(*element);
            let view_ty =
                device_view_element_type(element_ty, validation, allow_host_owned_tensor_entry)?;
            let view_count = device_view_element_count(element_ty, validation)?;
            outputs.push(MirKernelResource {
                group: DEFAULT_KERNEL_RESOURCE_GROUP,
                binding: next_binding
                    .checked_add(u32::try_from(index).unwrap_or(u32::MAX))
                    .ok_or_else(|| MirAbiError::new(ERR_KERNEL_RESOURCE_BINDING_OVERFLOW))?,
                kind: MirKernelResourceKind::StorageBuffer,
                role: MirKernelResourceRole::Output,
                access: MirKernelResourceAccess::Write,
                element_ty: view_ty,
                element_count: view_count,
                source_local: return_fields
                    .as_ref()
                    .and_then(|fields| fields.get(index).copied()),
                source_name: None,
                reduced_projection: None,
            });
        }
        let output = outputs.remove(0);
        let extra_outputs = outputs;
        // R6 (P1): the dispatch must cover EVERY output's extent — the union
        // (max) over the per-output extents, never the first output's count
        // alone. A first-output-only dispatch either overruns a smaller
        // buffer or leaves a larger one incomplete; each output's write is
        // guarded by its own extent under the union (kernel_plan).
        let dispatch_count = extra_outputs
            .iter()
            .fold(output.element_count, |union, extra| {
                union.max(extra.element_count)
            });

        let mut signature = Self {
            function: function.id,
            input,
            extra_inputs,
            output,
            extra_outputs,
            accumulation_buffers: Vec::new(),
            builtin_inputs: Vec::new(),
            called_builtins: called_builtins_for_function(function),
            workgroup: MirWorkgroupSize { x: 1, y: 1, z: 1 },
            dispatch_size: dispatch_size_for_1d_elements(dispatch_count),
        };
        maybe_append_runtime_extent(function, &mut signature, validation);
        Ok(signature)
    }

    /// The ordered field locals of a kernel's return-tuple `Construct` — the
    /// locals the result-tuple binds at full-tuple index `i` (the
    /// local-return and the temp-return shapes both resolve). `None` when
    /// the function does not return an ordered non-spread tuple of plain
    /// local places. The multi-output ABI carries these as the output
    /// resources' `MirLocal` origins (the S5-U1 forward-save identity), so
    /// the decomposition subchains' cross-kernel unification joins a
    /// producer and a consumer slot for the same local.
    fn return_tuple_field_locals(function: &MirFunction) -> Option<Vec<MirLocalId>> {
        if function.blocks.len() != 1 {
            return None;
        }
        let block = &function.blocks[0];
        let MirTerminatorKind::Return(Some(operand)) = &block.terminator.kind else {
            return None;
        };
        let base = match operand {
            MirOperand::Place(place) => place.base.clone(),
            MirOperand::Temp(temp) => MirPlaceBase::Temp(*temp),
            _ => return None,
        };
        let destination_matches = |place: &MirPlace| place.base == base;
        block.statements.iter().find_map(|statement| {
            let MirStatementKind::Construct {
                destination,
                aggregate,
            } = &statement.kind
            else {
                return None;
            };
            if !destination_matches(destination) {
                return None;
            }
            if aggregate.kind != MirAggregateKind::Tuple {
                return None;
            }
            let MirAggregateFields::Ordered(items) = &aggregate.fields else {
                return None;
            };
            items
                .iter()
                .map(|item| match item {
                    MirAggregateItem::Operand(MirOperand::Place(place)) => {
                        crate::read_analysis::place_local_base(place)
                    }
                    _ => None,
                })
                .collect::<Option<Vec<_>>>()
        })
    }

    /// S3-A5: synthesize the indexed-view reduction signature for a kernel
    /// whose body carries a Reduction contract and whose return is a scalar
    /// (the mean/sum result — the S3-B1 forward shape
    /// `simple_loss(x, w) → f32` with `mean(mul(x, w))`).
    ///
    /// Every non-vacuum tensor param becomes a read input; the output is the
    /// per-workgroup partials buffer from the resolved contract (count =
    /// `partials`), and the workgroup/dispatch come from the contract. The
    /// emitter's reduction arm writes each workgroup's partial there; the
    /// scalar return is the host-combined reduction result. `None` when the
    /// body carries no Reduction contract or the return is not a scalar.
    fn reduction_scalar_return_kernel(
        function: &MirFunction,
        validation: &MirValidationContext<'_>,
        interner: Option<&Interner>,
        allow_host_owned_tensor_entry: bool,
    ) -> Result<Option<Self>, MirAbiError> {
        let Some(contract) = collection_op_contract(function, validation)? else {
            return Ok(None);
        };
        let CollectionOpContract::Reduction {
            length,
            workgroup_x,
            partials,
        } = contract
        else {
            return Ok(None);
        };
        // The return must be a scalar (the reduction result) — tensor and
        // tuple returns have their own signature arms.
        if !matches!(
            validation.layouts.layout_for_type(function.return_ty),
            Ok(layout) if matches!(layout.kind, MirLayoutKind::Scalar(_))
        ) {
            return Ok(None);
        }

        let (input_resources, next_binding) = input_resources_for_params(
            &function.params,
            validation,
            interner,
            allow_host_owned_tensor_entry,
            device_view_element_type,
            device_view_element_count,
        )?;
        let input = input_resources
            .first()
            .cloned()
            .ok_or_else(|| MirAbiError::new(ERR_INDEXED_VIEW_REQUIRES_INPUT))?;
        let extra_inputs = input_resources.into_iter().skip(1).collect();
        let reduction_element_ty = input.element_ty;

        let mut signature = Self {
            function: function.id,
            input,
            extra_inputs,
            output: MirKernelResource {
                group: DEFAULT_KERNEL_RESOURCE_GROUP,
                binding: next_binding,
                kind: MirKernelResourceKind::StorageBuffer,
                role: MirKernelResourceRole::Output,
                access: MirKernelResourceAccess::Write,
                element_ty: reduction_element_ty,
                element_count: partials,
                source_local: None,
                source_name: None,
                reduced_projection: None,
            },
            extra_outputs: Vec::new(),
            accumulation_buffers: Vec::new(),
            builtin_inputs: Vec::new(),
            // The reduction recipe's grid-stride loop starts from the GLOBAL
            // thread index (the header's first builtin selects the index
            // expression); the tree + partial write add `local_id.x`/
            // `workgroup_id.x` via the emitter's C5 builtins.
            called_builtins: {
                let mut builtins = vec![MirKernelBuiltin::GlobalInvocationX];
                builtins.extend(called_builtins_for_function(function));
                builtins
            },
            workgroup: MirWorkgroupSize {
                x: workgroup_x,
                y: 1,
                z: 1,
            },
            dispatch_size: MirKernelDispatchSize {
                x: length,
                y: 1,
                z: 1,
            },
        };
        maybe_append_runtime_extent(function, &mut signature, validation);
        Ok(Some(signature))
    }

    fn return_buffer_kernel(
        function: &MirFunction,
        validation: &MirValidationContext<'_>,
        param: &MirParam,
        interner: Option<&Interner>,
        allow_host_owned_tensor_entry: bool,
    ) -> Result<Self, MirAbiError> {
        // The strict semantic-type match is the plain-elementwise law. A
        // collection-op kernel remaps the shape by contract (Transpose
        // `[M,N] → [N,M]`), so for those the flat element-count equality
        // below carries the binding shape and
        // `apply_collection_op_contract` supplies the dispatch facts.
        if !kernel_has_collection_op(function, |_| true)
            && !validation
                .types
                .equals(param.ty.semantic_id(), function.return_ty.semantic_id())
        {
            return Err(MirAbiError::new(ERR_KERNEL_PARAM_RETURN_MISMATCH));
        }

        let param_element_ty =
            buffer_element_type(param.ty, validation, allow_host_owned_tensor_entry)?;
        let return_element_ty = buffer_element_type(
            function.return_ty,
            validation,
            allow_host_owned_tensor_entry,
        )?;
        require_same_element_type(param_element_ty, return_element_ty, validation)?;
        let param_element_count = buffer_element_count(param.ty, validation)?;
        let return_element_count = buffer_element_count(function.return_ty, validation)?;
        // The return-buffer equality law (shared with the multi-input arm):
        // the input and output buffers are flat views of the same element
        // count, so a host can assume one elementwise shape across bindings.
        // A rectangular transpose's `[M,N]` and `[N,M]` shapes both hold the
        // flat `M·N` count, so it passes here; genuinely size-changing
        // kernels fail closed with the elementwise-only diagnostic.
        if param_element_count != return_element_count {
            return Err(MirAbiError::new(ERR_KERNEL_IO_ELEMENT_MISMATCH));
        }

        let mut signature = Self {
            function: function.id,
            input: MirKernelResource {
                group: DEFAULT_KERNEL_RESOURCE_GROUP,
                binding: 0,
                kind: MirKernelResourceKind::StorageBuffer,
                role: MirKernelResourceRole::Input,
                access: MirKernelResourceAccess::Read,
                element_ty: param_element_ty,
                element_count: param_element_count,
                source_local: Some(param.local),
                source_name: param_source_name(param, interner),
                reduced_projection: None,
            },
            extra_inputs: Vec::new(),
            output: MirKernelResource {
                group: DEFAULT_KERNEL_RESOURCE_GROUP,
                binding: 1,
                kind: MirKernelResourceKind::StorageBuffer,
                role: MirKernelResourceRole::Output,
                access: MirKernelResourceAccess::Write,
                element_ty: return_element_ty,
                element_count: return_element_count,
                source_local: None,
                source_name: None,
                reduced_projection: None,
            },
            extra_outputs: Vec::new(),
            accumulation_buffers: Vec::new(),
            builtin_inputs: Vec::new(),
            called_builtins: called_builtins_for_function(function),
            workgroup: MirWorkgroupSize { x: 1, y: 1, z: 1 },
            dispatch_size: dispatch_size_for_1d_elements(return_element_count),
        };
        maybe_append_runtime_extent(function, &mut signature, validation);
        // PRP-004 U3 (D1-A): collection-op kernels (Transpose, Reduction,
        // MatMul) carry their dispatch/workgroup/output-buffer contract even
        // through the single-input return-buffer arm (shared ABI law, all
        // compute targets). A rank-2 transpose `[M,N] → [N,M]` keeps the
        // flat `M·N` element counts but must advertise the transpose
        // workgroup/dispatch contract, not the generic 1D one.
        apply_collection_op_contract(function, &mut signature, validation)?;
        Ok(signature)
    }

    /// Synthesize the storage-buffer ABI for a multi-input device kernel
    /// entry (C3): every param becomes a read input buffer and the tensor
    /// return becomes the write output buffer.
    ///
    /// Shared ABI law for every compute lane: each backend consumes this
    /// resource shape through its own syntax. Equal element counts across
    /// every input and the return are enforced with the same equality law the
    /// single-input return-buffer path uses, so a host can assume one
    /// elementwise shape across bindings.
    fn multi_input_return_buffer_kernel(
        function: &MirFunction,
        validation: &MirValidationContext<'_>,
        inputs: &[MirParam],
        interner: Option<&Interner>,
        allow_host_owned_tensor_entry: bool,
    ) -> Result<Self, MirAbiError> {
        let return_element_ty = buffer_element_type(
            function.return_ty,
            validation,
            allow_host_owned_tensor_entry,
        )?;
        let return_element_count = buffer_element_count(function.return_ty, validation)?;

        let mut input_resources = Vec::with_capacity(inputs.len());
        for (binding, input) in inputs.iter().enumerate() {
            let input_element_ty =
                buffer_element_type(input.ty, validation, allow_host_owned_tensor_entry)?;
            require_same_element_type(input_element_ty, return_element_ty, validation)?;
            let input_element_count = buffer_element_count(input.ty, validation)?;
            if input_element_count != return_element_count {
                return Err(MirAbiError::new(ERR_KERNEL_IO_ELEMENT_MISMATCH));
            }
            input_resources.push(MirKernelResource {
                group: DEFAULT_KERNEL_RESOURCE_GROUP,
                binding: u32::try_from(binding)
                    .map_err(|_| MirAbiError::new(ERR_KERNEL_RESOURCE_BINDING_OVERFLOW))?,
                kind: MirKernelResourceKind::StorageBuffer,
                role: MirKernelResourceRole::Input,
                access: MirKernelResourceAccess::Read,
                element_ty: input_element_ty,
                element_count: input_element_count,
                source_local: Some(input.local),
                source_name: param_source_name(input, interner),
                reduced_projection: None,
            });
        }

        let input = input_resources
            .first()
            .cloned()
            .ok_or_else(|| MirAbiError::new(ERR_INDEXED_VIEW_REQUIRES_INPUT))?;
        let extra_inputs = input_resources.into_iter().skip(1).collect();

        let mut signature = Self {
            function: function.id,
            input,
            extra_inputs,
            output: MirKernelResource {
                group: DEFAULT_KERNEL_RESOURCE_GROUP,
                binding: u32::try_from(inputs.len())
                    .map_err(|_| MirAbiError::new(ERR_KERNEL_RESOURCE_BINDING_OVERFLOW))?,
                kind: MirKernelResourceKind::StorageBuffer,
                role: MirKernelResourceRole::Output,
                access: MirKernelResourceAccess::Write,
                element_ty: return_element_ty,
                element_count: return_element_count,
                source_local: None,
                source_name: None,
                reduced_projection: None,
            },
            extra_outputs: Vec::new(),
            accumulation_buffers: Vec::new(),
            builtin_inputs: Vec::new(),
            called_builtins: called_builtins_for_function(function),
            workgroup: MirWorkgroupSize { x: 1, y: 1, z: 1 },
            dispatch_size: dispatch_size_for_1d_elements(return_element_count),
        };
        maybe_append_runtime_extent(function, &mut signature, validation);
        // PRP-004 U3 (D1-A): collection-op kernels carry their contract
        // through this arm too — the dispatch/workgroup/output-buffer shape
        // is a property of the resolved collection op, not of the arity that
        // picked the ABI arm (shared ABI law, all compute targets).
        apply_collection_op_contract(function, &mut signature, validation)?;
        Ok(signature)
    }

    fn indexed_view_kernel(
        function: &MirFunction,
        input: &MirParam,
        output: &MirParam,
        index: &MirParam,
        context: IndexedViewKernelContext<'_>,
    ) -> Result<Self, MirAbiError> {
        let validation = context.validation;
        if !is_vacuum(function.return_ty, validation) {
            return Err(MirAbiError::new(ERR_INDEXED_VIEW_RETURNS_VACUUM));
        }
        require_u32_index(index.ty, validation)?;

        let input_element_ty =
            device_view_element_type(input.ty, validation, context.allow_host_owned_tensor_entry)?;
        let output_element_ty =
            device_view_element_type(output.ty, validation, context.allow_host_owned_tensor_entry)?;
        require_indexed_element_compatibility(
            function,
            input_element_ty,
            output_element_ty,
            validation,
            true,
        )?;
        let input_element_count = device_view_element_count(input.ty, validation)?;
        let output_element_count = device_view_element_count(output.ty, validation)?;

        let mut signature = Self {
            function: function.id,
            input: MirKernelResource {
                group: DEFAULT_KERNEL_RESOURCE_GROUP,
                binding: 0,
                kind: MirKernelResourceKind::StorageBuffer,
                role: MirKernelResourceRole::Input,
                access: MirKernelResourceAccess::Read,
                element_ty: input_element_ty,
                element_count: input_element_count,
                source_local: Some(input.local),
                source_name: param_source_name(input, context.interner),
                reduced_projection: None,
            },
            extra_inputs: Vec::new(),
            output: MirKernelResource {
                group: DEFAULT_KERNEL_RESOURCE_GROUP,
                binding: 1,
                kind: MirKernelResourceKind::StorageBuffer,
                role: MirKernelResourceRole::Output,
                access: MirKernelResourceAccess::Write,
                element_ty: output_element_ty,
                element_count: output_element_count,
                source_local: Some(output.local),
                source_name: param_source_name(output, context.interner),
                reduced_projection: None,
            },
            extra_outputs: Vec::new(),
            accumulation_buffers: Vec::new(),
            builtin_inputs: context.builtin_inputs,
            called_builtins: called_builtins_for_function(function),
            workgroup: MirWorkgroupSize { x: 1, y: 1, z: 1 },
            dispatch_size: dispatch_size_for_1d_elements(output_element_count),
        };
        maybe_append_runtime_extent(function, &mut signature, validation);
        // PRP-004 U3 (D1-A): reduction/matmul kernels carry a corrected
        // dispatch/workgroup/output-buffer contract (shared ABI law, all
        // compute targets).
        apply_collection_op_contract(function, &mut signature, validation)?;
        Ok(signature)
    }

    fn multi_input_indexed_view_kernel(
        function: &MirFunction,
        inputs: &[MirParam],
        output: &MirParam,
        index: &MirParam,
        context: IndexedViewKernelContext<'_>,
    ) -> Result<Self, MirAbiError> {
        let validation = context.validation;
        if !is_vacuum(function.return_ty, validation) {
            return Err(MirAbiError::new(ERR_INDEXED_VIEW_RETURNS_VACUUM));
        }
        require_u32_index(index.ty, validation)?;

        let output_element_ty =
            device_view_element_type(output.ty, validation, context.allow_host_owned_tensor_entry)?;
        let output_element_count = device_view_element_count(output.ty, validation)?;
        let mut input_resources = Vec::new();
        for (binding, input) in inputs.iter().enumerate() {
            let input_element_ty = device_view_element_type(
                input.ty,
                validation,
                context.allow_host_owned_tensor_entry,
            )?;
            require_indexed_element_compatibility(
                function,
                input_element_ty,
                output_element_ty,
                validation,
                binding == 0,
            )?;
            let input_element_count = device_view_element_count(input.ty, validation)?;
            input_resources.push(MirKernelResource {
                group: DEFAULT_KERNEL_RESOURCE_GROUP,
                binding: u32::try_from(binding)
                    .map_err(|_| MirAbiError::new(ERR_KERNEL_RESOURCE_BINDING_OVERFLOW))?,
                kind: MirKernelResourceKind::StorageBuffer,
                role: MirKernelResourceRole::Input,
                access: MirKernelResourceAccess::Read,
                element_ty: input_element_ty,
                element_count: input_element_count,
                source_local: Some(input.local),
                source_name: param_source_name(input, context.interner),
                reduced_projection: None,
            });
        }

        let input = input_resources
            .first()
            .cloned()
            .ok_or_else(|| MirAbiError::new(ERR_INDEXED_VIEW_REQUIRES_INPUT))?;
        let extra_inputs = input_resources.into_iter().skip(1).collect();

        let mut signature = Self {
            function: function.id,
            input,
            extra_inputs,
            output: MirKernelResource {
                group: DEFAULT_KERNEL_RESOURCE_GROUP,
                binding: u32::try_from(inputs.len())
                    .map_err(|_| MirAbiError::new(ERR_KERNEL_RESOURCE_BINDING_OVERFLOW))?,
                kind: MirKernelResourceKind::StorageBuffer,
                role: MirKernelResourceRole::Output,
                access: MirKernelResourceAccess::Write,
                element_ty: output_element_ty,
                element_count: output_element_count,
                source_local: Some(output.local),
                source_name: param_source_name(output, context.interner),
                reduced_projection: None,
            },
            extra_outputs: Vec::new(),
            accumulation_buffers: Vec::new(),
            builtin_inputs: context.builtin_inputs,
            called_builtins: called_builtins_for_function(function),
            workgroup: MirWorkgroupSize { x: 1, y: 1, z: 1 },
            dispatch_size: dispatch_size_for_1d_elements(output_element_count),
        };
        maybe_append_runtime_extent(function, &mut signature, validation);
        // PRP-004 U3 (D1-A): reduction/matmul kernels carry a corrected
        // dispatch/workgroup/output-buffer contract (shared ABI law, all
        // compute targets).
        apply_collection_op_contract(function, &mut signature, validation)?;
        Ok(signature)
    }

    pub fn input_resources(&self) -> impl Iterator<Item = &MirKernelResource> {
        std::iter::once(&self.input).chain(self.extra_inputs.iter())
    }

    /// S3-A1: attach an accumulation-buffer resource — read-write device
    /// state the kernel accumulates into across the grid. Bound after the
    /// outputs. The companion-aware materializer (S3-A2) attaches these when
    /// partial gradients accumulate; the ABI carries them so the plan surface
    /// and emitters bind them after the result-tuple outputs.
    pub fn push_accumulation_buffer(&mut self, resource: MirKernelResource) {
        self.accumulation_buffers.push(resource);
    }

    /// Every output resource (the first plus the extra tuple elements), in
    /// tuple order — each binds to a distinct slot (S3-A1 result-tuple
    /// contract).
    pub fn output_resources(&self) -> impl Iterator<Item = &MirKernelResource> {
        std::iter::once(&self.output).chain(self.extra_outputs.iter())
    }

    /// All storage-buffer resources in binding order: inputs, then the first
    /// output, then the extra tuple outputs, then the accumulation buffers.
    pub fn resources(&self) -> impl Iterator<Item = &MirKernelResource> {
        self.input_resources()
            .chain(std::iter::once(&self.output))
            .chain(self.extra_outputs.iter())
            .chain(self.accumulation_buffers.iter())
    }

    /// The kernel's runtime-extent count resource, when present (U2).
    ///
    /// Both emitters read the guard limit from this binding and fall back to
    /// the static dispatch-axis limit via [`Self::oob_guard_limit`] when
    /// absent. Returns the first extent resource (CXO F2: `extra_inputs` is
    /// `Vec`-capable; Wave 1 populates at most one).
    #[must_use]
    pub fn runtime_extent(&self) -> Option<&MirKernelResource> {
        self.extra_inputs
            .iter()
            .find(|resource| resource.kind == MirKernelResourceKind::RuntimeExtent)
    }

    #[must_use]
    pub fn header_builtins(&self) -> Vec<MirKernelBuiltin> {
        combined_builtin_dependencies(
            self.builtin_inputs.iter().map(|input| input.builtin),
            &self.called_builtins,
        )
    }

    /// Classify which invocation-header params the signature's builtins
    /// require. Backend-local text mapping stays in each emitter (L2).
    #[must_use]
    pub fn builtin_usage(&self) -> MirKernelBuiltinUsage {
        MirKernelBuiltinUsage::from_builtins(&self.header_builtins())
    }

    /// Return the dispatch-size axis that corresponds to the first invocation
    /// builtin.
    ///
    /// F-036/F-049: the OOB guard must compare against the correct dispatch
    /// axis instead of always using `dispatch_size.x`.
    ///
    /// `first_builtin` is an explicit argument (L1) because a lane may
    /// augment the header builtins with body-required builtins before
    /// selecting the first one; each emitter supplies its selected first
    /// builtin.
    #[must_use]
    pub fn oob_guard_limit(&self, first_builtin: MirKernelBuiltin) -> u32 {
        match first_builtin {
            // WorkgroupSize* are compile-time constants and never drive an OOB
            // guard; fall back to the X axis bound (same as pure-X invocation ids).
            MirKernelBuiltin::GlobalInvocationX
            | MirKernelBuiltin::GlobalInvocationId
            | MirKernelBuiltin::WorkgroupIdX
            | MirKernelBuiltin::WorkgroupId
            | MirKernelBuiltin::LocalInvocationIdX
            | MirKernelBuiltin::LocalInvocationId
            | MirKernelBuiltin::WorkgroupSizeX
            | MirKernelBuiltin::WorkgroupSizeY
            | MirKernelBuiltin::WorkgroupSizeZ => {
                u32::try_from(self.dispatch_size.x).unwrap_or(u32::MAX)
            }
            MirKernelBuiltin::GlobalInvocationY
            | MirKernelBuiltin::WorkgroupIdY
            | MirKernelBuiltin::LocalInvocationIdY => {
                u32::try_from(self.dispatch_size.y).unwrap_or(u32::MAX)
            }
            MirKernelBuiltin::GlobalInvocationZ
            | MirKernelBuiltin::WorkgroupIdZ
            | MirKernelBuiltin::LocalInvocationIdZ => {
                u32::try_from(self.dispatch_size.z).unwrap_or(u32::MAX)
            }
        }
    }
}
