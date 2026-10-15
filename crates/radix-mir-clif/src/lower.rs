//! Validated MIR to a Cranelift object (goal `radix-mir-clif`, unit C3).
//!
//! The lowering is a direct walk of MIR: every function becomes one exported
//! symbol, locals and temps become Cranelift variables, and every terminator
//! becomes a jump, branch or return. A shape outside the current subset ends
//! the whole emission with a [`Decline`] that names it, never with wrong code.

use cranelift_codegen::ir::condcodes::{FloatCC, IntCC};
use cranelift_codegen::ir::{
    AbiParam, Block, FuncRef, InstBuilder, MemFlagsData, Signature, TrapCode, Type as ClType,
    Value, types,
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

/// What the lowering needs to know about `genus` types: field order, field
/// types and display names, taken from the validated program.
#[derive(Debug, Clone, Default)]
pub struct StructMeta {
    pub order: FxHashMap<radix_hir::DefId, Vec<radix_lexer::Symbol>>,
    pub fields: FxHashMap<radix_hir::DefId, FxHashMap<radix_lexer::Symbol, MirType>>,
    pub names: FxHashMap<radix_hir::DefId, String>,
}

/// Everything the lowering reads about the program being emitted.
pub struct ProgramInput<'a> {
    pub structs: &'a StructMeta,
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
    /// `lista<T>`: a pointer to a runtime list of tagged elements.
    List,
    /// A `genus` instance: a pointer to a runtime record.
    Record,
    /// `T ∪ none`: null for none, else a pointer to a boxed payload.
    Opt,
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
            Self::Text | Self::Bits(_) | Self::List | Self::Record | Self::Opt => Some(types::I64),
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
        Type::Struct(_) => Ok(Rep::Record),
        Type::Option(inner) => {
            if matches!(table.get(*inner), Type::Option(_)) {
                return Err(Decline::new("nested optional value"));
            }
            rep_of(table, MirType::semantic(*inner))?;
            Ok(Rep::Opt)
        }
        // A promise carrier is its payload: async functions run to
        // completion and `cede` reads the value.
        Type::Promissum(inner) => rep_of(table, MirType::semantic(*inner)),
        Type::Array(element) => {
            rep_of(table, MirType::semantic(*element))?;
            Ok(Rep::List)
        }
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
        // A failable function returns its status, then its result, then the
        // error payload bits.
        if function.error_ty.is_some() {
            signature.returns.push(AbiParam::new(types::I8));
        }
        if let Some(cl) = rep.cl_type() {
            signature.returns.push(AbiParam::new(cl));
        }
        if let Some(error_ty) = function.error_ty {
            rep_of(self.input.types, error_ty)?;
            signature.returns.push(AbiParam::new(types::I64));
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
    slot_tys: FxHashMap<Slot, MirType>,
    ret_rep: Rep,
    error_rep: Option<Rep>,
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
    if function.is_generator {
        return Err(Decline::new("generator function"));
    }
    let ret_rep = rep_of(state.input.types, function.return_ty)?;
    let error_rep = function
        .error_ty
        .map(|error_ty| rep_of(state.input.types, error_ty))
        .transpose()?;
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
            slot_tys: FxHashMap::default(),
            ret_rep,
            error_rep,
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
            self.slot_tys.insert(Slot::Local(local.id), local.ty);
        }
        for temp in &function.temps {
            let rep = rep_of(table, temp.ty)?;
            self.declare(Slot::Temp(temp.id), rep);
            self.slot_tys.insert(Slot::Temp(temp.id), temp.ty);
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
            Rep::Text | Rep::Bits(_) | Rep::List | Rep::Record | Rep::Opt => {
                Some(self.builder.ins().iconst(types::I64, 0))
            }
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
            MirConstant::Nil => (Some(self.builder.ins().iconst(types::I64, 0)), Rep::Opt),
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

    // LISTS

    fn base_slot(place: &MirPlace) -> Slot {
        match place.base {
            MirPlaceBase::Local(id) => Slot::Local(id),
            MirPlaceBase::Temp(id) => Slot::Temp(id),
        }
    }

    /// The element type of a list type.
    fn element_type(&self, list: MirType) -> Lowered<MirType> {
        match resolve(self.state.input.types, list.semantic_id()) {
            Type::Array(element) => Ok(MirType::semantic(*element)),
            _ => Err(Decline::new("index of a non-list")),
        }
    }

    /// The `genus` definition behind a struct type.
    fn struct_def(&self, ty: MirType) -> Lowered<radix_hir::DefId> {
        match resolve(self.state.input.types, ty.semantic_id()) {
            Type::Struct(def) => Ok(*def),
            _ => Err(Decline::new("field of a non-genus")),
        }
    }

    /// Declaration index and type of field `name` of a `genus`.
    fn field_slot(
        &self,
        ty: MirType,
        name: radix_lexer::Symbol,
    ) -> Lowered<(radix_hir::DefId, usize, MirType)> {
        let def = self.struct_def(ty)?;
        let meta = self.state.input.structs;
        let index = meta
            .order
            .get(&def)
            .and_then(|order| order.iter().position(|field| *field == name))
            .ok_or_else(|| Decline::new("unknown field"))?;
        let field_ty = meta
            .fields
            .get(&def)
            .and_then(|fields| fields.get(&name))
            .copied()
            .ok_or_else(|| Decline::new("unknown field"))?;
        Ok((def, index, field_ty))
    }

    /// The type after one projection step.
    fn projected_type(&self, ty: MirType, projection: &MirProjection) -> Lowered<MirType> {
        match projection {
            MirProjection::Index(_) => self.element_type(ty),
            MirProjection::Field(name) => Ok(self.field_slot(ty, *name)?.2),
            _ => Err(Decline::new("place projection")),
        }
    }

    /// The static type of a place after its projections.
    fn place_type(&self, place: &MirPlace) -> Lowered<MirType> {
        let mut ty = *self
            .slot_tys
            .get(&Self::base_slot(place))
            .ok_or_else(|| Decline::new("undeclared storage slot"))?;
        for projection in &place.projections {
            ty = self.projected_type(ty, projection)?;
        }
        Ok(ty)
    }

    /// The static type of an operand when it is a plain place or temp.
    fn operand_type(&self, operand: &MirOperand) -> Lowered<MirType> {
        match operand {
            MirOperand::Place(place) => self.place_type(place),
            MirOperand::Temp(id) => self
                .slot_tys
                .get(&Slot::Temp(*id))
                .copied()
                .ok_or_else(|| Decline::new("undeclared storage slot")),
            _ => Err(Decline::new("operand type unknown")),
        }
    }

    /// The runtime tag of a list element of representation `rep`.
    fn element_tag(rep: Rep) -> Lowered<i64> {
        Ok(match rep {
            Rep::Int => abi::TAG_INT,
            Rep::Float => abi::TAG_FLOAT,
            Rep::Bool => abi::TAG_BOOL,
            Rep::Text => abi::TAG_TEXT,
            Rep::Bits(width) => abi::TAG_BITS + i64::from(width.bits().trailing_zeros() - 3),
            Rep::List => abi::TAG_LIST,
            Rep::Record => abi::TAG_STRUCT,
            Rep::Opt => abi::TAG_OPT,
            Rep::Unit => return Err(Decline::new("unit list element")),
        })
    }

    /// A value as the 64 bits a list element stores.
    fn element_bits(&mut self, value: Value, rep: Rep) -> Value {
        match rep {
            Rep::Float => self
                .builder
                .ins()
                .bitcast(types::I64, MemFlagsData::new(), value),
            Rep::Bool => self.builder.ins().uextend(types::I64, value),
            _ => value,
        }
    }

    /// The value held by 64 element bits of representation `rep`.
    fn element_value(&mut self, bits: Value, rep: Rep) -> Value {
        match rep {
            Rep::Float => self
                .builder
                .ins()
                .bitcast(types::F64, MemFlagsData::new(), bits),
            Rep::Bool => self.builder.ins().ireduce(types::I8, bits),
            _ => bits,
        }
    }

    /// Box `value` as a present optional.
    fn wrap_some(&mut self, value: Value, rep: Rep) -> Lowered<Value> {
        let tag = self
            .builder
            .ins()
            .iconst(types::I32, Self::element_tag(rep)?);
        let bits = self.element_bits(value, rep);
        self.call_runtime(&abi::OPT_SOME, &[tag, bits])?
            .ok_or_else(|| Decline::new("optional box result"))
    }

    /// `value` of representation `have` as a `want`: equal representations
    /// pass through, a plain value stored where an optional is expected is
    /// boxed as present.
    fn coerce(&mut self, value: Value, have: Rep, want: Rep) -> Lowered<Value> {
        if have == want {
            Ok(value)
        } else if want == Rep::Opt && have != Rep::Unit {
            self.wrap_some(value, have)
        } else {
            Err(Decline::new("representation mismatch"))
        }
    }

    /// `lhs vel rhs`: the left payload when present, else the fallback.
    fn coalesce(
        &mut self,
        lhs: (Value, Rep),
        rhs: (Value, Rep),
        result: Rep,
    ) -> Lowered<(Value, Rep)> {
        if lhs.1 != Rep::Opt {
            return Err(Decline::new("coalesce of a non-optional"));
        }
        if result == Rep::Opt {
            let fallback = self.coerce(rhs.0, rhs.1, Rep::Opt)?;
            let zero = self.builder.ins().iconst(types::I64, 0);
            let none = self.builder.ins().icmp(IntCC::Equal, lhs.0, zero);
            let picked = self.builder.ins().select(none, fallback, lhs.0);
            return Ok((picked, Rep::Opt));
        }
        if rhs.1 != result {
            return Err(Decline::new("coalesce fallback representation"));
        }
        let fallback = self.element_bits(rhs.0, rhs.1);
        let bits = self
            .call_runtime(&abi::OPT_BITS_OR, &[lhs.0, fallback])?
            .ok_or_else(|| Decline::new("coalesce result"))?;
        Ok((self.element_value(bits, result), result))
    }

    /// Optional-specific MIR operations.
    fn option_op(&mut self, op: &MirOptionOp, ty: MirType) -> Lowered<(Value, Rep)> {
        let result = rep_of(self.state.input.types, ty)?;
        match op {
            MirOptionOp::None => Ok((self.builder.ins().iconst(types::I64, 0), Rep::Opt)),
            MirOptionOp::Some(operand) => {
                let (value, rep) = self.scalar(operand)?;
                Ok((self.wrap_some(value, rep)?, Rep::Opt))
            }
            MirOptionOp::IsNil(operand) | MirOptionOp::IsNotNil(operand) => {
                let (value, rep) = self.scalar(operand)?;
                if rep != Rep::Opt {
                    return Err(Decline::new("nil test of a non-optional"));
                }
                let zero = self.builder.ins().iconst(types::I64, 0);
                let cc = if matches!(op, MirOptionOp::IsNil(_)) {
                    IntCC::Equal
                } else {
                    IntCC::NotEqual
                };
                Ok((self.builder.ins().icmp(cc, value, zero), Rep::Bool))
            }
            MirOptionOp::Unwrap { value, .. } => {
                let (handle, rep) = self.scalar(value)?;
                if rep != Rep::Opt {
                    return Err(Decline::new("unwrap of a non-optional"));
                }
                let bits = self
                    .call_runtime(&abi::OPT_BITS, &[handle])?
                    .ok_or_else(|| Decline::new("unwrap result"))?;
                Ok((self.element_value(bits, result), result))
            }
            MirOptionOp::Coalesce { value, fallback } => {
                let lhs = self.scalar(value)?;
                let rhs = self.scalar(fallback)?;
                self.coalesce(lhs, rhs, result)
            }
            MirOptionOp::Chain { .. } => Err(Decline::new("optional chain")),
        }
    }

    /// An operand that fills an element of type `element`: a bare literal into
    /// a `bN` element is the pattern itself.
    fn element_operand(&mut self, operand: &MirOperand, element: MirType) -> Lowered<(Value, Rep)> {
        let rep = rep_of(self.state.input.types, element)?;
        if let (Rep::Bits(width), MirOperand::Constant(constant)) = (rep, operand)
            && let Some(value) = self.bits_constant(constant, width)?
        {
            return Ok((value, rep));
        }
        let (value, have) = self.scalar(operand)?;
        let value = self.coerce(value, have, rep)?;
        Ok((value, rep))
    }

    /// Load a list element chain: every projection must be an index.
    fn read_projected(&mut self, place: &MirPlace) -> Lowered<(Value, Rep)> {
        let slot = Self::base_slot(place);
        let (variable, rep) = self.slot(slot)?;
        let mut ty = self.place_type(&MirPlace {
            base: place.base.clone(),
            projections: Vec::new(),
        })?;
        let mut current = self.builder.use_var(variable);
        let mut current_rep = rep;
        for projection in &place.projections {
            let next = self.projected_type(ty, projection)?;
            let next_rep = rep_of(self.state.input.types, next)?;
            let bits = match projection {
                MirProjection::Index(index) => {
                    if current_rep != Rep::List {
                        return Err(Decline::new("index of a non-list"));
                    }
                    let (index, index_rep) = self.scalar(index)?;
                    if index_rep != Rep::Int {
                        return Err(Decline::new("list index representation"));
                    }
                    self.call_runtime(&abi::LIST_GET, &[current, index])?
                }
                MirProjection::Field(name) => {
                    if current_rep != Rep::Record {
                        return Err(Decline::new("field of a non-genus"));
                    }
                    let (_, position, _) = self.field_slot(ty, *name)?;
                    let position = self.builder.ins().iconst(
                        types::I64,
                        i64::try_from(position).map_err(|_| Decline::new("field index"))?,
                    );
                    self.call_runtime(&abi::RECORD_GET, &[current, position])?
                }
                _ => return Err(Decline::new("place projection")),
            }
            .ok_or_else(|| Decline::new("projection result"))?;
            current = self.element_value(bits, next_rep);
            current_rep = next_rep;
            ty = next;
        }
        Ok((current, current_rep))
    }

    /// Store into `a[i]...[k]`: the list chain up to the last index is read,
    /// the last index is a `list_set`.
    fn write_projected(&mut self, place: &MirPlace, value: Value, rep: Rep) -> Lowered<()> {
        let Some((last, prefix)) = place.projections.split_last() else {
            return Err(Decline::new("place projection"));
        };
        let container = MirPlace {
            base: place.base.clone(),
            projections: prefix.to_vec(),
        };
        let container_ty = self.place_type(&container)?;
        let (handle, handle_rep) = if prefix.is_empty() {
            let (variable, rep) = self.place_slot(&container)?;
            (self.builder.use_var(variable), rep)
        } else {
            self.read_projected(&container)?
        };
        let tag = self
            .builder
            .ins()
            .iconst(types::I32, Self::element_tag(rep)?);
        let bits = self.element_bits(value, rep);
        match last {
            MirProjection::Index(last_index) => {
                if handle_rep != Rep::List {
                    return Err(Decline::new("index of a non-list"));
                }
                let (index, index_rep) = self.scalar(last_index)?;
                if index_rep != Rep::Int {
                    return Err(Decline::new("list index representation"));
                }
                self.call_runtime(&abi::LIST_SET, &[handle, index, tag, bits])?;
            }
            MirProjection::Field(name) => {
                if handle_rep != Rep::Record {
                    return Err(Decline::new("field of a non-genus"));
                }
                let (_, position, _) = self.field_slot(container_ty, *name)?;
                let position = self.builder.ins().iconst(
                    types::I64,
                    i64::try_from(position).map_err(|_| Decline::new("field index"))?,
                );
                let field = self.state.input.interner.resolve(*name).to_owned();
                let field = self.text_literal(field.as_bytes())?;
                self.call_runtime(&abi::RECORD_SET, &[handle, position, field, tag, bits])?;
            }
            _ => return Err(Decline::new("place projection")),
        }
        Ok(())
    }

    /// `[a, b, ...spread]` as a runtime list.
    fn construct_array(&mut self, destination: &MirPlace, aggregate: &MirAggregate) -> Lowered<()> {
        let element = self.element_type(aggregate.ty)?;
        let element_rep = rep_of(self.state.input.types, element)?;
        let MirAggregateFields::Ordered(items) = &aggregate.fields else {
            return Err(Decline::new("list fields"));
        };
        let list = self
            .call_runtime(&abi::LIST_NEW, &[])?
            .ok_or_else(|| Decline::new("list new result"))?;
        for item in items {
            match item {
                MirAggregateItem::Operand(operand) => {
                    let (value, rep) = self.element_operand(operand, element)?;
                    let tag = self
                        .builder
                        .ins()
                        .iconst(types::I32, Self::element_tag(element_rep)?);
                    let bits = self.element_bits(value, rep);
                    self.call_runtime(&abi::LIST_PUSH, &[list, tag, bits])?;
                }
                MirAggregateItem::Spread(operand) => {
                    let (source, rep) = self.scalar(operand)?;
                    if rep != Rep::List {
                        return Err(Decline::new("spread of a non-list"));
                    }
                    self.call_runtime(&abi::LIST_EXTEND, &[list, source])?;
                }
            }
        }
        let (variable, slot_rep) = self.place_slot(destination)?;
        if slot_rep != Rep::List {
            return Err(Decline::new("list destination representation"));
        }
        self.builder.def_var(variable, list);
        Ok(())
    }

    /// A `genus` construction: fields supplied by name, stored in declaration
    /// order.
    fn construct_struct(
        &mut self,
        destination: &MirPlace,
        aggregate: &MirAggregate,
    ) -> Lowered<()> {
        let MirAggregateKind::Struct(def) = aggregate.kind else {
            return Err(Decline::new("struct kind"));
        };
        let MirAggregateFields::Named(supplied) = &aggregate.fields else {
            return Err(Decline::new("struct fields"));
        };
        let meta = self.state.input.structs;
        let order = meta
            .order
            .get(&def)
            .ok_or_else(|| Decline::new("unknown genus"))?
            .clone();
        let name = meta
            .names
            .get(&def)
            .cloned()
            .unwrap_or_else(|| format!("def{}", def.0));
        let name = self.text_literal(name.as_bytes())?;
        let record = self
            .call_runtime(&abi::RECORD_NEW, &[name])?
            .ok_or_else(|| Decline::new("record new result"))?;
        for (position, field) in order.iter().enumerate() {
            let Some(item) = supplied.iter().find(|item| item.name == *field) else {
                return Err(Decline::new("genus field without a value"));
            };
            let (_, _, field_ty) = self.field_slot(aggregate.ty, *field)?;
            let (value, rep) = self.element_operand(&item.value, field_ty)?;
            let position = self.builder.ins().iconst(
                types::I64,
                i64::try_from(position).map_err(|_| Decline::new("field index"))?,
            );
            let text = self.state.input.interner.resolve(*field).to_owned();
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
            return Err(Decline::new("genus destination representation"));
        }
        self.builder.def_var(variable, record);
        Ok(())
    }

    /// Collection operations over lists (and text, where shared).
    fn collection_op(
        &mut self,
        destination: Option<&MirPlace>,
        op: MirCollectionOp,
        args: &[MirOperand],
    ) -> Lowered<()> {
        let Some(receiver) = args.first() else {
            return Err(Decline::new("collection arity"));
        };
        let (handle, rep) = self.scalar(receiver)?;
        let result = match (op, rep) {
            (MirCollectionOp::Length, Rep::Text) => {
                (self.call_runtime(&abi::TEXT_LEN, &[handle])?, Rep::Int)
            }
            (MirCollectionOp::Length, Rep::List) => {
                (self.call_runtime(&abi::LIST_LEN, &[handle])?, Rep::Int)
            }
            (MirCollectionOp::IsEmpty, Rep::Text | Rep::List) => {
                let symbol = if rep == Rep::Text {
                    &abi::TEXT_LEN
                } else {
                    &abi::LIST_LEN
                };
                let length = self
                    .call_runtime(symbol, &[handle])?
                    .ok_or_else(|| Decline::new("length result"))?;
                let zero = self.builder.ins().iconst(types::I64, 0);
                (
                    Some(self.builder.ins().icmp(IntCC::Equal, length, zero)),
                    Rep::Bool,
                )
            }
            (MirCollectionOp::Index, Rep::List) => {
                let [_, index] = args else {
                    return Err(Decline::new("index arity"));
                };
                let (index, index_rep) = self.scalar(index)?;
                if index_rep != Rep::Int {
                    return Err(Decline::new("list index representation"));
                }
                let Some(destination) = destination else {
                    return Err(Decline::new("index without destination"));
                };
                let element = self.place_type(destination)?;
                let element_rep = rep_of(self.state.input.types, element)?;
                let bits = self
                    .call_runtime(&abi::LIST_GET, &[handle, index])?
                    .ok_or_else(|| Decline::new("list get result"))?;
                (Some(self.element_value(bits, element_rep)), element_rep)
            }
            (MirCollectionOp::Append, Rep::List) => {
                let [_, item] = args else {
                    return Err(Decline::new("append arity"));
                };
                let element = self.element_type(self.operand_type(receiver)?)?;
                let (value, item_rep) = self.element_operand(item, element)?;
                let tag = self
                    .builder
                    .ins()
                    .iconst(types::I32, Self::element_tag(item_rep)?);
                let bits = self.element_bits(value, item_rep);
                self.call_runtime(&abi::LIST_PUSH, &[handle, tag, bits])?;
                return Ok(());
            }
            (MirCollectionOp::Contains, Rep::List) => {
                let [_, item] = args else {
                    return Err(Decline::new("contains arity"));
                };
                let element = self.element_type(self.operand_type(receiver)?)?;
                let (value, item_rep) = self.element_operand(item, element)?;
                let tag = self
                    .builder
                    .ins()
                    .iconst(types::I32, Self::element_tag(item_rep)?);
                let bits = self.element_bits(value, item_rep);
                (
                    self.call_runtime(&abi::LIST_CONTAINS, &[handle, tag, bits])?,
                    Rep::Bool,
                )
            }
            (MirCollectionOp::Reverse, Rep::List) => (
                self.call_runtime(&abi::LIST_REVERSED, &[handle])?,
                Rep::List,
            ),
            (MirCollectionOp::Sort, Rep::List) => {
                (self.call_runtime(&abi::LIST_SORTED, &[handle])?, Rep::List)
            }
            _ => return Err(Decline::new("collection operation")),
        };
        match (destination, result.0) {
            (Some(destination), value @ Some(_)) => {
                self.define_result(destination, value, result.1)
            }
            _ => Ok(()),
        }
    }

    fn operand(&mut self, operand: &MirOperand) -> Lowered<(Option<Value>, Rep)> {
        match operand {
            MirOperand::Constant(constant) => self.constant(constant),
            MirOperand::Place(place) if !place.projections.is_empty() => {
                let (value, rep) = self.read_projected(place)?;
                Ok((Some(value), rep))
            }
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
                let target_rep = rep_of(self.state.input.types, self.place_type(place)?)?;
                let (computed, rep) = match (&value.kind, target_rep) {
                    (MirValueKind::Operand(MirOperand::Constant(constant)), Rep::Bits(width)) => {
                        (self.bits_constant(constant, width)?, Rep::Bits(width))
                    }
                    _ => self.value(value)?,
                };
                self.values.insert(value.id, (computed, rep));
                if !place.projections.is_empty() {
                    let Some(computed) = computed else {
                        return Err(Decline::new("unit element store"));
                    };
                    let computed = self.coerce(computed, rep, target_rep)?;
                    return self.write_projected(place, computed, target_rep);
                }
                let (variable, slot_rep) = self.place_slot(place)?;
                if let Some(computed) = computed {
                    let computed = self.coerce(computed, rep, slot_rep)?;
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
            MirStatementKind::Construct {
                destination,
                aggregate,
            } if aggregate.kind == MirAggregateKind::Array => {
                self.construct_array(destination, aggregate)
            }
            MirStatementKind::Construct {
                destination,
                aggregate,
            } if matches!(aggregate.kind, MirAggregateKind::Struct(_)) => {
                self.construct_struct(destination, aggregate)
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
        let params: Vec<Rep> = {
            let callee = self
                .state
                .input
                .program
                .functions
                .iter()
                .find(|function| function.id == id)
                .ok_or_else(|| Decline::new("unknown callee"))?;
            callee
                .params
                .iter()
                .map(|param| rep_of(self.state.input.types, param.ty))
                .collect::<Lowered<_>>()?
        };
        if self
            .state
            .input
            .program
            .functions
            .iter()
            .any(|function| function.id == id && function.error_ty.is_some())
        {
            return Err(Decline::new("failable call outside a try"));
        }
        let mut values = Vec::with_capacity(args.len());
        for (position, arg) in args.iter().enumerate() {
            let (value, have) = self.scalar(arg)?;
            let value = match params.get(position) {
                Some(want) => self.coerce(value, have, *want)?,
                None => value,
            };
            values.push(value);
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

    /// A call to a failable function: branch on its status.
    fn try_call(
        &mut self,
        destination: Option<&MirPlace>,
        callee: &MirCallee,
        args: &[MirOperand],
        ok_block: MirBlockId,
        error_place: &MirPlace,
        error_block: MirBlockId,
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
        let (param_reps, result_rep, error_rep) = {
            let target = self
                .state
                .input
                .program
                .functions
                .iter()
                .find(|function| function.id == id)
                .ok_or_else(|| Decline::new("unknown callee"))?;
            let types = self.state.input.types;
            let params = target
                .params
                .iter()
                .map(|param| rep_of(types, param.ty))
                .collect::<Lowered<Vec<_>>>()?;
            let error = target
                .error_ty
                .map(|ty| rep_of(types, ty))
                .transpose()?
                .ok_or_else(|| Decline::new("try of a non-failable callee"))?;
            (params, rep_of(types, target.return_ty)?, error)
        };
        let reference = self.function_ref(id)?;
        let mut values = Vec::with_capacity(args.len());
        for (position, arg) in args.iter().enumerate() {
            let (value, have) = self.scalar(arg)?;
            let value = match param_reps.get(position) {
                Some(want) => self.coerce(value, have, *want)?,
                None => value,
            };
            values.push(value);
        }
        let call = self.builder.ins().call(reference, &values);
        let results = self.builder.inst_results(call).to_vec();
        let status = *results
            .first()
            .ok_or_else(|| Decline::new("failable call status"))?;
        let payload = *results
            .last()
            .ok_or_else(|| Decline::new("failable call payload"))?;
        let ok_target = self.block(ok_block)?;
        let error_target = self.block(error_block)?;
        let failed = self.builder.create_block();
        let succeeded = self.builder.create_block();
        self.builder.ins().brif(status, failed, &[], succeeded, &[]);

        self.builder.switch_to_block(succeeded);
        if let (Some(destination), true) = (destination, result_rep != Rep::Unit) {
            let result = *results
                .get(1)
                .ok_or_else(|| Decline::new("failable call result"))?;
            let (variable, slot_rep) = self.place_slot(destination)?;
            let result = self.coerce(result, result_rep, slot_rep)?;
            self.builder.def_var(variable, result);
        }
        self.builder.ins().jump(ok_target, &[]);

        self.builder.switch_to_block(failed);
        let (variable, slot_rep) = self.place_slot(error_place)?;
        let error = self.element_value(payload, error_rep);
        let error = self.coerce(error, error_rep, slot_rep)?;
        self.builder.def_var(variable, error);
        self.builder.ins().jump(error_target, &[]);
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
            MirIntrinsic::Collection(op) => self.collection_op(destination, *op, &call.args),
            MirIntrinsic::Duplicate => {
                let ([argument], Some(destination)) = (call.args.as_slice(), destination) else {
                    return Err(Decline::new("duplicate shape"));
                };
                let (value, rep) = self.scalar(argument)?;
                let copied = if matches!(rep, Rep::List | Rep::Record) {
                    let tag = self
                        .builder
                        .ins()
                        .iconst(types::I32, Self::element_tag(rep)?);
                    self.call_runtime(&abi::DUPLICATE, &[tag, value])?
                } else {
                    Some(value)
                };
                self.define_result(destination, copied, rep)
            }
            MirIntrinsic::Cede => {
                let ([argument], Some(destination)) = (call.args.as_slice(), destination) else {
                    return Err(Decline::new("cede shape"));
                };
                let (value, rep) = self.operand(argument)?;
                self.define_result(destination, value, rep)
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
            Rep::List => self.call_runtime(&abi::TEXT_FROM_LIST, &[value])?,
            Rep::Record => self.call_runtime(&abi::TEXT_FROM_RECORD, &[value])?,
            Rep::Opt => self.call_runtime(&abi::TEXT_FROM_OPT, &[value])?,
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
            (Rep::Float, Rep::Float)
            | (Rep::Bool, Rep::Bool)
            | (Rep::Text, Rep::Text)
            | (Rep::List, Rep::List)
            | (Rep::Record, Rep::Record)
            | (Rep::Opt, Rep::Opt) => value,
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
            Rep::List => {
                let reference = self.runtime_ref(&abi::PRINT_LIST)?;
                self.builder.ins().call(reference, &[stream, value]);
            }
            Rep::Record => {
                let reference = self.runtime_ref(&abi::PRINT_RECORD)?;
                self.builder.ins().call(reference, &[stream, value]);
            }
            Rep::Opt => {
                let reference = self.runtime_ref(&abi::PRINT_OPT)?;
                self.builder.ins().call(reference, &[stream, value]);
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
            MirValueKind::Option(op) => {
                let (computed, rep) = self.option_op(op, value.ty)?;
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
            (MirUnOp::IsNil | MirUnOp::IsNotNil, Rep::Opt) => {
                let zero = self.builder.ins().iconst(types::I64, 0);
                let cc = if op == MirUnOp::IsNil {
                    IntCC::Equal
                } else {
                    IntCC::NotEqual
                };
                Ok((self.builder.ins().icmp(cc, value, zero), Rep::Bool))
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
        if op == MirBinOp::Coalesce {
            return self.coalesce((a, a_rep), (b, b_rep), result);
        }
        if (a_rep == Rep::Opt || b_rep == Rep::Opt) && matches!(op, MirBinOp::Eq | MirBinOp::NotEq)
        {
            let a = self.coerce(a, a_rep, Rep::Opt)?;
            let b = self.coerce(b, b_rep, Rep::Opt)?;
            let same = self
                .call_runtime(&abi::OPT_EQ, &[a, b])?
                .ok_or_else(|| Decline::new("optional equality result"))?;
            if op == MirBinOp::Eq {
                return Ok((same, Rep::Bool));
            }
            let one = self.builder.ins().iconst(types::I8, 1);
            return Ok((self.builder.ins().bxor(same, one), Rep::Bool));
        }
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
            Rep::Bits(_) | Rep::List | Rep::Record | Rep::Opt | Rep::Unit => {
                Err(Decline::new("unit operands"))
            }
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
                let mut results = Vec::new();
                if self.error_rep.is_some() {
                    results.push(self.builder.ins().iconst(types::I8, 0));
                }
                if let Some(operand) = value
                    && let (Some(value), have) = self.operand(operand)?
                {
                    results.push(self.coerce(value, have, self.ret_rep)?);
                }
                if self.error_rep.is_some() {
                    results.push(self.builder.ins().iconst(types::I64, 0));
                }
                self.builder.ins().return_(&results);
                Ok(())
            }
            MirTerminatorKind::ReturnError(operand) => {
                let Some(error_rep) = self.error_rep else {
                    return Err(Decline::new("error return outside a failable function"));
                };
                let (value, have) = self.scalar(operand)?;
                let value = self.coerce(value, have, error_rep)?;
                let payload = self.element_bits(value, error_rep);
                let mut results = vec![self.builder.ins().iconst(types::I8, 1)];
                let ret_rep = self.ret_rep;
                if let Some(zero) = self.zero(ret_rep) {
                    results.push(zero);
                }
                results.push(payload);
                self.builder.ins().return_(&results);
                Ok(())
            }
            MirTerminatorKind::TryCall {
                destination,
                callee,
                args,
                ok_block,
                error_place,
                error_block,
            } => self.try_call(
                destination.as_ref(),
                callee,
                args,
                *ok_block,
                error_place,
                *error_block,
            ),
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
        }
    }

    fn block(&self, id: MirBlockId) -> Lowered<Block> {
        self.blocks
            .get(&id)
            .copied()
            .ok_or_else(|| Decline::new("unknown block"))
    }
}
