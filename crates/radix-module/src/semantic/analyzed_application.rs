//! Target-neutral analyzed application contract.
//!
//! One coherent snapshot owns every compiler fact an application-lane HIR
//! consumer may rely on. Consumers must not mix HIR, types, resolver, effects,
//! or identity tables from different analysis runs.

use crate::hir::{HirModule, LibraryRegistry};
use crate::semantic::function_facts::{analyze_function_facts, FunctionFactTable};
use crate::semantic::qualified_identity::{build_qualified_identities, QualifiedIdentityTable};
use crate::semantic::resolved_use::{analyze_resolved_uses, ResolvedUseIndex};
use crate::semantic::{AnnotationContractMetadata, RadixLaneMetadata, Resolver, SemanticError, TypeTable};
use std::sync::atomic::{AtomicU64, Ordering};

/// Monotonic stamp proving fields of an [`AnalyzedApplication`] share one run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AnalysisStamp(u64);

impl AnalysisStamp {
    /// Allocate the next monotonic stamp for a freshly analyzed unit.
    fn next() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self(NEXT.fetch_add(1, Ordering::Relaxed))
    }

    /// Construct a stamp from a previously persisted value (artifact loader).
    #[must_use]
    pub fn from_persisted(stamp: u64) -> Self {
        Self(stamp)
    }

    #[must_use]
    pub fn get(self) -> u64 {
        self.0
    }
}

/// Incomplete or mixed-snapshot construction failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnalyzedApplicationError {
    /// Required HIR is missing (hard semantic failure).
    MissingHir,
    /// Function facts were not produced for a declared function.
    IncompleteFunctionFacts { def_id: crate::hir::DefId },
    /// Caller attempted to mix fields from different analysis stamps.
    StampMismatch { expected: u64, found: u64 },
    /// Required fact family is absent from the snapshot.
    MissingRequiredFacts { family: &'static str },
    /// `parts.semantic_diagnostics` contains one or more hard (fatal) entries.
    HardSemanticErrors { count: usize },
}

impl std::fmt::Display for AnalyzedApplicationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingHir => write!(f, "analyzed application requires lowered HIR"),
            Self::IncompleteFunctionFacts { def_id } => {
                write!(f, "missing function facts for DefId({})", def_id.0)
            }
            Self::StampMismatch { expected, found } => {
                write!(f, "analysis stamp mismatch: expected {expected}, found {found}")
            }
            Self::MissingRequiredFacts { family } => {
                write!(f, "analyzed application missing required facts: {family}")
            }
            Self::HardSemanticErrors { count } => {
                write!(f, "analyzed application contains {count} hard semantic error(s)")
            }
        }
    }
}

impl std::error::Error for AnalyzedApplicationError {}

/// Inputs produced by the semantic pipeline before fact-table construction.
pub struct AnalyzedApplicationParts {
    pub hir: HirModule,
    pub types: TypeTable,
    pub resolver: Resolver,
    pub annotation_contracts: AnnotationContractMetadata,
    pub libraries: LibraryRegistry,
    pub radix_lanes: RadixLaneMetadata,
    pub gpu_builtins: Vec<(crate::hir::DefId, radix_types::MirKernelBuiltin)>,
    pub semantic_diagnostics: Vec<SemanticError>,
}

/// Coherent target-neutral analyzed application snapshot.
///
/// Construction only succeeds through [`AnalyzedApplication::build`], which
/// stamps every fact family from the same analysis run. There is no optional
/// escape hatch for required facts.
pub struct AnalyzedApplication {
    stamp: AnalysisStamp,
    hir: HirModule,
    types: TypeTable,
    resolver: Resolver,
    annotation_contracts: AnnotationContractMetadata,
    libraries: LibraryRegistry,
    radix_lanes: RadixLaneMetadata,
    gpu_builtins: Vec<(crate::hir::DefId, radix_types::MirKernelBuiltin)>,
    function_facts: FunctionFactTable,
    resolved_uses: ResolvedUseIndex,
    qualified_identities: QualifiedIdentityTable,
    semantic_diagnostics: Vec<SemanticError>,
}

impl AnalyzedApplication {
    /// Build a coherent snapshot. Fails closed when required facts are incomplete.
    ///
    /// # Errors
    ///
    /// Returns [`AnalyzedApplicationError`] when required function facts are
    /// missing or incomplete for any top-level function or struct method.
    // public constructor: destructures parts into snapshot
    pub fn build(
        parts: AnalyzedApplicationParts,
        interner: &crate::lexer::Interner,
    ) -> Result<Self, AnalyzedApplicationError> {
        let function_facts = analyze_function_facts(&parts.hir, &parts.types);
        let qualified_identities = build_qualified_identities(&parts.libraries, interner);
        let resolved_uses = analyze_resolved_uses(&parts.hir, &parts.types, &qualified_identities);

        // Fail closed: every top-level function/method must have a fact row.
        for item in &parts.hir.items {
            match &item.kind {
                crate::hir::HirItemKind::Function(_) if !function_facts.has_fact(item.def_id) => {
                    return Err(AnalyzedApplicationError::IncompleteFunctionFacts { def_id: item.def_id });
                }
                crate::hir::HirItemKind::Struct(strukt) => {
                    for method in &strukt.methods {
                        if !function_facts.has_fact(method.def_id) {
                            return Err(AnalyzedApplicationError::IncompleteFunctionFacts { def_id: method.def_id });
                        }
                    }
                }
                _ => {}
            }
        }

        // Fail closed: hard semantic errors must not produce a valid snapshot.
        // The driver already gates on `semantic_result.success()`, but `build()`
        // is a public API that direct callers can bypass.
        let hard_error_count = parts
            .semantic_diagnostics
            .iter()
            .filter(|e| e.is_error())
            .count();
        if hard_error_count > 0 {
            return Err(AnalyzedApplicationError::HardSemanticErrors { count: hard_error_count });
        }

        Ok(Self {
            stamp: AnalysisStamp::next(),
            hir: parts.hir,
            types: parts.types,
            resolver: parts.resolver,
            annotation_contracts: parts.annotation_contracts,
            libraries: parts.libraries,
            radix_lanes: parts.radix_lanes,
            gpu_builtins: parts.gpu_builtins,
            function_facts,
            resolved_uses,
            qualified_identities,
            semantic_diagnostics: parts.semantic_diagnostics,
        })
    }

    #[must_use]
    pub fn stamp(&self) -> AnalysisStamp {
        self.stamp
    }

    #[must_use]
    pub fn hir(&self) -> &HirModule {
        &self.hir
    }

    #[must_use]
    pub fn types(&self) -> &TypeTable {
        &self.types
    }

    #[must_use]
    pub fn resolver(&self) -> &Resolver {
        &self.resolver
    }

    #[must_use]
    pub fn annotation_contracts(&self) -> &AnnotationContractMetadata {
        &self.annotation_contracts
    }

    #[must_use]
    pub fn libraries(&self) -> &LibraryRegistry {
        &self.libraries
    }

    #[must_use]
    pub fn radix_lanes(&self) -> &RadixLaneMetadata {
        &self.radix_lanes
    }

    #[must_use]
    pub fn gpu_builtins(&self) -> &[(crate::hir::DefId, radix_types::MirKernelBuiltin)] {
        &self.gpu_builtins
    }

    #[must_use]
    pub fn function_facts(&self) -> &FunctionFactTable {
        &self.function_facts
    }

    #[must_use]
    pub fn resolved_uses(&self) -> &ResolvedUseIndex {
        &self.resolved_uses
    }

    #[must_use]
    pub fn qualified_identities(&self) -> &QualifiedIdentityTable {
        &self.qualified_identities
    }

    #[must_use]
    pub fn semantic_diagnostics(&self) -> &[SemanticError] {
        &self.semantic_diagnostics
    }

    /// Require that a borrowed subrecord carries this snapshot's stamp.
    ///
    /// # Errors
    ///
    /// Returns [`AnalyzedApplicationError::StampMismatch`] when the stamp
    /// does not match this snapshot's stamp.
    pub fn require_stamp(&self, stamp: AnalysisStamp) -> Result<(), AnalyzedApplicationError> {
        if stamp != self.stamp {
            return Err(AnalyzedApplicationError::StampMismatch { expected: self.stamp.get(), found: stamp.get() });
        }
        Ok(())
    }

    /// Destructure into driver-owned fields while preserving coherence stamp.
    #[must_use]
    pub fn into_driver_fields(self) -> AnalyzedApplicationDriverFields {
        AnalyzedApplicationDriverFields {
            stamp: self.stamp,
            hir: self.hir,
            types: self.types,
            resolver: self.resolver,
            annotation_contracts: self.annotation_contracts,
            libraries: self.libraries,
            radix_lanes: self.radix_lanes,
            gpu_builtins: self.gpu_builtins,
            function_facts: self.function_facts,
            resolved_uses: self.resolved_uses,
            qualified_identities: self.qualified_identities,
            semantic_diagnostics: self.semantic_diagnostics,
        }
    }
}

/// Flattened fields for embedding into [`crate::driver::AnalyzedModule`].
pub struct AnalyzedApplicationDriverFields {
    pub stamp: AnalysisStamp,
    pub hir: HirModule,
    pub types: TypeTable,
    pub resolver: Resolver,
    pub annotation_contracts: AnnotationContractMetadata,
    pub libraries: LibraryRegistry,
    pub radix_lanes: RadixLaneMetadata,
    pub gpu_builtins: Vec<(crate::hir::DefId, radix_types::MirKernelBuiltin)>,
    pub function_facts: FunctionFactTable,
    pub resolved_uses: ResolvedUseIndex,
    pub qualified_identities: QualifiedIdentityTable,
    pub semantic_diagnostics: Vec<SemanticError>,
}

/// Target qualification result companion to an analyzed application.
///
/// Qualification never mutates the base snapshot; it records target-local
/// support/erase/warn/reject decisions, stable H1 gap slugs, and notes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetQualification {
    pub target: crate::codegen::Target,
    pub base_stamp: AnalysisStamp,
    pub status: QualificationStatus,
    /// Stable machine-readable gap slugs (G5 H1 honesty); empty when capable.
    pub gap_slugs: Vec<&'static str>,
    pub notes: Vec<String>,
}

/// Outcome of qualifying an analyzed application for one target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QualificationStatus {
    Supported,
    SupportedWithLimitations,
    Unsupported,
}

/// Qualify a base analyzed application for a concrete target.
///
/// Combines (1) pre-emission HIR coverage classification (`classify_hir_coverage`)
/// with stable gap slugs and (2) Rust borrow/lifetime diagnostics when present.
/// Never emits or scans target code.
pub fn qualify_for_target(
    application: &AnalyzedApplication,
    target: crate::codegen::Target,
    borrow_diagnostics: &[SemanticError],
) -> TargetQualification {
    let mut gap_slugs = Vec::new();
    let mut notes = Vec::new();
    let mut status = QualificationStatus::Supported;

    if let Some(hir_target) = crate::codegen::coverage::from_target(target) {
        let coverage =
            crate::codegen::coverage::classify_hir_coverage(hir_target, application.hir(), application.types());
        match coverage {
            radix_types::Lowerability::Capable => {
                notes.push(format!("{} H1 coverage classifier: capable", target_label(target)));
            }
            radix_types::Lowerability::Rejected(gaps) => {
                for gap in &gaps {
                    gap_slugs.push(gap.slug());
                    notes.push(format!("{} H1 gap `{}` ({})", target_label(target), gap.slug(), gap.shape()));
                }
                status = QualificationStatus::Unsupported;
            }
        }
    } else {
        status = QualificationStatus::SupportedWithLimitations;
        notes.push(format!(
            "{} is outside the HIR application-lane product set for G5",
            target_label(target)
        ));
    }

    let has_borrow_errors = borrow_diagnostics
        .iter()
        .any(super::error::SemanticError::is_error);
    if matches!(target, crate::codegen::Target::HirRust) {
        if has_borrow_errors {
            gap_slugs.push("rust_target_borrow_lifetime_rejected");
            notes.push("rust borrow/lifetime qualification rejected the unit".to_owned());
            status = QualificationStatus::Unsupported;
        } else {
            notes.push("rust borrow/lifetime qualification passed".to_owned());
        }
    }

    gap_slugs.sort_unstable();
    gap_slugs.dedup();

    TargetQualification { target, base_stamp: application.stamp(), status, gap_slugs, notes }
}

fn target_label(target: crate::codegen::Target) -> &'static str {
    match target {
        crate::codegen::Target::MirStepper => "stepper",
        crate::codegen::Target::HirRust => "rust",
        crate::codegen::Target::HirTypeScript => "ts",
        crate::codegen::Target::HirGo => "go",
        crate::codegen::Target::HirSwift => "swift",
        crate::codegen::Target::HirPython => "python",
        crate::codegen::Target::HirHaskell => "haskell",
        crate::codegen::Target::HirFaber => "faber",
        crate::codegen::Target::MirWasm => "wasm-text",
        crate::codegen::Target::MirLlvm => "llvm-text",
        crate::codegen::Target::MirLlvmHost => "llvm-host",
        crate::codegen::Target::MirAmd => "amd",
        crate::codegen::Target::MirMetal => "metal-text",
        crate::codegen::Target::MirWgsl => "wgsl-text",
        crate::codegen::Target::MirSexp => "sexp",
        crate::codegen::Target::MirScena => "scena",
        crate::codegen::Target::MirFmir => "fmir-text",
        crate::codegen::Target::MirFmirBinary => "fmir",
        crate::codegen::Target::MirFmirBundle => "fmir-bin",
        crate::codegen::Target::HirFhir => "fhir",
    }
}

#[cfg(test)]
#[path = "analyzed_application_test.rs"]
mod tests;
