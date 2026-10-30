//! CLIF pre-emission capability classifier (goal `radix-mir-clif`, unit C2).
//!
//! Walks a validated MIR program in storage order and returns a
//! [`Lowerability`] verdict *before* emission, naming a gap for every
//! construct outside the current CLIF subset. The subset widens as units
//! C3-C19 land; the walker and the gap vocabulary are this unit's deliverable.
//!
//! BOUNDARY (mirrors the LLVM classifier): this classifies target
//! lowerability only. It does not re-check type correctness (owned by
//! `MirValidationContext`) and it does not duplicate emitter printing.
//! Only gaps deterministic from MIR structure are classified; deep-emit
//! `unsupported()` remains the safety net for anything unenumerated.
//!
//! Shape strings are the corpus contract: `diagnostic_contains` fragments
//! in the clif known-fail rows pin them, so they change only with a suite
//! regeneration. Policy declines (device shapes, net leaves per D7) use
//! [`CapabilityGap::TargetPolicyRejection`]; not-yet subset shapes use
//! [`CapabilityGap::UnsupportedMirShape`]; unresolved semantic types reuse
//! the LLVM wording (`unresolved type`) so shared semantics stay aligned.

use handroll::{FxHashMap, FxHashSet};
use radix_hir::DefId;
use radix_mir::visit::MirVisitor;
use radix_mir::{
    MirAggregate, MirAggregateKind, MirBinOp, MirCallee, MirCollectionOp, MirConstant,
    MirConversion, MirFunction, MirFunctionId, MirIntrinsic, MirLocal, MirOperand,
    MirOptionChainLink, MirOptionOp, MirParam, MirPlace, MirProgram, MirProjection, MirRuntimeCall,
    MirSermoOp, MirStatementKind, MirTemp, MirTerminator, MirTerminatorKind, MirType, MirUnOp,
    MirValue, MirValueKind, ValidatedMir,
};
use radix_types::vocabulary::{CapabilityGap, Lowerability};
use radix_types::{Primitive, SemanticMutability, Type, TypeId, TypeTable};

/// Slug for shapes the CLIF subset does not cover yet.
pub const SLUG_CLIF_SUBSET: &str = "clif-subset";
/// Slug for shapes CLIF rejects by policy (device family, net leaves).
pub const SLUG_CLIF_POLICY: &str = "clif-policy";

/// A gap kind plus its stable shape text.
enum Gap {
    Subset(String),
    Policy(String),
    Layout(String),
    Abi(String),
}

impl Gap {
    fn into_capability_gap(self) -> CapabilityGap {
        match self {
            Self::Subset(shape) => CapabilityGap::UnsupportedMirShape {
                shape,
                slug: SLUG_CLIF_SUBSET,
            },
            Self::Policy(shape) => CapabilityGap::TargetPolicyRejection {
                shape,
                slug: SLUG_CLIF_POLICY,
            },
            Self::Layout(shape) => CapabilityGap::MissingLayoutFact {
                shape,
                slug: "missing-layout-fact",
            },
            Self::Abi(shape) => CapabilityGap::MissingAbiFact {
                shape,
                slug: "missing-abi-fact",
            },
        }
    }
}

/// CLIF capability classifier. Collects every structural gap in storage
/// order; the first gap drives the diagnostic shape substring.
pub struct ClifCapability<'a> {
    types: &'a TypeTable,
    function_sources: FxHashSet<DefId>,
    /// When set, only these functions are classified (the entry's call
    /// closure); unreachable library code is never lowered, so it is never a gap.
    reachable: Option<FxHashSet<MirFunctionId>>,
    /// Semantic types of the conversation handles this program opens. The type
    /// table carries no `sermo` identity, so the handle is recognized by the
    /// `SermoOpen` result type it was opened as; a materializing conversion
    /// reads it back from the same program-wide table (a handle passed to a
    /// helper keeps that type).
    sermo_handles: FxHashSet<TypeId>,
    gaps: Vec<CapabilityGap>,
}

/// The semantic types conversations are opened as, across the whole program.
///
/// Handles enter MIR only as a `SermoOpen` result, so its `return_ty` is the
/// handle identity available to a classifier that never sees the HIR builtin
/// `DefId` for `sermo`.
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

/// Collects the functions a body calls directly.
struct DirectCallees<'a> {
    sources: &'a FxHashMap<DefId, MirFunctionId>,
    found: Vec<MirFunctionId>,
}

impl MirVisitor for DirectCallees<'_> {
    fn visit_callee(&mut self, callee: &MirCallee) {
        match callee {
            MirCallee::Function(id) => self.found.push(*id),
            MirCallee::Definition { source, .. } => {
                if let Some(id) = self.sources.get(source) {
                    self.found.push(*id);
                }
            }
            MirCallee::Closure(_) | MirCallee::Value(_) => {}
        }
        radix_mir::visit::walk_callee(self, callee);
    }

    fn visit_operand(&mut self, operand: &MirOperand) {
        if let MirOperand::Constant(MirConstant::Function(id)) = operand {
            self.found.push(*id);
        }
        radix_mir::visit::walk_operand(self, operand);
    }

    fn visit_runtime_call(&mut self, call: &MirRuntimeCall) {
        if let MirIntrinsic::CursorStream(stream) = &call.intrinsic
            && let Some(id) = self.sources.get(&stream.callee)
        {
            self.found.push(*id);
        }
        radix_mir::visit::walk_runtime_call(self, call);
    }
}

/// The functions reachable from `entry` through direct calls.
fn reachable_from(program: &MirProgram, entry: Option<MirFunctionId>) -> FxHashSet<MirFunctionId> {
    let sources: FxHashMap<DefId, MirFunctionId> = program
        .functions
        .iter()
        .filter_map(|function| function.source.map(|source| (source, function.id)))
        .collect();
    let mut seen = FxHashSet::default();
    let mut work: Vec<MirFunctionId> = entry.into_iter().collect();
    while let Some(id) = work.pop() {
        if !seen.insert(id) {
            continue;
        }
        let Some(function) = program.functions.iter().find(|function| function.id == id) else {
            continue;
        };
        let mut collector = DirectCallees {
            sources: &sources,
            found: Vec::new(),
        };
        collector.visit_function(function);
        work.extend(collector.found);
    }
    seen
}

impl<'a> ClifCapability<'a> {
    fn new(types: &'a TypeTable) -> Self {
        Self {
            types,
            function_sources: FxHashSet::default(),
            reachable: None,
            sermo_handles: FxHashSet::default(),
            gaps: Vec::new(),
        }
    }

    /// Classify a whole program. Returns [`Lowerability::Capable`] when no
    /// structural gap is found; otherwise every gap collected, in storage
    /// order.
    #[must_use]
    pub fn classify_program(mut self, program: &MirProgram) -> Lowerability {
        self.function_sources = program
            .functions
            .iter()
            .filter_map(|function| function.source)
            .collect();
        self.sermo_handles = sermo_handle_types(program);
        self.visit_program(program);
        if self.gaps.is_empty() {
            Lowerability::Capable
        } else {
            Lowerability::Rejected(self.gaps)
        }
    }

    fn record(&mut self, gap: Gap) {
        self.gaps.push(gap.into_capability_gap());
    }

    /// Classify one semantic type against the CLIF subset. `None` means the
    /// type is inside the subset (scalars and the control-only shapes).
    fn type_gap(&self, ty: MirType) -> Option<Gap> {
        match self.types.get(ty.semantic_id()) {
            Type::Primitive(
                Primitive::Numerus
                | Primitive::Fractus
                | Primitive::Bivalens
                | Primitive::Vacuum
                | Primitive::Numquam
                | Primitive::Textus
                | Primitive::Ascii
                | Primitive::Littera
                | Primitive::Regex
                | Primitive::Match
                | Primitive::Json
                | Primitive::Valor
                | Primitive::Instans
                | Primitive::Nihil
                | Primitive::Ignotum,
            )
            | Type::SizedNumeric(Primitive::Numerus, _)
            | Type::Bits(_)
            | Type::SizedInstans(_)
            | Type::BoundedTextus { .. }
            | Type::BoundedAscii { .. }
            | Type::Struct(_)
            | Type::Enum(_)
            | Type::Interface(_) => None,
            Type::SizedNumeric(Primitive::Fractus, width) if width.is_float() => None,
            Type::SizedNumeric(..) => Some(Gap::Subset("sized numeric value".to_owned())),
            Type::PolicyFloat(..) => Some(Gap::Subset("policy float value".to_owned())),
            Type::BoundedOcteti { .. } | Type::Primitive(Primitive::Octeti) => {
                crate::lower::byte_element(self.types)
                    .is_none()
                    .then(|| Gap::Subset("byte buffer value".to_owned()))
            }
            Type::Array(element) | Type::BoundedArray { element, .. } | Type::Cursor(element) => {
                self.type_gap(MirType::semantic(*element))
            }
            Type::Map(key, value) => self.map_type_gap(*key, *value),
            Type::Set(element) => self.type_gap(MirType::semantic(*element)).or_else(|| {
                self.is_letter(*element)
                    .then(|| Gap::Subset("character in a set".to_owned()))
            }),
            Type::Queue(_)
            | Type::BoundedQueue { .. }
            | Type::Stack(_)
            | Type::BoundedStack { .. }
            | Type::Ledger(_, _) => Some(Gap::Subset("collection value".to_owned())),
            Type::Record(fields) => fields
                .values()
                .find_map(|field| self.type_gap(MirType::semantic(*field))),
            Type::Tuple(elements) => elements
                .iter()
                .find_map(|element| self.type_gap(MirType::semantic(*element))),
            Type::Union(members) => members
                .iter()
                .find_map(|member| self.union_member_gap(*member)),
            Type::Intersection(_) => Some(Gap::Subset("union value".to_owned())),
            Type::Option(inner) => match self.types.get(*inner) {
                Type::Option(_) => Some(Gap::Subset("optional value".to_owned())),
                _ => self.type_gap(MirType::semantic(*inner)),
            },
            Type::Func(sig) => self.function_type_gap(sig),
            // A read-only reference is the value it borrows; a mutable one is
            // only a shared handle for the kinds that are handles.
            Type::Ref(SemanticMutability::Mutable, inner) => self.mutable_reference_gap(*inner),
            Type::Ref(_, inner) | Type::Promissum(inner) => {
                self.type_gap(MirType::semantic(*inner))
            }
            Type::ModularWord(width) | Type::SaturatingWord(width)
                if width.integer_range().is_some() =>
            {
                None
            }
            Type::ModularWord(_) => Some(Gap::Subset("wrapping value".to_owned())),
            Type::SaturatingWord(_) => Some(Gap::Subset("saturating value".to_owned())),
            Type::AsyncCursor(_, _) | Type::PromissumFailable(_, _) => {
                Some(Gap::Subset("async value".to_owned()))
            }
            Type::Intervallum(element, _) => (!matches!(
                self.types.get(*element),
                Type::Primitive(Primitive::Numerus)
            ))
            .then(|| Gap::Subset("interval value".to_owned())),
            Type::Vector(element, _) => self.type_gap(MirType::semantic(*element)),
            Type::Tensor(..)
            | Type::Matrix(..)
            | Type::Sparsa(..)
            | Type::Atomic(_)
            | Type::TensorEdgePolicy(_) => Some(Gap::Policy("device value".to_owned())),
            Type::Param(_) | Type::Infer(_) | Type::InferUnion(_) | Type::Error => {
                Some(Gap::Layout("unresolved type".to_owned()))
            }
            Type::Alias(_, inner) => self.alias_type_gap(*inner),
            _ => Some(Gap::Subset("unsupported type".to_owned())),
        }
    }

    fn map_type_gap(&self, key: TypeId, value: TypeId) -> Option<Gap> {
        self.type_gap(MirType::semantic(key))
            .or_else(|| self.type_gap(MirType::semantic(value)))
            .or_else(|| {
                [key, value]
                    .into_iter()
                    .any(|member| self.is_letter(member))
                    .then(|| Gap::Subset("character in a map".to_owned()))
            })
    }

    fn function_type_gap(&self, sig: &radix_types::FuncSig) -> Option<Gap> {
        if !sig.type_params.is_empty() || sig.is_async || sig.is_generator {
            return Some(Gap::Subset("function value".to_owned()));
        }
        sig.params
            .iter()
            .find_map(|param| {
                let value = match self.types.get(param.ty) {
                    Type::Ref(_, inner) => *inner,
                    _ => param.ty,
                };
                self.type_gap(MirType::semantic(value))
            })
            .or_else(|| self.type_gap(MirType::semantic(sig.ret)))
            .or_else(|| {
                sig.err
                    .and_then(|err| self.type_gap(MirType::semantic(err)))
            })
    }

    fn alias_type_gap(&self, inner: TypeId) -> Option<Gap> {
        let mut inner = inner;
        loop {
            match self.types.get(inner) {
                Type::Alias(_, next) => inner = *next,
                resolved => {
                    let probe = MirType::semantic(inner);
                    return match resolved {
                        Type::Param(_) | Type::Infer(_) | Type::Error => {
                            Some(Gap::Layout("unresolved type".to_owned()))
                        }
                        _ => self.type_gap(probe),
                    };
                }
            }
        }
    }

    fn mutable_reference_gap(&self, inner: TypeId) -> Option<Gap> {
        if matches!(
            self.types.get(inner),
            Type::Array(_)
                | Type::BoundedArray { .. }
                | Type::Struct(_)
                | Type::Record(_)
                | Type::Map(..)
                | Type::Set(_)
        ) {
            self.type_gap(MirType::semantic(inner))
        } else {
            Some(Gap::Subset("reference value".to_owned()))
        }
    }

    fn union_member_gap(&self, member: TypeId) -> Option<Gap> {
        if matches!(
            self.types.get(member),
            Type::Primitive(
                Primitive::Nihil | Primitive::Ignotum | Primitive::Json | Primitive::Valor
            ) | Type::Option(_)
                | Type::Bits(_)
                | Type::Union(_)
        ) {
            return Some(Gap::Subset("union value".to_owned()));
        }
        self.type_gap(MirType::semantic(member))
    }

    /// Whether `id` is a `littera`: a map or set display of one is not
    /// lowered (the runner's quoting of a letter there is not pinned).
    fn is_letter(&self, id: TypeId) -> bool {
        matches!(self.types.get(id), Type::Primitive(Primitive::Littera))
    }

    /// Short readable type-kind name for gap shapes.
    fn kind_name(&self, ty: MirType) -> String {
        let debug = format!("{:?}", self.types.get(ty.semantic_id()));
        let end = debug.find(['(', '{', ' ']).unwrap_or(debug.len());
        let head = &debug[..end];
        if head == "Primitive" {
            let inner = debug[end..].trim_start_matches('(');
            let stop = inner.find([')', '(', '{']).unwrap_or(inner.len());
            return inner[..stop].to_owned();
        }
        head.to_owned()
    }

    /// Readable intrinsic name for gap shapes (family plus operation).
    fn intrinsic_name(&self, intrinsic: &MirIntrinsic) -> String {
        match intrinsic {
            MirIntrinsic::Convert(conversion) => format!(
                "Convert {:?} {} {}->{}",
                conversion.flavor,
                if conversion.store.is_some() {
                    "store"
                } else {
                    "explicit"
                },
                self.kind_name(conversion.source_ty),
                self.kind_name(conversion.target_ty),
            ),
            other => {
                let debug = format!("{other:?}");
                let end = debug.find(['{', ' ']).unwrap_or(debug.len());
                debug[..end].chars().take(64).collect()
            }
        }
    }

    /// Resolve aliases before checking a semantic type identity.
    fn resolve_alias(&self, mut id: TypeId) -> TypeId {
        while let Type::Alias(_, inner) = self.types.get(id) {
            id = *inner;
        }
        id
    }

    /// Whether `ty` is a conversation handle this program opened.
    fn is_sermo_handle(&self, ty: MirType) -> bool {
        self.sermo_handles
            .contains(&self.resolve_alias(ty.semantic_id()))
    }

    /// A conversation handle materializing to a CLIF-supported value. The
    /// `SERMO_MATERIALIZE_I64`, `SERMO_DRAIN`, and
    /// `SERMO_MATERIALIZE_INSTANT` symbols cover numerus, vacuum, and instans;
    /// textus, fractus, and bivalens remain declined.
    fn sermo_materialization(&self, conversion: &MirConversion) -> bool {
        conversion.store.is_none()
            && self.is_sermo_handle(conversion.source_ty)
            && matches!(
                self.types
                    .get(self.resolve_alias(conversion.target_ty.semantic_id())),
                Type::Primitive(Primitive::Numerus | Primitive::Vacuum | Primitive::Instans)
                    | Type::SizedInstans(_)
            )
    }

    fn is_unknown(&self, ty: MirType) -> bool {
        matches!(
            self.types.get(ty.semantic_id()),
            Type::Primitive(Primitive::Ignotum)
        )
    }

    fn check_type(&mut self, ty: MirType) {
        if let Some(gap) = self.type_gap(ty) {
            self.record(gap);
        }
    }

    fn constant_gap(constant: &MirConstant) -> Option<Gap> {
        match constant {
            MirConstant::Int(_)
            | MirConstant::UInt(_)
            | MirConstant::Float(_)
            | MirConstant::Bool(_)
            | MirConstant::Unit
            | MirConstant::String(_)
            | MirConstant::Ascii(_)
            | MirConstant::Regex { .. }
            | MirConstant::Nil
            | MirConstant::Function(_)
            | MirConstant::Magnus(_) => None,
            MirConstant::Octeti(_) => Some(Gap::Subset("byte constant".to_owned())),
            MirConstant::ShapeSize { .. } => Some(Gap::Policy("device constant".to_owned())),
        }
    }

    fn aggregate_gap(kind: &MirAggregateKind) -> Option<Gap> {
        match kind {
            MirAggregateKind::Tuple
            | MirAggregateKind::Array
            | MirAggregateKind::Map
            | MirAggregateKind::Set
            | MirAggregateKind::Record
            | MirAggregateKind::Struct(_)
            | MirAggregateKind::EnumVariant(_)
            | MirAggregateKind::ClosureEnvironment(_)
            | MirAggregateKind::SharedCell
            | MirAggregateKind::Interval { .. }
            | MirAggregateKind::Vector => None,
            MirAggregateKind::Tensor
            | MirAggregateKind::SparseTensor
            | MirAggregateKind::Matrix => Some(Gap::Policy("device construction".to_owned())),
        }
    }

    fn supported_conversion(&self, intrinsic: &MirIntrinsic) -> bool {
        let scalar = self.supported_scalar_conversion(intrinsic);
        let open = self.supported_open_conversion(intrinsic);
        scalar || open
    }

    fn supported_scalar_conversion(&self, intrinsic: &MirIntrinsic) -> bool {
        let renders_text = matches!(
            intrinsic,
            MirIntrinsic::Convert(conversion)
                if conversion.store.is_none()
                    && !self.is_sermo_handle(conversion.source_ty)
                    && self.type_gap(conversion.source_ty).is_none()
                    && matches!(
                        self.types.get(conversion.target_ty.semantic_id()),
                        Type::Primitive(Primitive::Textus | Primitive::Ascii)
                    )
        );
        let set_to_list = matches!(
            intrinsic,
            MirIntrinsic::Convert(conversion)
                if conversion.store.is_none()
                    && matches!(
                        self.types.get(conversion.source_ty.semantic_id()),
                        Type::Set(_)
                    )
                    && matches!(
                        self.types.get(conversion.target_ty.semantic_id()),
                        Type::Array(_)
                    )
        );
        let numeric = matches!(
            intrinsic,
            MirIntrinsic::Convert(conversion)
                if conversion.store.is_none()
                    && crate::lower::numeric_conversion(
                        self.types,
                        conversion.source_ty.semantic_id(),
                        conversion.target_ty.semantic_id(),
                    )
                    .is_some()
        );
        let type_check = matches!(
            intrinsic,
            MirIntrinsic::TypeCheck(op)
                if crate::lower::member_tests(self.types, op.ty.semantic_id()).is_some()
        );
        let sequence = matches!(
            intrinsic,
            MirIntrinsic::Convert(conversion)
                if conversion.store.is_none()
                    && crate::lower::sequence_conversion(
                        self.types,
                        conversion.source_ty.semantic_id(),
                        conversion.target_ty.semantic_id(),
                    )
                    .is_some()
        );
        let sermo_materialization = matches!(
            intrinsic,
            MirIntrinsic::Convert(conversion) if self.sermo_materialization(conversion)
        );
        renders_text || sermo_materialization || numeric || sequence || type_check || set_to_list
    }

    fn supported_open_conversion(&self, intrinsic: &MirIntrinsic) -> bool {
        let open_data = matches!(
            intrinsic,
            MirIntrinsic::Convert(conversion)
                if conversion.store.is_none()
                    && matches!(
                        self.types.get(conversion.source_ty.semantic_id()),
                        Type::Array(_) | Type::BoundedArray { .. } | Type::Map(..)
                    )
                    && matches!(
                        self.types.get(conversion.target_ty.semantic_id()),
                        Type::Primitive(Primitive::Json | Primitive::Valor)
                    )
        );
        // A leaf opener is typed `valor` in HIR while the operand keeps its
        // own scalar representation, so the `SermoSetOpener` conversion is an
        // identity at the type table. It boxes the payload the shims read, so
        // it is not the open-data gap the list and map conversions name.
        let open_identity = matches!(
            intrinsic,
            MirIntrinsic::Convert(conversion)
                if conversion.store.is_none()
                    && self.resolve_alias(conversion.source_ty.semantic_id())
                        == self.resolve_alias(conversion.target_ty.semantic_id())
                    && matches!(
                        self.types
                            .get(self.resolve_alias(conversion.target_ty.semantic_id())),
                        Type::Primitive(Primitive::Json | Primitive::Valor)
                    )
        );
        open_data || open_identity
    }

    fn supported_collection(op: MirCollectionOp) -> bool {
        matches!(
            op,
            MirCollectionOp::Length
                | MirCollectionOp::Append
                | MirCollectionOp::IsEmpty
                | MirCollectionOp::Index
                | MirCollectionOp::Contains
                | MirCollectionOp::Reverse
                | MirCollectionOp::Sort
                | MirCollectionOp::Put
                | MirCollectionOp::Delete
                | MirCollectionOp::Keys
                | MirCollectionOp::Values
                | MirCollectionOp::AppendImmutable
                | MirCollectionOp::Sum
                | MirCollectionOp::Compare
                | MirCollectionOp::SortByKeys
                | MirCollectionOp::Uppercase
                | MirCollectionOp::Lowercase
                | MirCollectionOp::Trim
                | MirCollectionOp::StartsWith
                | MirCollectionOp::EndsWith
                | MirCollectionOp::Split
                | MirCollectionOp::Replace
                | MirCollectionOp::Slice
                | MirCollectionOp::Take
                | MirCollectionOp::TakeLast
                | MirCollectionOp::DropFirst
                | MirCollectionOp::First
                | MirCollectionOp::Last
                | MirCollectionOp::RemoveFirst
                | MirCollectionOp::RemoveLast
                | MirCollectionOp::ReverseInPlace
                | MirCollectionOp::Floor
                | MirCollectionOp::Ceiling
                | MirCollectionOp::Round
                | MirCollectionOp::Truncate
                | MirCollectionOp::CordCursor
                | MirCollectionOp::TextEscape
                | MirCollectionOp::RegexMatches
                | MirCollectionOp::RegexFind
                | MirCollectionOp::RegexFindAll
                | MirCollectionOp::RegexSplit
                | MirCollectionOp::RegexReplace
                | MirCollectionOp::RegexReplaceEach
                | MirCollectionOp::MatchText
                | MirCollectionOp::MatchStart
                | MirCollectionOp::MatchEnd
                | MirCollectionOp::MatchGroup
                | MirCollectionOp::MatchNamed
                | MirCollectionOp::Absolute
                | MirCollectionOp::Signum
                | MirCollectionOp::Minimum
                | MirCollectionOp::Maximum
                | MirCollectionOp::Approximate
                | MirCollectionOp::ArgMax
                | MirCollectionOp::ArgMin
                | MirCollectionOp::ScalarMath(_)
                | MirCollectionOp::Union
                | MirCollectionOp::Intersection
                | MirCollectionOp::Difference
                | MirCollectionOp::SymmetricDifference
                | MirCollectionOp::IsSubset
                | MirCollectionOp::IsSuperset
        ) || Self::supported_lift_or_interval(op)
    }

    fn supported_lift_or_interval(op: MirCollectionOp) -> bool {
        matches!(
            op,
            MirCollectionOp::ArrayAdd
                | MirCollectionOp::ArraySub
                | MirCollectionOp::ArrayMul
                | MirCollectionOp::ArrayDiv
                | MirCollectionOp::ArrayMod
                | MirCollectionOp::ArrayTrueDiv
                | MirCollectionOp::ArrayNeg
                | MirCollectionOp::ArrayAnd
                | MirCollectionOp::ArrayOr
                | MirCollectionOp::ArrayNot
                | MirCollectionOp::ArrayLt
                | MirCollectionOp::ArrayGt
                | MirCollectionOp::ArrayLtEq
                | MirCollectionOp::ArrayGtEq
                | MirCollectionOp::ArrayValueEq
                | MirCollectionOp::ArrayValueNotEq
                | MirCollectionOp::IntervalLength
                | MirCollectionOp::IntervalContains
                | MirCollectionOp::IntervalClamp
                | MirCollectionOp::IntervalIntersect
                | MirCollectionOp::IntervalUnion
                | MirCollectionOp::IntervalMaterialize
        )
    }

    fn value_gap(value: &MirValue) -> Option<Gap> {
        match &value.kind {
            MirValueKind::Option(MirOptionOp::Chain { link, .. })
                if !matches!(
                    link,
                    MirOptionChainLink::Field(_)
                        | MirOptionChainLink::Index(_)
                        | MirOptionChainLink::Call {
                            callee: MirCallee::Value(_),
                            ..
                        }
                ) =>
            {
                Some(Gap::Subset("optional chain".to_owned()))
            }
            MirValueKind::Operand(_)
            | MirValueKind::Unary { .. }
            | MirValueKind::Binary { .. }
            | MirValueKind::Closure(_)
            | MirValueKind::Option(_) => None,
            MirValueKind::TensorExtent { .. } => Some(Gap::Policy("device value".to_owned())),
        }
    }
}

impl MirVisitor for ClifCapability<'_> {
    fn visit_function(&mut self, function: &MirFunction) {
        if let Some(reachable) = &self.reachable
            && !reachable.contains(&function.id)
        {
            return;
        }
        if function.is_generator && function.error_ty.is_some() {
            self.record(Gap::Subset("failable generator function".to_owned()));
        }
        if function.shader_stage.is_some() {
            self.record(Gap::Policy("shader function".to_owned()));
        }
        self.check_type(function.return_ty);
        if let Some(error_ty) = function.error_ty {
            self.check_type(error_ty);
        }
        radix_mir::visit::walk_function(self, function);
    }

    fn visit_param(&mut self, param: &MirParam) {
        self.check_type(param.ty);
    }

    // An unknown-typed slot (a catch binding on a path nothing reaches) has no
    // storage; a use of it declines when it is lowered.
    fn visit_local(&mut self, local: &MirLocal) {
        if !self.is_unknown(local.ty) {
            self.check_type(local.ty);
        }
    }

    fn visit_temp(&mut self, temp: &MirTemp) {
        if !self.is_unknown(temp.ty) {
            self.check_type(temp.ty);
        }
    }

    fn visit_value(&mut self, value: &MirValue) {
        if let Some(gap) = Self::value_gap(value) {
            self.record(gap);
        }
        let bit_op = match &value.kind {
            MirValueKind::Binary { op, .. } => matches!(
                op,
                MirBinOp::BitAnd
                    | MirBinOp::BitOr
                    | MirBinOp::BitXor
                    | MirBinOp::Shl
                    | MirBinOp::Shr
            ),
            MirValueKind::Unary { op, .. } => *op == MirUnOp::BitNot,
            _ => false,
        };
        // Bitwise work on raw-bits values lowers at the type's width; on
        // numbers it is not lowered yet.
        let integer_result = matches!(
            &value.kind,
            MirValueKind::Binary { .. } | MirValueKind::Unary { .. }
        ) && self.type_gap(value.ty).is_none()
            && matches!(
                self.types.get(value.ty.semantic_id()),
                Type::Primitive(Primitive::Numerus)
                    | Type::SizedNumeric(Primitive::Numerus, _)
                    | Type::ModularWord(_)
                    | Type::SaturatingWord(_)
            );
        if bit_op
            && !integer_result
            && !matches!(self.types.get(value.ty.semantic_id()), Type::Bits(_))
        {
            self.record(Gap::Subset("bit operation".to_owned()));
        }
        radix_mir::visit::walk_value(self, value);
    }

    fn visit_operand(&mut self, operand: &MirOperand) {
        if let MirOperand::Constant(constant) = operand
            && let Some(gap) = Self::constant_gap(constant)
        {
            self.record(gap);
        }
        radix_mir::visit::walk_operand(self, operand);
    }

    fn visit_place(&mut self, place: &MirPlace) {
        if place.projections.iter().any(|projection| {
            !matches!(
                projection,
                MirProjection::Index(_)
                    | MirProjection::ProvenIndex(_)
                    | MirProjection::VectorLane(_)
                    | MirProjection::Field(_)
                    | MirProjection::VariantField { .. }
                    | MirProjection::ClosureCapture { .. }
                    | MirProjection::SharedCell
            )
        }) {
            self.record(Gap::Subset("place projection".to_owned()));
        }
        radix_mir::visit::walk_place(self, place);
    }

    fn visit_runtime_call(&mut self, call: &MirRuntimeCall) {
        if let MirIntrinsic::Convert(conversion) = &call.intrinsic
            && conversion.store.is_some()
            && self.type_gap(conversion.source_ty).is_none()
            && self.type_gap(conversion.target_ty).is_none()
        {
            radix_mir::visit::walk_runtime_call(self, call);
            return;
        }
        // A stepped stream is lazy in the oracle: materializing it would change
        // what the program prints, so it declines by name.
        if let MirIntrinsic::CursorStream(stream) = &call.intrinsic
            && stream.stepped
        {
            self.record(Gap::Subset("stepped cursor stream".to_owned()));
            radix_mir::visit::walk_runtime_call(self, call);
            return;
        }
        if matches!(call.intrinsic, MirIntrinsic::Diagnostic(_)) && call.args.len() == 1 {
            radix_mir::visit::walk_runtime_call(self, call);
            return;
        }
        // Scalar to text rendering, interpolation, and text length. A
        // conversation handle is not a renderable scalar: `sermo ↦ textus`
        // materializes a frame and has no CLIF symbol yet, so it stays a gap.
        if self.supported_conversion(&call.intrinsic)
            || matches!(
                call.intrinsic,
                MirIntrinsic::FormatString { .. }
                    | MirIntrinsic::FormatValue { .. }
                    | MirIntrinsic::CursorStream(_)
                    | MirIntrinsic::Assert
                    | MirIntrinsic::Panic
                    | MirIntrinsic::Cede
                    | MirIntrinsic::Duplicate
                    | MirIntrinsic::SermoOpen
                    | MirIntrinsic::SermoSetOpener
                    | MirIntrinsic::Sermo(MirSermoOp::DrainCursor)
            )
            || matches!(&call.intrinsic, MirIntrinsic::Collection(op) if Self::supported_collection(*op))
        {
            radix_mir::visit::walk_runtime_call(self, call);
            return;
        }
        let name = self.intrinsic_name(&call.intrinsic);
        self.record(Gap::Subset(format!("runtime call {name}")));
        radix_mir::visit::walk_runtime_call(self, call);
    }

    fn visit_aggregate(&mut self, aggregate: &MirAggregate) {
        if let Some(gap) = Self::aggregate_gap(&aggregate.kind) {
            self.record(gap);
        }
        radix_mir::visit::walk_aggregate(self, aggregate);
    }

    fn visit_callee(&mut self, callee: &MirCallee) {
        match callee {
            MirCallee::Definition { source, .. } => {
                if !self.function_sources.contains(source) {
                    self.record(Gap::Abi("external definition call".to_owned()));
                }
            }
            MirCallee::Function(_) | MirCallee::Closure(_) | MirCallee::Value(_) => {}
        }
        radix_mir::visit::walk_callee(self, callee);
    }

    fn visit_terminator(&mut self, terminator: &MirTerminator) {
        match &terminator.kind {
            MirTerminatorKind::TryCall { .. }
            | MirTerminatorKind::ReturnError(_)
            | MirTerminatorKind::Return(_)
            | MirTerminatorKind::Goto(_)
            | MirTerminatorKind::Branch { .. }
            | MirTerminatorKind::Switch { .. }
            | MirTerminatorKind::Unreachable => {}
            MirTerminatorKind::ReturnThread => self.record(Gap::Subset("return_thread".to_owned())),
        }
        radix_mir::visit::walk_terminator(self, terminator);
    }
}

/// Classify a validated program against the CLIF target before emission.
#[must_use]
pub fn classify_clif_lowerability(validated: &ValidatedMir<'_>) -> Lowerability {
    ClifCapability::new(validated.validation().types.as_table())
        .classify_program(validated.program())
}

/// Classify only the functions reachable from `entry` (what the lowering will
/// actually emit). A program with no entry emits nothing, so it is capable.
#[must_use]
pub fn classify_clif_reachable(
    validated: &ValidatedMir<'_>,
    entry: Option<MirFunctionId>,
) -> Lowerability {
    let program = validated.program();
    let mut classifier = ClifCapability::new(validated.validation().types.as_table());
    classifier.reachable = Some(reachable_from(program, entry));
    classifier.classify_program(program)
}

/// Classify with an explicit type table (fixture entry; same walk).
#[must_use]
pub fn classify_with_types(program: &MirProgram, types: &TypeTable) -> Lowerability {
    ClifCapability::new(types).classify_program(program)
}
