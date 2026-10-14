//! Program-layer discovery and path compile helpers.
//!
//! Physical home is this crate because forma, codegen, and `Compiler::compile`
//! call these helpers and `radix-module` cannot depend on `radix-program`.
//! Public admission is `radix::program` / `radix_program`, not module analysis.

pub mod compile;
pub mod default_import;
pub mod export_surface;
pub use compile::{
    LibraryBindingsResolver, PreparedProgramModule, analyze_prepared_program_name, analyze_prepared_program_path,
    compile_cli_input, compile_cli_input_with_library_bindings, compile_cli_input_with_locale_pack,
    compile_cli_input_with_locale_pack_explicit, compile_cli_path, compile_cli_path_with_locale_pack,
    compile_cli_source, compile_cli_source_with_locale_pack, compile_prepared_program_path, explicit_locale_pack,
    package_identity_facts_for_path, prepare_program_module, prepare_program_module_for_name, provider_unit_for_target,
    resolve_package_mode, should_treat_as_package,
};

pub use default_import::{apply_default_import_names, import_takes_default_name, program_region_name};
pub use export_surface::{program_declaration_surface, program_export_surface};

pub use crate::import_resolve::{
    FABER_LIBRARY_HOME_ENV, configured_library_home_setting, find_package_root, find_package_root_with_session,
    library_home, set_configured_library_home,
};
