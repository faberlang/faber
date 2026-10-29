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
    assert_eq!(diagnostic_arg(&exported_member, "member"), Some("VALUE"));

    let missing_member =
        namespace_call_diagnostic(&unit, &path_expr(namespace), absent, &interner, &exports)
            .expect("an unexported member is rejected");
    assert_eq!(
        diagnostic_arg(&missing_member, "reason"),
        Some("namespace_member_not_exported")
    );
    assert_eq!(diagnostic_arg(&missing_member, "member"), Some("missing"));

    let nested_receiver = field_expr(path_expr(namespace), nested);
    let nested_call =
        namespace_call_diagnostic(&unit, &nested_receiver, member, &interner, &exports)
            .expect("calls through a nested namespace are rejected");
    assert_eq!(
        diagnostic_arg(&nested_call, "reason"),
        Some("nested_namespace_call")
    );
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
