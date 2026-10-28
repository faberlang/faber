//! Tests for type-expression parsing: sugar forms, parameterized types, error diagnostics.
//!
//! Contract: each type form produces the expected `TypeExprKind` variant or a structured diagnostic.

use crate::ParseErrorKind;
use crate::test_support::args_have_no_legacy_message;
use crate::test_support::{parse_ok_la, parse_program_la};
use radix_syntax::{ClassMemberKind, ExprKind, FiguraExpr, GenericArgExpr, StmtKind, TypeExprKind};

fn parse_errors_include_issue(source: &str, kind: ParseErrorKind, issue: &str) {
    let result = parse_program_la(source);
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
    let result = parse_program_la(source);
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

fn generic_arg_type(arg: &radix_syntax::GenericArgExpr) -> &radix_syntax::TypeExpr {
    match arg {
        radix_syntax::GenericArgExpr::Type(ty) => ty,
        other => panic!("expected type generic argument, got {other:?}"),
    }
}

#[test]
fn rejects_postfix_array_param_type() {
    let result = parse_program_la("functio sum(numerus[] items) → numerus { redde 0 }");
    assert!(
        !result.success(),
        "postfix array param type must be rejected, got {:?}",
        result.errors
    );
    assert!(result.errors.iter().any(|err| {
        err.kind == ParseErrorKind::Expected
            && err
                .args
                .iter()
                .any(|arg| arg.name == "issue" && arg.value == "expected_identifier")
    }));
}

#[test]
fn invalid_type_emits_vector_sugar_angle_bracket_form() {
    let source = "functio f(vf32<f32> value) → vacuum {}";
    let result = parse_program_la(source);
    let err = result
        .errors
        .iter()
        .find(|err| err.kind == ParseErrorKind::TypeInvalid)
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
    let result = parse_program_la(source);
    let err = result
        .errors
        .iter()
        .find(|err| err.kind == ParseErrorKind::TypeInvalid)
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
fn parses_own_and_copy_type_position_markers() {
    let result =
        parse_ok_la("sit lista<numerus> owned ← own vacua\nsit lista<numerus> copied ← copy vacua");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Var(owned) = &program.statements[0].kind else {
        panic!("expected owned binding");
    };
    let StmtKind::Var(copied) = &program.statements[1].kind else {
        panic!("expected copied binding");
    };
    assert!(matches!(
        owned.ty.as_ref().expect("type").mode,
        Some(radix_syntax::TypeMode::Own)
    ));
    assert!(matches!(
        copied.ty.as_ref().expect("type").mode,
        Some(radix_syntax::TypeMode::Copy)
    ));
}

#[test]
fn parses_own_copy_markers_on_function_params() {
    let result =
        parse_ok_la("functio f(own lista<numerus> xs, copy lista<numerus> ys) → vacuum {}");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Func(func) = &program.statements[0].kind else {
        panic!("expected function");
    };
    assert!(matches!(
        func.params[0].ty.mode,
        Some(radix_syntax::TypeMode::Own)
    ));
    assert!(matches!(
        func.params[1].ty.mode,
        Some(radix_syntax::TypeMode::Copy)
    ));
}

#[test]
fn own_and_copy_remain_binding_names() {
    let _ = parse_ok_la("incipit { sit numerus own ← 1\n sit numerus copy ← 2 }");
}

#[test]
fn parses_type_position_markers_on_fn_type_params() {
    let result = parse_ok_la(
        "typus Handler = (de textus, varia numerus, own lista<numerus>, copy textus) → vacuum",
    );
    let program = result.program.as_ref().expect("program");
    let StmtKind::TypeAlias(alias) = &program.statements[0].kind else {
        panic!("expected type alias");
    };
    let TypeExprKind::Func(func) = &alias.ty.kind else {
        panic!("expected fn type");
    };
    assert!(matches!(
        func.params[0].mode,
        Some(radix_syntax::TypeMode::De)
    ));
    assert!(matches!(
        func.params[1].mode,
        Some(radix_syntax::TypeMode::In)
    ));
    assert!(matches!(
        func.params[2].mode,
        Some(radix_syntax::TypeMode::Own)
    ));
    assert!(matches!(
        func.params[3].mode,
        Some(radix_syntax::TypeMode::Copy)
    ));
}

#[test]
fn parses_type_position_markers_on_closure_params() {
    let result = parse_ok_la("sit _ f ← (de textus x, own lista<numerus> xs) ∴ xs");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Var(binding) = &program.statements[0].kind else {
        panic!("expected fixum");
    };
    let ExprKind::Clausura(clausura) = &binding.init.as_ref().expect("init").kind else {
        panic!("expected clausura");
    };
    assert!(matches!(
        clausura.params[0].ty.mode,
        Some(radix_syntax::TypeMode::De)
    ));
    assert!(matches!(
        clausura.params[1].ty.mode,
        Some(radix_syntax::TypeMode::Own)
    ));
}

#[test]
fn union_type_preserves_member_modes_when_flattening() {
    let result = parse_ok_la("typus MaybeBorrow = de textus ∪ nihil");
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
fn function_type_arrow_is_greedy_over_union_tail() {
    // The return type consumes `∪ nihil`, so the union sits inside the arrow
    // and the function type itself is not a union member.
    let result = parse_ok_la("typus Probe = (numerus) → numerus ∪ nihil");
    let program = result.program.as_ref().expect("program");
    let StmtKind::TypeAlias(alias) = &program.statements[0].kind else {
        panic!("expected type alias");
    };
    let TypeExprKind::Func(func) = &alias.ty.kind else {
        panic!(
            "expected function type at the top level, got {:?}",
            alias.ty.kind
        );
    };
    let TypeExprKind::Union(members) = &func.ret.kind else {
        panic!("expected union return type, got {:?}", func.ret.kind);
    };
    assert_eq!(members.len(), 2);
    assert!(
        matches!(&members[0].kind, TypeExprKind::Named(n, _) if result.interner.resolve(n.name) == "numerus")
    );
    assert!(
        matches!(&members[1].kind, TypeExprKind::Named(n, _) if result.interner.resolve(n.name) == "nihil")
    );
}

#[test]
fn parses_tensor_with_literal_and_infer_figura() {
    let result = parse_ok_la("functio f(tensor<f32, [2, 3]> value) → vacuum {}");
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
    assert_eq!(result.interner.resolve(elem_name.name), "f32");
    let FiguraExpr::Tuple(dims) = figura else {
        panic!("expected tuple figura");
    };
    assert!(matches!(dims[0], FiguraExpr::Literal(2)));
    assert!(matches!(dims[1], FiguraExpr::Literal(3)));
}

#[test]
fn parses_affine_tensor_dimensions_with_division_precedence() {
    let result = parse_ok_la("functio f(tensor<f32, [N - 1, D / 2 + 3]> value) → vacuum {}");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Func(func) = &program.statements[0].kind else {
        panic!("expected function");
    };
    let TypeExprKind::Tensor(_, FiguraExpr::Tuple(dims)) = &func.params[0].ty.kind else {
        panic!("expected tensor shape tuple");
    };
    assert!(matches!(&dims[0], FiguraExpr::Subtract(_, 1)));
    let FiguraExpr::Add(divided, 3) = &dims[1] else {
        panic!("expected addition after division");
    };
    assert!(matches!(divided.as_ref(), FiguraExpr::Divide(_, 2)));
}

#[test]
fn parses_atomic_with_single_element_type() {
    let result = parse_ok_la("functio f(atomic<i32> cell) → vacuum {}");
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
    let result = parse_ok_la("functio f(lista<_> value) → vacuum {}");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Func(func) = &program.statements[0].kind else {
        panic!("expected function");
    };
    let ty = &func.params[0].ty;
    let TypeExprKind::Named(name, params) = &ty.kind else {
        panic!("expected lista type");
    };
    assert_eq!(result.interner.resolve(name.name), "lista");
    assert!(matches!(
        generic_arg_type(&params[0]).kind,
        TypeExprKind::Infer
    ));
}

#[test]
fn types_test_parses_queue_and_stack_bounded_type_forms() {
    let cases: &[(&str, &str, &[&str])] = &[
        ("typus X = queue<numerus>", "queue", &["numerus"]),
        ("typus X = queue<numerus, 8>", "queue", &["numerus", "8"]),
        ("typus X = stack<textus>", "stack", &["textus"]),
        ("typus X = stack<textus, 8>", "stack", &["textus", "8"]),
    ];
    for (source, type_name, expected_params) in cases {
        let result = parse_ok_la(source);
        let program = result.program.as_ref().expect("program");
        let StmtKind::TypeAlias(alias) = &program.statements[0].kind else {
            panic!("expected type alias in {source}");
        };
        let TypeExprKind::Named(name, params) = &alias.ty.kind else {
            panic!("expected named type in {source}");
        };
        assert_eq!(result.interner.resolve(name.name), *type_name);
        assert_eq!(params.len(), expected_params.len());
        for (param, expected) in params.iter().zip(expected_params.iter().copied()) {
            match (param, expected) {
                (GenericArgExpr::Type(ty), expected) => {
                    let TypeExprKind::Named(name, _) = &ty.kind else {
                        panic!("expected named generic type argument, got {:?}", ty.kind);
                    };
                    assert_eq!(result.interner.resolve(name.name), expected);
                }
                (GenericArgExpr::Name(name), expected) => {
                    assert_eq!(result.interner.resolve(name.name), expected);
                }
                (GenericArgExpr::Size(FiguraExpr::Literal(value)), "8") => {
                    assert_eq!(*value, 8);
                }
                (other, expected) => {
                    panic!("unexpected queue/stack argument {other:?}, expected {expected}")
                }
            }
        }
    }
}

#[test]
fn types_test_parses_proposed_bounded_type_forms() {
    let cases: &[(&str, &str, &[&str])] = &[
        ("typus X = lista<numerus, 8>", "lista", &["numerus", "8"]),
        ("typus X = textus<8>", "textus", &["8"]),
        ("typus X = ascii<8>", "ascii", &["8"]),
    ];
    for (source, type_name, expected_params) in cases {
        let result = parse_ok_la(source);
        let program = result.program.as_ref().expect("program");
        let StmtKind::TypeAlias(alias) = &program.statements[0].kind else {
            panic!("expected type alias in {source}");
        };
        let TypeExprKind::Named(name, params) = &alias.ty.kind else {
            panic!("expected named type in {source}");
        };
        assert_eq!(result.interner.resolve(name.name), *type_name);
        assert_eq!(params.len(), expected_params.len(), "{source}");
        for (param, expected) in params.iter().zip(expected_params.iter()) {
            let TypeExprKind::Named(param_name, nested) = &generic_arg_type(param).kind else {
                panic!("expected named type argument in {source}");
            };
            assert!(nested.is_empty(), "{source}");
            assert_eq!(result.interner.resolve(param_name.name), *expected);
        }
    }
}

#[test]
fn parses_qualified_type_path() {
    let result = parse_ok_la("functio f(terminus.Terminus value) → vacuum {}");
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
    let result = parse_ok_la("functio f(tensor<f32, _> value) → vacuum {}");
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
        "functio f(tensor<f32> value) → vacuum {}",
        ParseErrorKind::Expected,
        "tensor_figura_after_element",
    );
}

#[test]
fn tensor_without_figura_preserves_precise_message() {
    let source = "functio f(tensor<f32> value) → vacuum {}";
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
fn generic_applied_arg_preserves_literal_as_source_value() {
    let result = parse_ok_la("typus Vector = Box<3>");
    let program = result.program.as_ref().expect("program");
    let StmtKind::TypeAlias(alias) = &program.statements[0].kind else {
        panic!("expected type alias");
    };
    let TypeExprKind::Named(_, params) = &alias.ty.kind else {
        panic!("expected named application");
    };
    assert!(matches!(
        params[0],
        GenericArgExpr::Size(FiguraExpr::Literal(3)),
    ));
}

#[test]
fn parses_tensor_sugar_with_infer_figura() {
    let result = parse_ok_la("functio f(tf32 value) → vacuum {}");
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
    let result = parse_ok_la("functio f(ti64[2, 3] value) → vacuum {}");
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
    let result = parse_ok_la("functio f(vector<f32, magnitudo 4> a) → vacuum {}");
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
    assert_eq!(result.interner.resolve(elem_name.name), "f32");
    assert!(elem_params.is_empty());
    // The long form's lane count follows the `magnitudo` marker.
    let FiguraExpr::Lift(extent) = width else {
        panic!("expected a spelled size lift");
    };
    assert!(matches!(**extent, radix_syntax::LiftExtent::Literal(4)));
}

#[test]
fn parses_vector_sugar_bracket_form() {
    let result = parse_ok_la("functio f(vf32[4] b) → vacuum {}");
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
    let result = parse_ok_la("functio f(matrix<f32, [4, 4]> a) → vacuum {}");
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
    assert_eq!(result.interner.resolve(elem_name.name), "f32");
    assert!(elem_params.is_empty());
    let FiguraExpr::Tuple(dims) = shape else {
        panic!("expected matrix shape");
    };
    assert!(matches!(dims[0], FiguraExpr::Literal(4)));
    assert!(matches!(dims[1], FiguraExpr::Literal(4)));
}

#[test]
fn parses_matrix_sugar_bracket_form() {
    let result = parse_ok_la("functio f(mf32[3, 2] b) → vacuum {}");
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
        ParseErrorKind::TypeInvalid,
        "matrix_sugar_missing_shape",
    );
}

#[test]
fn parses_tensor_sugar_rank_zero_brackets() {
    let result = parse_ok_la("functio f(tf32[] value) → vacuum {}");
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
    let result = parse_ok_la("functio f(sf32 value) → vacuum {}");
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
    let result = parse_ok_la("functio f(si64[N] value) → vacuum {}");
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
    let result = parse_ok_la("functio f(sf64[] value) → vacuum {}");
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
    let result = parse_ok_la("functio f(su32[4] value) → vacuum {}");
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
        ParseErrorKind::TypeInvalid,
        "type_sugar_angle_bracket_form",
    );
}

#[test]
fn parses_standalone_width_marker_type() {
    let result = parse_ok_la("functio f(f32 value) → vacuum {}");
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
        ParseErrorKind::TypeInvalid,
        "type_sugar_angle_bracket_form",
    );
}

#[test]
fn parses_lista_sugar_unsigned_width() {
    let result = parse_ok_la("functio f(lu32 rows) → vacuum {}");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Func(func) = &program.statements[0].kind else {
        panic!("expected function");
    };
    let TypeExprKind::Named(lista, params) = &func.params[0].ty.kind else {
        panic!("expected lista sugar desugar");
    };
    assert_eq!(result.interner.resolve(lista.name), "lista");
    assert_eq!(params.len(), 1);
    let TypeExprKind::Named(elem, _) = &generic_arg_type(&params[0]).kind else {
        panic!("expected width element");
    };
    assert_eq!(result.interner.resolve(elem.name), "u32");
}

#[test]
fn parses_lista_sugar_float_width() {
    let result = parse_ok_la("functio f(lf32 flat) → vacuum {}");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Func(func) = &program.statements[0].kind else {
        panic!("expected function");
    };
    let TypeExprKind::Named(lista, params) = &func.params[0].ty.kind else {
        panic!("expected lista sugar desugar");
    };
    assert_eq!(result.interner.resolve(lista.name), "lista");
    let TypeExprKind::Named(elem, _) = &generic_arg_type(&params[0]).kind else {
        panic!("expected width element");
    };
    assert_eq!(result.interner.resolve(elem.name), "f32");
}

#[test]
fn rejects_lista_sugar_angle_bracket_form() {
    parse_errors_include_issue(
        "functio f(lu32<u32> rows) → vacuum {}",
        ParseErrorKind::TypeInvalid,
        "lista_sugar_angle_bracket_form",
    );
}

#[test]
fn rejects_lista_sugar_bracket_suffix() {
    parse_errors_include_issue(
        "functio f(lu32[] rows) → vacuum {}",
        ParseErrorKind::TypeInvalid,
        "lista_sugar_bracket_form",
    );
}

#[test]
fn parses_sparsa_with_literal_figura() {
    let result = parse_ok_la("functio f(sparsa<f32, [2, 3]> value) → vacuum {}");
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
    assert_eq!(result.interner.resolve(elem_name.name), "f32");
    let FiguraExpr::Tuple(dims) = figura else {
        panic!("expected tuple figura");
    };
    assert!(matches!(dims[0], FiguraExpr::Literal(2)));
    assert!(matches!(dims[1], FiguraExpr::Literal(3)));
}

#[test]
fn parses_sparsa_with_infer_figura_hole() {
    let result = parse_ok_la("functio f(sparsa<f32, _> value) → vacuum {}");
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
    let result = parse_ok_la("functio f(iuncta<textus, numerus> pair) → vacuum {}");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Func(func) = &program.statements[0].kind else {
        panic!("expected function");
    };
    let TypeExprKind::Named(name, params) = &func.params[0].ty.kind else {
        panic!("expected iuncta type");
    };
    assert_eq!(result.interner.resolve(name.name), "iuncta");
    assert_eq!(params.len(), 2);
    let TypeExprKind::Named(first, _) = &generic_arg_type(&params[0]).kind else {
        panic!("expected first element type");
    };
    assert_eq!(result.interner.resolve(first.name), "textus");
    let TypeExprKind::Named(second, _) = &generic_arg_type(&params[1]).kind else {
        panic!("expected second element type");
    };
    assert_eq!(result.interner.resolve(second.name), "numerus");
}

#[test]
fn parses_labeled_and_unlabeled_iuncta_type_arguments() {
    let result =
        parse_ok_la("functio f(iuncta<gx: textus, numerus, fixum: fractus> pair) → vacuum {}");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Func(func) = &program.statements[0].kind else {
        panic!("expected function");
    };
    let TypeExprKind::Named(name, params) = &func.params[0].ty.kind else {
        panic!("expected iuncta type");
    };
    assert_eq!(result.interner.resolve(name.name), "iuncta");
    assert_eq!(params.len(), 3);
    assert!(matches!(
        generic_arg_type(&params[0]).kind,
        TypeExprKind::Named(_, _)
    ));
    assert!(matches!(
        generic_arg_type(&params[1]).kind,
        TypeExprKind::Named(_, _)
    ));
    assert!(matches!(
        generic_arg_type(&params[2]).kind,
        TypeExprKind::Named(_, _)
    ));
    let gx = generic_arg_type(&params[0])
        .iuncta_label
        .as_ref()
        .expect("gx label");
    assert_eq!(result.interner.resolve(gx.name), "gx");
    assert!(generic_arg_type(&params[1]).iuncta_label.is_none());
    let fixum = generic_arg_type(&params[2])
        .iuncta_label
        .as_ref()
        .expect("fixum label");
    assert_eq!(result.interner.resolve(fixum.name), "fixum");
}

#[test]
fn parses_labeled_iuncta_infer_argument() {
    let result = parse_ok_la("functio f(iuncta<loss: _, T> pair) → vacuum {}");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Func(func) = &program.statements[0].kind else {
        panic!("expected function");
    };
    let TypeExprKind::Named(_, params) = &func.params[0].ty.kind else {
        panic!("expected iuncta type");
    };
    assert!(matches!(
        generic_arg_type(&params[0]).kind,
        TypeExprKind::Infer
    ));
    assert!(matches!(
        generic_arg_type(&params[1]).kind,
        TypeExprKind::Named(_, _)
    ));
}

#[test]
fn rejects_duplicate_iuncta_labels_at_the_duplicate_label() {
    let source = "functio f(iuncta<gx: f32, gx: numerus> pair) → vacuum {}";
    let result = parse_program_la(source);
    let error = result
        .errors
        .iter()
        .find(|error| {
            error.kind == ParseErrorKind::Expected
                && error
                    .args
                    .iter()
                    .any(|arg| arg.name == "issue" && arg.value == "duplicate_tuple_element_label")
        })
        .expect("duplicate label diagnostic");
    let duplicate_label_start =
        u32::try_from(source.rfind("gx").expect("duplicate label")).expect("offset fits u32");
    assert_eq!(error.span.start, duplicate_label_start);
    assert_eq!(error.span.end, duplicate_label_start + 2);
}

#[test]
fn rejects_iuncta_label_spelling_on_call_site_type_arguments() {
    let source = "incipit { sit _ x ← f<gx: numerus>(x) }";
    let result = parse_program_la(source);
    let colon = u32::try_from(source.find(':').expect("label colon")).expect("offset fits u32");
    // The failed type-argument attempt falls back to the (retired) ASCII
    // comparison, which reports first; the label error is the one at the colon.
    let error = result
        .errors
        .iter()
        .find(|error| error.span.start == colon)
        .expect("call-site label must produce a parse error");
    assert_eq!(error.kind, ParseErrorKind::ExpressionInvalid);
}

#[test]
fn unlabeled_iuncta_elements_are_not_spelled_with_an_underscore_label() {
    let result = parse_program_la("functio f(iuncta<_: f32> pair) → vacuum {}");
    assert!(!result.success());
    assert!(
        result
            .errors
            .iter()
            .any(|error| error.kind == ParseErrorKind::Expected)
    );
}

#[test]
fn labeled_iuncta_arity_cap_counts_labeled_elements() {
    let args = (0..16)
        .map(|index| format!("e{index}: u8"))
        .collect::<Vec<_>>()
        .join(", ");
    let accepted = parse_ok_la(&format!("functio f(iuncta<{args}> many) → vacuum {{}}"));
    let program = accepted.program.as_ref().expect("program");
    let StmtKind::Func(func) = &program.statements[0].kind else {
        panic!("expected function");
    };
    let TypeExprKind::Named(_, params) = &func.params[0].ty.kind else {
        panic!("expected iuncta type");
    };
    assert_eq!(params.len(), 16);

    let args = (0..17)
        .map(|index| format!("e{index}: u8"))
        .collect::<Vec<_>>()
        .join(", ");
    let rejected = parse_program_la(&format!("functio f(iuncta<{args}> many) → vacuum {{}}"));
    assert!(rejected.errors.iter().any(|error| {
        error.kind == ParseErrorKind::TypeInvalid
            && error
                .args
                .iter()
                .any(|arg| arg.name == "issue" && arg.value == "iuncta_arity_exceeded")
    }));
}

#[test]
fn parses_promissum_failable_type_with_exit_arrow_channel() {
    let result = parse_ok_la("functio f(promissum<numerus ⇥ textus> future) → vacuum {}");
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
        ParseErrorKind::TypeInvalid,
        "promissum_alternate_uses_exit_arrow",
    );
}

#[test]
fn parses_iuncta_tuple_type_with_sixteen_elements() {
    // 16 elements — maximum allowed
    let source = "functio f(iuncta<u8, u8, u8, u8, u8, u8, u8, u8, u8, u8, u8, u8, u8, u8, u8, u8> many) → vacuum {}";
    let result = parse_ok_la(source);
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
        "incipit { sit iuncta<u8, u8, u8, u8, u8, u8, u8, u8, u8, u8, u8, u8, u8, u8, u8, u8, u8> big ← vacua }",
        ParseErrorKind::TypeInvalid,
        "iuncta_arity_exceeded",
    );
}

#[test]
fn parses_iuncta_with_single_element() {
    let result = parse_ok_la("functio f(iuncta<textus> single) → vacuum {}");
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
    let result =
        parse_ok_la("functio f(iuncta<lista<textus>, iuncta<numerus, f32>> nested) → vacuum {}");
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
    let TypeExprKind::Named(lista, lista_params) = &generic_arg_type(&params[0]).kind else {
        panic!("expected lista type");
    };
    assert_eq!(result.interner.resolve(lista.name), "lista");
    assert_eq!(lista_params.len(), 1);
    // Second param: iuncta<numerus, f32>
    let TypeExprKind::Named(inner_iuncta, inner_params) = &generic_arg_type(&params[1]).kind else {
        panic!("expected inner iuncta type");
    };
    assert_eq!(result.interner.resolve(inner_iuncta.name), "iuncta");
    assert_eq!(inner_params.len(), 2);
}

#[test]
fn parses_ratio_type_with_mandatory_labels_and_holes() {
    let result = parse_ok_la("functio f(ratio<g: f32, w: _, fixum: fractus> value) → vacuum {}");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Func(func) = &program.statements[0].kind else {
        panic!("expected function");
    };
    let TypeExprKind::Named(name, params) = &func.params[0].ty.kind else {
        panic!("expected ratio type");
    };
    assert_eq!(result.interner.resolve(name.name), "ratio");
    assert_eq!(params.len(), 3);
    assert!(matches!(
        generic_arg_type(&params[0]).kind,
        TypeExprKind::Named(_, _)
    ));
    assert!(matches!(
        generic_arg_type(&params[1]).kind,
        TypeExprKind::Infer
    ));
    assert!(matches!(
        generic_arg_type(&params[2]).kind,
        TypeExprKind::Named(_, _)
    ));
    let labels = params
        .iter()
        .map(|param| {
            generic_arg_type(param)
                .iuncta_label
                .as_ref()
                .map(|label| result.interner.resolve(label.name))
        })
        .collect::<Vec<_>>();
    assert_eq!(labels, vec![Some("g"), Some("w"), Some("fixum")]);
}

#[test]
fn parses_ratio_as_an_ordinary_binding_name() {
    let _ = parse_ok_la("sit numerus ratio ← 3\nfunctio f(f32 ratio) → vacuum {}");
}

#[test]
fn rejects_unlabeled_ratio_type_argument_with_targeted_issue() {
    expect_expected_issue(
        "functio f(ratio<f32, w: numerus> value) → vacuum {}",
        "ratio_type_arg_label_required",
    );
}

#[test]
fn record_field_list_hole_parses_as_hole_type() {
    let result = parse_ok_la("functio f(ratio<*> v) → vacuum {}");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Func(func) = &program.statements[0].kind else {
        panic!("expected function");
    };
    assert!(matches!(
        func.params[0].ty.kind,
        TypeExprKind::FieldListHole
    ));
}

#[test]
fn record_field_list_hole_must_be_sole_argument() {
    expect_expected_issue(
        "functio f(ratio<g: f32, *> value) → vacuum {}",
        "ratio_field_list_hole_not_sole",
    );
    expect_expected_issue(
        "functio f(ratio<*, g: f32> value) → vacuum {}",
        "ratio_field_list_hole_not_sole",
    );
}

#[test]
fn record_underscore_argument_still_requires_label() {
    expect_expected_issue(
        "functio f(ratio<_> value) → vacuum {}",
        "ratio_type_arg_label_required",
    );
}

#[test]
fn rejects_duplicate_ratio_labels_at_the_duplicate_label() {
    let source = "functio f(ratio<g: f32, g: numerus> value) → vacuum {}";
    let result = parse_program_la(source);
    let error = result
        .errors
        .iter()
        .find(|error| {
            error.kind == ParseErrorKind::Expected
                && error
                    .args
                    .iter()
                    .any(|arg| arg.name == "issue" && arg.value == "duplicate_tuple_element_label")
        })
        .expect("duplicate label diagnostic");
    let duplicate_label_start =
        u32::try_from(source.rfind('g').expect("duplicate label")).expect("offset fits u32");
    assert_eq!(error.span.start, duplicate_label_start);
    assert_eq!(error.span.end, duplicate_label_start + 1);
}

#[test]
fn rejects_trailing_comma_in_ratio_type_arguments() {
    parse_errors_include_issue(
        "functio f(ratio<g: f32,> value) → vacuum {}",
        ParseErrorKind::Expected,
        "item_list_trailing_comma",
    );
}

// =============================================================================
// union hole type (`∪`)
// =============================================================================

#[test]
fn parses_union_hole_binding_type() {
    let result = parse_ok_la("sit ∪ x ← 1");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Var(decl) = &program.statements[0].kind else {
        panic!("expected variable declaration");
    };
    let ty = decl.ty.as_ref().expect("explicit type slot");
    assert!(matches!(ty.kind, TypeExprKind::UnionHole));
}

#[test]
fn parses_union_hole_param_type() {
    let result = parse_ok_la("functio f(∪ value) → vacuum {}");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Func(func) = &program.statements[0].kind else {
        panic!("expected function");
    };
    assert!(matches!(func.params[0].ty.kind, TypeExprKind::UnionHole));
}

#[test]
fn parses_union_hole_lista_element() {
    let result = parse_ok_la("functio f(lista<∪> values) → vacuum {}");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Func(func) = &program.statements[0].kind else {
        panic!("expected function");
    };
    let TypeExprKind::Named(name, params) = &func.params[0].ty.kind else {
        panic!("expected lista type");
    };
    assert_eq!(result.interner.resolve(name.name), "lista");
    assert_eq!(params.len(), 1);
    assert!(matches!(
        generic_arg_type(&params[0]).kind,
        TypeExprKind::UnionHole
    ));
}

#[test]
fn parses_union_hole_return_type() {
    let result = parse_ok_la("functio f() → ∪ { redde 1 }");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Func(func) = &program.statements[0].kind else {
        panic!("expected function");
    };
    let ret = func.ret.as_ref().expect("return type");
    assert!(matches!(ret.kind, TypeExprKind::UnionHole));
}

#[test]
fn parses_union_hole_function_parameter_slot() {
    // `(∪)` as a parameter slot inside a function-type annotation.
    let result = parse_ok_la("functio f((∪) → numerus cb) → vacuum {}");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Func(func) = &program.statements[0].kind else {
        panic!("expected function");
    };
    let TypeExprKind::Func(sig) = &func.params[0].ty.kind else {
        panic!("expected function type");
    };
    assert_eq!(sig.params.len(), 1);
    assert!(matches!(sig.params[0].kind, TypeExprKind::UnionHole));
}

#[test]
fn parses_union_hole_genus_field() {
    let result = parse_ok_la("genus Panel { varia ∪ body }");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Class(class) = &program.statements[0].kind else {
        panic!("expected genus");
    };
    let ClassMemberKind::Field(field) = &class.members[0].kind else {
        panic!("expected field");
    };
    assert!(matches!(field.ty.kind, TypeExprKind::UnionHole));
    assert_eq!(result.interner.resolve(field.name.name), "body");
}

#[test]
fn parses_union_hole_tabula_value_slot() {
    let result = parse_ok_la("functio f(tabula<textus, ∪> m) → vacuum {}");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Func(func) = &program.statements[0].kind else {
        panic!("expected function");
    };
    let TypeExprKind::Named(name, params) = &func.params[0].ty.kind else {
        panic!("expected tabula type");
    };
    assert_eq!(result.interner.resolve(name.name), "tabula");
    assert_eq!(params.len(), 2);
    assert!(matches!(
        generic_arg_type(&params[1]).kind,
        TypeExprKind::UnionHole
    ));
}

#[test]
fn rejects_cup_after_union_hole() {
    // `A ∪ ∪`: the right-hand member slot is itself a union hole.
    parse_errors_include_issue(
        "functio f(A ∪ ∪ value) → vacuum {}",
        ParseErrorKind::TypeInvalid,
        "unexpected_cup_after_union_hole",
    );
    // `A ∪ ∪ B`: same, with a member following the stray cup.
    parse_errors_include_issue(
        "functio f(A ∪ ∪ B value) → vacuum {}",
        ParseErrorKind::TypeInvalid,
        "unexpected_cup_after_union_hole",
    );
    // `∪ ∪`: a hole directly followed by a cup.
    parse_errors_include_issue(
        "functio f(∪ ∪ value) → vacuum {}",
        ParseErrorKind::TypeInvalid,
        "unexpected_cup_after_union_hole",
    );
}

#[test]
fn rejects_union_hole_as_missing_binary_side() {
    // A leading `∪` is always the hole form; it can never be a binary cup
    // operator with a missing left side (`∪ B`). The hole consumes the whole
    // type expression, so the stray `B` is a structured parse error at the
    // enclosing grammar site.
    let result = parse_program_la("functio f(∪ B value) → vacuum {}");
    assert!(
        result
            .errors
            .iter()
            .any(|err| err.kind == ParseErrorKind::Expected),
        "expected a structured parse error for `∪ B`, got {:?}",
        result
            .errors
            .iter()
            .map(|err| (err.kind, err.args.clone()))
            .collect::<Vec<_>>()
    );
}

#[test]
fn union_hole_keeps_infer_and_binary_union_unchanged() {
    // `_` still parses as the monomorphic hole and may appear as a union member.
    let result = parse_ok_la("functio f(_ ∪ textus value) → vacuum {}");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Func(func) = &program.statements[0].kind else {
        panic!("expected function");
    };
    let TypeExprKind::Union(members) = &func.params[0].ty.kind else {
        panic!("expected union type");
    };
    assert_eq!(members.len(), 2);
    assert!(matches!(members[0].kind, TypeExprKind::Infer));
    assert!(
        matches!(&members[1].kind, TypeExprKind::Named(n, _) if result.interner.resolve(n.name) == "textus")
    );

    // `A ∪ B` explicit unions keep their meaning.
    let result = parse_ok_la("functio f(numerus ∪ textus value) → vacuum {}");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Func(func) = &program.statements[0].kind else {
        panic!("expected function");
    };
    let TypeExprKind::Union(members) = &func.params[0].ty.kind else {
        panic!("expected union type");
    };
    assert_eq!(members.len(), 2);
    assert!(
        matches!(&members[0].kind, TypeExprKind::Named(n, _) if result.interner.resolve(n.name) == "numerus")
    );
    assert!(
        matches!(&members[1].kind, TypeExprKind::Named(n, _) if result.interner.resolve(n.name) == "textus")
    );
}

// ── U0: type-level `∩` (Cap) grammar ─────────────────────────────────────────

#[test]
fn parses_intersection_chain_and_flattens() {
    let result = parse_ok_la("typus All = A ∩ B ∩ C");
    let program = result.program.as_ref().expect("program");
    let StmtKind::TypeAlias(alias) = &program.statements[0].kind else {
        panic!("expected type alias");
    };
    let TypeExprKind::Intersection(members) = &alias.ty.kind else {
        panic!("expected intersection type, got {:?}", alias.ty.kind);
    };
    assert_eq!(members.len(), 3);
    for expected in ["A", "B", "C"] {
        assert!(
            members
                .iter()
                .any(|m| matches!(&m.kind, TypeExprKind::Named(n, _)
                if result.interner.resolve(n.name) == expected)),
            "missing member {expected}"
        );
    }
}

#[test]
fn intersection_binds_tighter_than_union() {
    let result = parse_ok_la("typus Mixed = A ∩ B ∪ C");
    let program = result.program.as_ref().expect("program");
    let StmtKind::TypeAlias(alias) = &program.statements[0].kind else {
        panic!("expected type alias");
    };
    let TypeExprKind::Union(members) = &alias.ty.kind else {
        panic!("expected union at the top level, got {:?}", alias.ty.kind);
    };
    assert_eq!(members.len(), 2);
    assert!(
        matches!(&members[0].kind, TypeExprKind::Intersection(inner) if inner.len() == 2),
        "first union member must be the A ∩ B intersection"
    );
    assert!(
        matches!(&members[1].kind, TypeExprKind::Named(n, _) if result.interner.resolve(n.name) == "C")
    );
}

#[test]
fn implements_clause_accepts_cap_separator() {
    let result = parse_ok_la("genus Doc implet Readable ∩ Seekable {}");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Class(class) = &program.statements[0].kind else {
        panic!("expected class");
    };
    assert_eq!(class.implements.len(), 2);
    assert_eq!(
        result.interner.resolve(class.implements[0].name.name),
        "Readable"
    );
    assert_eq!(
        result.interner.resolve(class.implements[1].name.name),
        "Seekable"
    );
}

#[test]
fn implements_clause_rejects_union_separator() {
    // R1: `∪` is not an implements item separator — disjunctive conformance is
    // not parseable, so it fails closed.
    let result = parse_program_la("genus Doc implet Readable ∪ Seekable {}");
    assert!(!result.errors.is_empty(), "implements-∪ must not parse");
}

#[test]
fn implements_clause_rejects_hole() {
    let result = parse_program_la("genus Doc implet _ {}");
    assert!(!result.errors.is_empty(), "implements hole must not parse");
}

// --- authored type-form levels (ATF-1) ---

use radix_syntax::{TypeExpr, TypeFormLevel};

fn binding_annotation(source: &str) -> TypeExpr {
    let result = parse_ok_la(source);
    let program = result
        .program
        .as_ref()
        .expect("parser should produce a program");
    let StmtKind::Var(decl) = &program.statements[0].kind else {
        panic!("expected var binding, got {:?}", program.statements[0].kind);
    };
    decl.ty.clone().expect("binding should carry an annotation")
}

fn first_type_arg(expr: &TypeExpr) -> TypeExpr {
    let TypeExprKind::Named(_, args) = &expr.kind else {
        panic!("expected named type, got {:?}", expr.kind);
    };
    let GenericArgExpr::Type(child) = &args[0] else {
        panic!("expected type argument, got {:?}", args[0]);
    };
    child.clone()
}

fn first_tensor_element(expr: &TypeExpr) -> TypeExpr {
    let TypeExprKind::Tensor(elem, _) = &expr.kind else {
        panic!("expected tensor type, got {:?}", expr.kind);
    };
    (**elem).clone()
}

#[test]
fn type_form_levels_capture_sugar_roots_and_width_children() {
    // Long root keeps L0; its bare-width element is authored SHORT.
    let long_list = binding_annotation("sit lista<u32> xs ← vacua");
    assert_eq!(long_list.form, TypeFormLevel::LONG);
    assert_eq!(first_type_arg(&long_list).form, TypeFormLevel::SHORT);

    // Fused lista sugar: root SHORT, bare-width element SHORT.
    let fused_list = binding_annotation("sit lu32 xs ← vacua");
    assert_eq!(fused_list.form, TypeFormLevel::SHORT);
    assert_eq!(first_type_arg(&fused_list).form, TypeFormLevel::SHORT);

    // Long tensor keeps L0; bare-width element SHORT.
    let long_tensor = binding_annotation("sit tensor<f32, [2, 2]> t ← vacua");
    assert_eq!(long_tensor.form, TypeFormLevel::LONG);
    assert_eq!(
        first_tensor_element(&long_tensor).form,
        TypeFormLevel::SHORT
    );

    // Fused tensor sugar: root SHORT, element SHORT.
    let fused_tensor = binding_annotation("sit tf32[2, 2] t ← vacua");
    assert_eq!(fused_tensor.form, TypeFormLevel::SHORT);
    assert_eq!(
        first_tensor_element(&fused_tensor).form,
        TypeFormLevel::SHORT
    );

    // Bare width marker is itself SHORT.
    let bare_width = binding_annotation("sit u32 x ← 1");
    assert_eq!(bare_width.form, TypeFormLevel::SHORT);

    // Long collection head over a width marker: root LONG, width argument SHORT.
    let long_scalar = binding_annotation("sit lista<u32> x ← vacua");
    assert_eq!(long_scalar.form, TypeFormLevel::LONG);
    assert_eq!(first_type_arg(&long_scalar).form, TypeFormLevel::SHORT);
}

#[test]
fn type_form_levels_stay_independent_under_nesting() {
    // `lista<lista<u32>>`: both collection roots LONG, inner bare width SHORT.
    let nested = binding_annotation("sit lista<lista<u32>> xs ← vacua");
    assert_eq!(nested.form, TypeFormLevel::LONG);
    let mid = first_type_arg(&nested);
    assert_eq!(mid.form, TypeFormLevel::LONG);
    assert_eq!(first_type_arg(&mid).form, TypeFormLevel::SHORT);

    // Fused sugar does not propagate SHORT to ancestors: `lista<lu32>` keeps
    // the outer root LONG with a SHORT fused-list child.
    let fused_child = binding_annotation("sit lista<lu32> xs ← vacua");
    assert_eq!(fused_child.form, TypeFormLevel::LONG);
    let child = first_type_arg(&fused_child);
    assert_eq!(child.form, TypeFormLevel::SHORT);
    assert_eq!(first_type_arg(&child).form, TypeFormLevel::SHORT);
}
