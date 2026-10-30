//! Thin complete compiler façade over [`radix_module`] and [`radix_program`].
//!
//! Library-only: no binary, no clap, no process exit. The developer `radix`
//! binary lives in `radix-cli`.
//!
//! Registration surface, split by ownership: [`module`] re-exports the
//! module-compiler APIs and module-owned types, [`cli`] the compiler CLI-IR,
//! [`program`] whole-program package compilation, and [`services`] the
//! admitted typed services. Module-compiler conveniences are additionally
//! re-exported at the crate root.

/// Fail-closed module compiler APIs and supporting module-owned types.
pub mod module {
    #[cfg(feature = "mir-evm")]
    pub use radix_module::EvmOutput;
    #[cfg(feature = "device")]
    pub use radix_module::air;
    pub use radix_module::driver::{
        AnalyzeRequest, AnalyzedModule, ModuleCompileRequest, ModuleImportContract, TargetBindings,
        analyze_source,
    };
    #[cfg(feature = "mir-llvm")]
    pub use radix_module::llvm_host;
    #[cfg(feature = "mir-core")]
    pub use radix_module::mir;
    #[cfg(feature = "mir-clif")]
    pub use radix_module::{ClifEngine, ClifJitOutput, ClifRuntimeTable};
    pub use radix_module::{
        CompileResult, Compiler, Config, Diagnostic, FaberOutput, GoOutput, HaskellOutput, Output,
        PythonOutput, RustOutput, Session, SwiftOutput, Target, TypeScriptOutput, WarnPolicy,
        apply_warn_policy, is_known_diagnostic_code,
    };
    pub use radix_module::{
        abi, aspect, builtins, cli_descriptor, codegen, diagnostics, driver, file_interface, hir,
        intrinsics, kernel, lexer, live_tuus_cursor, locale, parser, runtime_contract, semantic,
        syntax,
    };
}

/// Compiler CLI-IR (`@ cli` / `@ imperium`), not developer CLI grammar.
pub mod cli {
    pub use radix_module::cli::*;
}

/// Typed program-layer compilation and whole-program package APIs.
pub mod program {
    pub use radix_module::program::{
        PreparedProgramModule, analyze_prepared_program_name, analyze_prepared_program_path,
        compile_prepared_program_path, configured_library_home_setting, explicit_locale_pack,
        find_package_root, library_home, package_identity_facts_for_path, prepare_program_module,
        prepare_program_module_for_name, set_configured_library_home,
    };
    pub use radix_module::{CompileResult, Compiler};
    #[cfg(feature = "mir-llvm")]
    pub use radix_program::llvm;
    #[cfg(any(feature = "mir-fmir", feature = "runner", feature = "device"))]
    pub use radix_program::mir;
    #[cfg(feature = "mir-wasm")]
    pub use radix_program::wasm;
    pub use radix_program::{
        LibraryImportBinding, PackageFile, analyze, artifact_plan, discovery, emit, file_interface,
        frontmatter, import_graph, library, lockfile, manifest, member_path, modules, paths,
        qualify, route_table, source_files, test_source_filter,
    };
    #[cfg(feature = "hir-rust")]
    pub use radix_program::{codegen, rust_target};
    #[cfg(feature = "hir-fhir")]
    pub use radix_program::{fhir, nucleum};
}

/// Admitted typed services: values + diagnostics, no printing or process exit.
pub mod services {
    pub use radix_module::program::{
        LibraryBindingsResolver, compile_cli_input, compile_cli_input_with_library_bindings,
        compile_cli_input_with_locale_pack, compile_cli_input_with_locale_pack_explicit,
        compile_cli_path, compile_cli_path_with_locale_pack, compile_cli_source,
        compile_cli_source_with_locale_pack, resolve_package_mode, should_treat_as_package,
    };
    pub use radix_module::services::{
        BuildCommand, CheckCommand, CheckDiagnosticsResult, DiagnosticMode, EmitCommand,
        FieldModifierCodemod, SourceInspection, TargetCapabilities, TargetCommandSurface,
        VerifyCommand, VerifySourceResult, build_output_path, check_diagnostics_for_source,
        check_diagnostics_for_source_with_config, check_diagnostics_for_source_with_locale_pack,
        cli_ir_inspection_for_source, cli_ir_inspection_for_source_with_config,
        discoverable_targets, hir_inspection_for_source, hir_inspection_for_source_with_config,
        hir_inspection_json, insert_field_modifiers, lex_inspection_for_source,
        lex_inspection_for_source_with_config, normalize_locale002_notes, output_code,
        parse_inspection_for_source, parse_inspection_for_source_with_config,
        source_declares_locale, target_capabilities, target_capabilities_for_surface,
        target_extension, target_leaves, target_leaves_label, target_name, verify_source,
        verify_source_with_config,
    };
    #[cfg(feature = "mir-core")]
    pub use radix_module::services::{
        mir_output_for_source, mir_output_for_source_with_config, output_reflection_json,
    };
}

pub use radix_module::abi;
pub use radix_module::aspect;
pub use radix_module::codegen;
pub use radix_module::diagnostics;
pub use radix_module::driver;
pub use radix_module::hir;
pub use radix_module::lexer;
pub use radix_module::locale;
#[cfg(feature = "mir-core")]
pub use radix_module::mir;
pub use radix_module::{
    CompileResult, Compiler, Config, Diagnostic, Output, Session, Target, is_known_diagnostic_code,
};
