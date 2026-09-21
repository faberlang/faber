//! HIR artifact loader for deserializing pre-analyzed compilation units.
//!
//! This module owns the public entry point for loading a
//! [`radix_hir_fhir::HirArtifact`] from a `postcard` binary and converting it
//! into an [`AnalyzedModule`]. The schema version gate (F3) and
//! referential-integrity validation (U2.3) run inside the `radix-hir-fhir`
//! format crate; this module only reconstructs arenas from the validated
//! artifact (U2.2).

use crate::driver::AnalyzedModule;
use crate::driver::GraphicsSourceFacts;
use crate::semantic::{
    AnnotationContractMetadata, CaptureFact, CaptureMode, FunctionFact, FunctionFactTable, FunctionProvenanceTable,
    QualifiedIdentityTable, ResolvedUse, ResolvedUseIndex, ResolvedUseKey, ResolvedUseKind, Resolver,
};
use radix_hir_fhir::{
    CaptureFactWire, CaptureModeWire, CliProgramWire, FunctionFactTableWire, FunctionFactWire, GpuBuiltinWire,
    HirArtifactError, RadixLaneWire, ResolvedUseKeyWire, ResolvedUseKindWire, ResolvedUseWire,
};
use radix_lexer::Interner;
use radix_types::TypeTable;
use rustc_hash::{FxHashMap, FxHashSet};
use std::collections::BTreeMap;

// ---------------------------------------------------------------------------
// Wire → live type mappings (F18, F20)
// ---------------------------------------------------------------------------

/// Map a wire GPU kernel builtin to the live `MirKernelBuiltin` (F13, F20).
fn map_gpu_builtin(wire: GpuBuiltinWire) -> radix_types::MirKernelBuiltin {
    match wire {
        GpuBuiltinWire::GlobalInvocationX => radix_types::MirKernelBuiltin::GlobalInvocationX,
        GpuBuiltinWire::GlobalInvocationY => radix_types::MirKernelBuiltin::GlobalInvocationY,
        GpuBuiltinWire::GlobalInvocationZ => radix_types::MirKernelBuiltin::GlobalInvocationZ,
        GpuBuiltinWire::GlobalInvocationId => radix_types::MirKernelBuiltin::GlobalInvocationId,
        GpuBuiltinWire::WorkgroupIdX => radix_types::MirKernelBuiltin::WorkgroupIdX,
        GpuBuiltinWire::WorkgroupIdY => radix_types::MirKernelBuiltin::WorkgroupIdY,
        GpuBuiltinWire::WorkgroupIdZ => radix_types::MirKernelBuiltin::WorkgroupIdZ,
        GpuBuiltinWire::WorkgroupId => radix_types::MirKernelBuiltin::WorkgroupId,
        GpuBuiltinWire::LocalInvocationIdX => radix_types::MirKernelBuiltin::LocalInvocationIdX,
        GpuBuiltinWire::LocalInvocationIdY => radix_types::MirKernelBuiltin::LocalInvocationIdY,
        GpuBuiltinWire::LocalInvocationIdZ => radix_types::MirKernelBuiltin::LocalInvocationIdZ,
        GpuBuiltinWire::LocalInvocationId => radix_types::MirKernelBuiltin::LocalInvocationId,
        GpuBuiltinWire::WorkgroupSizeX => radix_types::MirKernelBuiltin::WorkgroupSizeX,
        GpuBuiltinWire::WorkgroupSizeY => radix_types::MirKernelBuiltin::WorkgroupSizeY,
        GpuBuiltinWire::WorkgroupSizeZ => radix_types::MirKernelBuiltin::WorkgroupSizeZ,
    }
}

/// Map a wire resolved-use key to the live type (F20).
fn map_resolved_use_key(wire: ResolvedUseKeyWire) -> ResolvedUseKey {
    match wire {
        ResolvedUseKeyWire::Local(def_id) => ResolvedUseKey::Local(def_id),
        ResolvedUseKeyWire::Portable(key) => ResolvedUseKey::Portable(key),
    }
}

/// Map a wire resolved-use kind to the live type (F20).
fn map_resolved_use_kind(wire: ResolvedUseKindWire) -> ResolvedUseKind {
    match wire {
        ResolvedUseKindWire::Path => ResolvedUseKind::Path,
        ResolvedUseKindWire::Call => ResolvedUseKind::Call,
        ResolvedUseKindWire::MethodCall => ResolvedUseKind::MethodCall,
        ResolvedUseKindWire::TypeRef => ResolvedUseKind::TypeRef,
        ResolvedUseKindWire::ImportBinding => ResolvedUseKind::ImportBinding,
    }
}

/// Map a wire resolved use to the live type (F20).
fn map_resolved_use(wire: ResolvedUseWire) -> ResolvedUse {
    ResolvedUse {
        key: map_resolved_use_key(wire.key),
        kind: map_resolved_use_kind(wire.kind),
        enclosing: wire.enclosing,
    }
}

/// Map a wire capture mode to the live type (F20).
fn map_capture_mode(wire: CaptureModeWire) -> CaptureMode {
    match wire {
        CaptureModeWire::Read => CaptureMode::Read,
        CaptureModeWire::Mutate => CaptureMode::Mutate,
        CaptureModeWire::Consume => CaptureMode::Consume,
    }
}

/// Map a wire capture fact to the live type (F20).
fn map_capture_fact(wire: CaptureFactWire) -> CaptureFact {
    CaptureFact { def_id: wire.def_id, mode: map_capture_mode(wire.mode) }
}

/// Map a wire function fact to the live type (F20).
fn map_function_fact(wire: FunctionFactWire) -> FunctionFact {
    FunctionFact {
        direct_failure: wire.has_direct_failure,
        may_fail: wire.may_fail,
        is_async: wire.is_async,
        is_generator: wire.is_generator,
        requires_await: wire.requires_await,
        call_edges: wire.call_edges.into_iter().collect::<FxHashSet<_>>(),
        captures: wire.captures.into_iter().map(map_capture_fact).collect(),
        param_modes: wire.param_modes.into_iter().collect::<FxHashMap<_, _>>(),
        err_ty: wire.err_ty,
    }
}

/// Map a wire function-fact table to the live type (F20).
fn map_function_fact_table(wire: FunctionFactTableWire) -> FunctionFactTable {
    FunctionFactTable::from_raw(
        wire.entries
            .into_iter()
            .map(|(def_id, fact)| (def_id, map_function_fact(fact)))
            .collect(),
        wire.entry.map(map_function_fact),
    )
}

/// Map a wire provenance table to the live type (SCR-04).
fn map_provenance_table(wire: radix_hir_fhir::FunctionProvenanceTableWire) -> FunctionProvenanceTable {
    use radix_hir_fhir::{FunctionProvenanceWire, LastUseWire, RegionRelationshipWire, ReturnedBorrowWire};
    use radix_types::{RegionRelationship, ReturnedBorrowProvenance};

    fn map_row(row: FunctionProvenanceWire) -> crate::semantic::FunctionProvenanceSummary {
        crate::semantic::FunctionProvenanceSummary {
            function: row.function,
            returned_borrow: match row.returned_borrow {
                ReturnedBorrowWire::None => ReturnedBorrowProvenance::None,
                ReturnedBorrowWire::Parameter(def_id) => ReturnedBorrowProvenance::Parameter(def_id),
                ReturnedBorrowWire::Definition(def_id) => ReturnedBorrowProvenance::Definition(def_id),
            },
            required_regions: row
                .required_regions
                .into_iter()
                .map(|rel: RegionRelationshipWire| RegionRelationship { from: rel.from, to: rel.to })
                .collect(),
            last_uses: row
                .last_uses
                .into_iter()
                .map(|fact: LastUseWire| crate::semantic::LastUseFact { borrower: fact.borrower, site: fact.site })
                .collect(),
        }
    }

    FunctionProvenanceTable::from_rows(wire.functions.into_iter().map(map_row).collect())
}

/// Map a wire CLI program to the live `CliProgram` (F18).
fn map_cli_program(wire: CliProgramWire) -> crate::cli::CliProgram {
    use radix_hir_fhir::{
        CliCommandWire, CliDefaultWire, CliExitWire, CliModeWire, CliOperandWire, CliOptionWire, CliTypeWire,
    };

    fn map_cli_mode(w: CliModeWire) -> crate::cli::CliMode {
        match w {
            CliModeWire::NotCli => crate::cli::CliMode::NotCli,
            CliModeWire::SingleCommand => crate::cli::CliMode::SingleCommand,
            CliModeWire::Subcommand => crate::cli::CliMode::Subcommand,
        }
    }

    fn map_cli_type(w: CliTypeWire) -> crate::cli::CliType {
        match w {
            CliTypeWire::Textus => crate::cli::CliType::Textus,
            CliTypeWire::Numerus => crate::cli::CliType::Numerus,
            CliTypeWire::Fractus => crate::cli::CliType::Fractus,
            CliTypeWire::Bivalens => crate::cli::CliType::Bivalens,
            CliTypeWire::Octeti => crate::cli::CliType::Octeti,
            CliTypeWire::Ignotum => crate::cli::CliType::Ignotum,
            CliTypeWire::ListaTextus => crate::cli::CliType::ListaTextus,
            CliTypeWire::ListaNumerus => crate::cli::CliType::ListaNumerus,
        }
    }

    fn map_cli_default(w: CliDefaultWire) -> crate::cli::CliDefault {
        match w {
            CliDefaultWire::Text(s) => crate::cli::CliDefault::Text(s),
            CliDefaultWire::Integer(i) => crate::cli::CliDefault::Integer(i),
            CliDefaultWire::Float(f) => crate::cli::CliDefault::Float(f),
            CliDefaultWire::Bool(b) => crate::cli::CliDefault::Bool(b),
            CliDefaultWire::Nil => crate::cli::CliDefault::Nil,
            CliDefaultWire::Expr(s) => crate::cli::CliDefault::Expr(s),
        }
    }

    fn map_cli_exit(w: CliExitWire) -> crate::cli::CliExit {
        match w {
            CliExitWire::Fixed(i) => crate::cli::CliExit::Fixed(i),
            CliExitWire::Binding(s) => crate::cli::CliExit::Binding(s),
            CliExitWire::Field { object, field } => crate::cli::CliExit::Field { object, field },
            CliExitWire::Unsupported => crate::cli::CliExit::Unsupported,
        }
    }

    fn map_cli_option(w: CliOptionWire) -> crate::cli::CliOption {
        crate::cli::CliOption {
            binding: w.binding,
            binding_symbol: radix_lexer::Symbol(0),
            ty: map_cli_type(w.ty),
            short: w.short,
            long: w.long,
            description: w.description,
            global: w.global,
            default: w.default.map(map_cli_default),
            flag: w.is_flag,
            span: radix_lexer::Span::default(),
        }
    }

    fn map_cli_operand(w: CliOperandWire) -> crate::cli::CliOperand {
        crate::cli::CliOperand {
            binding: w.binding,
            binding_symbol: radix_lexer::Symbol(0),
            ty: map_cli_type(w.ty),
            rest: w.is_rest,
            description: w.description,
            global: w.global,
            default: w.default.map(map_cli_default),
            span: radix_lexer::Span::default(),
        }
    }

    fn map_cli_command(w: CliCommandWire) -> crate::cli::CliCommand {
        crate::cli::CliCommand {
            path: w.path,
            module_path: w.module_path,
            function: w.function,
            function_symbol: w.function_symbol,
            args_binding: w.args_binding,
            aliases: w.aliases,
            description: w.description,
            options: w.options.into_iter().map(map_cli_option).collect(),
            operands: w.operands.into_iter().map(map_cli_operand).collect(),
            span: radix_lexer::Span::default(),
        }
    }

    crate::cli::CliProgram {
        name: wire.name,
        entry_args: wire.entry_args,
        mode: map_cli_mode(wire.mode),
        version: wire.version,
        description: wire.description,
        global_options: wire
            .global_options
            .into_iter()
            .map(map_cli_option)
            .collect(),
        global_operands: wire
            .global_operands
            .into_iter()
            .map(map_cli_operand)
            .collect(),
        options: wire.options.into_iter().map(map_cli_option).collect(),
        operands: wire.operands.into_iter().map(map_cli_operand).collect(),
        commands: wire.commands.into_iter().map(map_cli_command).collect(),
        exit: wire.exit.map(map_cli_exit),
    }
}

// ---------------------------------------------------------------------------
// load_fhir
// ---------------------------------------------------------------------------

/// Deserialize a [`radix_hir_fhir::HirArtifact`] from a `postcard` byte slice
/// and return a fully reconstructed [`AnalyzedModule`].
///
/// # Schema versioning (F3)
///
/// `schema_version` is the first wire field, so the loader reads the version
/// prefix before interpreting the payload and rejects any mismatch — forward
/// or backward — with [`HirArtifactError::UnsupportedSchemaVersion`]. The
/// schema version ratchets on every breaking HIR change; there is no
/// compatibility decoder for earlier schemas.
///
/// # Arena reconstruction (U2.2)
///
/// Reconstructs all arenas from their snapshot forms:
/// - [`TypeTable`] from [`radix_types::TypeTableSnapshot`] (caches rebuilt per CTM E.2)
/// - [`Interner`] from ordered `Vec<String>` (stable `Symbol(u32)` indices)
/// - [`Resolver`] from [`radix_hir_fhir::ResolverSnapshot`] (builtin `DefId`
///   values re-derived as compiler constants, not deserialized)
/// - [`ResolvedUseIndex`] from `Vec<ResolvedUse>` (lookup maps rebuilt)
/// - Wire DTOs mapped to live types (F18, F20)
///
/// # Errors
///
/// Returns [`HirArtifactError`] for any deserialization, truncation,
/// schema-mismatch, or dangling-index problem. Never panics on the load
/// path (F7).
pub fn load_fhir(bytes: &[u8]) -> Result<AnalyzedModule, HirArtifactError> {
    // F3 schema-version gate and U2.3 referential-integrity validation run
    // inside the format crate's decode; only arena reconstruction remains
    // here (U2.2).
    let artifact = radix_hir_fhir::decode(bytes)?;
    reconstruct_unit(artifact)
}

/// Reconstruct an [`AnalyzedModule`] from a validated
/// [`radix_hir_fhir::HirArtifact`] (U2.2).
///
/// Shared by the single-unit loader and the FHIR package loader
/// (`crate::hir::package`), which reconstructs every embedded unit through
/// this same path so package units and standalone units rebuild identically.
pub(crate) fn reconstruct_unit(artifact: radix_hir_fhir::HirArtifact) -> Result<AnalyzedModule, HirArtifactError> {
    // TypeTable: rebuild from snapshot (CTM E.2), then restore the typecheck
    // binder's resolved generic call vectors (schema v10) so specialization
    // reads the original explicit/inferred/defaulted facts instead of
    // re-running inference.
    let mut types = TypeTable::from_snapshot(artifact.types)
        .map_err(|msg| HirArtifactError::MalformedSnapshot { kind: "types", detail: msg })?;
    types.resolved_generic_calls = artifact.resolved_generic_calls;

    // Interner: rebuild from ordered string table.
    let interner = Interner::from_string_table(artifact.interner);

    // Resolver: rebuild from snapshot (F5, F19). Builtins are compiler constants.
    let resolver = Resolver::from_snapshot(artifact.resolver);

    // ResolvedUseIndex: rebuild lookup maps from uses vector (F20).
    let resolved_uses: Vec<ResolvedUse> = artifact
        .resolved_uses
        .into_iter()
        .map(map_resolved_use)
        .collect();
    let resolved_uses = ResolvedUseIndex::from_snapshot(resolved_uses);

    // FunctionFactTable: map wire → live (F20).
    let function_facts = map_function_fact_table(artifact.function_facts);

    // CliProgram: map wire → live (F18).
    let cli_program = artifact.cli_program.map(map_cli_program);

    // GPU builtins: map wire → live (F13, F20).
    let gpu_builtins: Vec<(crate::hir::DefId, radix_types::MirKernelBuiltin)> = artifact
        .gpu_builtins
        .into_iter()
        .map(|(def_id, builtin)| (def_id, map_gpu_builtin(builtin)))
        .collect();

    // Capability provenance: map wire → live (SCR-04).
    let provenance = map_provenance_table(artifact.capability_provenance);

    let analysis_stamp = crate::semantic::AnalysisStamp::from_persisted(artifact.analysis_stamp);

    let radix_lanes =
        crate::semantic::RadixLaneMetadata::from_snapshot(artifact.radix_lanes.into_iter().map(|entry| {
            let lane = match entry.lane {
                RadixLaneWire::Air => crate::semantic::RadixLane::Air,
                RadixLaneWire::Mir => crate::semantic::RadixLane::Mir,
                RadixLaneWire::HirDirect => crate::semantic::RadixLane::HirDirect,
            };
            let backward = entry
                .backward
                .map(|backward| crate::semantic::RadixBackward {
                    companion_name: backward.companion_name,
                    companion_def_id: backward.companion_def_id,
                });
            (entry.def_id, lane, backward)
        }));

    let provider_units = provider_units_live(artifact.provider_units)?;

    let mut unit = AnalyzedModule::from_loaded_with_presentation(
        analysis_stamp,
        interner,
        types,
        resolver,
        artifact.hir,
        artifact.presentation,
        cli_program,
        artifact.libraries,
        AnnotationContractMetadata::default(),
        function_facts,
        resolved_uses,
        QualifiedIdentityTable::default(),
        provenance,
        radix_lanes,
        gpu_builtins,
        GraphicsSourceFacts::default(),
        Vec::new(), // diagnostics
    );
    // Reinstall the provider carriage (schema v14, ARCH-U4): the loaded
    // surface must emit provider imports from carried units exactly as the
    // direct surface does, with no disk re-derivation.
    unit.provider_units = provider_units;
    Ok(unit)
}

/// Rebuild a live [`crate::driver::ProviderUnitView`] from its wire form,
/// recursing through each fragment's nested view. Diagnostics are not wire
/// facts: carriage units carry warnings-only diagnostics by construction and
/// the emitter's fail-closed error check reads them only for `Failed`-free
/// entries, so loaded carriage units reconstruct with empty diagnostics — the
/// same treatment the loader gives the consumer unit's own diagnostics.
fn provider_units_live(
    wire: BTreeMap<String, radix_hir_fhir::ProviderCarriageEntryWire>,
) -> Result<crate::driver::ProviderUnitView, HirArtifactError> {
    use crate::driver::{CarriedProviderUnit, ProviderUnit};

    let mut units = BTreeMap::new();
    for (import_path, entry) in wire {
        let carried = match entry {
            radix_hir_fhir::ProviderCarriageEntryWire::Failed { message } => CarriedProviderUnit::Failed { message },
            radix_hir_fhir::ProviderCarriageEntryWire::Unit(fragment) => {
                let mut types = TypeTable::from_snapshot(fragment.types)
                    .map_err(|msg| HirArtifactError::MalformedSnapshot { kind: "types", detail: msg })?;
                types.resolved_generic_calls = fragment.resolved_generic_calls;
                let nested = provider_units_live(fragment.nested)?;
                CarriedProviderUnit::Unit(std::sync::Arc::new(std::sync::Mutex::new(ProviderUnit {
                    hir: fragment.hir,
                    types,
                    interner: Interner::from_string_table(fragment.interner),
                    resolver: Resolver::from_snapshot(fragment.resolver),
                    diagnostics: Vec::new(),
                    nested,
                })))
            }
        };
        units.insert(import_path, carried);
    }
    Ok(crate::driver::ProviderUnitView { units })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[path = "artifact_test.rs"]
mod tests;
