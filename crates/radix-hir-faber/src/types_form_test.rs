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
use radix_types::{IndexExpr, InferVar, NumericWidth, TypeTable};

/// Latin reader surface (`la`): the canonical built-in vocabulary.
struct LaSurface;

impl KeywordSurface for LaSurface {
    fn keyword(&self, _kind: TokenKind) -> &'static str {
        "kw"
    }

    fn type_surface(&self, kind: TypeBindingKind) -> &str {
        match kind {
            TypeBindingKind::Primitive(Primitive::Numerus) => "numerus",
            TypeBindingKind::Primitive(Primitive::Fractus) => "fractus",
            TypeBindingKind::Primitive(Primitive::Textus) => "textus",
            TypeBindingKind::Primitive(Primitive::Octeti) => "octeti",
            TypeBindingKind::Ratio => "ratio",
            TypeBindingKind::TrappingWord => "exactus",
            TypeBindingKind::SaturatingWord => "saturatus",
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
    fn keyword(&self, _kind: TokenKind) -> &'static str {
        "kw"
    }

    fn type_surface(&self, kind: TypeBindingKind) -> &str {
        match kind {
            TypeBindingKind::Primitive(Primitive::Numerus) => "int",
            TypeBindingKind::Ratio => "record",
            TypeBindingKind::TrappingWord => "trapping",
            TypeBindingKind::SaturatingWord => "saturating",
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
        label: None,
    }
}

fn type_arg(form: HirTypeForm) -> HirTypeFormArg {
    HirTypeFormArg::Type(Box::new(form))
}

fn codegen_with_forms(fx: &Fixture, forms: Vec<(HirTypeSite, HirTypeForm)>) -> FaberCodegen<'_> {
    let mut codegen = FaberCodegen::new(&fx.types, &fx.interner);
    codegen.authored_type_forms = forms.into_iter().collect();
    codegen
}

fn render_at(
    codegen: &FaberCodegen<'_>,
    site: &HirTypeSite,
    type_id: TypeId,
    surface: &impl KeywordSurface,
) -> String {
    codegen
        .type_to_faber_at(site, type_id, &FxHashMap::default(), surface)
        .expect("form must render")
}

fn short_width(ty: TypeId) -> HirTypeForm {
    semantic_form(TypeFormLevel::SHORT, ty, Vec::new())
}

/// Bits markers (`b8`..`b64`) round trip as bare, locale-neutral tokens at
/// SHORT and LONG, bare and as a list child, in both locales.
#[test]
fn bits_markers_render_bare_at_short_and_long_in_both_locales() {
    use radix_types::BitsWidth;
    let mut types = TypeTable::new();
    let interner = Interner::new();
    for (width, marker) in [
        (BitsWidth::B8, "b8"),
        (BitsWidth::B16, "b16"),
        (BitsWidth::B32, "b32"),
        (BitsWidth::B64, "b64"),
    ] {
        let bits_ty = types.bits(width);
        let list_ty = types.array(bits_ty);
        let list_form = semantic_form(
            TypeFormLevel::LONG,
            list_ty,
            vec![type_arg(short_width(bits_ty))],
        );
        let forms = vec![
            (declared_site(1), short_width(bits_ty)),
            (
                declared_site(2),
                semantic_form(TypeFormLevel::LONG, bits_ty, Vec::new()),
            ),
            (declared_site(3), list_form),
        ];
        let mut codegen = FaberCodegen::new(&types, &interner);
        codegen.authored_type_forms = forms.into_iter().collect();
        assert_eq!(
            render_at(&codegen, &declared_site(1), bits_ty, &LaSurface),
            marker
        );
        assert_eq!(
            render_at(&codegen, &declared_site(1), bits_ty, &EnSurface),
            marker
        );
        assert_eq!(
            render_at(&codegen, &declared_site(2), bits_ty, &LaSurface),
            marker
        );
        assert_eq!(
            render_at(&codegen, &declared_site(3), list_ty, &LaSurface),
            format!("lista<{marker}>")
        );
        assert_eq!(
            render_at(&codegen, &declared_site(3), list_ty, &EnSurface),
            format!("list<{marker}>")
        );
    }
}

/// An authored empty-argument list form is the retired `octeti`/`bytes`
/// word; it re-emits as the raw-bit list (ruling d477c76d).
#[test]
fn empty_arg_b8_list_form_renders_as_list_form() {
    use radix_types::BitsWidth;
    let mut types = TypeTable::new();
    let interner = Interner::new();
    let list_ty = types.bits(BitsWidth::B8);
    let list_ty = types.array(list_ty);
    let form = semantic_form(TypeFormLevel::LONG, list_ty, Vec::new());
    let forms = vec![(declared_site(1), form.clone()), (declared_site(2), form)];
    let mut codegen = FaberCodegen::new(&types, &interner);
    codegen.authored_type_forms = forms.into_iter().collect();
    assert_eq!(
        render_at(&codegen, &declared_site(1), list_ty, &LaSurface),
        "lista<b8>"
    );
    assert_eq!(
        render_at(&codegen, &declared_site(2), list_ty, &EnSurface),
        "list<b8>"
    );
}

/// An authored `iuncta` element label rides the child form and re-emits as
/// `label: type`; unlabeled slots keep the bare spelling.
#[test]
fn labeled_iuncta_elements_render_with_their_authored_labels() {
    let mut fx = fixture();
    let tuple_ty = fx.types.tuple(vec![fx.u32_ty, fx.f32_ty]);
    let mut labeled = semantic_form(TypeFormLevel::LONG, fx.u32_ty, Vec::new());
    labeled.label = Some(fx.interner.intern("gx"));
    let form = semantic_form(
        TypeFormLevel::LONG,
        tuple_ty,
        vec![type_arg(labeled), type_arg(short_width(fx.f32_ty))],
    );
    let site = declared_site(1);
    let codegen = codegen_with_forms(&fx, vec![(site.clone(), form)]);
    assert_eq!(
        render_at(&codegen, &site, tuple_ty, &LaSurface),
        "iuncta<gx: u32, f32>"
    );
}

/// The authored labeled record form writes the surface's own `[types]`
/// spelling of the record head (en `record`, la `ratio`), never the Latin
/// word under a non-Latin surface.
#[test]
fn labeled_record_form_renders_the_surface_record_spelling() {
    let mut fx = fixture();
    let g = fx.interner.intern("g");
    let w = fx.interner.intern("w");
    let record_ty = fx.types.record([(g, fx.f32_ty), (w, fx.u32_ty)]);
    let mut labeled_g = semantic_form(TypeFormLevel::LONG, fx.f32_ty, Vec::new());
    labeled_g.label = Some(g);
    let mut labeled_w = semantic_form(TypeFormLevel::LONG, fx.u32_ty, Vec::new());
    labeled_w.label = Some(w);
    let form = semantic_form(
        TypeFormLevel::LONG,
        record_ty,
        vec![type_arg(labeled_g), type_arg(labeled_w)],
    );
    let site = declared_site(1);
    let codegen = codegen_with_forms(&fx, vec![(site.clone(), form)]);
    let la = render_at(&codegen, &site, record_ty, &LaSurface);
    let en = render_at(&codegen, &site, record_ty, &EnSurface);
    assert!(la.starts_with("ratio<g: "), "la must keep ratio: {la}");
    assert!(en.starts_with("record<g: "), "en must write record: {en}");
    assert!(!en.contains("ratio"), "en must carry no Latin ratio: {en}");
}

#[test]
fn missing_presentation_falls_back_to_canonical() {
    let fx = fixture();
    let codegen = FaberCodegen::new(&fx.types, &fx.interner);
    let site = declared_site(1);

    // No form at the site: exact canonical/default spelling, including the
    // presentation-absent defaults (sized numerics are the bare width).
    assert_eq!(
        render_at(&codegen, &site, fx.lista_u32, &LaSurface),
        "lista<u32>"
    );
    assert_eq!(
        codegen.type_to_faber(fx.lista_u32, &FxHashMap::default(), &LaSurface),
        render_at(&codegen, &site, fx.lista_u32, &LaSurface)
    );
    assert_eq!(render_at(&codegen, &site, fx.f32_ty, &LaSurface), "f32");
}

#[test]
fn scalar_long_and_short_share_the_bare_width_over_one_type_id() {
    let fx = fixture();
    let long = codegen_with_forms(
        &fx,
        vec![(
            declared_site(1),
            semantic_form(TypeFormLevel::LONG, fx.u32_ty, Vec::new()),
        )],
    );
    let short = codegen_with_forms(&fx, vec![(declared_site(2), short_width(fx.u32_ty))]);

    // The long `numerus<u32>` spelling is retired (N7c): LONG re-emits as
    // the bare width, the same as SHORT.
    assert_eq!(
        render_at(&long, &declared_site(1), fx.u32_ty, &LaSurface),
        "u32"
    );
    assert_eq!(
        render_at(&short, &declared_site(2), fx.u32_ty, &LaSurface),
        "u32"
    );
}

/// D11.8: the explicit policy word and the bare marker stay two distinct
/// spellings over one `TypeId` (the retired long head re-emits as the bare
/// marker); each re-emits as authored, in both locales, and a list child
/// keeps its own level.
#[test]
fn trapping_policy_word_keeps_its_own_spelling_over_one_type_id() {
    let fx = fixture();
    let policy = semantic_form(TypeFormLevel::POLICY, fx.u32_ty, Vec::new());
    let codegen = codegen_with_forms(
        &fx,
        vec![
            (declared_site(1), policy.clone()),
            (declared_site(2), short_width(fx.u32_ty)),
            (
                declared_site(3),
                semantic_form(TypeFormLevel::LONG, fx.u32_ty, Vec::new()),
            ),
            (
                declared_site(4),
                semantic_form(TypeFormLevel::LONG, fx.lista_u32, vec![type_arg(policy)]),
            ),
        ],
    );

    assert_eq!(
        render_at(&codegen, &declared_site(1), fx.u32_ty, &LaSurface),
        "exactus<u32>"
    );
    assert_eq!(
        render_at(&codegen, &declared_site(1), fx.u32_ty, &EnSurface),
        "trapping<u32>"
    );
    assert_eq!(
        render_at(&codegen, &declared_site(2), fx.u32_ty, &EnSurface),
        "u32"
    );
    assert_eq!(
        render_at(&codegen, &declared_site(3), fx.u32_ty, &EnSurface),
        "u32"
    );
    assert_eq!(
        render_at(&codegen, &declared_site(4), fx.lista_u32, &EnSurface),
        "list<trapping<u32>>"
    );
}

/// N7E-1: the policy-float cell re-emits through the existing
/// `TrappingWord` / `SaturatingWord` bindings in both locales, both through
/// the canonical spelling (no recorded form) and through a recorded LONG form.
#[test]
fn policy_float_cells_re_emit_in_both_locales() {
    let mut fx = fixture();
    let trapping_f32 = fx.types.intern(Type::PolicyFloat(
        radix_types::FloatPolicy::Trapping,
        NumericWidth::F32,
    ));
    let saturating_f16 = fx.types.intern(Type::PolicyFloat(
        radix_types::FloatPolicy::Saturating,
        NumericWidth::F16,
    ));
    let codegen = codegen_with_forms(
        &fx,
        vec![
            (
                declared_site(1),
                semantic_form(TypeFormLevel::LONG, trapping_f32, Vec::new()),
            ),
            (
                declared_site(2),
                semantic_form(TypeFormLevel::LONG, saturating_f16, Vec::new()),
            ),
        ],
    );
    let names = FxHashMap::default();

    assert_eq!(
        codegen.type_to_faber(trapping_f32, &names, &LaSurface),
        "exactus<f32>"
    );
    assert_eq!(
        codegen.type_to_faber(trapping_f32, &names, &EnSurface),
        "trapping<f32>"
    );
    assert_eq!(
        codegen.type_to_faber(saturating_f16, &names, &LaSurface),
        "saturatus<f16>"
    );
    assert_eq!(
        codegen.type_to_faber(saturating_f16, &names, &EnSurface),
        "saturating<f16>"
    );

    assert_eq!(
        render_at(&codegen, &declared_site(1), trapping_f32, &LaSurface),
        "exactus<f32>"
    );
    assert_eq!(
        render_at(&codegen, &declared_site(1), trapping_f32, &EnSurface),
        "trapping<f32>"
    );
    assert_eq!(
        render_at(&codegen, &declared_site(2), saturating_f16, &LaSurface),
        "saturatus<f16>"
    );
    assert_eq!(
        render_at(&codegen, &declared_site(2), saturating_f16, &EnSurface),
        "saturating<f16>"
    );
}

/// N7E-2c: the authored POLICY level over a policy-float witness spells the
/// cell as authored (`trapping<f32>` / `saturating<f32>`) in both locales; a
/// recorded argument is still malformed.
#[test]
fn policy_float_cells_re_emit_through_the_policy_level() {
    let mut fx = fixture();
    let trapping_f32 = fx.types.intern(Type::PolicyFloat(
        radix_types::FloatPolicy::Trapping,
        NumericWidth::F32,
    ));
    let saturating_f32 = fx.types.intern(Type::PolicyFloat(
        radix_types::FloatPolicy::Saturating,
        NumericWidth::F32,
    ));
    let codegen = codegen_with_forms(
        &fx,
        vec![
            (
                declared_site(1),
                semantic_form(TypeFormLevel::POLICY, trapping_f32, Vec::new()),
            ),
            (
                declared_site(2),
                semantic_form(TypeFormLevel::POLICY, saturating_f32, Vec::new()),
            ),
        ],
    );

    assert_eq!(
        render_at(&codegen, &declared_site(1), trapping_f32, &LaSurface),
        "exactus<f32>"
    );
    assert_eq!(
        render_at(&codegen, &declared_site(1), trapping_f32, &EnSurface),
        "trapping<f32>"
    );
    assert_eq!(
        render_at(&codegen, &declared_site(2), saturating_f32, &LaSurface),
        "saturatus<f32>"
    );
    assert_eq!(
        render_at(&codegen, &declared_site(2), saturating_f32, &EnSurface),
        "saturating<f32>"
    );
}

#[test]
fn list_three_way_keeps_root_and_child_levels_exact() {
    let fx = fixture();
    let codegen = codegen_with_forms(
        &fx,
        vec![
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
        ],
    );

    // Three authored spellings over one interned `lista<u32>`; the retired
    // long child spelling re-emits as the bare marker.
    assert_eq!(
        render_at(&codegen, &declared_site(1), fx.lista_u32, &LaSurface),
        "lista<u32>"
    );
    assert_eq!(
        render_at(&codegen, &declared_site(2), fx.lista_u32, &LaSurface),
        "lista<u32>"
    );
    assert_eq!(
        render_at(&codegen, &declared_site(3), fx.lista_u32, &LaSurface),
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
    let long = codegen_with_forms(
        &fx,
        vec![(
            declared_site(1),
            semantic_form(TypeFormLevel::LONG, tensor, args()),
        )],
    );
    let short = codegen_with_forms(
        &fx,
        vec![(
            declared_site(2),
            semantic_form(TypeFormLevel::SHORT, tensor, args()),
        )],
    );

    assert_eq!(
        render_at(&long, &declared_site(1), tensor, &LaSurface),
        "tensor<f32, [2, 3]>"
    );
    assert_eq!(
        render_at(&short, &declared_site(2), tensor, &LaSurface),
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
    let vector_codegen = codegen_with_forms(
        &fx,
        vec![(
            declared_site(1),
            semantic_form(
                TypeFormLevel::SHORT,
                vector,
                vec![type_arg(short_width(fx.f32_ty))],
            ),
        )],
    );
    assert_eq!(
        render_at(&vector_codegen, &declared_site(1), vector, &LaSurface),
        "vf32[4]"
    );

    // matrix<f32, [2, 2]> authored as `mf32[2, 2]`.
    let matrix = fx.types.matrix_with_shape(fx.f32_ty, square);
    let matrix_codegen = codegen_with_forms(
        &fx,
        vec![(
            declared_site(2),
            semantic_form(
                TypeFormLevel::SHORT,
                matrix,
                vec![type_arg(short_width(fx.f32_ty))],
            ),
        )],
    );
    assert_eq!(
        render_at(&matrix_codegen, &declared_site(2), matrix, &LaSurface),
        "mf32[2, 2]"
    );

    // sparsa<f32, [2, 2]> authored as `sf32[2, 2]`.
    let sparsa = fx.types.sparsa_with_shape(fx.f32_ty, square);
    let sparsa_codegen = codegen_with_forms(
        &fx,
        vec![(
            declared_site(3),
            semantic_form(
                TypeFormLevel::SHORT,
                sparsa,
                vec![type_arg(short_width(fx.f32_ty))],
            ),
        )],
    );
    assert_eq!(
        render_at(&sparsa_codegen, &declared_site(3), sparsa, &LaSurface),
        "sf32[2, 2]"
    );
}

#[test]
fn literal_width_fused_sugar_re_emits_bracketed() {
    // ON-S1: lowering interns `vf32[4]`'s width as a bare Literal, not a
    // one-element Tuple; the literal path must still emit the bracketed
    // sugar (`vf32[4]`), never the concatenated `vf324`, which re-analysis
    // rejects as an unknown type.
    let mut fx = fixture();
    let lit4 = fx.types.intern_index(IndexExpr::Literal(4));
    let vector = fx.types.vector_with_width(fx.f32_ty, lit4);
    let codegen = codegen_with_forms(
        &fx,
        vec![(
            declared_site(1),
            semantic_form(
                TypeFormLevel::SHORT,
                vector,
                vec![type_arg(short_width(fx.f32_ty))],
            ),
        )],
    );
    assert_eq!(
        render_at(&codegen, &declared_site(1), vector, &LaSurface),
        "vf32[4]"
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
    let codegen = codegen_with_forms(
        &fx,
        vec![(
            declared_site(1),
            semantic_form(
                TypeFormLevel::SHORT,
                vector,
                vec![type_arg(short_width(fx.f32_ty))],
            ),
        )],
    );
    assert_eq!(
        render_at(&codegen, &declared_site(1), vector, &LaSurface),
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
    let list_fused = codegen_with_forms(
        &fx,
        vec![(
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
        )],
    );
    assert_eq!(
        render_at(&list_fused, &declared_site(1), list_of_list, &LaSurface),
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
    let long = codegen_with_forms(
        &fx,
        vec![(
            declared_site(2),
            wrap(semantic_form(TypeFormLevel::LONG, tensor, tensor_args())),
        )],
    );
    let short = codegen_with_forms(
        &fx,
        vec![(
            declared_site(3),
            wrap(semantic_form(TypeFormLevel::SHORT, tensor, tensor_args())),
        )],
    );

    assert_eq!(
        render_at(&long, &declared_site(2), outer_list, &LaSurface),
        "lista<lista<lista<tensor<f32, [2, 2]>>>>"
    );
    assert_eq!(
        render_at(&short, &declared_site(3), outer_list, &LaSurface),
        "lista<lista<lista<tf32[2, 2]>>>"
    );
}

#[test]
fn shared_type_id_branches_keep_independent_forms() {
    // tabula<lista<u32>, lu32>: both branches intern the same element type,
    // yet keep their own authored form at their own site.
    let mut fx = fixture();
    let map = fx.types.map(fx.lista_u32, fx.lista_u32);
    let codegen = codegen_with_forms(
        &fx,
        vec![(
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
        )],
    );
    assert_eq!(
        render_at(&codegen, &declared_site(1), map, &LaSurface),
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
        label: None,
    };
    let codegen = codegen_with_forms(&fx, vec![(declared_site(1), form)]);
    assert_eq!(
        render_at(&codegen, &declared_site(1), alias_ty, &LaSurface),
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
        label: None,
    };
    let codegen = codegen_with_forms(&fx, vec![(declared_site(1), form)]);
    assert!(
        codegen
            .type_to_faber_at(
                &declared_site(1),
                alias_ty,
                &FxHashMap::default(),
                &LaSurface
            )
            .is_err(),
        "named head spelling an unrelated definition must fail closed"
    );
}

#[test]
fn locale_projection_changes_vocabulary_and_preserves_levels() {
    let fx = fixture();
    let codegen = codegen_with_forms(
        &fx,
        vec![
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
        ],
    );

    // en: built-in vocabulary projects; compression levels survive.
    assert_eq!(
        render_at(&codegen, &declared_site(1), fx.lista_u32, &EnSurface),
        "list<u32>"
    );
    assert_eq!(
        render_at(&codegen, &declared_site(2), fx.lista_u32, &EnSurface),
        "lu32"
    );
    assert_eq!(
        render_at(&codegen, &declared_site(3), fx.u32_ty, &EnSurface),
        "u32"
    );
    assert_eq!(
        render_at(&codegen, &declared_site(4), fx.u32_ty, &EnSurface),
        "u32"
    );

    // la: same forms, canonical vocabulary.
    assert_eq!(
        render_at(&codegen, &declared_site(1), fx.lista_u32, &LaSurface),
        "lista<u32>"
    );
    assert_eq!(
        render_at(&codegen, &declared_site(2), fx.lista_u32, &LaSurface),
        "lu32"
    );
    assert_eq!(
        render_at(&codegen, &declared_site(3), fx.u32_ty, &LaSurface),
        "u32"
    );
    assert_eq!(
        render_at(&codegen, &declared_site(4), fx.u32_ty, &LaSurface),
        "u32"
    );
}

#[test]
fn registry_renders_special_and_sized_heads() {
    let mut fx = fixture();

    // A raw-bit buffer is a `lista<b8>`; the `octeti` word is retired
    // (ruling d477c76d). L0-only (delivery Settled Decision 1).
    let octeti = fx.types.primitive(Primitive::Octeti);
    let octeti_codegen = codegen_with_forms(
        &fx,
        vec![(
            declared_site(1),
            semantic_form(TypeFormLevel::LONG, octeti, Vec::new()),
        )],
    );
    assert_eq!(
        render_at(&octeti_codegen, &declared_site(1), octeti, &LaSurface),
        "lista<b8>"
    );

    // modulus<u32>: contextual keyword + unsigned width marker.
    let word = fx.types.intern(Type::ModularWord(NumericWidth::U32));
    let word_codegen = codegen_with_forms(
        &fx,
        vec![(
            declared_site(2),
            semantic_form(TypeFormLevel::LONG, word, Vec::new()),
        )],
    );
    assert_eq!(
        render_at(&word_codegen, &declared_site(2), word, &LaSurface),
        "modulus<u32>"
    );

    // LONG fractus re-emits as the bare marker (the wrapped spelling is
    // retired, N7c), like the presentation-absent default.
    let fractus_long = codegen_with_forms(
        &fx,
        vec![(
            declared_site(3),
            semantic_form(TypeFormLevel::LONG, fx.f32_ty, Vec::new()),
        )],
    );
    assert_eq!(
        render_at(&fractus_long, &declared_site(3), fx.f32_ty, &LaSurface),
        "f32"
    );
}

#[test]
fn hole_heads_render_the_authored_hole() {
    let mut fx = fixture();
    let infer_ty = fx.types.intern(Type::Infer(InferVar(0)));
    let union_ty = fx.types.intern(Type::InferUnion(InferVar(1)));
    let codegen = codegen_with_forms(
        &fx,
        vec![
            (
                declared_site(1),
                HirTypeForm {
                    ty: infer_ty,
                    head: HirTypeFormHead::Infer,
                    args: Vec::new(),
                    label: None,
                },
            ),
            (
                declared_site(2),
                HirTypeForm {
                    ty: union_ty,
                    head: HirTypeFormHead::UnionHole,
                    args: Vec::new(),
                    label: None,
                },
            ),
        ],
    );
    assert_eq!(
        render_at(&codegen, &declared_site(1), infer_ty, &LaSurface),
        "_"
    );
    assert_eq!(
        render_at(&codegen, &declared_site(2), union_ty, &LaSurface),
        "∪"
    );
}

#[test]
fn witness_mismatch_fails_closed() {
    let mut fx = fixture();
    let other = fx.types.array(fx.f32_ty);
    let codegen = codegen_with_forms(
        &fx,
        vec![(
            declared_site(1),
            semantic_form(
                TypeFormLevel::LONG,
                fx.lista_u32,
                vec![type_arg(short_width(fx.u32_ty))],
            ),
        )],
    );
    let err = codegen
        .type_to_faber_at(&declared_site(1), other, &FxHashMap::default(), &LaSurface)
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
            semantic_form(TypeFormLevel(3), fx.u32_ty, Vec::new()),
        ),
        // POLICY over a float witness: `trapping<f32>` is not a form.
        (
            declared_site(9),
            fx.f32_ty,
            semantic_form(TypeFormLevel::POLICY, fx.f32_ty, Vec::new()),
        ),
        // POLICY on a constructor with no policy word.
        (
            declared_site(10),
            numerus,
            semantic_form(TypeFormLevel::POLICY, numerus, Vec::new()),
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
                label: None,
            },
        ),
    ];

    for (site, type_id, form) in rejects {
        let codegen = codegen_with_forms(&fx, vec![(site.clone(), form)]);
        assert!(
            codegen
                .type_to_faber_at(&site, type_id, &FxHashMap::default(), &LaSurface)
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
        await_store: None,
        size_decl: false,
        size_static: false,
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
            label: None,
        },
    });

    let output = FaberCodegen::new(&fx.types, &fx.interner)
        .generate(&hir, &presentation, &LaSurface)
        .expect("authored hole must emit");
    assert!(
        output.code.contains("sit x ←"),
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
    let mut codegen = FaberCodegen::new(&fx.types, &fx.interner);
    codegen
        .generate(&hir, &presentation, &LaSurface)
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
    let mut fresh = FaberCodegen::new(&fx.types, &fx.interner);
    fresh
        .generate(&hir, &empty, &LaSurface)
        .expect("empty program must emit");
    assert!(fresh.authored_type_forms.is_empty());
}
