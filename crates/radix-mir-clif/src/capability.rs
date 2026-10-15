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

use radix_hir::DefId;
use radix_mir::visit::MirVisitor;
use radix_mir::*;
use radix_types::vocabulary::{CapabilityGap, Lowerability};
use radix_types::{NumericWidth, Primitive, Type, TypeTable};
use rustc_hash::{FxHashMap, FxHashSet};

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
    gaps: Vec<CapabilityGap>,
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
            Type::Primitive(Primitive::Numerus)
            | Type::Primitive(Primitive::Fractus)
            | Type::Primitive(Primitive::Bivalens)
            | Type::Primitive(Primitive::Vacuum)
            | Type::Primitive(Primitive::Numquam)
            | Type::SizedNumeric(Primitive::Numerus, _)
            | Type::Primitive(Primitive::Textus)
            | Type::Bits(_)
            | Type::SizedNumeric(Primitive::Fractus, NumericWidth::F64) => None,
            Type::SizedNumeric(Primitive::Fractus, width) if width.is_float() => None,
            Type::SizedNumeric(..) => Some(Gap::Subset("sized numeric value".to_owned())),
            Type::PolicyFloat(..) => Some(Gap::Subset("policy float value".to_owned())),
            Type::Primitive(Primitive::Ascii) => Some(Gap::Subset("string value".to_owned())),
            Type::Primitive(Primitive::Littera) => Some(Gap::Subset("character value".to_owned())),
            Type::Primitive(Primitive::Octeti) | Type::BoundedOcteti { .. } => {
                Some(Gap::Subset("byte buffer value".to_owned()))
            }
            Type::BoundedTextus { .. } | Type::BoundedAscii { .. } => {
                Some(Gap::Subset("bounded buffer value".to_owned()))
            }
            Type::Primitive(Primitive::Regex)
            | Type::Primitive(Primitive::Match)
            | Type::Primitive(Primitive::Json)
            | Type::Primitive(Primitive::Valor) => {
                Some(Gap::Subset("dynamic data value".to_owned()))
            }
            Type::Primitive(Primitive::Instans) | Type::SizedInstans(_) => {
                Some(Gap::Subset("instans value".to_owned()))
            }
            Type::Primitive(Primitive::Nihil) => Some(Gap::Subset("none value".to_owned())),
            Type::Primitive(Primitive::Ignotum) => Some(Gap::Subset("unknown value".to_owned())),
            Type::Array(element) => self.type_gap(MirType::semantic(*element)),
            Type::BoundedArray { element, .. } => self.type_gap(MirType::semantic(*element)),
            Type::Map(key, value) => self
                .type_gap(MirType::semantic(*key))
                .or_else(|| self.type_gap(MirType::semantic(*value))),
            Type::Set(element) => self.type_gap(MirType::semantic(*element)),
            Type::Queue(_)
            | Type::BoundedQueue { .. }
            | Type::Stack(_)
            | Type::BoundedStack { .. }
            | Type::Ledger(_, _) => Some(Gap::Subset("collection value".to_owned())),
            Type::Struct(_) => None,
            Type::Record(fields) => fields
                .values()
                .find_map(|field| self.type_gap(MirType::semantic(*field))),
            Type::Enum(_) => None,
            Type::Interface(_) => Some(Gap::Subset("enum value".to_owned())),
            Type::Tuple(elements) => elements
                .iter()
                .find_map(|element| self.type_gap(MirType::semantic(*element))),
            Type::Union(_) | Type::Intersection(_) => Some(Gap::Subset("union value".to_owned())),
            Type::Option(inner) => match self.types.get(*inner) {
                Type::Option(_) => Some(Gap::Subset("optional value".to_owned())),
                _ => self.type_gap(MirType::semantic(*inner)),
            },
            Type::Func(sig) => {
                if !sig.type_params.is_empty()
                    || sig.is_async
                    || sig.is_generator
                    || sig.err.is_some()
                {
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
            }
            Type::Ref(..) => Some(Gap::Subset("reference value".to_owned())),
            Type::ModularWord(width) | Type::SaturatingWord(width)
                if width.integer_range().is_some() =>
            {
                None
            }
            Type::ModularWord(_) => Some(Gap::Subset("wrapping value".to_owned())),
            Type::SaturatingWord(_) => Some(Gap::Subset("saturating value".to_owned())),
            Type::Promissum(inner) => self.type_gap(MirType::semantic(*inner)),
            Type::Cursor(_) | Type::AsyncCursor(_, _) | Type::PromissumFailable(_, _) => {
                Some(Gap::Subset("async value".to_owned()))
            }
            Type::Intervallum(_) => Some(Gap::Subset("interval value".to_owned())),
            Type::Tensor(..)
            | Type::Vector(..)
            | Type::Matrix(..)
            | Type::Sparsa(..)
            | Type::Atomic(_) => Some(Gap::Policy("device value".to_owned())),
            Type::TensorEdgePolicy(_) => Some(Gap::Policy("device value".to_owned())),
            Type::Param(_) | Type::Infer(_) | Type::InferUnion(_) | Type::Error => {
                Some(Gap::Layout("unresolved type".to_owned()))
            }
            Type::Alias(_, inner) => {
                let mut inner = *inner;
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
            _ => Some(Gap::Subset("unsupported type".to_owned())),
        }
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
            MirConstant::Int(_) | MirConstant::UInt(_) | MirConstant::Float(_) => None,
            MirConstant::Bool(_) | MirConstant::Unit => None,
            MirConstant::String(_) => None,
            MirConstant::Ascii(_) => Some(Gap::Subset("string constant".to_owned())),
            MirConstant::Octeti(_) => Some(Gap::Subset("byte constant".to_owned())),
            MirConstant::Regex { .. } => Some(Gap::Subset("regex constant".to_owned())),
            MirConstant::Nil => None,
            MirConstant::Function(_) => None,
            MirConstant::Magnus(_) => Some(Gap::Subset("inf constant".to_owned())),
            MirConstant::ShapeSize { .. } => Some(Gap::Policy("device constant".to_owned())),
        }
    }

    fn aggregate_gap(kind: &MirAggregateKind) -> Option<Gap> {
        match kind {
            MirAggregateKind::Tuple => None,
            MirAggregateKind::Array => None,
            MirAggregateKind::Map | MirAggregateKind::Set => None,
            MirAggregateKind::Record => None,
            MirAggregateKind::Struct(_) => None,
            MirAggregateKind::EnumVariant(_) => None,
            MirAggregateKind::ClosureEnvironment(_) => {
                Some(Gap::Subset("closure construction".to_owned()))
            }
            MirAggregateKind::SharedCell => {
                Some(Gap::Subset("shared cell construction".to_owned()))
            }
            MirAggregateKind::Tensor
            | MirAggregateKind::SparseTensor
            | MirAggregateKind::Vector
            | MirAggregateKind::Matrix => Some(Gap::Policy("device construction".to_owned())),
            MirAggregateKind::Interval { .. } => {
                Some(Gap::Subset("interval construction".to_owned()))
            }
        }
    }

    fn value_gap(value: &MirValue) -> Option<Gap> {
        match &value.kind {
            MirValueKind::Operand(_) | MirValueKind::Unary { .. } | MirValueKind::Binary { .. } => {
                None
            }
            MirValueKind::Closure(_) => Some(Gap::Subset("closure value".to_owned())),
            MirValueKind::Option(MirOptionOp::Chain {
                link: MirOptionChainLink::Field(_) | MirOptionChainLink::Index(_),
                ..
            }) => None,
            MirValueKind::Option(MirOptionOp::Chain { .. }) => {
                Some(Gap::Subset("optional chain".to_owned()))
            }
            MirValueKind::Option(_) => None,
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
                    | MirProjection::Field(_)
                    | MirProjection::VariantField { .. }
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
        if matches!(call.intrinsic, MirIntrinsic::Diagnostic(_)) && call.args.len() == 1 {
            radix_mir::visit::walk_runtime_call(self, call);
            return;
        }
        // Scalar to text rendering, interpolation, and text length.
        let renders_text = matches!(
            &call.intrinsic,
            MirIntrinsic::Convert(conversion)
                if conversion.store.is_none()
                    && self.type_gap(conversion.source_ty).is_none()
                    && matches!(
                        self.types.get(conversion.target_ty.semantic_id()),
                        Type::Primitive(Primitive::Textus)
                    )
        );
        let set_to_list = matches!(
            &call.intrinsic,
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
        // `fractus(int)`: the exact widening of an integer to a float.
        let int_to_float = matches!(
            &call.intrinsic,
            MirIntrinsic::Convert(conversion)
                if conversion.store.is_none()
                    && matches!(
                        self.types.get(conversion.source_ty.semantic_id()),
                        Type::Primitive(Primitive::Numerus)
                    )
                    && matches!(
                        self.types.get(conversion.target_ty.semantic_id()),
                        Type::Primitive(Primitive::Fractus)
                    )
        );
        let int_to_int = matches!(
            &call.intrinsic,
            MirIntrinsic::Convert(conversion)
                if conversion.store.is_none()
                    && crate::lower::int_conversion(
                        self.types,
                        conversion.source_ty.semantic_id(),
                        conversion.target_ty.semantic_id(),
                    )
                    .is_some()
        );
        let is_float = |ty: MirType| {
            matches!(
                self.types.get(ty.semantic_id()),
                Type::Primitive(Primitive::Fractus)
            ) || matches!(
                self.types.get(ty.semantic_id()),
                Type::SizedNumeric(Primitive::Fractus, width) if width.is_float()
            )
        };
        let float_to_float = matches!(
            &call.intrinsic,
            MirIntrinsic::Convert(conversion)
                if conversion.store.is_none()
                    && is_float(conversion.source_ty)
                    && is_float(conversion.target_ty)
        );
        if renders_text
            || float_to_float
            || int_to_int
            || int_to_float
            || set_to_list
            || matches!(
                call.intrinsic,
                MirIntrinsic::FormatString { .. }
                    | MirIntrinsic::CursorStream(_)
                    | MirIntrinsic::Assert
                    | MirIntrinsic::Panic
                    | MirIntrinsic::Cede
                    | MirIntrinsic::Duplicate
                    | MirIntrinsic::Collection(
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
                            | MirCollectionOp::Absolute
                            | MirCollectionOp::Signum
                            | MirCollectionOp::Minimum
                            | MirCollectionOp::Maximum
                    )
            )
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
            MirCallee::Function(_) => {}
            MirCallee::Definition { source, .. } => {
                if !self.function_sources.contains(source) {
                    self.record(Gap::Abi("external definition call".to_owned()));
                }
            }
            MirCallee::Closure(_) => {
                self.record(Gap::Subset("closure call".to_owned()));
            }
            MirCallee::Value(_) => {}
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
