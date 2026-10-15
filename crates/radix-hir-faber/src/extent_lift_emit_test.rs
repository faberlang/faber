//! Runtime-origin tensor extents are emitted as spelled size lifts.
//!
//! A bare value name in an extent position is refused by the checker (SEM008
//! `annotation_extent_bare_value_name`), so the emitter must write
//! `size n` / `size (n - 1)` (la `magnitudo`) for every extent that reads a
//! runtime value, and keep literal and size-parameter extents bare.

use crate::{FaberCodegen, KeywordSurface, TypeBindingKind};
use radix_hir::DefId;
use radix_lexer::{Interner, Symbol, TokenKind};
use radix_syntax::{FiguraExpr, LiftExtent, StmtKind, TypeExprKind};
use radix_types::{IndexExpr, IndexId, RuntimeExtentId, TypeTable};

/// English reader surface: `size`.
struct EnSurface;

impl KeywordSurface for EnSurface {
    fn keyword(&self, kind: TokenKind) -> &str {
        match kind {
            TokenKind::Magnitudo => "size",
            other => panic!("unexpected keyword: {other:?}"),
        }
    }

    fn type_surface(&self, _kind: TypeBindingKind) -> &'static str {
        "tensor"
    }
}

/// Latin reader surface: `magnitudo`.
struct LaSurface;

impl KeywordSurface for LaSurface {
    fn keyword(&self, kind: TokenKind) -> &str {
        match kind {
            TokenKind::Magnitudo => "magnitudo",
            other => panic!("unexpected keyword: {other:?}"),
        }
    }

    fn type_surface(&self, _kind: TypeBindingKind) -> &'static str {
        "tensor"
    }
}

struct Fixture {
    types: TypeTable,
    interner: Interner,
    n: Symbol,
}

impl Fixture {
    fn new() -> Self {
        let mut interner = Interner::new();
        let n = interner.intern("n");
        Self {
            types: TypeTable::new(),
            interner,
            n,
        }
    }

    fn runtime_n(&mut self) -> IndexId {
        self.types
            .intern_index(IndexExpr::Runtime(RuntimeExtentId::ScalarBinding(DefId(7))))
    }

    fn emit(&self, index: IndexId, surface: &impl KeywordSurface) -> String {
        let codegen = FaberCodegen::new(&self.types, &self.interner);
        codegen
            .runtime_value_names
            .borrow_mut()
            .insert(DefId(7), self.n);
        codegen.format_figura(index, surface)
    }
}

#[test]
fn bare_runtime_extent_is_lifted() {
    let mut fixture = Fixture::new();
    let n = fixture.runtime_n();
    assert_eq!(fixture.emit(n, &EnSurface), "size n");
    assert_eq!(fixture.emit(n, &LaSurface), "magnitudo n");
}

#[test]
fn affine_runtime_extent_lifts_the_whole_term() {
    let mut fixture = Fixture::new();
    let n = fixture.runtime_n();
    let minus = fixture.types.intern_index(IndexExpr::Affine {
        base: n,
        offset: -1,
    });
    let plus = fixture
        .types
        .intern_index(IndexExpr::Affine { base: n, offset: 2 });
    let halved = fixture.types.intern_index(IndexExpr::Divide {
        numerator: n,
        divisor: 2,
    });
    assert_eq!(fixture.emit(minus, &EnSurface), "size (n - 1)");
    assert_eq!(fixture.emit(plus, &LaSurface), "magnitudo (n + 2)");
    assert_eq!(fixture.emit(halved, &EnSurface), "size ((n) / 2)");
}

#[test]
fn literal_and_size_parameter_extents_stay_bare() {
    let mut fixture = Fixture::new();
    let big_n = fixture.interner.intern("N");
    let literal = fixture.types.intern_index(IndexExpr::Literal(4));
    let param = fixture.types.intern_index(IndexExpr::Param(big_n));
    let affine = fixture.types.intern_index(IndexExpr::Affine {
        base: param,
        offset: -1,
    });
    assert_eq!(fixture.emit(literal, &EnSurface), "4");
    assert_eq!(fixture.emit(param, &EnSurface), "N");
    assert_eq!(fixture.emit(affine, &EnSurface), "N - 1");
}

#[test]
fn tuple_lifts_only_the_runtime_dimensions() {
    let mut fixture = Fixture::new();
    let n = fixture.runtime_n();
    let two = fixture.types.intern_index(IndexExpr::Literal(2));
    let shifted = fixture.types.intern_index(IndexExpr::Affine {
        base: n,
        offset: -1,
    });
    let tuple = fixture
        .types
        .intern_index(IndexExpr::Tuple(vec![n, two, shifted]));
    assert_eq!(fixture.emit(tuple, &EnSurface), "[size n, 2, size (n - 1)]");
}

#[test]
fn lifted_extents_re_parse_as_size_lifts() {
    let mut fixture = Fixture::new();
    let n = fixture.runtime_n();
    let shifted = fixture.types.intern_index(IndexExpr::Affine {
        base: n,
        offset: -1,
    });
    let two = fixture.types.intern_index(IndexExpr::Literal(2));
    let tuple = fixture
        .types
        .intern_index(IndexExpr::Tuple(vec![n, two, shifted]));
    let figura = fixture.emit(tuple, &LaSurface);
    let source = format!("{{ sit tensor<f32, {figura}> t ← 0 }}");
    let parsed = radix_parser::parse(radix_lexer::lex(&source));
    assert!(
        parsed.success(),
        "must re-parse: {source}\n{:?}",
        parsed.errors
    );
    let program = parsed.program.expect("program");
    let StmtKind::Block(body) = &program.statements[0].kind else {
        panic!("expected a block, got {:?}", program.statements);
    };
    let StmtKind::Var(local) = &body.statements[0].kind else {
        panic!("expected a local, got {:?}", body.statements[0].kind);
    };
    let TypeExprKind::Tensor(_, FiguraExpr::Tuple(dims)) =
        &local.ty.as_ref().expect("annotated").kind
    else {
        panic!("expected a tensor type with tuple figura");
    };
    assert!(matches!(&dims[0], FiguraExpr::Lift(l) if matches!(**l, LiftExtent::Name(_))));
    assert!(matches!(&dims[1], FiguraExpr::Literal(2)));
    assert!(matches!(&dims[2], FiguraExpr::Lift(l) if matches!(**l, LiftExtent::Paren(_))));
}
