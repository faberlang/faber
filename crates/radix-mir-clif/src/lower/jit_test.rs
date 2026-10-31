//! The implicit return area of the `enable_multi_ret_implicit_sret` flag: a
//! signature with more return values than the host has return registers
//! compiles and round-trips its values.

use super::*;
use cranelift_codegen::ir::{AbiParam, InstBuilder, types};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};

/// Ten integer returns overflow every host's integer return-register budget
/// (x86-64 SystemV returns in two, arm64 in eight), so the callee hands them
/// back through the hidden return area the flag installs. Without the flag
/// the same module fails to compile with "Too many return values to fit in
/// registers", the x86-64 failure this guards against: the flag is what lets
/// a failable Faber function return status, result and error payload.
#[test]
fn return_values_beyond_the_register_budget_round_trip() {
    let values: Vec<i64> = (0..10).map(|index| i64::from(index) * 7 + 1).collect();
    let isa = host_isa(false).expect("host isa");
    let builder = JITBuilder::with_isa(isa, default_libcall_names());
    let mut module = JITModule::new(builder);

    let mut inner_signature = module.make_signature();
    for _ in &values {
        inner_signature.returns.push(AbiParam::new(types::I64));
    }
    let inner = module
        .declare_anonymous_function(&inner_signature)
        .expect("declare inner");
    let mut context = module.make_context();
    context.func.signature = inner_signature;
    {
        let mut builder_context = FunctionBuilderContext::new();
        let mut function = FunctionBuilder::new(&mut context.func, &mut builder_context);
        let block = function.create_block();
        function.switch_to_block(block);
        let returns = values
            .iter()
            .map(|value| function.ins().iconst(types::I64, *value))
            .collect::<Vec<_>>();
        function.ins().return_(&returns);
        function.seal_all_blocks();
        function.finalize(module.isa().frontend_config());
    }
    module
        .define_function(inner, &mut context)
        .expect("define inner");

    let mut entry_signature = module.make_signature();
    entry_signature.returns.push(AbiParam::new(types::I64));
    let entry = module
        .declare_anonymous_function(&entry_signature)
        .expect("declare entry");
    let mut context = module.make_context();
    context.func.signature = entry_signature;
    {
        let mut builder_context = FunctionBuilderContext::new();
        let mut function = FunctionBuilder::new(&mut context.func, &mut builder_context);
        let block = function.create_block();
        function.switch_to_block(block);
        let reference = module.declare_func_in_func(inner, function.func);
        let call = function.ins().call(reference, &[]);
        let mut sum = function.ins().iconst(types::I64, 0);
        for result in function.inst_results(call).to_vec() {
            sum = function.ins().iadd(sum, result);
        }
        function.ins().return_(&[sum]);
        function.seal_all_blocks();
        function.finalize(module.isa().frontend_config());
    }
    module
        .define_function(entry, &mut context)
        .expect("define entry");
    module.finalize_definitions().expect("finalize definitions");

    let code = module.get_finalized_function(entry);
    // SAFETY: `entry` is defined as `() -> i64` in the host's C calling
    // convention, the only shape it can take by construction above.
    let call: unsafe extern "C" fn() -> i64 = unsafe { std::mem::transmute(code) };
    // SAFETY: the image stays alive in `module` for the duration of the call.
    assert_eq!(unsafe { call() }, values.iter().sum::<i64>());
}
