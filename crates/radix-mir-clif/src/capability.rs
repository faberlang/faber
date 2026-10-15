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
use radix_types::{Primitive, Type, TypeTable};
use rustc_hash::FxHashSet;

/// Slug for shapes the CLIF subset does not cover yet.
pub const SLUG_CLIF_SUBSET: &str = "clif-subset";
/// Slug for shapes CLIF rejects by policy (device family, net leaves).
pub const SLUG_CLIF_POLICY: &str = "clif-policy";

/// A gap kind plus its stable shape text.
enum Gap {
    Subset(&'static str),
    Policy(&'static str),
    Layout(&'static str),
    Abi(&'static str),
}

impl Gap {
    fn into_capability_gap(self) -> CapabilityGap {
        match self {
            Self::Subset(shape) => CapabilityGap::UnsupportedMirShape {
                shape: shape.to_owned(),
                slug: SLUG_CLIF_SUBSET,
            },
            Self::Policy(shape) => CapabilityGap::TargetPolicyRejection {
                shape: shape.to_owned(),
                slug: SLUG_CLIF_POLICY,
            },
            Self::Layout(shape) => CapabilityGap::MissingLayoutFact {
                shape: shape.to_owned(),
                slug: "missing-layout-fact",
            },
            Self::Abi(shape) => CapabilityGap::MissingAbiFact {
                shape: shape.to_owned(),
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
    gaps: Vec<CapabilityGap>,
}

impl<'a> ClifCapability<'a> {
    fn new(types: &'a TypeTable) -> Self {
        Self {
            types,
            function_sources: FxHashSet::default(),
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
            | Type::SizedNumeric(..)
            | Type::PolicyFloat(..) => None,
            Type::Primitive(Primitive::Textus) | Type::Primitive(Primitive::Ascii) => {
                Some(Gap::Subset("string value"))
            }
            Type::Primitive(Primitive::Littera) => Some(Gap::Subset("character value")),
            Type::Primitive(Primitive::Octeti) | Type::BoundedOcteti { .. } => {
                Some(Gap::Subset("byte buffer value"))
            }
            Type::BoundedTextus { .. } | Type::BoundedAscii { .. } => {
                Some(Gap::Subset("bounded buffer value"))
            }
            Type::Primitive(Primitive::Regex)
            | Type::Primitive(Primitive::Match)
            | Type::Primitive(Primitive::Json)
            | Type::Primitive(Primitive::Valor) => Some(Gap::Subset("dynamic data value")),
            Type::Primitive(Primitive::Instans) | Type::SizedInstans(_) => {
                Some(Gap::Subset("instans value"))
            }
            Type::Primitive(Primitive::Nihil) => Some(Gap::Subset("none value")),
            Type::Primitive(Primitive::Ignotum) => Some(Gap::Subset("unknown value")),
            Type::Array(_)
            | Type::BoundedArray { .. }
            | Type::Queue(_)
            | Type::BoundedQueue { .. }
            | Type::Stack(_)
            | Type::BoundedStack { .. }
            | Type::Map(_, _)
            | Type::Ledger(_, _)
            | Type::Set(_) => Some(Gap::Subset("collection value")),
            Type::Record(_) | Type::Struct(_) => Some(Gap::Subset("record value")),
            Type::Enum(_) | Type::Interface(_) => Some(Gap::Subset("enum value")),
            Type::Tuple(_) => Some(Gap::Subset("tuple value")),
            Type::Union(_) | Type::Intersection(_) => Some(Gap::Subset("union value")),
            Type::Option(_) => Some(Gap::Subset("optional value")),
            Type::Func(_) => Some(Gap::Subset("function value")),
            Type::Ref(..) => Some(Gap::Subset("reference value")),
            Type::ModularWord(_) => Some(Gap::Subset("wrapping value")),
            Type::SaturatingWord(_) => Some(Gap::Subset("saturating value")),
            Type::Cursor(_)
            | Type::AsyncCursor(_, _)
            | Type::Promissum(_)
            | Type::PromissumFailable(_, _) => Some(Gap::Subset("async value")),
            Type::Intervallum(_) => Some(Gap::Subset("interval value")),
            Type::Tensor(..)
            | Type::Vector(..)
            | Type::Matrix(..)
            | Type::Sparsa(..)
            | Type::Atomic(_) => Some(Gap::Policy("device value")),
            Type::TensorEdgePolicy(_) => Some(Gap::Policy("device value")),
            Type::Param(_) | Type::Infer(_) | Type::InferUnion(_) | Type::Error => {
                Some(Gap::Layout("unresolved type"))
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
                                    Some(Gap::Layout("unresolved type"))
                                }
                                _ => self.type_gap(probe),
                            };
                        }
                    }
                }
            }
            _ => Some(Gap::Subset("unsupported type")),
        }
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
            MirConstant::String(_) | MirConstant::Ascii(_) => Some(Gap::Subset("string constant")),
            MirConstant::Octeti(_) => Some(Gap::Subset("byte constant")),
            MirConstant::Regex { .. } => Some(Gap::Subset("regex constant")),
            MirConstant::Nil => Some(Gap::Subset("none constant")),
            MirConstant::Function(_) => Some(Gap::Subset("function constant")),
            MirConstant::Magnus(_) => Some(Gap::Subset("inf constant")),
            MirConstant::ShapeSize { .. } => Some(Gap::Policy("device constant")),
        }
    }

    fn aggregate_gap(kind: &MirAggregateKind) -> Option<Gap> {
        match kind {
            MirAggregateKind::Tuple => Some(Gap::Subset("tuple construction")),
            MirAggregateKind::Array => Some(Gap::Subset("list construction")),
            MirAggregateKind::Map => Some(Gap::Subset("map construction")),
            MirAggregateKind::Set => Some(Gap::Subset("set construction")),
            MirAggregateKind::Record => Some(Gap::Subset("record construction")),
            MirAggregateKind::Struct(_) => Some(Gap::Subset("struct construction")),
            MirAggregateKind::EnumVariant(_) => Some(Gap::Subset("enum variant construction")),
            MirAggregateKind::ClosureEnvironment(_) => Some(Gap::Subset("closure construction")),
            MirAggregateKind::SharedCell => Some(Gap::Subset("shared cell construction")),
            MirAggregateKind::Tensor
            | MirAggregateKind::SparseTensor
            | MirAggregateKind::Vector
            | MirAggregateKind::Matrix => Some(Gap::Policy("device construction")),
            MirAggregateKind::Interval { .. } => Some(Gap::Subset("interval construction")),
        }
    }

    fn value_gap(value: &MirValue) -> Option<Gap> {
        match &value.kind {
            MirValueKind::Operand(_) | MirValueKind::Unary { .. } | MirValueKind::Binary { .. } => {
                None
            }
            MirValueKind::Closure(_) => Some(Gap::Subset("closure value")),
            MirValueKind::Option(_) => Some(Gap::Subset("optional value")),
            MirValueKind::TensorExtent { .. } => Some(Gap::Policy("device value")),
        }
    }
}

impl MirVisitor for ClifCapability<'_> {
    fn visit_function(&mut self, function: &MirFunction) {
        if function.is_async {
            self.record(Gap::Subset("async function"));
        }
        if function.is_generator {
            self.record(Gap::Subset("generator function"));
        }
        if function.shader_stage.is_some() {
            self.record(Gap::Policy("shader function"));
        }
        if function.error_ty.is_some() {
            self.record(Gap::Subset("error-typed function"));
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

    fn visit_local(&mut self, local: &MirLocal) {
        self.check_type(local.ty);
    }

    fn visit_temp(&mut self, temp: &MirTemp) {
        self.check_type(temp.ty);
    }

    fn visit_value(&mut self, value: &MirValue) {
        if let Some(gap) = Self::value_gap(value) {
            self.record(gap);
        }
        if let MirValueKind::Binary { op, .. } = &value.kind
            && matches!(
                op,
                MirBinOp::BitAnd
                    | MirBinOp::BitOr
                    | MirBinOp::BitXor
                    | MirBinOp::Shl
                    | MirBinOp::Shr
            )
        {
            self.record(Gap::Subset("bit operation"));
        }
        if let MirValueKind::Binary {
            op: MirBinOp::Coalesce,
            ..
        } = &value.kind
        {
            self.record(Gap::Subset("optional value"));
        }
        if let MirValueKind::Unary {
            op: MirUnOp::IsNil, ..
        } = &value.kind
        {
            self.record(Gap::Subset("optional value"));
        }
        if let MirValueKind::Unary {
            op: MirUnOp::BitNot,
            ..
        } = &value.kind
        {
            self.record(Gap::Subset("bit operation"));
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
        if !place.projections.is_empty() {
            self.record(Gap::Subset("place projection"));
        }
        radix_mir::visit::walk_place(self, place);
    }

    fn visit_runtime_call(&mut self, call: &MirRuntimeCall) {
        self.record(Gap::Subset("runtime call"));
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
                    self.record(Gap::Abi("external definition call"));
                }
            }
            MirCallee::Closure(_) => {
                self.record(Gap::Subset("closure call"));
            }
            MirCallee::Value(_) => {
                self.record(Gap::Subset("indirect call"));
            }
        }
        radix_mir::visit::walk_callee(self, callee);
    }

    fn visit_terminator(&mut self, terminator: &MirTerminator) {
        match &terminator.kind {
            MirTerminatorKind::TryCall { .. } => self.record(Gap::Subset("try_call")),
            MirTerminatorKind::ReturnError(_) => self.record(Gap::Subset("error return")),
            MirTerminatorKind::Return(_)
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

/// Classify with an explicit type table (fixture entry; same walk).
#[must_use]
pub fn classify_with_types(program: &MirProgram, types: &TypeTable) -> Lowerability {
    ClifCapability::new(types).classify_program(program)
}
