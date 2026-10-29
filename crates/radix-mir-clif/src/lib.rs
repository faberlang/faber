//! `radix-mir-clif` — the public Cranelift backend (goal
//! `docs/factory/radix-mir-clif/goal.md`).
//!
//! Unit C0 skeleton capability: emit one native object file exporting a
//! constant function ([`emit_constant_object`], kept as the C0 proof). Unit C3
//! adds [`emit_object`]: validated MIR to a linkable executable object. The
//! Cranelift pins and per-crate feature lists live in `Cargo.toml` per D3 and
//! the 2026-10-14 design review.

pub mod abi;
pub mod capability;
mod emit;
mod lower;

pub use emit::{ClifError, ClifObject, emit_jit, emit_object, emit_program};
pub use lower::{JitProgram, StructMeta};

#[cfg(test)]
mod capability_test;

use cranelift_codegen::ir::AbiParam;
use cranelift_codegen::ir::InstBuilder;
use cranelift_codegen::ir::types;
use cranelift_codegen::settings::Configurable;
use cranelift_codegen::settings::{self, Flags};
use cranelift_frontend::FunctionBuilder;
use cranelift_frontend::FunctionBuilderContext;
use cranelift_module::Linkage;
use cranelift_module::Module;
use cranelift_module::default_libcall_names;
use cranelift_object::ObjectBuilder;
use cranelift_object::ObjectModule;

/// Render a Cranelift error into the skeleton's `String` error channel.
fn render<E: std::fmt::Display>(error: E) -> String {
    error.to_string()
}

/// Emit a native object file defining `symbol` as `() -> i64` returning
/// `value`, exported for a final link to resolve.
///
/// Flags follow the goal's probe: `opt_level=speed`, `is_pic`. Unit C1's
/// runtime `main` links such an object and calls `faber_clif_incipit`.
///
/// # Errors
///
/// Returns a rendered message when host-ISA construction, module assembly or
/// object emission fails; unit C3 replaces this error shape with the
/// capability-gap report.
pub fn emit_constant_object(symbol: &str, value: i64) -> Result<Vec<u8>, String> {
    let mut flags = settings::builder();
    flags.set("opt_level", "speed").map_err(render)?;
    flags.set("is_pic", "true").map_err(render)?;
    let isa = cranelift_native::builder_with_options(false)
        .map_err(render)?
        .finish(Flags::new(flags))
        .map_err(render)?;

    let builder =
        ObjectBuilder::new(isa, "radix-mir-clif", default_libcall_names()).map_err(render)?;
    let mut module = ObjectModule::new(builder);

    let mut signature = module.make_signature();
    signature.returns.push(AbiParam::new(types::I64));
    let func_id = module
        .declare_function(symbol, Linkage::Export, &signature)
        .map_err(render)?;

    let mut context = module.make_context();
    context
        .func
        .signature
        .returns
        .push(AbiParam::new(types::I64));
    let mut fn_builder_context = FunctionBuilderContext::new();
    {
        let mut builder = FunctionBuilder::new(&mut context.func, &mut fn_builder_context);
        let entry = builder.create_block();
        builder.append_block_params_for_function_params(entry);
        builder.switch_to_block(entry);
        let result = builder.ins().iconst(types::I64, value);
        builder.ins().return_(&[result]);
        builder.seal_all_blocks();
        builder.finalize(module.isa().frontend_config());
    }
    module
        .define_function(func_id, &mut context)
        .map_err(render)?;

    module.finish().emit().map_err(render)
}
