//! Thin complete compiler façade over [`radix_module`] and [`radix_program`].
//!
//! Library-only: no binary, no clap, no process exit. The developer `radix`
//! binary lives in `radix-cli`.

/// Fail-closed module compiler APIs and supporting module-owned types.
pub mod module {
    #[cfg(feature = "device")]
    pub use radix_module::air;
    pub use radix_module::driver::{
        analyze_source, analyze_source_with_module_import_contract, AnalyzedModule,
        ModuleImportContract,
    };
    #[cfg(feature = "mir-core")]
    pub use radix_module::mir;
    pub use radix_module::{
        abi, aspect, builtins, cli_descriptor, codegen, diagnostics, driver, file_interface, hir,
        intrinsics, kernel, lexer, live_tuus_cursor, llvm_host, locale, parser, paths,
        runtime_contract, semantic, syntax,
    };
    pub use radix_module::{
        apply_warn_policy, is_known_diagnostic_code, CompileResult, Compiler, Config, Diagnostic,
        FaberOutput, GoOutput, HaskellOutput, Output, PythonOutput, RustOutput, Session,
        SwiftOutput, Target, TypeScriptOutput, WarnPolicy,
    };
}

/// Compiler CLI-IR (`@ cli` / `@ imperium`), not developer CLI grammar.
pub mod cli {
    pub use radix_module::cli::*;
}

/// Typed program-layer compilation and whole-program package APIs.
pub mod program {
    pub use radix_module::program::{
        analyze_prepared_program_name, analyze_prepared_program_path,
        compile_prepared_program_path, explicit_locale_pack, find_package_root, library_home,
        package_identity_facts_for_path, prepare_program_module, prepare_program_module_for_name,
        PreparedProgramModule, FABER_LIBRARY_HOME_ENV,
    };
    pub use radix_module::{CompileResult, Compiler};
    #[cfg(feature = "mir-llvm")]
    pub use radix_program::llvm;
    pub use radix_program::mir;
    #[cfg(feature = "mir-wasm")]
    pub use radix_program::wasm;
    pub use radix_program::{
        analyze, artifact_plan, discovery, emit, file_interface, frontmatter, import_graph,
        library, lockfile, manifest, member_path, modules, paths, source_files, test_source_filter,
        LibraryImportBinding, PackageFile,
    };
    #[cfg(feature = "hir-rust")]
    pub use radix_program::{codegen, rust_target};
    #[cfg(feature = "hir-fhir")]
    pub use radix_program::{fhir, nucleum};
}

/// Admitted typed services: values + diagnostics, no printing or process exit.
pub mod services {
    pub use radix_module::program::{
        analyze_prepared_program_name, analyze_prepared_program_path, compile_cli_input,
        compile_cli_input_with_locale_pack, compile_cli_input_with_locale_pack_explicit,
        compile_cli_path, compile_cli_path_with_locale_pack, package_identity_facts_for_path,
        prepare_program_module, prepare_program_module_for_name, resolve_package_mode,
        should_treat_as_package, PreparedProgramModule,
    };
    pub use radix_module::services::{
        build_output_path, check_diagnostics_for_source,
        check_diagnostics_for_source_with_locale_pack, cli_ir_inspection_for_source,
        compile_cli_source, compile_cli_source_with_locale_pack, hir_inspection_for_source,
        hir_inspection_json, lex_inspection_for_source, output_code, parse_inspection_for_source,
        target_capabilities, target_capabilities_for_surface, verify_source, BuildCommand,
        CheckCommand, CheckDiagnosticsResult, DiagnosticMode, EmitCommand, SourceInspection,
        TargetCapabilities, TargetCommandSurface, VerifyCommand, VerifySourceResult,
    };
    #[cfg(feature = "mir-core")]
    pub use radix_module::services::{mir_output_for_source, output_reflection_json};
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
    is_known_diagnostic_code, CompileResult, Compiler, Config, Diagnostic, Output, Session, Target,
};
