//! Parser coverage for explicit call-site type arguments.

use crate::test_support::parse_ok_la;
use radix_syntax::{ExprKind, IfBody, StmtKind, TypeExprKind};

fn first_call_expr(result: &crate::ParseResult) -> &radix_syntax::CallExpr {
    let program = result.program.as_ref().expect("program");
    let stmt = program.statements.last().expect("statement");
    let StmtKind::Incipit(entry) = &stmt.kind else {
        panic!("expected incipit");
    };
    let IfBody::Block(block) = &entry.body else {
        panic!("expected block body");
    };
    let StmtKind::Var(var) = &block.statements[0].kind else {
        panic!("expected var decl");
    };
    let init = var.init.as_ref().expect("initializer");
    let ExprKind::Call(call) = &init.kind else {
        panic!("expected call expression, got {:?}", init.kind);
    };
    call
}

fn generic_arg_type(arg: &radix_syntax::GenericArgExpr) -> &radix_syntax::TypeExpr {
    match arg {
        radix_syntax::GenericArgExpr::Type(ty) => ty,
        other => panic!("expected type generic argument, got {other:?}"),
    }
}

#[test]
fn parses_explicit_type_args_on_direct_call() {
    let result = parse_ok_la(
        r#"incipit {
    sit textus x ← id<textus>("salve")
}"#,
    );
    let call = first_call_expr(&result);
    assert_eq!(call.type_args.len(), 1);
    assert!(matches!(
        generic_arg_type(&call.type_args[0]).kind,
        TypeExprKind::Named(_, _)
    ));
}

#[test]
fn parses_explicit_type_args_on_member_call() {
    let result = parse_ok_la(
        r#"incipit {
    sit textus x ← module.f<textus>("x")
}"#,
    );
    let call = first_call_expr(&result);
    assert_eq!(call.type_args.len(), 1);
    let ExprKind::Member(member) = &call.callee.kind else {
        panic!("expected member callee");
    };
    assert!(matches!(member.object.kind, ExprKind::Ident(_)));
}

#[test]
fn parses_explicit_type_args_on_keyword_import_alias_method_call() {
    let result = parse_ok_la(
        r#"incipit {
    sit textus x ← solum.lege<textus>("x")
}"#,
    );
    let call = first_call_expr(&result);
    assert_eq!(call.type_args.len(), 1);
    let ExprKind::Member(member) = &call.callee.kind else {
        panic!("expected member callee");
    };
    assert!(matches!(member.object.kind, ExprKind::Ident(_)));
}

#[test]
fn parses_explicit_type_args_on_receiver_method_call() {
    let result = parse_ok_la(
        r"incipit {
    sit octeti frame ← receiver.method<octeti>()
}",
    );
    let call = first_call_expr(&result);
    assert_eq!(call.type_args.len(), 1);
    let ExprKind::Member(member) = &call.callee.kind else {
        panic!("expected member callee");
    };
    assert!(matches!(member.object.kind, ExprKind::Ident(_)));
}

#[test]
fn comparison_expression_still_parses_as_binary() {
    let result = crate::test_support::parse_recovering_ascii_comparison_la(
        r"incipit {
    sit bivalens ok ← a < b
}",
    );
    let program = result.program.as_ref().expect("program");
    let stmt = program.statements.last().expect("statement");
    let StmtKind::Incipit(entry) = &stmt.kind else {
        panic!("expected incipit");
    };
    let IfBody::Block(block) = &entry.body else {
        panic!("expected block body");
    };
    let StmtKind::Var(var) = &block.statements[0].kind else {
        panic!("expected var decl");
    };
    let init = var.init.as_ref().expect("initializer");
    assert!(matches!(init.kind, ExprKind::Binary(_)));
}

#[test]
fn parses_multiple_type_args_on_direct_call() {
    let result = parse_ok_la(
        r"incipit {
    sit bivalens ok ← convert<numerus, textus>(42)
}",
    );
    let call = first_call_expr(&result);
    assert_eq!(call.type_args.len(), 2);
    assert!(matches!(
        generic_arg_type(&call.type_args[0]).kind,
        TypeExprKind::Named(_, _)
    ));
    assert!(matches!(
        generic_arg_type(&call.type_args[1]).kind,
        TypeExprKind::Named(_, _)
    ));
}

#[test]
fn parses_natural_type_args_on_direct_call() {
    // gradus nn.fab: `return linear_from_raw<2, 2, 2>(raw_x, raw_w, raw_b)`
    // PARSE030.expected_expression when call-site typeArguments rejected NATURAL.
    let result = parse_ok_la(
        r"incipit {
    sit x ← linear_from_raw<2, 2, 2>(a, b, c)
}",
    );
    let call = first_call_expr(&result);
    assert_eq!(call.type_args.len(), 3);
    for arg in &call.type_args {
        let TypeExprKind::Named(name, params) = &generic_arg_type(arg).kind else {
            panic!(
                "expected NATURAL type argument, got {:?}",
                generic_arg_type(arg).kind
            );
        };
        assert!(params.is_empty());
        assert_eq!(result.interner.resolve(name.name), "2");
    }
}

#[test]
fn parsers_call_without_type_args_has_empty_type_args() {
    let result = parse_ok_la(
        r#"incipit {
    sit textus x ← greet("salve")
}"#,
    );
    let call = first_call_expr(&result);
    assert!(call.type_args.is_empty());
}

#[test]
fn parses_double_less_than_as_comparison_not_broken_type_args() {
    // Regression: shift must not be parsed as call-site type arguments.
    let result = parse_ok_la(
        r"incipit {
    sit numerus x ← a ⇐ 2
}",
    );
    let program = result.program.as_ref().expect("program");
    let stmt = program.statements.last().expect("statement");
    let StmtKind::Incipit(entry) = &stmt.kind else {
        panic!("expected incipit")
    };
    let IfBody::Block(block) = &entry.body else {
        panic!("expected block body")
    };
    let StmtKind::Var(var) = &block.statements[0].kind else {
        panic!("expected var decl")
    };
    let init = var.init.as_ref().expect("initializer");
    assert!(matches!(init.kind, ExprKind::Binary(_)));
}
