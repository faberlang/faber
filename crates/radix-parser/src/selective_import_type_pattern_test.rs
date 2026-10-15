//! BUG-92EE4566: a selective import's member binding is a `casu` type-pattern target.
//!
//! Selective members are the only import form whose binding resolves as a
//! bare type (`bind_selective_type_import`), so the parser routes them as
//! type-pattern candidates. Named and wildcard imports bind module
//! namespaces whose members are used qualified; their bindings stay value
//! patterns. The scrutinee's closed-set rule (SEM010) owns type-vs-value.

use crate::test_support::parse_ok;
use radix_syntax::{Pattern, StmtKind, TypeExprKind};

fn discerne_first_arm_pattern(
    result: &crate::ParseResult,
    func_index: usize,
    arm_index: usize,
) -> &Pattern {
    let program = result.program.as_ref().expect("program");
    let StmtKind::Func(func) = &program.statements[func_index].kind else {
        panic!("expected function");
    };
    let body = func.body.as_ref().expect("body");
    let StmtKind::Discerne(stmt) = &body.statements[0].kind else {
        panic!("expected discerne statement");
    };
    &stmt.arms[arm_index].patterns[0]
}

#[test]
fn selective_import_member_is_a_type_pattern_target() {
    let result = parse_ok(
        "importa ex \"./figura.fab\" fixum Figura, Forma ut FormaAliena\n\
         genus Punctum { varia numerus x }\n\
         functio f(Punctum ∪ Figura ∪ FormaAliena u) → vacuum {\n\
         \x20   discerne u {\n\
         \x20       casu Figura { tacet }\n\
         \x20       casu FormaAliena { tacet }\n\
         \x20       casu Punctum sit p { tacet }\n\
         \x20   }\n\
         }",
    );

    for (arm, expected) in [(0, "Figura"), (1, "FormaAliena"), (2, "Punctum")] {
        let pattern = discerne_first_arm_pattern(&result, 2, arm);
        let Pattern::Type(ty, _) = pattern else {
            panic!("arm {arm}: expected type pattern, got {pattern:?}");
        };
        let TypeExprKind::Named(name, _) = &ty.kind else {
            panic!("arm {arm}: expected named type, got {:?}", ty.kind);
        };
        assert_eq!(result.interner.resolve(name.name), expected);
    }
}

#[test]
fn named_import_binding_stays_a_value_pattern() {
    let result = parse_ok(
        "importa ex \"./aux\" aux\n\
         genus Punctum { varia numerus x }\n\
         functio f(Punctum u) → vacuum {\n\
         \x20   discerne u {\n\
         \x20       casu aux sit v { tacet }\n\
         \x20   }\n\
         }",
    );

    let pattern = discerne_first_arm_pattern(&result, 2, 0);
    let Pattern::Ident(name, _) = pattern else {
        panic!("expected value pattern, got {pattern:?}");
    };
    assert_eq!(result.interner.resolve(name.name), "aux");
}

#[test]
fn wildcard_import_alias_stays_a_value_pattern() {
    let result = parse_ok(
        "importa ex \"./gloria\" * ut g\n\
         genus Punctum { varia numerus x }\n\
         functio f(Punctum u) → vacuum {\n\
         \x20   discerne u {\n\
         \x20       casu g sit v { tacet }\n\
         \x20   }\n\
         }",
    );

    let pattern = discerne_first_arm_pattern(&result, 2, 0);
    let Pattern::Ident(name, _) = pattern else {
        panic!("expected value pattern, got {pattern:?}");
    };
    assert_eq!(result.interner.resolve(name.name), "g");
}

#[test]
fn selective_import_keyword_spelled_member_keeps_later_members() {
    let result = parse_ok(
        "importa ex \"./figura.fab\" fixum solum, Figura\n\
         genus Punctum { varia numerus x }\n\
         functio f(Punctum ∪ Figura u) → vacuum {\n\
         \x20   discerne u {\n\
         \x20       casu Figura { tacet }\n\
         \x20   }\n\
         }",
    );

    let pattern = discerne_first_arm_pattern(&result, 2, 0);
    let Pattern::Type(ty, _) = pattern else {
        panic!("expected type pattern, got {pattern:?}");
    };
    let TypeExprKind::Named(name, _) = &ty.kind else {
        panic!("expected named type, got {:?}", ty.kind);
    };
    assert_eq!(result.interner.resolve(name.name), "Figura");
}

#[test]
fn selective_import_keyword_spelled_alias_keeps_later_members() {
    let result = parse_ok(
        "importa ex \"./figura.fab\" fixum Forma ut solum, Figura\n\
         genus Punctum { varia numerus x }\n\
         functio f(Punctum ∪ Figura u) → vacuum {\n\
         \x20   discerne u {\n\
         \x20       casu Figura { tacet }\n\
         \x20   }\n\
         }",
    );

    let pattern = discerne_first_arm_pattern(&result, 2, 0);
    let Pattern::Type(ty, _) = pattern else {
        panic!("expected type pattern, got {pattern:?}");
    };
    let TypeExprKind::Named(name, _) = &ty.kind else {
        panic!("expected named type, got {:?}", ty.kind);
    };
    assert_eq!(result.interner.resolve(name.name), "Figura");
}

#[test]
fn selective_import_trailing_comma_before_declaration_still_ends_list() {
    let result = parse_ok(
        "importa ex \"./figura.fab\" fixum Figura,\n\
         genus Punctum { varia numerus x }",
    );

    let program = result.program.as_ref().expect("program");
    let StmtKind::Class(class) = &program.statements[1].kind else {
        panic!(
            "expected genus declaration after the trailing comma, got {:?}",
            program.statements[1].kind
        );
    };
    assert_eq!(result.interner.resolve(class.name.name), "Punctum");
}
