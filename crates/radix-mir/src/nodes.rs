//! Core MIR node model.
//!
//! This file defines the representation shared by MIR lowering, validation,
//! dumps, visitors, and backend probes. The model is intentionally plain data:
//! MIR producers assign stable IDs and preserve storage order, while downstream
//! passes derive maps or control-flow views from those vectors when needed.
//!
//! INVARIANTS
//! ==========
//! - IDs are local to their MIR namespace and are not positional aliases, even
//!   when current lowerers allocate them monotonically.
//! - `MirType` wraps semantic type-table IDs; optional layout IDs are a later
//!   lowering hook and may be absent.
//! - Places describe assignable storage, operands describe readable inputs, and
//!   values describe typed computations whose result may be referenced by ID
//!   after the defining statement in the same block.
//! - Terminators own CFG edges. Statements may call or construct values, but
//!   only terminators decide where control moves next.

use crate::{MirAtomicOp, MirCollectionOp, MirKernelBuiltin, MirKernelShaderStage};
use handroll::FxHashMap;
use radix_hir::DefId;
use radix_lexer::{Span, Symbol};
use radix_types::{IndexId, TypeId};
use serde::{Deserialize, Serialize};

/// Stable identifier for a MIR function within one `MirProgram`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct MirFunctionId(pub u32);

/// The `u32` function-id space is exhausted; [`MirProgram::allocate_function_id`]
/// cannot mint another id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MirFunctionIdOverflow;

/// Parent-relative identity for a synthetic kernel closure.
///
/// The identity is independent of the source-less MIR function's position in
/// the final program vector, so later composition can retain the closure's
/// origin without inferring it from a name or shader stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MirSyntheticFunctionIdentity {
    pub parent: MirFunctionId,
    pub ordinal: u32,
}

/// Origin fact carried by explicit device-function metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MirFunctionOrigin {
    Source,
    SyntheticKernelClosure,
}

/// Visibility fact for a lowered device function.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MirFunctionVisibility {
    Public,
    Private,
}

/// Stable identifier for a basic block within one MIR function.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct MirBlockId(pub u32);

/// Stable identifier for a named or synthetic local storage slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct MirLocalId(pub u32);

/// Stable identifier for compiler-created temporary storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct MirTempId(pub u32);

/// Stable identifier for a typed value computed by a statement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct MirValueId(pub u32);

/// Product/module identity for a callable whose body is not in the current MIR
/// program but whose signature is known to validation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MirExternalFunctionIdentity {
    pub product: String,
    pub module_path: Vec<String>,
    pub name: String,
}

impl MirExternalFunctionIdentity {
    pub fn new(
        product: impl Into<String>,
        module_path: Vec<String>,
        name: impl Into<String>,
    ) -> Self {
        Self {
            product: product.into(),
            module_path,
            name: name.into(),
        }
    }
}

/// Backend-layout handle reserved for later physical representation lowering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct MirLayoutId(pub u32);

/// Concrete identity for one generic source callable instantiation.
///
/// `shape_args` holds the bound `magnitudo` shape values in declaration order
/// (parallel to `type_args` for `typus` parameters), so distinct shapes yield
/// distinct instances and one shape is never confused with another.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MirMonomorphizationKey {
    pub source: DefId,
    pub type_args: Vec<MirType>,
    pub shape_args: Vec<IndexId>,
}

/// MIR-owned link from a generic source callable to a concrete MIR function.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MirMonomorphization {
    pub key: MirMonomorphizationKey,
    pub function: MirFunctionId,
}

/// Stable identifier for a captured-closure environment in one lowered unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct MirClosureEnvironmentId(pub u32);

/// MIR-owned layout fact for one captured closure environment.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MirClosureEnvironment {
    pub id: MirClosureEnvironmentId,
    pub function: MirFunctionId,
    pub value_ty: MirType,
    pub environment_local: MirLocalId,
    pub captures: Vec<MirClosureCapture>,
}

/// One captured binding stored in a closure environment aggregate.
///
/// An unmarked closure shares its enclosing scope (operator ruling
/// 2026-09-24): a captured name IS the enclosing binding. `ty` is always the
/// binding's value type. `mode` says how the environment slot carries it:
///
/// - [`MirCaptureMode::Value`] — the binding is never rebound after its
///   declaration, so a copy at capture is observably identical to sharing it;
///   `source_place` reads the value.
/// - [`MirCaptureMode::Shared`] — the binding is rebound somewhere (outside
///   or inside a closure). Its storage is a shared cell
///   ([`MirAggregateKind::SharedCell`]); `source_place` names the cell itself
///   and the environment slot holds that same cell. Reads and writes through
///   the capture project [`MirProjection::SharedCell`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MirClosureCapture {
    pub source: DefId,
    pub name: Symbol,
    pub ty: MirType,
    pub source_place: MirPlace,
    pub mode: MirCaptureMode,
}

/// How a closure environment slot carries one captured binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MirCaptureMode {
    /// Copy of a binding that is never rebound after declaration.
    Value,
    /// The binding's shared cell; the closure and its enclosing scope alias
    /// one storage location.
    Shared,
}

/// Runtime closure value pairing a synthetic function with its environment.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MirClosureValue {
    pub function: MirFunctionId,
    pub environment: MirOperand,
    pub environment_id: MirClosureEnvironmentId,
}

/// Same-program captured closure call target.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MirClosureCallee {
    pub function: MirFunctionId,
    pub environment: MirOperand,
    pub environment_id: MirClosureEnvironmentId,
}

/// MIR type reference: semantic identity plus optional backend layout identity.
///
/// MIR validation and current probes reason through `semantic`; `layout` exists
/// so later ABI or storage lowering can attach target-specific representation
/// without rewriting the semantic type graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MirType {
    semantic: TypeId,
    layout: Option<MirLayoutId>,
}

impl MirType {
    #[must_use]
    pub fn semantic(semantic: TypeId) -> Self {
        Self {
            semantic,
            layout: None,
        }
    }

    #[must_use]
    pub fn with_layout(semantic: TypeId, layout: MirLayoutId) -> Self {
        Self {
            semantic,
            layout: Some(layout),
        }
    }

    #[must_use]
    pub fn semantic_id(self) -> TypeId {
        self.semantic
    }

    #[must_use]
    pub fn layout_id(self) -> Option<MirLayoutId> {
        self.layout
    }
}

/// Function-id to vector-position index over one function set.
///
/// This is the crate's single function-id resolution mechanism. Producers may
/// append or reorder functions after allocating their stable IDs, so
/// consumers that perform repeated lookups build this map once and resolve
/// through [`Self::function`] / [`Self::function_mut`] (or the
/// [`MirProgram`] wrappers [`MirProgram::indexed_function`] /
/// [`MirProgram::indexed_function_mut`]). Producers that append functions
/// record each append with [`Self::insert`].
#[derive(Debug, Clone, Default)]
pub struct MirFunctionIndex {
    positions: FxHashMap<MirFunctionId, usize>,
}

impl MirFunctionIndex {
    /// Build from any function set (a program's, or an in-flight lowering
    /// vec that has no `MirProgram` yet).
    #[must_use]
    pub fn for_functions(functions: &[MirFunction]) -> Self {
        let mut positions = FxHashMap::default();
        for (position, function) in functions.iter().enumerate() {
            positions.entry(function.id).or_insert(position);
        }
        Self { positions }
    }

    /// Record one appended function's position. The first position for an id
    /// wins, matching [`Self::for_functions`].
    pub fn insert(&mut self, id: MirFunctionId, position: usize) {
        self.positions.entry(id).or_insert(position);
    }

    #[must_use]
    pub fn position(&self, id: MirFunctionId) -> Option<usize> {
        self.positions.get(&id).copied()
    }

    #[must_use]
    pub fn get(&self, id: &MirFunctionId) -> Option<&usize> {
        self.positions.get(id)
    }

    /// Resolve an id against the function set this index was built for.
    #[must_use]
    pub fn function<'a>(
        &self,
        functions: &'a [MirFunction],
        id: MirFunctionId,
    ) -> Option<&'a MirFunction> {
        let position = self.position(id)?;
        functions.get(position).filter(|function| function.id == id)
    }

    /// Mutable variant of [`Self::function`].
    #[must_use]
    pub fn function_mut<'a>(
        &self,
        functions: &'a mut [MirFunction],
        id: MirFunctionId,
    ) -> Option<&'a mut MirFunction> {
        let position = self.position(id)?;
        functions
            .get_mut(position)
            .filter(|function| function.id == id)
    }
}

/// Complete MIR unit emitted for one analyzed source/package entry.
///
/// Function order is stable output order for dumps and probes. Do not treat the
/// vector index as the function ID; callers should build explicit maps when
/// they need ID lookup.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct MirProgram {
    pub functions: Vec<MirFunction>,
}

impl MirProgram {
    #[must_use]
    pub fn new() -> Self {
        Self {
            functions: Vec::new(),
        }
    }

    #[must_use]
    pub fn function_index(&self) -> MirFunctionIndex {
        MirFunctionIndex::for_functions(&self.functions)
    }

    /// O(1) lookup through a caller-owned index built for this program.
    #[must_use]
    pub fn indexed_function(
        &self,
        index: &MirFunctionIndex,
        id: MirFunctionId,
    ) -> Option<&MirFunction> {
        index.function(&self.functions, id)
    }

    /// Mutable lookup through a caller-owned index built for this program.
    #[must_use]
    pub fn indexed_function_mut(
        &mut self,
        index: &MirFunctionIndex,
        id: MirFunctionId,
    ) -> Option<&mut MirFunction> {
        index.function_mut(&mut self.functions, id)
    }

    /// Derive the next unused function id from a function set: `max + 1`,
    /// or `0` when the set is empty. This is the workspace's only
    /// function-id allocator; the checked overflow policy cannot be bypassed
    /// by a copy.
    ///
    /// Named debt: every call scans the set, so allocation is O(N), and the
    /// merge/specialization loops allocate inside a loop. Swap trigger: seed
    /// a counter inside this function alone (never a `MirProgram` counter
    /// field — public fields, serde, and hand-pushed ids make that worse)
    /// when profiling implicates merge/monomorphize, or a package with ≥10³
    /// linked functions exists.
    ///
    /// # Errors
    /// Returns [`MirFunctionIdOverflow`] when no function id remains.
    pub fn allocate_function_id(
        functions: &[MirFunction],
    ) -> Result<MirFunctionId, MirFunctionIdOverflow> {
        functions.iter().map(|function| function.id.0).max().map_or(
            Ok(MirFunctionId(0)),
            |maximum| {
                maximum
                    .checked_add(1)
                    .map(MirFunctionId)
                    .ok_or(MirFunctionIdOverflow)
            },
        )
    }
}

/// Execution-shaped body for one lowered function.
///
/// Parameters are modeled as locals so calls, assignments, and projections can
/// use one place model. `blocks` are ordered for deterministic rendering; CFG
/// reachability is expressed by terminator target IDs rather than by adjacency.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MirFunction {
    pub id: MirFunctionId,
    pub source: Option<DefId>,
    pub name: Option<Symbol>,
    pub params: Vec<MirParam>,
    pub locals: Vec<MirLocal>,
    pub temps: Vec<MirTemp>,
    pub blocks: Vec<MirBlock>,
    pub return_ty: MirType,
    pub error_ty: Option<MirType>,
    /// Source async posture, including legacy `@ futura`.
    pub is_async: bool,
    /// Source stream posture, including legacy `@ cursor`.
    pub is_generator: bool,
    /// Shader stage assigned from `@ kernel`/`@ vertex`/`@ fragment` during
    /// HIR→MIR lowering. Ordinary synthetic helpers stay `None`; synthetic
    /// kernel closures carry `Compute` explicitly for the device boundary.
    pub shader_stage: Option<MirKernelShaderStage>,
    pub span: Span,
}

impl MirFunction {
    /// O(1) lookup when `id.0` matches the dense vector index (the common
    /// lowering allocation order). Falls back to a linear scan when IDs are
    /// sparse or reordered.
    #[must_use]
    pub fn block_by_id(&self, id: MirBlockId) -> Option<&MirBlock> {
        if let Some(block) = self.blocks.get(id.0 as usize)
            && block.id == id
        {
            return Some(block);
        }
        self.blocks.iter().find(|block| block.id == id)
    }

    /// Type of the compiler-created temporary with the given ID, when present.
    #[must_use]
    pub fn temp_ty(&self, id: MirTempId) -> Option<MirType> {
        self.temps
            .iter()
            .find(|temp| temp.id == id)
            .map(|temp| temp.ty)
    }
}

/// Shallow ownership encoding for MIR parameter modes.
///
/// The serde labels preserve the pre-D-14 wire spellings while the Rust
/// variants mirror [`radix_types::SemanticParamMode`] plus compiler-owned lane
/// binding markers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MirParamMode {
    #[serde(rename = "Owned")]
    Owned,
    #[serde(rename = "De")]
    Ref,
    #[serde(rename = "In")]
    MutRef,
    #[serde(rename = "Ex")]
    Move,
    /// Compiler-injected `@ nucleum lane` position parameter.
    #[serde(rename = "Lane")]
    Lane,
    /// Fragment-local lane parameter inherited from its composition caller.
    #[serde(rename = "LaneInherited")]
    LaneInherited,
}

/// Function parameter bound to the local slot that carries its runtime value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MirParam {
    pub local: MirLocalId,
    pub name: Option<Symbol>,
    pub ty: MirType,
    pub mode: MirParamMode,
    pub span: Span,
}

/// Addressable function-local storage.
///
/// Locals include source bindings and synthetic slots that need stable storage
/// identity across statements or blocks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MirLocal {
    pub id: MirLocalId,
    pub name: Option<Symbol>,
    pub ty: MirType,
    pub mutable: bool,
    pub span: Span,
}

/// Compiler-created temporary storage with a known semantic type.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MirTemp {
    pub id: MirTempId,
    pub ty: MirType,
    pub span: Span,
}

/// Explicit shader-I/O role for a MIR local.
///
/// Carried on stage-entry locals so emitters do not infer input/output from
/// mutability or name spelling.  Stage-input locals are read-only (vertex
/// attributes, fragment varyings); stage-output locals are writable (vertex
/// varying outputs, `out_position`, fragment `out_color`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MirShaderIoRole {
    /// Read-only stage input: vertex attribute or fragment varying.
    StageInput,
    /// Writable stage output: vertex varying/builtin output or fragment color.
    StageOutput,
}

/// Basic block: straight-line statements followed by exactly one terminator.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MirBlock {
    pub id: MirBlockId,
    pub statements: Vec<MirStatement>,
    pub terminator: MirTerminator,
    pub span: Span,
}

/// Side-effecting or value-defining operation that does not choose CFG edges.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MirStatement {
    pub kind: MirStatementKind,
    pub span: Span,
}

/// Statement forms that execute within the current block.
///
/// Calls and runtime calls may write to a destination, while failable calls are
/// terminators because success and alternate-exit paths split control flow.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MirStatementKind {
    Assign {
        place: MirPlace,
        value: MirValue,
    },
    Call {
        destination: Option<MirPlace>,
        callee: MirCallee,
        args: Vec<MirOperand>,
    },
    RuntimeCall {
        destination: Option<MirPlace>,
        call: MirRuntimeCall,
    },
    Construct {
        destination: MirPlace,
        aggregate: MirAggregate,
    },
}

/// Block-final control-flow operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MirTerminator {
    pub kind: MirTerminatorKind,
    pub span: Span,
}

/// Terminator taxonomy for all CFG exits from a block.
///
/// `TryCall` is a terminator, not a statement, because the callee's success and
/// error continuations must be explicit in the CFG. `ReturnError` is separate
/// from `Return` so alternate-exit typing remains visible after HIR lowering.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MirTerminatorKind {
    Return(Option<MirOperand>),
    /// End the current thread lane, not the containing function.
    ReturnThread,
    ReturnError(MirOperand),
    TryCall {
        destination: Option<MirPlace>,
        callee: MirCallee,
        args: Vec<MirOperand>,
        ok_block: MirBlockId,
        error_place: MirPlace,
        error_block: MirBlockId,
    },
    Goto(MirBlockId),
    Branch {
        condition: MirOperand,
        then_block: MirBlockId,
        else_block: MirBlockId,
    },
    Switch {
        value: MirOperand,
        cases: Vec<MirSwitchCase>,
        default: MirBlockId,
    },
    Unreachable,
}

/// One constant-dispatch edge for `Switch`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MirSwitchCase {
    pub value: MirConstant,
    pub target: MirBlockId,
}

/// Typed computation result defined by a statement.
///
/// A `MirValueId` is valid only after its defining statement has been processed
/// in the same block. Validation intentionally enforces that local ordering so
/// MIR cannot smuggle unsequenced data dependencies into later phases.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MirValue {
    pub id: MirValueId,
    pub kind: MirValueKind,
    pub ty: MirType,
    pub span: Span,
}

/// Computation shapes that produce a `MirValue`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MirValueKind {
    Operand(MirOperand),
    Closure(MirClosureValue),
    Unary {
        op: MirUnOp,
        operand: MirOperand,
    },
    Binary {
        op: MirBinOp,
        lhs: MirOperand,
        rhs: MirOperand,
    },
    Option(MirOptionOp),
    /// Runtime extent of one axis of a tensor (`numerus`). Scalar and
    /// device-legal: a kernel reads a runtime-shaped tensor's extent through
    /// this value (never through a list). Emitters map it to a per-axis
    /// scalar launch argument.
    TensorExtent {
        tensor: MirOperand,
        axis: u32,
    },
}

/// Readable input to a computation, call, projection, or terminator.
///
/// Operands are intentionally smaller than expressions. Complex computation is
/// made explicit as statement-defined `MirValue`s so later passes can inspect
/// evaluation order without recovering it from nested syntax.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MirOperand {
    Place(MirPlace),
    Temp(MirTempId),
    Value(MirValueId),
    Constant(MirConstant),
}

/// Assignable or projectable storage location.
///
/// A place starts from a local or temp and then applies field, variant-field, or
/// index projections. Validation owns the type walk through those projections.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MirPlace {
    pub base: MirPlaceBase,
    pub projections: Vec<MirProjection>,
}

impl MirPlace {
    #[must_use]
    pub fn local(id: MirLocalId) -> Self {
        Self {
            base: MirPlaceBase::Local(id),
            projections: Vec::new(),
        }
    }

    #[must_use]
    pub fn temp(id: MirTempId) -> Self {
        Self {
            base: MirPlaceBase::Temp(id),
            projections: Vec::new(),
        }
    }
}

impl MirTerminatorKind {
    /// Stable lowercase name for diagnostics and probe error messages.
    #[must_use]
    pub fn kind_name(&self) -> &'static str {
        match self {
            Self::Return(_) => "return",
            Self::ReturnThread => "return_thread",
            Self::ReturnError(_) => "return_error",
            Self::TryCall { .. } => "try_call",
            Self::Goto(_) => "goto",
            Self::Branch { .. } => "branch",
            Self::Switch { .. } => "switch",
            Self::Unreachable => "unreachable",
        }
    }
}

impl MirAggregateKind {
    /// `DefId` for `Struct`/`EnumVariant` aggregates; `None` for every other
    /// aggregate kind.
    #[must_use]
    pub fn definition_id(&self) -> Option<DefId> {
        match self {
            Self::Struct(def_id) | Self::EnumVariant(def_id) => Some(*def_id),
            Self::Tuple
            | Self::Array
            | Self::Record
            | Self::ClosureEnvironment(_)
            | Self::SharedCell
            | Self::Map
            | Self::Set
            | Self::Tensor
            | Self::SparseTensor
            | Self::Interval { .. }
            | Self::Vector
            | Self::Matrix => None,
        }
    }
}

impl MirAggregateFields {
    /// Flattened operand list for an aggregate construction, regardless of field shape.
    /// For `Keyed`, both key and value operands are included in storage order.
    #[must_use]
    pub fn operands(&self) -> Vec<&MirOperand> {
        match self {
            Self::Ordered(items) => items
                .iter()
                .map(|item| match item {
                    MirAggregateItem::Operand(operand) | MirAggregateItem::Spread(operand) => {
                        operand
                    }
                })
                .collect(),
            Self::Named(items) => items.iter().map(|item| &item.value).collect(),
            Self::Keyed(items) => items
                .iter()
                .flat_map(|item| [&item.key, &item.value])
                .collect(),
        }
    }

    /// Whether the fields contain at least one spread operand. Only `Ordered` fields can spread.
    #[must_use]
    pub fn has_spread(&self) -> bool {
        match self {
            Self::Ordered(items) => items
                .iter()
                .any(|item| matches!(item, MirAggregateItem::Spread(_))),
            Self::Named(_) | Self::Keyed(_) => false,
        }
    }
}

/// Root storage namespace for a `MirPlace`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MirPlaceBase {
    Local(MirLocalId),
    Temp(MirTempId),
}

/// Projection from a place base into a narrower storage location.
///
/// Variant-field projections carry the variant `DefId` because field symbols
/// alone are not globally unique across enum constructors.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MirProjection {
    Field(Symbol),
    VariantField {
        variant: DefId,
        field: Symbol,
    },
    ClosureCapture {
        environment: MirClosureEnvironmentId,
        source: DefId,
    },
    /// Content of a shared cell built by [`MirAggregateKind::SharedCell`].
    /// Type-preserving: a local that holds a cell is typed with the cell's
    /// content type, and this projection reads or writes that content.
    SharedCell,
    Index(MirOperand),
    /// An [`MirProjection::Index`] whose operand the checker proved
    /// `0 <= i < bound` at the access: the live length of a
    /// list or text, or each axis extent of a tensor for its own coordinate.
    /// Same meaning and same operand as `Index`; a consumer may omit the
    /// bounds check and must never derive a proof of its own. Only checker
    /// proofs reach it; a consumer that ignores proofs treats it as `Index`.
    ProvenIndex(MirOperand),
    VectorLane(u8),
    MatrixCell {
        row: u8,
        column: u8,
    },
}

impl MirProjection {
    /// The index operand of an [`Self::Index`] or [`Self::ProvenIndex`].
    #[must_use]
    pub fn index_operand(&self) -> Option<&MirOperand> {
        match self {
            Self::Index(index) | Self::ProvenIndex(index) => Some(index),
            _ => None,
        }
    }

    /// Whether the checker proved this projection's index in range.
    #[must_use]
    pub fn is_proven_index(&self) -> bool {
        matches!(self, Self::ProvenIndex(_))
    }
}

/// Literal values embedded directly in MIR.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MirConstant {
    Int(i64),
    /// Unsigned integer literal (`numerus<u64>` magnitude; never sign-encoded).
    UInt(u64),
    Float(f64),
    String(Symbol),
    Ascii(Symbol),
    Bool(bool),
    Nil,
    Unit,
    Octeti(Vec<u8>),
    Regex {
        pattern: Symbol,
        flags: Option<Symbol>,
    },
    /// Reference to a lowered MIR function (closure or top-level callable).
    Function(MirFunctionId),
    /// Compiler-owned shape-derived scalar preserved across AIR→MIR: the
    /// element count of a tensor shape (`axis: None`) or the extent of one
    /// axis, carried symbolically when the shape contains `Param` dims.
    /// Constant-folds to `MirConstant::Float` at lowering when every dim is
    /// literal; full substitution of the concrete value at call sites is a
    /// later (Phase-4) codegen step, so validation only needs it to type as
    /// a float scalar.
    ShapeSize {
        shape: IndexId,
        axis: Option<i64>,
    },
    /// Exact integer literal of the unbounded `inf` width (inf-delivery
    /// rulings 34, 46): the canonical decimal digits (`-` sign for negatives,
    /// no leading zeros, no `+`, no separators). Appended so the positional
    /// serde index of every earlier variant is unchanged (an FMIR image reads
    /// back by position). Lowering emits it only for a value outside the
    /// `i64`/`u64` constants (a value that fits those stays `Int`/`UInt`),
    /// but every consumer must read it as an exact integer of any size.
    Magnus(String),
}

impl Eq for MirConstant {}

impl std::hash::Hash for MirConstant {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
        match self {
            Self::Int(v) => v.hash(state),
            Self::UInt(v) => v.hash(state),
            // WHY: derived `PartialEq` makes `0.0 == -0.0`, so both zeros
            // must share one hash.
            Self::Float(v) => (if *v == 0.0 { 0 } else { v.to_bits() }).hash(state),
            Self::String(v) | Self::Ascii(v) => v.hash(state),
            Self::Bool(v) => v.hash(state),
            Self::Nil | Self::Unit => {}
            Self::Octeti(v) => v.hash(state),
            Self::Regex { pattern, flags } => {
                pattern.hash(state);
                flags.hash(state);
            }
            Self::Function(v) => v.hash(state),
            Self::ShapeSize { shape, axis } => {
                shape.hash(state);
                axis.hash(state);
            }
            Self::Magnus(v) => v.hash(state),
        }
    }
}

/// Unary operation lowered into MIR.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MirUnOp {
    Neg,
    Not,
    BitNot,
    IsNil,
    IsNotNil,
    /// Floating-point square root (register vector longitudo / normalizata).
    Sqrt,
    /// Rectified linear unit: max(0, x). No domain constraints beyond finite input.
    Relu,
    /// Gaussian Error Linear Unit using the tanh approximation:
    /// 0.5*x*(1+tanh(√(2/π)*(x+0.044715*x³))). Error < 1e-6 vs exact Gelu.
    /// All finite f32 inputs are valid. No domain constraints beyond finite input.
    Gelu,
    /// Hyperbolic tangent: tanh(x). All finite f32 inputs valid.
    Tanh,
    /// Natural exponentiation: e^x. All finite f32 inputs are valid.
    /// Overflow (result non-finite) is rejected per the domain policy.
    Exp,
    /// Natural logarithm: ln(x). Domain: x > 0.
    /// Negative/zero inputs and non-finite results are rejected per the domain policy.
    Log,
    /// Half-precision round trip (GI3-1 contract amendment, task):
    /// `f16_round(x) = half_to_f32(f32_to_f16_rn(x))` — round-to-nearest-even
    /// through f16, matching the pinned comparator's K/V-cache register
    /// rounding (`kv_f16_round`). All finite f32 inputs are valid; the
    /// result is the nearest f16-representable value (with f16 subnormals).
    F16Round,
}

/// Binary operation lowered into MIR.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MirBinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Eq,
    NotEq,
    Lt,
    Gt,
    LtEq,
    GtEq,
    And,
    Or,
    Coalesce,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
}

/// Callable target reference.
///
/// Direct MIR functions are used for functions already lowered into the same
/// program. Definition IDs let validators and probes bridge back to semantic
/// signatures when the callee is known but not represented by a MIR ID.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MirCallee {
    Function(MirFunctionId),
    Definition {
        source: DefId,
        type_args: Vec<MirType>,
    },
    Closure(MirClosureCallee),
    Value(MirOperand),
}

impl MirCallee {
    #[must_use]
    pub fn definition(source: DefId) -> Self {
        Self::Definition {
            source,
            type_args: Vec::new(),
        }
    }

    #[must_use]
    pub fn definition_with_type_args(source: DefId, type_args: Vec<MirType>) -> Self {
        Self::Definition { source, type_args }
    }
}

/// Runtime intrinsic call with explicit arguments and result type.
///
/// These nodes keep compiler-known operations distinct from ordinary function
/// calls so validation and backend probes can apply operation-specific arity and
/// type policy.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MirRuntimeCall {
    pub intrinsic: MirIntrinsic,
    pub args: Vec<MirOperand>,
    pub return_ty: MirType,
}

/// Runtime extent equality checked at a typed tensor boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MirShapeRelation {
    pub left: MirShapeExpr,
    pub right: MirShapeExpr,
}

/// Runtime witness for a generic dimension inferred at a call boundary. The
/// ordinal labels the callee's generic descriptor; the value stays carried by
/// the explicit call arguments and is never part of an instance key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MirShapeBinding {
    pub parameter: u32,
    pub witness: MirShapeExpr,
}

/// A shape expression evaluated against the explicit operands of its enclosing
/// `TensorShapeCheck` call. Argument indexes are therefore independent of
/// source local IDs and work for call, entry, and return boundaries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MirShapeExpr {
    Literal(u64),
    /// Product of tensor extents for a whole-shape element count. The empty
    /// product is one, matching rank-zero tensors.
    Product(Vec<Self>),
    ScalarArgument {
        argument: u32,
    },
    TensorAxis {
        argument: u32,
        axis: u32,
    },
    TensorFieldAxis {
        argument: u32,
        fields: Vec<Symbol>,
        axis: u32,
    },
    Affine {
        base: Box<Self>,
        offset: i128,
    },
    Divide {
        numerator: Box<Self>,
        divisor: u64,
    },
}

/// Equalities checked against explicit runtime values before a tensor boundary
/// is crossed. It has no success value; a mismatch follows the caller's
/// existing recoverable-error route.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MirShapeCheck {
    pub relations: Vec<MirShapeRelation>,
    pub size_bindings: Vec<MirShapeBinding>,
}

impl MirShapeCheck {
    /// Whether the checked equalities connect these two runtime witnesses.
    /// Only `relations` are walked; `size_bindings` are not consulted, so a
    /// caller that wants a binding to mediate passes its `witness` as an
    /// endpoint.
    #[must_use]
    pub fn proves_equal(&self, left: &MirShapeExpr, right: &MirShapeExpr) -> bool {
        let mut reached = vec![left];
        let mut cursor = 0;
        while cursor < reached.len() {
            let current = reached[cursor];
            if current == right {
                return true;
            }
            for relation in &self.relations {
                let next = if &relation.left == current {
                    Some(&relation.right)
                } else if &relation.right == current {
                    Some(&relation.left)
                } else {
                    None
                };
                if let Some(next) = next
                    && !reached.contains(&next)
                {
                    reached.push(next);
                }
            }
            cursor += 1;
        }
        false
    }
}

/// Compiler-owned edge resolution selected by `tensor.limes<Policy>()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MirBuiltinEdgePolicy {
    Clamp,
    Reflect,
    Wrap,
}

/// Resolved edge-policy dispatch carried by a limes runtime call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MirEdgePolicy {
    Builtin(MirBuiltinEdgePolicy),
    Custom {
        policy_def: DefId,
        remap: MirFunctionId,
        transform: Option<MirFunctionId>,
    },
}

/// Device fault-delivery marker (U9-5): where a handled block opens, closes
/// and delivers queued kernel errors.
///
/// The markers are runtime calls with no destination and no value. Only the
/// MIR runner acts on them; every other executor treats them as no-ops. They
/// are emitted only in a unit that declares a `⇥` kernel (the only source of a
/// recoverable device fault), so a program without one never sees them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MirDeviceBlockOp {
    /// A handled block opens. One argument: the handler's error place, which
    /// receives a delivered error's text. `handler` is the `cape` block a
    /// delivery jumps to. `omnia` selects `fac omnia` (queued) over plain
    /// `fac` (a `⇥` kernel call waits at the call).
    Enter { omnia: bool, handler: MirBlockId },
    /// The innermost open block closes. `error_edge` marks the handler-entry
    /// edge (an error already chosen the handler); otherwise the edge is a
    /// fall-through, `rumpe`, `perge` or `redde` and an omnia block delivers.
    Exit { error_edge: bool },
    /// Take an error a delivery point already surfaced (no device work), or
    /// with `sync` complete the queued launches first (the `dum` back-edge).
    Deliver { sync: bool },
}

/// Compiler-runtime operations that are not ordinary user calls.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MirIntrinsic {
    /// Device fault-delivery marker; see [`MirDeviceBlockOp`].
    DeviceBlock(MirDeviceBlockOp),
    Diagnostic(MirDiagnosticKind),
    Assert,
    FormatString {
        template: Symbol,
    },
    /// `value ¶ "spec"` (D2.1): render one built-in value as `textus`.
    ///
    /// `spec` is the validated spec literal's text; executors parse it with
    /// `radix_types::format_spec`. One argument; returns `textus`.
    FormatValue {
        spec: Symbol,
    },
    Convert(MirConversion),
    Collection(MirCollectionOp),
    Atomic(MirAtomicOp),
    Panic,
    Provider(MirProvider),
    /// Open a stream `ad` conversation (Request frame + opaque handle).
    SermoOpen,
    /// Patch Request `data` from the stream opener expression.
    SermoSetOpener,
    /// Operate on a compiler-owned live sermo directional view.
    Sermo(MirSermoOp),
    ReadLine,
    /// Read all of stdin to EOF (`lege` with no line argument).
    ReadAll,
    /// Async suspend (`cede`) or generator yield (`cede` inside `@ cursor`).
    Cede,
    /// Read one compiler-owned GPU execution builtin inside a device function.
    GpuBuiltin(MirKernelBuiltin),
    /// Materialize a `@ cursor` call into a `lista<T>` for `itera ex` lowering.
    CursorStream(MirCursorStream),
    /// Gradient handle runtime operation (create, accumulate, read, zero).
    Gradient(MirGradientOp),
    /// Compiler-owned runtime variant-tag check (`est <type>` / `non est <type>`).
    ///
    /// Runner-dispatched type check on the runtime value's variant tag against
    /// a concrete valor-carried target type; never attempts extraction. The
    /// MIR runner is the first systems-lane consumer of the `est`
    /// variant-check surface (archived `docs/archived/est-variant-check/`);
    /// device/application lanes fail closed on it.
    TypeCheck(MirTypeCheckOp),
    /// Runtime equality obligations for shape terms at tensor call, function
    /// entry, or return boundaries. Other executors reject this until they
    /// implement the same recoverable check.
    TensorShapeCheck(MirShapeCheck),
    /// Evaluate one runtime shape extent from explicit runtime operands.
    TensorShapeSize(MirShapeExpr),
    /// Declared `filum` reduction. The runtime call IS the declaration site;
    /// the kernel-plan pass resolves it from the carried facts, never by
    /// pattern recognition over the body.
    ///
    /// Arguments: the source tensor operand; then, for
    /// [`MirFilumForm::Grid`] and [`MirFilumForm::Nested`], one extent
    /// operand per source axis (row-major, the `TensorExtent` temps the lane
    /// lowering assigned); then the `coalesce` identity operand when the
    /// source wrote one.
    ///
    /// [`MirFilumForm::Declared`] is the ungrouped `sum` (SFR-5): the plan
    /// pass resolves it to
    /// [`crate::kernel_plan::CollectionKernelPlan::DistributedSum`].
    /// [`MirFilumForm::Grid`] is the whole-grid reduction lane region
    /// (C4-8): the plan pass resolves it to
    /// [`crate::kernel_plan::CollectionKernelPlan::GridReduce`].
    /// [`MirFilumForm::Nested`] is the reduction inside the BODY of a lane
    /// region (gpu-reset rule 14.5); it validates like `Grid` but resolves
    /// to no plan here — later lowering owns it.
    FilumReduce(MirFilumReduceOp),
    /// Ordered whole-source prefix operation. The runtime call is the typed
    /// declaration site for both the reference interpreter and device plan.
    /// Its arguments are the source tensor, term captures and helper, then
    /// one capture/helper pair per grouped key axis.
    Scan(MirScanOp),
    /// Deep duplicate-then-own for the `copy` access marker (mutability
    /// engine D-6): the lone argument is the source value; the result is an
    /// independent deep copy of the same payload type — no container, field,
    /// or element storage is shared with the source, which stays live.
    /// Lowered at `copy` bind initialization and `copy` parameter entry.
    Duplicate,
    /// A scalar operator applied per element over tensors (gpu-reset rule 15,
    /// K4). Operands are tensors or scalars in the operator's own order (one
    /// scalar broadcasts; two tensors have identical shapes); the result is a
    /// tensor typed by `return_ty`. Each element is computed exactly as the
    /// scalar operator would and stored under the result element type. Both
    /// operands of `And`/`Or` and all three of `Select` are always evaluated.
    /// The MIR runner is the reference; every other executor fails closed.
    TensorLift(MirLiftOp),
    /// IEEE 754 pow (C99 Annex F) over two fractus operands, the correctly
    /// rounded core of fractus `potentia`/`power`. The runner evaluates the
    /// host libm pow; other lanes reject it until the target carry (corpus
    /// coverage F4, audit gap #11).
    Pow,
    /// Resolve a lazy shifted tensor edge view; the single argument is the
    /// shifted receiver and the payload selects its built-in or user policy.
    TensorLimes(MirEdgePolicy),
    /// A vector method deferred past lowering. Templates carry it with
    /// symbolic widths; one expander rewrites it into the unrolled lanes once
    /// the width is a literal. No executor or backend ever sees it: a
    /// `VectorOp` that survives to validation is rejected by name.
    VectorOp(MirVectorOp),
}

/// The scalar operator a lane-wise [`MirVectorOp`] applies per lane.
///
/// `Div` floors on integers and is IEEE on floats; `TrueDiv` (`÷`) converts
/// integer lanes to the result float first and traps an integer zero divisor;
/// `Max` and `Min` (`⤒`, `⤓`) pick the larger or smaller lane (IEEE
/// `maximum`/`minimum`). Every other operator is the plain [`MirBinOp`] of the
/// same name. The comparisons (`Lt Gt LtEq GtEq`, and `Eq`/`NotEq` for `≅`/`≇`)
/// and `And`/`Or` give a bool lane; both lanes are always evaluated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MirLaneOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    TrueDiv,
    Max,
    Min,
    Lt,
    Gt,
    LtEq,
    GtEq,
    Eq,
    NotEq,
    And,
    Or,
}

impl MirLaneOp {
    /// The plain binary operator the lane applies, when it is one.
    #[must_use]
    pub fn bin_op(self) -> Option<MirBinOp> {
        match self {
            Self::Add => Some(MirBinOp::Add),
            Self::Sub => Some(MirBinOp::Sub),
            Self::Mul => Some(MirBinOp::Mul),
            Self::Div => Some(MirBinOp::Div),
            Self::Mod => Some(MirBinOp::Mod),
            Self::Lt => Some(MirBinOp::Lt),
            Self::Gt => Some(MirBinOp::Gt),
            Self::LtEq => Some(MirBinOp::LtEq),
            Self::GtEq => Some(MirBinOp::GtEq),
            Self::Eq => Some(MirBinOp::Eq),
            Self::NotEq => Some(MirBinOp::NotEq),
            Self::And => Some(MirBinOp::And),
            Self::Or => Some(MirBinOp::Or),
            Self::TrueDiv | Self::Max | Self::Min => None,
        }
    }
}

/// The vector method a [`MirIntrinsic::VectorOp`] defers.
///
/// Arity: `Count`, `Norm`, `Normalized`, `Negate`, `Not`, `Crea` and `Swizzle` take one
/// operand (the receiver; `Crea` uses only its declared shape); `Select` takes
/// three (mask, then, else); everything else takes two (receiver, peer).
/// `Swizzle` carries resolved lane indices, never a symbol, so symbol remapping
/// needs no rewrite.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MirVectorOp {
    /// Lane-wise operator between two vectors of the same width. With
    /// `checked`, each lane is stored under the result element type with the
    /// integer store trap (an exact lane that leaves its range traps).
    Elementwise {
        op: MirLaneOp,
        checked: bool,
    },
    /// Lane-wise operator against one scalar operand (the peer is a
    /// scalar, not a vector): lane `i` is `lane[i] op scalar`, or
    /// `scalar op lane[i]` when `scalar_first` (`s - v`, `s / v`). Operands
    /// are always `[vector, scalar]`, whichever side the scalar was written.
    /// `checked` as for [`Self::Elementwise`].
    ElementwiseScalar {
        op: MirLaneOp,
        scalar_first: bool,
        checked: bool,
    },
    /// Lane-wise negation (`-v`); `checked` as for [`Self::Elementwise`].
    Negate {
        checked: bool,
    },
    /// Lane-wise boolean negation (`non m`) of a bool vector.
    Not,
    /// `mask ✓ then ✗ else` over a bool vector: lane `i` takes the then lane
    /// where the mask lane is true, else the else lane. Operands are
    /// `[mask, then, else]`; then and else are each a vector of the mask's
    /// width or one scalar every lane shares. All three are evaluated.
    Select,
    Dot,
    Cross,
    /// Euclidean norm `sqrt(dot(v, v))` (`norm`), in the element type.
    Norm,
    /// Lane count (`longitudo`): the element count of the vector, an
    /// integer constant once the width is literal (operator ruling
    /// 2026-10-15, Q-X1).
    Count,
    Normalized,
    Crea,
    /// Result lane `i` reads source lane `lanes[i]`.
    Swizzle(Vec<u8>),
}

impl MirVectorOp {
    /// Number of operands.
    #[must_use]
    pub fn arity(&self) -> usize {
        match self {
            Self::Norm
            | Self::Count
            | Self::Normalized
            | Self::Negate { .. }
            | Self::Not
            | Self::Crea
            | Self::Swizzle(_) => 1,
            Self::Elementwise { .. } | Self::ElementwiseScalar { .. } | Self::Dot | Self::Cross => {
                2
            }
            Self::Select => 3,
        }
    }

    /// Stable lowercase name for dumps and diagnostics.
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            Self::Elementwise { .. } => "elementwise",
            Self::ElementwiseScalar { .. } => "elementwise_scalar",
            Self::Negate { .. } => "negate",
            Self::Not => "not",
            Self::Select => "select",
            Self::Dot => "dot",
            Self::Cross => "cross",
            Self::Norm => "norm",
            Self::Count => "count",
            Self::Normalized => "normalized",
            Self::Crea => "crea",
            Self::Swizzle(_) => "swizzle",
        }
    }

    /// `true` when the result is a vector rather than the element scalar.
    #[must_use]
    pub fn returns_vector(&self) -> bool {
        !matches!(self, Self::Dot | Self::Norm | Self::Count)
    }
}

/// The scalar operator a [`MirIntrinsic::TensorLift`] applies per element.
///
/// Arity: `Neg` and `Not` take one operand, `Select` takes three (mask,
/// then-value, else-value), everything else takes two. `WholeEq` and
/// `WholeNotEq` (`≡`/`≢` over two tensors) are the exception to the lift: they
/// compare the two whole tensors and yield one scalar `bivalens`, not a tensor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MirLiftOp {
    Add,
    Sub,
    Mul,
    /// Floor division on integers, IEEE on floats (`/`).
    Div,
    /// Floor remainder on integers (`%`).
    Mod,
    /// True division (`÷`): integers convert to the result float first.
    TrueDiv,
    Neg,
    Not,
    Eq,
    NotEq,
    Lt,
    Gt,
    LtEq,
    GtEq,
    And,
    Or,
    /// `vel`: the fallback where the element is `nihil`.
    Coalesce,
    /// `⤒`: the larger element (IEEE `maximum`).
    Max,
    /// `⤓`: the smaller element.
    Min,
    Select,
    /// `a ≡ b` over two tensors: one verdict. True when both have the same
    /// shape and every element pair is `≡` (NaN is never `≡`); a shape
    /// difference is `false`, not an error.
    WholeEq,
    /// `a ≢ b` over two tensors: `not (a ≡ b)`.
    WholeNotEq,
}

impl MirLiftOp {
    /// Number of operands.
    #[must_use]
    pub fn arity(self) -> usize {
        match self {
            Self::Neg | Self::Not => 1,
            Self::Select => 3,
            Self::Add
            | Self::Sub
            | Self::Mul
            | Self::Div
            | Self::Mod
            | Self::TrueDiv
            | Self::Eq
            | Self::NotEq
            | Self::Lt
            | Self::Gt
            | Self::LtEq
            | Self::GtEq
            | Self::And
            | Self::Or
            | Self::Coalesce
            | Self::Max
            | Self::Min
            | Self::WholeEq
            | Self::WholeNotEq => 2,
        }
    }

    /// `true` when the result is a tensor; `WholeEq`/`WholeNotEq` give one
    /// `bivalens`.
    #[must_use]
    pub fn returns_tensor(self) -> bool {
        !matches!(self, Self::WholeEq | Self::WholeNotEq)
    }

    /// Stable lowercase name for dumps and diagnostics.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Add => "add",
            Self::Sub => "sub",
            Self::Mul => "mul",
            Self::Div => "div",
            Self::Mod => "mod",
            Self::TrueDiv => "true_div",
            Self::Neg => "neg",
            Self::Not => "not",
            Self::Eq => "eq",
            Self::NotEq => "not_eq",
            Self::Lt => "lt",
            Self::Gt => "gt",
            Self::LtEq => "lt_eq",
            Self::GtEq => "gt_eq",
            Self::And => "and",
            Self::Or => "or",
            Self::Coalesce => "coalesce",
            Self::Max => "max",
            Self::Min => "min",
            Self::Select => "select",
            Self::WholeEq => "whole_eq",
            Self::WholeNotEq => "whole_not_eq",
        }
    }
}

/// The reduction operator of a [`MirFilumReduceOp`]; mirrors the closed
/// source family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MirReduceOp {
    Sum,
    Product,
    Max,
    Min,
    ArgMax,
    ArgMin,
    All,
    Any,
    Count,
}

impl MirReduceOp {
    /// Stable lowercase name (dumps, diagnostics).
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Sum => "sum",
            Self::Product => "product",
            Self::Max => "max",
            Self::Min => "min",
            Self::ArgMax => "argmax",
            Self::ArgMin => "argmin",
            Self::All => "all",
            Self::Any => "any",
            Self::Count => "count",
        }
    }
}

/// How a [`MirFilumReduceOp`] sits in its kernel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MirFilumForm {
    /// The ungrouped `sum` inside an ordinary kernel body (SFR-5): the
    /// declared distributed sum, frozen lane width, one workgroup per
    /// `lane_width` source elements.
    Declared,
    /// The kernel's lane region (C4-8): the node IS the launch grid. The
    /// function takes the `Lane` builtin; `lane_width` is the workgroup size
    /// and out-of-range lanes contribute the identity.
    Grid,
    /// The reduction node inside the BODY of a lane region (gpu-reset rule
    /// 14.5): a `thread lane` reduce nested in an outer `thread lane` loop.
    /// One workgroup per outer coordinate; lanes stride the inner extent in
    /// chunks of `lane_width`. No stage 2, no launch split, and no `Lane`
    /// parameter of its own. Operands are exactly those of
    /// [`MirFilumForm::Grid`] (source, one extent per axis, optional
    /// identity, a function term's captures and function constant).
    Nested,
}

impl MirFilumForm {
    /// Stable lowercase name (dumps, diagnostics).
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Declared => "declared",
            Self::Grid => "grid",
            Self::Nested => "nested",
        }
    }
}

/// Filum reduction facts carried by [`MirIntrinsic::FilumReduce`] (SFR-5,
/// C4-8): the operator, filum metadata (lane binder + workgroup lane width),
/// the per-element binders, and the captured term-body MIR the per-lane
/// inliner replays. The axis count is `coordinates.len()`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MirFilumReduceOp {
    /// The reduction operator.
    pub op: MirReduceOp,
    /// Declared distributed sum or whole-grid lane region.
    pub form: MirFilumForm,
    /// The `filum f` lane binder's source name (metadata for emitters).
    pub filum_binder: Option<Symbol>,
    /// The per-element term binder local the term body reads.
    pub term_binder: MirLocalId,
    /// The term binder's source name.
    pub term_binder_name: Option<Symbol>,
    /// The coordinate binder locals (one per source tensor axis).
    pub coordinates: Vec<MirFilumCoordinate>,
    /// Workgroup lane width `W` (a power of two; tails fail closed at
    /// admission for the declared form).
    pub lane_width: u32,
    /// The captured term-body MIR: per-element statements that produce the
    /// `redde` term value into `term_result`. Empty for a term that is a
    /// function (see [`Self::term_captures`]).
    pub term_body: Vec<MirStatement>,
    /// A grid-form term that is a first-class MIR function (C4-8 U5):
    /// `Some(n)` when the term segment ends in a `MirConstant::Function`
    /// and the `n` operands before it are the values
    /// the term captures from before the node. The function's parameters are
    /// those `n` captures (in order), then one per coordinate, then the
    /// element binder; it returns the term value. The function carries its
    /// own control flow and helper calls, which a flat statement list cannot.
    /// `coordinates[i].local` and `term_binder` then name the term function's
    /// own parameter locals. `None` is the flat-list form (the declared sum,
    /// and a grid term with no body, whose term is the element binder).
    /// Group-key capture/function segments, when present, follow this segment.
    pub term_captures: Option<u32>,
    /// The local the term value lands in at term-body exit.
    pub term_result: MirLocalId,
    /// The type of the per-element term value.
    pub term_ty: MirType,
    /// Group-key functions and the literal result extents for a grouped sum
    /// or count. Each key has its own typed helper and explicit captures so
    /// device lowering can validate every axis before selecting a bucket.
    pub group: Option<MirFilumGroupOp>,
}

/// The literal output shape and per-axis key helpers of a grouped reduction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MirFilumGroupOp {
    /// Literal bucket extents, one per key expression.
    pub dimensions: Vec<u64>,
    /// Accumulator/result element type. Grouped sums widen here before any
    /// partial is formed; counts keep their fixed `u32` partial carrier.
    pub result_element_ty: MirType,
    /// One scalar helper for each key axis, in result-axis order.
    pub keys: Vec<MirFilumGroupKey>,
}

/// Typed capture contract for one grouped-reduction key helper.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MirFilumGroupKey {
    /// Number of explicit capture operands before this helper's function id.
    pub captures: u32,
    /// Checked integer type returned by the key helper.
    pub key_ty: MirType,
}

/// Whether a scan stores the value before or after incorporating the current
/// source element.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MirScanTotal {
    Inclusive,
    Exclusive,
}

/// Typed scan facts carried by [`MirIntrinsic::Scan`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MirScanOp {
    /// The same closed operation family used by reductions. Arg operations
    /// are rejected before MIR lowering.
    pub op: MirReduceOp,
    /// Inclusive or identity-based exclusive output.
    pub total: MirScanTotal,
    /// Whether traversal visits flattened source indexes from last to first.
    pub reverse: bool,
    /// Source tensor rank.
    pub rank: u8,
    /// Term helper result type.
    pub term_ty: MirType,
    /// Scan accumulator and result element type.
    pub result_element_ty: MirType,
    /// Number of capture operands preceding the term helper function id.
    pub term_captures: u32,
    /// Finite grouped key axes, each with the proven key interval and helper.
    pub group: Option<MirScanGroupOp>,
}

/// Finite key-domain facts and helpers for a grouped scan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MirScanGroupOp {
    pub axes: Vec<MirScanGroupAxis>,
}

/// One key coordinate in the finite group domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MirScanGroupAxis {
    /// Lowest valid key value, inclusive.
    pub min: i128,
    /// Number of valid values beginning at `min`.
    pub extent: u64,
    /// The helper's integer return type.
    pub key_ty: MirType,
    /// Number of capture operands preceding the key helper function id.
    pub captures: u32,
}

/// One coordinate binder of a filum declaration (SFR-5).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MirFilumCoordinate {
    /// The coordinate's local (numerus; one per tensor axis).
    pub local: MirLocalId,
    /// The coordinate binder's source name.
    pub name: Option<Symbol>,
}

// `Eq` marker for the plan-carried declaration facts — the same bitwise
// equality stance `impl Eq for MirConstant` already takes for float-bearing
// payloads; the kernel plan derives `Eq`.
impl Eq for MirFilumReduceOp {}

// Plan-carried MIR facts use the same bitwise equality stance as other
// float-bearing MIR nodes; scan contracts themselves are structurally typed.
impl Eq for MirScanOp {}

impl Eq for MirFilumCoordinate {}

/// Type-check intrinsic payload (`est <type>` / `non est <type>`).
///
/// `positive` distinguishes `est` (the runtime tag matches `ty`) from
/// `non est` (the tag does not match). `ty` is the valor-carried concrete
/// target type whose runtime variant the operand must match (semantic type
/// id, resolved like [`MirConversion::target_ty`]); inner type parameters
/// are erased at Valor boxing time and are not checked.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MirTypeCheckOp {
    pub positive: bool,
    pub ty: MirType,
}

/// Gradient handle runtime operations.
///
/// These are lower-level than MIR collection ops — they operate on
/// runtime-owned gradient storage identified by opaque handles, not on
/// MIR-level values. Generated backward code emits these to accumulate
/// per-parameter gradients through the `__faber_rt_v1_gradient_*` ABI.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum MirGradientOp {
    Create,
    Accumulate,
    Read,
    Zero,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MirSermoOp {
    ViewMeus,
    ViewTuus,
    Send,
    /// `meus.flush()`: push any pending outbound frames and keep the
    /// conversation open (Finish stays the close).
    Flush,
    Finish,
    /// `s.close()` (D8.3): close the handle; `bivalens` says whether this
    /// call closed it. A failure is kept for [`Self::CloseFailed`] and
    /// [`Self::CloseError`], which lowering reads right after.
    Close,
    /// Whether the last `close()` on this conversation reported an error.
    CloseFailed,
    /// The last `close()` error message (`textus`), taken.
    CloseError,
    Receive {
        frame: DefId,
    },
    Cursor {
        frame: DefId,
    },
    DrainCursor,
}

impl MirSermoOp {
    #[must_use]
    pub fn spelling(&self) -> &'static str {
        match self {
            Self::ViewMeus => "view_meus",
            Self::ViewTuus => "view_tuus",
            Self::Send => "send",
            Self::Flush => "flush",
            Self::Finish => "finish",
            Self::Close => "close",
            Self::CloseFailed => "close_failed",
            Self::CloseError => "close_error",
            Self::Receive { .. } => "receive",
            Self::Cursor { .. } => "cursor",
            Self::DrainCursor => "drain_cursor",
        }
    }
}

/// `@ cursor` call: the generator's `cede` yields read as a `lista<T>`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MirCursorStream {
    pub callee: DefId,
    /// `true` when the only consumer is a stepped `itera ex` walk that
    /// re-reads `length` before each index read (D6.6: cursors are lazy).
    /// The oracle (`radix-mir-runner`, `eval_cursor_stream`) materializes
    /// only unstepped streams — a `stepped` stream stays lazy, and a backend
    /// that cannot suspend the generator must decline by name (for example
    /// `stepped cursor stream`) rather than materialize: an eager lowering
    /// changes what the program prints (generator-side output interleaves
    /// with the loop body) and hangs on an unbounded generator under `break`.
    /// A suspending backend may resume the generator one `cede` per step and
    /// abandon it on `rumpe`.
    #[serde(default)]
    pub stepped: bool,
}

/// Diagnostic runtime channels surfaced by Faber source constructs.
///
/// WHY: Latin variant names mirror `HirScribeKind` Faber output builtins (`nota`,
/// `vide`, `mone`, `scribe`) lowered into MIR — not an English diagnostic
/// taxonomy. Zone 3 MIR uses these spellings as Faber-channel handles, not
/// `Note`/`Warn`/`Log` compiler vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MirDiagnosticKind {
    Nota,
    Vide,
    Mone,
    Scribe,
}

/// Conversion intrinsic payload.
///
/// `params` preserve source hints for runtime conversion policy; `recovery`
/// captures explicit inline recovery behavior as a normal MIR operand.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MirConversion {
    /// `Some` when this conversion is an implicit store (d11-6 rulings 5 and
    /// 17): the slot's policy applies at runtime and a failure traps, never
    /// entering the `⇥` channel. The site names the slot in the trap message.
    #[serde(default)]
    pub store: Option<radix_hir::HirStoreSite>,
    pub flavor: MirConversionFlavor,
    pub source_ty: MirType,
    pub target_ty: MirType,
    pub params: Vec<Symbol>,
    pub recovery: Option<MirOperand>,
    pub struct_defaults: Vec<MirStructFieldDefaults>,
}

/// Conversion-site defaults for `valor ↦ genus` extraction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MirStructFieldDefaults {
    pub def_id: DefId,
    pub fields: Vec<MirNamedOperand>,
}

/// Conversion implementation category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MirConversionFlavor {
    Cast,
    Runtime,
    /// An implicit narrowing store whose value range was proven to fit at
    /// compile time; never traps, carries no store site.
    Proven,
}

/// Provider classification for host dispatch routing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MirProviderKind {
    Package,
    Kernel(crate::kernel_module::KernelModule),
}

/// Provider runtime identity retained from module/name resolution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MirProvider {
    pub kind: MirProviderKind,
    pub module: Vec<Symbol>,
    pub name: Symbol,
}

/// Aggregate construction payload.
///
/// Kind and field shape are intentionally separate so validation can reject
/// mismatches such as map construction with ordered fields or struct
/// construction without named operands.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MirAggregate {
    pub kind: MirAggregateKind,
    pub ty: MirType,
    pub fields: MirAggregateFields,
}

/// Domain kind for a constructed aggregate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MirAggregateKind {
    Tuple,
    Array,
    Record,
    ClosureEnvironment(MirClosureEnvironmentId),
    /// Fresh shared cell holding one positional item: the storage of a
    /// binding captured by reference and rebound somewhere. Executed at the
    /// binding's declaration, so each declaration execution (e.g. each loop
    /// iteration) gets its own cell. The aggregate type is the content type.
    SharedCell,
    Map,
    Set,
    Tensor,
    SparseTensor,
    Interval {
        inclusive: bool,
    },
    Vector,
    Matrix,
    Struct(DefId),
    EnumVariant(DefId),
}

/// Field payload shape for aggregate construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MirAggregateFields {
    Ordered(Vec<MirAggregateItem>),
    Named(Vec<MirNamedOperand>),
    Keyed(Vec<MirKeyValueOperand>),
}

/// Ordered aggregate element, either a single operand or a spread.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MirAggregateItem {
    Operand(MirOperand),
    Spread(MirOperand),
}

/// Named aggregate field value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MirNamedOperand {
    pub name: Symbol,
    pub value: MirOperand,
}

/// Key/value aggregate entry used by map construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MirKeyValueOperand {
    pub key: MirOperand,
    pub value: MirOperand,
}

/// Explicit nullable-value operation.
///
/// Nullable behavior is represented as MIR operations instead of implicit codegen
/// convention so validation can enforce option payload/result contracts before
/// target-specific lowering.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MirOptionOp {
    None,
    Some(MirOperand),
    IsNil(MirOperand),
    IsNotNil(MirOperand),
    Unwrap {
        value: MirOperand,
        mode: MirOptionUnwrapMode,
    },
    Coalesce {
        value: MirOperand,
        fallback: MirOperand,
    },
    Chain {
        base: MirOperand,
        link: MirOptionChainLink,
    },
}

/// Policy for unwrap operations after semantic nullability analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MirOptionUnwrapMode {
    Assert,
    Assume,
}

/// One optional-chain step from a nullable base.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MirOptionChainLink {
    Field(Symbol),
    VariantField {
        variant: DefId,
        field: Symbol,
    },
    Index(MirOperand),
    Call {
        callee: MirCallee,
        args: Vec<MirOperand>,
    },
}

#[cfg(test)]
#[path = "nodes_test.rs"]
mod nodes_test;
