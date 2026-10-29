//! Structural lowerability classifier for the MIR runner (scena).
//!
//! TARGET: pre-execution capability check mirroring the runtime `unsupported`
//! sites in [`super::runtime`] and [`super`] statement dispatch, so the fast
//! coverage matrix and callers receive a classified [`CapabilityGap`] verdict
//! without executing the program.
//!
//! WHY: before this module, the runner discovered unsupported MIR shapes ad
//! hoc at runtime through `FaberScript unsupported: ...` strings. LLVM and Wasm
//! emit targets already classify lowerability structurally through
//! [`CapabilityGap`]. This module gives the runner the same vocabulary,
//! closing SA-001.
//!
//! BOUNDARY: structural lowerability only — *can the runner dispatch this MIR
//! shape?* It walks the statement shapes the runtime explicitly rejects
//! (unhandled [`MirIntrinsic`] families, the `Vector` aggregate construct,
//! `VectorLane` / `ClosureCapture` projection writes) plus the entry-function
//! existence check from [`super::run_entry`]. It does NOT duplicate semantic
//! typechecking (owned by [`MirValidationContext`]) and does NOT prove
//! behavioral success: a `Capable` verdict means no structural gap was found;
//! run-tier execution may still reject behaviorally (e.g. a provider sym the
//! host cannot resolve, numerus overflow, tensor element-count policy).
//!
//! INVARIANT: structural-rejected ⊇ shape-level run-tier-rejected. Behavioral
//! run-tier rejections (overflow, provider resolution, element-count policy)
//! are intentionally out of scope. Over-rejection — a gap in code the runtime
//! never reaches — is acceptable and conservative: the structural tier is a
//! lower bound on run-tier capability, never an upper bound.

use handroll::FxHashSet;
use radix_mir::names::MirNames;
use radix_mir::{
    CapabilityGap, Lowerability, MirAggregateFields, MirAggregateItem, MirCallee, MirIntrinsic,
    MirOperand, MirOptionChainLink, MirOptionOp, MirPlace, MirProjection, MirProviderKind,
    MirRuntimeAbi, MirRuntimeAbiFamily, MirRuntimeCall, MirStatementKind, MirTerminatorKind,
    MirValueKind, ValidatedMir,
};

/// Slug for an unsupported MIR shape (mirrors `mir::coverage`).
pub(crate) const SLUG_UNSUPPORTED: &str = "unsupported-mir-shape";
/// Slug for a target-policy rejection (mirrors `mir::coverage`).
pub(crate) const SLUG_POLICY: &str = "target-policy-rejection";

/// Runner dispatch status for a [`MirRuntimeAbiFamily`].
///
/// WHY: the runtime-boundary ledger's runner column. Each family is in exactly
/// one status; the exhaustive match in [`runner_dispatch_status`] is the
/// compile-time guard that a new family cannot land without a ledger row.
///
/// Companion: `docs/factory/stepper-alignment/stepper-runtime-ledger.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunnerDispatchStatus {
    /// Runner evaluates the intrinsic directly in-process.
    Implement,
    /// Runner delegates to the [`super::Host`] boundary (kernel/provider/IO).
    HostDelegate,
    /// Runner rejects by policy (e.g. device lane); not script-mode debt.
    IntentionalReject,
    /// Runner does not yet dispatch; tracked deferred debt (Stage 4 / ad factory).
    Defer,
}

/// Ledger dispatch status for a runtime ABI family under the runner.
///
/// GUARD: this match is exhaustive over [`MirRuntimeAbiFamily`]. Adding a new
/// family variant without assigning a status is a compile error, which forces a
/// row in the runtime-boundary ledger's runner column. The table test
/// `every_runtime_family_has_a_runner_ledger_status` pins the mapping so a
/// reclassification is a deliberate change.
#[must_use]
pub fn runner_dispatch_status(family: MirRuntimeAbiFamily) -> RunnerDispatchStatus {
    match family {
        MirRuntimeAbiFamily::RuntimeDiagnostic
        | MirRuntimeAbiFamily::RuntimeAssert
        | MirRuntimeAbiFamily::RuntimeFormat
        | MirRuntimeAbiFamily::RuntimeConvert
        | MirRuntimeAbiFamily::RuntimeCollection
        | MirRuntimeAbiFamily::RuntimePanic
        | MirRuntimeAbiFamily::RuntimeReadLine
        | MirRuntimeAbiFamily::RuntimeCede
        | MirRuntimeAbiFamily::RuntimeDeviceBlock
        | MirRuntimeAbiFamily::RuntimeCursorStream
        | MirRuntimeAbiFamily::RuntimeSermo
        | MirRuntimeAbiFamily::RuntimeGradient
        | MirRuntimeAbiFamily::RuntimeTypeCheck
        | MirRuntimeAbiFamily::RuntimeTensorShapeCheck
        | MirRuntimeAbiFamily::RuntimeTensorShapeSize
        | MirRuntimeAbiFamily::RuntimeTensorLimes
        | MirRuntimeAbiFamily::RuntimeDuplicate
        | MirRuntimeAbiFamily::RuntimeTensorLift
        | MirRuntimeAbiFamily::RuntimePow
        | MirRuntimeAbiFamily::RuntimeScan => RunnerDispatchStatus::Implement,
        MirRuntimeAbiFamily::RuntimeProvider => RunnerDispatchStatus::HostDelegate,
        // SFR-5: the filum sum declaration is device-lane only; the host
        // script runner rejects it by target policy.
        MirRuntimeAbiFamily::RuntimeFilumReduce
        | MirRuntimeAbiFamily::RuntimeVectorOp
        | MirRuntimeAbiFamily::RuntimeAtomic
        | MirRuntimeAbiFamily::RuntimeGpuBuiltin => RunnerDispatchStatus::IntentionalReject,
    }
}

/// `true` when the runner dispatches `family` in script mode (`Implement` or
/// `HostDelegate`). Single source of truth shared by the classifier and the
/// runtime-boundary ledger.
fn family_dispatchable_in_script(family: MirRuntimeAbiFamily) -> bool {
    matches!(
        runner_dispatch_status(family),
        RunnerDispatchStatus::Implement | RunnerDispatchStatus::HostDelegate
    )
}

/// Classify whether the MIR runner can structurally lower `program` for
/// script execution.
///
/// Returns [`Lowerability::Capable`] when no structural gap is found. This is a
/// necessary, not sufficient, condition for run-tier success: behavioral
/// authority belongs to the runner runtime and `Host` dispatch.
#[must_use]
pub fn classify_runner_lowerability(validated: &ValidatedMir<'_>) -> Lowerability {
    let program = validated.program();
    let validation = validated.validation();
    let mut gaps: Vec<CapabilityGap> = Vec::new();
    let mut seen: FxHashSet<String> = FxHashSet::default();

    // WHY: entry-function absence is the first rejection site in `run_entry`.
    // Structural classification must mirror it so the structural tier never
    // approves a program the run tier rejects for lacking an incipit.
    if let Some(interner) = validation.interner.as_interner() {
        let names = MirNames::new(program, validation.types.as_table(), interner);
        if names.explicit_entry_function().is_none() {
            push_gap(
                &mut gaps,
                &mut seen,
                CapabilityGap::UnsupportedMirShape {
                    shape: radix_mir::names::NO_ENTRY_FUNCTION.to_owned(),
                    slug: SLUG_UNSUPPORTED,
                },
            );
        }
    }

    for function in &program.functions {
        for block in &function.blocks {
            for statement in &block.statements {
                classify_statement(&statement.kind, &mut gaps, &mut seen);
            }
            classify_terminator(&block.terminator.kind, &mut gaps, &mut seen);
        }
    }

    if gaps.is_empty() {
        Lowerability::Capable
    } else {
        Lowerability::Rejected(gaps)
    }
}

fn classify_statement(
    kind: &MirStatementKind,
    gaps: &mut Vec<CapabilityGap>,
    seen: &mut FxHashSet<String>,
) {
    match kind {
        MirStatementKind::Assign { place, value } => {
            classify_value(&value.kind, gaps, seen);
            classify_place_write(place, gaps, seen);
        }
        // E3: a conversion arm is lowered as an ordinary call in the
        // propagate error block, so the existing call capability row is the
        // runner inventory entry; no conversion-specific opcode is needed.
        MirStatementKind::Call {
            destination,
            callee,
            args,
        } => {
            classify_callee(callee, gaps, seen);
            classify_operands(args, gaps, seen);
            if let Some(place) = destination {
                classify_place_write(place, gaps, seen);
            }
        }
        MirStatementKind::RuntimeCall { destination, call } => {
            classify_runtime_call(call, gaps, seen);
            classify_operands(&call.args, gaps, seen);
            if let Some(place) = destination {
                classify_place_write(place, gaps, seen);
            }
        }
        MirStatementKind::Construct {
            destination,
            aggregate,
        } => {
            classify_aggregate_fields(&aggregate.fields, gaps, seen);
            classify_place_write(destination, gaps, seen);
        }
    }
}

fn classify_runtime_call(
    call: &MirRuntimeCall,
    gaps: &mut Vec<CapabilityGap>,
    seen: &mut FxHashSet<String>,
) {
    classify_intrinsic(&call.intrinsic, gaps, seen);
    // Sparse and dense tensors share the runner's `Value::Tensor` carrier.
    // The declared target type still controls sparse operations, so a
    // tensor↔sparsa conversion is structurally dispatchable and must not be
    // rejected merely because no separate runtime carrier exists.
}

fn classify_terminator(
    kind: &MirTerminatorKind,
    gaps: &mut Vec<CapabilityGap>,
    seen: &mut FxHashSet<String>,
) {
    match kind {
        MirTerminatorKind::Return(operand) => {
            if let Some(operand) = operand {
                classify_operand(operand, gaps, seen);
            }
        }
        MirTerminatorKind::ReturnError(operand) => classify_operand(operand, gaps, seen),
        MirTerminatorKind::TryCall {
            destination,
            callee,
            args,
            error_place,
            ..
        } => {
            classify_callee(callee, gaps, seen);
            classify_operands(args, gaps, seen);
            if let Some(destination) = destination {
                classify_place_write(destination, gaps, seen);
            }
            classify_place_write(error_place, gaps, seen);
        }
        MirTerminatorKind::Goto(_) => {}
        MirTerminatorKind::Branch { condition, .. } => classify_operand(condition, gaps, seen),
        MirTerminatorKind::Switch { value, .. } => classify_operand(value, gaps, seen),
        MirTerminatorKind::ReturnThread => push_gap(
            gaps,
            seen,
            CapabilityGap::UnsupportedMirShape {
                shape: "return_thread terminator".to_owned(),
                slug: SLUG_UNSUPPORTED,
            },
        ),
        MirTerminatorKind::Unreachable => push_gap(
            gaps,
            seen,
            CapabilityGap::UnsupportedMirShape {
                shape: "unreachable terminator".to_owned(),
                slug: SLUG_UNSUPPORTED,
            },
        ),
    }
}

/// Classify a runtime intrinsic by family against the supported script-mode set.
fn classify_intrinsic(
    intrinsic: &MirIntrinsic,
    gaps: &mut Vec<CapabilityGap>,
    seen: &mut FxHashSet<String>,
) {
    if let MirIntrinsic::Provider(provider) = intrinsic
        && provider.kind == MirProviderKind::Package
    {
        push_gap(
            gaps,
            seen,
            CapabilityGap::UnsupportedMirShape {
                shape: "package provider has no runner host bridge".to_owned(),
                slug: SLUG_UNSUPPORTED,
            },
        );
        return;
    }
    let family = MirRuntimeAbi::from_intrinsic(intrinsic).family;
    if family_dispatchable_in_script(family) {
        return;
    }
    let shape = match family {
        // EDGE: shapes mirror the runtime `unsupported: intrinsic <Variant>`
        // strings for the families that appear in the live corpus, so the
        // structural and run tiers share a histogram vocabulary.
        MirRuntimeAbiFamily::RuntimeCede => "intrinsic Cede",
        MirRuntimeAbiFamily::RuntimeCursorStream => "intrinsic CursorStream",
        MirRuntimeAbiFamily::RuntimeSermo => "intrinsic Sermo",
        MirRuntimeAbiFamily::RuntimeAtomic => "intrinsic Atomic",
        MirRuntimeAbiFamily::RuntimeGpuBuiltin => "intrinsic GpuBuiltin",
        MirRuntimeAbiFamily::RuntimeFilumReduce => "intrinsic FilumReduce",
        _ => "intrinsic",
    };
    let gap = if matches!(
        family,
        MirRuntimeAbiFamily::RuntimeAtomic
            | MirRuntimeAbiFamily::RuntimeGpuBuiltin
            | MirRuntimeAbiFamily::RuntimeFilumReduce
    ) {
        // WHY: GPU builtins, atomics and the filum reduction declaration
        // belong to storage/device lanes; host script mode rejects them by
        // policy, not by missing implementation.
        CapabilityGap::TargetPolicyRejection {
            shape: shape.to_owned(),
            slug: SLUG_POLICY,
        }
    } else {
        CapabilityGap::UnsupportedMirShape {
            shape: shape.to_owned(),
            slug: SLUG_UNSUPPORTED,
        }
    };
    push_gap(gaps, seen, gap);
}

#[allow(clippy::match_same_arms)]
fn classify_value(
    kind: &MirValueKind,
    gaps: &mut Vec<CapabilityGap>,
    seen: &mut FxHashSet<String>,
) {
    match kind {
        MirValueKind::Operand(operand) => classify_operand(operand, gaps, seen),
        MirValueKind::Unary { operand, .. } => classify_operand(operand, gaps, seen),
        MirValueKind::TensorExtent { tensor, .. } => classify_operand(tensor, gaps, seen),
        MirValueKind::Binary { lhs, rhs, .. } => {
            classify_operand(lhs, gaps, seen);
            classify_operand(rhs, gaps, seen);
        }
        MirValueKind::Option(op) => classify_option_op(op, gaps, seen),
        MirValueKind::Closure(closure) => classify_operand(&closure.environment, gaps, seen),
    }
}

fn classify_option_op(
    op: &MirOptionOp,
    gaps: &mut Vec<CapabilityGap>,
    seen: &mut FxHashSet<String>,
) {
    match op {
        MirOptionOp::None => {}
        MirOptionOp::Some(operand)
        | MirOptionOp::IsNil(operand)
        | MirOptionOp::IsNotNil(operand)
        | MirOptionOp::Unwrap { value: operand, .. } => classify_operand(operand, gaps, seen),
        MirOptionOp::Coalesce { value, fallback } => {
            classify_operand(value, gaps, seen);
            classify_operand(fallback, gaps, seen);
        }
        MirOptionOp::Chain { base, link } => {
            classify_operand(base, gaps, seen);
            classify_option_chain_link(link, gaps, seen);
        }
    }
}

fn classify_option_chain_link(
    link: &MirOptionChainLink,
    gaps: &mut Vec<CapabilityGap>,
    seen: &mut FxHashSet<String>,
) {
    match link {
        MirOptionChainLink::Field(_) | MirOptionChainLink::VariantField { .. } => {}
        MirOptionChainLink::Index(index) => classify_operand(index, gaps, seen),
        MirOptionChainLink::Call { callee, args } => {
            classify_callee(callee, gaps, seen);
            classify_operands(args, gaps, seen);
            push_gap(
                gaps,
                seen,
                CapabilityGap::UnsupportedMirShape {
                    shape: "option chain call".to_owned(),
                    slug: SLUG_UNSUPPORTED,
                },
            );
        }
    }
}

fn classify_callee(
    callee: &MirCallee,
    gaps: &mut Vec<CapabilityGap>,
    seen: &mut FxHashSet<String>,
) {
    match callee {
        MirCallee::Function(_) | MirCallee::Definition { .. } | MirCallee::Closure(_) => {}
        MirCallee::Value(operand) => classify_operand(operand, gaps, seen),
    }
}

fn classify_operands(
    operands: &[MirOperand],
    gaps: &mut Vec<CapabilityGap>,
    seen: &mut FxHashSet<String>,
) {
    for operand in operands {
        classify_operand(operand, gaps, seen);
    }
}

fn classify_operand(
    operand: &MirOperand,
    gaps: &mut Vec<CapabilityGap>,
    seen: &mut FxHashSet<String>,
) {
    if let MirOperand::Place(place) = operand {
        classify_place_read(place, gaps, seen);
    }
}

fn classify_aggregate_fields(
    fields: &MirAggregateFields,
    gaps: &mut Vec<CapabilityGap>,
    seen: &mut FxHashSet<String>,
) {
    match fields {
        MirAggregateFields::Ordered(items) => {
            for item in items {
                match item {
                    MirAggregateItem::Operand(operand) | MirAggregateItem::Spread(operand) => {
                        classify_operand(operand, gaps, seen);
                    }
                }
            }
        }
        MirAggregateFields::Named(items) => {
            for item in items {
                classify_operand(&item.value, gaps, seen);
            }
        }
        MirAggregateFields::Keyed(items) => {
            for item in items {
                classify_operand(&item.key, gaps, seen);
                classify_operand(&item.value, gaps, seen);
            }
        }
    }
}

/// Classify projection reads the runtime rejects in `project`.
fn classify_place_read(
    place: &MirPlace,
    gaps: &mut Vec<CapabilityGap>,
    seen: &mut FxHashSet<String>,
) {
    for projection in &place.projections {
        if let MirProjection::Index(index) | MirProjection::ProvenIndex(index) = projection {
            classify_operand(index, gaps, seen);
        }
    }
}

/// Classify projection writes the runtime rejects in `project_write`.
fn classify_place_write(
    place: &MirPlace,
    gaps: &mut Vec<CapabilityGap>,
    seen: &mut FxHashSet<String>,
) {
    for projection in &place.projections {
        let shape = match projection {
            MirProjection::ClosureCapture { .. } => "closure capture projection write",
            _ => continue,
        };
        push_gap(
            gaps,
            seen,
            CapabilityGap::UnsupportedMirShape {
                shape: shape.to_owned(),
                slug: SLUG_UNSUPPORTED,
            },
        );
    }
}

fn push_gap(gaps: &mut Vec<CapabilityGap>, seen: &mut FxHashSet<String>, gap: CapabilityGap) {
    // WHY: dedup by shape so a repeated gap (e.g. many Cede calls) collapses to
    // a single classified entry, keeping the verdict readable and stable.
    if seen.insert(gap.shape().to_owned()) {
        gaps.push(gap);
    }
}
