//! Parser tests for the top-level `=` constant (st1 R1).
//!
//! Contract: `fixum T NAME = expr` at top level parses to an immutable
//! `compile_time` `VarDecl` that is not `is_static` (the `generis` static keeps
//! that flag until the lock unit). `_` is a legal type slot. A module-level
//! `=` constant claims the build-time embed and `praefixum` exactly as a
//! `generis` declaration does; a block-level constant does not claim the embed.

use crate::ParseResult;
use crate::test_support::{parse_ok, parse_program};
use radix_syntax::{AstMutability, ExprKind, StmtKind, VarDecl};

fn first_decl(result: &ParseResult) -> &VarDecl {
    let program = result.program.as_ref().expect("program");
    let StmtKind::Var(decl) = &program.statements[0].kind else {
        panic!("expected var decl, got {:?}", program.statements[0].kind);
    };
    decl
}

#[test]
fn top_level_fixum_equals_is_a_compile_time_constant() {
    let result = parse_ok("fixum numerus LIMES = 4 * 1024");
    let decl = first_decl(&result);
    assert!(decl.compile_time);
    assert!(!decl.is_static);
    assert_eq!(decl.mutability, AstMutability::Immutable);
    assert!(decl.ty.is_some());
    assert!(decl.init.is_some());
}

#[test]
fn top_level_fixum_hole_type_parses() {
    let result = parse_ok("fixum _ N = 10");
    assert!(first_decl(&result).compile_time);
}

#[test]
fn top_level_fixum_claims_the_embed_and_praefixum_initializers() {
    let result = parse_ok("fixum textus S = insere \"nuntius.txt\"");
    let init = first_decl(&result).init.as_ref().expect("initializer");
    assert!(matches!(init.kind, ExprKind::Insere(_)), "{:?}", init.kind);
    let result = parse_ok("fixum numerus Z = praefixum {\n    redde 7\n}");
    let init = first_decl(&result).init.as_ref().expect("initializer");
    assert!(
        matches!(init.kind, ExprKind::Praefixum(_)),
        "{:?}",
        init.kind
    );
}

#[test]
fn embed_spelling_stays_an_identifier_outside_a_module_constant() {
    // Block level: the embed is not claimed, so no `Insere` node is produced.
    let result = parse_program(
        "functio f() → textus {\n    fixum textus x = insere \"a.txt\"\n    redde x\n}",
    );
    assert!(!format!("{:?}", result.program).contains("Insere("));
    // Module level without a string path: still an ordinary call.
    let result = parse_ok("fixum numerus N = insere(3)");
    let init = first_decl(&result).init.as_ref().expect("initializer");
    assert!(!matches!(init.kind, ExprKind::Insere(_)), "{:?}", init.kind);
}
