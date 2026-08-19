//! Typed compiler command services: values + diagnostics, no print/exit.

mod check;
mod commands;
mod emit;
mod inspect;
pub(crate) mod json;
#[cfg(feature = "mir-core")]
mod reflection;
mod targets;
mod verify;

pub use check::{check_diagnostics_for_source, check_diagnostics_for_source_with_locale_pack, CheckDiagnosticsResult};
pub use commands::{BuildCommand, CheckCommand, DiagnosticMode, EmitCommand, VerifyCommand};
pub use crate::program::{
    analyze_prepared_program_name, analyze_prepared_program_path, compile_cli_input, compile_cli_input_with_locale_pack,
    compile_cli_input_with_locale_pack_explicit, compile_cli_path, compile_cli_path_with_locale_pack, compile_cli_source,
    compile_cli_source_with_locale_pack, package_identity_facts_for_path, prepare_program_module,
    prepare_program_module_for_name, resolve_package_mode, should_treat_as_package, PreparedProgramModule,
};
pub use emit::{build_output_path, output_code};
pub use inspect::{
    cli_ir_inspection_for_source, hir_inspection_for_source, hir_inspection_json, lex_inspection_for_source,
    parse_inspection_for_source, SourceInspection,
};
#[cfg(feature = "mir-core")]
pub use inspect::mir_output_for_source;
#[cfg(feature = "mir-core")]
pub use reflection::output_reflection_json;
pub use targets::{
    discoverable_targets, target_capabilities, target_capabilities_for_surface, target_extension, target_name,
    TargetCapabilities, TargetCommandSurface,
};
pub use verify::{verify_source, VerifySourceResult};


#[cfg(test)]
#[path = "services_test.rs"]
mod tests;
