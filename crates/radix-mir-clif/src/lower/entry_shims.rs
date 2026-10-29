//! The program entry shims: `faber_clif_incipit` and, for a linked
//! executable, the C `main`.

use super::*;

/// `faber_clif_incipit` (tokens, then the program entry), and in
/// [`EntryMode::Process`] the C `main`. Returns the id of the incipit.
pub(super) fn define_entry_shims(
    state: &mut ModuleState<'_>,
    entry: Option<FuncId>,
    mode: EntryMode,
) -> Lowered<FuncId> {
    let tokens = state.input.tokens.clone();
    let token_data: Vec<DataId> = tokens
        .iter()
        .map(|token| state.string_data(token.as_bytes()))
        .collect::<Lowered<_>>()?;
    let set_tokens = state.runtime_id(&abi::SET_DISPLAY_TOKENS)?;

    // faber_clif_incipit() -> i32: the process exit status the entry hands back
    // (`main exit <expr>`), 0 when the entry returns nothing.
    let mut incipit_signature = state.module.make_signature();
    incipit_signature.returns.push(AbiParam::new(types::I32));
    let incipit_id = state
        .module
        .declare_function(abi::PROGRAM_INCIPIT, Linkage::Export, &incipit_signature)
        .map_err(cranelift_error)?;
    let mut context = state.module.make_context();
    context.func.signature = incipit_signature;
    let mut builder_context = FunctionBuilderContext::new();
    {
        let mut builder = FunctionBuilder::new(&mut context.func, &mut builder_context);
        let block = builder.create_block();
        builder.switch_to_block(block);
        let mut args = Vec::new();
        for (data, token) in token_data.iter().zip(&tokens) {
            let global = state.module.declare_data_in_func(*data, builder.func);
            args.push(builder.ins().symbol_value(state.pointer, global));
            let length = i64::try_from(token.len()).unwrap_or(0);
            args.push(builder.ins().iconst(types::I64, length));
        }
        let set_ref = state.module.declare_func_in_func(set_tokens, builder.func);
        builder.ins().call(set_ref, &args);
        let mut status = builder.ins().iconst(types::I32, 0);
        if let Some(entry) = entry {
            let returns_status = state
                .module
                .declarations()
                .get_function_decl(entry)
                .signature
                .returns
                .first()
                .is_some_and(|param| param.value_type == types::I64);
            let entry_ref = state.module.declare_func_in_func(entry, builder.func);
            let call = builder.ins().call(entry_ref, &[]);
            if returns_status {
                // The status is a `numerus`; the process keeps its low 32 bits,
                // as the runner does.
                let code = builder.inst_results(call)[0];
                status = builder.ins().ireduce(types::I32, code);
            }
        }
        builder.ins().return_(&[status]);
        builder.seal_all_blocks();
        builder.finalize(state.module.isa().frontend_config());
    }
    state
        .module
        .define_function(incipit_id, &mut context)
        .map_err(cranelift_error)?;

    if mode == EntryMode::InProcess {
        return Ok(incipit_id);
    }
    let main_target = state.runtime_id(&abi::MAIN)?;

    // main(argc, argv) -> __faber_clif_v1_main(argc, argv, faber_clif_incipit)
    let mut main_signature = state.module.make_signature();
    main_signature.params.push(AbiParam::new(types::I32));
    main_signature.params.push(AbiParam::new(state.pointer));
    main_signature.returns.push(AbiParam::new(types::I32));
    let main_id = state
        .module
        .declare_function("main", Linkage::Export, &main_signature)
        .map_err(cranelift_error)?;
    let mut context = state.module.make_context();
    context.func.signature = main_signature;
    let mut builder_context = FunctionBuilderContext::new();
    {
        let mut builder = FunctionBuilder::new(&mut context.func, &mut builder_context);
        let block = builder.create_block();
        builder.append_block_params_for_function_params(block);
        builder.switch_to_block(block);
        let count = builder.block_params(block)[0];
        let values = builder.block_params(block)[1];
        let target = state.module.declare_func_in_func(main_target, builder.func);
        let incipit_ref = state.module.declare_func_in_func(incipit_id, builder.func);
        let incipit_address = builder.ins().func_addr(state.pointer, incipit_ref);
        let call = builder
            .ins()
            .call(target, &[count, values, incipit_address]);
        let result = builder.inst_results(call)[0];
        builder.ins().return_(&[result]);
        builder.seal_all_blocks();
        builder.finalize(state.module.isa().frontend_config());
    }
    state
        .module
        .define_function(main_id, &mut context)
        .map_err(cranelift_error)?;
    Ok(incipit_id)
}
