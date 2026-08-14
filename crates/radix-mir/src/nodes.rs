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

use crate::{MirKernelBuiltin, MirKernelShaderStage};
use radix_hir::DefId;
use radix_lexer::{Span, Symbol};
use radix_types::{IndexId, TypeId};
use serde::{Deserialize, Serialize};

/// Stable identifier for a MIR function within one `MirProgram`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct MirFunctionId(pub u32);

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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MirClosureCapture {
    pub source: DefId,
    pub name: Symbol,
    pub ty: MirType,
    pub source_place: MirPlace,
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

    /// O(1) lookup when `id.0` matches the dense vector index (the common
    /// lowering allocation order). Falls back to a linear scan when IDs are
    /// sparse or reordered.
    #[must_use]
    pub fn function_by_id(&self, id: MirFunctionId) -> Option<&MirFunction> {
        if let Some(function) = self.functions.get(id.0 as usize) {
            if function.id == id {
                return Some(function);
            }
        }
        self.functions.iter().find(|function| function.id == id)
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
    /// HIR→MIR lowering. `None` for non-shader functions and synthetic helpers.
    pub shader_stage: Option<MirKernelShaderStage>,
    pub span: Span,
}

impl MirFunction {
    /// O(1) lookup when `id.0` matches the dense vector index (the common
    /// lowering allocation order). Falls back to a linear scan when IDs are
    /// sparse or reordered.
    #[must_use]
    pub fn block_by_id(&self, id: MirBlockId) -> Option<&MirBlock> {
        if let Some(block) = self.blocks.get(id.0 as usize) {
            if block.id == id {
                return Some(block);
            }
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

/// Shallow ownership encoding for `de` / `in` / `ex` parameter modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MirParamMode {
    Owned,
    De,
    In,
    Ex,
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
    /// `DefId` for `Struct`/`EnumVariant` aggregates; `None` for tuple/array/map/set/tensor/vector.
    #[must_use]
    pub fn definition_id(&self) -> Option<DefId> {
        match self {
            Self::Struct(def_id) | Self::EnumVariant(def_id) => Some(*def_id),
            _ => None,
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
    Index(MirOperand),
    VectorLane(u8),
    MatrixCell {
        row: u8,
        column: u8,
    },
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
}

impl Eq for MirConstant {}

impl std::hash::Hash for MirConstant {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
        match self {
            Self::Int(v) => v.hash(state),
            Self::UInt(v) => v.hash(state),
            Self::Float(v) => v.to_bits().hash(state),
            Self::String(v) => v.hash(state),
            Self::Ascii(v) => v.hash(state),
            Self::Bool(v) => v.hash(state),
            Self::Nil => {}
            Self::Unit => {}
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

/// Compiler-runtime operations that are not ordinary user calls.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MirIntrinsic {
    Diagnostic(MirDiagnosticKind),
    Assert,
    FormatString {
        template: Symbol,
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
}

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
    Finish,
    Receive { frame: DefId, data_field: Symbol },
    Cursor { frame: DefId, data_field: Symbol },
    DrainCursor,
}

impl MirSermoOp {
    #[must_use]
    pub fn spelling(&self) -> &'static str {
        match self {
            Self::ViewMeus => "view_meus",
            Self::ViewTuus => "view_tuus",
            Self::Send => "send",
            Self::Finish => "finish",
            Self::Receive { .. } => "receive",
            Self::Cursor { .. } => "cursor",
            Self::DrainCursor => "drain_cursor",
        }
    }
}

/// Compiler-owned atomic cell operation category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MirAtomicOp {
    Load,
    Store,
    Exchange,
    CompareExchange,
}

impl MirAtomicOp {
    #[must_use]
    pub fn spelling(self) -> &'static str {
        match self {
            Self::Load => "load",
            Self::Store => "store",
            Self::Exchange => "exchange",
            Self::CompareExchange => "compare_exchange",
        }
    }
}

/// `@ cursor` call collected into a `lista<T>` before indexed iteration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MirCursorStream {
    pub callee: DefId,
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
}

/// Built-in collection operation category.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MirCollectionOp {
    Append,
    AppendImmutable,
    Put,
    Delete,
    Index,
    IsEmpty,
    Length,
    Contains,
    First,
    Last,
    RemoveFirst,
    RemoveLast,
    Reverse,
    ReverseInPlace,
    Sort,
    Union,
    Intersection,
    Difference,
    SymmetricDifference,
    IsSubset,
    IsSuperset,
    Slice,
    Take,
    TakeLast,
    DropFirst,
    StartsWith,
    EndsWith,
    Uppercase,
    Lowercase,
    Trim,
    Split,
    Replace,
    Absolute,
    Signum,
    Minimum,
    Maximum,
    Approximate,
    Sum,
    /// Sum across a specific axis: `sum(x, axis)`. The axis is a trailing
    /// const operand (`MirConstant::Int`) in the `MirRuntimeCall::args` list.
    /// Result rank is input rank - 1.
    TensorSumAxis,
    Round,
    Floor,
    Ceiling,
    Truncate,
    TensorShape,
    TensorReshape,
    TensorGet,
    TensorSet,
    TensorFill,
    TensorCreate,
    TensorFromFlat,
    TensorFlatten,
    TensorMaterialize,
    TensorSlice,
    TensorAdd,
    TensorSub,
    TensorMul,
    TensorMatMul,
    TensorMean,
    /// Layer normalization: `(x - mean) / sqrt(var + eps)` with optional
    /// affine transform (gamma, beta). Axis and epsilon are trailing const
    /// operands in the `MirRuntimeCall::args` list, NOT a payload struct —
    /// `MirIntrinsic::Collection` is a bare discriminant.
    TensorLayerNorm,
    /// Root-mean-square normalization (the CCI-1 `rms_norm` source contract;
    /// `decoder_ops::rms_norm` semantics): `y = x·(1/sqrt(mean(x²)+eps))·gamma`,
    /// no mean subtraction, f64 sum-of-squares → f32 mean. Axis and epsilon
    /// are trailing const operands (`[x, axis, eps, gamma]`), gamma is the
    /// mandatory affine peer operand — mirroring `TensorLayerNorm`'s const
    /// layout without beta. Source-side axis-valid + gamma-width contract is
    /// enforced at the HIR→AIR construction site (fail-closed).
    TensorRmsNorm,
    /// llama-arch NORM consecutive-pair rotary position embedding (the CCI-1
    /// `rope_norm` source contract): pairs `(x[2k], x[2k+1])` with
    /// `theta[k] = pos·freq_base^(−2k/dim)`, `freq_base` frozen at 100000
    /// (v1 source constant). `pos` + `dim` are trailing const operands
    /// (`[x, pos, dim]`); elements beyond `dim` on the last axis are
    /// preserved. Odd/overlarge `dim` fail closed at the source surface.
    TensorRope,
    /// Row gather over the first axis (the CCI-1 `gather` source contract):
    /// `table[V,D]` + `ids[T]` → `[T,D]` with `out[t] = table[ids[t]]`,
    /// repeated/out-of-order ids preserved. The ids vector is a plain
    /// operand (`[x, ids]`); out-of-range/negative ids fail closed at the
    /// source surface (no silent clamping). Shape-transforming — not
    /// `TensorGet`/`TensorSlice`.
    TensorGather,
    /// Causal masked-row softmax (GI3-1): numerically stable row softmax
    /// over the causal attention scores with zero probability above the
    /// diagonal (`[x]` — the rank-2 causal score matrix; row `i` masks
    /// columns `j > i`). No mask tensor input (the causal structure is the
    /// recipe's semantics); the diagonal is included.
    TensorCausalMaskedSoftmax,
    TensorDiv,
    TensorNeg,
    /// Gaussian Error Linear Unit using the tanh approximation:
    /// 0.5*x*(1+tanh(√(2/π)*(x+0.044715*x³))). Error < 1e-6 vs exact Gelu.
    /// Elementwise unary activation on tensor values. All finite f32 inputs valid.
    TensorGelu,
    /// Rectified linear unit: max(0, x), elementwise unary activation on tensor
    /// values. All finite inputs valid; no domain constraints.
    TensorRelu,
    /// Softmax: `exp(x_i` - max(x)) / `sum(exp(x_j` - max(x))) with numerical
    /// stability. Operates on rank-1 (vector) or rank-2 (batched row-wise,
    /// axis 1 — the last axis). The AIR VJP exists
    /// (`radix-air/src/reverse_ad/vjp.rs`, softmax walk: Mul → `TensorSumAxis`
    /// → Fill → Sub → Mul); the MIR op itself is a forward collection op and
    /// the device recipe (`RowSoftmax`) is admitted by the stage-6 P1 unit.
    TensorSoftmax,
    TensorTranspose,
    /// Categorical cross-entropy loss: forward-only. Takes (logits, targets)
    /// as 2-arg collection op and returns scalar f32 loss.
    TensorCruxEntropia,
    /// Number of explicitly stored nonzero entries in a sparse tensor.
    SparseNonzeroCount,
    /// Materialize a sparse tensor as a dense tensor with the same element and shape.
    SparseDensify,
    /// Read a sparse coordinate, returning the element zero when absent.
    SparseGet,
    /// Store or remove a sparse coordinate; storing zero removes it.
    SparseSet,
    IntervalIntersect,
    IntervalUnion,
    IntervalLength,
    IntervalContains,
    IntervalClamp,
    IntervalMaterialize,
    /// Map key snapshot used by `itera de` over `tabula<K,V>`.
    Keys,
    /// Map value snapshot used by `itera ex` over `tabula<K,V>`.
    Values,
    /// Regex `consentit`: whether the `regex` receiver's pattern matches a
    /// `textus` argument (the Rust `regex` crate `is_match` semantics).
    RegexMatches,
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
    Map,
    Set,
    Tensor,
    SparseTensor,
    Interval { inclusive: bool },
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
