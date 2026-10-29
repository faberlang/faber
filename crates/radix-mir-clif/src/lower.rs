//! Validated MIR to a Cranelift object (goal `radix-mir-clif`, unit C3).
//!
//! The lowering is a direct walk of MIR: every function becomes one exported
//! symbol, locals and temps become Cranelift variables, and every terminator
//! becomes a jump, branch or return. A shape outside the current subset ends
//! the whole emission with a [`Decline`] that names it, never with wrong code.

use cranelift_codegen::ir::condcodes::{FloatCC, IntCC};
use cranelift_codegen::ir::{
    AbiParam, Block, BlockArg, FuncRef, InstBuilder, MemFlagsData, Signature, StackSlot,
    StackSlotData, StackSlotKind, TrapCode, Type as ClType, Value, types,
};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext, Variable};
use cranelift_module::{DataDescription, DataId, FuncId, Linkage, Module};
use handroll::{FxHashMap, FxHashSet};
use radix_lexer::Interner;
use radix_mir::{
    MirAggregate, MirAggregateFields, MirAggregateItem, MirAggregateKind, MirBinOp, MirBlockId,
    MirCallee, MirCaptureMode, MirClosureEnvironment, MirClosureEnvironmentId, MirCollectionOp,
    MirConstant, MirConversion, MirDiagnosticKind, MirFunction, MirFunctionId, MirIntrinsic,
    MirLocalId, MirNamedOperand, MirOperand, MirOptionChainLink, MirOptionOp, MirParamMode,
    MirPlace, MirPlaceBase, MirProgram, MirProjection, MirRuntimeCall, MirSermoOp, MirStatement,
    MirStatementKind, MirSwitchCase, MirTempId, MirTerminator, MirTerminatorKind, MirType, MirUnOp,
    MirValue, MirValueId, MirValueKind,
};
use radix_runtime_contract::trap::text as trap_text;
use radix_types::{
    BitsWidth, MirMathFunction, NumericWidth, Primitive, SemanticMutability, Type, TypeId,
    TypeTable,
};

use crate::abi::{self, AbiTy, RuntimeSymbol};

mod closure;
mod convert;
mod entry_shims;
mod jit;
mod lift;
mod wide;
pub(crate) use convert::{byte_element, member_tests, numeric_conversion, sequence_conversion};
pub(crate) use jit::EntryMode;
pub use jit::{JitProgram, lower_program, lower_program_jit};

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
    /// The variants of each `discretio`, for fields declared shared.
    pub enum_variants: FxHashMap<radix_hir::DefId, Vec<radix_hir::DefId>>,
    /// Parameters that admit none although declared with a plain type (the
    /// `optional` modifier): function, position and the optional type.
    pub optional_params: FxHashMap<(MirFunctionId, usize), MirType>,
    /// Captured-closure environments by id.
    pub environments: FxHashMap<MirClosureEnvironmentId, MirClosureEnvironment>,
}

/// Whether a parameter of type `ty` is passed by address when declared `mut`:
/// the callee may rebind it, and the caller sees the new value.
fn pointer_rep(table: &TypeTable, ty: MirType) -> bool {
    rep_of(table, ty).is_ok_and(|rep| rep != Rep::Unit)
}

/// Whether parameter `index` of `function` is passed by address: a `mut`
/// parameter of a value kind, whose new value the caller must see.
fn pointer_param(table: &TypeTable, function: &MirFunction, index: usize) -> bool {
    !function.is_generator
        && function
            .params
            .get(index)
            .is_some_and(|param| param.mode == MirParamMode::MutRef && pointer_rep(table, param.ty))
}

/// Whether `function` rebinds local `id` as a whole. A reference parameter
/// shares its handle, so mutation through projections reaches the caller; a
/// whole-value rebind would not, and stays declined.
fn rebinds_local(function: &MirFunction, id: MirLocalId) -> bool {
    let whole = |place: &MirPlace| {
        place.projections.is_empty()
            && matches!(place.base, MirPlaceBase::Local(found) if found == id)
    };
    function.blocks.iter().any(|block| {
        block
            .statements
            .iter()
            .any(|statement| match &statement.kind {
                MirStatementKind::Assign { place, .. } => whole(place),
                MirStatementKind::Call { destination, .. }
                | MirStatementKind::RuntimeCall { destination, .. } => {
                    destination.as_ref().is_some_and(whole)
                }
                MirStatementKind::Construct { destination, .. } => whole(destination),
            })
            || match &block.terminator.kind {
                MirTerminatorKind::TryCall {
                    destination,
                    error_place,
                    ..
                } => destination.as_ref().is_some_and(whole) || whole(error_place),
                _ => false,
            }
    })
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
    /// Integers of every width, held as `i64` in a variable. An expression at
    /// the cap computes in an `i128` carrier (see `lower/wide.rs`) and the
    /// store narrows it.
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
    /// `tabula<K, V>`: a pointer to a runtime map.
    Map,
    /// `copia<T>`: a pointer to a runtime set.
    Set,
    /// `T ∪ none`: null for none, else a pointer to a boxed payload.
    Opt,
    /// A function value: the address of a compiled function.
    Func,
    /// A tuple: a pointer to a runtime list of tagged items.
    Tuple,
    /// `textus`: a pointer to `[len: i64][bytes...]` (see `abi::TEXT_HEADER`).
    Text,
    /// `littera`: a text handle of one letter. It displays as that letter
    /// alone, and quoted with single quotes inside a collection.
    Char,
    /// `regex`: a handle to a compiled program.
    Regex,
    /// `match`: a handle to one match record.
    Match,
    /// `d64`: a handle to an exact decimal (carrier and scale).
    Dec,
    /// `inf`: a handle to an unbounded integer.
    Inf,
    /// `instans`: a handle to a nanosecond count and its precision.
    Ins,
    /// No value.
    Unit,
}

impl Rep {
    fn cl_type(self) -> Option<ClType> {
        match self {
            Self::Float => Some(types::F64),
            Self::Bool => Some(types::I8),
            Self::Int
            | Self::Text
            | Self::Char
            | Self::Regex
            | Self::Match
            | Self::Dec
            | Self::Inf
            | Self::Ins
            | Self::Bits(_)
            | Self::List
            | Self::Record
            | Self::Opt
            | Self::Map
            | Self::Set
            | Self::Func
            | Self::Tuple => Some(types::I64),
            Self::Unit => None,
        }
    }
}

/// A function-type parameter's value type: a reference mode wraps the type
/// of the shared handle the callee receives.
fn param_value_type(table: &TypeTable, id: TypeId) -> TypeId {
    match table.get(id) {
        Type::Ref(_, inner) => *inner,
        _ => id,
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
        Type::Primitive(Primitive::Littera) => Ok(Rep::Char),
        Type::Primitive(Primitive::Regex) => Ok(Rep::Regex),
        Type::Primitive(Primitive::Match) => Ok(Rep::Match),
        Type::Primitive(Primitive::Bivalens) => Ok(Rep::Bool),
        Type::Primitive(Primitive::Textus | Primitive::Ascii)
        | Type::BoundedTextus { .. }
        | Type::BoundedAscii { .. } => Ok(Rep::Text),
        Type::Bits(width) => Ok(Rep::Bits(*width)),
        Type::Struct(_) | Type::Enum(_) | Type::Interface(_) => Ok(Rep::Record),
        Type::Map(key, value) => {
            rep_of(table, MirType::semantic(*key))?;
            rep_of(table, MirType::semantic(*value))?;
            Ok(Rep::Map)
        }
        Type::Set(element) => {
            rep_of(table, MirType::semantic(*element))?;
            Ok(Rep::Set)
        }
        Type::Tuple(elements) => {
            for element in elements {
                rep_of(table, MirType::semantic(*element))?;
            }
            Ok(Rep::Tuple)
        }
        Type::Record(fields) => {
            for field in fields.values() {
                rep_of(table, MirType::semantic(*field))?;
            }
            Ok(Rep::Record)
        }
        Type::Func(signature) => function_rep(table, signature),
        // An inline union is a box of one tagged payload, the optional's
        // layout without the null; a union with a none, bits or optional
        // member stays declined.
        Type::Union(members) => union_rep(table, members),
        Type::Option(inner) => {
            if matches!(resolve(table, *inner), Type::Option(_)) {
                return Err(Decline::new("nested optional value"));
            }
            rep_of(table, MirType::semantic(*inner))?;
            Ok(Rep::Opt)
        }
        Type::Array(element) | Type::BoundedArray { element, .. } | Type::Cursor(element) => {
            rep_of(table, MirType::semantic(*element))?;
            Ok(Rep::List)
        }
        // An integer interval is a runtime list of start, end and the
        // inclusive flag.
        Type::Intervallum(element, _) => match resolve(table, *element) {
            Type::Primitive(Primitive::Numerus) => Ok(Rep::Tuple),
            _ => Err(Decline::new("interval of non-integers")),
        },
        // A bounded byte buffer is a bounded list of raw bytes.
        Type::BoundedOcteti { .. } | Type::Primitive(Primitive::Octeti) => {
            convert::byte_element(table).ok_or_else(|| Decline::new("byte buffer element"))?;
            Ok(Rep::List)
        }
        // A read-only reference is the value it borrows; a mutable one is a
        // shared handle, which only the handle kinds are.
        Type::Ref(SemanticMutability::Mutable, inner) => {
            match rep_of(table, MirType::semantic(*inner))? {
                rep @ (Rep::List | Rep::Record | Rep::Map | Rep::Set) => Ok(rep),
                _ => Err(Decline::new("reference value")),
            }
        }
        // A promise carrier is its payload: async functions run to
        // completion and `cede` reads the value. A read-only reference is
        // the value it borrows.
        Type::Promissum(inner) | Type::Ref(_, inner) => rep_of(table, MirType::semantic(*inner)),
        // The type of `null`: an optional that is always none. An unknown
        // value is a box of any tagged payload, or none.
        // Open data (`json`, `valor`) is a box of any tagged payload, as `ignotum`.
        Type::Primitive(
            Primitive::Nihil | Primitive::Ignotum | Primitive::Json | Primitive::Valor,
        ) => Ok(Rep::Opt),
        Type::Primitive(Primitive::Instans) | Type::SizedInstans(_) => Ok(Rep::Ins),
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
            NumericWidth::D64 => Ok(Rep::Dec),
            NumericWidth::Inf => Ok(Rep::Inf),
            _ => Err(Decline::new("sized integer value")),
        },
        Type::ModularWord(NumericWidth::Inf) | Type::SaturatingWord(NumericWidth::Inf) => {
            Ok(Rep::Inf)
        }
        Type::ModularWord(width) | Type::SaturatingWord(width)
            if width.integer_range().is_some() =>
        {
            Ok(Rep::Int)
        }
        // Narrow floats are carried as f64 and narrowed only on display, as
        // the runner carries them.
        Type::SizedNumeric(Primitive::Fractus, width) if width.is_float() => Ok(Rep::Float),
        Type::SizedNumeric(Primitive::Fractus, _) => Err(Decline::new("narrow float value")),
        _ => Err(Decline::new("non-scalar value")),
    }
}

/// Whether a float value or cell of type `ty` rounds through `f32`: every
/// declared float narrower than `f64` computes and stores at binary32, as
/// the runner's `round_float` does for the same widths.
fn rounds_f32(table: &TypeTable, ty: MirType) -> bool {
    matches!(
        resolve(table, ty.semantic_id()),
        Type::SizedNumeric(Primitive::Fractus, width) if *width != NumericWidth::F64
    )
}

fn union_rep(table: &TypeTable, members: &[TypeId]) -> Lowered<Rep> {
    for member in members {
        if matches!(
            resolve(table, *member),
            Type::Primitive(
                Primitive::Nihil | Primitive::Ignotum | Primitive::Json | Primitive::Valor
            ) | Type::Option(_)
                | Type::Bits(_)
                | Type::Union(_)
        ) {
            return Err(Decline::new("union member"));
        }
        rep_of(table, MirType::semantic(*member))?;
    }
    Ok(Rep::Opt)
}

fn function_rep(table: &TypeTable, signature: &radix_types::FuncSig) -> Lowered<Rep> {
    if !signature.type_params.is_empty() || signature.is_async || signature.is_generator {
        return Err(Decline::new("function value"));
    }
    for param in &signature.params {
        rep_of(table, MirType::semantic(param_value_type(table, param.ty)))?;
    }
    rep_of(table, MirType::semantic(signature.ret))?;
    if let Some(err) = signature.err {
        rep_of(table, MirType::semantic(err))?;
    }
    Ok(Rep::Func)
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

/// Where a failable call goes.
enum TryTarget {
    Direct(MirFunctionId, Option<Value>),
    Indirect(Value, Value),
}

type TryCallInfo = (TryTarget, Vec<Rep>, Rep, Rep, Option<Signature>, Vec<bool>);

/// Module-level state shared by every function being lowered.
struct ModuleState<'a> {
    /// The module being filled: an object file or an in-memory JIT image.
    module: &'a mut (dyn Module + 'static),
    input: &'a ProgramInput<'a>,
    pointer: ClType,
    functions: FxHashMap<MirFunctionId, FuncId>,
    pending: Vec<MirFunctionId>,
    runtime: FxHashMap<&'static str, FuncId>,
    strings: FxHashMap<Vec<u8>, DataId>,
    texts: FxHashMap<Vec<u8>, DataId>,
    /// Function-value shims by function, and those still to define.
    shims: FxHashMap<MirFunctionId, FuncId>,
    pending_shims: Vec<MirFunctionId>,
    /// Semantic types of the conversation handles this program opens. The
    /// type table carries no `sermo` identity, so the handle is recognized by
    /// the `SermoOpen` result type it was opened as (mirrors
    /// `capability::sermo_handle_types`).
    sermo_handles: FxHashSet<TypeId>,
}

/// The semantic types conversations are opened as, across the whole program.
///
/// Handles enter MIR only as a `SermoOpen` result, so its `return_ty` is the
/// handle identity available to a lowering that never sees the builtin
/// definition.
fn sermo_handle_types(program: &MirProgram) -> FxHashSet<TypeId> {
    let mut handles = FxHashSet::default();
    for function in &program.functions {
        for block in &function.blocks {
            for statement in &block.statements {
                if let MirStatementKind::RuntimeCall { call, .. } = &statement.kind
                    && matches!(call.intrinsic, MirIntrinsic::SermoOpen)
                {
                    handles.insert(call.return_ty.semantic_id());
                }
            }
        }
    }
    handles
}

impl ModuleState<'_> {
    /// Whether `ty` is a conversation handle this program opened.
    fn is_sermo_handle(&self, ty: MirType) -> bool {
        let mut id = ty.semantic_id();
        while let Type::Alias(_, inner) = self.input.types.get(id) {
            id = *inner;
        }
        self.sermo_handles.contains(&id)
    }

    /// The type a parameter is passed as: an optional parameter admits none.
    fn param_type(&self, function: MirFunctionId, index: usize, declared: MirType) -> MirType {
        self.input
            .structs
            .optional_params
            .get(&(function, index))
            .copied()
            .unwrap_or(declared)
    }

    fn signature_for(&self, function: &MirFunction) -> Lowered<Signature> {
        let mut signature = self.module.make_signature();
        if function.is_generator {
            // The collector: a runtime list every `cede` appends to.
            signature.params.push(AbiParam::new(types::I64));
        }
        if self.closure_environment(function.id).is_some() {
            // A closure body receives its environment record first.
            signature.params.push(AbiParam::new(types::I64));
        }
        for (index, param) in function.params.iter().enumerate() {
            if !matches!(
                param.mode,
                MirParamMode::Owned | MirParamMode::Ref | MirParamMode::MutRef | MirParamMode::Move
            ) || (matches!(param.mode, MirParamMode::Ref | MirParamMode::MutRef)
                && !pointer_param(self.input.types, function, index)
                && rebinds_local(function, param.local))
            {
                return Err(Decline::new(format!(
                    "reference parameter {index} mode {:?} of f{}",
                    param.mode, function.id.0
                )));
            }
            let rep = rep_of(
                self.input.types,
                self.param_type(function.id, index, param.ty),
            )?;
            let Some(cl) = rep.cl_type() else {
                return Err(Decline::new("unit parameter"));
            };
            if pointer_param(self.input.types, function, index) {
                signature.params.push(AbiParam::new(types::I64));
            } else {
                signature.params.push(AbiParam::new(cl));
            }
        }
        let rep = rep_of(self.input.types, function.return_ty)?;
        if function.is_generator {
            return Ok(signature);
        }
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

/// Declare and define every function the program reaches in `module`, then the
/// entry shims. Returns the id of `faber_clif_incipit`.
pub(super) fn lower_module(
    module: &mut (dyn Module + 'static),
    input: &ProgramInput<'_>,
    mode: EntryMode,
) -> Result<FuncId, Decline> {
    let pointer = module.isa().pointer_type();
    let mut state = ModuleState {
        module,
        input,
        pointer,
        functions: FxHashMap::default(),
        pending: Vec::new(),
        runtime: FxHashMap::default(),
        strings: FxHashMap::default(),
        texts: FxHashMap::default(),
        shims: FxHashMap::default(),
        pending_shims: Vec::new(),
        sermo_handles: sermo_handle_types(input.program),
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
    loop {
        if let Some(id) = state.pending.pop() {
            let function = input
                .program
                .functions
                .iter()
                .find(|function| function.id == id)
                .ok_or_else(|| Decline::new("unknown callee"))?;
            lower_function(&mut state, function)?;
        } else if let Some(id) = state.pending_shims.pop() {
            closure::define_value_shim(&mut state, id)?;
        } else {
            break;
        }
    }
    entry_shims::define_entry_shims(&mut state, entry, mode)
}

/// The policy family of an integer store slot.
/// How a modular word result reduces to its representative.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reduction {
    Mask(i64),
    Sign(u32),
}

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
    /// The return type rounds a float through `f32` (narrower than `f64`).
    ret_narrow: bool,
    error_rep: Option<Rep>,
    values: FxHashMap<MirValueId, (Option<Value>, Rep)>,
    refs: FxHashMap<&'static str, FuncRef>,
    function_refs: FxHashMap<MirFunctionId, FuncRef>,
    /// A generator's collector list, its leading parameter.
    collector: Option<Value>,
    generator: bool,
    /// Slots that hold a shared cell instead of their content.
    cells: handroll::FxHashSet<Slot>,
    /// The next scratch temporary id above every declared one.
    scratch_floor: u32,
    /// `mut` parameters passed by address: the slot, the address and the
    /// representation to write back at every exit.
    pointer_params: Vec<(Slot, Variable, Rep)>,
    /// Temporaries that hold the wide integer carrier.
    wide_slots: handroll::FxHashSet<Slot>,
    /// Wide values known to be an `i64` extended.
    narrow: handroll::FxHashSet<Value>,
    /// Wide constants beyond the `i64` carrier (a `u64` literal at or above
    /// 2^63): narrowing one is declined, not trapped at run time.
    huge: handroll::FxHashSet<Value>,
    /// The recovery region (an orphan block chain the success path skips) that
    /// defines each recovery temp: a failing conversion branches to it.
    recovery_regions: handroll::FxHashMap<MirTempId, MirBlockId>,
}

fn lower_function(state: &mut ModuleState<'_>, function: &MirFunction) -> Lowered<()> {
    let signature = state.signature_for(function)?;
    let func_id = *state
        .functions
        .get(&function.id)
        .ok_or_else(|| Decline::new("function not declared"))?;
    if function.is_generator && function.error_ty.is_some() {
        return Err(Decline::new("failable generator function"));
    }
    let ret_rep = rep_of(state.input.types, function.return_ty)?;
    let ret_narrow = ret_rep == Rep::Float && rounds_f32(state.input.types, function.return_ty);
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
            ret_narrow,
            error_rep,
            values: FxHashMap::default(),
            refs: FxHashMap::default(),
            function_refs: FxHashMap::default(),
            collector: None,
            generator: function.is_generator,
            cells: handroll::FxHashSet::default(),
            scratch_floor: 0,
            pointer_params: Vec::new(),
            wide_slots: handroll::FxHashSet::default(),
            narrow: handroll::FxHashSet::default(),
            huge: handroll::FxHashSet::default(),
            recovery_regions: handroll::FxHashMap::default(),
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

        let environment = self.state.closure_environment(function.id);
        self.declare_function_slots(function, environment.as_ref())?;
        self.bind_function_params(function, entry, environment.as_ref())?;

        let reachable = self.reachable_blocks(function);
        for (position, block) in function.blocks.iter().enumerate() {
            let created = self.blocks[&block.id];
            if position != 0 {
                self.builder.switch_to_block(created);
            }
            if !reachable.contains(&block.id) {
                self.builder.ins().trap(TrapCode::unwrap_user(1));
                continue;
            }
            for statement in &block.statements {
                self.statement(statement)?;
            }
            self.terminator(&block.terminator)?;
        }
        Ok(())
    }

    fn declare_function_slots(
        &mut self,
        function: &MirFunction,
        environment: Option<&MirClosureEnvironment>,
    ) -> Lowered<()> {
        let table = self.state.input.types;
        let (cells, environments) = closure::construction_slots(function);
        self.cells = cells;
        for local in &function.locals {
            let env_slot = environments.contains(&Slot::Local(local.id))
                || environment
                    .as_ref()
                    .is_some_and(|environment| environment.environment_local == local.id);
            let declared = function
                .params
                .iter()
                .position(|param| param.local == local.id)
                .map_or(local.ty, |index| {
                    self.state.param_type(function.id, index, local.ty)
                });
            let slot = Slot::Local(local.id);
            let rep = if self.cells.contains(&slot) {
                Rep::List
            } else if env_slot {
                Rep::Record
            } else {
                rep_of(table, declared)?
            };
            self.declare(slot, rep);
            self.slot_tys.insert(slot, declared);
        }
        let wide_temps = wide::wide_temps(table, function);
        for temp in &function.temps {
            let slot = Slot::Temp(temp.id);
            if wide_temps.contains(&temp.id)
                && !self.cells.contains(&slot)
                && !environments.contains(&slot)
            {
                let variable = self.builder.declare_var(types::I128);
                self.slots.insert(slot, (variable, Rep::Int));
                self.slot_tys.insert(slot, temp.ty);
                self.wide_slots.insert(slot);
                continue;
            }
            let rep = if self.cells.contains(&slot) {
                Rep::List
            } else if environments.contains(&slot) {
                Rep::Record
            } else {
                rep_of(table, temp.ty)?
            };
            self.declare(slot, rep);
            self.slot_tys.insert(slot, temp.ty);
        }
        Ok(())
    }

    fn bind_function_params(
        &mut self,
        function: &MirFunction,
        entry: Block,
        environment: Option<&MirClosureEnvironment>,
    ) -> Lowered<()> {
        let table = self.state.input.types;
        let mut offset = usize::from(function.is_generator);
        if function.is_generator {
            self.collector = Some(self.builder.block_params(entry)[0]);
        }
        if let Some(environment) = &environment {
            let value = self.builder.block_params(entry)[offset];
            let (variable, _) = self.slot(Slot::Local(environment.environment_local))?;
            self.builder.def_var(variable, value);
            offset += 1;
        }
        for (index, param) in function.params.iter().enumerate() {
            let rep = rep_of(table, self.state.param_type(function.id, index, param.ty))?;
            let value = self.builder.block_params(entry)[index + offset];
            let (variable, _) = self.slot(Slot::Local(param.local))?;
            if pointer_param(self.state.input.types, function, index) {
                let cl = rep
                    .cl_type()
                    .ok_or_else(|| Decline::new("unit parameter"))?;
                let address = self.builder.declare_var(types::I64);
                self.builder.def_var(address, value);
                let loaded = self.builder.ins().load(cl, MemFlagsData::new(), value, 0);
                self.builder.def_var(variable, loaded);
                self.pointer_params
                    .push((Slot::Local(param.local), address, rep));
            } else {
                self.builder.def_var(variable, value);
            }
        }
        // Locals start zeroed so every variable is defined on every path.
        for (slot, (variable, rep)) in &self.slots.clone() {
            let is_param = matches!(slot, Slot::Local(id) if function.params.iter().any(|param| param.local == *id)
                || environment.as_ref().is_some_and(|environment| environment.environment_local == *id));
            if is_param {
                continue;
            }
            let zero = if self.wide_slots.contains(slot) {
                Some(self.wide_zero())
            } else {
                self.zero(*rep)
            };
            if let Some(zero) = zero {
                self.builder.def_var(*variable, zero);
            }
        }

        Ok(())
    }

    fn reachable_blocks(&mut self, function: &MirFunction) -> FxHashSet<MirBlockId> {
        // A block no edge reaches never runs: it is closed with a trap
        // instead of being lowered, so dead code cannot decline the program.
        // A recovery region (a side-effecting `⊥` fallback) has no edge in the
        // graph: the failing conversion branches to it.
        let successors_of = |id: MirBlockId| -> Vec<MirBlockId> {
            function
                .blocks
                .iter()
                .find(|block| block.id == id)
                .map(|block| radix_mir::visit::terminator_successors(&block.terminator.kind))
                .unwrap_or_default()
        };
        let closure_from = |roots: Vec<MirBlockId>,
                            stop: &handroll::FxHashSet<MirBlockId>|
         -> handroll::FxHashSet<MirBlockId> {
            let mut seen: handroll::FxHashSet<MirBlockId> = handroll::FxHashSet::default();
            let mut pending = roots;
            while let Some(id) = pending.pop() {
                if stop.contains(&id) || !seen.insert(id) {
                    continue;
                }
                pending.extend(successors_of(id));
            }
            seen
        };
        let none: handroll::FxHashSet<MirBlockId> = handroll::FxHashSet::default();
        let mut reachable = closure_from(vec![function.blocks[0].id], &none);
        let mut has_predecessor: handroll::FxHashSet<MirBlockId> = handroll::FxHashSet::default();
        for block in &function.blocks {
            has_predecessor.extend(radix_mir::visit::terminator_successors(
                &block.terminator.kind,
            ));
        }
        let main = reachable.clone();
        for block in &function.blocks {
            if main.contains(&block.id) || has_predecessor.contains(&block.id) {
                continue;
            }
            let region = closure_from(vec![block.id], &main);
            for member in function.blocks.iter().filter(|b| region.contains(&b.id)) {
                for statement in &member.statements {
                    let defined = match &statement.kind {
                        MirStatementKind::Assign { place, .. } => Some(place),
                        MirStatementKind::Call { destination, .. }
                        | MirStatementKind::RuntimeCall { destination, .. } => destination.as_ref(),
                        MirStatementKind::Construct { destination, .. } => Some(destination),
                    };
                    if let Some(place) = defined
                        && place.projections.is_empty()
                        && let MirPlaceBase::Temp(id) = place.base
                    {
                        self.recovery_regions.entry(id).or_insert(block.id);
                    }
                }
            }
        }
        // Regions a reachable conversion can fall into run as well.
        loop {
            let mut grew = false;
            let live = reachable.clone();
            for block in function.blocks.iter().filter(|b| live.contains(&b.id)) {
                for statement in &block.statements {
                    let MirStatementKind::RuntimeCall {
                        call:
                            MirRuntimeCall {
                                intrinsic: MirIntrinsic::Convert(conversion),
                                ..
                            },
                        ..
                    } = &statement.kind
                    else {
                        continue;
                    };
                    let Some(MirOperand::Temp(id)) = &conversion.recovery else {
                        continue;
                    };
                    let Some(entry) = self.recovery_regions.get(id).copied() else {
                        continue;
                    };
                    for member in closure_from(vec![entry], &main) {
                        grew |= reachable.insert(member);
                    }
                }
            }
            if !grew {
                break;
            }
        }
        reachable
    }

    fn read_projection_bits(
        &mut self,
        projection: &MirProjection,
        current: Value,
        current_rep: Rep,
        ty: MirType,
    ) -> Lowered<Value> {
        let bits = match projection {
            MirProjection::ClosureCapture {
                environment,
                source,
            } => {
                if current_rep != Rep::Record {
                    return Err(Decline::new("capture of a non-environment"));
                }
                let (position, _, _) = self.capture_slot(*environment, *source)?;
                let position = self.builder.ins().iconst(
                    types::I64,
                    i64::try_from(position).map_err(|_| Decline::new("capture index"))?,
                );
                self.call_runtime(&abi::RECORD_GET, &[current, position])?
            }
            MirProjection::SharedCell => {
                if current_rep != Rep::List {
                    return Err(Decline::new("cell projection of a non-cell"));
                }
                let zero = self.builder.ins().iconst(types::I64, 0);
                self.call_runtime(&abi::LIST_GET, &[current, zero])?
            }
            MirProjection::Index(index) | MirProjection::ProvenIndex(index)
                if current_rep == Rep::Map =>
            {
                let (key_ty, _) = self.map_types(ty)?;
                let (key, key_rep) = self.element_operand(index, key_ty)?;
                let tag = self.cell_tag_value(key_rep, key_ty)?;
                let key = self.element_bits(key, key_rep);
                self.call_runtime(&abi::MAP_GET, &[current, tag, key])?
            }
            MirProjection::Index(index) | MirProjection::ProvenIndex(index) => {
                if current_rep != Rep::List && current_rep != Rep::Tuple {
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
                let (position, _) = self.field_slot(ty, *name)?;
                let position = self.builder.ins().iconst(
                    types::I64,
                    i64::try_from(position).map_err(|_| Decline::new("field index"))?,
                );
                self.call_runtime(&abi::RECORD_GET, &[current, position])?
            }
            MirProjection::VariantField { variant, field } => {
                if current_rep != Rep::Record {
                    return Err(Decline::new("variant field of a non-enum"));
                }
                let (position, _) = self.variant_slot(*variant, *field)?;
                let position = self.builder.ins().iconst(
                    types::I64,
                    i64::try_from(position).map_err(|_| Decline::new("field index"))?,
                );
                self.call_runtime(&abi::RECORD_GET, &[current, position])?
            }
            _ => {
                return Err(Decline::new(format!(
                    "place projection {projection:?} read"
                )));
            }
        }
        .ok_or_else(|| Decline::new("projection result"))?;
        Ok(bits)
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
            Rep::Float => Some(self.builder.ins().f64const(0.0)),
            Rep::Bool => Some(self.builder.ins().iconst(types::I8, 0)),
            Rep::Int
            | Rep::Text
            | Rep::Char
            | Rep::Regex
            | Rep::Match
            | Rep::Dec
            | Rep::Inf
            | Rep::Ins
            | Rep::Bits(_)
            | Rep::List
            | Rep::Record
            | Rep::Opt
            | Rep::Map
            | Rep::Set
            | Rep::Func
            | Rep::Tuple => Some(self.builder.ins().iconst(types::I64, 0)),
            Rep::Unit => None,
        }
    }

    fn slot(&self, slot: Slot) -> Lowered<(Variable, Rep)> {
        self.slots
            .get(&slot)
            .copied()
            .ok_or_else(|| Decline::new(format!("undeclared storage slot {slot:?}")))
    }

    fn place_slot(&self, place: &MirPlace) -> Lowered<(Variable, Rep)> {
        if !place.projections.is_empty() {
            return Err(Decline::new(format!(
                "place projection {:?} in a slot",
                place.projections.first()
            )));
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
                if let Ok(value) = i64::try_from(*value) {
                    (Some(self.builder.ins().iconst(types::I64, value)), Rep::Int)
                } else {
                    // Beyond the i64 carrier: an exact intermediate.
                    let constant = self.wide_const(i128::from(*value));
                    self.huge.insert(constant);
                    (Some(constant), Rep::Int)
                }
            }
            MirConstant::Magnus(digits) => {
                let text = self.text_literal(digits.as_bytes())?;
                let big = self.call_runtime(&abi::INF_DIGITS, &[text])?;
                (big, Rep::Inf)
            }
            MirConstant::Float(value) => (Some(self.builder.ins().f64const(*value)), Rep::Float),
            MirConstant::Bool(value) => (
                Some(self.builder.ins().iconst(types::I8, i64::from(*value))),
                Rep::Bool,
            ),
            MirConstant::Unit => (None, Rep::Unit),
            MirConstant::Nil => (Some(self.builder.ins().iconst(types::I64, 0)), Rep::Opt),
            MirConstant::Function(id) => (Some(self.plain_function_value(*id)?), Rep::Func),
            MirConstant::Regex { pattern, flags } => {
                // The body is raw; the flags prefix an inline group, as the
                // runner builds it.
                let pattern = self.state.input.interner.resolve(*pattern).to_owned();
                let flags = flags.map(|flags| self.state.input.interner.resolve(flags).to_owned());
                let mapped: String = flags
                    .iter()
                    .flat_map(|flags| flags.chars())
                    .filter(|flag| matches!(flag, 'i' | 'm' | 's' | 'u' | 'U' | 'x'))
                    .collect();
                let text = if mapped.is_empty() {
                    pattern
                } else {
                    format!("(?{mapped}){pattern}")
                };
                let literal = self.text_literal(text.as_bytes())?;
                let program = self.call_runtime(&abi::REGEX_LITERAL, &[literal])?;
                (program, Rep::Regex)
            }
            MirConstant::String(symbol) | MirConstant::Ascii(symbol) => {
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
            Type::Array(element) | Type::BoundedArray { element, .. } => {
                Ok(MirType::semantic(*element))
            }
            Type::BoundedOcteti { .. } | Type::Primitive(Primitive::Octeti) => {
                convert::byte_element(self.state.input.types)
                    .map(MirType::semantic)
                    .ok_or_else(|| Decline::new("byte buffer element"))
            }
            _ => Err(Decline::new("index of a non-list")),
        }
    }

    /// Key and value types of a map type.
    fn map_types(&self, ty: MirType) -> Lowered<(MirType, MirType)> {
        match resolve(self.state.input.types, ty.semantic_id()) {
            Type::Map(key, value) => Ok((MirType::semantic(*key), MirType::semantic(*value))),
            _ => Err(Decline::new("key access of a non-map")),
        }
    }

    /// The `genus` definition behind a struct type.
    fn struct_def(&self, ty: MirType) -> Lowered<radix_hir::DefId> {
        match resolve(self.state.input.types, ty.semantic_id()) {
            Type::Struct(def) => Ok(*def),
            _ => Err(Decline::new("field of a non-genus")),
        }
    }

    /// Fields of an anonymous record in name order: a record's runtime layout
    /// is its field names sorted, so every site agrees on indices.
    fn anonymous_fields(&self, ty: MirType) -> Option<Vec<(String, TypeId)>> {
        let Type::Record(fields) = resolve(self.state.input.types, ty.semantic_id()) else {
            return None;
        };
        let mut named: Vec<(String, TypeId)> = fields
            .iter()
            .map(|(name, ty)| (self.state.input.interner.resolve(*name).to_owned(), *ty))
            .collect();
        named.sort_by(|a, b| a.0.cmp(&b.0));
        Some(named)
    }
    /// Declaration index and type of field `name` of a `genus`.
    fn field_slot(&self, ty: MirType, name: radix_lexer::Symbol) -> Lowered<(usize, MirType)> {
        if let Some(fields) = self.anonymous_fields(ty) {
            let wanted = self.state.input.interner.resolve(name);
            let index = fields
                .iter()
                .position(|(field, _)| field == wanted)
                .ok_or_else(|| Decline::new("unknown field"))?;
            return Ok((index, MirType::semantic(fields[index].1)));
        }
        if let Type::Enum(def) = resolve(self.state.input.types, ty.semantic_id()) {
            return self.shared_field_slot(*def, name);
        }
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
        Ok((index, field_ty))
    }

    /// A field declared shared by every variant of `enum_def`: the position
    /// must agree across the variants that carry it, or the access is declined.
    fn shared_field_slot(
        &self,
        enum_def: radix_hir::DefId,
        name: radix_lexer::Symbol,
    ) -> Lowered<(usize, MirType)> {
        let meta = self.state.input.structs;
        let variants = meta
            .enum_variants
            .get(&enum_def)
            .ok_or_else(|| Decline::new("field of a non-genus"))?;
        let mut found: Option<(usize, MirType)> = None;
        for variant in variants {
            let Some(order) = meta.order.get(variant) else {
                continue;
            };
            let Some(position) = order.iter().position(|field| *field == name) else {
                return Err(Decline::new("field absent from a variant"));
            };
            let field_ty = self.variant_slot(*variant, name)?.1;
            match found {
                None => found = Some((position, field_ty)),
                Some((seen, _)) if seen == position => {}
                Some(_) => return Err(Decline::new("shared field at differing positions")),
            }
        }
        found.ok_or_else(|| Decline::new("unknown field"))
    }

    /// Declaration index and type of field `name` of an enum variant.
    fn variant_slot(
        &self,
        variant: radix_hir::DefId,
        name: radix_lexer::Symbol,
    ) -> Lowered<(usize, MirType)> {
        let meta = self.state.input.structs;
        let index = meta
            .order
            .get(&variant)
            .and_then(|order| order.iter().position(|field| *field == name))
            .ok_or_else(|| Decline::new("unknown variant field"))?;
        let field_ty = meta
            .fields
            .get(&variant)
            .and_then(|fields| fields.get(&name))
            .copied()
            .ok_or_else(|| Decline::new("unknown variant field"))?;
        Ok((index, field_ty))
    }

    /// The type after one projection step.
    fn projected_type(&self, ty: MirType, projection: &MirProjection) -> Lowered<MirType> {
        match projection {
            MirProjection::Index(index) | MirProjection::ProvenIndex(index) => {
                match resolve(self.state.input.types, ty.semantic_id()) {
                    Type::Map(_, value) => Ok(MirType::semantic(*value)),
                    Type::Tuple(elements) => match index {
                        MirOperand::Constant(MirConstant::Int(position)) => {
                            usize::try_from(*position)
                                .ok()
                                .and_then(|position| elements.get(position))
                                .map(|element| MirType::semantic(*element))
                                .ok_or_else(|| Decline::new("tuple index"))
                        }
                        _ => Err(Decline::new("tuple index")),
                    },
                    _ => self.element_type(ty),
                }
            }
            MirProjection::Field(name) => Ok(self.field_slot(ty, *name)?.1),
            MirProjection::VariantField { variant, field } => {
                Ok(self.variant_slot(*variant, *field)?.1)
            }
            MirProjection::ClosureCapture {
                environment,
                source,
            } => Ok(self.capture_slot(*environment, *source)?.1),
            // A cell is typed with its content.
            MirProjection::SharedCell => Ok(ty),
            _ => Err(Decline::new(format!(
                "place projection {projection:?} type"
            ))),
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
            Rep::Char => abi::TAG_CHAR,
            Rep::Regex => abi::TAG_REGEX,
            Rep::Match => abi::TAG_MATCH,
            Rep::Dec => abi::TAG_DEC,
            Rep::Inf => abi::TAG_INF,
            Rep::Ins => abi::TAG_INS,
            Rep::Bits(width) => abi::TAG_BITS + i64::from(width.bits().trailing_zeros() - 3),
            Rep::List | Rep::Func | Rep::Tuple => abi::TAG_LIST,
            Rep::Record => abi::TAG_STRUCT,
            Rep::Opt => abi::TAG_OPT,
            Rep::Map | Rep::Set => abi::TAG_MAP,
            Rep::Unit => return Err(Decline::new("unit list element")),
        })
    }

    /// The element tag of a container cell of representation `rep` whose
    /// static type is `ty`: a float cell of declared `f32` width carries the
    /// f32 tag, so the runtime renders and orders it at single precision;
    /// every other cell keeps its representation's tag.
    fn cell_tag(table: &TypeTable, rep: Rep, ty: MirType) -> Lowered<i64> {
        Ok(match rep {
            Rep::Float if rounds_f32(table, ty) => abi::TAG_FLOAT32,
            rep => Self::element_tag(rep)?,
        })
    }

    /// The iconst cell tag of `rep` at a cell of static type `ty`.
    fn cell_tag_value(&mut self, rep: Rep, ty: MirType) -> Lowered<Value> {
        let tag = Self::cell_tag(self.state.input.types, rep, ty)?;
        Ok(self.builder.ins().iconst(types::I32, tag))
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
        if have == want
            || matches!(
                (have, want),
                (Rep::Text, Rep::Char) | (Rep::Char, Rep::Text)
            )
        {
            Ok(value)
        } else if have == Rep::Int && want == Rep::Dec {
            self.decimal_operand(value, have, true)
        } else if have == Rep::Int && want == Rep::Inf {
            self.inf_operand(value, have)
        } else if want == Rep::Opt && have != Rep::Unit {
            self.wrap_some(value, have)
        } else if have == Rep::Opt && want != Rep::Unit {
            // A parameter or field declared `optional` keeps its plain type:
            // where a value is required it is known to be present.
            let zero = self.builder.ins().iconst(types::I64, 0);
            let absent = self.builder.ins().icmp(IntCC::Equal, value, zero);
            self.trap_if(absent, "none where a value is required")?;
            let bits = self
                .call_runtime(&abi::OPT_BITS, &[value])?
                .ok_or_else(|| Decline::new("unwrap result"))?;
            Ok(self.element_value(bits, want))
        } else {
            Err(Decline::new(format!(
                "representation mismatch {have:?} to {want:?}"
            )))
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
            // A left side that can never be none is the answer.
            let value = self.coerce(lhs.0, lhs.1, result)?;
            return Ok((value, result));
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
                    // Nothing that is not an optional is ever none.
                    let never = matches!(op, MirOptionOp::IsNil(_));
                    let _ = value;
                    return Ok((
                        self.builder.ins().iconst(types::I8, i64::from(!never)),
                        Rep::Bool,
                    ));
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
            MirOptionOp::Chain { base, link } => self.option_chain(base, link),
        }
    }

    /// `base?.field`: none when the base is none, else the field (boxed as
    /// present unless the field is itself optional).
    fn option_chain(
        &mut self,
        base: &MirOperand,
        link: &MirOptionChainLink,
    ) -> Lowered<(Value, Rep)> {
        if !matches!(
            link,
            MirOptionChainLink::Field(_)
                | MirOptionChainLink::Index(_)
                | MirOptionChainLink::Call {
                    callee: MirCallee::Value(_),
                    ..
                }
        ) {
            return Err(Decline::new("optional chain link"));
        }
        let (handle, rep) = self.scalar(base)?;
        let base_ty = self.operand_type(base)?;
        if rep != Rep::Opt {
            // A base that is already present: only the link's own optional.
            let wrapped = self.chain_step(handle, base_ty, link)?;
            return Ok((wrapped, Rep::Opt));
        }
        if matches!(
            resolve(self.state.input.types, base_ty.semantic_id()),
            Type::Primitive(Primitive::Nihil)
        ) {
            // A chain off the type of `null` stays none.
            return Ok((self.builder.ins().iconst(types::I64, 0), Rep::Opt));
        }
        let Type::Option(inner) = resolve(self.state.input.types, base_ty.semantic_id()) else {
            return Err(Decline::new("chain of a non-optional"));
        };
        let inner = MirType::semantic(*inner);
        let done = self.builder.create_block();
        self.builder.append_block_param(done, types::I64);
        let present = self.builder.create_block();
        let zero = self.builder.ins().iconst(types::I64, 0);
        let is_none = self.builder.ins().icmp(IntCC::Equal, handle, zero);
        self.builder
            .ins()
            .brif(is_none, done, &[BlockArg::Value(zero)], present, &[]);
        self.builder.switch_to_block(present);
        let payload = self
            .call_runtime(&abi::OPT_BITS, &[handle])?
            .ok_or_else(|| Decline::new("chain base result"))?;
        let wrapped = self.chain_step(payload, inner, link)?;
        self.builder.ins().jump(done, &[BlockArg::Value(wrapped)]);
        self.builder.switch_to_block(done);
        Ok((self.builder.block_params(done)[0], Rep::Opt))
    }

    /// One chain link applied to a present base of type `inner`: an optional.
    fn chain_step(
        &mut self,
        payload: Value,
        inner: MirType,
        link: &MirOptionChainLink,
    ) -> Lowered<Value> {
        let inner_rep = rep_of(self.state.input.types, inner)?;
        Ok(match link {
            MirOptionChainLink::Field(name) => {
                if inner_rep != Rep::Record {
                    return Err(Decline::new("chain field of a non-genus"));
                }
                let (position, field_ty) = self.field_slot(inner, *name)?;
                let field_rep = rep_of(self.state.input.types, field_ty)?;
                let position = self.builder.ins().iconst(
                    types::I64,
                    i64::try_from(position).map_err(|_| Decline::new("field index"))?,
                );
                let bits = self
                    .call_runtime(&abi::RECORD_GET, &[payload, position])?
                    .ok_or_else(|| Decline::new("chain field result"))?;
                let value = self.element_value(bits, field_rep);
                self.coerce(value, field_rep, Rep::Opt)?
            }
            MirOptionChainLink::Index(index) if inner_rep == Rep::Map => {
                let (key_ty, value_ty) = self.map_types(inner)?;
                if rep_of(self.state.input.types, value_ty)? == Rep::Opt {
                    return Err(Decline::new("chain index of an optional value"));
                }
                let (key, key_rep) = self.element_operand(index, key_ty)?;
                let tag = self.cell_tag_value(key_rep, key_ty)?;
                let key = self.element_bits(key, key_rep);
                self.call_runtime(&abi::MAP_GET_OPT, &[payload, tag, key])?
                    .ok_or_else(|| Decline::new("chain index result"))?
            }
            MirOptionChainLink::Index(index) if inner_rep == Rep::List => {
                let element = self.element_type(inner)?;
                if rep_of(self.state.input.types, element)? == Rep::Opt {
                    return Err(Decline::new("chain index of an optional element"));
                }
                let (index, index_rep) = self.scalar(index)?;
                if index_rep != Rep::Int {
                    return Err(Decline::new("list index representation"));
                }
                self.call_runtime(&abi::LIST_GET_OPT, &[payload, index])?
                    .ok_or_else(|| Decline::new("chain index result"))?
            }
            // `f?(args)` on a present function value: the call's result is
            // boxed as present unless it is itself optional. The arguments are
            // evaluated by the caller only on the present path.
            MirOptionChainLink::Call {
                callee: MirCallee::Value(_),
                args,
            } if inner_rep == Rep::Func => {
                let (result, result_rep) = self
                    .indirect_call_value(payload, inner, args)?
                    .ok_or_else(|| Decline::new("chain call result"))?;
                self.coerce(result, result_rep, Rep::Opt)?
            }
            _ => return Err(Decline::new("optional chain link")),
        })
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
        // A container cell carries the real float width: an element narrower
        // than `f64` stores its value rounded through `f32`.
        if rep == Rep::Float {
            let value = self.round_float_cell(value, element);
            return Ok((value, rep));
        }
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
            if let (MirProjection::Index(index) | MirProjection::ProvenIndex(index), Rep::Text) =
                (projection, current_rep)
            {
                let (index, index_rep) = self.scalar(index)?;
                if index_rep == Rep::Tuple {
                    // The runtime ends the run itself on a window outside the text.
                    current = self
                        .call_runtime(&abi::TEXT_RANGE, &[current, index])?
                        .ok_or_else(|| Decline::new("text window result"))?;
                    continue;
                }
                if index_rep != Rep::Int {
                    return Err(Decline::new("text index representation"));
                }
                current = self
                    .call_runtime(&abi::TEXT_INDEX, &[current, index])?
                    .ok_or_else(|| Decline::new("text index result"))?;
                continue;
            }
            let next = self.projected_type(ty, projection)?;
            let next_rep = match projection {
                // A shared capture is the cell itself until it is projected.
                MirProjection::ClosureCapture {
                    environment,
                    source,
                } if self.capture_slot(*environment, *source)?.2 == MirCaptureMode::Shared => {
                    Rep::List
                }
                _ => rep_of(self.state.input.types, next)?,
            };
            let bits = self.read_projection_bits(projection, current, current_rep, ty)?;
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
            return Err(Decline::new("place projection write without projections"));
        };
        let container = MirPlace {
            base: place.base.clone(),
            projections: prefix.to_vec(),
        };
        let container_ty = self.place_type(&container)?;
        let cell_ty = self.place_type(place)?;
        let (handle, handle_rep) = if prefix.is_empty() {
            let (variable, rep) = self.place_slot(&container)?;
            (self.builder.use_var(variable), rep)
        } else {
            self.read_projected(&container)?
        };
        let value = if rep == Rep::Float {
            self.round_float_cell(value, cell_ty)
        } else {
            value
        };
        let tag = self.cell_tag_value(rep, cell_ty)?;
        let bits = self.element_bits(value, rep);
        match last {
            MirProjection::Index(last_index) | MirProjection::ProvenIndex(last_index)
                if handle_rep == Rep::Map =>
            {
                let (key_ty, _) = self.map_types(container_ty)?;
                let (key, key_rep) = self.element_operand(last_index, key_ty)?;
                let key_tag = self.cell_tag_value(key_rep, key_ty)?;
                let key = self.element_bits(key, key_rep);
                self.call_runtime(&abi::MAP_SET, &[handle, key_tag, key, tag, bits])?;
            }
            MirProjection::Index(last_index) | MirProjection::ProvenIndex(last_index) => {
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
                let (position, _) = self.field_slot(container_ty, *name)?;
                let position = self.builder.ins().iconst(
                    types::I64,
                    i64::try_from(position).map_err(|_| Decline::new("field index"))?,
                );
                let field = self.state.input.interner.resolve(*name).to_owned();
                let field = self.text_literal(field.as_bytes())?;
                self.call_runtime(&abi::RECORD_SET, &[handle, position, field, tag, bits])?;
            }
            MirProjection::SharedCell => {
                if handle_rep != Rep::List {
                    return Err(Decline::new("cell projection of a non-cell"));
                }
                let zero = self.builder.ins().iconst(types::I64, 0);
                self.call_runtime(&abi::LIST_SET, &[handle, zero, tag, bits])?;
            }
            MirProjection::VariantField { variant, field } => {
                if handle_rep != Rep::Record {
                    return Err(Decline::new("variant field of a non-enum"));
                }
                let (position, _) = self.variant_slot(*variant, *field)?;
                let position = self.builder.ins().iconst(
                    types::I64,
                    i64::try_from(position).map_err(|_| Decline::new("field index"))?,
                );
                let name = self.state.input.interner.resolve(*field).to_owned();
                let name = self.text_literal(name.as_bytes())?;
                self.call_runtime(&abi::RECORD_SET, &[handle, position, name, tag, bits])?;
            }
            _ => return Err(Decline::new(format!("place projection {last:?} write"))),
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
                    let tag = self.cell_tag_value(element_rep, element)?;
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

    /// A tuple literal: tagged items in a runtime list.
    fn construct_tuple(&mut self, destination: &MirPlace, aggregate: &MirAggregate) -> Lowered<()> {
        let Type::Tuple(elements) = resolve(self.state.input.types, aggregate.ty.semantic_id())
        else {
            return Err(Decline::new("tuple type"));
        };
        let elements = elements.clone();
        let MirAggregateFields::Ordered(items) = &aggregate.fields else {
            return Err(Decline::new("tuple fields"));
        };
        if items.len() != elements.len() {
            return Err(Decline::new("tuple arity"));
        }
        let list = self
            .call_runtime(&abi::LIST_NEW, &[])?
            .ok_or_else(|| Decline::new("list new result"))?;
        for (item, element) in items.iter().zip(elements) {
            let MirAggregateItem::Operand(operand) = item else {
                return Err(Decline::new("tuple spread"));
            };
            let element_ty = MirType::semantic(element);
            let (value, rep) = self.element_operand(operand, element_ty)?;
            let tag = self.cell_tag_value(rep, element_ty)?;
            let bits = self.element_bits(value, rep);
            self.call_runtime(&abi::LIST_PUSH, &[list, tag, bits])?;
        }
        let (variable, slot_rep) = self.place_slot(destination)?;
        if slot_rep != Rep::Tuple {
            return Err(Decline::new("tuple destination representation"));
        }
        self.builder.def_var(variable, list);
        Ok(())
    }

    /// An integer interval `start‥end` or `start‥=end`: a runtime list of the
    /// two bounds and the inclusive flag. Reversed bounds fail.
    fn construct_interval(
        &mut self,
        destination: &MirPlace,
        aggregate: &MirAggregate,
    ) -> Lowered<()> {
        let MirAggregateKind::Interval { inclusive } = aggregate.kind else {
            return Err(Decline::new("interval kind"));
        };
        let MirAggregateFields::Ordered(items) = &aggregate.fields else {
            return Err(Decline::new("interval fields"));
        };
        let [
            MirAggregateItem::Operand(start),
            MirAggregateItem::Operand(end),
        ] = items.as_slice()
        else {
            return Err(Decline::new("stepped interval"));
        };
        let (start, start_rep) = self.scalar(start)?;
        let (end, end_rep) = self.scalar(end)?;
        if start_rep != Rep::Int || end_rep != Rep::Int {
            return Err(Decline::new("interval bound representation"));
        }
        let reversed = self
            .builder
            .ins()
            .icmp(IntCC::SignedGreaterThan, start, end);
        self.trap_if(reversed, "interval bounds must be monotonic (start ≤ end)")?;
        let list = self
            .call_runtime(&abi::LIST_NEW, &[])?
            .ok_or_else(|| Decline::new("list new result"))?;
        let flag = self.builder.ins().iconst(types::I64, i64::from(inclusive));
        for (rep, bits) in [(Rep::Int, start), (Rep::Int, end), (Rep::Bool, flag)] {
            let tag = self
                .builder
                .ins()
                .iconst(types::I32, Self::element_tag(rep)?);
            self.call_runtime(&abi::LIST_PUSH, &[list, tag, bits])?;
        }
        let (variable, slot_rep) = self.place_slot(destination)?;
        if slot_rep != Rep::Tuple {
            return Err(Decline::new("interval destination representation"));
        }
        self.builder.def_var(variable, list);
        Ok(())
    }

    /// `tabula` and `copia` literals.
    fn construct_map(&mut self, destination: &MirPlace, aggregate: &MirAggregate) -> Lowered<()> {
        let is_set = aggregate.kind == MirAggregateKind::Set;
        let dynamic = matches!(
            resolve(self.state.input.types, aggregate.ty.semantic_id()),
            Type::Primitive(Primitive::Json | Primitive::Valor)
        );
        let flag = self.builder.ins().iconst(types::I32, i64::from(is_set));
        let map = self
            .call_runtime(&abi::MAP_NEW, &[flag])?
            .ok_or_else(|| Decline::new("map new result"))?;
        if is_set {
            let element = match resolve(self.state.input.types, aggregate.ty.semantic_id()) {
                Type::Set(element) => MirType::semantic(*element),
                _ => return Err(Decline::new("set type")),
            };
            let MirAggregateFields::Ordered(items) = &aggregate.fields else {
                return Err(Decline::new("set fields"));
            };
            for item in items {
                let MirAggregateItem::Operand(operand) = item else {
                    return Err(Decline::new("set spread"));
                };
                let (value, rep) = self.element_operand(operand, element)?;
                let tag = self.cell_tag_value(rep, element)?;
                let bits = self.element_bits(value, rep);
                let zero_tag = self.builder.ins().iconst(types::I32, 0);
                let zero = self.builder.ins().iconst(types::I64, 0);
                self.call_runtime(&abi::MAP_SET, &[map, tag, bits, zero_tag, zero])?;
            }
        } else {
            let (key_ty, value_ty) = if dynamic {
                // An object literal of open data: text keys, boxed values.
                (
                    MirType::semantic(self.state.input.types.primitive(Primitive::Textus)),
                    aggregate.ty,
                )
            } else {
                self.map_types(aggregate.ty)?
            };
            let MirAggregateFields::Keyed(entries) = &aggregate.fields else {
                return Err(Decline::new("map fields"));
            };
            for entry in entries {
                let (key, key_rep) = self.element_operand(&entry.key, key_ty)?;
                let (value, value_rep) = self.element_operand(&entry.value, value_ty)?;
                let key_tag = self.cell_tag_value(key_rep, key_ty)?;
                let key = self.element_bits(key, key_rep);
                let value_tag = self.cell_tag_value(value_rep, value_ty)?;
                let value = self.element_bits(value, value_rep);
                self.call_runtime(&abi::MAP_SET, &[map, key_tag, key, value_tag, value])?;
            }
        }
        let (variable, slot_rep) = self.place_slot(destination)?;
        if dynamic && slot_rep == Rep::Opt && !is_set {
            let boxed = self.wrap_some(map, Rep::Map)?;
            self.builder.def_var(variable, boxed);
            return Ok(());
        }
        if slot_rep != (if is_set { Rep::Set } else { Rep::Map }) {
            return Err(Decline::new("map destination representation"));
        }
        self.builder.def_var(variable, map);
        Ok(())
    }

    /// A `genus` construction: fields supplied by name, stored in declaration
    /// order.
    /// An anonymous record literal: fields stored in name order.
    fn construct_anonymous(
        &mut self,
        destination: &MirPlace,
        aggregate: &MirAggregate,
    ) -> Lowered<()> {
        let MirAggregateFields::Named(supplied) = &aggregate.fields else {
            return Err(Decline::new("record fields"));
        };
        let fields = self
            .anonymous_fields(aggregate.ty)
            .ok_or_else(|| Decline::new("record type"))?;
        let name = self.text_literal(b"")?;
        let variant_flag = self.builder.ins().iconst(types::I32, 0);
        let record = self
            .call_runtime(&abi::RECORD_NEW, &[name, variant_flag])?
            .ok_or_else(|| Decline::new("record new result"))?;
        for (position, (field, field_ty)) in fields.iter().enumerate() {
            let Some(item) = supplied
                .iter()
                .find(|item| self.state.input.interner.resolve(item.name) == field.as_str())
            else {
                return Err(Decline::new("record field without a value"));
            };
            let field_ty = MirType::semantic(*field_ty);
            let (value, rep) = self.element_operand(&item.value, field_ty)?;
            let position = self.builder.ins().iconst(
                types::I64,
                i64::try_from(position).map_err(|_| Decline::new("field index"))?,
            );
            let text = self.text_literal(field.as_bytes())?;
            let tag = self.cell_tag_value(rep, field_ty)?;
            let bits = self.element_bits(value, rep);
            self.call_runtime(&abi::RECORD_SET, &[record, position, text, tag, bits])?;
        }
        let (variable, slot_rep) = self.place_slot(destination)?;
        if slot_rep != Rep::Record {
            return Err(Decline::new("record destination representation"));
        }
        self.builder.def_var(variable, record);
        Ok(())
    }

    fn construct_struct(
        &mut self,
        destination: &MirPlace,
        aggregate: &MirAggregate,
    ) -> Lowered<()> {
        if aggregate.kind == MirAggregateKind::Record {
            return self.construct_anonymous(destination, aggregate);
        }
        let (def, variant) = match aggregate.kind {
            MirAggregateKind::Struct(def) => (def, false),
            MirAggregateKind::EnumVariant(def) => (def, true),
            _ => return Err(Decline::new("struct kind")),
        };
        let supplied: &[MirNamedOperand] = match &aggregate.fields {
            MirAggregateFields::Named(supplied) => supplied,
            MirAggregateFields::Ordered(items) if items.is_empty() => &[],
            _ => return Err(Decline::new("struct fields")),
        };
        let meta = self.state.input.structs;
        let order = match meta.order.get(&def) {
            Some(order) => order.clone(),
            None if variant => Vec::new(),
            None => return Err(Decline::new("unknown genus")),
        };
        let name = meta
            .names
            .get(&def)
            .cloned()
            .unwrap_or_else(|| format!("def{}", def.0));
        let name = self.text_literal(name.as_bytes())?;
        let variant_flag = self.builder.ins().iconst(types::I32, i64::from(variant));
        let record = self
            .call_runtime(&abi::RECORD_NEW, &[name, variant_flag])?
            .ok_or_else(|| Decline::new("record new result"))?;
        for (position, field) in order.iter().enumerate() {
            let field_ty = if variant {
                self.variant_slot(def, *field)?.1
            } else {
                self.field_slot(aggregate.ty, *field)?.1
            };
            let (value, rep) = match supplied.iter().find(|item| item.name == *field) {
                Some(item) => self.element_operand(&item.value, field_ty)?,
                // An omitted optional field is none.
                None if rep_of(self.state.input.types, field_ty)? == Rep::Opt => {
                    (self.builder.ins().iconst(types::I64, 0), Rep::Opt)
                }
                None => return Err(Decline::new("genus field without a value")),
            };
            let position = self.builder.ins().iconst(
                types::I64,
                i64::try_from(position).map_err(|_| Decline::new("field index"))?,
            );
            let text = self.state.input.interner.resolve(*field).to_owned();
            let text = self.text_literal(text.as_bytes())?;
            let tag = self.cell_tag_value(rep, field_ty)?;
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
        if let Some(lift) = lift::list_lift(op) {
            return self.list_lift(destination, lift, args);
        }
        let (handle, rep) = self.scalar(receiver)?;
        self.collection_scalar(destination, op, args, receiver, handle, rep)
    }

    fn collection_scalar(
        &mut self,
        destination: Option<&MirPlace>,
        op: MirCollectionOp,
        args: &[MirOperand],
        receiver: &MirOperand,
        handle: Value,
        rep: Rep,
    ) -> Lowered<()> {
        match (op, rep) {
            (
                MirCollectionOp::Floor
                | MirCollectionOp::Ceiling
                | MirCollectionOp::Round
                | MirCollectionOp::Truncate,
                Rep::Float,
            ) => self.collection_floor_scalar(destination, op, handle),
            (MirCollectionOp::Absolute, Rep::Int) => {
                self.collection_absolute_int(destination, handle)
            }
            (MirCollectionOp::Absolute, Rep::Float) => {
                self.collection_absolute_float(destination, handle)
            }
            (MirCollectionOp::Signum, Rep::Int) => self.collection_signum_int(destination, handle),
            (MirCollectionOp::Minimum | MirCollectionOp::Maximum, Rep::Int | Rep::Float) => {
                self.collection_minimum_scalar(destination, op, args, handle, rep)
            }
            (MirCollectionOp::Compare, Rep::Int | Rep::Float | Rep::Text) => {
                self.collection_compare_scalar(destination, args, handle, rep)
            }
            (MirCollectionOp::Signum, Rep::Float) => {
                self.collection_signum_float(destination, handle)
            }
            (MirCollectionOp::ScalarMath(function), Rep::Float | Rep::Int) => {
                self.collection_scalar_math(destination, handle, rep, function)
            }
            (MirCollectionOp::Approximate, Rep::Float | Rep::Int) => {
                self.collection_approximate_scalar(destination, args)
            }
            _ => self.collection_pattern(destination, op, args, receiver, handle, rep),
        }
    }

    fn collection_pattern(
        &mut self,
        destination: Option<&MirPlace>,
        op: MirCollectionOp,
        args: &[MirOperand],
        receiver: &MirOperand,
        handle: Value,
        rep: Rep,
    ) -> Lowered<()> {
        match (op, rep) {
            (
                MirCollectionOp::RegexMatches
                | MirCollectionOp::RegexFind
                | MirCollectionOp::RegexFindAll
                | MirCollectionOp::RegexSplit,
                Rep::Regex,
            ) => self.collection_query_regex(destination, op, args, handle),
            (MirCollectionOp::RegexReplace | MirCollectionOp::RegexReplaceEach, Rep::Regex) => {
                self.collection_replace_regex(destination, op, args, handle)
            }
            (
                MirCollectionOp::MatchText
                | MirCollectionOp::MatchStart
                | MirCollectionOp::MatchEnd,
                Rep::Match,
            ) => self.collection_query_match(destination, op, handle),
            (MirCollectionOp::MatchGroup | MirCollectionOp::MatchNamed, Rep::Match) => {
                self.collection_select_match(destination, op, args, handle)
            }
            _ => self.collection_interval(destination, op, args, receiver, handle, rep),
        }
    }

    fn collection_interval(
        &mut self,
        destination: Option<&MirPlace>,
        op: MirCollectionOp,
        args: &[MirOperand],
        receiver: &MirOperand,
        handle: Value,
        rep: Rep,
    ) -> Lowered<()> {
        match (op, rep) {
            (MirCollectionOp::IntervalClamp, Rep::Int) => {
                self.collection_clamp_int(destination, args, handle)
            }
            (MirCollectionOp::IntervalClamp, Rep::Tuple) => {
                self.collection_interval_clamp(destination, args, handle)
            }
            (MirCollectionOp::IntervalLength | MirCollectionOp::IntervalContains, Rep::Tuple) => {
                self.collection_query_interval(destination, op, args, handle)
            }
            (MirCollectionOp::IntervalIntersect | MirCollectionOp::IntervalUnion, Rep::Tuple) => {
                self.collection_combine_intervals(destination, op, args, handle)
            }
            (MirCollectionOp::IntervalMaterialize, Rep::Tuple) => {
                self.collection_materialize_interval(destination, handle)
            }
            _ => self.collection_list(destination, op, args, receiver, handle, rep),
        }
    }

    fn collection_list(
        &mut self,
        destination: Option<&MirPlace>,
        op: MirCollectionOp,
        args: &[MirOperand],
        receiver: &MirOperand,
        handle: Value,
        rep: Rep,
    ) -> Lowered<()> {
        match (op, rep) {
            (MirCollectionOp::Length, Rep::List) => {
                self.collection_length_list(destination, handle)
            }
            (MirCollectionOp::Minimum | MirCollectionOp::Maximum, Rep::List) => {
                self.collection_minimum_list(destination, op, args, receiver, handle)
            }
            (MirCollectionOp::Index, Rep::List) => {
                self.collection_index_list(destination, args, handle)
            }
            (MirCollectionOp::Append, Rep::List) => {
                self.collection_append_list(args, receiver, handle)
            }
            (MirCollectionOp::Contains, Rep::List) => {
                self.collection_contains_list(destination, args, receiver, handle)
            }
            (MirCollectionOp::Sum, Rep::List) => {
                self.collection_sum_list(destination, receiver, handle)
            }
            (MirCollectionOp::SortByKeys, Rep::List) => {
                self.collection_sort_list_by_keys(destination, args, handle)
            }
            (MirCollectionOp::AppendImmutable, Rep::List) => {
                self.collection_append_list_copy(destination, args, receiver, handle)
            }
            (
                MirCollectionOp::Take | MirCollectionOp::TakeLast | MirCollectionOp::DropFirst,
                Rep::List,
            ) => self.collection_take_list(destination, op, args, handle),
            (MirCollectionOp::First | MirCollectionOp::Last, Rep::List) => {
                self.collection_first_list(destination, op, handle)
            }
            (MirCollectionOp::RemoveFirst | MirCollectionOp::RemoveLast, Rep::List) => {
                self.collection_remove_list_end(destination, op, handle)
            }
            (MirCollectionOp::ReverseInPlace, Rep::List) => {
                self.collection_reverse_list_in_place(handle)
            }
            (MirCollectionOp::Compare, Rep::Tuple | Rep::List) => {
                self.collection_compare_list(destination, args, handle, rep)
            }
            (MirCollectionOp::ArgMax | MirCollectionOp::ArgMin, Rep::List) => {
                self.collection_arg_extremum_list(destination, op, handle)
            }
            (MirCollectionOp::Reverse, Rep::List) => {
                self.collection_reverse_list(destination, handle)
            }
            (MirCollectionOp::Sort, Rep::List) => self.collection_sort_list(destination, handle),
            _ => self.collection_map(destination, op, args, receiver, handle, rep),
        }
    }

    fn collection_map(
        &mut self,
        destination: Option<&MirPlace>,
        op: MirCollectionOp,
        args: &[MirOperand],
        receiver: &MirOperand,
        handle: Value,
        rep: Rep,
    ) -> Lowered<()> {
        match (op, rep) {
            (MirCollectionOp::Length, Rep::Map | Rep::Set) => {
                self.collection_length_map(destination, handle)
            }
            (MirCollectionOp::IsEmpty, Rep::Map | Rep::Set) => {
                self.collection_is_empty_map(destination, handle)
            }
            (MirCollectionOp::Index, Rep::Map) => {
                self.collection_index_map(destination, args, receiver, handle)
            }
            (MirCollectionOp::Contains | MirCollectionOp::Delete, Rep::Map | Rep::Set) => {
                self.collection_contains_map(destination, op, args, receiver, handle)
            }
            (MirCollectionOp::Put, Rep::Map) => self.collection_put_map(args, receiver, handle),
            (MirCollectionOp::Append, Rep::Set) => {
                self.collection_append_set(args, receiver, handle)
            }
            (MirCollectionOp::Keys, Rep::Map) => self.collection_keys_map(destination, handle),
            (MirCollectionOp::Values, Rep::Map) => self.collection_values_map(destination, handle),
            (
                MirCollectionOp::Union
                | MirCollectionOp::Intersection
                | MirCollectionOp::Difference
                | MirCollectionOp::SymmetricDifference,
                Rep::Set,
            ) => self.collection_set_algebra(destination, op, args, handle),
            (MirCollectionOp::IsSubset | MirCollectionOp::IsSuperset, Rep::Set) => {
                self.collection_set_relation(destination, op, args, handle)
            }
            _ => self.collection_text(destination, op, args, handle, rep),
        }
    }

    fn collection_text(
        &mut self,
        destination: Option<&MirPlace>,
        op: MirCollectionOp,
        args: &[MirOperand],
        handle: Value,
        rep: Rep,
    ) -> Lowered<()> {
        match (op, rep) {
            (MirCollectionOp::Length, Rep::Text) => {
                self.collection_length_text(destination, handle)
            }
            (MirCollectionOp::TextEscape, Rep::Text) => {
                self.collection_escape_text(destination, handle)
            }
            (MirCollectionOp::Index, Rep::Text) => {
                self.collection_index_text(destination, args, handle)
            }
            (
                MirCollectionOp::Uppercase | MirCollectionOp::Lowercase | MirCollectionOp::Trim,
                Rep::Text,
            ) => self.collection_map_text(destination, op, handle),
            (
                MirCollectionOp::StartsWith | MirCollectionOp::EndsWith | MirCollectionOp::Contains,
                Rep::Text,
            ) => self.collection_test_text(destination, op, args, handle),
            (MirCollectionOp::Split, Rep::Text) => {
                self.collection_split_text(destination, args, handle)
            }
            (MirCollectionOp::Replace, Rep::Text) => {
                self.collection_replace_text(destination, args, handle)
            }
            _ => self.collection_shared(destination, op, args, handle, rep),
        }
    }

    fn collection_shared(
        &mut self,
        destination: Option<&MirPlace>,
        op: MirCollectionOp,
        args: &[MirOperand],
        handle: Value,
        rep: Rep,
    ) -> Lowered<()> {
        match (op, rep) {
            (MirCollectionOp::IsEmpty, Rep::Text | Rep::List) => {
                self.collection_is_empty_sequence(destination, handle, rep)
            }
            (MirCollectionOp::Slice, Rep::List | Rep::Text) => {
                self.collection_slice_shared(destination, args, handle, rep)
            }
            _ => Err(Decline::new(format!(
                "collection operation {op:?} on {rep:?}"
            ))),
        }
    }

    fn finish_collection(
        &mut self,
        destination: Option<&MirPlace>,
        result: (Option<Value>, Rep),
    ) -> Lowered<()> {
        match (destination, result.0) {
            (Some(destination), value @ Some(_)) => {
                self.define_result(destination, value, result.1)
            }
            _ => Ok(()),
        }
    }

    fn collection_length_text(
        &mut self,
        destination: Option<&MirPlace>,
        handle: Value,
    ) -> Lowered<()> {
        let result = { (self.call_runtime(&abi::TEXT_LEN, &[handle])?, Rep::Int) };

        self.finish_collection(destination, result)
    }

    fn collection_length_list(
        &mut self,
        destination: Option<&MirPlace>,
        handle: Value,
    ) -> Lowered<()> {
        let result = { (self.call_runtime(&abi::LIST_LEN, &[handle])?, Rep::Int) };

        self.finish_collection(destination, result)
    }

    fn collection_is_empty_sequence(
        &mut self,
        destination: Option<&MirPlace>,
        handle: Value,
        rep: Rep,
    ) -> Lowered<()> {
        let result = {
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
        };

        self.finish_collection(destination, result)
    }

    fn collection_floor_scalar(
        &mut self,
        destination: Option<&MirPlace>,
        op: MirCollectionOp,
        handle: Value,
    ) -> Lowered<()> {
        let result = {
            let rounded = match op {
                MirCollectionOp::Floor => self.builder.ins().floor(handle),
                MirCollectionOp::Ceiling => self.builder.ins().ceil(handle),
                MirCollectionOp::Round => self.builder.ins().nearest(handle),
                _ => self.builder.ins().trunc(handle),
            };
            (
                Some(self.builder.ins().fcvt_to_sint(types::I64, rounded)),
                Rep::Int,
            )
        };

        self.finish_collection(destination, result)
    }

    fn collection_absolute_int(
        &mut self,
        destination: Option<&MirPlace>,
        handle: Value,
    ) -> Lowered<()> {
        let result = {
            let zero = self.builder.ins().iconst(types::I64, 0);
            let negative = self.builder.ins().icmp(IntCC::SignedLessThan, handle, zero);
            let flipped = self.builder.ins().ineg(handle);
            (
                Some(self.builder.ins().select(negative, flipped, handle)),
                Rep::Int,
            )
        };

        self.finish_collection(destination, result)
    }

    fn collection_absolute_float(
        &mut self,
        destination: Option<&MirPlace>,
        handle: Value,
    ) -> Lowered<()> {
        let result = { (Some(self.builder.ins().fabs(handle)), Rep::Float) };

        self.finish_collection(destination, result)
    }

    fn collection_signum_int(
        &mut self,
        destination: Option<&MirPlace>,
        handle: Value,
    ) -> Lowered<()> {
        let result = {
            let zero = self.builder.ins().iconst(types::I64, 0);
            let positive = self
                .builder
                .ins()
                .icmp(IntCC::SignedGreaterThan, handle, zero);
            let negative = self.builder.ins().icmp(IntCC::SignedLessThan, handle, zero);
            let positive = self.builder.ins().uextend(types::I64, positive);
            let negative = self.builder.ins().uextend(types::I64, negative);
            (Some(self.builder.ins().isub(positive, negative)), Rep::Int)
        };

        self.finish_collection(destination, result)
    }

    fn collection_minimum_scalar(
        &mut self,
        destination: Option<&MirPlace>,
        op: MirCollectionOp,
        args: &[MirOperand],
        handle: Value,
        rep: Rep,
    ) -> Lowered<()> {
        let result = {
            let [_, other] = args else {
                return Err(Decline::new("minimum arity"));
            };
            let (other, other_rep) = self.scalar(other)?;
            if other_rep != rep {
                return Err(Decline::new("minimum operand representation"));
            }
            let smallest = op == MirCollectionOp::Minimum;
            let picked = match (rep, smallest) {
                (Rep::Int, true) => self.builder.ins().smin(handle, other),
                (Rep::Int, false) => self.builder.ins().smax(handle, other),
                (_, true) => self.builder.ins().fmin(handle, other),
                (_, false) => self.builder.ins().fmax(handle, other),
            };
            (Some(picked), rep)
        };

        self.finish_collection(destination, result)
    }

    fn collection_minimum_list(
        &mut self,
        destination: Option<&MirPlace>,
        op: MirCollectionOp,
        args: &[MirOperand],
        receiver: &MirOperand,
        handle: Value,
    ) -> Lowered<()> {
        let result = {
            let [_, bound] = args else {
                return Err(Decline::new("list extremum arity"));
            };
            let element = self.element_type(self.operand_type(receiver)?)?;
            let element_rep = rep_of(self.state.input.types, element)?;
            let (bound, bound_rep) = self.scalar(bound)?;
            if bound_rep != element_rep {
                return Err(Decline::new("list extremum bound representation"));
            }
            let mode = match (element_rep, op == MirCollectionOp::Minimum) {
                (Rep::Int, true) => 0,
                (Rep::Int, false) => 1,
                (Rep::Float, true) => 2,
                (Rep::Float, false) => 3,
                _ => return Err(Decline::new("list extremum over a non-number list")),
            };
            let bound = self.element_bits(bound, element_rep);
            let mode = self.builder.ins().iconst(types::I32, mode);
            (
                self.call_runtime(&abi::LIST_EXTREMUM, &[handle, bound, mode])?,
                Rep::List,
            )
        };

        self.finish_collection(destination, result)
    }

    fn collection_escape_text(
        &mut self,
        destination: Option<&MirPlace>,
        handle: Value,
    ) -> Lowered<()> {
        let result = { (self.call_runtime(&abi::TEXT_ESCAPE, &[handle])?, Rep::Text) };

        self.finish_collection(destination, result)
    }

    fn collection_query_regex(
        &mut self,
        destination: Option<&MirPlace>,
        op: MirCollectionOp,
        args: &[MirOperand],
        handle: Value,
    ) -> Lowered<()> {
        let result = {
            let [_, text] = args else {
                return Err(Decline::new("regex operation arity"));
            };
            let (text, text_rep) = self.scalar(text)?;
            if !matches!(text_rep, Rep::Text | Rep::Char) {
                return Err(Decline::new("regex operation on a non-text"));
            }
            let (symbol, result) = match op {
                MirCollectionOp::RegexMatches => (&abi::REGEX_MATCHES, Rep::Bool),
                MirCollectionOp::RegexFind => (&abi::REGEX_FIND, Rep::Opt),
                MirCollectionOp::RegexFindAll => (&abi::REGEX_FIND_ALL, Rep::List),
                _ => (&abi::REGEX_SPLIT, Rep::List),
            };
            (self.call_runtime(symbol, &[handle, text])?, result)
        };

        self.finish_collection(destination, result)
    }

    fn collection_replace_regex(
        &mut self,
        destination: Option<&MirPlace>,
        op: MirCollectionOp,
        args: &[MirOperand],
        handle: Value,
    ) -> Lowered<()> {
        let result = {
            let [_, text, replacement] = args else {
                return Err(Decline::new("regex replace arity"));
            };
            let (text, text_rep) = self.scalar(text)?;
            let (replacement, replacement_rep) = self.scalar(replacement)?;
            let wanted = if op == MirCollectionOp::RegexReplace {
                Rep::Text
            } else {
                Rep::List
            };
            if !matches!(text_rep, Rep::Text | Rep::Char)
                || (replacement_rep != wanted
                    && !(wanted == Rep::Text && replacement_rep == Rep::Char))
            {
                return Err(Decline::new("regex replace operands"));
            }
            let symbol = if op == MirCollectionOp::RegexReplace {
                &abi::REGEX_REPLACE
            } else {
                &abi::REGEX_REPLACE_EACH
            };
            (
                self.call_runtime(symbol, &[handle, text, replacement])?,
                Rep::Text,
            )
        };

        self.finish_collection(destination, result)
    }

    fn collection_query_match(
        &mut self,
        destination: Option<&MirPlace>,
        op: MirCollectionOp,
        handle: Value,
    ) -> Lowered<()> {
        let result = {
            let (symbol, result) = match op {
                MirCollectionOp::MatchText => (&abi::MATCH_TEXT, Rep::Text),
                MirCollectionOp::MatchStart => (&abi::MATCH_START, Rep::Int),
                _ => (&abi::MATCH_END, Rep::Int),
            };
            (self.call_runtime(symbol, &[handle])?, result)
        };

        self.finish_collection(destination, result)
    }

    fn collection_select_match(
        &mut self,
        destination: Option<&MirPlace>,
        op: MirCollectionOp,
        args: &[MirOperand],
        handle: Value,
    ) -> Lowered<()> {
        let result = {
            let [_, selector] = args else {
                return Err(Decline::new("match group arity"));
            };
            let (selector, selector_rep) = self.scalar(selector)?;
            let symbol = match (op, selector_rep) {
                (MirCollectionOp::MatchGroup, Rep::Int) => &abi::MATCH_GROUP,
                (MirCollectionOp::MatchNamed, Rep::Text | Rep::Char) => &abi::MATCH_NAMED,
                _ => return Err(Decline::new("match group selector")),
            };
            (self.call_runtime(symbol, &[handle, selector])?, Rep::Opt)
        };

        self.finish_collection(destination, result)
    }

    fn collection_index_text(
        &mut self,
        destination: Option<&MirPlace>,
        args: &[MirOperand],
        handle: Value,
    ) -> Lowered<()> {
        let result = {
            let [_, index] = args else {
                return Err(Decline::new("index arity"));
            };
            let (index, index_rep) = self.scalar(index)?;
            if index_rep != Rep::Int {
                return Err(Decline::new("text index representation"));
            }
            let optional = match destination {
                Some(destination) => {
                    let ty = self.place_type(destination)?;
                    rep_of(self.state.input.types, ty)? == Rep::Opt
                }
                None => false,
            };
            if optional {
                let found = self
                    .call_runtime(&abi::TEXT_INDEX_OPT, &[handle, index])?
                    .ok_or_else(|| Decline::new("text index result"))?;
                (Some(found), Rep::Opt)
            } else {
                let character = self
                    .call_runtime(&abi::TEXT_INDEX, &[handle, index])?
                    .ok_or_else(|| Decline::new("text index result"))?;
                (Some(character), Rep::Text)
            }
        };

        self.finish_collection(destination, result)
    }

    fn collection_clamp_int(
        &mut self,
        destination: Option<&MirPlace>,
        args: &[MirOperand],
        handle: Value,
    ) -> Lowered<()> {
        let result = {
            let [_, target] = args else {
                return Err(Decline::new("interval clamp arity"));
            };
            let (target, target_rep) = self.scalar(target)?;
            if target_rep != Rep::Tuple {
                return Err(Decline::new("interval clamp representation"));
            }
            let mode = self.builder.ins().iconst(types::I32, 2);
            (
                self.call_runtime(&abi::INTERVAL_QUERY, &[mode, target, handle])?,
                Rep::Int,
            )
        };

        self.finish_collection(destination, result)
    }

    fn collection_interval_clamp(
        &mut self,
        destination: Option<&MirPlace>,
        args: &[MirOperand],
        handle: Value,
    ) -> Lowered<()> {
        let result = {
            let [_, other] = args else {
                return Err(Decline::new("interval clamp arity"));
            };
            let (other, other_rep) = self.scalar(other)?;
            if other_rep != Rep::Tuple {
                return Err(Decline::new("interval clamp representation"));
            }
            let mode = self.builder.ins().iconst(types::I32, 0);
            let boxed = self
                .call_runtime(&abi::INTERVAL_COMBINE, &[mode, handle, other])?
                .ok_or_else(|| Decline::new("interval clamp result"))?;
            let zero = self.builder.ins().iconst(types::I64, 0);
            let empty = self.builder.ins().icmp(IntCC::Equal, boxed, zero);
            self.trap_if(empty, "interval clamp produced an empty interval")?;
            (self.call_runtime(&abi::OPT_BITS, &[boxed])?, Rep::Tuple)
        };

        self.finish_collection(destination, result)
    }

    fn collection_query_interval(
        &mut self,
        destination: Option<&MirPlace>,
        op: MirCollectionOp,
        args: &[MirOperand],
        handle: Value,
    ) -> Lowered<()> {
        let result = {
            let (mode, value) = if op == MirCollectionOp::IntervalLength {
                (0, self.builder.ins().iconst(types::I64, 0))
            } else {
                let [_, value] = args else {
                    return Err(Decline::new("interval query arity"));
                };
                let (value, value_rep) = self.scalar(value)?;
                if value_rep != Rep::Int {
                    return Err(Decline::new("interval query representation"));
                }
                (
                    if op == MirCollectionOp::IntervalContains {
                        1
                    } else {
                        2
                    },
                    value,
                )
            };
            let mode = self.builder.ins().iconst(types::I32, mode);
            let answer = self
                .call_runtime(&abi::INTERVAL_QUERY, &[mode, handle, value])?
                .ok_or_else(|| Decline::new("interval query result"))?;
            if op == MirCollectionOp::IntervalContains {
                let zero = self.builder.ins().iconst(types::I64, 0);
                (
                    Some(self.builder.ins().icmp(IntCC::NotEqual, answer, zero)),
                    Rep::Bool,
                )
            } else {
                (Some(answer), Rep::Int)
            }
        };

        self.finish_collection(destination, result)
    }

    fn collection_combine_intervals(
        &mut self,
        destination: Option<&MirPlace>,
        op: MirCollectionOp,
        args: &[MirOperand],
        handle: Value,
    ) -> Lowered<()> {
        let result = {
            let [_, other] = args else {
                return Err(Decline::new("interval combine arity"));
            };
            let (other, other_rep) = self.scalar(other)?;
            if other_rep != Rep::Tuple {
                return Err(Decline::new("interval combine representation"));
            }
            let mode = self
                .builder
                .ins()
                .iconst(types::I32, i64::from(op == MirCollectionOp::IntervalUnion));
            (
                self.call_runtime(&abi::INTERVAL_COMBINE, &[mode, handle, other])?,
                Rep::Opt,
            )
        };

        self.finish_collection(destination, result)
    }

    fn collection_materialize_interval(
        &mut self,
        destination: Option<&MirPlace>,
        handle: Value,
    ) -> Lowered<()> {
        let result = {
            if let Some(destination) = destination {
                let ty = self.place_type(destination)?;
                if rep_of(self.state.input.types, ty)? != Rep::List {
                    return Err(Decline::new("interval materialized as a non-list"));
                }
            }
            (
                self.call_runtime(&abi::INTERVAL_MATERIALIZE, &[handle])?,
                Rep::List,
            )
        };

        self.finish_collection(destination, result)
    }

    fn collection_index_list(
        &mut self,
        destination: Option<&MirPlace>,
        args: &[MirOperand],
        handle: Value,
    ) -> Lowered<()> {
        let result = {
            let [_, index] = args else {
                return Err(Decline::new("index arity"));
            };
            let (index, index_rep) = self.scalar(index)?;
            if index_rep == Rep::Tuple {
                let window = self
                    .call_runtime(&abi::LIST_RANGE, &[handle, index])?
                    .ok_or_else(|| Decline::new("list window result"))?;
                // The runtime ends the run itself on a window outside the list.
                let Some(destination) = destination else {
                    return Ok(());
                };
                return self.define_result(destination, Some(window), Rep::List);
            }
            if index_rep != Rep::Int {
                return Err(Decline::new("list index representation"));
            }
            let Some(destination) = destination else {
                return Err(Decline::new("index without destination"));
            };
            let element = self.place_type(destination)?;
            let element_rep = rep_of(self.state.input.types, element)?;
            let list_element = self.element_type(self.operand_type(&args[0])?)?;
            if element_rep == Rep::Opt && rep_of(self.state.input.types, list_element)? != Rep::Opt
            {
                let found = self
                    .call_runtime(&abi::LIST_GET_OPT, &[handle, index])?
                    .ok_or_else(|| Decline::new("list get result"))?;
                (Some(found), Rep::Opt)
            } else {
                let bits = self
                    .call_runtime(&abi::LIST_GET, &[handle, index])?
                    .ok_or_else(|| Decline::new("list get result"))?;
                (Some(self.element_value(bits, element_rep)), element_rep)
            }
        };

        self.finish_collection(destination, result)
    }

    fn collection_append_list(
        &mut self,
        args: &[MirOperand],
        receiver: &MirOperand,
        handle: Value,
    ) -> Lowered<()> {
        let [_, item] = args else {
            return Err(Decline::new("append arity"));
        };
        if let Type::BoundedArray { capacity, .. } = resolve(
            self.state.input.types,
            self.operand_type(receiver)?.semantic_id(),
        ) {
            let radix_types::IndexExpr::Literal(bound) =
                self.state.input.types.get_index(*capacity)
            else {
                return Err(Decline::new("append to a list of computed capacity"));
            };
            let bound = i64::try_from(*bound).map_err(|_| Decline::new("bounded list capacity"))?;
            let live = self
                .call_runtime(&abi::LIST_LEN, &[handle])?
                .ok_or_else(|| Decline::new("length result"))?;
            let limit = self.builder.ins().iconst(types::I64, bound);
            let full = self
                .builder
                .ins()
                .icmp(IntCC::SignedGreaterThanOrEqual, live, limit);
            self.trap_if(full, "bounded collection push at capacity")?;
        }
        let element = self.element_type(self.operand_type(receiver)?)?;
        let (value, item_rep) = self.element_operand(item, element)?;
        let tag = self.cell_tag_value(item_rep, element)?;
        let bits = self.element_bits(value, item_rep);
        self.call_runtime(&abi::LIST_PUSH, &[handle, tag, bits])?;
        Ok(())
    }

    fn collection_contains_list(
        &mut self,
        destination: Option<&MirPlace>,
        args: &[MirOperand],
        receiver: &MirOperand,
        handle: Value,
    ) -> Lowered<()> {
        let result = {
            let [_, item] = args else {
                return Err(Decline::new("contains arity"));
            };
            let element = self.element_type(self.operand_type(receiver)?)?;
            let (value, item_rep) = self.element_operand(item, element)?;
            let tag = self.cell_tag_value(item_rep, element)?;
            let bits = self.element_bits(value, item_rep);
            (
                self.call_runtime(&abi::LIST_CONTAINS, &[handle, tag, bits])?,
                Rep::Bool,
            )
        };

        self.finish_collection(destination, result)
    }

    fn collection_length_map(
        &mut self,
        destination: Option<&MirPlace>,
        handle: Value,
    ) -> Lowered<()> {
        let result = { (self.call_runtime(&abi::MAP_LEN, &[handle])?, Rep::Int) };

        self.finish_collection(destination, result)
    }

    fn collection_is_empty_map(
        &mut self,
        destination: Option<&MirPlace>,
        handle: Value,
    ) -> Lowered<()> {
        let result = {
            let length = self
                .call_runtime(&abi::MAP_LEN, &[handle])?
                .ok_or_else(|| Decline::new("length result"))?;
            let zero = self.builder.ins().iconst(types::I64, 0);
            (
                Some(self.builder.ins().icmp(IntCC::Equal, length, zero)),
                Rep::Bool,
            )
        };

        self.finish_collection(destination, result)
    }

    fn collection_index_map(
        &mut self,
        destination: Option<&MirPlace>,
        args: &[MirOperand],
        receiver: &MirOperand,
        handle: Value,
    ) -> Lowered<()> {
        let result = {
            let [_, key] = args else {
                return Err(Decline::new("index arity"));
            };
            let Some(destination) = destination else {
                return Err(Decline::new("index without destination"));
            };
            let (key_ty, _) = self.map_types(self.operand_type(receiver)?)?;
            let (key, key_rep) = self.element_operand(key, key_ty)?;
            let key_tag = self.cell_tag_value(key_rep, key_ty)?;
            let key = self.element_bits(key, key_rep);
            let result_rep = rep_of(self.state.input.types, self.place_type(destination)?)?;
            if result_rep == Rep::Opt {
                (
                    self.call_runtime(&abi::MAP_GET_OPT, &[handle, key_tag, key])?,
                    Rep::Opt,
                )
            } else {
                let bits = self
                    .call_runtime(&abi::MAP_GET, &[handle, key_tag, key])?
                    .ok_or_else(|| Decline::new("map get result"))?;
                (Some(self.element_value(bits, result_rep)), result_rep)
            }
        };

        self.finish_collection(destination, result)
    }

    fn collection_contains_map(
        &mut self,
        destination: Option<&MirPlace>,
        op: MirCollectionOp,
        args: &[MirOperand],
        receiver: &MirOperand,
        handle: Value,
    ) -> Lowered<()> {
        let result = {
            let [_, key] = args else {
                return Err(Decline::new("key arity"));
            };
            let key_ty = match resolve(
                self.state.input.types,
                self.operand_type(receiver)?.semantic_id(),
            ) {
                Type::Map(key, _) | Type::Set(key) => MirType::semantic(*key),
                _ => return Err(Decline::new("key access of a non-map")),
            };
            let (key, key_rep) = self.element_operand(key, key_ty)?;
            let key_tag = self.cell_tag_value(key_rep, key_ty)?;
            let key = self.element_bits(key, key_rep);
            let symbol = if op == MirCollectionOp::Delete {
                &abi::MAP_REMOVE
            } else {
                &abi::MAP_HAS
            };
            (
                self.call_runtime(symbol, &[handle, key_tag, key])?,
                Rep::Bool,
            )
        };

        self.finish_collection(destination, result)
    }

    fn collection_put_map(
        &mut self,
        args: &[MirOperand],
        receiver: &MirOperand,
        handle: Value,
    ) -> Lowered<()> {
        let [_, key, value] = args else {
            return Err(Decline::new("put arity"));
        };
        let (key_ty, value_ty) = self.map_types(self.operand_type(receiver)?)?;
        let (key, key_rep) = self.element_operand(key, key_ty)?;
        let (value, value_rep) = self.element_operand(value, value_ty)?;
        let key_tag = self.cell_tag_value(key_rep, key_ty)?;
        let key = self.element_bits(key, key_rep);
        let value_tag = self.cell_tag_value(value_rep, value_ty)?;
        let value = self.element_bits(value, value_rep);
        self.call_runtime(&abi::MAP_SET, &[handle, key_tag, key, value_tag, value])?;
        Ok(())
    }

    fn collection_append_set(
        &mut self,
        args: &[MirOperand],
        receiver: &MirOperand,
        handle: Value,
    ) -> Lowered<()> {
        let [_, item] = args else {
            return Err(Decline::new("append arity"));
        };
        let element = match resolve(
            self.state.input.types,
            self.operand_type(receiver)?.semantic_id(),
        ) {
            Type::Set(element) => MirType::semantic(*element),
            _ => return Err(Decline::new("set type")),
        };
        let (value, rep) = self.element_operand(item, element)?;
        let tag = self.cell_tag_value(rep, element)?;
        let bits = self.element_bits(value, rep);
        let zero_tag = self.builder.ins().iconst(types::I32, 0);
        let zero = self.builder.ins().iconst(types::I64, 0);
        self.call_runtime(&abi::MAP_SET, &[handle, tag, bits, zero_tag, zero])?;
        Ok(())
    }

    fn collection_keys_map(
        &mut self,
        destination: Option<&MirPlace>,
        handle: Value,
    ) -> Lowered<()> {
        let result = { (self.call_runtime(&abi::MAP_KEYS, &[handle])?, Rep::List) };

        self.finish_collection(destination, result)
    }

    fn collection_values_map(
        &mut self,
        destination: Option<&MirPlace>,
        handle: Value,
    ) -> Lowered<()> {
        let result = { (self.call_runtime(&abi::MAP_VALUES, &[handle])?, Rep::List) };

        self.finish_collection(destination, result)
    }

    fn collection_map_text(
        &mut self,
        destination: Option<&MirPlace>,
        op: MirCollectionOp,
        handle: Value,
    ) -> Lowered<()> {
        let result = {
            let mode = match op {
                MirCollectionOp::Uppercase => 0,
                MirCollectionOp::Lowercase => 1,
                _ => 2,
            };
            let mode = self.builder.ins().iconst(types::I32, mode);
            (
                self.call_runtime(&abi::TEXT_MAP, &[handle, mode])?,
                Rep::Text,
            )
        };

        self.finish_collection(destination, result)
    }

    fn collection_test_text(
        &mut self,
        destination: Option<&MirPlace>,
        op: MirCollectionOp,
        args: &[MirOperand],
        handle: Value,
    ) -> Lowered<()> {
        let result = {
            let [_, other] = args else {
                return Err(Decline::new("text test arity"));
            };
            let (other, other_rep) = self.scalar(other)?;
            if other_rep != Rep::Text {
                return Err(Decline::new("text test operand"));
            }
            let mode = match op {
                MirCollectionOp::StartsWith => 0,
                MirCollectionOp::EndsWith => 1,
                _ => 2,
            };
            let mode = self.builder.ins().iconst(types::I32, mode);
            (
                self.call_runtime(&abi::TEXT_TEST, &[handle, other, mode])?,
                Rep::Bool,
            )
        };

        self.finish_collection(destination, result)
    }

    fn collection_split_text(
        &mut self,
        destination: Option<&MirPlace>,
        args: &[MirOperand],
        handle: Value,
    ) -> Lowered<()> {
        let result = {
            let [_, delimiter] = args else {
                return Err(Decline::new("split arity"));
            };
            let (delimiter, delimiter_rep) = self.scalar(delimiter)?;
            if delimiter_rep != Rep::Text {
                return Err(Decline::new("split delimiter"));
            }
            (
                self.call_runtime(&abi::TEXT_SPLIT, &[handle, delimiter])?,
                Rep::List,
            )
        };

        self.finish_collection(destination, result)
    }

    fn collection_replace_text(
        &mut self,
        destination: Option<&MirPlace>,
        args: &[MirOperand],
        handle: Value,
    ) -> Lowered<()> {
        let result = {
            let [_, from, to] = args else {
                return Err(Decline::new("replace arity"));
            };
            let (from, from_rep) = self.scalar(from)?;
            let (to, to_rep) = self.scalar(to)?;
            if from_rep != Rep::Text || to_rep != Rep::Text {
                return Err(Decline::new("replace operands"));
            }
            (
                self.call_runtime(&abi::TEXT_REPLACE, &[handle, from, to])?,
                Rep::Text,
            )
        };

        self.finish_collection(destination, result)
    }

    fn collection_sum_list(
        &mut self,
        destination: Option<&MirPlace>,
        receiver: &MirOperand,
        handle: Value,
    ) -> Lowered<()> {
        let result = {
            let element = self.element_type(self.operand_type(receiver)?)?;
            match rep_of(self.state.input.types, element)? {
                Rep::Int => (self.call_runtime(&abi::LIST_SUM_I64, &[handle])?, Rep::Int),
                Rep::Float => (
                    self.call_runtime(&abi::LIST_SUM_F64, &[handle])?,
                    Rep::Float,
                ),
                _ => return Err(Decline::new("sum of a non-number list")),
            }
        };

        self.finish_collection(destination, result)
    }

    fn collection_compare_scalar(
        &mut self,
        destination: Option<&MirPlace>,
        args: &[MirOperand],
        handle: Value,
        rep: Rep,
    ) -> Lowered<()> {
        let result = {
            let [_, other] = args else {
                return Err(Decline::new("compare arity"));
            };
            let (other, other_rep) = self.scalar(other)?;
            if other_rep != rep {
                return Err(Decline::new("compare operand representation"));
            }
            let ordering = match rep {
                Rep::Text => self
                    .call_runtime(&abi::TEXT_CMP, &[handle, other])?
                    .ok_or_else(|| Decline::new("compare result"))?,
                Rep::Int => {
                    let less = self
                        .builder
                        .ins()
                        .icmp(IntCC::SignedLessThan, handle, other);
                    let more = self
                        .builder
                        .ins()
                        .icmp(IntCC::SignedGreaterThan, handle, other);
                    let less = self.builder.ins().uextend(types::I64, less);
                    let more = self.builder.ins().uextend(types::I64, more);
                    self.builder.ins().isub(more, less)
                }
                _ => {
                    let less = self.builder.ins().fcmp(FloatCC::LessThan, handle, other);
                    let more = self.builder.ins().fcmp(FloatCC::GreaterThan, handle, other);
                    let less = self.builder.ins().uextend(types::I64, less);
                    let more = self.builder.ins().uextend(types::I64, more);
                    self.builder.ins().isub(more, less)
                }
            };
            (Some(ordering), Rep::Int)
        };

        self.finish_collection(destination, result)
    }

    fn collection_sort_list_by_keys(
        &mut self,
        destination: Option<&MirPlace>,
        args: &[MirOperand],
        handle: Value,
    ) -> Lowered<()> {
        let result = {
            let [_, keys] = args else {
                return Err(Decline::new("sort by keys arity"));
            };
            let (keys, keys_rep) = self.scalar(keys)?;
            if keys_rep != Rep::List {
                return Err(Decline::new("sort keys representation"));
            }
            (
                self.call_runtime(&abi::LIST_SORTED_BY_KEYS, &[handle, keys])?,
                Rep::List,
            )
        };

        self.finish_collection(destination, result)
    }

    fn collection_append_list_copy(
        &mut self,
        destination: Option<&MirPlace>,
        args: &[MirOperand],
        receiver: &MirOperand,
        handle: Value,
    ) -> Lowered<()> {
        let result = {
            let [_, item] = args else {
                return Err(Decline::new("append arity"));
            };
            let element = self.element_type(self.operand_type(receiver)?)?;
            let (value, item_rep) = self.element_operand(item, element)?;
            let tag = self.cell_tag_value(item_rep, element)?;
            let bits = self.element_bits(value, item_rep);
            (
                self.call_runtime(&abi::LIST_APPENDED, &[handle, tag, bits])?,
                Rep::List,
            )
        };

        self.finish_collection(destination, result)
    }

    fn collection_slice_shared(
        &mut self,
        destination: Option<&MirPlace>,
        args: &[MirOperand],
        handle: Value,
        rep: Rep,
    ) -> Lowered<()> {
        let result = {
            let [_, start, end] = args else {
                return Err(Decline::new("slice arity"));
            };
            let (start, start_rep) = self.scalar(start)?;
            let (end, end_rep) = self.scalar(end)?;
            if start_rep != Rep::Int || end_rep != Rep::Int {
                return Err(Decline::new("slice bound representation"));
            }
            let symbol = if rep == Rep::Text {
                &abi::TEXT_SLICE
            } else {
                &abi::LIST_SLICE
            };
            (self.call_runtime(symbol, &[handle, start, end])?, rep)
        };

        self.finish_collection(destination, result)
    }

    fn collection_take_list(
        &mut self,
        destination: Option<&MirPlace>,
        op: MirCollectionOp,
        args: &[MirOperand],
        handle: Value,
    ) -> Lowered<()> {
        let result = {
            let [_, count] = args else {
                return Err(Decline::new("window arity"));
            };
            let (count, count_rep) = self.scalar(count)?;
            if count_rep != Rep::Int {
                return Err(Decline::new("window count representation"));
            }
            let mode = match op {
                MirCollectionOp::Take => 0,
                MirCollectionOp::TakeLast => 1,
                _ => 2,
            };
            let mode = self.builder.ins().iconst(types::I32, mode);
            (
                self.call_runtime(&abi::LIST_WINDOW, &[handle, mode, count])?,
                Rep::List,
            )
        };

        self.finish_collection(destination, result)
    }

    fn collection_first_list(
        &mut self,
        destination: Option<&MirPlace>,
        op: MirCollectionOp,
        handle: Value,
    ) -> Lowered<()> {
        let result = {
            let index = if op == MirCollectionOp::First {
                self.builder.ins().iconst(types::I64, 0)
            } else {
                let length = self
                    .call_runtime(&abi::LIST_LEN, &[handle])?
                    .ok_or_else(|| Decline::new("length result"))?;
                let one = self.builder.ins().iconst(types::I64, 1);
                self.builder.ins().isub(length, one)
            };
            (
                self.call_runtime(&abi::LIST_GET_OPT, &[handle, index])?,
                Rep::Opt,
            )
        };

        self.finish_collection(destination, result)
    }

    fn collection_remove_list_end(
        &mut self,
        destination: Option<&MirPlace>,
        op: MirCollectionOp,
        handle: Value,
    ) -> Lowered<()> {
        let result = {
            let symbol = if op == MirCollectionOp::RemoveFirst {
                &abi::LIST_REMOVE_FIRST
            } else {
                &abi::LIST_REMOVE_LAST
            };
            (self.call_runtime(symbol, &[handle])?, Rep::Opt)
        };

        self.finish_collection(destination, result)
    }

    fn collection_reverse_list_in_place(&mut self, handle: Value) -> Lowered<()> {
        self.call_runtime(&abi::LIST_REVERSE_IN_PLACE, &[handle])?;
        Ok(())
    }

    fn collection_signum_float(
        &mut self,
        destination: Option<&MirPlace>,
        handle: Value,
    ) -> Lowered<()> {
        let result = {
            // `f64::signum`: the sign of the operand, NaN stays NaN.
            let one = self.builder.ins().f64const(1.0);
            let signed = self.builder.ins().fcopysign(one, handle);
            let nan = self.builder.ins().fcmp(FloatCC::Unordered, handle, handle);
            (
                Some(self.builder.ins().select(nan, handle, signed)),
                Rep::Float,
            )
        };

        self.finish_collection(destination, result)
    }

    fn collection_compare_list(
        &mut self,
        destination: Option<&MirPlace>,
        args: &[MirOperand],
        handle: Value,
        rep: Rep,
    ) -> Lowered<()> {
        let result = {
            let [_, other] = args else {
                return Err(Decline::new("compare arity"));
            };
            let (other, other_rep) = self.scalar(other)?;
            if other_rep != rep {
                return Err(Decline::new("compare operand representation"));
            }
            (
                self.call_runtime(&abi::LIST_COMPARE, &[handle, other])?,
                Rep::Int,
            )
        };

        self.finish_collection(destination, result)
    }

    fn collection_scalar_math(
        &mut self,
        destination: Option<&MirPlace>,
        handle: Value,
        rep: Rep,
        function: MirMathFunction,
    ) -> Lowered<()> {
        let result = {
            let Some(destination) = destination else {
                return Err(Decline::new("scalar math without destination"));
            };
            let number = if rep == Rep::Int {
                self.builder.ins().fcvt_from_sint(types::F64, handle)
            } else {
                handle
            };
            let computed = match function {
                MirMathFunction::Abs => self.builder.ins().fabs(number),
                MirMathFunction::Sqrt => self.builder.ins().sqrt(number),
                other => {
                    let code = match other {
                        MirMathFunction::Exp => 0,
                        MirMathFunction::Ln => 1,
                        MirMathFunction::Log10 => 2,
                        MirMathFunction::Sin => 3,
                        MirMathFunction::Cos => 4,
                        MirMathFunction::Cbrt => 6,
                        _ => 5,
                    };
                    let code = self.builder.ins().iconst(types::I32, code);
                    self.call_runtime(&abi::MATH_F64, &[code, number])?
                        .ok_or_else(|| Decline::new("math result"))?
                }
            };
            // The result rounds to the destination's float width.
            let rounded = if self.float_narrow_place(destination) == 1 {
                let narrow = self.builder.ins().fdemote(types::F32, computed);
                self.builder.ins().fpromote(types::F64, narrow)
            } else {
                computed
            };
            (Some(rounded), Rep::Float)
        };

        self.finish_collection(destination, result)
    }

    fn collection_approximate_scalar(
        &mut self,
        destination: Option<&MirPlace>,
        args: &[MirOperand],
    ) -> Lowered<()> {
        let result = { self.approximate(args)? };

        self.finish_collection(destination, result)
    }

    fn collection_arg_extremum_list(
        &mut self,
        destination: Option<&MirPlace>,
        op: MirCollectionOp,
        handle: Value,
    ) -> Lowered<()> {
        let result = {
            let flag = i64::from(op == MirCollectionOp::ArgMax);
            let flag = self.builder.ins().iconst(types::I32, flag);
            (
                self.call_runtime(&abi::LIST_ARG_EXTREMUM, &[handle, flag])?,
                Rep::Opt,
            )
        };

        self.finish_collection(destination, result)
    }

    fn collection_set_algebra(
        &mut self,
        destination: Option<&MirPlace>,
        op: MirCollectionOp,
        args: &[MirOperand],
        handle: Value,
    ) -> Lowered<()> {
        let result = {
            let [_, other] = args else {
                return Err(Decline::new("set algebra arity"));
            };
            let (other, other_rep) = self.scalar(other)?;
            if other_rep != Rep::Set {
                return Err(Decline::new("set algebra operand"));
            }
            let code = match op {
                MirCollectionOp::Union => 0,
                MirCollectionOp::Intersection => 1,
                MirCollectionOp::Difference => 2,
                _ => 3,
            };
            let code = self.builder.ins().iconst(types::I32, code);
            (
                self.call_runtime(&abi::SET_ALGEBRA, &[handle, other, code])?,
                Rep::Set,
            )
        };

        self.finish_collection(destination, result)
    }

    fn collection_set_relation(
        &mut self,
        destination: Option<&MirPlace>,
        op: MirCollectionOp,
        args: &[MirOperand],
        handle: Value,
    ) -> Lowered<()> {
        let result = {
            let [_, other] = args else {
                return Err(Decline::new("set algebra arity"));
            };
            let (other, other_rep) = self.scalar(other)?;
            if other_rep != Rep::Set {
                return Err(Decline::new("set algebra operand"));
            }
            let mode = i64::from(op == MirCollectionOp::IsSuperset);
            let mode = self.builder.ins().iconst(types::I32, mode);
            (
                self.call_runtime(&abi::SET_SUBSET, &[handle, other, mode])?,
                Rep::Bool,
            )
        };

        self.finish_collection(destination, result)
    }

    fn collection_reverse_list(
        &mut self,
        destination: Option<&MirPlace>,
        handle: Value,
    ) -> Lowered<()> {
        let result = {
            (
                self.call_runtime(&abi::LIST_REVERSED, &[handle])?,
                Rep::List,
            )
        };

        self.finish_collection(destination, result)
    }

    fn collection_sort_list(
        &mut self,
        destination: Option<&MirPlace>,
        handle: Value,
    ) -> Lowered<()> {
        let result = { (self.call_runtime(&abi::LIST_SORTED, &[handle])?, Rep::List) };

        self.finish_collection(destination, result)
    }

    /// 1 when `place` holds a float narrower than `f64`, else 0.
    fn float_narrow_place(&self, place: &MirPlace) -> i64 {
        let Ok(ty) = self.place_type(place) else {
            return 0;
        };
        match resolve(self.state.input.types, ty.semantic_id()) {
            Type::SizedNumeric(Primitive::Fractus, width)
                if width.is_float() && *width != NumericWidth::F64 =>
            {
                1
            }
            _ => 0,
        }
    }

    /// Round a float through `f32`: the store and per-operation width of a
    /// float narrower than `f64`. The round trip is exact on
    /// an already-rounded value, so re-rounding is free.
    fn round_f32(&mut self, value: Value) -> Value {
        let narrow = self.builder.ins().fdemote(types::F32, value);
        self.builder.ins().fpromote(types::F64, narrow)
    }

    /// A float value as the cell `ty` holds it: rounded through `f32` when
    /// the cell is narrower than `f64`, unchanged for an `f64` cell.
    fn round_float_cell(&mut self, value: Value, ty: MirType) -> Value {
        if rounds_f32(self.state.input.types, ty) {
            self.round_f32(value)
        } else {
            value
        }
    }

    /// A float value at a declared `width`: rounded through `f32` when the
    /// width is narrower than `f64`, unchanged at `f64`.
    fn round_float_width(&mut self, value: Value, width: NumericWidth) -> Value {
        if width == NumericWidth::F64 {
            value
        } else {
            self.round_f32(value)
        }
    }

    /// An operand as a float; integers convert exactly as the runner does.
    fn float_operand(&mut self, operand: &MirOperand) -> Lowered<Value> {
        let (value, rep) = self.scalar(operand)?;
        match rep {
            Rep::Float => Ok(value),
            Rep::Int => Ok(self.builder.ins().fcvt_from_sint(types::F64, value)),
            _ => Err(Decline::new("approximate operand")),
        }
    }

    /// `f64::max`: a NaN operand yields the other one.
    fn rust_fmax(&mut self, a: Value, b: Value) -> Value {
        let greater = self.builder.ins().fcmp(FloatCC::GreaterThan, a, b);
        let picked = self.builder.ins().select(greater, a, b);
        let b_nan = self.builder.ins().fcmp(FloatCC::Unordered, b, b);
        self.builder.ins().select(b_nan, a, picked)
    }

    /// `≈` and `.approx`: the exact integer rule between two integers at the
    /// default tolerances, a float tolerance test otherwise.
    fn approximate(&mut self, args: &[MirOperand]) -> Lowered<(Option<Value>, Rep)> {
        match args {
            [left, right, tolerance] => {
                let left = self.float_operand(left)?;
                let right = self.float_operand(right)?;
                let tolerance = self.float_operand(tolerance)?;
                let difference = self.builder.ins().fsub(left, right);
                let distance = self.builder.ins().fabs(difference);
                let within = self
                    .builder
                    .ins()
                    .fcmp(FloatCC::LessThanOrEqual, distance, tolerance);
                Ok((Some(within), Rep::Bool))
            }
            [left, right, relative, absolute, negated] => {
                let (negated, negated_rep) = self.scalar(negated)?;
                if negated_rep != Rep::Bool {
                    return Err(Decline::new("approximate negation flag"));
                }
                let default_tolerance = matches!(
                    (relative, absolute),
                    (
                        MirOperand::Constant(MirConstant::Float(relative)),
                        MirOperand::Constant(MirConstant::Float(absolute)),
                    ) if (*relative - 1e-9).abs() == 0.0 && *absolute == 0.0
                );
                let (a, a_rep) = self.scalar(left)?;
                let (b, b_rep) = self.scalar(right)?;
                let equal = if a_rep == Rep::Int && b_rep == Rep::Int && default_tolerance {
                    self.call_runtime(&abi::APPROX_INT, &[a, b])?
                        .ok_or_else(|| Decline::new("approximate result"))?
                } else {
                    let narrow = [left, right].iter().all(|operand| {
                        self.operand_type(operand).is_ok_and(|ty| {
                            matches!(
                                resolve(self.state.input.types, ty.semantic_id()),
                                Type::SizedNumeric(Primitive::Fractus, width)
                                    if *width != NumericWidth::F64
                            )
                        })
                    });
                    let mut a = self.float_operand(left)?;
                    let mut b = self.float_operand(right)?;
                    let mut relative = self.float_operand(relative)?;
                    let mut absolute = self.float_operand(absolute)?;
                    if narrow {
                        for value in [&mut a, &mut b, &mut relative, &mut absolute] {
                            let reduced = self.builder.ins().fdemote(types::F32, *value);
                            *value = self.builder.ins().fpromote(types::F64, reduced);
                        }
                    }
                    let magnitude_a = self.builder.ins().fabs(a);
                    let magnitude_b = self.builder.ins().fabs(b);
                    let larger = self.rust_fmax(magnitude_a, magnitude_b);
                    let scaled = self.builder.ins().fmul(relative, larger);
                    let limit = self.rust_fmax(scaled, absolute);
                    let difference = self.builder.ins().fsub(a, b);
                    let distance = self.builder.ins().fabs(difference);
                    self.builder
                        .ins()
                        .fcmp(FloatCC::LessThanOrEqual, distance, limit)
                };
                Ok((Some(self.builder.ins().bxor(equal, negated)), Rep::Bool))
            }
            _ => Err(Decline::new("approximate arity")),
        }
    }

    /// Whether `value` is a plain read of a union-typed place.
    fn reads_union(&self, value: &MirValue) -> bool {
        let MirValueKind::Operand(operand @ MirOperand::Place(_)) = &value.kind else {
            return false;
        };
        self.operand_type(operand).is_ok_and(|ty| {
            matches!(
                resolve(self.state.input.types, ty.semantic_id()),
                Type::Union(_) | Type::Primitive(Primitive::Ignotum)
            )
        })
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

    /// An operand, possibly unit, with an integer in the `i64` carrier.
    fn operand_narrow(&mut self, operand: &MirOperand) -> Lowered<(Option<Value>, Rep)> {
        match self.operand(operand)? {
            (Some(value), Rep::Int) => Ok((Some(self.narrow(value)?), Rep::Int)),
            other => Ok(other),
        }
    }

    /// A scalar operand with an integer in the `i64` carrier: a wide
    /// intermediate narrows, trapping when it does not fit.
    fn scalar(&mut self, operand: &MirOperand) -> Lowered<(Value, Rep)> {
        let (value, rep) = self.scalar_wide(operand)?;
        if rep == Rep::Int {
            return Ok((self.narrow(value)?, rep));
        }
        Ok((value, rep))
    }

    /// A scalar operand as computed: an integer may be a wide intermediate.
    fn scalar_wide(&mut self, operand: &MirOperand) -> Lowered<(Value, Rep)> {
        match self.operand(operand)? {
            (Some(value), rep) => Ok((value, rep)),
            (None, _) => Err(Decline::new("unit operand")),
        }
    }

    // STATEMENTS

    fn assign(&mut self, place: &MirPlace, value: &MirValue) -> Lowered<()> {
        let target_rep = rep_of(self.state.input.types, self.place_type(place)?)?;
        let (computed, rep) = match (&value.kind, target_rep) {
            (MirValueKind::Operand(MirOperand::Constant(constant)), Rep::Bits(width)) => {
                (self.bits_constant(constant, width)?, Rep::Bits(width))
            }
            _ => self.value(value)?,
        };
        self.values.insert(value.id, (computed, rep));
        let computed = match computed {
            Some(computed) if rep == Rep::Int && target_rep != Rep::Inf => {
                let wide = place.projections.is_empty()
                    && self.wide_slots.contains(&Self::base_slot(place));
                Some(self.fit_int(computed, wide)?)
            }
            other => other,
        };
        if !place.projections.is_empty() {
            let Some(computed) = computed else {
                return Err(Decline::new("unit element store"));
            };
            let computed = self.coerce(computed, rep, target_rep)?;
            return self.write_projected(place, computed, target_rep);
        }
        let (variable, slot_rep) = self.place_slot(place)?;
        if let Some(computed) = computed {
            let computed = if rep == Rep::Opt && slot_rep != Rep::Opt && self.reads_union(value) {
                // Narrowing a union to the member a type test proved.
                let bits = self
                    .call_runtime(&abi::OPT_BITS, &[computed])?
                    .ok_or_else(|| Decline::new("narrowing result"))?;
                self.element_value(bits, slot_rep)
            } else {
                self.coerce(computed, rep, slot_rep)?
            };
            self.builder.def_var(variable, computed);
        }
        Ok(())
    }

    fn statement(&mut self, statement: &MirStatement) -> Lowered<()> {
        match &statement.kind {
            MirStatementKind::Assign { place, value } => self.assign(place, value),
            MirStatementKind::Call {
                destination: Some(destination),
                ..
            }
            | MirStatementKind::RuntimeCall {
                destination: Some(destination),
                ..
            } if !destination.projections.is_empty() => {
                self.statement_into_projection(statement, destination)
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
            } if matches!(
                aggregate.kind,
                MirAggregateKind::Map | MirAggregateKind::Set
            ) =>
            {
                self.construct_map(destination, aggregate)
            }
            MirStatementKind::Construct {
                destination,
                aggregate,
            } if matches!(
                aggregate.kind,
                MirAggregateKind::Struct(_)
                    | MirAggregateKind::EnumVariant(_)
                    | MirAggregateKind::Record
            ) =>
            {
                self.construct_struct(destination, aggregate)
            }
            MirStatementKind::Construct {
                destination,
                aggregate,
            } if aggregate.kind == MirAggregateKind::Tuple => {
                self.construct_tuple(destination, aggregate)
            }
            MirStatementKind::Construct {
                destination,
                aggregate,
            } if matches!(aggregate.kind, MirAggregateKind::Interval { .. }) => {
                self.construct_interval(destination, aggregate)
            }
            MirStatementKind::Construct {
                destination,
                aggregate,
            } if aggregate.kind == MirAggregateKind::SharedCell => {
                self.construct_cell(destination, aggregate)
            }
            MirStatementKind::Construct {
                destination,
                aggregate,
            } if matches!(aggregate.kind, MirAggregateKind::ClosureEnvironment(_)) => {
                let MirAggregateKind::ClosureEnvironment(id) = aggregate.kind else {
                    return Err(Decline::new("closure environment kind"));
                };
                self.construct_environment(destination, id, aggregate)
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
        let mut environment = None;
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
            MirCallee::Value(operand) => {
                return self.indirect_call(destination, operand, args);
            }
            MirCallee::Closure(closure) => {
                let (record, rep) = self.scalar(&closure.environment)?;
                if rep != Rep::Record {
                    return Err(Decline::new("closure environment representation"));
                }
                environment = Some(record);
                closure.function
            }
            MirCallee::Definition { .. } => return Err(Decline::new("indirect call")),
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
                .enumerate()
                .map(|(index, param)| {
                    rep_of(
                        self.state.input.types,
                        self.state.param_type(id, index, param.ty),
                    )
                })
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
        if self
            .state
            .input
            .program
            .functions
            .iter()
            .any(|function| function.id == id && function.is_generator)
        {
            return Err(Decline::new("direct call of a generator"));
        }
        let mut values = Vec::with_capacity(args.len() + 1);
        values.extend(environment);
        let by_address = self.address_flags(id);
        let returns = self.call_arguments(&by_address, args, &params, &mut values)?;
        let call = self.builder.ins().call(reference, &values);
        let results = self.builder.inst_results(call).to_vec();
        self.read_back_arguments(args, &returns)?;
        if let Some(destination) = destination
            && let Some(result) = results.first()
        {
            let (variable, _) = self.place_slot(destination)?;
            self.builder.def_var(variable, *result);
        }
        Ok(())
    }

    /// `@ cursor` consumed eagerly: run the generator to completion against a
    /// fresh collector list and read the list.
    fn cursor_stream(
        &mut self,
        destination: &MirPlace,
        callee: radix_hir::DefId,
        args: &[MirOperand],
    ) -> Lowered<()> {
        let mut matches = self
            .state
            .input
            .program
            .functions
            .iter()
            .filter(|function| function.source == Some(callee));
        let (Some(target), None) = (matches.next(), matches.next()) else {
            return Err(Decline::new("ambiguous generator"));
        };
        if !target.is_generator {
            return Err(Decline::new("cursor stream of a non-generator"));
        }
        let id = target.id;
        let params: Vec<Rep> = target
            .params
            .iter()
            .enumerate()
            .map(|(index, param)| {
                rep_of(
                    self.state.input.types,
                    self.state.param_type(id, index, param.ty),
                )
            })
            .collect::<Lowered<_>>()?;
        let reference = self.function_ref(id)?;
        let list = self
            .call_runtime(&abi::LIST_NEW, &[])?
            .ok_or_else(|| Decline::new("list new result"))?;
        let mut values = vec![list];
        for (position, arg) in args.iter().enumerate() {
            let (value, have) = self.scalar(arg)?;
            let value = match params.get(position) {
                Some(want) => self.coerce(value, have, *want)?,
                None => value,
            };
            values.push(value);
        }
        self.builder.ins().call(reference, &values);
        let (variable, slot_rep) = self.place_slot(destination)?;
        if slot_rep != Rep::List {
            return Err(Decline::new("cursor stream destination"));
        }
        self.builder.def_var(variable, list);
        Ok(())
    }

    /// A call through a function value.
    fn indirect_call(
        &mut self,
        destination: Option<&MirPlace>,
        callee: &MirOperand,
        args: &[MirOperand],
    ) -> Lowered<()> {
        let (object, rep) = self.scalar(callee)?;
        if rep != Rep::Func {
            return Err(Decline::new("call of a non-function"));
        }
        let ty = self.operand_type(callee)?;
        let result = self.indirect_call_value(object, ty, args)?;
        if let Some(destination) = destination
            && let Some((result, result_rep)) = result
        {
            let (variable, slot_rep) = self.place_slot(destination)?;
            let result = self.coerce(result, result_rep, slot_rep)?;
            self.builder.def_var(variable, result);
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
        let (target, param_reps, result_rep, error_rep, abi_signature, indirect_flags) =
            match callee {
                MirCallee::Value(operand) => self.try_indirect_target(operand)?,
                _ => self.try_direct_target(callee)?,
            };
        let mut values = Vec::with_capacity(args.len() + 1);
        if let TryTarget::Direct(_, Some(environment)) | TryTarget::Indirect(_, environment) =
            &target
        {
            values.push(*environment);
        }
        let by_address = match &target {
            TryTarget::Direct(id, _) => self.address_flags(*id),
            TryTarget::Indirect(..) => indirect_flags.clone(),
        };
        let returns = self.call_arguments(&by_address, args, &param_reps, &mut values)?;
        let call = match target {
            TryTarget::Direct(id, _) => {
                let reference = self.function_ref(id)?;
                self.builder.ins().call(reference, &values)
            }
            TryTarget::Indirect(code, _) => {
                let signature = abi_signature.ok_or_else(|| Decline::new("indirect signature"))?;
                let signature_ref = self.builder.import_signature(signature);
                self.builder
                    .ins()
                    .call_indirect(signature_ref, code, &values)
            }
        };
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
        self.read_back_arguments(args, &returns)?;
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
        self.read_back_arguments(args, &returns)?;
        let (variable, slot_rep) = self.place_slot(error_place)?;
        let error = self.element_value(payload, error_rep);
        let error = self.coerce(error, error_rep, slot_rep)?;
        self.builder.def_var(variable, error);
        self.builder.ins().jump(error_target, &[]);
        Ok(())
    }

    fn try_direct_target(&mut self, callee: &MirCallee) -> Lowered<TryCallInfo> {
        let types_table = self.state.input.types;
        let mut environment = None;
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
            MirCallee::Closure(closure) => {
                let (record, rep) = self.scalar(&closure.environment)?;
                if rep != Rep::Record {
                    return Err(Decline::new("closure environment representation"));
                }
                environment = Some(record);
                closure.function
            }
            _ => return Err(Decline::new("indirect call")),
        };
        let target = self
            .state
            .input
            .program
            .functions
            .iter()
            .find(|function| function.id == id)
            .ok_or_else(|| Decline::new("unknown callee"))?;
        let params = target
            .params
            .iter()
            .enumerate()
            .map(|(index, param)| rep_of(types_table, self.state.param_type(id, index, param.ty)))
            .collect::<Lowered<Vec<_>>>()?;
        let error = target
            .error_ty
            .map(|ty| rep_of(types_table, ty))
            .transpose()?
            .ok_or_else(|| Decline::new("try of a non-failable callee"))?;
        let result = rep_of(types_table, target.return_ty)?;
        Ok((
            TryTarget::Direct(id, environment),
            params,
            result,
            error,
            None,
            Vec::new(),
        ))
    }

    fn try_indirect_target(&mut self, operand: &MirOperand) -> Lowered<TryCallInfo> {
        let types_table = self.state.input.types;
        let (object, rep) = self.scalar(operand)?;
        if rep != Rep::Func {
            return Err(Decline::new("call of a non-function"));
        }
        let ty = self.operand_type(operand)?;
        let Type::Func(signature) = resolve(types_table, ty.semantic_id()) else {
            return Err(Decline::new("call of a non-function"));
        };
        let Some(err) = signature.err else {
            return Err(Decline::new("try of a non-failable callee"));
        };
        let flags = self.type_address_flags(signature);
        let mut abi_signature = self.state.module.make_signature();
        abi_signature.params.push(AbiParam::new(types::I64));
        let mut params = Vec::with_capacity(signature.params.len());
        for (position, param) in signature.params.iter().enumerate() {
            let rep = rep_of(
                types_table,
                MirType::semantic(param_value_type(types_table, param.ty)),
            )?;
            let Some(cl) = rep.cl_type() else {
                return Err(Decline::new("unit parameter"));
            };
            let cl = if flags.get(position).copied().unwrap_or(false) {
                types::I64
            } else {
                cl
            };
            abi_signature.params.push(AbiParam::new(cl));
            params.push(rep);
        }
        let result = rep_of(types_table, MirType::semantic(signature.ret))?;
        let error = rep_of(types_table, MirType::semantic(err))?;
        abi_signature.returns.push(AbiParam::new(types::I8));
        if let Some(cl) = result.cl_type() {
            abi_signature.returns.push(AbiParam::new(cl));
        }
        abi_signature.returns.push(AbiParam::new(types::I64));
        let (code, environment) = self.function_parts(object)?;
        Ok((
            TryTarget::Indirect(code, environment),
            params,
            result,
            error,
            Some(abi_signature),
            flags,
        ))
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
            MirIntrinsic::Convert(conversion) => self.convert(destination, conversion, &call.args),
            MirIntrinsic::FormatString { template } => {
                let Some(destination) = destination else {
                    return Err(Decline::new("format without destination"));
                };
                self.format_string(destination, *template, &call.args)
            }
            MirIntrinsic::FormatValue { spec } => {
                let ([operand], Some(destination)) = (call.args.as_slice(), destination) else {
                    return Err(Decline::new("format value shape"));
                };
                let rendered = self.format_value(*spec, operand)?;
                self.define_result(destination, Some(rendered), Rep::Text)
            }
            MirIntrinsic::Collection(MirCollectionOp::CordCursor) => {
                let ([text], Some(destination)) = (call.args.as_slice(), destination) else {
                    return Err(Decline::new("cord cursor shape"));
                };
                let (text, rep) = self.scalar(text)?;
                if !matches!(rep, Rep::Text | Rep::Char) {
                    return Err(Decline::new("cord cursor over a non-text"));
                }
                let letters = self.call_runtime(&abi::TEXT_CHARS, &[text])?;
                self.define_result(destination, letters, Rep::List)
            }
            // An eager cursor is its list of items: draining copies it.
            MirIntrinsic::Sermo(MirSermoOp::DrainCursor) => {
                let ([cursor], Some(destination)) = (call.args.as_slice(), destination) else {
                    return Err(Decline::new("cursor drain shape"));
                };
                let (cursor, rep) = self.scalar(cursor)?;
                if rep != Rep::List {
                    return Err(Decline::new("cursor drain of a non-list"));
                }
                let start = self.builder.ins().iconst(types::I64, 0);
                let end = self.builder.ins().iconst(types::I64, i64::MAX);
                let copied = self.call_runtime(&abi::LIST_SLICE, &[cursor, start, end])?;
                self.define_result(destination, copied, Rep::List)
            }
            // A conversation opens on a route: the route literal lowers to raw
            // bytes — a bare data symbol, deliberately not the headered text
            // handle — and the leaf answers a handle the destination carries.
            MirIntrinsic::SermoOpen => self.runtime_sermo_open(destination, call),
            // The opener a conversation reopens on: `nihil` clears it, a
            // `numerus` passes itself, a boxed `valor` passes its tagged
            // payload, and no other representation has a leaf symbol. The
            // call answers nothing, so nothing is stored.
            MirIntrinsic::SermoSetOpener => self.runtime_sermo_set_opener(call),
            MirIntrinsic::Collection(op) => self.collection_op(destination, *op, &call.args),
            MirIntrinsic::TypeCheck(op) => {
                let ([operand], Some(destination)) = (call.args.as_slice(), destination) else {
                    return Err(Decline::new("type check shape"));
                };
                let result = self.type_check(op, operand)?;
                self.define_result(destination, Some(result), Rep::Bool)
            }
            MirIntrinsic::Duplicate => self.runtime_duplicate(destination, call),
            MirIntrinsic::Cede => self.runtime_cede(destination, call),
            MirIntrinsic::CursorStream(stream) => {
                if stream.stepped {
                    return Err(Decline::new("stepped cursor stream"));
                }
                let Some(destination) = destination else {
                    return Err(Decline::new("cursor stream without destination"));
                };
                self.cursor_stream(destination, stream.callee, &call.args)
            }
            MirIntrinsic::Assert => self.runtime_assert(call),
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

    fn runtime_sermo_open(
        &mut self,
        destination: Option<&MirPlace>,
        call: &MirRuntimeCall,
    ) -> Lowered<()> {
        let Some(destination) = destination else {
            return Err(Decline::new("sermo open without destination"));
        };
        let [MirOperand::Constant(MirConstant::String(symbol))] = call.args.as_slice() else {
            return Err(Decline::new("sermo route operand"));
        };
        let text = radix_lexer::textus_literal::decode_textus_literal(
            self.state.input.interner.resolve(*symbol),
        );
        let data = self.state.string_data(text.as_bytes())?;
        let global = self
            .state
            .module
            .declare_data_in_func(data, self.builder.func);
        let route = self.builder.ins().symbol_value(self.state.pointer, global);
        let length = self
            .builder
            .ins()
            .iconst(types::I64, i64::try_from(text.len()).unwrap_or(0));
        let handle = self.call_runtime(&abi::SERMO_OPEN, &[route, length])?;
        self.define_result(destination, handle, Rep::Record)
    }

    fn runtime_sermo_set_opener(&mut self, call: &MirRuntimeCall) -> Lowered<()> {
        let [handle, opener] = call.args.as_slice() else {
            return Err(Decline::new("sermo opener shape"));
        };
        let (handle, _) = self.scalar(handle)?;
        match self.operand(opener)? {
            (_, Rep::Unit) => {
                self.call_runtime(&abi::SERMO_SET_OPENER_NIL, &[handle])?;
            }
            (_, Rep::Opt) if matches!(opener, MirOperand::Constant(MirConstant::Nil)) => {
                self.call_runtime(&abi::SERMO_SET_OPENER_NIL, &[handle])?;
            }
            (Some(value), Rep::Int) => {
                let value = self.narrow(value)?;
                self.call_runtime(&abi::SERMO_SET_OPENER_I64, &[handle, value])?;
            }
            (Some(value), Rep::Opt) => {
                self.call_runtime(&abi::SERMO_SET_OPENER_VALOR, &[handle, value])?;
            }
            _ => return Err(Decline::new("sermo opener representation")),
        }
        Ok(())
    }

    fn runtime_assert(&mut self, call: &MirRuntimeCall) -> Lowered<()> {
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

    fn runtime_duplicate(
        &mut self,
        destination: Option<&MirPlace>,
        call: &MirRuntimeCall,
    ) -> Lowered<()> {
        let ([argument], Some(destination)) = (call.args.as_slice(), destination) else {
            return Err(Decline::new("duplicate shape"));
        };
        let (value, rep) = self.scalar(argument)?;
        let copied = if matches!(rep, Rep::List | Rep::Record | Rep::Map | Rep::Set) {
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

    fn runtime_cede(
        &mut self,
        destination: Option<&MirPlace>,
        call: &MirRuntimeCall,
    ) -> Lowered<()> {
        let ([argument], Some(destination)) = (call.args.as_slice(), destination) else {
            return Err(Decline::new("cede shape"));
        };
        let (value, rep) = self.operand_narrow(argument)?;
        if let (Some(collector), Some(item)) = (self.collector, value) {
            let tag = self
                .builder
                .ins()
                .iconst(types::I32, Self::element_tag(rep)?);
            let bits = self.element_bits(item, rep);
            self.call_runtime(&abi::LIST_PUSH, &[collector, tag, bits])?;
        }
        self.define_result(destination, value, rep)
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
        self.render_text_at(value, rep, 0)
    }

    /// A value that holds a tuple inside a collection or optional displays
    /// through the runtime's list rendering, which has no tuple head: decline
    /// to display it rather than print it wrong.
    fn refuse_nested_tuple_display(&self, operand: &MirOperand) -> Lowered<()> {
        let Ok(ty) = self.operand_type(operand) else {
            return Ok(());
        };
        let table = self.state.input.types;
        let mut pending = vec![ty.semantic_id()];
        let mut depth = 0;
        while let Some(id) = pending.pop() {
            depth += 1;
            if depth > 32 {
                break;
            }
            match resolve(table, id) {
                Type::Tuple(_) if id != ty.semantic_id() => {
                    return Err(Decline::new("display of a nested tuple"));
                }
                Type::Array(element) | Type::BoundedArray { element, .. } | Type::Set(element) => {
                    pending.push(*element);
                }
                Type::Option(inner) => pending.push(*inner),
                Type::Map(key, value) => {
                    pending.push(*key);
                    pending.push(*value);
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// `1` when the operand is a float narrower than `f64`: it displays at
    /// its own width.
    pub(super) fn float_narrow(&self, operand: &MirOperand) -> i64 {
        let Ok(ty) = self.operand_type(operand) else {
            return 0;
        };
        match resolve(self.state.input.types, ty.semantic_id()) {
            Type::SizedNumeric(Primitive::Fractus, width)
                if width.is_float() && *width != NumericWidth::F64 =>
            {
                1
            }
            _ => 0,
        }
    }

    fn render_text_at(&mut self, value: Value, rep: Rep, narrow: i64) -> Lowered<Value> {
        let rendered = match rep {
            Rep::Text | Rep::Char => return Ok(value),
            Rep::Regex => self.call_runtime(&abi::REGEX_TEXT, &[value])?,
            Rep::Match => self.call_runtime(&abi::MATCH_TEXT, &[value])?,
            Rep::Dec => self.call_runtime(&abi::DEC_TEXT, &[value])?,
            Rep::Inf => self.call_runtime(&abi::INF_TEXT, &[value])?,
            Rep::Ins => self.call_runtime(&abi::INS_TEXT, &[value])?,
            Rep::Int if self.is_wide(value) => self.text_from_wide(value)?,
            Rep::Int => self.call_runtime(&abi::TEXT_FROM_I64, &[value])?,
            Rep::Float => {
                let narrow = self.builder.ins().iconst(types::I32, narrow);
                self.call_runtime(&abi::TEXT_FROM_F64, &[value, narrow])?
            }
            Rep::Bool => self.call_runtime(&abi::TEXT_FROM_BOOL, &[value])?,
            Rep::List => self.call_runtime(&abi::TEXT_FROM_LIST, &[value])?,
            Rep::Record => self.call_runtime(&abi::TEXT_FROM_RECORD, &[value])?,
            Rep::Opt => self.call_runtime(&abi::TEXT_FROM_OPT, &[value])?,
            Rep::Map | Rep::Set => self.call_runtime(&abi::TEXT_FROM_MAP, &[value])?,
            Rep::Bits(width) => {
                let bits = self
                    .builder
                    .ins()
                    .iconst(types::I32, i64::from(width.bits()));
                self.call_runtime(&abi::TEXT_FROM_BITS, &[value, bits])?
            }
            Rep::Func => return Err(Decline::new("function value as text")),
            Rep::Tuple => return Err(Decline::new("tuple as text")),
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
        enum Part {
            Literal(String),
            Argument(usize),
        }
        let template = self.state.input.interner.resolve(template).to_owned();
        let mut rendered: Vec<Value> = Vec::with_capacity(args.len());
        for arg in args {
            let (value, rep) = self.scalar_wide(arg)?;
            self.refuse_nested_tuple_display(arg)?;
            let narrow = self.float_narrow(arg);
            rendered.push(self.render_text_at(value, rep, narrow)?);
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
        destination: Option<&MirPlace>,
        conversion: &MirConversion,
        args: &[MirOperand],
    ) -> Lowered<()> {
        let [argument] = args else {
            return Err(Decline::new("conversion arity"));
        };
        // A unit conversation is consumed for effects and has no result value,
        // so a drain statement may carry no destination at all.
        if conversion.store.is_none()
            && self.state.is_sermo_handle(conversion.source_ty)
            && matches!(
                resolve(self.state.input.types, conversion.target_ty.semantic_id()),
                Type::Primitive(Primitive::Vacuum)
            )
        {
            let (handle, _rep) = self.scalar(argument)?;
            self.call_runtime(&abi::SERMO_DRAIN, &[handle])?;
            return match destination {
                Some(destination) => self.define_result(destination, None, Rep::Unit),
                None => Ok(()),
            };
        }
        // Every other conversion stores a result.
        let Some(destination) = destination else {
            return Err(Decline::new("conversion without destination"));
        };
        // The sync conversation's `sermo ↦ numerus` route materializes the
        // opened conversation's leaf value as an integer.
        if conversion.store.is_none()
            && self.state.is_sermo_handle(conversion.source_ty)
            && matches!(
                resolve(self.state.input.types, conversion.target_ty.semantic_id()),
                Type::Primitive(Primitive::Numerus)
            )
        {
            let (handle, _rep) = self.scalar(argument)?;
            let fallback = match &conversion.recovery {
                Some(recovery) => self.scalar(recovery)?.0,
                None => self.builder.ins().iconst(types::I64, 0),
            };
            let value = self.call_runtime(&abi::SERMO_MATERIALIZE_I64, &[handle, fallback])?;
            return self.define_result(destination, value, Rep::Int);
        }
        // A conversation can also materialize its leaf's integer epoch as an
        // instant at the target type's precision.
        if conversion.store.is_none()
            && self.state.is_sermo_handle(conversion.source_ty)
            && matches!(
                resolve(self.state.input.types, conversion.target_ty.semantic_id()),
                Type::Primitive(Primitive::Instans) | Type::SizedInstans(_)
            )
        {
            let (handle, _rep) = self.scalar(argument)?;
            let fallback = match &conversion.recovery {
                Some(recovery) => self.scalar(recovery)?.0,
                None => self.builder.ins().iconst(types::I64, 0),
            };
            let epoch = self
                .call_runtime(&abi::SERMO_MATERIALIZE_I64, &[handle, fallback])?
                .ok_or_else(|| Decline::new("sermo materialize result"))?;
            let convert::Num::Ins(code) =
                convert::num_of(self.state.input.types, conversion.target_ty.semantic_id())
                    .ok_or_else(|| Decline::new("sermo instant precision"))?
            else {
                return Err(Decline::new("sermo instant precision"));
            };
            let code = self.builder.ins().iconst(types::I32, code);
            let instant = self
                .call_runtime(&abi::INS_FROM_EPOCH, &[epoch, code])?
                .ok_or_else(|| Decline::new("instant result"))?;
            return self.define_result(destination, Some(instant), Rep::Ins);
        }
        // Every other target from a conversation handle has no CLIF materialize
        // symbol; decline it rather than let the handle render as a record.
        if conversion.store.is_none() && self.state.is_sermo_handle(conversion.source_ty) {
            return Err(Decline::new("sermo materialization target"));
        }
        match &conversion.store {
            None => self.explicit_convert(destination, conversion, argument),
            Some(site) => self.store_convert(destination, conversion, argument, site),
        }
    }

    /// An explicit conversion, including boxing, sequence conversion and rendering.
    fn explicit_convert(
        &mut self,
        destination: &MirPlace,
        conversion: &MirConversion,
        argument: &MirOperand,
    ) -> Lowered<()> {
        let (value, rep) = self.scalar(argument)?;
        let (variable, slot_rep) = self.place_slot(destination)?;
        if rep == Rep::Set && slot_rep == Rep::List {
            let keys = self
                .call_runtime(&abi::MAP_KEYS, &[value])?
                .ok_or_else(|| Decline::new("set keys result"))?;
            self.builder.def_var(variable, keys);
            return Ok(());
        }
        // Open data (`json`, `valor`) is a tagged box, so an operand
        // carrying any other representation is boxed into it. A leaf
        // opener is typed `valor` while its operand keeps its own scalar
        // representation, so the box is keyed on the operand rather than
        // on the conversion's (identical) declared types.
        if rep != Rep::Opt
            && slot_rep == Rep::Opt
            && matches!(
                resolve(self.state.input.types, conversion.target_ty.semantic_id()),
                Type::Primitive(Primitive::Json | Primitive::Valor)
            )
        {
            let tag = self
                .builder
                .ins()
                .iconst(types::I32, Self::element_tag(rep)?);
            let bits = self.element_bits(value, rep);
            let boxed = self
                .call_runtime(&abi::DYN_BOX, &[tag, bits])?
                .ok_or_else(|| Decline::new("open data result"))?;
            self.builder.def_var(variable, boxed);
            return Ok(());
        }
        // An identity conversion of an open-data box is a copy: the box
        // already carries its tagged payload.
        if rep == Rep::Opt
            && slot_rep == Rep::Opt
            && resolve(self.state.input.types, conversion.source_ty.semantic_id())
                == resolve(self.state.input.types, conversion.target_ty.semantic_id())
            && matches!(
                resolve(self.state.input.types, conversion.target_ty.semantic_id()),
                Type::Primitive(Primitive::Json | Primitive::Valor)
            )
        {
            self.builder.def_var(variable, value);
            return Ok(());
        }
        if let Some(plan) = sequence_conversion(
            self.state.input.types,
            conversion.source_ty.semantic_id(),
            conversion.target_ty.semantic_id(),
        ) {
            return self.sequence_convert(plan, conversion, value, variable);
        }
        if let Some(plan) = numeric_conversion(
            self.state.input.types,
            conversion.source_ty.semantic_id(),
            conversion.target_ty.semantic_id(),
        ) {
            return self.numeric_convert(plan, conversion, value, variable);
        }
        if slot_rep != Rep::Text || rep == Rep::Unit {
            return Err(Decline::new("explicit conversion"));
        }
        self.refuse_nested_tuple_display(argument)?;
        let narrow = self.float_narrow(argument);
        let rendered = self.render_text_at(value, rep, narrow)?;
        self.builder.def_var(variable, rendered);
        Ok(())
    }

    fn store_convert(
        &mut self,
        destination: &MirPlace,
        conversion: &MirConversion,
        argument: &MirOperand,
        site: &radix_hir::HirStoreSite,
    ) -> Lowered<()> {
        let (value, rep) = self.scalar_wide(argument)?;
        let (variable, slot_rep) = self.place_slot(destination)?;
        let (value, rep) = if rep == Rep::Int && slot_rep == Rep::Dec {
            (self.decimal_operand(value, rep, true)?, Rep::Dec)
        } else {
            (value, rep)
        };
        if rep == Rep::Dec && slot_rep == Rep::Dec {
            let stored = self.decimal_store(site, value)?;
            self.builder.def_var(variable, stored);
            return Ok(());
        }
        let stored = match (rep, slot_rep) {
            (Rep::Int, Rep::Int) => self.int_store(site, conversion.target_ty, value)?,
            (Rep::Float, Rep::Float) => {
                // A conversion store lands in a typed cell: a float target
                // narrower than `f64` rounds through `f32` at the slot.
                self.round_float_cell(value, conversion.target_ty)
            }
            (Rep::Bool, Rep::Bool)
            | (Rep::Text, Rep::Text)
            | (Rep::List, Rep::List)
            | (Rep::Record, Rep::Record)
            | (Rep::Opt, Rep::Opt)
            | (Rep::Map, Rep::Map)
            | (Rep::Set, Rep::Set)
            | (Rep::Func, Rep::Func)
            | (Rep::Tuple, Rep::Tuple) => value,
            (Rep::Bits(from), Rep::Bits(to)) if from == to => value,
            _ => return Err(Decline::new("conversion between representations")),
        };
        let stored = if rep == Rep::Int {
            let wide = self.wide_slots.contains(&Self::base_slot(destination));
            self.fit_int(stored, wide)?
        } else {
            stored
        };
        self.builder.def_var(variable, stored);
        Ok(())
    }

    /// A decimal handle for an operand: a decimal as is, an integer as the
    /// scale-8 carrier (`carrier`, arithmetic) or as a whole number.
    fn decimal_operand(&mut self, value: Value, rep: Rep, carrier: bool) -> Lowered<Value> {
        let value = match rep {
            Rep::Dec => return Ok(value),
            Rep::Int => self.narrow(value)?,
            _ => return Err(Decline::new("decimal and non-decimal operands")),
        };
        let symbol = if carrier {
            &abi::DEC_STORED
        } else {
            &abi::DEC_INT
        };
        self.call_runtime(symbol, &[value])?
            .ok_or_else(|| Decline::new("decimal operand result"))
    }

    /// An inf handle for an operand: an inf as is, an integer by value.
    fn inf_operand(&mut self, value: Value, rep: Rep) -> Lowered<Value> {
        let result = match rep {
            Rep::Inf => return Ok(value),
            Rep::Int if self.is_wide(value) => {
                let (low, high) = self.split_wide(value);
                self.call_runtime(&abi::INF_WIDE, &[low, high])?
            }
            Rep::Int => self.call_runtime(&abi::INF_INT, &[value])?,
            _ => return Err(Decline::new("inf and non-integer operands")),
        };
        result.ok_or_else(|| Decline::new("inf operand result"))
    }

    /// A binary operation with an `inf` operand or result.
    fn inf_binary(
        &mut self,
        op: MirBinOp,
        lhs: (Value, Rep),
        rhs: (Value, Rep),
    ) -> Lowered<(Value, Rep)> {
        let a = self.inf_operand(lhs.0, lhs.1)?;
        let b = self.inf_operand(rhs.0, rhs.1)?;
        let arithmetic = match op {
            MirBinOp::Add => Some(0),
            MirBinOp::Sub => Some(1),
            MirBinOp::Mul => Some(2),
            MirBinOp::Div => Some(3),
            MirBinOp::Mod => Some(4),
            _ => None,
        };
        if let Some(code) = arithmetic {
            let code = self.builder.ins().iconst(types::I32, code);
            let result = self
                .call_runtime(&abi::INF_BINARY, &[a, b, code])?
                .ok_or_else(|| Decline::new("inf result"))?;
            return Ok((result, Rep::Inf));
        }
        let cc = match op {
            MirBinOp::Eq => IntCC::Equal,
            MirBinOp::NotEq => IntCC::NotEqual,
            MirBinOp::Lt => IntCC::SignedLessThan,
            MirBinOp::Gt => IntCC::SignedGreaterThan,
            MirBinOp::LtEq => IntCC::SignedLessThanOrEqual,
            MirBinOp::GtEq => IntCC::SignedGreaterThanOrEqual,
            _ => return Err(Decline::new("inf operation")),
        };
        let order = self
            .call_runtime(&abi::INF_COMPARE, &[a, b])?
            .ok_or_else(|| Decline::new("inf comparison result"))?;
        let zero = self.builder.ins().iconst(types::I32, 0);
        Ok((self.builder.ins().icmp(cc, order, zero), Rep::Bool))
    }

    /// A binary operation with a decimal operand or result.
    fn dec_binary(
        &mut self,
        op: MirBinOp,
        lhs: (Value, Rep),
        rhs: (Value, Rep),
    ) -> Lowered<(Value, Rep)> {
        let arithmetic = match op {
            MirBinOp::Add => Some(0),
            MirBinOp::Sub => Some(1),
            MirBinOp::Mul => Some(2),
            MirBinOp::Div => Some(3),
            MirBinOp::Mod => Some(4),
            _ => None,
        };
        let a = self.decimal_operand(lhs.0, lhs.1, arithmetic.is_some())?;
        let b = self.decimal_operand(rhs.0, rhs.1, arithmetic.is_some())?;
        if let Some(code) = arithmetic {
            let code = self.builder.ins().iconst(types::I32, code);
            let result = self
                .call_runtime(&abi::DEC_BINARY, &[a, b, code])?
                .ok_or_else(|| Decline::new("decimal result"))?;
            return Ok((result, Rep::Dec));
        }
        let cc = match op {
            MirBinOp::Eq => IntCC::Equal,
            MirBinOp::NotEq => IntCC::NotEqual,
            MirBinOp::Lt => IntCC::SignedLessThan,
            MirBinOp::Gt => IntCC::SignedGreaterThan,
            MirBinOp::LtEq => IntCC::SignedLessThanOrEqual,
            MirBinOp::GtEq => IntCC::SignedGreaterThanOrEqual,
            _ => return Err(Decline::new("decimal operation")),
        };
        let order = self
            .call_runtime(&abi::DEC_COMPARE, &[a, b])?
            .ok_or_else(|| Decline::new("decimal comparison result"))?;
        let zero = self.builder.ins().iconst(types::I32, 0);
        Ok((self.builder.ins().icmp(cc, order, zero), Rep::Bool))
    }

    /// An integer against a float, compared exactly (the integer is never
    /// rounded to a float first).
    fn int_float_compare(
        &mut self,
        op: MirBinOp,
        lhs: (Value, Rep),
        rhs: (Value, Rep),
    ) -> Lowered<(Value, Rep)> {
        let (int, float, flipped) = if lhs.1 == Rep::Int {
            (lhs.0, rhs.0, false)
        } else {
            (rhs.0, lhs.0, true)
        };
        let int = self.narrow(int)?;
        let op = match (op, flipped) {
            (MirBinOp::Lt, true) => MirBinOp::Gt,
            (MirBinOp::Gt, true) => MirBinOp::Lt,
            (MirBinOp::LtEq, true) => MirBinOp::GtEq,
            (MirBinOp::GtEq, true) => MirBinOp::LtEq,
            (other, _) => other,
        };
        let order = self
            .call_runtime(&abi::INT_FLOAT_COMPARE, &[int, float])?
            .ok_or_else(|| Decline::new("comparison result"))?;
        let at = |this: &mut Self, cc: IntCC, value: i64| {
            let value = this.builder.ins().iconst(types::I32, value);
            this.builder.ins().icmp(cc, order, value)
        };
        let result = match op {
            MirBinOp::Eq => at(self, IntCC::Equal, 0),
            // An unordered pair (NaN) is unequal.
            MirBinOp::NotEq => at(self, IntCC::NotEqual, 0),
            MirBinOp::Lt => at(self, IntCC::Equal, -1),
            MirBinOp::Gt => at(self, IntCC::Equal, 1),
            MirBinOp::LtEq => {
                let less = at(self, IntCC::Equal, -1);
                let same = at(self, IntCC::Equal, 0);
                self.builder.ins().bor(less, same)
            }
            MirBinOp::GtEq => {
                let more = at(self, IntCC::Equal, 1);
                let same = at(self, IntCC::Equal, 0);
                self.builder.ins().bor(more, same)
            }
            _ => return Err(Decline::new("mixed integer and float operation")),
        };
        Ok((result, Rep::Bool))
    }

    /// An exact decimal lands in a `d64` slot: round half-even to scale 8, trap
    /// outside the `i64` carrier.
    fn decimal_store(&mut self, site: &radix_hir::HirStoreSite, value: Value) -> Lowered<Value> {
        let stored = self
            .call_runtime(&abi::DEC_STORE, &[value])?
            .ok_or_else(|| Decline::new("decimal store result"))?;
        let slot = site
            .slot
            .map(|slot| self.state.input.interner.resolve(slot).to_owned());
        let suffix = radix_mir::store_trap::store_trap_suffix(
            site,
            slot.as_deref(),
            NumericWidth::D64.rust_type(),
            false,
        );
        let zero = self.builder.ins().iconst(types::I64, 0);
        let outside = self.builder.ins().icmp(IntCC::Equal, stored, zero);
        let trap = self.builder.create_block();
        self.builder.set_cold_block(trap);
        let next = self.builder.create_block();
        self.builder.ins().brif(outside, trap, &[], next, &[]);
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
        let reference = self.runtime_ref(&abi::DEC_TRAP_STORE)?;
        self.builder
            .ins()
            .call(reference, &[value, pointer, length]);
        self.builder.ins().trap(TrapCode::unwrap_user(1));
        self.builder.switch_to_block(next);
        Ok(stored)
    }

    fn int_store(
        &mut self,
        site: &radix_hir::HirStoreSite,
        target: MirType,
        value: Value,
    ) -> Lowered<Value> {
        let table = self.state.input.types;
        let (policy, width) = match resolve(table, target.semantic_id()) {
            Type::Primitive(Primitive::Numerus) => (StorePolicy::Trapping, NumericWidth::I64),
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
        let wide = self.is_wide(value);
        match policy {
            StorePolicy::Trapping => {
                if width == NumericWidth::I64 && !wide {
                    return Ok(value);
                }
                let below = {
                    let bound = self.bound_const(low, wide);
                    self.builder.ins().icmp(IntCC::SignedLessThan, value, bound)
                };
                let above = {
                    let bound = self.bound_const(high, wide);
                    self.builder
                        .ins()
                        .icmp(IntCC::SignedGreaterThan, value, bound)
                };
                let outside = if wide && width == NumericWidth::U64 {
                    // A u64 at or above 2^63 is a valid value the i64 carrier
                    // cannot hold: refuse it by name at run time.
                    self.trap_if(
                        above,
                        "u64 value beyond 2^63 is not carried by the clif target",
                    )?;
                    below
                } else {
                    self.builder.ins().bor(below, above)
                };
                self.trap_store_if(outside, value, site, width)?;
                self.narrow(value)
            }
            StorePolicy::Saturating => {
                let low = self.bound_const(low, wide);
                let high = self.bound_const(high, wide);
                let raised = self.builder.ins().smax(value, low);
                let clamped = self.builder.ins().smin(raised, high);
                self.narrow(clamped)
            }
            StorePolicy::Wrapping => {
                // The low 64 bits agree with every narrower modulus.
                let value = if wide {
                    self.builder.ins().ireduce(types::I64, value)
                } else {
                    value
                };
                match width {
                    NumericWidth::U8 | NumericWidth::U16 | NumericWidth::U32 => {
                        let mask = self.builder.ins().iconst(types::I64, high);
                        Ok(self.builder.ins().band(value, mask))
                    }
                    NumericWidth::I8 => Ok(self.reduce_word(value, Reduction::Sign(8))),
                    NumericWidth::I16 => Ok(self.reduce_word(value, Reduction::Sign(16))),
                    NumericWidth::I32 => Ok(self.reduce_word(value, Reduction::Sign(32))),
                    NumericWidth::I64 => Ok(value),
                    _ => Err(Decline::new("wrapping store at this width")),
                }
            }
        }
    }

    /// A signed bound in the carrier of the value it is compared with.
    fn bound_const(&mut self, bound: i64, wide: bool) -> Value {
        if wide {
            let low = self.builder.ins().iconst(types::I64, bound);
            let high = self.builder.ins().iconst(types::I64, bound >> 63);
            self.builder.ins().iconcat(low, high)
        } else {
            self.builder.ins().iconst(types::I64, bound)
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
        if self.is_wide(value) {
            let (low, high) = self.split_wide(value);
            let reference = self.runtime_ref(&abi::TRAP_STORE_WIDE)?;
            self.builder
                .ins()
                .call(reference, &[low, high, pointer, length, unsigned]);
        } else {
            let reference = self.runtime_ref(&abi::TRAP_STORE)?;
            self.builder
                .ins()
                .call(reference, &[value, pointer, length, unsigned]);
        }
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
        let (value, rep) = self.scalar_wide(argument)?;
        self.refuse_nested_tuple_display(argument)?;
        match rep {
            Rep::Int if self.is_wide(value) => self.print_wide(stream, value)?,
            Rep::Int => {
                let reference = self.runtime_ref(&abi::PRINT_I64)?;
                self.builder.ins().call(reference, &[stream, value]);
            }
            Rep::Float => {
                let reference = self.runtime_ref(&abi::PRINT_F64)?;
                let narrow = self.float_narrow(argument);
                let narrow = self.builder.ins().iconst(types::I32, narrow);
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
            Rep::Map | Rep::Set => {
                let reference = self.runtime_ref(&abi::PRINT_MAP)?;
                self.builder.ins().call(reference, &[stream, value]);
            }
            Rep::Regex | Rep::Match | Rep::Dec | Rep::Inf | Rep::Ins => {
                let text = self.render_text(value, rep)?;
                let reference = self.runtime_ref(&abi::PRINT_TEXT)?;
                self.builder.ins().call(reference, &[stream, text]);
            }
            Rep::Text | Rep::Char => {
                let reference = self.runtime_ref(&abi::PRINT_TEXT)?;
                self.builder.ins().call(reference, &[stream, value]);
            }
            Rep::Func => return Err(Decline::new("function value print")),
            Rep::Tuple => return Err(Decline::new("tuple print")),
            Rep::Unit => return Err(Decline::new("unit print")),
        }
        Ok(())
    }

    // VALUES

    /// How a `wrapping<W>` result type reduces: unsigned words mask, signed
    /// words sign-extend from their width.
    fn word_reduction(&self, ty: MirType) -> Option<Reduction> {
        match resolve(self.state.input.types, ty.semantic_id()) {
            Type::ModularWord(NumericWidth::U8) => Some(Reduction::Mask(0xff)),
            Type::ModularWord(NumericWidth::U16) => Some(Reduction::Mask(0xffff)),
            Type::ModularWord(NumericWidth::U32) => Some(Reduction::Mask(0xffff_ffff)),
            Type::ModularWord(NumericWidth::I8) => Some(Reduction::Sign(8)),
            Type::ModularWord(NumericWidth::I16) => Some(Reduction::Sign(16)),
            Type::ModularWord(NumericWidth::I32) => Some(Reduction::Sign(32)),
            Type::ModularWord(NumericWidth::I64) => Some(Reduction::Sign(64)),
            _ => None,
        }
    }

    fn reduce_word(&mut self, value: Value, reduction: Reduction) -> Value {
        match reduction {
            Reduction::Mask(mask) => {
                let mask = self.builder.ins().iconst(types::I64, mask);
                self.builder.ins().band(value, mask)
            }
            Reduction::Sign(64) => value,
            Reduction::Sign(bits) => {
                let shift = i64::from(64 - bits);
                let raised = self.builder.ins().ishl_imm_u(value, shift);
                self.builder.ins().sshr_imm_u(raised, shift)
            }
        }
    }

    /// An operation whose result is a modular word: computed modulo 2^64
    /// (the low bits agree with every narrower modulus) and masked to the
    /// word, as the runner wraps each word operation.
    fn word_value(
        &mut self,
        value: &MirValue,
        reduction: Reduction,
    ) -> Lowered<Option<(Value, Rep)>> {
        let computed = match &value.kind {
            MirValueKind::Unary { op, operand } if matches!(op, MirUnOp::BitNot | MirUnOp::Neg) => {
                let (operand, rep) = self.scalar(operand)?;
                if rep != Rep::Int {
                    return Ok(None);
                }
                if *op == MirUnOp::BitNot {
                    self.builder.ins().bnot(operand)
                } else {
                    self.builder.ins().ineg(operand)
                }
            }
            MirValueKind::Binary { op, lhs, rhs }
                if matches!(
                    op,
                    MirBinOp::Add
                        | MirBinOp::Sub
                        | MirBinOp::Mul
                        | MirBinOp::Div
                        | MirBinOp::Mod
                        | MirBinOp::BitAnd
                        | MirBinOp::BitOr
                        | MirBinOp::BitXor
                        | MirBinOp::Shl
                        | MirBinOp::Shr
                ) =>
            {
                let (a, a_rep) = self.scalar(lhs)?;
                let (b, b_rep) = self.scalar(rhs)?;
                if a_rep != Rep::Int || b_rep != Rep::Int {
                    return Ok(None);
                }
                match op {
                    MirBinOp::Add => self.builder.ins().iadd(a, b),
                    MirBinOp::Sub => self.builder.ins().isub(a, b),
                    MirBinOp::Mul => self.builder.ins().imul(a, b),
                    // Division acts on the reduced representatives; only the
                    // result is reduced (`-128 / -1` is 128, which wraps).
                    MirBinOp::Div => self.floor_div(a, b, false)?,
                    MirBinOp::Mod => self.floor_div(a, b, true)?,
                    MirBinOp::BitAnd => self.builder.ins().band(a, b),
                    MirBinOp::BitOr => self.builder.ins().bor(a, b),
                    MirBinOp::BitXor => self.builder.ins().bxor(a, b),
                    MirBinOp::Shl => {
                        let zero = self.builder.ins().iconst(types::I64, 0);
                        let limit = self.builder.ins().iconst(types::I64, 64);
                        let large =
                            self.builder
                                .ins()
                                .icmp(IntCC::UnsignedGreaterThanOrEqual, b, limit);
                        let moved = self.builder.ins().ishl(a, b);
                        self.builder.ins().select(large, zero, moved)
                    }
                    _ => {
                        let limit = self.builder.ins().iconst(types::I64, 63);
                        let clamped = self.builder.ins().umin(b, limit);
                        // The representative is non-negative for an unsigned
                        // word, so the arithmetic shift is its floor division.
                        self.builder.ins().sshr(a, clamped)
                    }
                }
            }
            _ => return Ok(None),
        };
        Ok(Some((self.reduce_word(computed, reduction), Rep::Int)))
    }

    fn value(&mut self, value: &MirValue) -> Lowered<(Option<Value>, Rep)> {
        if let Some(reduction) = self.word_reduction(value.ty)
            && let Some((computed, rep)) = self.word_value(value, reduction)?
        {
            return Ok((Some(computed), rep));
        }
        let (computed, rep) = match &value.kind {
            MirValueKind::Operand(operand) => self.operand(operand),
            MirValueKind::Unary { op, operand } => {
                let cap = wide::cap_int(self.state.input.types, value.ty);
                if *op == MirUnOp::Neg && rep_of(self.state.input.types, value.ty)? == Rep::Dec {
                    let (carrier, rep) = self.scalar_wide(operand)?;
                    let handle = self.decimal_operand(carrier, rep, true)?;
                    let negated = self.call_runtime(&abi::DEC_NEG, &[handle])?;
                    let negated = negated.ok_or_else(|| Decline::new("decimal result"))?;
                    return Ok((Some(negated), Rep::Dec));
                }
                if *op == MirUnOp::Neg && rep_of(self.state.input.types, value.ty)? == Rep::Inf {
                    let (integer, rep) = self.scalar_wide(operand)?;
                    let handle = self.inf_operand(integer, rep)?;
                    let negated = self.call_runtime(&abi::INF_NEG, &[handle])?;
                    let negated = negated.ok_or_else(|| Decline::new("inf result"))?;
                    return Ok((Some(negated), Rep::Inf));
                }
                let (computed, rep) = self.unary(*op, operand, cap)?;
                Ok((Some(computed), rep))
            }
            MirValueKind::Option(op) => {
                let (computed, rep) = self.option_op(op, value.ty)?;
                Ok((Some(computed), rep))
            }
            MirValueKind::Binary { op, lhs, rhs } => {
                let result = rep_of(self.state.input.types, value.ty)?;
                let cap = wide::cap_int(self.state.input.types, value.ty);
                let narrow = result == Rep::Float && rounds_f32(self.state.input.types, value.ty);
                let (computed, rep) = self.binary(*op, lhs, rhs, result, cap, narrow)?;
                Ok((Some(computed), rep))
            }
            MirValueKind::Closure(closure) => Ok((Some(self.closure_value(closure)?), Rep::Func)),
            MirValueKind::TensorExtent { .. } => Err(Decline::new("value kind")),
        }?;
        // A float value narrower than `f64` lands at its declared width
        // here, where the computation stores into a typed cell, exactly as
        // the runner's `apply_declared_float_width` rounds at the value's
        // type.
        let (computed, rep) = match (computed, rep) {
            (Some(computed), Rep::Float) => (Some(self.round_float_cell(computed, value.ty)), rep),
            other => other,
        };
        Ok((computed, rep))
    }

    fn unary(&mut self, op: MirUnOp, operand: &MirOperand, cap: bool) -> Lowered<(Value, Rep)> {
        let (value, rep) = self.scalar_wide(operand)?;
        if rep == Rep::Inf && op == MirUnOp::Neg {
            let negated = self.call_runtime(&abi::INF_NEG, &[value])?;
            return Ok((negated.ok_or_else(|| Decline::new("inf result"))?, Rep::Inf));
        }
        if rep == Rep::Dec && op == MirUnOp::Neg {
            let negated = self.call_runtime(&abi::DEC_NEG, &[value])?;
            return Ok((
                negated.ok_or_else(|| Decline::new("decimal result"))?,
                Rep::Dec,
            ));
        }
        if (cap || self.is_wide(value)) && rep == Rep::Int {
            match op {
                MirUnOp::Neg => return Ok((self.wide_negate(value)?, Rep::Int)),
                MirUnOp::BitNot => return Ok((self.wide_bit_not(value)?, Rep::Int)),
                _ => {}
            }
        }
        let value = if rep == Rep::Int {
            self.narrow(value)?
        } else {
            value
        };
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
            // Nothing that is not an optional is ever none.
            (MirUnOp::IsNil | MirUnOp::IsNotNil, _) => Ok((
                self.builder
                    .ins()
                    .iconst(types::I8, i64::from(op == MirUnOp::IsNotNil)),
                Rep::Bool,
            )),
            (MirUnOp::Sqrt, Rep::Float | Rep::Int) => {
                let number = if rep == Rep::Int {
                    self.builder.ins().fcvt_from_sint(types::F64, value)
                } else {
                    value
                };
                let zero = self.builder.ins().f64const(0.0);
                let valid = self
                    .builder
                    .ins()
                    .fcmp(FloatCC::GreaterThanOrEqual, number, zero);
                let one = self.builder.ins().iconst(types::I8, 1);
                let invalid = self.builder.ins().bxor(valid, one);
                self.trap_if(invalid, "sqrt of negative float")?;
                Ok((self.builder.ins().sqrt(number), Rep::Float))
            }
            (MirUnOp::Exp | MirUnOp::Log, Rep::Float | Rep::Int) => self.exp_log(op, value, rep),
            (MirUnOp::BitNot, Rep::Int) => Ok((self.builder.ins().bnot(value), Rep::Int)),
            (MirUnOp::BitNot, Rep::Bits(width)) => {
                let mask = if width == BitsWidth::B64 {
                    -1
                } else {
                    (1i64 << width.bits()) - 1
                };
                let mask = self.builder.ins().iconst(types::I64, mask);
                Ok((self.builder.ins().bxor(value, mask), Rep::Bits(width)))
            }
            _ => Err(Decline::new(format!("unary operation {op:?} on {rep:?}"))),
        }
    }

    fn exp_log(&mut self, op: MirUnOp, value: Value, rep: Rep) -> Lowered<(Value, Rep)> {
        let number = if rep == Rep::Int {
            self.builder.ins().fcvt_from_sint(types::F64, value)
        } else {
            value
        };
        let logarithm = op == MirUnOp::Log;
        if logarithm {
            let zero = self.builder.ins().f64const(0.0);
            let positive = self.builder.ins().fcmp(FloatCC::GreaterThan, number, zero);
            let one = self.builder.ins().iconst(types::I8, 1);
            let invalid = self.builder.ins().bxor(positive, one);
            self.trap_if(invalid, "log requires positive input")?;
        }
        let code = self.builder.ins().iconst(types::I32, i64::from(logarithm));
        let result = self
            .call_runtime(&abi::MATH_F64, &[code, number])?
            .ok_or_else(|| Decline::new("math result"))?;
        let magnitude = self.builder.ins().fabs(result);
        let infinity = self.builder.ins().f64const(f64::INFINITY);
        let finite = self
            .builder
            .ins()
            .fcmp(FloatCC::LessThan, magnitude, infinity);
        let one = self.builder.ins().iconst(types::I8, 1);
        let trap = self.builder.ins().bxor(finite, one);
        self.trap_if(
            trap,
            if logarithm {
                "log produced non-finite result"
            } else {
                "exp overflow"
            },
        )?;
        Ok((result, Rep::Float))
    }

    fn binary(
        &mut self,
        op: MirBinOp,
        lhs: &MirOperand,
        rhs: &MirOperand,
        result: Rep,
        cap: bool,
        narrow: bool,
    ) -> Lowered<(Value, Rep)> {
        let (mut a, a_rep) = self.scalar_wide(lhs)?;
        let (mut b, b_rep) = self.scalar_wide(rhs)?;
        if (a_rep == Rep::Dec || b_rep == Rep::Dec || result == Rep::Dec)
            && op != MirBinOp::Coalesce
            && a_rep != Rep::Opt
            && b_rep != Rep::Opt
        {
            return self.dec_binary(op, (a, a_rep), (b, b_rep));
        }
        if a_rep == Rep::Ins && b_rep == Rep::Ins {
            return self.instant_compare(op, a, b);
        }
        if (a_rep == Rep::Inf || b_rep == Rep::Inf || result == Rep::Inf)
            && op != MirBinOp::Coalesce
            && a_rep != Rep::Opt
            && b_rep != Rep::Opt
        {
            return self.inf_binary(op, (a, a_rep), (b, b_rep));
        }
        if matches!(
            (a_rep, b_rep),
            (Rep::Int, Rep::Float) | (Rep::Float, Rep::Int)
        ) {
            return self.int_float_compare(op, (a, a_rep), (b, b_rep));
        }
        let exact = a_rep == Rep::Int && b_rep == Rep::Int;
        if exact && op != MirBinOp::Coalesce {
            return self.int_binary(op, a, b, cap);
        }
        if a_rep == Rep::Int {
            a = self.narrow(a)?;
        }
        if b_rep == Rep::Int {
            b = self.narrow(b)?;
        }
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
        if matches!(
            (a_rep, b_rep),
            (Rep::Record, Rep::Record) | (Rep::List, Rep::List) | (Rep::Tuple, Rep::Tuple)
        ) && matches!(op, MirBinOp::Eq | MirBinOp::NotEq)
        {
            return self.structural_equal(op, a, b, a_rep);
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
        // A letter and a text compare as texts.
        let (a_rep, b_rep) = match (a_rep, b_rep) {
            (Rep::Char, Rep::Text | Rep::Char) | (Rep::Text, Rep::Char) => (Rep::Text, Rep::Text),
            other => other,
        };
        if a_rep != b_rep {
            return Err(Decline::new("mixed-representation operands"));
        }
        match a_rep {
            Rep::Int => self.int_binary(op, a, b, cap),
            Rep::Float => self.float_binary(op, a, b, narrow),
            Rep::Bool => self.bool_binary(op, a, b),
            Rep::Text => self.text_binary(op, a, b),
            Rep::Char
            | Rep::Regex
            | Rep::Match
            | Rep::Dec
            | Rep::Inf
            | Rep::Ins
            | Rep::Bits(_)
            | Rep::List
            | Rep::Record
            | Rep::Opt
            | Rep::Map
            | Rep::Set
            | Rep::Func
            | Rep::Tuple
            | Rep::Unit => Err(Decline::new(format!(
                "operands of {a_rep:?} representation"
            ))),
        }
    }

    fn instant_compare(&mut self, op: MirBinOp, a: Value, b: Value) -> Lowered<(Value, Rep)> {
        let cc = match op {
            MirBinOp::Eq => IntCC::Equal,
            MirBinOp::NotEq => IntCC::NotEqual,
            MirBinOp::Lt => IntCC::SignedLessThan,
            MirBinOp::Gt => IntCC::SignedGreaterThan,
            MirBinOp::LtEq => IntCC::SignedLessThanOrEqual,
            MirBinOp::GtEq => IntCC::SignedGreaterThanOrEqual,
            _ => return Err(Decline::new("instant operation")),
        };
        let order = self
            .call_runtime(&abi::INS_COMPARE, &[a, b])?
            .ok_or_else(|| Decline::new("instant comparison result"))?;
        let zero = self.builder.ins().iconst(types::I32, 0);
        Ok((self.builder.ins().icmp(cc, order, zero), Rep::Bool))
    }

    fn structural_equal(
        &mut self,
        op: MirBinOp,
        a: Value,
        b: Value,
        a_rep: Rep,
    ) -> Lowered<(Value, Rep)> {
        let symbol = if a_rep == Rep::Record {
            &abi::RECORD_EQ
        } else {
            &abi::LIST_EQ
        };
        let same = self
            .call_runtime(symbol, &[a, b])?
            .ok_or_else(|| Decline::new("structural equality result"))?;
        if op == MirBinOp::Eq {
            return Ok((same, Rep::Bool));
        }
        let one = self.builder.ins().iconst(types::I8, 1);
        Ok((self.builder.ins().bxor(same, one), Rep::Bool))
    }

    fn int_binary(&mut self, op: MirBinOp, a: Value, b: Value, cap: bool) -> Lowered<(Value, Rep)> {
        // A wide operand makes the operation exact whatever its result type
        // says (`(max + 1) % 1000` is typed by its divisor).
        if wide::arithmetic(op) && (cap || self.is_wide(a) || self.is_wide(b)) {
            return Ok((self.wide_arithmetic(op, a, b)?, Rep::Int));
        }
        let compare = |this: &mut Self, cc: IntCC| (this.wide_compare(cc, a, b), Rep::Bool);
        Ok(match op {
            MirBinOp::Add => (self.checked_add(a, b)?, Rep::Int),
            MirBinOp::Sub => (self.checked_sub(a, b)?, Rep::Int),
            MirBinOp::Mul => (self.checked_mul(a, b)?, Rep::Int),
            MirBinOp::Div => (self.floor_div(a, b, false)?, Rep::Int),
            MirBinOp::Mod => (self.floor_div(a, b, true)?, Rep::Int),
            MirBinOp::BitAnd => (self.builder.ins().band(a, b), Rep::Int),
            MirBinOp::BitOr => (self.builder.ins().bor(a, b), Rep::Int),
            MirBinOp::BitXor => (self.builder.ins().bxor(a, b), Rep::Int),
            MirBinOp::Shl => (self.int_shift_left(a, b)?, Rep::Int),
            MirBinOp::Shr => (self.int_shift_right(a, b)?, Rep::Int),
            MirBinOp::Eq => compare(self, IntCC::Equal),
            MirBinOp::NotEq => compare(self, IntCC::NotEqual),
            MirBinOp::Lt => compare(self, IntCC::SignedLessThan),
            MirBinOp::Gt => compare(self, IntCC::SignedGreaterThan),
            MirBinOp::LtEq => compare(self, IntCC::SignedLessThanOrEqual),
            MirBinOp::GtEq => compare(self, IntCC::SignedGreaterThanOrEqual),
            _ => return Err(Decline::new("integer operation")),
        })
    }

    /// `x ⇐ n` on a number is `x · 2ⁿ`: exact, so an overflow of the 64-bit
    /// carrier traps like any other, and a negative count is an error.
    fn int_shift_left(&mut self, a: Value, count: Value) -> Lowered<Value> {
        let zero = self.builder.ins().iconst(types::I64, 0);
        let negative = self.builder.ins().icmp(IntCC::SignedLessThan, count, zero);
        self.trap_if(negative, "negative shift count")?;
        let limit = self.builder.ins().iconst(types::I64, 63);
        let large = self
            .builder
            .ins()
            .icmp(IntCC::UnsignedGreaterThanOrEqual, count, limit);
        let one = self.builder.ins().iconst(types::I64, 1);
        let moved = self.builder.ins().ishl(one, count);
        let power = self.builder.ins().select(large, zero, moved);
        let (product, overflow) = self.builder.ins().smul_overflow(a, power);
        let nonzero = self.builder.ins().icmp(IntCC::NotEqual, a, zero);
        let too_far = self.builder.ins().band(large, nonzero);
        let failed = self.builder.ins().bor(overflow, too_far);
        self.overflow_guard(failed)?;
        Ok(self.builder.ins().select(large, zero, product))
    }

    /// `x ⇒ n` on a number is `⌊x / 2ⁿ⌋`.
    fn int_shift_right(&mut self, a: Value, count: Value) -> Lowered<Value> {
        let zero = self.builder.ins().iconst(types::I64, 0);
        let negative = self.builder.ins().icmp(IntCC::SignedLessThan, count, zero);
        self.trap_if(negative, "negative shift count")?;
        let limit = self.builder.ins().iconst(types::I64, 63);
        let clamped = self.builder.ins().umin(count, limit);
        Ok(self.builder.ins().sshr(a, clamped))
    }

    /// Float arithmetic at a declared width narrower than `f64` rounds its
    /// operands and its result through `f32`, as the runner's `float_binop`
    /// does; the operation itself still runs in `f64`, and comparisons take
    /// the operands exactly as stored.
    fn float_binary(
        &mut self,
        op: MirBinOp,
        a: Value,
        b: Value,
        narrow: bool,
    ) -> Lowered<(Value, Rep)> {
        if narrow
            && matches!(
                op,
                MirBinOp::Add | MirBinOp::Sub | MirBinOp::Mul | MirBinOp::Div | MirBinOp::Mod
            )
        {
            let a = self.round_f32(a);
            let b = self.round_f32(b);
            let computed = match op {
                MirBinOp::Add => self.builder.ins().fadd(a, b),
                MirBinOp::Sub => self.builder.ins().fsub(a, b),
                MirBinOp::Mul => self.builder.ins().fmul(a, b),
                MirBinOp::Div => self.builder.ins().fdiv(a, b),
                _ => self.float_floor_rem(a, b)?,
            };
            return Ok((self.round_f32(computed), Rep::Float));
        }
        let compare = |this: &mut Self, cc: FloatCC| (this.builder.ins().fcmp(cc, a, b), Rep::Bool);
        Ok(match op {
            MirBinOp::Add => (self.builder.ins().fadd(a, b), Rep::Float),
            MirBinOp::Sub => (self.builder.ins().fsub(a, b), Rep::Float),
            MirBinOp::Mul => (self.builder.ins().fmul(a, b), Rep::Float),
            MirBinOp::Div => (self.builder.ins().fdiv(a, b), Rep::Float),
            MirBinOp::Mod => (self.float_floor_rem(a, b)?, Rep::Float),
            MirBinOp::Eq => compare(self, FloatCC::Equal),
            MirBinOp::NotEq => compare(self, FloatCC::NotEqual),
            MirBinOp::Lt => compare(self, FloatCC::LessThan),
            MirBinOp::Gt => compare(self, FloatCC::GreaterThan),
            MirBinOp::LtEq => compare(self, FloatCC::LessThanOrEqual),
            MirBinOp::GtEq => compare(self, FloatCC::GreaterThanOrEqual),
            _ => return Err(Decline::new(format!("float operation {op:?}"))),
        })
    }

    /// Float `%` takes the sign of the divisor: `fmod`, then the divisor once
    /// when the signs disagree; an exact multiple is a zero with the divisor's
    /// sign.
    fn float_floor_rem(&mut self, a: Value, b: Value) -> Lowered<Value> {
        let rest = self
            .call_runtime(&abi::FLOAT_REM, &[a, b])?
            .ok_or_else(|| Decline::new("float remainder result"))?;
        let zero = self.builder.ins().f64const(0.0);
        let exact = self.builder.ins().fcmp(FloatCC::Equal, rest, zero);
        let signed_zero = self.builder.ins().fcopysign(zero, b);
        let rest_negative = self.builder.ins().fcmp(FloatCC::LessThan, rest, zero);
        let divisor_negative = self.builder.ins().fcmp(FloatCC::LessThan, b, zero);
        let differ = self.builder.ins().bxor(rest_negative, divisor_negative);
        let shifted = self.builder.ins().fadd(rest, b);
        let adjusted = self.builder.ins().select(differ, shifted, rest);
        Ok(self.builder.ins().select(exact, signed_zero, adjusted))
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
        self.trap_if(flag, trap_text::NUMERUS_OVERFLOW)
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
            self.trap_if(guarded, trap_text::NUMERUS_OVERFLOW)?;
            negated
        };
        Ok(if remainder {
            self.builder.ins().select(is_minus_one, zero, floored_rest)
        } else {
            self.builder.ins().select(is_minus_one, negated, floored)
        })
    }

    // TERMINATORS

    /// Store each by-address `mut` parameter's final value through its address.
    fn write_back_pointer_params(&mut self) -> Lowered<()> {
        for (slot, address, _) in self.pointer_params.clone() {
            let (variable, _) = self.slot(slot)?;
            let value = self.builder.use_var(variable);
            let address = self.builder.use_var(address);
            self.builder
                .ins()
                .store(MemFlagsData::new(), value, address, 0);
        }
        Ok(())
    }

    /// Which parameters of function `id` are passed by address.
    fn address_flags(&self, id: MirFunctionId) -> Vec<bool> {
        let table = self.state.input.types;
        self.state
            .input
            .program
            .functions
            .iter()
            .find(|function| function.id == id)
            .map(|function| {
                (0..function.params.len())
                    .map(|index| pointer_param(table, function, index))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Which parameters of a function type are passed by address.
    fn type_address_flags(&self, signature: &radix_types::FuncSig) -> Vec<bool> {
        let table = self.state.input.types;
        signature
            .params
            .iter()
            .map(|param| match table.get(param.ty) {
                Type::Ref(SemanticMutability::Mutable, inner) => {
                    pointer_rep(table, MirType::semantic(*inner))
                }
                _ => false,
            })
            .collect()
    }

    /// The argument values of a call to `target`, a `mut` parameter the
    /// callee rebinds going through a stack slot that is read back after.
    fn call_arguments(
        &mut self,
        by_address: &[bool],
        args: &[MirOperand],
        param_reps: &[Rep],
        values: &mut Vec<Value>,
    ) -> Lowered<Vec<(StackSlot, usize, Rep)>> {
        let mut returns = Vec::new();
        for (position, arg) in args.iter().enumerate() {
            let (value, have) = self.scalar(arg)?;
            let value = match param_reps.get(position) {
                Some(want) => self.coerce(value, have, *want)?,
                None => value,
            };
            if by_address.get(position).copied().unwrap_or(false) {
                let rep = param_reps
                    .get(position)
                    .copied()
                    .ok_or_else(|| Decline::new("reference parameter"))?;
                // The place passed receives the callee's final value, whole
                // variable or part of one (a field, an element).
                let writes_back = matches!(arg, MirOperand::Place(_) | MirOperand::Temp(_));
                let slot = self.builder.create_sized_stack_slot(StackSlotData::new(
                    StackSlotKind::ExplicitSlot,
                    8,
                    3,
                ));
                let pointer = self.state.pointer;
                let address = self.builder.ins().stack_addr(pointer, slot, 0);
                self.builder
                    .ins()
                    .store(MemFlagsData::new(), value, address, 0);
                values.push(address);
                if writes_back {
                    returns.push((slot, position, rep));
                }
            } else {
                values.push(value);
            }
        }
        Ok(returns)
    }

    /// Read back the by-address arguments after the call.
    fn read_back_arguments(
        &mut self,
        args: &[MirOperand],
        returns: &[(StackSlot, usize, Rep)],
    ) -> Lowered<()> {
        for (slot, position, rep) in returns {
            let cl = rep
                .cl_type()
                .ok_or_else(|| Decline::new("unit parameter"))?;
            let pointer = self.state.pointer;
            let address = self.builder.ins().stack_addr(pointer, *slot, 0);
            let value = self.builder.ins().load(cl, MemFlagsData::new(), address, 0);
            match &args[*position] {
                MirOperand::Place(place) if place.projections.is_empty() => {
                    let (variable, _) = self.place_slot(place)?;
                    let wide = self.wide_slots.contains(&Self::base_slot(place));
                    let value = self.fit_int(value, wide && *rep == Rep::Int)?;
                    self.builder.def_var(variable, value);
                }
                MirOperand::Place(place) => self.write_projected(place, value, *rep)?,
                MirOperand::Temp(id) => {
                    let (variable, _) = self.slot(Slot::Temp(*id))?;
                    let wide = self.wide_slots.contains(&Slot::Temp(*id));
                    let value = self.fit_int(value, wide && *rep == Rep::Int)?;
                    self.builder.def_var(variable, value);
                }
                _ => return Err(Decline::new("reference argument")),
            }
        }
        Ok(())
    }

    fn terminator(&mut self, terminator: &MirTerminator) -> Lowered<()> {
        match &terminator.kind {
            MirTerminatorKind::Return(_) if self.generator => {
                self.builder.ins().return_(&[]);
                Ok(())
            }
            MirTerminatorKind::Return(value) => {
                self.write_back_pointer_params()?;
                let mut results = Vec::new();
                if self.error_rep.is_some() {
                    results.push(self.builder.ins().iconst(types::I8, 0));
                }
                if let Some(operand) = value
                    && let (Some(value), have) = self.operand_narrow(operand)?
                {
                    let value = self.coerce(value, have, self.ret_rep)?;
                    // The return slot is a store site: a float narrower than
                    // `f64` returns rounded through `f32`.
                    let value = if self.ret_narrow {
                        self.round_f32(value)
                    } else {
                        value
                    };
                    results.push(value);
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
                self.write_back_pointer_params()?;
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
            } => self.switch(value, cases, *default),
            MirTerminatorKind::Unreachable => {
                self.builder.ins().trap(TrapCode::unwrap_user(2));
                Ok(())
            }
        }
    }

    fn switch(
        &mut self,
        value: &MirOperand,
        cases: &[MirSwitchCase],
        default: MirBlockId,
    ) -> Lowered<()> {
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
        let default = self.block(default)?;
        self.builder.ins().jump(default, &[]);
        Ok(())
    }

    fn block(&self, id: MirBlockId) -> Lowered<Block> {
        self.blocks
            .get(&id)
            .copied()
            .ok_or_else(|| Decline::new("unknown block"))
    }
}

#[cfg(test)]
#[path = "lower_test.rs"]
mod lower_test;
