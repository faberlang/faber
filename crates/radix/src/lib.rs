//! Thin complete compiler façade over [`radix_module`] and [`radix_program`].
//!
//! Library-only: no binary, no clap, no process exit. The developer `radix`
//! binary lives in `radix-cli`.

/// Fail-closed module analysis.
pub mod module {
    pub use radix_module::driver::{
        analyze_source, analyze_source_with_module_import_contract, AnalyzedModule, Config,
        ModuleImportContract, Session,
    };
}

/// Typed program-layer compilation and prepared-module analysis.
pub mod program {
    pub use radix_module::program::{
        analyze_prepared_program_name, analyze_prepared_program_path,
        compile_prepared_program_path, explicit_locale_pack, find_package_root, library_home,
        package_identity_facts_for_path, prepare_program_module, prepare_program_module_for_name,
        PreparedProgramModule, FABER_LIBRARY_HOME_ENV,
    };
    pub use radix_module::{CompileResult, Compiler};
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
