//! Function values, captured environments and shared cells.
//!
//! A function value is a runtime object `[code, environment]`. Calling one
//! passes the environment first, so a function referenced as a value goes
//! through a shim that drops it, and a closure body takes it as a hidden
//! leading parameter whose record holds the captures in capture order.
//!
//! A binding that is rebound after capture lives in a shared cell, a
//! one-element runtime list; the binding's slot holds the cell and every read
//! or write of it goes through the `SharedCell` projection.

use radix_mir::{MirClosureValue, MirOperand};

use super::{
    AbiParam, Decline, FuncId, FunctionBuilder, FunctionBuilderContext, FunctionLower, InstBuilder,
    Lowered, MirAggregate, MirAggregateFields, MirAggregateItem, MirAggregateKind, MirCaptureMode,
    MirClosureEnvironment, MirClosureEnvironmentId, MirFunction, MirFunctionId, MirPlace,
    MirPlaceBase, MirStatement, MirStatementKind, MirTempId, MirType, Module, ModuleState, Rep,
    Slot, Type, Value, abi, cranelift_error, param_value_type, rep_of, resolve, types,
};

/// The environment of a closure body, when `function` is one.
fn environment_of(
    state: &ModuleState<'_>,
    function: MirFunctionId,
) -> Option<MirClosureEnvironment> {
    state
        .input
        .structs
        .environments
        .values()
        .find(|environment| environment.function == function)
        .cloned()
}

impl ModuleState<'_> {
    /// The environment a closure body receives, when `function` is one.
    pub(super) fn closure_environment(
        &self,
        function: MirFunctionId,
    ) -> Option<MirClosureEnvironment> {
        environment_of(self, function)
    }

    /// The shim that lets a plain function be called as a function value: the
    /// function's signature behind a leading environment parameter.
    pub(super) fn value_shim(&mut self, function: MirFunctionId) -> Lowered<FuncId> {
        if let Some(found) = self.shims.get(&function) {
            return Ok(*found);
        }
        let target = self
            .input
            .program
            .functions
            .iter()
            .find(|candidate| candidate.id == function)
            .ok_or_else(|| Decline::new("unknown function value"))?;
        let mut signature = self.signature_for(target)?;
        signature.params.insert(1, AbiParam::new(types::I64));
        let id = self
            .module
            .declare_anonymous_function(&signature)
            .map_err(cranelift_error)?;
        self.shims.insert(function, id);
        self.pending_shims.push(function);
        Ok(id)
    }
}

/// Define the shim of `function`: drop the environment, call through.
pub(super) fn define_value_shim(
    state: &mut ModuleState<'_>,
    function: MirFunctionId,
) -> Lowered<()> {
    let shim = state.value_shim(function)?;
    let target = state.function_id(function)?;
    let mut signature = state.module.make_signature();
    let callee = state
        .input
        .program
        .functions
        .iter()
        .find(|candidate| candidate.id == function)
        .ok_or_else(|| Decline::new("unknown function value"))?;
    let inner = state.signature_for(callee)?;
    signature.params.push(AbiParam::new(state.pointer));
    signature.params.push(AbiParam::new(types::I64));
    signature
        .params
        .extend(inner.params.iter().skip(1).copied());
    signature.returns.extend(inner.returns.iter().copied());
    let mut context = state.module.make_context();
    context.func.signature = signature;
    let mut builder_context = FunctionBuilderContext::new();
    {
        let mut builder = FunctionBuilder::new(&mut context.func, &mut builder_context);
        let block = builder.create_block();
        builder.append_block_params_for_function_params(block);
        builder.switch_to_block(block);
        let params = builder.block_params(block).to_vec();
        let reference = state.module.declare_func_in_func(target, builder.func);
        let mut call_args = vec![params[0]];
        call_args.extend_from_slice(&params[2..]);
        let call = builder.ins().call(reference, &call_args);
        let results = builder.inst_results(call).to_vec();
        builder.ins().return_(&results);
        builder.seal_all_blocks();
        builder.finalize(state.module.isa().frontend_config());
    }
    state
        .module
        .define_function(shim, &mut context)
        .map_err(|error| Decline::new(format!("cranelift: {error:?}")))
}

impl FunctionLower<'_, '_> {
    /// `[code, environment]` as a runtime object.
    pub(super) fn function_object(&mut self, code: Value, environment: Value) -> Lowered<Value> {
        let object = self
            .call_runtime(&abi::LIST_NEW, &[])?
            .ok_or_else(|| Decline::new("list new result"))?;
        let tag = self.builder.ins().iconst(types::I32, abi::TAG_INT);
        self.call_runtime(&abi::LIST_PUSH, &[object, tag, code])?;
        let tag = self.builder.ins().iconst(types::I32, abi::TAG_STRUCT);
        self.call_runtime(&abi::LIST_PUSH, &[object, tag, environment])?;
        Ok(object)
    }

    /// A plain function as a function value.
    pub(super) fn plain_function_value(&mut self, function: MirFunctionId) -> Lowered<Value> {
        let shim = self.state.value_shim(function)?;
        let reference = self
            .state
            .module
            .declare_func_in_func(shim, self.builder.func);
        let pointer = self.state.pointer;
        let code = self.builder.ins().func_addr(pointer, reference);
        let none = self.builder.ins().iconst(types::I64, 0);
        self.function_object(code, none)
    }

    /// A closure value: the closure body and its environment record.
    pub(super) fn closure_value(&mut self, closure: &MirClosureValue) -> Lowered<Value> {
        let (environment, rep) = self.scalar(&closure.environment)?;
        if rep != Rep::Record {
            return Err(Decline::new("closure environment representation"));
        }
        let reference = self.function_ref(closure.function)?;
        let pointer = self.state.pointer;
        let code = self.builder.ins().func_addr(pointer, reference);
        self.function_object(code, environment)
    }

    /// The code pointer and environment of a function value object.
    pub(super) fn function_parts(&mut self, object: Value) -> Lowered<(Value, Value)> {
        let zero = self.builder.ins().iconst(types::I64, 0);
        let one = self.builder.ins().iconst(types::I64, 1);
        let code = self
            .call_runtime(&abi::LIST_GET, &[object, zero])?
            .ok_or_else(|| Decline::new("function code"))?;
        let environment = self
            .call_runtime(&abi::LIST_GET, &[object, one])?
            .ok_or_else(|| Decline::new("function environment"))?;
        Ok((code, environment))
    }

    /// Call the function object `object` of function type `ty`; the result and
    /// its representation, when the function returns one.
    pub(super) fn indirect_call_value(
        &mut self,
        object: Value,
        ty: MirType,
        args: &[MirOperand],
    ) -> Lowered<Option<(Value, Rep)>> {
        let (pointer, environment) = self.function_parts(object)?;
        let table = self.state.input.types;
        let Type::Func(signature) = resolve(table, ty.semantic_id()) else {
            return Err(Decline::new("call of a non-function"));
        };
        let flags = self.type_address_flags(signature);
        let mut abi_signature = self.state.module.make_signature();
        abi_signature.params.push(AbiParam::new(self.state.pointer));
        abi_signature.params.push(AbiParam::new(types::I64));
        let mut wanted = Vec::with_capacity(signature.params.len());
        for (position, param) in signature.params.iter().enumerate() {
            let rep = rep_of(table, MirType::semantic(param_value_type(table, param.ty)))?;
            let Some(cl) = rep.cl_type() else {
                return Err(Decline::new("unit parameter"));
            };
            let cl = if flags.get(position).copied().unwrap_or(false) {
                types::I64
            } else {
                cl
            };
            abi_signature.params.push(AbiParam::new(cl));
            wanted.push(rep);
        }
        let result_rep = rep_of(table, MirType::semantic(signature.ret))?;
        if let Some(cl) = result_rep.cl_type() {
            abi_signature.returns.push(AbiParam::new(cl));
        }
        let mut values = Vec::with_capacity(args.len() + 2);
        values.push(self.arena.expect("arena entry parameter"));
        values.push(environment);
        let returns = self.call_arguments(&flags, args, &wanted, &mut values)?;
        let signature_ref = self.builder.import_signature(abi_signature);
        let call = self
            .builder
            .ins()
            .call_indirect(signature_ref, pointer, &values);
        self.read_back_arguments(args, &returns)?;
        let results = self.builder.inst_results(call).to_vec();
        Ok(results.first().map(|result| (*result, result_rep)))
    }

    /// A call whose result lands in a projected place (a shared cell's
    /// content): run it into a scratch slot, then store the slot.
    pub(super) fn statement_into_projection(
        &mut self,
        statement: &MirStatement,
        destination: &MirPlace,
    ) -> Lowered<()> {
        let ty = self.place_type(destination)?;
        let rep = rep_of(self.state.input.types, ty)?;
        let scratch = MirTempId(
            self.slots
                .keys()
                .filter_map(|slot| match slot {
                    Slot::Temp(id) => Some(id.0 + 1),
                    Slot::Local(_) => None,
                })
                .max()
                .unwrap_or(0)
                .max(self.scratch_floor),
        );
        self.scratch_floor = scratch.0 + 1;
        let slot = Slot::Temp(scratch);
        self.declare(slot, rep);
        self.slot_tys.insert(slot, ty);
        let target = MirPlace {
            base: MirPlaceBase::Temp(scratch),
            projections: Vec::new(),
        };
        let kind = match &statement.kind {
            MirStatementKind::Call { callee, args, .. } => MirStatementKind::Call {
                destination: Some(target.clone()),
                callee: callee.clone(),
                args: args.clone(),
            },
            MirStatementKind::RuntimeCall { call, .. } => MirStatementKind::RuntimeCall {
                destination: Some(target.clone()),
                call: call.clone(),
            },
            _ => return Err(Decline::new("projected destination")),
        };
        self.statement(&MirStatement {
            kind,
            span: statement.span,
        })?;
        let (variable, _) = self.slot(slot)?;
        let value = self.builder.use_var(variable);
        self.write_projected(destination, value, rep)
    }

    /// `construct closure_env`: a record of the captures in capture order.
    pub(super) fn construct_environment(
        &mut self,
        destination: &MirPlace,
        id: MirClosureEnvironmentId,
        aggregate: &MirAggregate,
    ) -> Lowered<()> {
        let environment = self
            .state
            .input
            .structs
            .environments
            .get(&id)
            .cloned()
            .ok_or_else(|| Decline::new("unknown closure environment"))?;
        let MirAggregateFields::Named(supplied) = &aggregate.fields else {
            return Err(Decline::new("closure environment fields"));
        };
        let name = self.text_literal(b"closure")?;
        let variant = self.builder.ins().iconst(types::I32, 0);
        let record = self
            .call_runtime(&abi::RECORD_NEW, &[name, variant])?
            .ok_or_else(|| Decline::new("record new result"))?;
        for (position, capture) in environment.captures.iter().enumerate() {
            let Some(item) = supplied.iter().find(|item| item.name == capture.name) else {
                return Err(Decline::new("closure capture without a value"));
            };
            let (value, rep) = if capture.mode == MirCaptureMode::Shared {
                let (value, rep) = self.scalar(&item.value)?;
                if rep != Rep::List {
                    return Err(Decline::new("shared capture without a cell"));
                }
                (value, rep)
            } else {
                self.element_operand(&item.value, capture.ty)?
            };
            let position = self.builder.ins().iconst(
                types::I64,
                i64::try_from(position).map_err(|_| Decline::new("capture index"))?,
            );
            let text = self.state.input.interner.resolve(capture.name).to_owned();
            let text = self.text_literal(text.as_bytes())?;
            let tag = self
                .builder
                .ins()
                .iconst(types::I32, Self::element_tag(rep)?);
            let bits = self.element_bits(value, rep);
            self.call_runtime(&abi::RECORD_SET, &[record, position, text, tag, bits])?;
        }
        let (variable, slot_rep) = self.place_slot(destination)?;
        if slot_rep != Rep::Record {
            return Err(Decline::new("closure environment destination"));
        }
        self.builder.def_var(variable, record);
        Ok(())
    }

    /// `construct shared_cell`: a one-element list holding the binding.
    pub(super) fn construct_cell(
        &mut self,
        destination: &MirPlace,
        aggregate: &MirAggregate,
    ) -> Lowered<()> {
        let MirAggregateFields::Ordered(items) = &aggregate.fields else {
            return Err(Decline::new("shared cell fields"));
        };
        let [MirAggregateItem::Operand(operand)] = items.as_slice() else {
            return Err(Decline::new("shared cell fields"));
        };
        let (value, rep) = self.element_operand(operand, aggregate.ty)?;
        let cell = self
            .call_runtime(&abi::LIST_NEW, &[])?
            .ok_or_else(|| Decline::new("list new result"))?;
        let tag = self
            .builder
            .ins()
            .iconst(types::I32, Self::element_tag(rep)?);
        let bits = self.element_bits(value, rep);
        self.call_runtime(&abi::LIST_PUSH, &[cell, tag, bits])?;
        let (variable, slot_rep) = self.place_slot(destination)?;
        if slot_rep != Rep::List {
            return Err(Decline::new("shared cell destination"));
        }
        self.builder.def_var(variable, cell);
        Ok(())
    }

    /// Position, type and mode of the capture `source` in the environment.
    pub(super) fn capture_slot(
        &self,
        id: MirClosureEnvironmentId,
        source: radix_hir::DefId,
    ) -> Lowered<(usize, MirType, MirCaptureMode)> {
        let environment = self
            .state
            .input
            .structs
            .environments
            .get(&id)
            .ok_or_else(|| Decline::new("unknown closure environment"))?;
        environment
            .captures
            .iter()
            .position(|capture| capture.source == source)
            .map(|position| {
                let capture = &environment.captures[position];
                (position, capture.ty, capture.mode)
            })
            .ok_or_else(|| Decline::new("unknown closure capture"))
    }
}

/// The slots a function fills with shared cells and with environments, by
/// scanning its constructions.
pub(super) fn construction_slots(
    function: &MirFunction,
) -> (handroll::FxHashSet<Slot>, handroll::FxHashSet<Slot>) {
    let mut cells = handroll::FxHashSet::default();
    let mut environments = handroll::FxHashSet::default();
    for block in &function.blocks {
        for statement in &block.statements {
            let MirStatementKind::Construct {
                destination,
                aggregate,
            } = &statement.kind
            else {
                continue;
            };
            if !destination.projections.is_empty() {
                continue;
            }
            let slot = match destination.base {
                MirPlaceBase::Local(id) => Slot::Local(id),
                MirPlaceBase::Temp(id) => Slot::Temp(id),
            };
            match aggregate.kind {
                MirAggregateKind::SharedCell => {
                    cells.insert(slot);
                }
                MirAggregateKind::ClosureEnvironment(_) => {
                    environments.insert(slot);
                }
                _ => {}
            }
        }
    }
    (cells, environments)
}
