//! Program-layer discovery and path compile helpers.
//!
//! Physical home is this crate because forma, codegen, and `Compiler::compile`
//! call these helpers and `radix-module` cannot depend on `radix-program`.
//! Public admission is `radix::program` / `radix_program`, not module analysis.

pub mod compile;
pub use compile::{
    analyze_prepared_program_name, analyze_prepared_program_path, compile_cli_input,
    compile_cli_input_with_locale_pack, compile_cli_input_with_locale_pack_explicit, compile_cli_path,
    compile_cli_path_with_locale_pack, compile_cli_source, compile_cli_source_with_locale_pack,
    compile_prepared_program_path, explicit_locale_pack, package_identity_facts_for_path, prepare_program_module,
    prepare_program_module_for_name, resolve_package_mode, should_treat_as_package, PreparedProgramModule,
};

pub use crate::import_resolve::{find_package_root, library_home, FABER_LIBRARY_HOME_ENV};
