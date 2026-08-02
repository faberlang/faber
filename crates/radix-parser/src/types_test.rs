//! Tests for type-expression parsing: sugar forms, parameterized types, error diagnostics.
//!
//! Contract: each type form produces the expected `TypeExprKind` variant or a structured diagnostic.

use crate::test_support::args_have_no_legacy_message;
use crate::test_support::parse_ok;
use crate::{parse, ParseErrorKind};
use radix_lexer::lex;
use radix_syntax::{FiguraExpr, StmtKind, TypeExprKind};

fn parse_errors_include_issue(source: &str, kind: ParseErrorKind, issue: &str) {
    let result = parse(lex(source));
    let errors = result
        .errors
        .iter()
        .map(|err| (err.kind, err.args.clone()))
        .collect::<Vec<_>>();
    assert!(
        result.errors.iter().any(|err| {
            err.kind == kind
                && err
                    .args
                    .iter()
                    .any(|arg| arg.name == "issue" && arg.value == issue)
        }),
        "expected {kind:?} parse error with issue {issue:?}, got {errors:?}"
    );
}

fn expect_expected_issue(source: &str, issue: &str) -> crate::ParseError {
    let result = parse(lex(source));
    let err = result
        .errors
        .into_iter()
        .find(|err| {
            err.kind == ParseErrorKind::Expected
                && err
                    .args
                    .iter()
                    .any(|arg| arg.name == "issue" && arg.value == issue)
        })
        .expect("structured expected diagnostic");
    assert!(
        args_have_no_legacy_message(&err.args),
        "migrated parser diagnostic should not choose prose at emit site"
    );
    err
}

#[test]
fn rejects_postfix_array_param_type() {
    parse_errors_include_issue(
        "functio sum(numerus[] items) → numerus { redde 0 }",
        ParseErrorKind::InvalidType,
        "postfix_array_syntax",
    );
}

#[test]
fn rejects_postfix_array_variable_type() {
    parse_errors_include_issue(
        "fixum textus[] words ← vacua",
        ParseErrorKind::InvalidType,
        "postfix_array_syntax",
    );
}

#[test]
fn invalid_type_emits_vector_sugar_angle_bracket_form() {
    let source = "functio f(vf32<f32> value) → vacuum {}";
    let result = parse(lex(source));
    let err = result
        .errors
        .iter()
        .find(|err| err.kind == ParseErrorKind::InvalidType)
        .expect("invalid type diagnostic");

    assert!(
        args_have_no_legacy_message(&err.args),
        "migrated diagnostic should not choose prose at emit site"
    );
    assert!(
        err.args
            .iter()
            .any(|arg| arg.name == "issue" && arg.value == "vector_sugar_angle_bracket_form"),
        "expected vector_sugar_angle_bracket_form issue, got {:?}",
        err.args
    );
    assert!(
        err.args.iter().all(|arg| arg.name != "sugar"),
        "non-shaped-sugar issue should not carry a sugar discriminator, got {:?}",
        err.args
    );
}

#[test]
fn invalid_type_tensor_sugar_angle_form_carries_sugar_discriminator() {
    let source = "functio f(tf32<f32> value) → vacuum {}";
    let result = parse(lex(source));
    let err = result
        .errors
        .iter()
        .find(|err| err.kind == ParseErrorKind::InvalidType)
        .expect("invalid type diagnostic");

    assert!(
        args_have_no_legacy_message(&err.args),
        "migrated diagnostic should not choose prose at emit site"
    );
    assert!(
        err.args
            .iter()
            .any(|arg| arg.name == "issue" && arg.value == "type_sugar_angle_bracket_form"),
        "expected type_sugar_angle_bracket_form issue, got {:?}",
        err.args
    );
    assert!(
        err.args
            .iter()
            .any(|arg| arg.name == "sugar" && arg.value == "tensor"),
        "tensor sugar angle form should carry sugar=tensor, got {:?}",
        err.args
    );
}

#[test]
fn union_type_preserves_member_modes_when_flattening() {
    let result = parse_ok("typus MaybeBorrow = de textus ∪ nihil");
    let program = result.program.as_ref().expect("program");
    let StmtKind::TypeAlias(alias) = &program.statements[0].kind else {
        panic!("expected type alias");
    };

    let TypeExprKind::Union(members) = &alias.ty.kind else {
        panic!("expected union type");
    };
    assert_eq!(members.len(), 2);
    assert!(matches!(members[0].mode, Some(radix_syntax::TypeMode::De)));
    assert!(members[1].mode.is_none());

    let TypeExprKind::Named(name, _) = &members[1].kind else {
        panic!("expected named nihil member");
    };
    assert_eq!(result.interner.resolve(name.name), "nihil");
}

#[test]
fn parses_tensor_with_literal_and_infer_figura() {
    let result = parse_ok("functio f(tensor<fractus<f32>, [2, 3]> value) → vacuum {}");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Func(func) = &program.statements[0].kind else {
        panic!("expected function");
    };
    let ty = &func.params[0].ty;
    let TypeExprKind::Tensor(elem, figura) = &ty.kind else {
        panic!("expected tensor type");
    };
    let TypeExprKind::Named(elem_name, _) = &elem.kind else {
        panic!("expected element type");
    };
    assert_eq!(result.interner.resolve(elem_name.name), "fractus");
    let FiguraExpr::Tuple(dims) = figura else {
        panic!("expected tuple figura");
    };
    assert!(matches!(dims[0], FiguraExpr::Literal(2)));
    assert!(matches!(dims[1], FiguraExpr::Literal(3)));
}

#[test]
fn parses_atomic_with_single_element_type() {
    let result = parse_ok("functio f(atomic<i32> cell) → vacuum {}");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Func(func) = &program.statements[0].kind else {
        panic!("expected function");
    };
    let TypeExprKind::Atomic(elem) = &func.params[0].ty.kind else {
        panic!("expected atomic type");
    };
    let TypeExprKind::Named(elem_name, _) = &elem.kind else {
        panic!("expected atomic element type");
    };
    assert_eq!(result.interner.resolve(elem_name.name), "i32");
}

#[test]
fn atomic_extra_type_parameter_emits_structured_expected_issue() {
    let source = "functio f(atomic<i32, u32> cell) → vacuum {}";
    expect_expected_issue(source, "atomic_single_element");
}

#[test]
fn parses_lista_infer_element_hole() {
    let result = parse_ok("functio f(lista<_> value) → vacuum {}");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Func(func) = &program.statements[0].kind else {
        panic!("expected function");
    };
    let ty = &func.params[0].ty;
    let TypeExprKind::Named(name, params) = &ty.kind else {
        panic!("expected lista type");
    };
    assert_eq!(result.interner.resolve(name.name), "lista");
    assert!(matches!(params[0].kind, TypeExprKind::Infer));
}

#[test]
fn parses_qualified_type_path() {
    let result = parse_ok("functio f(terminus.Terminus value) → vacuum {}");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Func(func) = &program.statements[0].kind else {
        panic!("expected function");
    };
    let TypeExprKind::Qualified(path, params) = &func.params[0].ty.kind else {
        panic!("expected qualified type");
    };
    assert!(params.is_empty());
    assert_eq!(path.len(), 2);
    assert_eq!(result.interner.resolve(path[0].name), "terminus");
    assert_eq!(result.interner.resolve(path[1].name), "Terminus");
}

#[test]
fn parses_tensor_with_infer_figura_hole() {
    let result = parse_ok("functio f(tensor<fractus<f32>, _> value) → vacuum {}");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Func(func) = &program.statements[0].kind else {
        panic!("expected function");
    };
    let TypeExprKind::Tensor(_, figura) = &func.params[0].ty.kind else {
        panic!("expected tensor type");
    };
    assert!(matches!(figura, FiguraExpr::Infer));
}

#[test]
fn rejects_tensor_without_figura() {
    parse_errors_include_issue(
        "functio f(tensor<fractus<f32>> value) → vacuum {}",
        ParseErrorKind::Expected,
        "tensor_figura_after_element",
    );
}

#[test]
fn tensor_without_figura_preserves_precise_message() {
    let source = "functio f(tensor<fractus<f32>> value) → vacuum {}";
    expect_expected_issue(source, "tensor_figura_after_element");
}

#[test]
fn sparsa_without_figura_emits_structured_expected_issue() {
    let source = "functio f(sparsa<f32> value) → vacuum {}";
    expect_expected_issue(source, "sparsa_figura_after_element");
}

#[test]
fn vector_without_width_emits_structured_expected_issue() {
    let source = "functio f(vector<f32> value) → vacuum {}";
    expect_expected_issue(source, "vector_width_after_element");
}

#[test]
fn matrix_without_figura_emits_structured_expected_issue() {
    let source = "functio f(matrix<f32> value) → vacuum {}";
    expect_expected_issue(source, "matrix_figura_after_element");
}

#[test]
fn parses_tensor_sugar_with_infer_figura() {
    let result = parse_ok("functio f(tf32 value) → vacuum {}");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Func(func) = &program.statements[0].kind else {
        panic!("expected function");
    };
    let TypeExprKind::Tensor(elem, figura) = &func.params[0].ty.kind else {
        panic!("expected tensor sugar type");
    };
    let TypeExprKind::Named(elem_name, _) = &elem.kind else {
        panic!("expected width element type");
    };
    assert_eq!(result.interner.resolve(elem_name.name), "f32");
    assert!(matches!(figura, FiguraExpr::Infer));
}

#[test]
fn parses_tensor_sugar_with_bracket_figura() {
    let result = parse_ok("functio f(ti64[2, 3] value) → vacuum {}");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Func(func) = &program.statements[0].kind else {
        panic!("expected function");
    };
    let TypeExprKind::Tensor(elem, figura) = &func.params[0].ty.kind else {
        panic!("expected tensor sugar type");
    };
    let TypeExprKind::Named(elem_name, _) = &elem.kind else {
        panic!("expected width element type");
    };
    assert_eq!(result.interner.resolve(elem_name.name), "i64");
    let FiguraExpr::Tuple(dims) = figura else {
        panic!("expected tuple figura");
    };
    assert!(matches!(dims[0], FiguraExpr::Literal(2)));
    assert!(matches!(dims[1], FiguraExpr::Literal(3)));
}

#[test]
fn parses_vector_long_form() {
    let result = parse_ok("functio f(vector<fractus<f32>, 4> a) → vacuum {}");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Func(func) = &program.statements[0].kind else {
        panic!("expected function");
    };
    let TypeExprKind::Vector(elem, width) = &func.params[0].ty.kind else {
        panic!("expected vector long form");
    };
    let TypeExprKind::Named(elem_name, elem_params) = &elem.kind else {
        panic!("expected element type");
    };
    assert_eq!(result.interner.resolve(elem_name.name), "fractus");
    let TypeExprKind::Named(width_elem, _) = &elem_params[0].kind else {
        panic!("expected fractus width");
    };
    assert_eq!(result.interner.resolve(width_elem.name), "f32");
    assert!(matches!(width, FiguraExpr::Literal(4)));
}

#[test]
fn parses_vector_sugar_bracket_form() {
    let result = parse_ok("functio f(vf32[4] b) → vacuum {}");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Func(func) = &program.statements[0].kind else {
        panic!("expected function");
    };
    let TypeExprKind::Vector(sugar_elem, sugar_width) = &func.params[0].ty.kind else {
        panic!("expected vector sugar type");
    };
    let TypeExprKind::Named(sugar_elem_name, _) = &sugar_elem.kind else {
        panic!("expected sugar element");
    };
    assert_eq!(result.interner.resolve(sugar_elem_name.name), "f32");
    let FiguraExpr::Tuple(dims) = sugar_width else {
        panic!("expected bracket width");
    };
    assert!(matches!(dims[0], FiguraExpr::Literal(4)));
}

#[test]
fn parses_matrix_long_form() {
    let result = parse_ok("functio f(matrix<fractus<f32>, [4, 4]> a) → vacuum {}");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Func(func) = &program.statements[0].kind else {
        panic!("expected function");
    };
    let TypeExprKind::Matrix(elem, shape) = &func.params[0].ty.kind else {
        panic!("expected matrix long form");
    };
    let TypeExprKind::Named(elem_name, elem_params) = &elem.kind else {
        panic!("expected element type");
    };
    assert_eq!(result.interner.resolve(elem_name.name), "fractus");
    let TypeExprKind::Named(width_elem, _) = &elem_params[0].kind else {
        panic!("expected fractus width");
    };
    assert_eq!(result.interner.resolve(width_elem.name), "f32");
    let FiguraExpr::Tuple(dims) = shape else {
        panic!("expected matrix shape");
    };
    assert!(matches!(dims[0], FiguraExpr::Literal(4)));
    assert!(matches!(dims[1], FiguraExpr::Literal(4)));
}

#[test]
fn parses_matrix_sugar_bracket_form() {
    let result = parse_ok("functio f(mf32[3, 2] b) → vacuum {}");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Func(func) = &program.statements[0].kind else {
        panic!("expected function");
    };
    let TypeExprKind::Matrix(sugar_elem, sugar_shape) = &func.params[0].ty.kind else {
        panic!("expected matrix sugar type");
    };
    let TypeExprKind::Named(sugar_elem_name, _) = &sugar_elem.kind else {
        panic!("expected sugar element");
    };
    assert_eq!(result.interner.resolve(sugar_elem_name.name), "f32");
    let FiguraExpr::Tuple(dims) = sugar_shape else {
        panic!("expected matrix sugar shape");
    };
    assert!(matches!(dims[0], FiguraExpr::Literal(3)));
    assert!(matches!(dims[1], FiguraExpr::Literal(2)));
}

#[test]
fn matrix_sugar_requires_bracket_shape() {
    parse_errors_include_issue(
        "functio f(mf32 value) → vacuum {}",
        ParseErrorKind::InvalidType,
        "matrix_sugar_missing_shape",
    );
}

#[test]
fn parses_tensor_sugar_rank_zero_brackets() {
    let result = parse_ok("functio f(tf32[] value) → vacuum {}");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Func(func) = &program.statements[0].kind else {
        panic!("expected function");
    };
    let TypeExprKind::Tensor(_, figura) = &func.params[0].ty.kind else {
        panic!("expected tensor sugar type");
    };
    let FiguraExpr::Tuple(dims) = figura else {
        panic!("expected rank-0 tuple figura");
    };
    assert!(dims.is_empty());
}

#[test]
fn parses_sparsa_sugar_with_infer_figura() {
    let result = parse_ok("functio f(sf32 value) → vacuum {}");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Func(func) = &program.statements[0].kind else {
        panic!("expected function");
    };
    let TypeExprKind::Sparsa(elem, figura) = &func.params[0].ty.kind else {
        panic!("expected sparsa sugar type");
    };
    let TypeExprKind::Named(elem_name, _) = &elem.kind else {
        panic!("expected width element type");
    };
    assert_eq!(result.interner.resolve(elem_name.name), "f32");
    assert!(matches!(figura, FiguraExpr::Infer));
}

#[test]
fn parses_sparsa_sugar_with_bracket_figura() {
    let result = parse_ok("functio f(si64[N] value) → vacuum {}");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Func(func) = &program.statements[0].kind else {
        panic!("expected function");
    };
    let TypeExprKind::Sparsa(elem, figura) = &func.params[0].ty.kind else {
        panic!("expected sparsa sugar type");
    };
    let TypeExprKind::Named(elem_name, _) = &elem.kind else {
        panic!("expected width element type");
    };
    assert_eq!(result.interner.resolve(elem_name.name), "i64");
    let FiguraExpr::Tuple(dims) = figura else {
        panic!("expected tuple figura");
    };
    assert!(matches!(dims[0], FiguraExpr::Var(_)));
}

#[test]
fn parses_sparsa_sugar_rank_zero_brackets() {
    let result = parse_ok("functio f(sf64[] value) → vacuum {}");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Func(func) = &program.statements[0].kind else {
        panic!("expected function");
    };
    let TypeExprKind::Sparsa(_, figura) = &func.params[0].ty.kind else {
        panic!("expected sparsa sugar type");
    };
    let FiguraExpr::Tuple(dims) = figura else {
        panic!("expected rank-0 tuple figura");
    };
    assert!(dims.is_empty());
}

#[test]
fn parses_sparsa_sugar_unsigned_width() {
    let result = parse_ok("functio f(su32[4] value) → vacuum {}");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Func(func) = &program.statements[0].kind else {
        panic!("expected function");
    };
    let TypeExprKind::Sparsa(elem, figura) = &func.params[0].ty.kind else {
        panic!("expected sparsa sugar type");
    };
    let TypeExprKind::Named(elem_name, _) = &elem.kind else {
        panic!("expected width element type");
    };
    assert_eq!(result.interner.resolve(elem_name.name), "u32");
    let FiguraExpr::Tuple(dims) = figura else {
        panic!("expected tuple figura");
    };
    assert!(matches!(dims[0], FiguraExpr::Literal(4)));
}

#[test]
fn rejects_sparsa_sugar_angle_bracket_form() {
    parse_errors_include_issue(
        "functio f(sf32<f32> value) → vacuum {}",
        ParseErrorKind::InvalidType,
        "type_sugar_angle_bracket_form",
    );
}

#[test]
fn parses_standalone_width_marker_type() {
    let result = parse_ok("functio f(f32 value) → vacuum {}");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Func(func) = &program.statements[0].kind else {
        panic!("expected function");
    };
    let TypeExprKind::Named(name, params) = &func.params[0].ty.kind else {
        panic!("expected named width type");
    };
    assert_eq!(result.interner.resolve(name.name), "f32");
    assert!(params.is_empty());
}

#[test]
fn rejects_tensor_sugar_angle_bracket_form() {
    parse_errors_include_issue(
        "functio f(tf32<f32> value) → vacuum {}",
        ParseErrorKind::InvalidType,
        "type_sugar_angle_bracket_form",
    );
}

#[test]
fn parses_lista_sugar_unsigned_width() {
    let result = parse_ok("functio f(lu32 rows) → vacuum {}");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Func(func) = &program.statements[0].kind else {
        panic!("expected function");
    };
    let TypeExprKind::Named(lista, params) = &func.params[0].ty.kind else {
        panic!("expected lista sugar desugar");
    };
    assert_eq!(result.interner.resolve(lista.name), "lista");
    assert_eq!(params.len(), 1);
    let TypeExprKind::Named(elem, _) = &params[0].kind else {
        panic!("expected width element");
    };
    assert_eq!(result.interner.resolve(elem.name), "u32");
}

#[test]
fn parses_lista_sugar_float_width() {
    let result = parse_ok("functio f(lf32 flat) → vacuum {}");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Func(func) = &program.statements[0].kind else {
        panic!("expected function");
    };
    let TypeExprKind::Named(lista, params) = &func.params[0].ty.kind else {
        panic!("expected lista sugar desugar");
    };
    assert_eq!(result.interner.resolve(lista.name), "lista");
    let TypeExprKind::Named(elem, _) = &params[0].kind else {
        panic!("expected width element");
    };
    assert_eq!(result.interner.resolve(elem.name), "f32");
}

#[test]
fn rejects_lista_sugar_angle_bracket_form() {
    parse_errors_include_issue(
        "functio f(lu32<u32> rows) → vacuum {}",
        ParseErrorKind::InvalidType,
        "lista_sugar_angle_bracket_form",
    );
}

#[test]
fn rejects_lista_sugar_bracket_suffix() {
    parse_errors_include_issue(
        "functio f(lu32[] rows) → vacuum {}",
        ParseErrorKind::InvalidType,
        "lista_sugar_bracket_form",
    );
}

#[test]
fn parses_sparsa_with_literal_figura() {
    let result = parse_ok("functio f(sparsa<fractus<f32>, [2, 3]> value) → vacuum {}");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Func(func) = &program.statements[0].kind else {
        panic!("expected function");
    };
    let ty = &func.params[0].ty;
    let TypeExprKind::Sparsa(elem, figura) = &ty.kind else {
        panic!("expected sparsa type");
    };
    let TypeExprKind::Named(elem_name, _) = &elem.kind else {
        panic!("expected element type");
    };
    assert_eq!(result.interner.resolve(elem_name.name), "fractus");
    let FiguraExpr::Tuple(dims) = figura else {
        panic!("expected tuple figura");
    };
    assert!(matches!(dims[0], FiguraExpr::Literal(2)));
    assert!(matches!(dims[1], FiguraExpr::Literal(3)));
}

#[test]
fn parses_sparsa_with_infer_figura_hole() {
    let result = parse_ok("functio f(sparsa<f32, _> value) → vacuum {}");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Func(func) = &program.statements[0].kind else {
        panic!("expected function");
    };
    let ty = &func.params[0].ty;
    let TypeExprKind::Sparsa(elem, figura) = &ty.kind else {
        panic!("expected sparsa type");
    };
    assert!(
        matches!(&elem.kind, TypeExprKind::Named(n, _) if result.interner.resolve(n.name) == "f32")
    );
    assert!(matches!(figura, FiguraExpr::Infer));
}

// =============================================================================
// iuncta tuple type
// =============================================================================

#[test]
fn parses_iuncta_tuple_type_with_two_elements() {
    let result = parse_ok("functio f(iuncta<textus, numerus> pair) → vacuum {}");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Func(func) = &program.statements[0].kind else {
        panic!("expected function");
    };
    let TypeExprKind::Named(name, params) = &func.params[0].ty.kind else {
        panic!("expected iuncta type");
    };
    assert_eq!(result.interner.resolve(name.name), "iuncta");
    assert_eq!(params.len(), 2);
    let TypeExprKind::Named(first, _) = &params[0].kind else {
        panic!("expected first element type");
    };
    assert_eq!(result.interner.resolve(first.name), "textus");
    let TypeExprKind::Named(second, _) = &params[1].kind else {
        panic!("expected second element type");
    };
    assert_eq!(result.interner.resolve(second.name), "numerus");
}

#[test]
fn parses_promissum_failable_type_with_exit_arrow_channel() {
    let result = parse_ok("functio f(promissum<numerus ⇥ textus> future) → vacuum {}");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Func(func) = &program.statements[0].kind else {
        panic!("expected function");
    };
    let TypeExprKind::PromissumFailable(success, alternate) = &func.params[0].ty.kind else {
        panic!("expected failable promissum type");
    };
    let TypeExprKind::Named(success, _) = &success.kind else {
        panic!("expected success type");
    };
    assert_eq!(result.interner.resolve(success.name), "numerus");
    let TypeExprKind::Named(alternate, _) = &alternate.kind else {
        panic!("expected alternate type");
    };
    assert_eq!(result.interner.resolve(alternate.name), "textus");
}

#[test]
fn rejects_promissum_comma_alternate_type() {
    parse_errors_include_issue(
        "functio f(promissum<numerus, textus> future) → vacuum {}",
        ParseErrorKind::InvalidType,
        "promissum_alternate_uses_exit_arrow",
    );
}

#[test]
fn parses_iuncta_tuple_type_with_sixteen_elements() {
    // 16 elements — maximum allowed
    let source = "functio f(iuncta<u8, u8, u8, u8, u8, u8, u8, u8, u8, u8, u8, u8, u8, u8, u8, u8> many) → vacuum {}";
    let result = parse_ok(source);
    let program = result.program.as_ref().expect("program");
    let StmtKind::Func(func) = &program.statements[0].kind else {
        panic!("expected function");
    };
    let TypeExprKind::Named(name, params) = &func.params[0].ty.kind else {
        panic!("expected iuncta type");
    };
    assert_eq!(result.interner.resolve(name.name), "iuncta");
    assert_eq!(params.len(), 16);
}

#[test]
fn rejects_iuncta_with_seventeen_elements() {
    parse_errors_include_issue(
        "incipit { fixum iuncta<u8, u8, u8, u8, u8, u8, u8, u8, u8, u8, u8, u8, u8, u8, u8, u8, u8> big ← vacua }",
        ParseErrorKind::InvalidType,
        "iuncta_arity_exceeded",
    );
}

#[test]
fn parses_iuncta_with_single_element() {
    let result = parse_ok("functio f(iuncta<textus> single) → vacuum {}");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Func(func) = &program.statements[0].kind else {
        panic!("expected function");
    };
    let TypeExprKind::Named(name, params) = &func.params[0].ty.kind else {
        panic!("expected iuncta type");
    };
    assert_eq!(result.interner.resolve(name.name), "iuncta");
    assert_eq!(params.len(), 1);
}

#[test]
fn parses_iuncta_with_nested_types() {
    let result = parse_ok(
        "functio f(iuncta<lista<textus>, iuncta<numerus, fractus<f32>>> nested) → vacuum {}",
    );
    let program = result.program.as_ref().expect("program");
    let StmtKind::Func(func) = &program.statements[0].kind else {
        panic!("expected function");
    };
    let TypeExprKind::Named(name, params) = &func.params[0].ty.kind else {
        panic!("expected iuncta type");
    };
    assert_eq!(result.interner.resolve(name.name), "iuncta");
    assert_eq!(params.len(), 2);
    // First param: lista<textus>
    let TypeExprKind::Named(lista, lista_params) = &params[0].kind else {
        panic!("expected lista type");
    };
    assert_eq!(result.interner.resolve(lista.name), "lista");
    assert_eq!(lista_params.len(), 1);
    // Second param: iuncta<numerus, fractus<f32>>
    let TypeExprKind::Named(inner_iuncta, inner_params) = &params[1].kind else {
        panic!("expected inner iuncta type");
    };
    assert_eq!(result.interner.resolve(inner_iuncta.name), "iuncta");
    assert_eq!(inner_params.len(), 2);
}
