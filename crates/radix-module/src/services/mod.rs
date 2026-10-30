//! Typed compiler command services: values + diagnostics, no print/exit.

mod check;
mod commands;
mod emit;
mod field_modifier_codemod;
mod inspect;
pub(crate) mod json;
#[cfg(feature = "mir-core")]
mod reflection;
mod targets;
mod verify;

pub use check::{
    CheckDiagnosticsResult, check_diagnostics_for_source, check_diagnostics_for_source_with_config,
    check_diagnostics_for_source_with_locale_pack, normalize_locale002_notes, source_declares_locale,
};
pub use commands::{BuildCommand, CheckCommand, DiagnosticMode, EmitCommand, VerifyCommand};
pub use emit::{build_output_path, output_code};
pub use field_modifier_codemod::{FieldModifierCodemod, insert_field_modifiers};
pub use inspect::{
    SourceInspection, cli_ir_inspection_for_source, cli_ir_inspection_for_source_with_config,
    hir_inspection_for_source, hir_inspection_for_source_with_config, hir_inspection_json, lex_inspection_for_source,
    lex_inspection_for_source_with_config, parse_inspection_for_source, parse_inspection_for_source_with_config,
};
#[cfg(feature = "mir-core")]
pub use inspect::{mir_output_for_source, mir_output_for_source_with_config};
#[cfg(feature = "mir-core")]
pub use reflection::output_reflection_json;
pub use targets::{
    TargetCapabilities, TargetCommandSurface, discoverable_targets, target_capabilities,
    target_capabilities_for_surface, target_extension, target_leaves, target_leaves_label, target_name,
};
pub use verify::{VerifySourceResult, verify_source, verify_source_with_config};

#[cfg(test)]
#[path = "services_test.rs"]
mod tests;
