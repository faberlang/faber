//! Contract bounds on type parameters and generic `implet` (DECISIONS D1.1,
//! D1.2, operator default F1).
//!
//! A type parameter names the contracts its witnesses must declare with
//! `implet`; several bounds join with `∩` (the comma separates parameters).
//! A genus `implet` clause names each contract with its type arguments in
//! full. Arity and conformance are semantic checks, not parser checks.

use radix_syntax::{ClassDecl, FuncDecl, StmtKind, TypeExprKind};

use crate::test_support::parse_ok;

fn first_function(source: &str) -> FuncDecl {
    let program = parse_ok(source).program.expect("program");
    for stmt in program.statements {
        if let StmtKind::Func(decl) = stmt.kind {
            return decl;
        }
    }
    panic!("no function in {source:?}");
}

fn first_class(source: &str) -> ClassDecl {
    let program = parse_ok(source).program.expect("program");
    for stmt in program.statements {
        if let StmtKind::Class(decl) = stmt.kind {
            return decl;
        }
    }
    panic!("no genus in {source:?}");
}

#[test]
fn type_parameter_accepts_generic_contract_bound() {
    let func = first_function("functio maior<T implet Ordinabile<T>>(T a, T b) → T { redde a }\n");
    assert_eq!(func.type_params.len(), 1);
    let bounds = &func.type_params[0].bounds;
    assert_eq!(bounds.len(), 1);
    assert_eq!(bounds[0].args.len(), 1);
    assert!(matches!(bounds[0].args[0].kind, TypeExprKind::Named(_, _)));
}

#[test]
fn several_bounds_join_with_cap_and_comma_separates_parameters() {
    let func = first_function("functio f<T implet A<T> ∩ B, U>(T a, U b) → T { redde a }\n");
    assert_eq!(func.type_params.len(), 2);
    assert_eq!(func.type_params[0].bounds.len(), 2);
    assert!(func.type_params[0].bounds[1].args.is_empty());
    assert!(func.type_params[1].bounds.is_empty());
}

#[test]
fn bound_precedes_default() {
    let func = first_function("functio f<T implet A<T> = numerus>(T a) → T { redde a }\n");
    let param = &func.type_params[0];
    assert_eq!(param.bounds.len(), 1);
    assert!(param.default.is_some());
}

#[test]
fn genus_implet_takes_type_arguments() {
    let class = first_class(
        "genus Persona implet Ordinabile<Persona>, Nominatum {\n    varia textus nomen\n}\n",
    );
    assert_eq!(class.implements.len(), 2);
    assert_eq!(class.implements[0].args.len(), 1);
    assert!(class.implements[1].args.is_empty());
}

#[test]
fn genus_implet_accepts_cap_between_generic_contracts() {
    let class = first_class("genus Arca<T> implet A<T> ∩ B<lista<T>> {\n    varia T x\n}\n");
    assert_eq!(class.implements.len(), 2);
    assert_eq!(class.implements[1].args.len(), 1);
}
