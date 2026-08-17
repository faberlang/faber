//! HIR artifact serialization from analyzed units.
//!
//! Public constructor for creating a [`radix_hir_fhir::HirArtifact`]
//! from an analyzed unit. Serialization into `postcard` bytes and writing
//! to disk happen at the call site.

use crate::driver::AnalyzedModule;
use radix_hir_fhir::{
    CaptureFactWire, CaptureModeWire, CliCommandWire, CliDefaultWire, CliExitWire, CliModeWire, CliOperandWire,
    CliOptionWire, CliProgramWire, CliTypeWire, FunctionFactTableWire, FunctionFactWire, GpuBuiltinWire, HirArtifact,
    ResolvedUseKeyWire, ResolvedUseKindWire, ResolvedUseWire, SourceIdentity, SCHEMA_VERSION,
};

// ── Entry point ───────────────────────────────────────────────────────────

/// Build a [`HirArtifact`] from a fully analyzed unit.
///
/// Populates all serializable fields: HIR program, type table snapshot,
/// interner table, resolver snapshot, CLI program (F18), function facts,
/// resolved uses, analysis stamp, and GPU builtins.
///
/// The `content_hash` should be a hex-encoded SHA-256 of the source bytes.
/// The `relative_path` should be a path relative to the project root.
pub fn from_analyzed(unit: &AnalyzedModule, content_hash: &str, relative_path: &str) -> HirArtifact {
    HirArtifact {
        schema_version: SCHEMA_VERSION,
        source_identity: SourceIdentity {
            content_hash: content_hash.to_string(),
            relative_path: relative_path.to_string(),
        },
        hir: unit.hir.clone(),
        presentation: unit.presentation.clone(),
        cli_program: unit.cli_program.as_ref().map(CliProgramWire::from),
        types: unit.types.snapshot(),
        interner: unit.interner.to_string_table(),
        resolver: unit.resolver.to_snapshot(),
        libraries: unit.libraries.clone(),
        function_facts: function_fact_table_to_wire(&unit.function_facts),
        resolved_uses: unit
            .resolved_uses
            .to_snapshot()
            .into_iter()
            .map(|u| resolved_use_to_wire(&u))
            .collect(),
        analysis_stamp: unit.analysis_stamp.get(),
        gpu_builtins: unit
            .gpu_builtins
            .iter()
            .map(|(def_id, builtin)| (*def_id, gpu_builtin_to_wire(*builtin)))
            .collect(),
    }
}

// ── Wire-conversion helpers ───────────────────────────────────────────────

fn capture_fact_to_wire(fact: &crate::semantic::CaptureFact) -> CaptureFactWire {
    CaptureFactWire {
        def_id: fact.def_id,
        mode: match fact.mode {
            crate::semantic::CaptureMode::Read => CaptureModeWire::Read,
            crate::semantic::CaptureMode::Mutate => CaptureModeWire::Mutate,
            crate::semantic::CaptureMode::Consume => CaptureModeWire::Consume,
        },
    }
}

fn function_fact_to_wire(fact: &crate::semantic::FunctionFact) -> FunctionFactWire {
    FunctionFactWire {
        direct_failure: fact.direct_failure,
        may_fail: fact.may_fail,
        is_async: fact.is_async,
        is_generator: fact.is_generator,
        requires_await: fact.requires_await,
        call_edges: fact.call_edges.iter().copied().collect(),
        captures: fact.captures.iter().map(capture_fact_to_wire).collect(),
        param_modes: fact.param_modes.iter().map(|(k, v)| (*k, *v)).collect(),
        err_ty: fact.err_ty,
    }
}

fn function_fact_table_to_wire(ft: &crate::semantic::FunctionFactTable) -> FunctionFactTableWire {
    FunctionFactTableWire {
        entries: ft
            .iter()
            .map(|(def_id, fact)| (def_id, function_fact_to_wire(fact)))
            .collect(),
        entry: ft.entry.as_ref().map(function_fact_to_wire),
    }
}

fn resolved_use_to_wire(u: &crate::semantic::ResolvedUse) -> ResolvedUseWire {
    ResolvedUseWire {
        key: match &u.key {
            crate::semantic::ResolvedUseKey::Local(def_id) => ResolvedUseKeyWire::Local(*def_id),
            crate::semantic::ResolvedUseKey::Portable(s) => ResolvedUseKeyWire::Portable(s.clone()),
        },
        kind: match u.kind {
            crate::semantic::ResolvedUseKind::Path => ResolvedUseKindWire::Path,
            crate::semantic::ResolvedUseKind::Call => ResolvedUseKindWire::Call,
            crate::semantic::ResolvedUseKind::MethodCall => ResolvedUseKindWire::MethodCall,
            crate::semantic::ResolvedUseKind::TypeRef => ResolvedUseKindWire::TypeRef,
            crate::semantic::ResolvedUseKind::ImportBinding => ResolvedUseKindWire::ImportBinding,
        },
        enclosing: u.enclosing,
    }
}

fn gpu_builtin_to_wire(b: radix_types::MirKernelBuiltin) -> GpuBuiltinWire {
    match b {
        radix_types::MirKernelBuiltin::GlobalInvocationX => GpuBuiltinWire::GlobalInvocationX,
        radix_types::MirKernelBuiltin::GlobalInvocationY => GpuBuiltinWire::GlobalInvocationY,
        radix_types::MirKernelBuiltin::GlobalInvocationZ => GpuBuiltinWire::GlobalInvocationZ,
        radix_types::MirKernelBuiltin::GlobalInvocationId => GpuBuiltinWire::GlobalInvocationId,
        radix_types::MirKernelBuiltin::WorkgroupIdX => GpuBuiltinWire::WorkgroupIdX,
        radix_types::MirKernelBuiltin::WorkgroupIdY => GpuBuiltinWire::WorkgroupIdY,
        radix_types::MirKernelBuiltin::WorkgroupIdZ => GpuBuiltinWire::WorkgroupIdZ,
        radix_types::MirKernelBuiltin::WorkgroupId => GpuBuiltinWire::WorkgroupId,
        radix_types::MirKernelBuiltin::LocalInvocationIdX => GpuBuiltinWire::LocalInvocationIdX,
        radix_types::MirKernelBuiltin::LocalInvocationIdY => GpuBuiltinWire::LocalInvocationIdY,
        radix_types::MirKernelBuiltin::LocalInvocationIdZ => GpuBuiltinWire::LocalInvocationIdZ,
        radix_types::MirKernelBuiltin::LocalInvocationId => GpuBuiltinWire::LocalInvocationId,
        radix_types::MirKernelBuiltin::WorkgroupSizeX => GpuBuiltinWire::WorkgroupSizeX,
        radix_types::MirKernelBuiltin::WorkgroupSizeY => GpuBuiltinWire::WorkgroupSizeY,
        radix_types::MirKernelBuiltin::WorkgroupSizeZ => GpuBuiltinWire::WorkgroupSizeZ,
    }
}

// ── CLI wire conversions (F18 Shape B — forward mapping) ─────────────────

impl From<&crate::cli::CliType> for CliTypeWire {
    fn from(ty: &crate::cli::CliType) -> Self {
        match ty {
            crate::cli::CliType::Textus => CliTypeWire::Textus,
            crate::cli::CliType::Numerus => CliTypeWire::Numerus,
            crate::cli::CliType::Fractus => CliTypeWire::Fractus,
            crate::cli::CliType::Bivalens => CliTypeWire::Bivalens,
            crate::cli::CliType::Octeti => CliTypeWire::Octeti,
            crate::cli::CliType::Ignotum => CliTypeWire::Ignotum,
            crate::cli::CliType::ListaTextus => CliTypeWire::ListaTextus,
            crate::cli::CliType::ListaNumerus => CliTypeWire::ListaNumerus,
        }
    }
}

impl From<&crate::cli::CliDefault> for CliDefaultWire {
    fn from(d: &crate::cli::CliDefault) -> Self {
        match d {
            crate::cli::CliDefault::Text(s) => CliDefaultWire::Text(s.clone()),
            crate::cli::CliDefault::Integer(i) => CliDefaultWire::Integer(*i),
            crate::cli::CliDefault::Float(f) => CliDefaultWire::Float(*f),
            crate::cli::CliDefault::Bool(b) => CliDefaultWire::Bool(*b),
            crate::cli::CliDefault::Nil => CliDefaultWire::Nil,
            crate::cli::CliDefault::Expr(s) => CliDefaultWire::Expr(s.clone()),
        }
    }
}

impl From<&crate::cli::CliExit> for CliExitWire {
    fn from(e: &crate::cli::CliExit) -> Self {
        match e {
            crate::cli::CliExit::Fixed(v) => CliExitWire::Fixed(*v),
            crate::cli::CliExit::Binding(s) => CliExitWire::Binding(s.clone()),
            crate::cli::CliExit::Field { object, field } => {
                CliExitWire::Field { object: object.clone(), field: field.clone() }
            }
            crate::cli::CliExit::Unsupported => CliExitWire::Unsupported,
        }
    }
}

impl From<&crate::cli::CliOption> for CliOptionWire {
    fn from(o: &crate::cli::CliOption) -> Self {
        CliOptionWire {
            binding: o.binding.clone(),
            ty: CliTypeWire::from(&o.ty),
            short: o.short.clone(),
            long: o.long.clone(),
            description: o.description.clone(),
            global: o.global,
            default: o.default.as_ref().map(CliDefaultWire::from),
            flag: o.flag,
        }
    }
}

impl From<&crate::cli::CliOperand> for CliOperandWire {
    fn from(o: &crate::cli::CliOperand) -> Self {
        CliOperandWire {
            binding: o.binding.clone(),
            ty: CliTypeWire::from(&o.ty),
            rest: o.rest,
            description: o.description.clone(),
            global: o.global,
            default: o.default.as_ref().map(CliDefaultWire::from),
        }
    }
}

impl From<&crate::cli::CliCommand> for CliCommandWire {
    fn from(c: &crate::cli::CliCommand) -> Self {
        CliCommandWire {
            path: c.path.clone(),
            module_path: c.module_path.clone(),
            function: c.function.clone(),
            function_symbol: c.function_symbol,
            args_binding: c.args_binding.clone(),
            aliases: c.aliases.clone(),
            description: c.description.clone(),
            options: c.options.iter().map(CliOptionWire::from).collect(),
            operands: c.operands.iter().map(CliOperandWire::from).collect(),
        }
    }
}

impl From<&crate::cli::CliMode> for CliModeWire {
    fn from(m: &crate::cli::CliMode) -> Self {
        match m {
            crate::cli::CliMode::NotCli => CliModeWire::NotCli,
            crate::cli::CliMode::SingleCommand => CliModeWire::SingleCommand,
            crate::cli::CliMode::Subcommand => CliModeWire::Subcommand,
        }
    }
}

impl From<&crate::cli::CliProgram> for CliProgramWire {
    fn from(p: &crate::cli::CliProgram) -> Self {
        CliProgramWire {
            name: p.name.clone(),
            entry_args: p.entry_args.clone(),
            mode: CliModeWire::from(&p.mode),
            version: p.version.clone(),
            description: p.description.clone(),
            global_options: p.global_options.iter().map(CliOptionWire::from).collect(),
            global_operands: p.global_operands.iter().map(CliOperandWire::from).collect(),
            options: p.options.iter().map(CliOptionWire::from).collect(),
            operands: p.operands.iter().map(CliOperandWire::from).collect(),
            commands: p.commands.iter().map(CliCommandWire::from).collect(),
            exit: p.exit.as_ref().map(CliExitWire::from),
        }
    }
}
