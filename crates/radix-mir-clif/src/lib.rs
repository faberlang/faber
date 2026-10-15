//! `radix-mir-clif` — the public Cranelift backend (goal
//! `docs/factory/radix-mir-clif/goal.md`).
//!
//! Unit C0 skeleton capability: emit one native object file exporting a
//! constant function. Unit C3 grows this into
//! `emit_object(&ValidatedMir, &Interner, &ClifOptions)`; the Cranelift pins
//! and per-crate feature lists live in `Cargo.toml` per D3 and the
//! 2026-10-14 design review.

pub mod capability;

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

/// Emit a native object file with the executable entry pair unit C1 links:
/// `main(argument_count, argument_values)` tail-calling the runtime's
/// `__faber_clif_v1_main`, plus `faber_clif_incipit` returning `value`.
///
/// The object owns the `main` symbol and the archive never does (the LLVM
/// route's pattern), so linking the runtime as an rlib never collides with
/// a Rust entry point.
///
/// # Errors
///
/// Returns a rendered message when host-ISA construction, module assembly or
/// object emission fails; unit C3 replaces this error shape with the
/// capability-gap report.
pub fn emit_entry_object(value: i64) -> Result<Vec<u8>, String> {
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

    // The runtime entry: (i32, pointer-to-pointer) -> i32.
    let mut entry_signature = module.make_signature();
    entry_signature.params.push(AbiParam::new(types::I32));
    entry_signature
        .params
        .push(AbiParam::new(module.isa().pointer_type()));
    entry_signature.returns.push(AbiParam::new(types::I32));
    let entry_id = module
        .declare_function("__faber_clif_v1_main", Linkage::Import, &entry_signature)
        .map_err(render)?;

    // main(argument_count, argument_values) -> entry(argument_count, argument_values)
    let mut main_signature = module.make_signature();
    main_signature.params.push(AbiParam::new(types::I32));
    main_signature
        .params
        .push(AbiParam::new(module.isa().pointer_type()));
    main_signature.returns.push(AbiParam::new(types::I32));
    let main_id = module
        .declare_function("main", Linkage::Export, &main_signature)
        .map_err(render)?;

    let mut incipit_signature = module.make_signature();
    incipit_signature.returns.push(AbiParam::new(types::I64));
    let incipit_id = module
        .declare_function("faber_clif_incipit", Linkage::Export, &incipit_signature)
        .map_err(render)?;

    let mut context = module.make_context();
    context.func.signature = main_signature;
    let mut fn_builder_context = FunctionBuilderContext::new();
    {
        let mut builder = FunctionBuilder::new(&mut context.func, &mut fn_builder_context);
        let entry_block = builder.create_block();
        builder.append_block_params_for_function_params(entry_block);
        builder.switch_to_block(entry_block);
        let argument_count = builder.block_params(entry_block)[0];
        let argument_values = builder.block_params(entry_block)[1];
        let entry_ref = module.declare_func_in_func(entry_id, builder.func);
        let call = builder
            .ins()
            .call(entry_ref, &[argument_count, argument_values]);
        let result = builder.inst_results(call)[0];
        builder.ins().return_(&[result]);
        builder.seal_all_blocks();
        builder.finalize(module.isa().frontend_config());
    }
    module
        .define_function(main_id, &mut context)
        .map_err(render)?;

    let mut context = module.make_context();
    context.func.signature = incipit_signature;
    let mut fn_builder_context = FunctionBuilderContext::new();
    {
        let mut builder = FunctionBuilder::new(&mut context.func, &mut fn_builder_context);
        let entry_block = builder.create_block();
        builder.append_block_params_for_function_params(entry_block);
        builder.switch_to_block(entry_block);
        let result = builder.ins().iconst(types::I64, value);
        builder.ins().return_(&[result]);
        builder.seal_all_blocks();
        builder.finalize(module.isa().frontend_config());
    }
    module
        .define_function(incipit_id, &mut context)
        .map_err(render)?;

    module.finish().emit().map_err(render)
}
