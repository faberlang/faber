//! Tests for `instans` type lowering: precision-suffixed (ms, µs, ns) and bare forms.
//!
//! Contract: each `instans<...>` syntax maps to the correct `Type::SizedInstans` variant.

use super::{LowerRequest, lower};
use crate::hir::{HirFunction, HirItemKind};
use crate::lexer::Interner;
use crate::semantic::{InstansPrecision, Primitive, Resolver, Type, TypeTable, passes};

fn lower_source(source: &str) -> (crate::hir::LoweredProgram, Interner, TypeTable) {
    let lex_result = crate::lexer::lex(source);
    let parse_result = crate::parser::parse(lex_result);
    assert!(parse_result.success(), "parse failed: {:?}", parse_result.errors);

    let program = parse_result.program.expect("parsed program");
    let mut interner = parse_result.interner;
    let mut resolver = Resolver::new();
    let mut types = TypeTable::new();

    crate::builtins::register(&mut resolver, &mut types, &mut interner);
    passes::collect::collect(&program, &mut resolver, &mut types).expect("collect");
    passes::resolve::resolve(&program, &mut resolver, &interner, &mut types).expect("resolve");

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
    (lowered, interner, types)
}

fn function_item(lowered: &crate::hir::LoweredProgram) -> &HirFunction {
    let HirItemKind::Function(function) = &lowered.hir.items[0].kind else {
        panic!("expected function item");
    };
    function
}

#[test]
fn lowers_bare_and_sized_instans_types() {
    let source = r"
        functio probe(instans secunda, instans<ms> milli, instans<us> micro, instans<ns> nano) → vacuum {
        }
    ";

    let (lowered, _interner, types) = lower_source(source);
    let func = function_item(&lowered);
    let param_tys: Vec<_> = func.params.iter().map(|param| param.ty).collect();

    assert_eq!(param_tys.len(), 4);
    assert!(matches!(types.get(param_tys[0]), Type::Primitive(Primitive::Instans)));
    assert!(matches!(types.get(param_tys[1]), Type::SizedInstans(InstansPrecision::Millis)));
    assert!(matches!(types.get(param_tys[2]), Type::SizedInstans(InstansPrecision::Micros)));
    assert!(matches!(types.get(param_tys[3]), Type::SizedInstans(InstansPrecision::Nanos)));
}
