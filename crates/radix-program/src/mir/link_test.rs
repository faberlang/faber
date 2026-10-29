//! Focused behavior tests for package-MIR linking and rewrite decisions.

use super::*;
use radix_module::hir::visit::HirVisitorMut;
use radix_module::lexer::Span;

fn path_expr(def_id: DefId) -> HirExpression {
    HirExpression {
        id: radix_module::hir::HirId(0),
        kind: HirExpressionKind::Path(def_id),
        ty: None,
        span: Span::default(),
    }
}

fn field_expr(receiver: HirExpression, member: radix_module::lexer::Symbol) -> HirExpression {
    HirExpression {
        id: receiver.id,
        kind: HirExpressionKind::Field(Box::new(receiver), member),
        ty: None,
        span: Span::default(),
    }
}

fn arg_value(def_id: DefId) -> HirCallArg {
    HirCallArg {
        name: None,
        spread: false,
        marker: None,
        expr: path_expr(def_id),
        span: Span::default(),
    }
}

fn diagnostic_arg<'a>(diagnostic: &'a Diagnostic, name: &str) -> Option<&'a str> {
    diagnostic
        .args
        .iter()
        .find(|arg| arg.name == name)
        .map(|arg| arg.value.as_str())
}

#[test]
fn namespace_call_diagnostics_distinguish_member_and_nested_failures() {
    let unit = PathBuf::from("unit.fab");
    let mut interner = Interner::default();
    let member = interner.intern("VALUE");
    let absent = interner.intern("missing");
    let nested = interner.intern("child");
    let namespace = DefId(17);
    let mut exports = NamespaceExports::new();
    exports.insert(
        (unit.clone(), namespace),
        ["VALUE".to_owned()].into_iter().collect(),
    );

    let exported_member =
        namespace_call_diagnostic(&unit, &path_expr(namespace), member, &interner, &exports)
            .expect("an exported non-function member is unsupported as a call");
    assert_eq!(
        diagnostic_arg(&exported_member, "reason"),
        Some("non_function_namespace_member")
    );
    assert_eq!(exported_member.code, Some("PKG001"));
    assert_eq!(diagnostic_arg(&exported_member, "member"), Some("VALUE"));

    let missing_member =
        namespace_call_diagnostic(&unit, &path_expr(namespace), absent, &interner, &exports)
            .expect("an unexported member is rejected");
    assert_eq!(
        diagnostic_arg(&missing_member, "reason"),
        Some("namespace_member_not_exported")
    );
    assert_eq!(missing_member.code, Some("PKG001"));
    assert_eq!(diagnostic_arg(&missing_member, "member"), Some("missing"));

    let nested_receiver = field_expr(path_expr(namespace), nested);
    let nested_call =
        namespace_call_diagnostic(&unit, &nested_receiver, member, &interner, &exports)
            .expect("calls through a nested namespace are rejected");
    assert_eq!(
        diagnostic_arg(&nested_call, "reason"),
        Some("nested_namespace_call")
    );
    assert_eq!(nested_call.code, Some("PKG001"));
    assert_eq!(diagnostic_arg(&nested_call, "member"), Some("child.VALUE"));
}

#[test]
fn namespace_rewrite_links_calls_and_const_reads_without_losing_types() {
    let unit = PathBuf::from("unit.fab");
    let namespace = DefId(21);
    let mut interner = Interner::default();
    let method = interner.intern("compute");
    let target = DefId(2_000_000_001);
    let mut targets = NamespaceCallTargets::new();
    targets.insert((unit.clone(), namespace, "compute".to_owned()), target);
    let mut const_targets = NamespaceDataMemberTargets::new();
    const_targets.insert(
        (unit.clone(), namespace, "VALUE".to_owned()),
        DefId(2_000_000_002),
    );

    let call_arg = arg_value(DefId(90));
    let mut call = HirExpression {
        id: radix_module::hir::HirId(1),
        kind: HirExpressionKind::Call(
            Box::new(field_expr(path_expr(namespace), method)),
            vec![radix_types::TypeId(13)],
            vec![call_arg.clone()],
            None,
        ),
        ty: None,
        span: Span::default(),
    };
    let mut diagnostics = Vec::new();
    rewrite_expr(
        &unit,
        &mut call,
        &interner,
        &targets,
        &const_targets,
        &NamespaceExports::new(),
        &mut diagnostics,
    );
    assert!(diagnostics.is_empty());
    let HirExpressionKind::Call(callee, type_args, args, _) = call.kind else {
        panic!("a linked namespace call becomes a direct synthetic call");
    };
    assert!(matches!(callee.kind, HirExpressionKind::Path(def) if def == target));
    assert_eq!(type_args, vec![radix_types::TypeId(13)]);
    assert_eq!(args, vec![call_arg]);

    let value = interner.intern("VALUE");
    let value_type = radix_types::TypeId(27);
    let mut read = HirExpression {
        ty: Some(value_type),
        ..field_expr(path_expr(namespace), value)
    };
    rewrite_expr(
        &unit,
        &mut read,
        &interner,
        &targets,
        &const_targets,
        &NamespaceExports::new(),
        &mut diagnostics,
    );
    assert!(matches!(
        read.kind,
        HirExpressionKind::Path(DefId(2_000_000_002))
    ));
    assert_eq!(read.ty, Some(value_type));
    assert!(diagnostics.is_empty());
}

#[test]
fn linked_method_call_recovers_failable_callee_type_and_receiver_argument() {
    let mut types = TypeTable::new();
    let int = types.primitive(Primitive::Numerus);
    let receiver_def = DefId(73);
    let receiver_ty = types.intern(Type::Struct(receiver_def));
    let function_ty = types.function(radix_types::FuncSig {
        type_params: Vec::new(),
        params: Vec::new(),
        ret: int,
        err: Some(int),
        is_async: false,
        is_generator: false,
    });
    let mut interner = Interner::default();
    let method = interner.intern("load");
    let target = DefId(2_000_000_010);
    let methods = [((receiver_def, "load".to_owned()), target)]
        .into_iter()
        .collect();
    let method_function_types = [((receiver_def, "load".to_owned()), function_ty)]
        .into_iter()
        .collect();
    let receiver = HirExpression {
        ty: Some(receiver_ty),
        ..path_expr(DefId(74))
    };
    let original_arg = arg_value(DefId(75));
    let mut call = HirExpression {
        id: radix_module::hir::HirId(4),
        kind: HirExpressionKind::MethodCall(
            Box::new(receiver.clone()),
            method,
            vec![int],
            vec![original_arg.clone()],
            None,
        ),
        ty: Some(int),
        span: Span::default(),
    };
    let mut rewriter = LibraryMethodCallRewriter {
        types: &types,
        interner: &interner,
        methods: &methods,
        method_function_types: &method_function_types,
        diagnostics: Vec::new(),
    };
    rewriter.visit_expr_mut(&mut call);

    let HirExpressionKind::Call(callee, type_args, args, _) = call.kind else {
        panic!("a linked nominal method becomes a direct synthetic call");
    };
    assert!(matches!(callee.kind, HirExpressionKind::Path(def) if def == target));
    assert_eq!(callee.ty, Some(function_ty));
    assert!(matches!(types.get(function_ty), Type::Func(sig) if sig.err == Some(int)));
    assert_eq!(type_args, vec![int]);
    assert_eq!(args.len(), 2);
    assert_eq!(args[0].expr, receiver);
    assert_eq!(args[1], original_arg);
    assert!(rewriter.diagnostics.is_empty());
}

#[test]
fn linker_allocates_distinct_const_sources_and_exposes_only_public_members() {
    let caller = PathBuf::from("caller.fab");
    let source = PathBuf::from("source.fab");
    let import = DefId(120);
    let mut links = PackageMirLinkAccumulator::default();
    let mut interner = Interner::default();
    let public_name = interner.intern("PUBLIC");
    let private_name = interner.intern("PRIVATE");
    let value = path_expr(DefId(121));
    let public = TopLevelConst {
        name: "PUBLIC".to_owned(),
        def_id: DefId(122),
        konst: HirConst {
            name: public_name,
            ty: None,
            value: value.clone(),
            mutable: false,
            is_await: false,
            statica: None,
        },
    };
    let private = TopLevelConst {
        name: "PRIVATE".to_owned(),
        def_id: DefId(123),
        konst: HirConst {
            name: private_name,
            ty: None,
            value,
            mutable: false,
            is_await: false,
            statica: None,
        },
    };

    link_const_member(&mut links, &caller, import, &source, &public, true);
    link_const_member(&mut links, &caller, import, &source, &private, false);

    let public_synthetic = DefId(PACKAGE_MIR_SYNTHETIC_DEF_BASE);
    let private_synthetic = DefId(PACKAGE_MIR_SYNTHETIC_DEF_BASE + 1);
    assert_eq!(links.next_synthetic, PACKAGE_MIR_SYNTHETIC_DEF_BASE + 2);
    assert_eq!(
        links.source_rewrites.get(&(source.clone(), public.def_id)),
        Some(&public_synthetic)
    );
    assert_eq!(
        links.source_rewrites.get(&(source.clone(), private.def_id)),
        Some(&private_synthetic)
    );
    assert_eq!(
        links
            .data_member_targets
            .get(&(caller.clone(), import, "PUBLIC".to_owned())),
        Some(&public_synthetic)
    );
    assert!(
        !links
            .data_member_targets
            .contains_key(&(caller, import, "PRIVATE".to_owned()))
    );
    assert_eq!(
        links
            .data_members
            .iter()
            .map(|member| member.synthetic)
            .collect::<Vec<_>>(),
        vec![public_synthetic, private_synthetic]
    );
}

#[test]
fn linker_refusal_diagnostics_keep_issue_args_and_consumer_policy() {
    let unit = PathBuf::from("caller.fab");
    let mut diagnostics = Vec::new();
    push_library_import_unsupported(
        &mut diagnostics,
        &unit,
        "provider:module",
        PackageMirConsumer::Interpreted,
    );
    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].code, Some("PKG001"));
    assert_eq!(
        diagnostic_arg(&diagnostics[0], "issue"),
        Some("package_mir_library_imports_unsupported")
    );
    assert_eq!(
        diagnostic_arg(&diagnostics[0], "import"),
        Some("provider:module")
    );

    let mut external_diagnostics = Vec::new();
    push_library_import_unsupported(
        &mut external_diagnostics,
        &unit,
        "provider:module",
        PackageMirConsumer::ExternalTarget,
    );
    assert!(external_diagnostics.is_empty());

    push_companion_unresolved(
        &mut diagnostics,
        &unit,
        "provider:module",
        "backward_step",
        PackageMirConsumer::Interpreted,
    );
    let companion = diagnostics.last().expect("companion refusal diagnostic");
    assert_eq!(companion.code, Some("PKG001"));
    assert_eq!(
        diagnostic_arg(companion, "issue"),
        Some("package_mir_companion_unresolved")
    );
    assert_eq!(diagnostic_arg(companion, "import"), Some("provider:module"));
    assert_eq!(
        diagnostic_arg(companion, "companion"),
        Some("backward_step")
    );
}

#[test]
fn namespace_rewrite_walks_optional_non_null_and_conditional_payloads() {
    let unit = PathBuf::from("nested.fab");
    let namespace = DefId(81);
    let call_target = DefId(2_000_000_081);
    let const_target = DefId(2_000_000_082);
    let mut interner = Interner::default();
    let verb = interner.intern("run");
    let value = interner.intern("LIMIT");
    let mut calls = NamespaceCallTargets::new();
    calls.insert((unit.clone(), namespace, "run".to_owned()), call_target);
    let mut members = NamespaceDataMemberTargets::new();
    members.insert((unit.clone(), namespace, "LIMIT".to_owned()), const_target);

    let nested_call_arg = HirCallArg {
        name: Some(interner.intern("payload")),
        spread: true,
        marker: None,
        expr: HirExpression {
            id: radix_module::hir::HirId(2),
            kind: HirExpressionKind::MethodCall(
                Box::new(path_expr(namespace)),
                verb,
                vec![radix_types::TypeId(9)],
                vec![arg_value(DefId(82))],
                None,
            ),
            ty: None,
            span: Span::default(),
        },
        span: Span::default(),
    };
    let original_arg_name = nested_call_arg.name;
    let optional = HirExpression {
        id: radix_module::hir::HirId(3),
        kind: HirExpressionKind::OptionalChain(
            Box::new(path_expr(DefId(83))),
            HirOptionalChainKind::Call(vec![nested_call_arg.clone()]),
        ),
        ty: None,
        span: Span::default(),
    };
    let non_null = HirExpression {
        id: radix_module::hir::HirId(4),
        kind: HirExpressionKind::NonNull(
            Box::new(path_expr(DefId(84))),
            radix_module::hir::HirNonNullKind::Index(Box::new(HirExpression {
                ty: Some(radix_types::TypeId(12)),
                ..field_expr(path_expr(namespace), value)
            })),
        ),
        ty: None,
        span: Span::default(),
    };
    let mut expression = HirExpression {
        id: radix_module::hir::HirId(5),
        kind: HirExpressionKind::Si {
            cond: Box::new(path_expr(DefId(85))),
            then_block: HirBlock::simple(vec![], Some(Box::new(optional)), Span::default()),
            then_catch: None,
            else_block: Some(HirBlock::simple(
                vec![],
                Some(Box::new(non_null)),
                Span::default(),
            )),
        },
        ty: None,
        span: Span::default(),
    };
    let mut diagnostics = Vec::new();
    rewrite_expr(
        &unit,
        &mut expression,
        &interner,
        &calls,
        &members,
        &NamespaceExports::new(),
        &mut diagnostics,
    );
    assert!(diagnostics.is_empty());

    let HirExpressionKind::Si {
        then_block,
        else_block: Some(else_block),
        ..
    } = expression.kind
    else {
        panic!("conditional expression keeps both branches");
    };
    let optional = then_block.expr.expect("optional branch expression");
    let HirExpressionKind::OptionalChain(_, HirOptionalChainKind::Call(args)) = optional.kind
    else {
        panic!("optional call structure is preserved");
    };
    assert_eq!(args[0].name, original_arg_name);
    assert!(args[0].spread);
    assert_eq!(args[0].marker, None);
    let HirExpressionKind::Call(callee, type_args, forwarded, _) = &args[0].expr.kind else {
        panic!("namespace method inside optional-call arguments is linked");
    };
    assert_eq!(callee.kind, HirExpressionKind::Path(call_target));
    assert_eq!(type_args, &vec![radix_types::TypeId(9)]);
    assert_eq!(forwarded, &vec![arg_value(DefId(82))]);

    let non_null = else_block.expr.expect("non-null branch expression");
    let radix_module::hir::HirExpressionKind::NonNull(
        _,
        radix_module::hir::HirNonNullKind::Index(index),
    ) = non_null.kind
    else {
        panic!("non-null index structure is preserved");
    };
    assert_eq!(index.kind, HirExpressionKind::Path(const_target));
    assert_eq!(index.ty, Some(radix_types::TypeId(12)));
}

fn handled_with_calls(
    binding_ty: Option<TypeId>,
    binding: Symbol,
    callees: &[DefId],
) -> HirExpression {
    let statements = callees
        .iter()
        .map(|def_id| HirStatement {
            id: radix_module::hir::HirId(0),
            kind: HirStatementKind::Expr(HirExpression {
                id: radix_module::hir::HirId(0),
                kind: HirExpressionKind::Call(Box::new(path_expr(*def_id)), vec![], vec![], None),
                ty: None,
                span: Span::default(),
            }),
            span: Span::default(),
        })
        .collect();
    HirExpression {
        id: radix_module::hir::HirId(0),
        kind: HirExpressionKind::Handled {
            body: HirBlock::simple(statements, None, Span::default()),
            catch: Box::new(HirCape {
                binding_def_id: DefId(90),
                binding_name: binding,
                binding_ty,
                body: HirBlock::default(),
                span: Span::default(),
            }),
        },
        ty: None,
        span: Span::default(),
    }
}

#[test]
fn synthetic_failable_calls_recover_only_unambiguous_unresolved_cape_types() {
    let mut types = TypeTable::new();
    let number = types.primitive(Primitive::Numerus);
    let text = types.primitive(Primitive::Textus);
    let ignotum = types.primitive(Primitive::Ignotum);
    let failable_number = types.function(radix_types::FuncSig {
        type_params: Vec::new(),
        params: Vec::new(),
        ret: number,
        err: Some(number),
        is_async: false,
        is_generator: false,
    });
    let failable_text = types.function(radix_types::FuncSig {
        type_params: Vec::new(),
        params: Vec::new(),
        ret: text,
        err: Some(text),
        is_async: false,
        is_generator: false,
    });
    let method_types = [(DefId(101), failable_number), (DefId(102), failable_text)]
        .into_iter()
        .collect();
    let mut interner = Interner::default();
    let caught = interner.intern("caught");

    let mut agreed = handled_with_calls(Some(ignotum), caught, &[DefId(101)]);
    SyntheticCapeBindingPass {
        method_types: &method_types,
        types: &types,
    }
    .visit_expr_mut(&mut agreed);
    let HirExpressionKind::Handled { catch, .. } = agreed.kind else {
        panic!("handled expression is retained");
    };
    assert_eq!(catch.binding_ty, Some(number));

    let mut ambiguous = handled_with_calls(Some(ignotum), caught, &[DefId(101), DefId(102)]);
    SyntheticCapeBindingPass {
        method_types: &method_types,
        types: &types,
    }
    .visit_expr_mut(&mut ambiguous);
    let HirExpressionKind::Handled { catch, .. } = ambiguous.kind else {
        panic!("handled expression is retained");
    };
    assert_eq!(catch.binding_ty, Some(ignotum));

    let mut already_typed = handled_with_calls(Some(text), caught, &[DefId(101)]);
    SyntheticCapeBindingPass {
        method_types: &method_types,
        types: &types,
    }
    .visit_expr_mut(&mut already_typed);
    let HirExpressionKind::Handled { catch, .. } = already_typed.kind else {
        panic!("handled expression is retained");
    };
    assert_eq!(catch.binding_ty, Some(text));
}

#[test]
fn linked_method_without_recoverable_signature_is_left_as_method_call() {
    let mut types = TypeTable::new();
    let number = types.primitive(Primitive::Numerus);
    let receiver_def = DefId(110);
    let receiver_ty = types.intern(Type::Struct(receiver_def));
    let mut interner = Interner::default();
    let method = interner.intern("load");
    let methods = [((receiver_def, "load".to_owned()), DefId(2_000_000_110))]
        .into_iter()
        .collect();
    let receiver = HirExpression {
        ty: Some(receiver_ty),
        ..path_expr(DefId(111))
    };
    let mut call = HirExpression {
        id: radix_module::hir::HirId(6),
        kind: HirExpressionKind::MethodCall(
            Box::new(receiver),
            method,
            vec![],
            vec![arg_value(DefId(112))],
            None,
        ),
        ty: Some(number),
        span: Span::default(),
    };
    let original = call.clone();
    let mut rewriter = LibraryMethodCallRewriter {
        types: &types,
        interner: &interner,
        methods: &methods,
        method_function_types: &HashMap::new(),
        diagnostics: Vec::new(),
    };
    rewriter.visit_expr_mut(&mut call);

    assert_eq!(call.kind, original.kind);
    assert_eq!(call.ty, original.ty);
    assert_eq!(rewriter.diagnostics.len(), 1);
}
