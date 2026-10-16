//! HIR lowering preserves explicit call-site type arguments.

use super::{LowerRequest, lower};
use crate::hir::{HirExpressionKind, HirStatementKind};
use crate::semantic::{Resolver, TypeTable, passes};

fn prepare_lower_pipeline(source: &str) -> (crate::syntax::Program, Resolver, TypeTable, crate::lexer::Interner) {
    let lex_result = crate::lexer::lex(source);
    let parse_result = crate::parser::parse(lex_result);
    assert!(parse_result.success(), "parse failed: {:?}", parse_result.errors);

    let program = parse_result.program.expect("expected parsed program");
    let mut interner = parse_result.interner;
    let mut resolver = Resolver::new();
    let mut types = TypeTable::new();

    crate::builtins::register(&mut resolver, &mut types, &mut interner);
    crate::builtins::register_frame_types_if_needed(&program, &mut resolver, &mut types, &mut interner, None);

    assert!(passes::collect::collect(&program, &mut resolver, &mut types).is_ok());
    if let Err(errors) = passes::resolve::resolve(&program, &mut resolver, &interner, &mut types) {
        panic!("resolve failed: {errors:?}");
    }

    (program, resolver, types, interner)
}

fn lower_source(source: &str) -> crate::hir::LoweredProgram {
    let (program, mut resolver, mut types, mut interner) = prepare_lower_pipeline(source);
    let (lowered, errors) = lower(LowerRequest {
        program: &program,
        resolver: &mut resolver,
        types: &mut types,
        interner: &mut interner,
        cli_program: None,
        locale_pack: None,
        annotation_contracts: None,
    });
    assert!(errors.is_empty(), "lower failed: {errors:?}");
    lowered
}

fn assert_entry_method_call_type_args(source: &str, expected: usize) {
    let lowered = lower_source(source);
    let entry = lowered.hir.entry.expect("expected entry block");
    for stmt in &entry.statements {
        let HirStatementKind::Local(local) = &stmt.kind else {
            continue;
        };
        let Some(init) = local.init.as_ref() else {
            continue;
        };
        if let HirExpressionKind::MethodCall(_, _, type_args, _, _) = &init.kind {
            assert_eq!(type_args.len(), expected);
            return;
        }
    }
    panic!("expected method call in entry block");
}

#[test]
fn lowers_receiver_method_call_with_explicit_type_args() {
    assert_entry_method_call_type_args(
        r#"genus frame {
    functio method<T>(T seed) → T {
        redde seed
    }
}

incipit {
    sit frame s ← frame {}
    sit textus data ← s.method<textus>("x")
}"#,
        1,
    );
}

#[test]
fn rejects_template_call_with_explicit_type_args_at_lowering() {
    let (program, mut resolver, mut types, mut interner) = prepare_lower_pipeline(
        r#"incipit {
    sit _ s ← "§"<textus>("x")
}"#,
    );
    let (lowered, errors) = lower(LowerRequest {
        program: &program,
        resolver: &mut resolver,
        types: &mut types,
        interner: &mut interner,
        cli_program: None,
        locale_pack: None,
        annotation_contracts: None,
    });
    assert!(
        errors.iter().any(|err| err
            .args
            .iter()
            .any(|arg| arg.name == "issue" && arg.value == "template_type_args")),
        "expected lowering error, got: {errors:?}"
    );
    let entry = lowered.hir.entry.expect("expected entry block");
    let HirStatementKind::Local(local) = &entry.statements[0].kind else {
        panic!("expected local stmt");
    };
    let init = local.init.as_ref().expect("init");
    assert!(matches!(init.kind, HirExpressionKind::Error));
}
