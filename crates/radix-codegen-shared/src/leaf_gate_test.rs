//! The leaf gate refuses `solum:*` on emit-only targets and admits it on
//! rust / runner / Faber re-emit.

use super::*;
use radix_hir::{HirBlock, HirId, HirStatement, HirStatementKind};
use radix_lexer::{Interner, Span};

fn ad_module(interner: &mut Interner, route: &str) -> HirModule {
    let route = interner.intern(route);
    let expression = HirExpression {
        id: HirId(1),
        kind: HirExpressionKind::Ad {
            route,
            opener: None,
        },
        ty: None,
        span: Span::new(0, 0),
    };
    let statement = HirStatement {
        id: HirId(2),
        kind: HirStatementKind::Expr(expression),
        span: Span::new(0, 0),
    };
    HirModule {
        items: Vec::new(),
        entry: Some(HirBlock::simple(vec![statement], None, Span::new(0, 0))),
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    }
}

fn issue_of(error: &CodegenError) -> Option<&str> {
    error
        .args
        .iter()
        .find(|arg| arg.name == "issue")
        .map(|arg| arg.value.as_str())
}

fn routes_of(hir: &HirModule, interner: &Interner) -> Vec<String> {
    let mut routes = Vec::new();
    collect_ad_routes(hir, interner, &mut routes);
    routes
}

#[test]
fn rust_and_runner_admit_the_n2_leaves() {
    let mut interner = Interner::new();
    let module = ad_module(&mut interner, "solum:lege");
    let routes = routes_of(&module, &interner);
    for target in [
        Target::HirRust,
        Target::MirStepper,
        Target::MirFmir,
        Target::HirFaber,
        Target::HirFhir,
    ] {
        reject_unsupported_leaf(&routes, target)
            .unwrap_or_else(|error| panic!("{target} must declare fs: {error:?}"));
    }
}

#[test]
fn go_and_wasm_reject_fs_by_name() {
    let mut interner = Interner::new();
    let module = ad_module(&mut interner, "solum:lege");
    let routes = routes_of(&module, &interner);
    for target in [Target::HirGo, Target::HirTypeScript, Target::MirWasm] {
        let error =
            reject_unsupported_leaf(&routes, target).expect_err("emit-only target refuses fs");
        assert_eq!(issue_of(&error), Some("leaf_unsupported_on_target"));
        assert!(
            error
                .args
                .iter()
                .any(|arg| arg.name == "leaf" && arg.value == "fs")
        );
        assert!(
            error
                .args
                .iter()
                .any(|arg| arg.name == "route" && arg.value == "solum:lege")
        );
    }
}

#[test]
fn builtins_and_unknown_routes_are_untouched() {
    let mut interner = Interner::new();
    for route in ["processus:exi", "runtime:echo", "ignotum:route"] {
        let module = ad_module(&mut interner, route);
        let routes = routes_of(&module, &interner);
        reject_unsupported_leaf(&routes, Target::HirGo)
            .unwrap_or_else(|error| panic!("{route} is not a leaf: {error:?}"));
    }
}
