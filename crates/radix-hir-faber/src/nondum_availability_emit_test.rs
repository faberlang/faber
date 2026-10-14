//! Emitter proofs for the bare `@ nondum` availability annotation.
//!
//! A method marked with a bare `@ nondum` (no target, no reason) carries
//! `HirAvailability::Nondum { target: None, reason: None }`. The legal surface
//! spelling is the bare annotation: the braced record form is only authored
//! with at least one field, so re-emitting an empty `{  }` record is not
//! canonical source (and carries a doubled space). The annotation line must
//! project through the reader surface (`nondum` under `la`, `unstable` under
//! `en`).

use crate::surface::{KeywordSurface, TypeBindingKind};
use crate::{FaberCodegen, empty_presentation};
use radix_hir::{
    DefId, HirAvailability, HirId, HirInterface, HirInterfaceMethod, HirItem, HirItemKind,
    HirModule,
};
use radix_lexer::{Interner, Span, TokenKind};
use radix_syntax::CallablePosture;
use radix_types::{TypeTable, USER_DEF_ID_BASE};

struct LatinSurface;

impl KeywordSurface for LatinSurface {
    fn keyword(&self, _kind: TokenKind) -> &'static str {
        "kw"
    }

    fn contextual_keyword(&self, key: &'static str) -> &'static str {
        key
    }

    fn type_surface(&self, _kind: TypeBindingKind) -> &'static str {
        "type"
    }
}

struct EnglishSurface;

impl KeywordSurface for EnglishSurface {
    fn keyword(&self, _kind: TokenKind) -> &'static str {
        "kw"
    }

    fn contextual_keyword(&self, key: &'static str) -> &'static str {
        match key {
            "nondum" => "unstable",
            _ => key,
        }
    }

    fn type_surface(&self, _kind: TypeBindingKind) -> &'static str {
        "type"
    }
}

/// One interface whose only method is unavailable with neither a target nor a
/// reason — the bare-annotation lowered shape.
fn bare_nondum_module(interner: &mut Interner) -> HirModule {
    let method = HirInterfaceMethod {
        def_id: DefId(USER_DEF_ID_BASE + 1),
        availability: vec![HirAvailability::Nondum {
            target: None,
            reason: None,
        }],
        name: interner.intern("send"),
        params: Vec::new(),
        ret_ty: None,
        err_ty: None,
        posture: CallablePosture::SyncFinite,
        span: Span::default(),
    };
    let interface = HirInterface {
        name: interner.intern("Socket"),
        type_params: Vec::new(),
        methods: vec![method],
    };
    HirModule {
        items: vec![HirItem {
            id: HirId(1),
            def_id: DefId(USER_DEF_ID_BASE),
            annotations: Vec::new(),
            kind: HirItemKind::Interface(interface),
            span: Span::default(),
        }],
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    }
}

fn emit(
    surface: &impl KeywordSurface,
    types: &TypeTable,
    interner: &Interner,
    hir: &HirModule,
) -> String {
    FaberCodegen::new(types, interner)
        .generate(hir, &empty_presentation(), surface)
        .expect("bare nondum interface must emit")
        .code
}

fn annotation_line(code: &str) -> &str {
    code.lines()
        .find(|line| line.trim_start().starts_with('@'))
        .expect("emission carries the availability annotation")
}

#[test]
fn bare_nondum_availability_emits_unbraced_annotation() {
    let mut interner = Interner::new();
    let hir = bare_nondum_module(&mut interner);
    let types = TypeTable::new();

    let cases = [
        ("@ nondum", emit(&LatinSurface, &types, &interner, &hir)),
        ("@ unstable", emit(&EnglishSurface, &types, &interner, &hir)),
    ];
    for (expected, code) in cases {
        let annotation = annotation_line(&code).trim();
        assert_eq!(
            annotation, expected,
            "bare availability must emit only the annotation: {code}"
        );
        assert!(
            !annotation.contains("  "),
            "annotation line must not carry a doubled space: {code}"
        );
        assert!(
            !code.contains("{  }"),
            "bare availability must not emit an empty braced record: {code}"
        );
    }
}
