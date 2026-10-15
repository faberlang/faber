//! Validated MIR to a Cranelift object (goal `radix-mir-clif`, unit C3).
//!
//! The lowering is a direct walk of MIR: every function becomes one exported
//! symbol, locals and temps become Cranelift variables, and every terminator
//! becomes a jump, branch or return. A shape outside the current subset ends
//! the whole emission with a [`Decline`] that names it, never with wrong code.

use cranelift_codegen::ir::condcodes::{FloatCC, IntCC};
use cranelift_codegen::ir::{
    AbiParam, Block, FuncRef, InstBuilder, Signature, TrapCode, Type as ClType, Value, types,
};
use cranelift_codegen::settings::{self, Configurable, Flags};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use cranelift_module::{DataDescription, DataId, FuncId, Linkage, Module, default_libcall_names};
use cranelift_object::{ObjectBuilder, ObjectModule};
use radix_lexer::Interner;
use radix_mir::*;
use radix_types::{BitsWidth, NumericWidth, Primitive, Type, TypeId, TypeTable};
use rustc_hash::FxHashMap;

use crate::abi::{self, AbiTy, RuntimeSymbol};

/// A named refusal discovered during lowering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decline {
    pub shape: String,
}

impl Decline {
    pub(crate) fn new(shape: impl Into<String>) -> Self {
        Self {
            shape: shape.into(),
        }
    }
}

type Lowered<T> = Result<T, Decline>;

fn cranelift_error<E: std::fmt::Display>(error: E) -> Decline {
    Decline::new(format!("cranelift: {error}"))
}

/// Everything the lowering reads about the program being emitted.
pub struct ProgramInput<'a> {
    pub program: &'a MirProgram,
    pub types: &'a TypeTable,
    pub interner: &'a Interner,
    /// The program entry; a program without one still links and does nothing.
    pub entry: Option<MirFunctionId>,
    /// Output words: true, false, none, tuple.
    pub tokens: [String; 4],
}

/// Runtime representation of a scalar MIR type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Rep {
    /// Integers of every width, held as `i64` (exact within the 64-bit range;
    /// an operation leaving it traps).
    Int,
    /// `f64`.
    Float,
    /// `bool` as `i8` 0/1.
    Bool,
    /// A raw-bits value `bN`: the pattern zero-extended in an `i64`.
    Bits(BitsWidth),
    /// `textus`: a pointer to `[len: i64][bytes...]` (see `abi::TEXT_HEADER`).
    Text,
    /// No value.
    Unit,
}

impl Rep {
    fn cl_type(self) -> Option<ClType> {
        match self {
            Self::Int => Some(types::I64),
            Self::Float => Some(types::F64),
            Self::Bool => Some(types::I8),
            Self::Text | Self::Bits(_) => Some(types::I64),
            Self::Unit => None,
        }
    }
}

fn resolve(table: &TypeTable, mut id: TypeId) -> &Type {
    loop {
        match table.get(id) {
            Type::Alias(_, inner) => id = *inner,
            other => return other,
        }
    }
}

/// Representation of `ty`, or the decline naming why it has none yet.
pub(crate) fn rep_of(table: &TypeTable, ty: MirType) -> Lowered<Rep> {
    match resolve(table, ty.semantic_id()) {
        Type::Primitive(Primitive::Numerus) => Ok(Rep::Int),
        Type::Primitive(Primitive::Fractus) => Ok(Rep::Float),
        Type::Primitive(Primitive::Bivalens) => Ok(Rep::Bool),
        Type::Primitive(Primitive::Textus) => Ok(Rep::Text),
        Type::Bits(width) => Ok(Rep::Bits(*width)),
        Type::Primitive(Primitive::Vacuum | Primitive::Numquam) => Ok(Rep::Unit),
        Type::SizedNumeric(Primitive::Numerus, width) => match width {
            NumericWidth::I8
            | NumericWidth::I16
            | NumericWidth::I32
            | NumericWidth::I64
            | NumericWidth::U8
            | NumericWidth::U16
            | NumericWidth::U32
            | NumericWidth::U64 => Ok(Rep::Int),
            _ => Err(Decline::new("sized integer value")),
        },
        Type::ModularWord(width) | Type::SaturatingWord(width)
            if width.integer_range().is_some() =>
        {
            Ok(Rep::Int)
        }
        Type::SizedNumeric(Primitive::Fractus, NumericWidth::F64) => Ok(Rep::Float),
        Type::SizedNumeric(Primitive::Fractus, _) => Err(Decline::new("narrow float value")),
        _ => Err(Decline::new("non-scalar value")),
    }
}

fn abi_ty(ty: AbiTy, pointer: ClType) -> ClType {
    match ty {
        AbiTy::I8 => types::I8,
        AbiTy::I32 => types::I32,
        AbiTy::I64 => types::I64,
        AbiTy::F64 => types::F64,
        AbiTy::Ptr => pointer,
    }
}

/// Module-level state shared by every function being lowered.
struct ModuleState<'a> {
    module: ObjectModule,
    input: &'a ProgramInput<'a>,
    pointer: ClType,
    functions: FxHashMap<MirFunctionId, FuncId>,
    pending: Vec<MirFunctionId>,
    runtime: FxHashMap<&'static str, FuncId>,
    strings: FxHashMap<Vec<u8>, DataId>,
    texts: FxHashMap<Vec<u8>, DataId>,
}

impl<'a> ModuleState<'a> {
    fn signature_for(&self, function: &MirFunction) -> Lowered<Signature> {
        let mut signature = self.module.make_signature();
        for param in &function.params {
            if param.mode != MirParamMode::Owned {
                return Err(Decline::new("reference parameter"));
            }
            let rep = rep_of(self.input.types, param.ty)?;
            let Some(cl) = rep.cl_type() else {
                return Err(Decline::new("unit parameter"));
            };
            signature.params.push(AbiParam::new(cl));
        }
        let rep = rep_of(self.input.types, function.return_ty)?;
        if let Some(cl) = rep.cl_type() {
            signature.returns.push(AbiParam::new(cl));
        }
        Ok(signature)
    }

    fn function_id(&mut self, id: MirFunctionId) -> Lowered<FuncId> {
        if let Some(found) = self.functions.get(&id) {
            return Ok(*found);
        }
        let function = self
            .input
            .program
            .functions
            .iter()
            .find(|function| function.id == id)
            .ok_or_else(|| Decline::new("unknown callee"))?;
        let signature = self.signature_for(function)?;
        let func_id = self
            .module
            .declare_function(
                &format!("faber_clif_fn_{}", id.0),
                Linkage::Local,
                &signature,
            )
            .map_err(cranelift_error)?;
        self.functions.insert(id, func_id);
        self.pending.push(id);
        Ok(func_id)
    }

    fn runtime_id(&mut self, symbol: &RuntimeSymbol) -> Lowered<FuncId> {
        if let Some(found) = self.runtime.get(symbol.name) {
            return Ok(*found);
        }
        let mut signature = self.module.make_signature();
        for param in symbol.params {
            signature
                .params
                .push(AbiParam::new(abi_ty(*param, self.pointer)));
        }
        if let Some(ret) = symbol.ret {
            signature
                .returns
                .push(AbiParam::new(abi_ty(ret, self.pointer)));
        }
        let id = self
            .module
            .declare_function(symbol.name, Linkage::Import, &signature)
            .map_err(cranelift_error)?;
        self.runtime.insert(symbol.name, id);
        Ok(id)
    }

    /// A text literal as static data in the text handle layout.
    fn text_data(&mut self, bytes: &[u8]) -> Lowered<DataId> {
        if let Some(found) = self.texts.get(bytes) {
            return Ok(*found);
        }
        let id = self
            .module
            .declare_anonymous_data(false, false)
            .map_err(cranelift_error)?;
        let mut body = Vec::with_capacity(abi::TEXT_HEADER + bytes.len());
        body.extend_from_slice(&i64::try_from(bytes.len()).unwrap_or(0).to_ne_bytes());
        body.extend_from_slice(bytes);
        let mut description = DataDescription::new();
        description.define(body.into_boxed_slice());
        description.set_align(8);
        self.module
            .define_data(id, &description)
            .map_err(cranelift_error)?;
        self.texts.insert(bytes.to_vec(), id);
        Ok(id)
    }

    fn string_data(&mut self, bytes: &[u8]) -> Lowered<DataId> {
        if let Some(found) = self.strings.get(bytes) {
            return Ok(*found);
        }
        let id = self
            .module
            .declare_anonymous_data(false, false)
            .map_err(cranelift_error)?;
        let mut description = DataDescription::new();
        description.define(bytes.to_vec().into_boxed_slice());
        self.module
            .define_data(id, &description)
            .map_err(cranelift_error)?;
        self.strings.insert(bytes.to_vec(), id);
        Ok(id)
    }
}

/// Lower `input` to relocatable object bytes for the host triple.
///
/// # Errors
///
/// Returns the [`Decline`] naming the first shape the lowering cannot emit.
pub fn lower_program(input: &ProgramInput<'_>) -> Result<Vec<u8>, Decline> {
    let mut flags = settings::builder();
    flags.set("opt_level", "speed").map_err(cranelift_error)?;
    flags.set("is_pic", "true").map_err(cranelift_error)?;
    let isa = cranelift_native::builder_with_options(false)
        .map_err(cranelift_error)?
        .finish(Flags::new(flags))
        .map_err(cranelift_error)?;
    let pointer = isa.pointer_type();
    let builder = ObjectBuilder::new(isa, "radix-mir-clif", default_libcall_names())
        .map_err(cranelift_error)?;
    let mut state = ModuleState {
        module: ObjectModule::new(builder),
        input,
        pointer,
        functions: FxHashMap::default(),
        pending: Vec::new(),
        runtime: FxHashMap::default(),
        strings: FxHashMap::default(),
        texts: FxHashMap::default(),
    };

    let entry = match input.entry {
        Some(id) => {
            let entry_function = input
                .program
                .functions
                .iter()
                .find(|function| function.id == id)
                .ok_or_else(|| Decline::new("missing entry function"))?;
            if !entry_function.params.is_empty() {
                return Err(Decline::new("entry with parameters"));
            }
            Some(state.function_id(id)?)
        }
        None => None,
    };
    while let Some(id) = state.pending.pop() {
        let function = input
            .program
            .functions
            .iter()
            .find(|function| function.id == id)
            .ok_or_else(|| Decline::new("unknown callee"))?;
        lower_function(&mut state, function)?;
    }
    define_entry_shims(&mut state, entry)?;
    state.module.finish().emit().map_err(cranelift_error)
}

/// `faber_clif_incipit` (tokens, then the program entry) and the C `main`.
fn define_entry_shims(state: &mut ModuleState<'_>, entry: Option<FuncId>) -> Lowered<()> {
    let tokens = state.input.tokens.clone();
    let token_data: Vec<DataId> = tokens
        .iter()
        .map(|token| state.string_data(token.as_bytes()))
        .collect::<Lowered<_>>()?;
    let set_tokens = state.runtime_id(&abi::SET_DISPLAY_TOKENS)?;
    let main_target = state.runtime_id(&abi::MAIN)?;

    // faber_clif_incipit()
    let incipit_signature = state.module.make_signature();
    let incipit_id = state
        .module
        .declare_function(abi::PROGRAM_INCIPIT, Linkage::Export, &incipit_signature)
        .map_err(cranelift_error)?;
    let mut context = state.module.make_context();
    context.func.signature = incipit_signature;
    let mut builder_context = FunctionBuilderContext::new();
    {
        let mut builder = FunctionBuilder::new(&mut context.func, &mut builder_context);
        let block = builder.create_block();
        builder.switch_to_block(block);
        let mut args = Vec::new();
        for (data, token) in token_data.iter().zip(&tokens) {
            let global = state.module.declare_data_in_func(*data, builder.func);
            args.push(builder.ins().symbol_value(state.pointer, global));
            let length = i64::try_from(token.len()).unwrap_or(0);
            args.push(builder.ins().iconst(types::I64, length));
        }
        let set_ref = state.module.declare_func_in_func(set_tokens, builder.func);
        builder.ins().call(set_ref, &args);
        if let Some(entry) = entry {
            let entry_ref = state.module.declare_func_in_func(entry, builder.func);
            builder.ins().call(entry_ref, &[]);
        }
        builder.ins().return_(&[]);
        builder.seal_all_blocks();
        builder.finalize(state.module.isa().frontend_config());
    }
    state
        .module
        .define_function(incipit_id, &mut context)
        .map_err(cranelift_error)?;

    // main(argc, argv) -> __faber_clif_v1_main(argc, argv)
    let mut main_signature = state.module.make_signature();
    main_signature.params.push(AbiParam::new(types::I32));
    main_signature.params.push(AbiParam::new(state.pointer));
    main_signature.returns.push(AbiParam::new(types::I32));
    let main_id = state
        .module
        .declare_function("main", Linkage::Export, &main_signature)
        .map_err(cranelift_error)?;
    let mut context = state.module.make_context();
    context.func.signature = main_signature;
    let mut builder_context = FunctionBuilderContext::new();
    {
        let mut builder = FunctionBuilder::new(&mut context.func, &mut builder_context);
        let block = builder.create_block();
        builder.append_block_params_for_function_params(block);
        builder.switch_to_block(block);
        let count = builder.block_params(block)[0];
        let values = builder.block_params(block)[1];
        let target = state.module.declare_func_in_func(main_target, builder.func);
        let call = builder.ins().call(target, &[count, values]);
        let result = builder.inst_results(call)[0];
        builder.ins().return_(&[result]);
        builder.seal_all_blocks();
        builder.finalize(state.module.isa().frontend_config());
    }
    state
        .module
        .define_function(main_id, &mut context)
        .map_err(cranelift_error)
}

/// The policy family of an integer store slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StorePolicy {
    Trapping,
    Wrapping,
    Saturating,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Slot {
    Local(MirLocalId),
    Temp(MirTempId),
}

/// Per-function lowering state.
struct FunctionLower<'f, 'a> {
    builder: FunctionBuilder<'f>,
    state: &'f mut ModuleState<'a>,
    blocks: FxHashMap<MirBlockId, Block>,
    slots: FxHashMap<Slot, (Variable, Rep)>,
    values: FxHashMap<MirValueId, (Option<Value>, Rep)>,
    refs: FxHashMap<&'static str, FuncRef>,
    function_refs: FxHashMap<MirFunctionId, FuncRef>,
}

fn lower_function(state: &mut ModuleState<'_>, function: &MirFunction) -> Lowered<()> {
    let signature = state.signature_for(function)?;
    let func_id = *state
        .functions
        .get(&function.id)
        .ok_or_else(|| Decline::new("function not declared"))?;
    if function.error_ty.is_some() {
        return Err(Decline::new("error-typed function"));
    }
    if function.is_async || function.is_generator {
        return Err(Decline::new("async or generator function"));
    }
    let mut context = state.module.make_context();
    context.func.signature = signature;
    let mut builder_context = FunctionBuilderContext::new();
    {
        let builder = FunctionBuilder::new(&mut context.func, &mut builder_context);
        let mut lower = FunctionLower {
            builder,
            state,
            blocks: FxHashMap::default(),
            slots: FxHashMap::default(),
            values: FxHashMap::default(),
            refs: FxHashMap::default(),
            function_refs: FxHashMap::default(),
        };
        lower.run(function)?;
        lower.builder.seal_all_blocks();
        let config = lower.state.module.isa().frontend_config();
        lower.builder.finalize(config);
    }
    state
        .module
        .define_function(func_id, &mut context)
        .map_err(|error| Decline::new(format!("cranelift: {error:?}")))
}

impl FunctionLower<'_, '_> {
    fn run(&mut self, function: &MirFunction) -> Lowered<()> {
        for block in &function.blocks {
            let created = self.builder.create_block();
            self.blocks.insert(block.id, created);
        }
        let entry = *self
            .blocks
            .get(
                &function
                    .blocks
                    .first()
                    .ok_or_else(|| Decline::new("empty function"))?
                    .id,
            )
            .ok_or_else(|| Decline::new("missing entry block"))?;
        self.builder.append_block_params_for_function_params(entry);
        self.builder.switch_to_block(entry);

        let table = self.state.input.types;
        for local in &function.locals {
            let rep = rep_of(table, local.ty)?;
            self.declare(Slot::Local(local.id), rep);
        }
        for temp in &function.temps {
            let rep = rep_of(table, temp.ty)?;
            self.declare(Slot::Temp(temp.id), rep);
        }
        for (index, param) in function.params.iter().enumerate() {
            let rep = rep_of(table, param.ty)?;
            let value = self.builder.block_params(entry)[index];
            let (variable, _) = self.slot(Slot::Local(param.local))?;
            self.builder.def_var(variable, value);
            let _ = rep;
        }
        // Locals start zeroed so every variable is defined on every path.
        for (slot, (variable, rep)) in &self.slots.clone() {
            let is_param = matches!(slot, Slot::Local(id) if function.params.iter().any(|param| param.local == *id));
            if is_param {
                continue;
            }
            let zero = self.zero(*rep);
            if let Some(zero) = zero {
                self.builder.def_var(*variable, zero);
            }
        }

        for (position, block) in function.blocks.iter().enumerate() {
            let created = self.blocks[&block.id];
            if position != 0 {
                self.builder.switch_to_block(created);
            }
            for statement in &block.statements {
                self.statement(statement)?;
            }
            self.terminator(&block.terminator)?;
        }
        Ok(())
    }

    fn declare(&mut self, slot: Slot, rep: Rep) {
        if let Some(cl) = rep.cl_type() {
            let variable = self.builder.declare_var(cl);
            self.slots.insert(slot, (variable, rep));
        } else {
            // Unit slots carry no storage; a dummy variable keeps lookups total.
            let variable = self.builder.declare_var(types::I8);
            self.slots.insert(slot, (variable, Rep::Unit));
        }
    }

    fn zero(&mut self, rep: Rep) -> Option<Value> {
        match rep {
            Rep::Int => Some(self.builder.ins().iconst(types::I64, 0)),
            Rep::Float => Some(self.builder.ins().f64const(0.0)),
            Rep::Bool => Some(self.builder.ins().iconst(types::I8, 0)),
            Rep::Text | Rep::Bits(_) => Some(self.builder.ins().iconst(types::I64, 0)),
            Rep::Unit => None,
        }
    }

    fn slot(&self, slot: Slot) -> Lowered<(Variable, Rep)> {
        self.slots
            .get(&slot)
            .copied()
            .ok_or_else(|| Decline::new("undeclared storage slot"))
    }

    fn place_slot(&self, place: &MirPlace) -> Lowered<(Variable, Rep)> {
        if !place.projections.is_empty() {
            return Err(Decline::new("place projection"));
        }
        match place.base {
            MirPlaceBase::Local(id) => self.slot(Slot::Local(id)),
            MirPlaceBase::Temp(id) => self.slot(Slot::Temp(id)),
        }
    }

    fn runtime_ref(&mut self, symbol: &RuntimeSymbol) -> Lowered<FuncRef> {
        if let Some(found) = self.refs.get(symbol.name) {
            return Ok(*found);
        }
        let id = self.state.runtime_id(symbol)?;
        let reference = self
            .state
            .module
            .declare_func_in_func(id, self.builder.func);
        self.refs.insert(symbol.name, reference);
        Ok(reference)
    }

    fn function_ref(&mut self, id: MirFunctionId) -> Lowered<FuncRef> {
        if let Some(found) = self.function_refs.get(&id) {
            return Ok(*found);
        }
        let func_id = self.state.function_id(id)?;
        let reference = self
            .state
            .module
            .declare_func_in_func(func_id, self.builder.func);
        self.function_refs.insert(id, reference);
        Ok(reference)
    }

    // OPERANDS

    fn constant(&mut self, constant: &MirConstant) -> Lowered<(Option<Value>, Rep)> {
        Ok(match constant {
            MirConstant::Int(value) => (
                Some(self.builder.ins().iconst(types::I64, *value)),
                Rep::Int,
            ),
            MirConstant::UInt(value) => {
                let value = i64::try_from(*value)
                    .map_err(|_| Decline::new("integer constant beyond i64"))?;
                (Some(self.builder.ins().iconst(types::I64, value)), Rep::Int)
            }
            MirConstant::Float(value) => (Some(self.builder.ins().f64const(*value)), Rep::Float),
            MirConstant::Bool(value) => (
                Some(self.builder.ins().iconst(types::I8, i64::from(*value))),
                Rep::Bool,
            ),
            MirConstant::Unit => (None, Rep::Unit),
            MirConstant::String(symbol) => {
                let text = radix_lexer::textus_literal::decode_textus_literal(
                    self.state.input.interner.resolve(*symbol),
                );
                (Some(self.text_literal(text.as_bytes())?), Rep::Text)
            }
            _ => return Err(Decline::new("constant")),
        })
    }

    /// A literal operand stored into a `bN` slot: the pattern itself.
    fn bits_constant(
        &mut self,
        constant: &MirConstant,
        _width: BitsWidth,
    ) -> Lowered<Option<Value>> {
        let pattern = match constant {
            MirConstant::Int(literal) => *literal,
            MirConstant::UInt(literal) => i64::from_ne_bytes(literal.to_ne_bytes()),
            _ => return Err(Decline::new("bits constant")),
        };
        Ok(Some(self.builder.ins().iconst(types::I64, pattern)))
    }

    /// The handle of a static text literal.
    fn text_literal(&mut self, bytes: &[u8]) -> Lowered<Value> {
        let data = self.state.text_data(bytes)?;
        let global = self
            .state
            .module
            .declare_data_in_func(data, self.builder.func);
        Ok(self.builder.ins().symbol_value(self.state.pointer, global))
    }

    /// Call a runtime symbol; returns its first result when it has one.
    fn call_runtime(&mut self, symbol: &RuntimeSymbol, args: &[Value]) -> Lowered<Option<Value>> {
        let reference = self.runtime_ref(symbol)?;
        let call = self.builder.ins().call(reference, args);
        Ok(self.builder.inst_results(call).first().copied())
    }

    fn operand(&mut self, operand: &MirOperand) -> Lowered<(Option<Value>, Rep)> {
        match operand {
            MirOperand::Constant(constant) => self.constant(constant),
            MirOperand::Place(place) => {
                let (variable, rep) = self.place_slot(place)?;
                if rep == Rep::Unit {
                    return Ok((None, Rep::Unit));
                }
                Ok((Some(self.builder.use_var(variable)), rep))
            }
            MirOperand::Temp(id) => {
                let (variable, rep) = self.slot(Slot::Temp(*id))?;
                if rep == Rep::Unit {
                    return Ok((None, Rep::Unit));
                }
                Ok((Some(self.builder.use_var(variable)), rep))
            }
            MirOperand::Value(id) => self
                .values
                .get(id)
                .copied()
                .ok_or_else(|| Decline::new("value used before definition")),
        }
    }

    fn scalar(&mut self, operand: &MirOperand) -> Lowered<(Value, Rep)> {
        match self.operand(operand)? {
            (Some(value), rep) => Ok((value, rep)),
            (None, _) => Err(Decline::new("unit operand")),
        }
    }

    // STATEMENTS

    fn statement(&mut self, statement: &MirStatement) -> Lowered<()> {
        match &statement.kind {
            MirStatementKind::Assign { place, value } => {
                let (_, slot_rep) = self.place_slot(place)?;
                let (computed, rep) = match (&value.kind, slot_rep) {
                    (MirValueKind::Operand(MirOperand::Constant(constant)), Rep::Bits(width)) => {
                        (self.bits_constant(constant, width)?, Rep::Bits(width))
                    }
                    _ => self.value(value)?,
                };
                self.values.insert(value.id, (computed, rep));
                let (variable, slot_rep) = self.place_slot(place)?;
                if let Some(computed) = computed {
                    if slot_rep != rep {
                        return Err(Decline::new("assignment representation mismatch"));
                    }
                    self.builder.def_var(variable, computed);
                }
                Ok(())
            }
            MirStatementKind::Call {
                destination,
                callee,
                args,
            } => self.call(destination.as_ref(), callee, args),
            MirStatementKind::RuntimeCall { destination, call } => {
                self.runtime_call(destination.as_ref(), call)
            }
            MirStatementKind::Construct { .. } => Err(Decline::new("aggregate construction")),
        }
    }

    fn call(
        &mut self,
        destination: Option<&MirPlace>,
        callee: &MirCallee,
        args: &[MirOperand],
    ) -> Lowered<()> {
        let id = match callee {
            MirCallee::Function(id) => *id,
            MirCallee::Definition { source, type_args } if type_args.is_empty() => {
                let mut matches = self
                    .state
                    .input
                    .program
                    .functions
                    .iter()
                    .filter(|function| function.source == Some(*source));
                match (matches.next(), matches.next()) {
                    (Some(function), None) => function.id,
                    _ => return Err(Decline::new("ambiguous definition call")),
                }
            }
            _ => return Err(Decline::new("indirect call")),
        };
        let reference = self.function_ref(id)?;
        let mut values = Vec::with_capacity(args.len());
        for arg in args {
            values.push(self.scalar(arg)?.0);
        }
        let call = self.builder.ins().call(reference, &values);
        if let Some(destination) = destination {
            let results = self.builder.inst_results(call).to_vec();
            if let Some(result) = results.first() {
                let (variable, _) = self.place_slot(destination)?;
                self.builder.def_var(variable, *result);
            }
        }
        Ok(())
    }

    fn runtime_call(
        &mut self,
        destination: Option<&MirPlace>,
        call: &MirRuntimeCall,
    ) -> Lowered<()> {
        match &call.intrinsic {
            MirIntrinsic::Diagnostic(kind) => {
                let [argument] = call.args.as_slice() else {
                    return Err(Decline::new("diagnostic arity"));
                };
                self.print(*kind, argument)
            }
            MirIntrinsic::Convert(conversion) => {
                let Some(destination) = destination else {
                    return Err(Decline::new("conversion without destination"));
                };
                self.convert(destination, conversion, &call.args)
            }
            MirIntrinsic::FormatString { template } => {
                let Some(destination) = destination else {
                    return Err(Decline::new("format without destination"));
                };
                self.format_string(destination, *template, &call.args)
            }
            MirIntrinsic::Collection(MirCollectionOp::Length) => {
                let (Some(destination), [receiver]) = (destination, call.args.as_slice()) else {
                    return Err(Decline::new("length shape"));
                };
                let (handle, rep) = self.scalar(receiver)?;
                if rep != Rep::Text {
                    return Err(Decline::new("length of a non-text value"));
                }
                let length = self.call_runtime(&abi::TEXT_LEN, &[handle])?;
                self.define_result(destination, length, Rep::Int)
            }
            MirIntrinsic::Assert => {
                let Some(condition) = call.args.first() else {
                    return Err(Decline::new("assert arity"));
                };
                let (flag, rep) = self.scalar(condition)?;
                if rep != Rep::Bool {
                    return Err(Decline::new("assert on a non-boolean"));
                }
                let failing = self.builder.create_block();
                self.builder.set_cold_block(failing);
                let next = self.builder.create_block();
                self.builder.ins().brif(flag, next, &[], failing, &[]);
                self.builder.switch_to_block(failing);
                let message = match call.args.get(1) {
                    Some(argument) => {
                        let (value, rep) = self.scalar(argument)?;
                        self.render_text(value, rep)?
                    }
                    None => self.text_literal(b"assertion failed")?,
                };
                self.abort_with(message)?;
                self.builder.switch_to_block(next);
                Ok(())
            }
            MirIntrinsic::Panic => {
                let [argument] = call.args.as_slice() else {
                    return Err(Decline::new("panic arity"));
                };
                let (value, rep) = self.scalar(argument)?;
                let message = self.render_text(value, rep)?;
                self.abort_with(message)?;
                // The MIR block continues (a terminator follows the panic).
                let dead = self.builder.create_block();
                self.builder.switch_to_block(dead);
                Ok(())
            }
            _ => Err(Decline::new("runtime call")),
        }
    }

    /// Abort the process with `message`; the block ends in a trap.
    fn abort_with(&mut self, message: Value) -> Lowered<()> {
        self.call_runtime(&abi::ABORT_TEXT, &[message])?;
        self.builder.ins().trap(TrapCode::unwrap_user(1));
        Ok(())
    }

    /// Store `value` into `destination`, which must hold `rep`.
    fn define_result(
        &mut self,
        destination: &MirPlace,
        value: Option<Value>,
        rep: Rep,
    ) -> Lowered<()> {
        let (variable, slot_rep) = self.place_slot(destination)?;
        if slot_rep != rep {
            return Err(Decline::new("result representation mismatch"));
        }
        if let Some(value) = value {
            self.builder.def_var(variable, value);
        }
        Ok(())
    }

    /// The text handle that displays `value`.
    fn render_text(&mut self, value: Value, rep: Rep) -> Lowered<Value> {
        let rendered = match rep {
            Rep::Text => return Ok(value),
            Rep::Int => self.call_runtime(&abi::TEXT_FROM_I64, &[value])?,
            Rep::Float => {
                let narrow = self.builder.ins().iconst(types::I32, 0);
                self.call_runtime(&abi::TEXT_FROM_F64, &[value, narrow])?
            }
            Rep::Bool => self.call_runtime(&abi::TEXT_FROM_BOOL, &[value])?,
            Rep::Bits(width) => {
                let bits = self
                    .builder
                    .ins()
                    .iconst(types::I32, i64::from(width.bits()));
                self.call_runtime(&abi::TEXT_FROM_BITS, &[value, bits])?
            }
            Rep::Unit => return Err(Decline::new("unit rendered as text")),
        };
        rendered.ok_or_else(|| Decline::new("text rendering result"))
    }

    /// Interpolation: the template's `§` placeholders become display calls and
    /// concatenations, split once at compile time.
    fn format_string(
        &mut self,
        destination: &MirPlace,
        template: radix_lexer::Symbol,
        args: &[MirOperand],
    ) -> Lowered<()> {
        let template = self.state.input.interner.resolve(template).to_owned();
        let mut rendered: Vec<Value> = Vec::with_capacity(args.len());
        for arg in args {
            let (value, rep) = self.scalar(arg)?;
            rendered.push(self.render_text(value, rep)?);
        }
        enum Part {
            Literal(String),
            Argument(usize),
        }
        let mut parts: Vec<Part> = Vec::new();
        let mut literal = String::new();
        let mut chars = template.chars().peekable();
        let mut next_arg = 0usize;
        while let Some(ch) = chars.next() {
            if ch != '§' {
                literal.push(ch);
                continue;
            }
            let mut index = String::new();
            while chars.peek().is_some_and(char::is_ascii_digit) {
                if let Some(digit) = chars.next() {
                    index.push(digit);
                }
            }
            let arg_index = if index.is_empty() {
                let current = next_arg;
                next_arg += 1;
                current
            } else {
                index.parse::<usize>().unwrap_or(usize::MAX)
            };
            if arg_index < rendered.len() {
                if !literal.is_empty() {
                    parts.push(Part::Literal(std::mem::take(&mut literal)));
                }
                parts.push(Part::Argument(arg_index));
            } else {
                literal.push('§');
                literal.push_str(&index);
            }
        }
        if !literal.is_empty() {
            parts.push(Part::Literal(literal));
        }
        let mut accumulated: Option<Value> = None;
        for part in parts {
            let piece = match part {
                Part::Literal(text) => self.text_literal(text.as_bytes())?,
                Part::Argument(index) => rendered[index],
            };
            accumulated = Some(match accumulated {
                None => piece,
                Some(left) => self
                    .call_runtime(&abi::TEXT_CONCAT, &[left, piece])?
                    .ok_or_else(|| Decline::new("text concat result"))?,
            });
        }
        let result = match accumulated {
            Some(value) => value,
            None => self.text_literal(b"")?,
        };
        self.define_result(destination, Some(result), Rep::Text)
    }

    /// An implicit store into a typed integer slot (policy applied once, at
    /// the store) or a scalar identity store.
    fn convert(
        &mut self,
        destination: &MirPlace,
        conversion: &MirConversion,
        args: &[MirOperand],
    ) -> Lowered<()> {
        let [argument] = args else {
            return Err(Decline::new("conversion arity"));
        };
        let Some(site) = &conversion.store else {
            let (value, rep) = self.scalar(argument)?;
            let (variable, slot_rep) = self.place_slot(destination)?;
            if slot_rep != Rep::Text || rep == Rep::Unit {
                return Err(Decline::new("explicit conversion"));
            }
            let rendered = self.render_text(value, rep)?;
            self.builder.def_var(variable, rendered);
            return Ok(());
        };
        let (value, rep) = self.scalar(argument)?;
        let (variable, slot_rep) = self.place_slot(destination)?;
        let stored = match (rep, slot_rep) {
            (Rep::Int, Rep::Int) => self.int_store(site, conversion.target_ty, value)?,
            (Rep::Float, Rep::Float) | (Rep::Bool, Rep::Bool) | (Rep::Text, Rep::Text) => value,
            (Rep::Bits(from), Rep::Bits(to)) if from == to => value,
            _ => return Err(Decline::new("conversion between representations")),
        };
        self.builder.def_var(variable, stored);
        Ok(())
    }

    fn int_store(
        &mut self,
        site: &radix_hir::HirStoreSite,
        target: MirType,
        value: Value,
    ) -> Lowered<Value> {
        let table = self.state.input.types;
        let (policy, width) = match resolve(table, target.semantic_id()) {
            Type::Primitive(Primitive::Numerus) => return Ok(value),
            Type::SizedNumeric(Primitive::Numerus, width) => (StorePolicy::Trapping, *width),
            Type::ModularWord(width) => (StorePolicy::Wrapping, *width),
            Type::SaturatingWord(width) => (StorePolicy::Saturating, *width),
            _ => return Err(Decline::new("store into a non-integer slot")),
        };
        let Some((low, high)) = width.integer_range() else {
            return Err(Decline::new("store into an unbounded integer slot"));
        };
        let low = i64::try_from(low).map_err(|_| Decline::new("store range"))?;
        // A u64 slot's upper bound is beyond the i64 carrier: only the sign
        // can fail there.
        let high = i64::try_from(high).unwrap_or(i64::MAX);
        match policy {
            StorePolicy::Trapping => {
                if width == NumericWidth::I64 {
                    return Ok(value);
                }
                let below = {
                    let bound = self.builder.ins().iconst(types::I64, low);
                    self.builder.ins().icmp(IntCC::SignedLessThan, value, bound)
                };
                let above = {
                    let bound = self.builder.ins().iconst(types::I64, high);
                    self.builder
                        .ins()
                        .icmp(IntCC::SignedGreaterThan, value, bound)
                };
                let outside = self.builder.ins().bor(below, above);
                self.trap_store_if(outside, value, site, width)?;
                Ok(value)
            }
            StorePolicy::Saturating => {
                let low = self.builder.ins().iconst(types::I64, low);
                let high = self.builder.ins().iconst(types::I64, high);
                let raised = self.builder.ins().smax(value, low);
                Ok(self.builder.ins().smin(raised, high))
            }
            StorePolicy::Wrapping => match width {
                NumericWidth::U8 | NumericWidth::U16 | NumericWidth::U32 => {
                    let mask = self.builder.ins().iconst(types::I64, high);
                    Ok(self.builder.ins().band(value, mask))
                }
                _ => Err(Decline::new("wrapping store at this width")),
            },
        }
    }

    fn trap_store_if(
        &mut self,
        flag: Value,
        value: Value,
        site: &radix_hir::HirStoreSite,
        width: NumericWidth,
    ) -> Lowered<()> {
        let slot = site
            .slot
            .map(|slot| self.state.input.interner.resolve(slot).to_owned());
        let suffix = radix_mir::store_trap::store_trap_suffix(
            site,
            slot.as_deref(),
            width.rust_type(),
            false,
        );
        let trap = self.builder.create_block();
        self.builder.set_cold_block(trap);
        let next = self.builder.create_block();
        self.builder.ins().brif(flag, trap, &[], next, &[]);
        self.builder.switch_to_block(trap);
        let data = self.state.string_data(suffix.as_bytes())?;
        let global = self
            .state
            .module
            .declare_data_in_func(data, self.builder.func);
        let pointer = self.builder.ins().symbol_value(self.state.pointer, global);
        let length = self
            .builder
            .ins()
            .iconst(types::I64, i64::try_from(suffix.len()).unwrap_or(0));
        let unsigned = self
            .builder
            .ins()
            .iconst(types::I8, i64::from(width.is_unsigned_int()));
        let reference = self.runtime_ref(&abi::TRAP_STORE)?;
        self.builder
            .ins()
            .call(reference, &[value, pointer, length, unsigned]);
        self.builder.ins().trap(TrapCode::unwrap_user(1));
        self.builder.switch_to_block(next);
        Ok(())
    }

    fn print(&mut self, kind: MirDiagnosticKind, argument: &MirOperand) -> Lowered<()> {
        let stream = match kind {
            MirDiagnosticKind::Nota | MirDiagnosticKind::Scribe => 0,
            MirDiagnosticKind::Vide | MirDiagnosticKind::Mone => 1,
        };
        let stream = self.builder.ins().iconst(types::I32, stream);
        let (value, rep) = self.scalar(argument)?;
        match rep {
            Rep::Int => {
                let reference = self.runtime_ref(&abi::PRINT_I64)?;
                self.builder.ins().call(reference, &[stream, value]);
            }
            Rep::Float => {
                let reference = self.runtime_ref(&abi::PRINT_F64)?;
                let narrow = self.builder.ins().iconst(types::I32, 0);
                self.builder.ins().call(reference, &[stream, value, narrow]);
            }
            Rep::Bool => {
                let reference = self.runtime_ref(&abi::PRINT_BOOL)?;
                self.builder.ins().call(reference, &[stream, value]);
            }
            Rep::Bits(width) => {
                let bits = self
                    .builder
                    .ins()
                    .iconst(types::I32, i64::from(width.bits()));
                let reference = self.runtime_ref(&abi::PRINT_BITS)?;
                self.builder.ins().call(reference, &[stream, value, bits]);
            }
            Rep::Text => {
                let reference = self.runtime_ref(&abi::PRINT_TEXT)?;
                self.builder.ins().call(reference, &[stream, value]);
            }
            Rep::Unit => return Err(Decline::new("unit print")),
        }
        Ok(())
    }

    // VALUES

    fn value(&mut self, value: &MirValue) -> Lowered<(Option<Value>, Rep)> {
        match &value.kind {
            MirValueKind::Operand(operand) => self.operand(operand),
            MirValueKind::Unary { op, operand } => {
                let (computed, rep) = self.unary(*op, operand)?;
                Ok((Some(computed), rep))
            }
            MirValueKind::Binary { op, lhs, rhs } => {
                let result = rep_of(self.state.input.types, value.ty)?;
                let (computed, rep) = self.binary(*op, lhs, rhs, result)?;
                Ok((Some(computed), rep))
            }
            _ => Err(Decline::new("value kind")),
        }
    }

    fn unary(&mut self, op: MirUnOp, operand: &MirOperand) -> Lowered<(Value, Rep)> {
        let (value, rep) = self.scalar(operand)?;
        match (op, rep) {
            (MirUnOp::Neg, Rep::Int) => {
                let zero = self.builder.ins().iconst(types::I64, 0);
                Ok((self.checked_sub(zero, value)?, Rep::Int))
            }
            (MirUnOp::Neg, Rep::Float) => Ok((self.builder.ins().fneg(value), Rep::Float)),
            (MirUnOp::Not, Rep::Bool) => {
                let one = self.builder.ins().iconst(types::I8, 1);
                Ok((self.builder.ins().bxor(value, one), Rep::Bool))
            }
            (MirUnOp::BitNot, Rep::Bits(width)) => {
                let mask = if width == BitsWidth::B64 {
                    -1
                } else {
                    (1i64 << width.bits()) - 1
                };
                let mask = self.builder.ins().iconst(types::I64, mask);
                Ok((self.builder.ins().bxor(value, mask), Rep::Bits(width)))
            }
            _ => Err(Decline::new("unary operation")),
        }
    }

    fn binary(
        &mut self,
        op: MirBinOp,
        lhs: &MirOperand,
        rhs: &MirOperand,
        result: Rep,
    ) -> Lowered<(Value, Rep)> {
        let (a, a_rep) = self.scalar(lhs)?;
        let (b, b_rep) = self.scalar(rhs)?;
        if let Some(width) = [a_rep, b_rep, result]
            .into_iter()
            .find_map(|rep| match rep {
                Rep::Bits(width) => Some(width),
                _ => None,
            })
        {
            return self.bits_binary(op, (lhs, a, a_rep), (rhs, b, b_rep), width);
        }
        if a_rep != b_rep {
            return Err(Decline::new("mixed-representation operands"));
        }
        match a_rep {
            Rep::Int => self.int_binary(op, a, b),
            Rep::Float => self.float_binary(op, a, b),
            Rep::Bool => self.bool_binary(op, a, b),
            Rep::Text => self.text_binary(op, a, b),
            Rep::Bits(_) | Rep::Unit => Err(Decline::new("unit operands")),
        }
    }

    fn int_binary(&mut self, op: MirBinOp, a: Value, b: Value) -> Lowered<(Value, Rep)> {
        let compare = |this: &mut Self, cc: IntCC| (this.builder.ins().icmp(cc, a, b), Rep::Bool);
        Ok(match op {
            MirBinOp::Add => (self.checked_add(a, b)?, Rep::Int),
            MirBinOp::Sub => (self.checked_sub(a, b)?, Rep::Int),
            MirBinOp::Mul => (self.checked_mul(a, b)?, Rep::Int),
            MirBinOp::Div => (self.floor_div(a, b, false)?, Rep::Int),
            MirBinOp::Mod => (self.floor_div(a, b, true)?, Rep::Int),
            MirBinOp::Eq => compare(self, IntCC::Equal),
            MirBinOp::NotEq => compare(self, IntCC::NotEqual),
            MirBinOp::Lt => compare(self, IntCC::SignedLessThan),
            MirBinOp::Gt => compare(self, IntCC::SignedGreaterThan),
            MirBinOp::LtEq => compare(self, IntCC::SignedLessThanOrEqual),
            MirBinOp::GtEq => compare(self, IntCC::SignedGreaterThanOrEqual),
            _ => return Err(Decline::new("integer operation")),
        })
    }

    fn float_binary(&mut self, op: MirBinOp, a: Value, b: Value) -> Lowered<(Value, Rep)> {
        let compare = |this: &mut Self, cc: FloatCC| (this.builder.ins().fcmp(cc, a, b), Rep::Bool);
        Ok(match op {
            MirBinOp::Add => (self.builder.ins().fadd(a, b), Rep::Float),
            MirBinOp::Sub => (self.builder.ins().fsub(a, b), Rep::Float),
            MirBinOp::Mul => (self.builder.ins().fmul(a, b), Rep::Float),
            MirBinOp::Div => (self.builder.ins().fdiv(a, b), Rep::Float),
            MirBinOp::Eq => compare(self, FloatCC::Equal),
            MirBinOp::NotEq => compare(self, FloatCC::NotEqual),
            MirBinOp::Lt => compare(self, FloatCC::LessThan),
            MirBinOp::Gt => compare(self, FloatCC::GreaterThan),
            MirBinOp::LtEq => compare(self, FloatCC::LessThanOrEqual),
            MirBinOp::GtEq => compare(self, FloatCC::GreaterThanOrEqual),
            _ => return Err(Decline::new("float operation")),
        })
    }

    /// A bits operand as its zero-extended pattern: a bare literal operand is
    /// the pattern itself (the compiler range-checked it against `bN`).
    fn bits_pattern(
        &mut self,
        operand: &MirOperand,
        value: Value,
        rep: Rep,
        width: BitsWidth,
    ) -> Lowered<Value> {
        match (rep, operand) {
            (Rep::Bits(have), _) if have == width => Ok(value),
            (Rep::Int, MirOperand::Constant(MirConstant::Int(literal))) => {
                Ok(self.builder.ins().iconst(types::I64, *literal))
            }
            (Rep::Int, MirOperand::Constant(MirConstant::UInt(literal))) => Ok(self
                .builder
                .ins()
                .iconst(types::I64, i64::from_ne_bytes(literal.to_ne_bytes()))),
            _ => Err(Decline::new("mixed-representation operands")),
        }
    }

    /// `bN` operations at the type's own width: bitwise ops and comparisons on
    /// the pattern, guarded shifts (a count of `N` or more gives zero, a
    /// negative count aborts).
    fn bits_binary(
        &mut self,
        op: MirBinOp,
        lhs: (&MirOperand, Value, Rep),
        rhs: (&MirOperand, Value, Rep),
        width: BitsWidth,
    ) -> Lowered<(Value, Rep)> {
        let bits = i64::from(width.bits());
        let mask = if width == BitsWidth::B64 {
            -1
        } else {
            (1i64 << bits) - 1
        };
        let a = self.bits_pattern(lhs.0, lhs.1, lhs.2, width)?;
        if matches!(op, MirBinOp::Shl | MirBinOp::Shr) {
            if rhs.2 != Rep::Int {
                return Err(Decline::new("shift count representation"));
            }
            let count = rhs.1;
            let negative = {
                let zero = self.builder.ins().iconst(types::I64, 0);
                self.builder.ins().icmp(IntCC::SignedLessThan, count, zero)
            };
            self.trap_if(negative, "negative shift count")?;
            let limit = self.builder.ins().iconst(types::I64, bits);
            let inside = self
                .builder
                .ins()
                .icmp(IntCC::UnsignedLessThan, count, limit);
            let shifted = if op == MirBinOp::Shl {
                let moved = self.builder.ins().ishl(a, count);
                let mask = self.builder.ins().iconst(types::I64, mask);
                self.builder.ins().band(moved, mask)
            } else {
                self.builder.ins().ushr(a, count)
            };
            let zero = self.builder.ins().iconst(types::I64, 0);
            let result = self.builder.ins().select(inside, shifted, zero);
            return Ok((result, Rep::Bits(width)));
        }
        let b = self.bits_pattern(rhs.0, rhs.1, rhs.2, width)?;
        let compare = |this: &mut Self, cc: IntCC| (this.builder.ins().icmp(cc, a, b), Rep::Bool);
        Ok(match op {
            MirBinOp::BitAnd => (self.builder.ins().band(a, b), Rep::Bits(width)),
            MirBinOp::BitOr => (self.builder.ins().bor(a, b), Rep::Bits(width)),
            MirBinOp::BitXor => (self.builder.ins().bxor(a, b), Rep::Bits(width)),
            MirBinOp::Eq => compare(self, IntCC::Equal),
            MirBinOp::NotEq => compare(self, IntCC::NotEqual),
            MirBinOp::Lt => compare(self, IntCC::UnsignedLessThan),
            MirBinOp::Gt => compare(self, IntCC::UnsignedGreaterThan),
            MirBinOp::LtEq => compare(self, IntCC::UnsignedLessThanOrEqual),
            MirBinOp::GtEq => compare(self, IntCC::UnsignedGreaterThanOrEqual),
            _ => return Err(Decline::new("bits operation")),
        })
    }

    fn text_binary(&mut self, op: MirBinOp, a: Value, b: Value) -> Lowered<(Value, Rep)> {
        let equal = |this: &mut Self| -> Lowered<Value> {
            this.call_runtime(&abi::TEXT_EQ, &[a, b])?
                .ok_or_else(|| Decline::new("text equality result"))
        };
        let order = |this: &mut Self, cc: IntCC| -> Lowered<(Value, Rep)> {
            let ordering = this
                .call_runtime(&abi::TEXT_CMP, &[a, b])?
                .ok_or_else(|| Decline::new("text ordering result"))?;
            let zero = this.builder.ins().iconst(types::I64, 0);
            Ok((this.builder.ins().icmp(cc, ordering, zero), Rep::Bool))
        };
        match op {
            MirBinOp::Add => {
                let joined = self
                    .call_runtime(&abi::TEXT_CONCAT, &[a, b])?
                    .ok_or_else(|| Decline::new("text concat result"))?;
                Ok((joined, Rep::Text))
            }
            MirBinOp::Eq => Ok((equal(self)?, Rep::Bool)),
            MirBinOp::NotEq => {
                let same = equal(self)?;
                let one = self.builder.ins().iconst(types::I8, 1);
                Ok((self.builder.ins().bxor(same, one), Rep::Bool))
            }
            MirBinOp::Lt => order(self, IntCC::SignedLessThan),
            MirBinOp::Gt => order(self, IntCC::SignedGreaterThan),
            MirBinOp::LtEq => order(self, IntCC::SignedLessThanOrEqual),
            MirBinOp::GtEq => order(self, IntCC::SignedGreaterThanOrEqual),
            _ => Err(Decline::new("text operation")),
        }
    }

    fn bool_binary(&mut self, op: MirBinOp, a: Value, b: Value) -> Lowered<(Value, Rep)> {
        Ok(match op {
            MirBinOp::And => (self.builder.ins().band(a, b), Rep::Bool),
            MirBinOp::Or => (self.builder.ins().bor(a, b), Rep::Bool),
            MirBinOp::Eq => (self.builder.ins().icmp(IntCC::Equal, a, b), Rep::Bool),
            MirBinOp::NotEq => (self.builder.ins().icmp(IntCC::NotEqual, a, b), Rep::Bool),
            _ => return Err(Decline::new("bool operation")),
        })
    }

    // CHECKED INTEGER ARITHMETIC

    /// Branch to an out-of-line trap block when `flag` is set; continue in a
    /// fresh block. The trap block reports `message` and ends the process.
    fn trap_if(&mut self, flag: Value, message: &'static str) -> Lowered<()> {
        let trap = self.builder.create_block();
        self.builder.set_cold_block(trap);
        let next = self.builder.create_block();
        self.builder.ins().brif(flag, trap, &[], next, &[]);
        self.builder.switch_to_block(trap);
        let data = self.state.string_data(message.as_bytes())?;
        let global = self
            .state
            .module
            .declare_data_in_func(data, self.builder.func);
        let pointer = self.builder.ins().symbol_value(self.state.pointer, global);
        let length = self
            .builder
            .ins()
            .iconst(types::I64, i64::try_from(message.len()).unwrap_or(0));
        let reference = self.runtime_ref(&abi::TRAP)?;
        self.builder.ins().call(reference, &[pointer, length]);
        self.builder.ins().trap(TrapCode::unwrap_user(1));
        self.builder.switch_to_block(next);
        Ok(())
    }

    fn checked_add(&mut self, a: Value, b: Value) -> Lowered<Value> {
        let (result, flag) = self.builder.ins().sadd_overflow(a, b);
        self.overflow_guard(flag)?;
        Ok(result)
    }

    fn checked_sub(&mut self, a: Value, b: Value) -> Lowered<Value> {
        let (result, flag) = self.builder.ins().ssub_overflow(a, b);
        self.overflow_guard(flag)?;
        Ok(result)
    }

    fn checked_mul(&mut self, a: Value, b: Value) -> Lowered<Value> {
        let (result, flag) = self.builder.ins().smul_overflow(a, b);
        self.overflow_guard(flag)?;
        Ok(result)
    }

    fn overflow_guard(&mut self, flag: Value) -> Lowered<()> {
        self.trap_if(flag, "numerus overflow")
    }

    /// Floor division (`remainder == false`) or floor remainder, with a zero
    /// check. `x / -1` goes through negation so `i64::MIN / -1` is an overflow
    /// of the 64-bit carrier rather than a hardware trap.
    fn floor_div(&mut self, a: Value, b: Value, remainder: bool) -> Lowered<Value> {
        let zero = self.builder.ins().iconst(types::I64, 0);
        let is_zero = self.builder.ins().icmp(IntCC::Equal, b, zero);
        self.trap_if(is_zero, "numerus division failed")?;
        let minus_one = self.builder.ins().iconst(types::I64, -1);
        let is_minus_one = self.builder.ins().icmp(IntCC::Equal, b, minus_one);
        let one = self.builder.ins().iconst(types::I64, 1);
        let safe = self.builder.ins().select(is_minus_one, one, b);
        let quotient = self.builder.ins().sdiv(a, safe);
        let rest = self.builder.ins().srem(a, safe);
        let rest_nonzero = self.builder.ins().icmp(IntCC::NotEqual, rest, zero);
        let signs = self.builder.ins().bxor(rest, safe);
        let signs_differ = self.builder.ins().icmp(IntCC::SignedLessThan, signs, zero);
        let adjust = self.builder.ins().band(rest_nonzero, signs_differ);
        let adjust_wide = self.builder.ins().uextend(types::I64, adjust);
        let floored = self.builder.ins().isub(quotient, adjust_wide);
        let floored_rest = {
            let corrected = self.builder.ins().iadd(rest, safe);
            self.builder.ins().select(adjust, corrected, rest)
        };
        // b == -1: quotient is -a (overflow only for MIN), remainder is 0.
        let negated = {
            let (negated, flag) = self.builder.ins().ssub_overflow(zero, a);
            let guarded = self.builder.ins().band(flag, is_minus_one);
            self.trap_if(guarded, "numerus overflow")?;
            negated
        };
        Ok(if remainder {
            self.builder.ins().select(is_minus_one, zero, floored_rest)
        } else {
            self.builder.ins().select(is_minus_one, negated, floored)
        })
    }

    // TERMINATORS

    fn terminator(&mut self, terminator: &MirTerminator) -> Lowered<()> {
        match &terminator.kind {
            MirTerminatorKind::Return(value) => {
                match value {
                    Some(operand) => match self.operand(operand)? {
                        (Some(value), _) => {
                            self.builder.ins().return_(&[value]);
                        }
                        (None, _) => {
                            self.builder.ins().return_(&[]);
                        }
                    },
                    None => {
                        self.builder.ins().return_(&[]);
                    }
                }
                Ok(())
            }
            MirTerminatorKind::Goto(target) => {
                let block = self.block(*target)?;
                self.builder.ins().jump(block, &[]);
                Ok(())
            }
            MirTerminatorKind::Branch {
                condition,
                then_block,
                else_block,
            } => {
                let (condition, _) = self.scalar(condition)?;
                let then_block = self.block(*then_block)?;
                let else_block = self.block(*else_block)?;
                self.builder
                    .ins()
                    .brif(condition, then_block, &[], else_block, &[]);
                Ok(())
            }
            MirTerminatorKind::Switch {
                value,
                cases,
                default,
            } => {
                let (value, rep) = self.scalar(value)?;
                for case in cases {
                    let hit = match (&case.value, rep) {
                        (MirConstant::Int(literal), Rep::Int) => {
                            let literal = self.builder.ins().iconst(types::I64, *literal);
                            self.builder.ins().icmp(IntCC::Equal, value, literal)
                        }
                        (MirConstant::UInt(literal), Rep::Int) => {
                            let literal = i64::try_from(*literal)
                                .map_err(|_| Decline::new("switch case constant"))?;
                            let literal = self.builder.ins().iconst(types::I64, literal);
                            self.builder.ins().icmp(IntCC::Equal, value, literal)
                        }
                        (MirConstant::Bool(literal), Rep::Bool) => {
                            let literal = self.builder.ins().iconst(types::I8, i64::from(*literal));
                            self.builder.ins().icmp(IntCC::Equal, value, literal)
                        }
                        (MirConstant::Float(literal), Rep::Float) => {
                            let literal = self.builder.ins().f64const(*literal);
                            self.builder.ins().fcmp(FloatCC::Equal, value, literal)
                        }
                        (MirConstant::String(symbol), Rep::Text) => {
                            let text = radix_lexer::textus_literal::decode_textus_literal(
                                self.state.input.interner.resolve(*symbol),
                            );
                            let literal = self.text_literal(text.as_bytes())?;
                            self.call_runtime(&abi::TEXT_EQ, &[value, literal])?
                                .ok_or_else(|| Decline::new("text equality result"))?
                        }
                        _ => return Err(Decline::new("switch case constant")),
                    };
                    let target = self.block(case.target)?;
                    let next = self.builder.create_block();
                    self.builder.ins().brif(hit, target, &[], next, &[]);
                    self.builder.switch_to_block(next);
                }
                let default = self.block(*default)?;
                self.builder.ins().jump(default, &[]);
                Ok(())
            }
            MirTerminatorKind::Unreachable => {
                self.builder.ins().trap(TrapCode::unwrap_user(2));
                Ok(())
            }
            MirTerminatorKind::ReturnError(_) | MirTerminatorKind::TryCall { .. } => {
                Err(Decline::new("error channel"))
            }
        }
    }

    fn block(&self, id: MirBlockId) -> Lowered<Block> {
        self.blocks
            .get(&id)
            .copied()
            .ok_or_else(|| Decline::new("unknown block"))
    }
}
