//! Target-neutral analyzed application contract.
//!
//! One coherent snapshot owns every compiler fact an application-lane HIR
//! consumer may rely on. Consumers must not mix HIR, types, resolver, effects,
//! or identity tables from different analysis runs.

use crate::access_facts::AccessFactTable;
use crate::function_facts::{FunctionFactTable, analyze_function_facts};
use crate::qualified_identity::{QualifiedIdentityTable, build_qualified_identities};
use crate::resolved_use::{ResolvedUseIndex, analyze_resolved_uses};
use crate::{AnnotationContractMetadata, RadixLaneMetadata, Resolver, SemanticError, TypeTable};
use radix_hir::{HirModule, LibraryRegistry};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};

/// Package-local import identity facts for user-code cross-module calls.
///
/// Supplied by package-aware callers (the CLI compile path, package harnesses)
/// so MIR lowering can resolve user-code provider imports to their canonical
/// `(product, module_path, item)` identities (campaign D-PA1). `None` keeps
/// the legacy provider routing: raw import spelling + interner indices, which
/// are meaningless across the module boundary.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PackageImportIdentities {
    /// Package identity: manifest `package.name`, or package-root-derived
    /// fallback (the package root directory name).
    pub product: String,
    /// This file's canonical module segments (frontmatter `group`, else
    /// source-root-relative path with `.fab`/`main.fab`/`mod.fab`
    /// normalization) — mirror of `module_segments_for_file`.
    pub module_segments: Vec<String>,
    /// Resolved target module segments per import binding (`binding →
    /// target file's canonical module segments`) for local sibling imports —
    /// mirror of `resolve_import` + `module_segments_for_file`.
    pub imports: BTreeMap<String, Vec<String>>,
    /// Body-bearing file interface per import binding, for imports whose
    /// target exports at least one device function (DFV2-3). Recorded by the
    /// same package-identity pass that fills `imports` and read by the MIR
    /// provider-route seam. Per-unit by construction: the map rides this
    /// value through `ModuleCompileRequest` into its analyzed application and
    /// dies with it, so an interface resolved for one unit can never serve
    /// another package's or module's unit (S01-7).
    pub device_interfaces: BTreeMap<String, crate::file_interface::FileInterface>,
}

/// Borrowed view of the HIR v1 analyzed-application contract consumed by MIR lowering.
#[derive(Clone, Copy)]
pub struct AnalyzedModuleApplicationView<'a> {
    pub stamp: AnalysisStamp,
    pub hir: &'a HirModule,
    pub types: &'a TypeTable,
    pub shape_obligations: &'a crate::shape_obligations::ShapeObligationTable,
    pub kernel_requirements: &'a crate::passes::kernel_requirements::KernelRequirementsReport,
    pub tensor_edge_policies: &'a radix_hir::TensorEdgePolicyTable,
    pub resolver: &'a Resolver,
    pub annotation_contracts: &'a AnnotationContractMetadata,
    pub libraries: &'a LibraryRegistry,
    pub function_facts: &'a FunctionFactTable,
    pub resolved_uses: &'a ResolvedUseIndex,
    pub qualified_identities: &'a QualifiedIdentityTable,
    pub package_import_identities: Option<&'a PackageImportIdentities>,
    pub provenance: &'a crate::capability::FunctionProvenanceTable,
    pub radix_lanes: &'a RadixLaneMetadata,
    pub gpu_builtins: &'a [(radix_hir::DefId, radix_types::MirKernelBuiltin)],
    pub cli_program: Option<&'a crate::cli::CliProgram>,
    pub display_tokens: &'a radix_runtime_contract::display_tokens::DisplayTokens,
    pub interner: &'a radix_lexer::Interner,
}

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
    AbsentHir,
    /// Function facts were not produced for a declared function.
    IncompleteFunctionFacts { def_id: radix_hir::DefId },
    /// Caller attempted to mix fields from different analysis stamps.
    StampMismatch { expected: u64, found: u64 },
    /// Required fact family is absent from the snapshot.
    AbsentRequiredFacts { family: &'static str },
    /// A borrow-returning function's provenance summary could not be tied to
    /// the region engine's facts (SCR-04).
    IncoherentProvenance { def_id: radix_hir::DefId },
    /// `parts.semantic_diagnostics` contains one or more hard (fatal) entries.
    HardSemanticErrors { count: usize },
}

impl std::fmt::Display for AnalyzedApplicationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AbsentHir => write!(f, "analyzed application requires lowered HIR"),
            Self::IncompleteFunctionFacts { def_id } => {
                write!(f, "missing function facts for DefId({})", def_id.0)
            }
            Self::StampMismatch { expected, found } => {
                write!(
                    f,
                    "analysis stamp mismatch: expected {expected}, found {found}"
                )
            }
            Self::AbsentRequiredFacts { family } => {
                write!(f, "analyzed application missing required facts: {family}")
            }
            Self::IncoherentProvenance { def_id } => {
                write!(f, "incoherent borrow provenance for DefId({})", def_id.0)
            }
            Self::HardSemanticErrors { count } => {
                write!(
                    f,
                    "analyzed application contains {count} hard semantic error(s)"
                )
            }
        }
    }
}

impl std::error::Error for AnalyzedApplicationError {}

/// Inputs produced by the semantic pipeline, including its one fact-table
/// handoff.
pub struct AnalyzedApplicationParts {
    pub hir: HirModule,
    pub types: TypeTable,
    pub resolver: Resolver,
    pub annotation_contracts: AnnotationContractMetadata,
    pub libraries: LibraryRegistry,
    pub radix_lanes: RadixLaneMetadata,
    pub gpu_builtins: Vec<(radix_hir::DefId, radix_types::MirKernelBuiltin)>,
    /// Resolved shifted-view policies and generic forwarding facts from this HIR run.
    pub tensor_edge_policies: radix_hir::TensorEdgePolicyTable,
    /// Per-target inferred device requirements from the same typed-HIR run.
    pub kernel_requirements: crate::passes::kernel_requirements::KernelRequirementsReport,
    pub access_facts: AccessFactTable,
    /// Region facts carried from the pipeline's single region-engine run on
    /// this same analysis snapshot; `build` never re-runs the engine.
    ///
    /// `None` means the engine could not run (unsupported control-flow
    /// shape); reduction then fails closed for borrow-returning functions.
    /// Supplying facts from a different run is a snapshot-coherence violation
    /// and fails closed at the reduction boundary.
    pub region_facts: Option<crate::passes::borrow_region::RegionFacts>,
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
    gpu_builtins: Vec<(radix_hir::DefId, radix_types::MirKernelBuiltin)>,
    tensor_edge_policies: radix_hir::TensorEdgePolicyTable,
    kernel_requirements: crate::passes::kernel_requirements::KernelRequirementsReport,
    access_facts: AccessFactTable,
    function_facts: FunctionFactTable,
    resolved_uses: ResolvedUseIndex,
    qualified_identities: QualifiedIdentityTable,
    provenance: crate::capability::FunctionProvenanceTable,
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
        interner: &radix_lexer::Interner,
    ) -> Result<Self, AnalyzedApplicationError> {
        let access_facts = parts.access_facts;
        let function_facts = analyze_function_facts(&parts.hir, &parts.types);
        let qualified_identities = build_qualified_identities(&parts.libraries, interner);
        let resolved_uses = analyze_resolved_uses(&parts.hir, &parts.types, &qualified_identities);

        // Fail closed: every top-level function/method must have a fact row.
        for item in &parts.hir.items {
            match &item.kind {
                radix_hir::HirItemKind::Function(_) if !function_facts.has_fact(item.def_id) => {
                    return Err(AnalyzedApplicationError::IncompleteFunctionFacts {
                        def_id: item.def_id,
                    });
                }
                radix_hir::HirItemKind::Struct(strukt) => {
                    for method in &strukt.methods {
                        if !function_facts.has_fact(method.def_id) {
                            return Err(AnalyzedApplicationError::IncompleteFunctionFacts {
                                def_id: method.def_id,
                            });
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
            return Err(AnalyzedApplicationError::HardSemanticErrors {
                count: hard_error_count,
            });
        }

        // SCR-04: reduce the region facts carried from the pipeline's single
        // engine run on this same snapshot — `build` never re-runs the engine.
        // Absent facts (the engine rejected an unsupported shape) and facts
        // from a foreign run share the one reduction and its fail-closed law.
        let regions = parts.region_facts;
        let provenance = match crate::passes::borrow_region::reduce(
            &parts.hir,
            &parts.types,
            regions.as_ref(),
        ) {
            Ok(table) => table,
            Err(crate::passes::borrow_region::ProvenanceError::Unavailable { .. }) => {
                return Err(AnalyzedApplicationError::AbsentRequiredFacts {
                    family: "borrow_provenance",
                });
            }
            Err(crate::passes::borrow_region::ProvenanceError::Incoherent { function }) => {
                return Err(AnalyzedApplicationError::IncoherentProvenance { def_id: function });
            }
        };

        Ok(Self {
            stamp: AnalysisStamp::next(),
            hir: parts.hir,
            types: parts.types,
            resolver: parts.resolver,
            annotation_contracts: parts.annotation_contracts,
            libraries: parts.libraries,
            radix_lanes: parts.radix_lanes,
            gpu_builtins: parts.gpu_builtins,
            tensor_edge_policies: parts.tensor_edge_policies,
            kernel_requirements: parts.kernel_requirements,
            access_facts,
            function_facts,
            resolved_uses,
            qualified_identities,
            provenance,
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
    pub fn gpu_builtins(&self) -> &[(radix_hir::DefId, radix_types::MirKernelBuiltin)] {
        &self.gpu_builtins
    }

    #[must_use]
    pub fn tensor_edge_policies(&self) -> &radix_hir::TensorEdgePolicyTable {
        &self.tensor_edge_policies
    }

    #[must_use]
    pub fn access_facts(&self) -> &AccessFactTable {
        &self.access_facts
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
    pub fn provenance(&self) -> &crate::capability::FunctionProvenanceTable {
        &self.provenance
    }

    #[must_use]
    pub fn semantic_diagnostics(&self) -> &[SemanticError] {
        &self.semantic_diagnostics
    }

    /// Per-target device requirements inferred from this exact HIR snapshot.
    #[must_use]
    pub fn kernel_requirements(
        &self,
    ) -> &crate::passes::kernel_requirements::KernelRequirementsReport {
        &self.kernel_requirements
    }

    /// Require that a borrowed subrecord carries this snapshot's stamp.
    ///
    /// # Errors
    ///
    /// Returns [`AnalyzedApplicationError::StampMismatch`] when the stamp
    /// does not match this snapshot's stamp.
    pub fn require_stamp(&self, stamp: AnalysisStamp) -> Result<(), AnalyzedApplicationError> {
        if stamp != self.stamp {
            return Err(AnalyzedApplicationError::StampMismatch {
                expected: self.stamp.get(),
                found: stamp.get(),
            });
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
            tensor_edge_policies: self.tensor_edge_policies,
            kernel_requirements: self.kernel_requirements,
            access_facts: self.access_facts,
            function_facts: self.function_facts,
            resolved_uses: self.resolved_uses,
            qualified_identities: self.qualified_identities,
            provenance: self.provenance,
            semantic_diagnostics: self.semantic_diagnostics,
        }
    }
}

/// Flattened fields for embedding into the driver-owned `AnalyzedModule`.
pub struct AnalyzedApplicationDriverFields {
    pub stamp: AnalysisStamp,
    pub hir: HirModule,
    pub types: TypeTable,
    pub resolver: Resolver,
    pub annotation_contracts: AnnotationContractMetadata,
    pub libraries: LibraryRegistry,
    pub radix_lanes: RadixLaneMetadata,
    pub gpu_builtins: Vec<(radix_hir::DefId, radix_types::MirKernelBuiltin)>,
    pub tensor_edge_policies: radix_hir::TensorEdgePolicyTable,
    pub kernel_requirements: crate::passes::kernel_requirements::KernelRequirementsReport,
    pub access_facts: AccessFactTable,
    pub function_facts: FunctionFactTable,
    pub resolved_uses: ResolvedUseIndex,
    pub qualified_identities: QualifiedIdentityTable,
    pub provenance: crate::capability::FunctionProvenanceTable,
    pub semantic_diagnostics: Vec<SemanticError>,
}

/// Target qualification result companion to an analyzed application.
///
/// Qualification never mutates the base snapshot; it records target-local
/// support/erase/warn/reject decisions, stable H1 gap slugs, and notes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetQualification {
    pub target: radix_target::Target,
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
    target: radix_target::Target,
    borrow_diagnostics: &[SemanticError],
) -> TargetQualification {
    let mut gap_slugs = Vec::new();
    let mut notes = Vec::new();
    let mut status = QualificationStatus::Supported;

    if let Some(hir_target) = crate::coverage::from_target(target) {
        let coverage = crate::coverage::classify_hir_coverage(
            hir_target,
            application.hir(),
            application.types(),
        );
        match coverage {
            radix_types::Lowerability::Capable => {
                notes.push(format!(
                    "{} H1 coverage classifier: capable",
                    target_label(target)
                ));
            }
            radix_types::Lowerability::Rejected(gaps) => {
                for gap in &gaps {
                    gap_slugs.push(gap.slug());
                    notes.push(format!(
                        "{} H1 gap `{}` ({})",
                        target_label(target),
                        gap.slug(),
                        gap.shape()
                    ));
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
    if matches!(target, radix_target::Target::HirRust) {
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

    TargetQualification {
        target,
        base_stamp: application.stamp(),
        status,
        gap_slugs,
        notes,
    }
}

fn target_label(target: radix_target::Target) -> &'static str {
    target.as_str()
}
