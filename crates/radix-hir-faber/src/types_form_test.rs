//! Authored type-form registry + renderer tests (authored-type-forms ATF-6).
//!
//! Contract: `type_to_faber_at` renders the recorded form node-exactly
//! through the target keyword surface, falls back to the canonical spelling
//! when a site carries no form, and fails closed on malformed forms (witness
//! mismatch, illegal level, wrong child arity/kind).

use super::*;
use crate::{FaberCodegen, empty_presentation};
use radix_hir::{
    HirAuthoredTypeUse, HirBlock, HirExpression, HirExpressionKind, HirId, HirLiteral, HirLocal,
    HirModule, HirOwnerRef, HirStatement, HirStatementKind, HirTypeSlot,
};
use radix_lexer::{Span, TokenKind};
use radix_types::{IndexExpr, InferVar, NumericWidth};

/// Latin reader surface (`la`): the canonical built-in vocabulary.
struct LaSurface;

impl KeywordSurface for LaSurface {
    fn keyword(&self, _kind: TokenKind) -> &str {
        "kw"
    }

    fn type_surface(&self, kind: TypeBindingKind) -> &str {
        match kind {
            TypeBindingKind::Primitive(Primitive::Numerus) => "numerus",
            TypeBindingKind::Primitive(Primitive::Fractus) => "fractus",
            TypeBindingKind::Primitive(Primitive::Textus) => "textus",
            TypeBindingKind::Primitive(Primitive::Octeti) => "octeti",
            TypeBindingKind::Collection(CollectionKind::Lista) => "lista",
            TypeBindingKind::Collection(CollectionKind::Tabula) => "tabula",
            TypeBindingKind::TensorFamily(TensorFamilyKind::Tensor) => "tensor",
            TypeBindingKind::TensorFamily(TensorFamilyKind::Vector) => "vector",
            TypeBindingKind::TensorFamily(TensorFamilyKind::Matrix) => "matrix",
            TypeBindingKind::TensorFamily(TensorFamilyKind::Sparsa) => "sparsa",
            _ => "ignotum",
        }
    }
}

/// English reader surface: built-in vocabulary projects (`lista` → `list`,
/// `numerus` → `int`); width markers keep their language-wide spelling.
struct EnSurface;

impl KeywordSurface for EnSurface {
    fn keyword(&self, _kind: TokenKind) -> &str {
        "kw"
    }

    fn type_surface(&self, kind: TypeBindingKind) -> &str {
        match kind {
            TypeBindingKind::Primitive(Primitive::Numerus) => "int",
            TypeBindingKind::Collection(CollectionKind::Lista) => "list",
            other => LaSurface.type_surface(other),
        }
    }
}

struct Fixture {
    types: TypeTable,
    interner: Interner,
    u32_ty: TypeId,
    f32_ty: TypeId,
    lista_u32: TypeId,
}

fn fixture() -> Fixture {
    let mut types = TypeTable::new();
    let u32_ty = types.sized_numeric(Primitive::Numerus, NumericWidth::U32);
    let f32_ty = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let lista_u32 = types.array(u32_ty);
    Fixture {
        interner: Interner::new(),
        u32_ty,
        f32_ty,
        lista_u32,
        types,
    }
}

fn declared_site(def: u32) -> HirTypeSite {
    HirTypeSite {
        owner: HirOwnerRef::Def(DefId(def)),
        slot: HirTypeSlot::Declared,
    }
}

fn semantic_form(level: TypeFormLevel, ty: TypeId, args: Vec<HirTypeFormArg>) -> HirTypeForm {
    HirTypeForm {
        ty,
        head: HirTypeFormHead::Semantic(level),
        args,
    }
}

fn type_arg(form: HirTypeForm) -> HirTypeFormArg {
    HirTypeFormArg::Type(Box::new(form))
}

fn codegen_with_forms(forms: Vec<(HirTypeSite, HirTypeForm)>) -> FaberCodegen {
    let mut codegen = FaberCodegen::default();
    codegen.authored_type_forms = forms.into_iter().collect();
    codegen
}

fn render_at(
    codegen: &FaberCodegen,
    site: &HirTypeSite,
    type_id: TypeId,
    types: &TypeTable,
    interner: &Interner,
    surface: &impl KeywordSurface,
) -> String {
    codegen
        .type_to_faber_at(
            site,
            type_id,
            types,
            &FxHashMap::default(),
            interner,
            surface,
        )
        .expect("form must render")
}

fn short_width(ty: TypeId) -> HirTypeForm {
    semantic_form(TypeFormLevel::SHORT, ty, Vec::new())
}

#[test]
fn missing_presentation_falls_back_to_canonical() {
    let fx = fixture();
    let codegen = FaberCodegen::default();
    let site = declared_site(1);

    // No form at the site: exact canonical/default spelling, including the
    // presentation-absent defaults (sized fractus stays the bare width).
    assert_eq!(
        render_at(
            &codegen,
            &site,
            fx.lista_u32,
            &fx.types,
            &fx.interner,
            &LaSurface
        ),
        "lista<numerus<u32>>"
    );
    assert_eq!(
        codegen.type_to_faber(
            fx.lista_u32,
            &fx.types,
            &FxHashMap::default(),
            &fx.interner,
            &LaSurface
        ),
        render_at(
            &codegen,
            &site,
            fx.lista_u32,
            &fx.types,
            &fx.interner,
            &LaSurface
        )
    );
    assert_eq!(
        render_at(
            &codegen,
            &site,
            fx.f32_ty,
            &fx.types,
            &fx.interner,
            &LaSurface
        ),
        "f32"
    );
}

#[test]
fn scalar_long_and_short_stay_distinct_over_one_type_id() {
    let fx = fixture();
    let long = codegen_with_forms(vec![(
        declared_site(1),
        semantic_form(TypeFormLevel::LONG, fx.u32_ty, Vec::new()),
    )]);
    let short = codegen_with_forms(vec![(declared_site(2), short_width(fx.u32_ty))]);

    assert_eq!(
        render_at(
            &long,
            &declared_site(1),
            fx.u32_ty,
            &fx.types,
            &fx.interner,
            &LaSurface
        ),
        "numerus<u32>"
    );
    assert_eq!(
        render_at(
            &short,
            &declared_site(2),
            fx.u32_ty,
            &fx.types,
            &fx.interner,
            &LaSurface
        ),
        "u32"
    );
}

#[test]
fn list_three_way_keeps_root_and_child_levels_exact() {
    let fx = fixture();
    let codegen = codegen_with_forms(vec![
        (
            declared_site(1),
            semantic_form(
                TypeFormLevel::LONG,
                fx.lista_u32,
                vec![type_arg(semantic_form(
                    TypeFormLevel::LONG,
                    fx.u32_ty,
                    Vec::new(),
                ))],
            ),
        ),
        (
            declared_site(2),
            semantic_form(
                TypeFormLevel::LONG,
                fx.lista_u32,
                vec![type_arg(short_width(fx.u32_ty))],
            ),
        ),
        (
            declared_site(3),
            semantic_form(
                TypeFormLevel::SHORT,
                fx.lista_u32,
                vec![type_arg(short_width(fx.u32_ty))],
            ),
        ),
    ]);

    // Three authored spellings over one interned `lista<u32>`.
    assert_eq!(
        render_at(
            &codegen,
            &declared_site(1),
            fx.lista_u32,
            &fx.types,
            &fx.interner,
            &LaSurface
        ),
        "lista<numerus<u32>>"
    );
    assert_eq!(
        render_at(
            &codegen,
            &declared_site(2),
            fx.lista_u32,
            &fx.types,
            &fx.interner,
            &LaSurface
        ),
        "lista<u32>"
    );
    assert_eq!(
        render_at(
            &codegen,
            &declared_site(3),
            fx.lista_u32,
            &fx.types,
            &fx.interner,
            &LaSurface
        ),
        "lu32"
    );
}

#[test]
fn tensor_two_way_keeps_long_and_short_roots() {
    let mut fx = fixture();
    let lit2 = fx.types.intern_index(IndexExpr::Literal(2));
    let lit3 = fx.types.intern_index(IndexExpr::Literal(3));
    let shape = fx.types.intern_index(IndexExpr::Tuple(vec![lit2, lit3]));
    let tensor = fx.types.tensor_with_shape(fx.f32_ty, shape);

    let args = || {
        vec![
            type_arg(short_width(fx.f32_ty)),
            HirTypeFormArg::Size(shape),
        ]
    };
    let long = codegen_with_forms(vec![(
        declared_site(1),
        semantic_form(TypeFormLevel::LONG, tensor, args()),
    )]);
    let short = codegen_with_forms(vec![(
        declared_site(2),
        semantic_form(TypeFormLevel::SHORT, tensor, args()),
    )]);

    assert_eq!(
        render_at(
            &long,
            &declared_site(1),
            tensor,
            &fx.types,
            &fx.interner,
            &LaSurface
        ),
        "tensor<f32, [2, 3]>"
    );
    assert_eq!(
        render_at(
            &short,
            &declared_site(2),
            tensor,
            &fx.types,
            &fx.interner,
            &LaSurface
        ),
        "tf32[2, 3]"
    );
}

#[test]
fn all_prefixed_families_round_trip_at_their_own_level() {
    let mut fx = fixture();
    let lit2 = fx.types.intern_index(IndexExpr::Literal(2));
    let lit4 = fx.types.intern_index(IndexExpr::Literal(4));
    let width4 = fx.types.intern_index(IndexExpr::Tuple(vec![lit4]));
    let square = fx.types.intern_index(IndexExpr::Tuple(vec![lit2, lit2]));

    // vector<f32, [4]> authored as `vf32[4]`.
    let vector = fx.types.vector_with_width(fx.f32_ty, width4);
    let vector_codegen = codegen_with_forms(vec![(
        declared_site(1),
        semantic_form(
            TypeFormLevel::SHORT,
            vector,
            vec![type_arg(short_width(fx.f32_ty))],
        ),
    )]);
    assert_eq!(
        render_at(
            &vector_codegen,
            &declared_site(1),
            vector,
            &fx.types,
            &fx.interner,
            &LaSurface
        ),
        "vf32[4]"
    );

    // matrix<f32, [2, 2]> authored as `mf32[2, 2]`.
    let matrix = fx.types.matrix_with_shape(fx.f32_ty, square);
    let matrix_codegen = codegen_with_forms(vec![(
        declared_site(2),
        semantic_form(
            TypeFormLevel::SHORT,
            matrix,
            vec![type_arg(short_width(fx.f32_ty))],
        ),
    )]);
    assert_eq!(
        render_at(
            &matrix_codegen,
            &declared_site(2),
            matrix,
            &fx.types,
            &fx.interner,
            &LaSurface
        ),
        "mf32[2, 2]"
    );

    // sparsa<f32, [2, 2]> authored as `sf32[2, 2]`.
    let sparsa = fx.types.sparsa_with_shape(fx.f32_ty, square);
    let sparsa_codegen = codegen_with_forms(vec![(
        declared_site(3),
        semantic_form(
            TypeFormLevel::SHORT,
            sparsa,
            vec![type_arg(short_width(fx.f32_ty))],
        ),
    )]);
    assert_eq!(
        render_at(
            &sparsa_codegen,
            &declared_site(3),
            sparsa,
            &fx.types,
            &fx.interner,
            &LaSurface
        ),
        "sf32[2, 2]"
    );
}

#[test]
fn fused_short_roots_without_figura_stay_bare() {
    // `vf32` authored without brackets lowers to an inferred width and
    // re-emits without brackets.
    let mut fx = fixture();
    let var = fx.types.fresh_index_infer_var();
    let width = fx.types.intern_index(IndexExpr::Infer(var));
    let vector = fx.types.vector_with_width(fx.f32_ty, width);
    let codegen = codegen_with_forms(vec![(
        declared_site(1),
        semantic_form(
            TypeFormLevel::SHORT,
            vector,
            vec![type_arg(short_width(fx.f32_ty))],
        ),
    )]);
    assert_eq!(
        render_at(
            &codegen,
            &declared_site(1),
            vector,
            &fx.types,
            &fx.interner,
            &LaSurface
        ),
        "vf32"
    );
}

#[test]
fn deep_nesting_changes_only_the_inner_node() {
    let mut fx = fixture();
    let lit2 = fx.types.intern_index(IndexExpr::Literal(2));
    let square = fx.types.intern_index(IndexExpr::Tuple(vec![lit2, lit2]));
    let tensor = fx.types.tensor_with_shape(fx.f32_ty, square);

    // list<list<lu32>>
    let fused_lista = semantic_form(
        TypeFormLevel::SHORT,
        fx.lista_u32,
        vec![type_arg(short_width(fx.u32_ty))],
    );
    let list_of_lista = fx.types.array(fx.lista_u32);
    let list_of_list = fx.types.array(list_of_lista);
    let list_fused = codegen_with_forms(vec![(
        declared_site(1),
        semantic_form(
            TypeFormLevel::LONG,
            list_of_list,
            vec![type_arg(semantic_form(
                TypeFormLevel::LONG,
                list_of_lista,
                vec![type_arg(fused_lista)],
            ))],
        ),
    )]);
    assert_eq!(
        render_at(
            &list_fused,
            &declared_site(1),
            list_of_list,
            &fx.types,
            &fx.interner,
            &LaSurface
        ),
        "lista<lista<lu32>>"
    );

    // list<list<list<tensor<f32, [2, 2]>>>>: only the tensor node's level
    // separates the two spellings.
    let inner_list = fx.types.array(tensor);
    let mid_list = fx.types.array(inner_list);
    let outer_list = fx.types.array(mid_list);
    let tensor_args = || {
        vec![
            type_arg(short_width(fx.f32_ty)),
            HirTypeFormArg::Size(square),
        ]
    };
    let wrap = |tensor_form: HirTypeForm| {
        semantic_form(
            TypeFormLevel::LONG,
            outer_list,
            vec![type_arg(semantic_form(
                TypeFormLevel::LONG,
                mid_list,
                vec![type_arg(semantic_form(
                    TypeFormLevel::LONG,
                    inner_list,
                    vec![type_arg(tensor_form)],
                ))],
            ))],
        )
    };
    let long = codegen_with_forms(vec![(
        declared_site(2),
        wrap(semantic_form(TypeFormLevel::LONG, tensor, tensor_args())),
    )]);
    let short = codegen_with_forms(vec![(
        declared_site(3),
        wrap(semantic_form(TypeFormLevel::SHORT, tensor, tensor_args())),
    )]);

    assert_eq!(
        render_at(
            &long,
            &declared_site(2),
            outer_list,
            &fx.types,
            &fx.interner,
            &LaSurface
        ),
        "lista<lista<lista<tensor<f32, [2, 2]>>>>"
    );
    assert_eq!(
        render_at(
            &short,
            &declared_site(3),
            outer_list,
            &fx.types,
            &fx.interner,
            &LaSurface
        ),
        "lista<lista<lista<tf32[2, 2]>>>"
    );
}

#[test]
fn shared_type_id_branches_keep_independent_forms() {
    // tabula<lista<u32>, lu32>: both branches intern the same element type,
    // yet keep their own authored form at their own site.
    let mut fx = fixture();
    let map = fx.types.map(fx.lista_u32, fx.lista_u32);
    let codegen = codegen_with_forms(vec![(
        declared_site(1),
        semantic_form(
            TypeFormLevel::LONG,
            map,
            vec![
                type_arg(semantic_form(
                    TypeFormLevel::LONG,
                    fx.lista_u32,
                    vec![type_arg(short_width(fx.u32_ty))],
                )),
                type_arg(semantic_form(
                    TypeFormLevel::SHORT,
                    fx.lista_u32,
                    vec![type_arg(short_width(fx.u32_ty))],
                )),
            ],
        ),
    )]);
    assert_eq!(
        render_at(
            &codegen,
            &declared_site(1),
            map,
            &fx.types,
            &fx.interner,
            &LaSurface
        ),
        "tabula<lista<u32>, lu32>"
    );
}

#[test]
fn named_heads_render_authored_paths_and_args() {
    let mut fx = fixture();
    let alias_def = DefId(0x9000_0010);
    let alias_ty = fx.types.intern(Type::Alias(alias_def, fx.u32_ty));
    let module = fx.interner.intern("module");
    let word = fx.interner.intern("Word");
    let form = HirTypeForm {
        ty: alias_ty,
        head: HirTypeFormHead::Named {
            def_id: alias_def,
            path: vec![module, word],
        },
        args: vec![type_arg(short_width(fx.u32_ty))],
    };
    let codegen = codegen_with_forms(vec![(declared_site(1), form)]);
    assert_eq!(
        render_at(
            &codegen,
            &declared_site(1),
            alias_ty,
            &fx.types,
            &fx.interner,
            &LaSurface
        ),
        "module.Word<u32>"
    );
}

#[test]
fn named_head_over_unrelated_def_fails_closed() {
    let mut fx = fixture();
    let alias_ty = fx.types.intern(Type::Alias(DefId(0x9000_0010), fx.u32_ty));
    let word = fx.interner.intern("Word");
    let form = HirTypeForm {
        ty: alias_ty,
        head: HirTypeFormHead::Named {
            def_id: DefId(0x9000_0020),
            path: vec![word],
        },
        args: Vec::new(),
    };
    let codegen = codegen_with_forms(vec![(declared_site(1), form)]);
    assert!(
        codegen
            .type_to_faber_at(
                &declared_site(1),
                alias_ty,
                &fx.types,
                &FxHashMap::default(),
                &fx.interner,
                &LaSurface
            )
            .is_err(),
        "named head spelling an unrelated definition must fail closed"
    );
}

#[test]
fn locale_projection_changes_vocabulary_and_preserves_levels() {
    let fx = fixture();
    let codegen = codegen_with_forms(vec![
        (
            declared_site(1),
            semantic_form(
                TypeFormLevel::LONG,
                fx.lista_u32,
                vec![type_arg(semantic_form(
                    TypeFormLevel::LONG,
                    fx.u32_ty,
                    Vec::new(),
                ))],
            ),
        ),
        (
            declared_site(2),
            semantic_form(
                TypeFormLevel::SHORT,
                fx.lista_u32,
                vec![type_arg(short_width(fx.u32_ty))],
            ),
        ),
        (
            declared_site(3),
            semantic_form(TypeFormLevel::LONG, fx.u32_ty, Vec::new()),
        ),
        (declared_site(4), short_width(fx.u32_ty)),
    ]);

    // en: built-in vocabulary projects; compression levels survive.
    assert_eq!(
        render_at(
            &codegen,
            &declared_site(1),
            fx.lista_u32,
            &fx.types,
            &fx.interner,
            &EnSurface
        ),
        "list<int<u32>>"
    );
    assert_eq!(
        render_at(
            &codegen,
            &declared_site(2),
            fx.lista_u32,
            &fx.types,
            &fx.interner,
            &EnSurface
        ),
        "lu32"
    );
    assert_eq!(
        render_at(
            &codegen,
            &declared_site(3),
            fx.u32_ty,
            &fx.types,
            &fx.interner,
            &EnSurface
        ),
        "int<u32>"
    );
    assert_eq!(
        render_at(
            &codegen,
            &declared_site(4),
            fx.u32_ty,
            &fx.types,
            &fx.interner,
            &EnSurface
        ),
        "u32"
    );

    // la: same forms, canonical vocabulary.
    assert_eq!(
        render_at(
            &codegen,
            &declared_site(1),
            fx.lista_u32,
            &fx.types,
            &fx.interner,
            &LaSurface
        ),
        "lista<numerus<u32>>"
    );
    assert_eq!(
        render_at(
            &codegen,
            &declared_site(2),
            fx.lista_u32,
            &fx.types,
            &fx.interner,
            &LaSurface
        ),
        "lu32"
    );
    assert_eq!(
        render_at(
            &codegen,
            &declared_site(3),
            fx.u32_ty,
            &fx.types,
            &fx.interner,
            &LaSurface
        ),
        "numerus<u32>"
    );
    assert_eq!(
        render_at(
            &codegen,
            &declared_site(4),
            fx.u32_ty,
            &fx.types,
            &fx.interner,
            &LaSurface
        ),
        "u32"
    );
}

#[test]
fn registry_renders_special_and_sized_heads() {
    let mut fx = fixture();

    // `octeti` is its own primitive: L0-only (delivery Settled Decision 1).
    let octeti = fx.types.primitive(Primitive::Octeti);
    let octeti_codegen = codegen_with_forms(vec![(
        declared_site(1),
        semantic_form(TypeFormLevel::LONG, octeti, Vec::new()),
    )]);
    assert_eq!(
        render_at(
            &octeti_codegen,
            &declared_site(1),
            octeti,
            &fx.types,
            &fx.interner,
            &LaSurface
        ),
        "octeti"
    );

    // modulus<u32>: contextual keyword + unsigned width marker.
    let word = fx.types.intern(Type::ModularWord(NumericWidth::U32));
    let word_codegen = codegen_with_forms(vec![(
        declared_site(2),
        semantic_form(TypeFormLevel::LONG, word, Vec::new()),
    )]);
    assert_eq!(
        render_at(
            &word_codegen,
            &declared_site(2),
            word,
            &fx.types,
            &fx.interner,
            &LaSurface
        ),
        "modulus<u32>"
    );

    // LONG fractus keeps its wrapped spelling; only the presentation-absent
    // default emits the bare marker.
    let fractus_long = codegen_with_forms(vec![(
        declared_site(3),
        semantic_form(TypeFormLevel::LONG, fx.f32_ty, Vec::new()),
    )]);
    assert_eq!(
        render_at(
            &fractus_long,
            &declared_site(3),
            fx.f32_ty,
            &fx.types,
            &fx.interner,
            &LaSurface
        ),
        "fractus<f32>"
    );
}

#[test]
fn hole_heads_render_the_authored_hole() {
    let mut fx = fixture();
    let infer_ty = fx.types.intern(Type::Infer(InferVar(0)));
    let union_ty = fx.types.intern(Type::InferUnion(InferVar(1)));
    let codegen = codegen_with_forms(vec![
        (
            declared_site(1),
            HirTypeForm {
                ty: infer_ty,
                head: HirTypeFormHead::Infer,
                args: Vec::new(),
            },
        ),
        (
            declared_site(2),
            HirTypeForm {
                ty: union_ty,
                head: HirTypeFormHead::UnionHole,
                args: Vec::new(),
            },
        ),
    ]);
    assert_eq!(
        render_at(
            &codegen,
            &declared_site(1),
            infer_ty,
            &fx.types,
            &fx.interner,
            &LaSurface
        ),
        "_"
    );
    assert_eq!(
        render_at(
            &codegen,
            &declared_site(2),
            union_ty,
            &fx.types,
            &fx.interner,
            &LaSurface
        ),
        "∪"
    );
}

#[test]
fn witness_mismatch_fails_closed() {
    let mut fx = fixture();
    let other = fx.types.array(fx.f32_ty);
    let codegen = codegen_with_forms(vec![(
        declared_site(1),
        semantic_form(
            TypeFormLevel::LONG,
            fx.lista_u32,
            vec![type_arg(short_width(fx.u32_ty))],
        ),
    )]);
    let err = codegen
        .type_to_faber_at(
            &declared_site(1),
            other,
            &fx.types,
            &FxHashMap::default(),
            &fx.interner,
            &LaSurface,
        )
        .expect_err("witness mismatch must fail closed");
    assert!(
        err.args
            .iter()
            .any(|arg| arg.name == "issue" && arg.value == "authored_type_form_witness_mismatch"),
        "expected structured authored_type_form_witness_mismatch issue, got: {err:?}"
    );
}

#[test]
fn illegal_levels_and_arity_fail_closed() {
    let mut fx = fixture();
    let numerus = fx.types.primitive(Primitive::Numerus);
    let lit2 = fx.types.intern_index(IndexExpr::Literal(2));
    let square = fx.types.intern_index(IndexExpr::Tuple(vec![lit2, lit2]));
    let tensor = fx.types.tensor_with_shape(fx.f32_ty, square);
    let other_shape = fx.types.intern_index(IndexExpr::Literal(9));

    // (site, requested type, form) triples that must all be rejected.
    let rejects: Vec<(HirTypeSite, TypeId, HirTypeForm)> = vec![
        // SHORT on a constructor with no compression form.
        (
            declared_site(1),
            numerus,
            semantic_form(TypeFormLevel::SHORT, numerus, Vec::new()),
        ),
        // Unknown level.
        (
            declared_site(2),
            fx.u32_ty,
            semantic_form(TypeFormLevel(2), fx.u32_ty, Vec::new()),
        ),
        // LONG lista with no children.
        (
            declared_site(3),
            fx.lista_u32,
            semantic_form(TypeFormLevel::LONG, fx.lista_u32, Vec::new()),
        ),
        // LONG sized numeric carrying a width child (widths ride the witness).
        (
            declared_site(4),
            fx.u32_ty,
            semantic_form(
                TypeFormLevel::LONG,
                fx.u32_ty,
                vec![type_arg(short_width(fx.u32_ty))],
            ),
        ),
        // Fused root whose child is not a SHORT width.
        (
            declared_site(5),
            fx.lista_u32,
            semantic_form(
                TypeFormLevel::SHORT,
                fx.lista_u32,
                vec![type_arg(semantic_form(
                    TypeFormLevel::LONG,
                    fx.u32_ty,
                    Vec::new(),
                ))],
            ),
        ),
        // Fused root over a non-numeric child witness.
        (
            declared_site(6),
            fx.lista_u32,
            semantic_form(
                TypeFormLevel::SHORT,
                fx.lista_u32,
                vec![type_arg(short_width(numerus))],
            ),
        ),
        // Tensor missing its size argument.
        (
            declared_site(7),
            tensor,
            semantic_form(
                TypeFormLevel::LONG,
                tensor,
                vec![type_arg(short_width(fx.f32_ty))],
            ),
        ),
        // Tensor size argument contradicting the witness.
        (
            declared_site(8),
            tensor,
            semantic_form(
                TypeFormLevel::SHORT,
                tensor,
                vec![
                    type_arg(short_width(fx.f32_ty)),
                    HirTypeFormArg::Size(other_shape),
                ],
            ),
        ),
        // Hole heads carrying children.
        (
            declared_site(9),
            fx.u32_ty,
            HirTypeForm {
                ty: fx.u32_ty,
                head: HirTypeFormHead::Infer,
                args: vec![type_arg(short_width(fx.u32_ty))],
            },
        ),
    ];

    for (site, type_id, form) in rejects {
        let codegen = codegen_with_forms(vec![(site.clone(), form)]);
        assert!(
            codegen
                .type_to_faber_at(
                    &site,
                    type_id,
                    &fx.types,
                    &FxHashMap::default(),
                    &fx.interner,
                    &LaSurface
                )
                .is_err(),
            "malformed form at {site:?} must fail closed"
        );
    }
}

#[test]
fn generate_local_hole_uses_authored_form_without_legacy_flags() {
    let mut fx = fixture();
    let name = fx.interner.intern("x");
    let infer_ty = fx.types.intern(Type::Infer(InferVar(0)));
    let def_id = DefId(1);
    let site = declared_site(def_id.0);
    let local = HirLocal {
        def_id,
        name,
        ty: Some(infer_ty),
        init: Some(HirExpression {
            id: HirId(2),
            kind: HirExpressionKind::Literal(HirLiteral::Int(1)),
            ty: Some(infer_ty),
            span: Span::default(),
        }),
        mutable: false,
        is_await: false,
        runtime_provided: false,
    };
    let hir = HirModule {
        items: Vec::new(),
        entry: Some(HirBlock::simple(
            vec![HirStatement {
                id: HirId(3),
                kind: HirStatementKind::Local(local),
                span: Span::default(),
            }],
            None,
            Span::default(),
        )),
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };
    let mut presentation = empty_presentation();
    presentation.authored_types.push(HirAuthoredTypeUse {
        site,
        form: HirTypeForm {
            ty: infer_ty,
            head: HirTypeFormHead::Infer,
            args: Vec::new(),
        },
    });

    let output = FaberCodegen::default()
        .generate(&hir, &presentation, &fx.types, &fx.interner, &LaSurface)
        .expect("authored hole must emit");
    assert!(
        output.code.contains("fixum _ x ←"),
        "authored infer form must re-emit as a hole: {}",
        output.code
    );
}

#[test]
fn generate_builds_the_type_site_index_from_presentation() {
    let fx = fixture();
    let site = declared_site(7);
    let form = short_width(fx.u32_ty);
    let mut presentation = empty_presentation();
    presentation.authored_types.push(HirAuthoredTypeUse {
        site: site.clone(),
        form,
    });

    let hir = HirModule {
        items: Vec::new(),
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };
    let mut codegen = FaberCodegen::default();
    codegen
        .generate(&hir, &presentation, &fx.types, &fx.interner, &LaSurface)
        .expect("empty program must emit");
    assert_eq!(
        codegen.authored_type_forms.get(&site),
        presentation
            .authored_types
            .first()
            .map(|authored| &authored.form),
        "generate must index authored forms by site"
    );

    // Empty presentation keeps everything canonical.
    let empty = empty_presentation();
    let mut fresh = FaberCodegen::default();
    fresh
        .generate(&hir, &empty, &fx.types, &fx.interner, &LaSurface)
        .expect("empty program must emit");
    assert!(fresh.authored_type_forms.is_empty());
}
