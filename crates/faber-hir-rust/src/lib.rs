//! Faber-owned Rust HIR target support.
//!
//! This crate is the Faber-side leaf for the `hir-rust` target. Radix owns
//! lowering Faber `HIR` to Rust source. Faber owns product packaging around
//! that Rust source: generated Cargo crates, runtime-plan decisions, package
//! linking, binding probes, and build/run orchestration.
//!
//! The first responsibility of this leaf is to own the Rust adapter surface
//! Faber needs from Radix. Package assembly should move here in follow-up
//! chunks so the main `faber` crate remains target-neutral.

pub use radix::codegen::rust::{
    build_local_import_function_params, build_local_import_namespaces,
    generate_with_library_registry_test_selection_and_imports, local_import_module_key,
    remap_function_param_info, render_binding_probe, rust_gpu_builtins, to_cli_ir,
    ImportedFunctionParams, ImportedNamespaceInfo, ModuleGenerationRequest, RustCodegen,
    RustFieldNamePolicy, SiblingModuleExports, TestSelection,
};
