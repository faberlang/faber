use super::*;
use radix_hir::DefId;
use radix_lexer::Interner;
use radix_types::{IndexExpr, IndexVar, RuntimeExtentId, TypeTable};

/// Latin reader surface: only the size-lift keyword is ever consulted here.
struct LaSurface;

impl crate::KeywordSurface for LaSurface {
    fn keyword(&self, kind: radix_lexer::TokenKind) -> &str {
        match kind {
            radix_lexer::TokenKind::Magnitudo => "magnitudo",
            other => panic!("unexpected keyword: {other:?}"),
        }
    }

    fn type_surface(&self, _kind: crate::TypeBindingKind) -> &str {
        "tensor"
    }
}

fn figura(codegen: &crate::FaberCodegen<'_>, index: IndexId) -> String {
    codegen.format_figura(index, &LaSurface)
}

#[test]
fn format_figura_literal_param_tuple_and_infer() {
    let mut types = TypeTable::new();
    let mut interner = Interner::new();
    let n_sym = interner.intern("N");

    let lit = types.intern_index(IndexExpr::Literal(4));
    assert_eq!(
        figura(&crate::FaberCodegen::new(&types, &interner), lit),
        "4"
    );

    let param = types.intern_index(IndexExpr::Param(n_sym));
    assert_eq!(
        figura(&crate::FaberCodegen::new(&types, &interner), param),
        "N"
    );

    let d2 = types.intern_index(IndexExpr::Literal(2));
    let d3 = types.intern_index(IndexExpr::Literal(3));
    let tuple = types.intern_index(IndexExpr::Tuple(vec![d2, d3]));
    assert_eq!(
        figura(&crate::FaberCodegen::new(&types, &interner), tuple),
        "[2, 3]"
    );

    let empty = types.intern_index(IndexExpr::Tuple(vec![]));
    assert_eq!(
        figura(&crate::FaberCodegen::new(&types, &interner), empty),
        "[]"
    );

    let infer = types.intern_index(IndexExpr::Infer(IndexVar(0)));
    assert_eq!(
        figura(&crate::FaberCodegen::new(&types, &interner), infer),
        "_"
    );

    let unspecified = types.intern_index(IndexExpr::Unspecified);
    assert_eq!(
        figura(&crate::FaberCodegen::new(&types, &interner), unspecified),
        "_"
    );
}

#[test]
fn format_figura_affine_and_exact_division_without_changing_precedence() {
    let mut types = TypeTable::new();
    let mut interner = Interner::new();
    let symbol = interner.intern("D");
    let d = types.intern_index(IndexExpr::Param(symbol));
    let divided = types.intern_index(IndexExpr::Divide {
        numerator: d,
        divisor: 2,
    });
    let shifted = types.intern_index(IndexExpr::Affine {
        base: divided,
        offset: -1,
    });
    assert_eq!(
        figura(&crate::FaberCodegen::new(&types, &interner), shifted),
        "(D) / 2 - 1"
    );
}

#[test]
fn format_figura_runtime_extent_fails_closed_instead_of_guessing_a_literal() {
    let mut types = TypeTable::new();
    let interner = Interner::new();
    let runtime = types.intern_index(IndexExpr::Runtime(RuntimeExtentId::FunctionParameterAxis {
        parameter: 0,
        axis: 0,
    }));
    assert_eq!(
        figura(&crate::FaberCodegen::new(&types, &interner), runtime),
        "<error>"
    );
}

#[test]
fn format_figura_runtime_scalar_uses_emitter_binding_names() {
    let mut types = TypeTable::new();
    let mut interner = Interner::new();
    let local_name = interner.intern("n");
    let parameter_name = interner.intern("width");
    let local = types.intern_index(IndexExpr::Runtime(RuntimeExtentId::ScalarBinding(DefId(7))));
    let parameter = types.intern_index(IndexExpr::Runtime(RuntimeExtentId::ScalarParameter {
        parameter: 0,
    }));
    let codegen = crate::FaberCodegen::new(&types, &interner);
    codegen
        .runtime_value_names
        .borrow_mut()
        .insert(DefId(7), local_name);
    codegen
        .runtime_parameter_names
        .borrow_mut()
        .insert(0, parameter_name);

    assert_eq!(figura(&codegen, local), "magnitudo n");
    assert_eq!(figura(&codegen, parameter), "magnitudo width");
}
